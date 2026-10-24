// Translated stages on disk (<data dir>/vulkan_stages/<translation hash>/), so a later session skips the translation of
// every stage it has seen. A file per lookup key (the stage's leading bytes and control section, as the in-memory
// stage key hashes them, and the translation inputs) holds its candidates: the guest code length each one was
// translated from, a hash of that code, the SPIR-V and its manifest. A candidate is used only when the guest code
// hashes the same. A change to the translation (its source, hashed by build.rs) starts a new directory; older ones
// are removed.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

use ash::vk;

const MAGIC: u32 = 0x3153_584E; // "NXS1"

fn dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let root = crate::profile::data_subdir("vulkan_stages")?;
        let build = env!("NX_TRANSLATION_HASH");
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy() != build {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        let d = root.join(build);
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    })
    .as_ref()
}

/// A hash of the guest code a stage was translated from: its first `len` bytes and the constant bank the
/// translator reads from the code (jump tables), as the control section places it.
pub(super) unsafe fn code_hash(data: u64, control: u64, len: usize) -> u64 {
    let mut h = super::texture::hash_bytes(std::slice::from_raw_parts(data as *const u8, len));
    if control >= 0x1_0000_0000 && *(control as *const u32) == 0x9876_1234 && *((control + 0x20) as *const u32) >= 0x708 {
        let off = *((control + 0x700) as *const u32) as u64;
        let size = (*((control + 0x6fc) as *const u32) as usize).min(0x10000);
        if off > 0 && size > 0 {
            h ^= super::texture::hash_bytes(std::slice::from_raw_parts((data + off) as *const u8, size)).rotate_left(1);
        }
    }
    h
}

/// The cached SPIR-V and manifest for lookup key `key` whose guest code at `data` (with `control`) is unchanged.
/// Also returns the code's hash.
pub(super) unsafe fn load(key: u64, data: u64, control: u64, readable: usize) -> Option<(Vec<u8>, sass2spv::manifest::ShaderManifest, u64)> {
    let mut found = None;
    candidates(key, |len, hash| {
        let hit = len <= readable && code_hash(data, control, len) == hash;
        if hit {
            found = Some(hash);
        }
        hit
    })
    .map(|(s, m)| (s, m, found.unwrap()))
}

/// The cached SPIR-V and manifest of lookup key `key` translated from code hashing to `code`.
pub(super) fn load_id(key: u64, code: u64) -> Option<(Vec<u8>, sass2spv::manifest::ShaderManifest)> {
    candidates(key, |_, hash| hash == code)
}

/// The first of `key`'s entries `pick` (given its code length and hash) accepts.
fn candidates(key: u64, mut pick: impl FnMut(usize, u64) -> bool) -> Option<(Vec<u8>, sass2spv::manifest::ShaderManifest)> {
    let bytes = std::fs::read(dir()?.join(format!("{key:016x}.bin"))).ok()?;
    let mut at = 0;
    let u32_at = |b: &[u8], o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
    while at + 24 <= bytes.len() {
        if u32_at(&bytes, at)? != MAGIC {
            return None;
        }
        let len = u32_at(&bytes, at + 4)? as usize;
        let hash = u64::from_le_bytes(bytes.get(at + 8..at + 16)?.try_into().unwrap());
        let (spv_len, man_len) = (u32_at(&bytes, at + 16)? as usize, u32_at(&bytes, at + 20)? as usize);
        let spv = bytes.get(at + 24..at + 24 + spv_len)?;
        let man = bytes.get(at + 24 + spv_len..at + 24 + spv_len + man_len)?;
        at += 24 + spv_len + man_len;
        if pick(len, hash) {
            let m = serde_json::from_slice(man).ok()?;
            return Some((spv.to_vec(), m));
        }
    }
    None
}

/// Adds a translation to the cache.
pub(super) fn store(key: u64, len: usize, hash: u64, spirv: &[u8], manifest: &sass2spv::manifest::ShaderManifest) {
    let Some(d) = dir() else { return };
    let Ok(man) = serde_json::to_vec(manifest) else { return };
    let mut rec = Vec::with_capacity(24 + spirv.len() + man.len());
    for v in [MAGIC, len as u32] {
        rec.extend_from_slice(&v.to_le_bytes());
    }
    rec.extend_from_slice(&hash.to_le_bytes());
    for v in [spirv.len() as u32, man.len() as u32] {
        rec.extend_from_slice(&v.to_le_bytes());
    }
    rec.extend_from_slice(spirv);
    rec.extend_from_slice(&man);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(d.join(format!("{key:016x}.bin"))) {
        let _ = f.write_all(&rec);
    }
}

/// The pipeline cache saved with the stages (the driver's compiled pipelines; MoltenVK keeps its converted shaders).
fn pipeline_cache_path() -> Option<PathBuf> {
    Some(dir()?.join("pipelines.vkcache"))
}

/// A pipeline cache holding what the last session saved.
pub(super) unsafe fn open_pipeline_cache(dev: &super::device::Dev) -> vk::PipelineCache {
    let data = pipeline_cache_path().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
    let info = vk::PipelineCacheCreateInfo::default().initial_data(&data);
    match dev.device.create_pipeline_cache(&info, None) {
        Ok(c) => c,
        // Data from another driver version: start empty.
        Err(_) => dev.device.create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None).unwrap_or(vk::PipelineCache::null()),
    }
}

/// Writes the pipeline cache (replacing the saved one whole).
pub(super) unsafe fn save_pipeline_cache(dev: &super::device::Dev, cache: vk::PipelineCache) {
    let (Some(path), Ok(data)) = (pipeline_cache_path(), dev.device.get_pipeline_cache_data(cache)) else { return };
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &data).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}
