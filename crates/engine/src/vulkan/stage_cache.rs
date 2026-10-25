// Shader stages on disk, per game, under <data dir>/vulkan_stages/<translator source hash>/:
// - stages.txt: every stage seen, by its shader cache name (`s_<full>`: FNV-1a 64 over the stage's code, as the Metal
//   shader cache names it) with what a draw needs to recognise it (the hash of its first 0x130 bytes, its length,
//   the inputs derived from its control section) and its vertex inputs;
// - code/<full>.bin: its code (for pipeline lists, which name stages that are not in memory yet);
// - t/<key>.bin: translations, by stage and translation inputs (Stage key);
// - pipelines.vkcache: the Vulkan pipeline cache.
// A change to the translator (its source hash) starts a new directory; older ones are removed.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use ash::vk;

use super::shader::Prep;

/// Bytes of a stage's start that select its candidates (the Metal shader cache's weak hash).
pub const WEAK_LEN: usize = 0x130;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    h
}

static CUSTOM_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_cache_dir(dir: PathBuf) {
    let _ = CUSTOM_DIR.set(dir);
}

pub fn dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let root = CUSTOM_DIR.get().cloned().or_else(|| {
            crate::settings::try_get()
                .and_then(|s| s.files.shader_cache.clone())
                .or_else(|| crate::profile::data_subdir("vulkan_stages"))
        })?;
        let build = sass2spv::SOURCE_HASH;
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy() != build {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        let d = root.join(build);
        std::fs::create_dir_all(d.join("t")).ok()?;
        std::fs::create_dir_all(d.join("code")).ok()?;
        Some(d)
    })
    .as_ref()
}

/// A stage seen in this or an earlier session.
#[derive(Clone, Debug)]
pub struct Known {
    pub full: u64,
    pub weak: u64,
    /// Bytes of code the name covers.
    pub size: usize,
    pub prep: Prep,
    /// Its input locations (a vertex stage's attributes).
    pub inputs: Vec<u32>,
}

impl Known {
    fn line(&self) -> String {
        let p = &self.prep;
        let cf = p.compute_fields.map_or("-".into(), |c| c.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));
        let inputs = if self.inputs.is_empty() { ".".into() } else { self.inputs.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",") };
        format!("s {:016x} {} {:016x} {} {} {} {} {cf} {inputs}", self.weak, self.size, self.full, p.hint, p.cb1, p.code_len, p.compute as u8)
    }

    fn parse(l: &str) -> Option<Known> {
        let f: Vec<&str> = l.split_whitespace().collect();
        let ["s", weak, size, full, hint, cb1, code_len, compute, cf, inputs] = f.as_slice() else { return None };
        let compute_fields = if *cf == "-" {
            None
        } else {
            let v: Vec<i32> = cf.split(',').filter_map(|v| v.parse().ok()).collect();
            Some(v.try_into().ok()?)
        };
        Some(Known {
            weak: u64::from_str_radix(weak, 16).ok()?,
            size: size.parse().ok()?,
            full: u64::from_str_radix(full, 16).ok()?,
            prep: Prep { hint: hint.parse().ok()?, cb1: cb1.parse().ok()?, code_len: code_len.parse().ok()?, compute: *compute == "1", compute_fields },
            inputs: if *inputs == "." { Vec::new() } else { inputs.split(',').filter_map(|v| v.parse().ok()).collect() },
        })
    }
}

#[derive(Default)]
struct Index {
    by_weak: HashMap<u64, Vec<Arc<Known>>>,
    by_full: HashMap<u64, Arc<Known>>,
    file: Option<std::fs::File>,
}

fn index() -> &'static Mutex<Index> {
    static I: OnceLock<Mutex<Index>> = OnceLock::new();
    I.get_or_init(|| {
        let mut i = Index::default();
        if let Some(text) = dir().and_then(|d| std::fs::read_to_string(d.join("stages.txt")).ok()) {
            for k in text.lines().filter_map(Known::parse) {
                let k = Arc::new(k);
                i.by_weak.entry(k.weak).or_default().push(k.clone());
                i.by_full.insert(k.full, k);
            }
        }
        Mutex::new(i)
    })
}

/// The known stage whose code `code` starts with, translated with `prep`.
pub fn identify(code: &[u8], prep: &Prep) -> Option<Arc<Known>> {
    let weak = fnv1a64(code.get(..WEAK_LEN)?);
    let i = index().lock().unwrap();
    i.by_weak.get(&weak)?.iter().find(|k| k.prep == *prep && k.size <= code.len() && fnv1a64(&code[..k.size]) == k.full).cloned()
}

/// The known stage named `full`.
pub fn by_name(full: u64) -> Option<Arc<Known>> {
    index().lock().unwrap().by_full.get(&full).cloned()
}

/// Adds a stage (its first `size` bytes name it) and keeps its code.
pub fn remember(code: &[u8], size: usize, prep: Prep, inputs: Vec<u32>) -> Arc<Known> {
    let size = size.min(code.len());
    let full = fnv1a64(&code[..size]);
    let k = Arc::new(Known { full, weak: fnv1a64(&code[..WEAK_LEN.min(code.len())]), size, prep, inputs });
    let mut i = index().lock().unwrap();
    if let Some(old) = i.by_full.get(&full).filter(|o| o.prep == k.prep) {
        return old.clone();
    }
    i.by_weak.entry(k.weak).or_default().push(k.clone());
    i.by_full.insert(full, k.clone());
    if let Some(d) = dir() {
        let keep = code.len().min(k.prep.code_len.max(size));
        let _ = std::fs::write(d.join("code").join(format!("{full:016x}.bin")), &code[..keep]);
        if i.file.is_none() {
            i.file = std::fs::OpenOptions::new().create(true).append(true).open(d.join("stages.txt")).ok();
        }
        let line = k.line();
        if let Some(f) = i.file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }
    k
}

/// The code kept for a known stage.
pub fn code(full: u64) -> Option<Vec<u8>> {
    std::fs::read(dir()?.join("code").join(format!("{full:016x}.bin"))).ok()
}

/// A translation: SPIR-V and its manifest.
pub fn load(key: u64) -> Option<(Vec<u8>, sass2spv::manifest::ShaderManifest)> {
    let bytes = std::fs::read(dir()?.join("t").join(format!("{key:016x}.bin"))).ok()?;
    let n = u32::from_le_bytes(bytes.get(..4)?.try_into().unwrap()) as usize;
    let spv = bytes.get(4..4 + n)?.to_vec();
    let m = serde_json::from_slice(bytes.get(4 + n..)?).ok()?;
    Some((spv, m))
}

pub fn store(key: u64, spirv: &[u8], manifest: &sass2spv::manifest::ShaderManifest) {
    let (Some(d), Ok(man)) = (dir(), serde_json::to_vec(manifest)) else { return };
    let mut rec = Vec::with_capacity(4 + spirv.len() + man.len());
    rec.extend_from_slice(&(spirv.len() as u32).to_le_bytes());
    rec.extend_from_slice(spirv);
    rec.extend_from_slice(&man);
    let path = d.join("t").join(format!("{key:016x}.bin"));
    let tmp = path.with_extension(format!("tmp{:?}", std::thread::current().id()).replace(['(', ')'], ""));
    if std::fs::write(&tmp, &rec).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// The pipeline cache saved with the stages (the driver's compiled pipelines; MoltenVK keeps its converted shaders).
pub fn pipeline_cache_path() -> Option<PathBuf> {
    Some(dir()?.join("pipelines.vkcache"))
}

/// A pipeline cache holding what the last session saved.
pub unsafe fn open_pipeline_cache(dev: &super::device::Dev) -> vk::PipelineCache {
    let data = pipeline_cache_path().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
    let info = vk::PipelineCacheCreateInfo::default().initial_data(&data);
    match dev.device.create_pipeline_cache(&info, None) {
        Ok(c) => c,
        // Data from another driver version: start empty.
        Err(_) => dev.device.create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None).unwrap_or(vk::PipelineCache::null()),
    }
}

/// Writes the pipeline cache (replacing the saved one whole).
pub unsafe fn save_pipeline_cache(dev: &super::device::Dev, cache: vk::PipelineCache) {
    let (Some(path), Ok(data)) = (pipeline_cache_path(), dev.device.get_pipeline_cache_data(cache)) else { return };
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &data).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}
