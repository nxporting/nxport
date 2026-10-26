// crates/cli/src/extract_shaders.rs
// Pure Rust RomFS shader extractor.
// Extracts every shader stage from RomFS (port of the former tools/extract_romfs_shaders.py; identical output).

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::sarc;
use sass2spv::ShaderStage;

pub const WEAK_LEN: usize = 0x130;
pub const STAGE_NAMES: [&str; 6] = [
    ShaderStage::Vertex.snake_name(),
    ShaderStage::TessellationControl.snake_name(),
    ShaderStage::TessellationEvaluation.snake_name(),
    ShaderStage::Geometry.snake_name(),
    ShaderStage::Fragment.snake_name(),
    ShaderStage::Compute.snake_name(),
];

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn decompress_zstd(data: &[u8], dict: Option<&[u8]>) -> Result<Vec<u8>, std::io::Error> {
    if let Some(d) = dict {
        let mut decoder = zstd::Decoder::with_dictionary(data, d)?;
        let mut out = Vec::new();
        decoder.read_to_end(&mut out)?;
        Ok(out)
    } else {
        zstd::decode_all(data)
    }
}

pub fn decompress_yaz0(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    if data.len() < 16 || &data[0..4] != b"Yaz0" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid Yaz0 header",
        ));
    }
    let uncompressed_size = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
    let mut out = Vec::with_capacity(uncompressed_size);
    let mut src = 16;
    while src < data.len() && out.len() < uncompressed_size {
        let code = data[src];
        src += 1;
        for bit in (0..8).rev() {
            if out.len() >= uncompressed_size {
                break;
            }
            if (code & (1 << bit)) != 0 {
                if src >= data.len() {
                    break;
                }
                out.push(data[src]);
                src += 1;
            } else {
                if src + 1 >= data.len() {
                    break;
                }
                let b0 = data[src] as usize;
                let b1 = data[src + 1] as usize;
                src += 2;
                let dist = ((b0 & 0x0f) << 8) | b1;
                let count = match b0 >> 4 {
                    0 => {
                        if src >= data.len() {
                            break;
                        }
                        let b2 = data[src] as usize;
                        src += 1;
                        b2 + 0x12
                    }
                    n => n + 2,
                };
                if dist >= out.len() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Yaz0 distance {} exceeds output length {}", dist, out.len()),
                    ));
                }
                let mut copy_src = out.len() - (dist + 1);
                for _ in 0..count {
                    if out.len() >= uncompressed_size {
                        break;
                    }
                    out.push(out[copy_src]);
                    copy_src += 1;
                }
            }
        }
    }
    Ok(out)
}

pub fn read_romfs_file(path: &Path, dicts: &HashMap<String, Vec<u8>>) -> Result<Vec<u8>, std::io::Error> {
    let raw = std::fs::read(path)?;
    if raw.starts_with(b"Yaz0") {
        return decompress_yaz0(&raw);
    }
    if path.extension().and_then(|s| s.to_str()) == Some("cmp") {
        if raw.len() > 4 {
            return zstd::decode_all(&raw[4..]);
        }
        return Ok(raw);
    }

    if path.extension().and_then(|s| s.to_str()) != Some("zs") {
        return Ok(raw);
    }

    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let dict = if file_name.ends_with(".pack.zs") {
        dicts.get("pack").map(|v| v.as_slice())
    } else if file_name.ends_with(".bcett.byml.zs") {
        dicts.get("bcett").map(|v| v.as_slice())
    } else {
        dicts.get("zs").map(|v| v.as_slice())
    };

    decompress_zstd(&raw, dict)
}

pub fn bnsh_blobs(data: &[u8]) -> Vec<&[u8]> {
    let mut blobs = Vec::new();
    let mut i = 0;
    while i + 0x64 <= data.len() {
        if &data[i..i + 4] == b"BNSH" {
            let size = u32::from_le_bytes(data[i + 0x1C..i + 0x20].try_into().unwrap()) as usize;
            if size > 0x80 && size <= data.len() - i && &data[i + 0x60..i + 0x64] == b"grsc" {
                blobs.push(&data[i..i + size]);
                i += size;
                continue;
            }
        }
        i += 4;
    }
    blobs
}

pub fn shab_blobs(data: &[u8]) -> Vec<&[u8]> {
    let mut blobs = Vec::new();
    let mut i = 0;
    while i + 0x20 <= data.len() {
        if &data[i..i + 4] == b"BAHS" { // Little-endian 'SHAB'
            let version = u32::from_le_bytes(data[i + 4..i + 8].try_into().unwrap());
            if version < 0x100 {
                let size = u32::from_le_bytes(data[i + 8..i + 12].try_into().unwrap()) as usize;
                if size > 0x20 && size <= data.len() - i {
                    blobs.push(&data[i..i + size]);
                    i += size;
                    continue;
                }
            }
        }
        i += 4;
    }
    blobs
}

fn valid_stage(stage: ShaderStage, ctrl: &[u8], code: &[u8]) -> bool {
    if ctrl.len() < 4 || u32::from_le_bytes(ctrl[0..4].try_into().unwrap()) != 0x98761234 {
        return false;
    }
    if code.len() < 4 {
        return false;
    }
    let magic = u32::from_le_bytes(code[0..4].try_into().unwrap());
    if stage == ShaderStage::Compute {
        let workgroup = ctrl.len() >= 0x74c && u32::from_le_bytes(ctrl[0x748..0x74c].try_into().unwrap()) > 0;
        return magic == 0x12345679 || (magic != 0x12345678 && workgroup);
    }
    if code.len() < 0x34 {
        return false;
    }
    let sph_type = (u32::from_le_bytes(code[0x30..0x34].try_into().unwrap()) >> 10) & 0xF;
    magic == 0x12345678 && ShaderStage::from_sph_type(sph_type) == Some(stage)
}

pub fn bnsh_stages(bnsh: &[u8]) -> Vec<(usize, usize, Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    let n = bnsh.len();
    if n < 0x60 + 0x28 {
        return out;
    }

    let nvar = u32::from_le_bytes(bnsh[0x60 + 0x1C..0x60 + 0x20].try_into().unwrap()) as usize;
    let var_off = u64::from_le_bytes(bnsh[0x60 + 0x20..0x60 + 0x28].try_into().unwrap()) as usize;

    for v in 0..nvar {
        let vo = var_off + v * 0x40;
        if vo + 0x18 > n {
            break;
        }
        let prog = u64::from_le_bytes(bnsh[vo + 0x10..vo + 0x18].try_into().unwrap()) as usize;
        if prog == 0 || prog + 0x38 > n {
            continue;
        }

        for stage in ShaderStage::ALL_PIPELINE_STAGES {
            let st = stage.slot();
            let stage_rec_off = prog + 8 + st * 8;
            if stage_rec_off + 8 > n {
                continue;
            }
            let so = u64::from_le_bytes(bnsh[stage_rec_off..stage_rec_off + 8].try_into().unwrap()) as usize;
            if so == 0 || so + 0x20 > n {
                continue;
            }

            let ctrl_off = u64::from_le_bytes(bnsh[so + 8..so + 16].try_into().unwrap()) as usize;
            let code_off = u64::from_le_bytes(bnsh[so + 16..so + 24].try_into().unwrap()) as usize;
            let code_size = u32::from_le_bytes(bnsh[so + 24..so + 28].try_into().unwrap()) as usize;
            let ctrl_size = u32::from_le_bytes(bnsh[so + 28..so + 32].try_into().unwrap()) as usize;

            if ctrl_off + ctrl_size > n || code_off + code_size > n || code_size < 0x80 {
                continue;
            }

            let ctrl = &bnsh[ctrl_off..ctrl_off + ctrl_size];
            let code_slice = if code_size < WEAK_LEN {
                let max_end = (code_off + 0x10000).min(n);
                &bnsh[code_off..max_end]
            } else {
                &bnsh[code_off..code_off + code_size]
            };

            if valid_stage(stage, ctrl, code_slice) {
                out.push((v, st, ctrl.to_vec(), code_slice.to_vec()));
            }
        }
    }
    out
}

pub fn shab_stages(shab: &[u8]) -> Vec<(usize, usize, Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    if shab.len() < 0x20 {
        return out;
    }

    let pool = u32::from_le_bytes(shab[0x1C..0x20].try_into().unwrap()) as usize;
    let mut ctrls = HashSet::new();
    let mut c = 0x60;
    while c + 4 <= shab.len() {
        if u32::from_le_bytes(shab[c..c + 4].try_into().unwrap()) == 0x98761234 {
            ctrls.insert(c);
        }
        c += 4;
    }

    let mut seen = HashSet::new();
    let limit = pool.min(shab.len());
    let mut f = 0x20;
    while f + 0x10 <= limit {
        let rel_c = i32::from_le_bytes(shab[f + 0xC..f + 0x10].try_into().unwrap()) as isize;
        let c_pos = (f as isize + rel_c) as usize;
        if ctrls.contains(&c_pos) {
            let code_off = u32::from_le_bytes(shab[f..f + 4].try_into().unwrap()) as usize;
            let ctrl_size = u32::from_le_bytes(shab[f + 8..f + 12].try_into().unwrap()) as usize;
            let a = pool + code_off;

            if !seen.contains(&code_off) && a + WEAK_LEN <= shab.len() && c_pos + ctrl_size <= shab.len() {
                let max_code_end = (a + 0x10000).min(shab.len());
                let code = &shab[a..max_code_end];
                let magic = u32::from_le_bytes(code[0..4].try_into().unwrap());
                let stage = if magic == 0x12345679 {
                    Some(ShaderStage::Compute)
                } else if code.len() >= 0x34 {
                    let sph_type = (u32::from_le_bytes(code[0x30..0x34].try_into().unwrap()) >> 10) & 0xF;
                    ShaderStage::from_sph_type(sph_type)
                } else {
                    None
                };

                let ctrl = &shab[c_pos..c_pos + ctrl_size];
                if let Some(stg) = stage {
                    if valid_stage(stg, ctrl, code) {
                        seen.insert(code_off);
                        out.push((0, stg.slot(), ctrl.to_vec(), code.to_vec()));
                    }
                }
            }
        }
        f += 4;
    }

    out
}

pub fn raw_nvn_stages(data: &[u8]) -> Vec<(usize, usize, Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    if data.len() < 16 {
        return out;
    }
    let first = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    if first >= 8 && first % 8 == 0 && first <= data.len() {
        let num_stages = first / 8;
        let mut ok = true;
        for i in 0..num_stages {
            let ctrl_off = u32::from_le_bytes(data[i * 4..(i + 1) * 4].try_into().unwrap()) as usize;
            let code_off = u32::from_le_bytes(data[(num_stages + i) * 4..(num_stages + i + 1) * 4].try_into().unwrap()) as usize;
            if ctrl_off + 0x24 > data.len() || code_off + 0x34 > data.len() {
                ok = false;
                break;
            }
            if u32::from_le_bytes(data[ctrl_off..ctrl_off + 4].try_into().unwrap()) != 0x98761234 {
                ok = false;
                break;
            }
            let code_magic = u32::from_le_bytes(data[code_off..code_off + 4].try_into().unwrap());
            if code_magic != 0x12345678 && code_magic != 0x12345679 {
                ok = false;
                break;
            }
        }
        if ok {
            for i in 0..num_stages {
                let ctrl_off = u32::from_le_bytes(data[i * 4..(i + 1) * 4].try_into().unwrap()) as usize;
                let code_off = u32::from_le_bytes(data[(num_stages + i) * 4..(num_stages + i + 1) * 4].try_into().unwrap()) as usize;
                let ctrl_size = u32::from_le_bytes(data[ctrl_off + 0x20..ctrl_off + 0x24].try_into().unwrap()) as usize;
                let next_code_off = if i + 1 < num_stages {
                    u32::from_le_bytes(data[(num_stages + i + 1) * 4..(num_stages + i + 2) * 4].try_into().unwrap()) as usize
                } else {
                    data.len()
                };
                let ctrl_end = (ctrl_off + ctrl_size).min(data.len());
                let ctrl = &data[ctrl_off..ctrl_end];
                let code_end = next_code_off.min(data.len());
                let code = &data[code_off..code_end];
                let code_magic = u32::from_le_bytes(code[0..4].try_into().unwrap());
                let stage = if code_magic == 0x12345679 {
                    Some(ShaderStage::Compute)
                } else if code.len() >= 0x34 {
                    let sph_type = (u32::from_le_bytes(code[0x30..0x34].try_into().unwrap()) >> 10) & 0xF;
                    ShaderStage::from_sph_type(sph_type)
                } else {
                    None
                };
                if let Some(stg) = stage {
                    if valid_stage(stg, ctrl, code) {
                        out.push((0, stg.slot(), ctrl.to_vec(), code.to_vec()));
                    }
                }
            }
        }
    }
    out
}

/// The RomFS directories to scan: the game profile's (the dump is RomFS's parent), else every top-level directory.
pub fn default_dirs(romfs: &Path) -> Vec<String> {
    let profile = romfs.parent().and_then(|dump| nxport::profile::identify(dump).profile);
    match profile.filter(|p| !p.shader_dirs.is_empty()) {
        Some(p) => p.shader_dirs.iter().map(|d| d.to_string()).collect(),
        None => std::fs::read_dir(romfs)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
    }
}

struct ExtractState {
    seen: HashSet<Vec<u8>>,
    weak_count: HashMap<u64, usize>,
    per_stage: [usize; 6],
    total_stages: usize,
    err: Option<std::io::Error>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderExtractPhase {
    ExtractingStages,
}

impl ShaderExtractPhase {
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExtractingStages => "Extracting shader stages",
        }
    }
}

impl std::fmt::Display for ShaderExtractPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn has_existing_stages(dir: &Path) -> bool {
    if dir.join(".extracted").is_file() {
        return true;
    }
    // Backward compatibility for existing stages directories:
    // If the directory exists and has at least 50 .code files, consider it already extracted.
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut count = 0;
        for entry in entries.flatten() {
            if entry.path().extension().map_or(false, |ext| ext == "code") {
                count += 1;
                if count >= 50 {
                    let _ = std::fs::write(dir.join(".extracted"), format!("count: {count}\n"));
                    return true;
                }
            }
        }
    }
    false
}

pub fn extract_all(
    romfs: &Path,
    out_dir: &Path,
    dirs: &[&str],
    ctrl_out: Option<&Path>,
    on_progress: Option<&(dyn Fn(ShaderExtractPhase, usize, usize) + Sync)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let force = std::env::var("NXPORT_FORCE_EXTRACT").map_or(false, |v| v == "1" || v.eq_ignore_ascii_case("true"));
    if !force && has_existing_stages(out_dir) {
        println!("[+] Shader stages already extracted in {}. Skipping RomFS scan.", out_dir.display());
        if let Some(cb) = on_progress {
            cb(ShaderExtractPhase::ExtractingStages, 1, 1);
        }
        return Ok(());
    }

    println!("[+] Extracting shaders from RomFS: {}", romfs.display());
    println!("    Output directory: {}", out_dir.display());

    let mut dicts = HashMap::new();
    let zsdic_pack = romfs.join("Pack/ZsDic.pack.zs");
    if zsdic_pack.exists() {
        if let Ok(raw) = std::fs::read(&zsdic_pack) {
            if let Ok(dec) = zstd::decode_all(raw.as_slice()) {
                if let Ok(files) = sarc::read_sarc(&dec) {
                    for (name, data) in files {
                        let key = name.split(".zsdic").next().unwrap().split('.').next().unwrap().to_string();
                        dicts.insert(key, data);
                    }
                    println!("[+] Loaded {} zstd dictionaries from ZsDic.pack.zs", dicts.len());
                }
            }
        }
    }

    std::fs::create_dir_all(out_dir)?;
    if let Some(d) = ctrl_out {
        std::fs::create_dir_all(d)?;
    }

    let mut dir_file_lists = Vec::new();
    let mut total_files = 0;
    for &dir_name in dirs {
        let scan_dir = romfs.join(dir_name);
        if scan_dir.is_dir() {
            let mut files = Vec::new();
            collect_files(&scan_dir, &mut files);
            println!("[+] Scanning '{}' ({} files)...", dir_name, files.len());
            total_files += files.len();
            dir_file_lists.push((dir_name, files));
        }
    }
    if let Some(cb) = on_progress {
        cb(ShaderExtractPhase::ExtractingStages, 0, total_files);
    }

    let cpus = std::thread::available_parallelism().map_or(4, |n| n.get());
    let jobs = cpus.saturating_sub(2).max(2);

    let state = Mutex::new(ExtractState {
        seen: HashSet::new(),
        weak_count: HashMap::new(),
        per_stage: [0usize; 6],
        total_stages: 0,
        err: None,
    });
    let done_files = AtomicUsize::new(0);

    for (dir_name, files) in dir_file_lists {
        let dir_stages = AtomicUsize::new(0);
        let dir_new = AtomicUsize::new(0);
        let next_file = AtomicUsize::new(0);
        let total_dir_files = files.len();

        // Files are read and parsed in parallel, a batch at a time; their stages are named and written in file order,
        // so the `<weak>_<n>` numbering of stages sharing a weak hash does not depend on thread timing.
        let batch = jobs * 16;
        for start in (0..total_dir_files).step_by(batch) {
            let chunk = &files[start..(start + batch).min(total_dir_files)];
            let results: Vec<Mutex<Vec<(usize, usize, Vec<u8>, Vec<u8>)>>> = chunk.iter().map(|_| Mutex::new(Vec::new())).collect();
            next_file.store(0, Ordering::Relaxed);
            std::thread::scope(|s| {
                for _ in 0..jobs.min(chunk.len()) {
                    s.spawn(|| loop {
                        let idx = next_file.fetch_add(1, Ordering::Relaxed);
                        if idx >= chunk.len() {
                            break;
                        }
                        let done = done_files.fetch_add(1, Ordering::Relaxed) + 1;
                        if let Some(cb) = on_progress {
                            cb(ShaderExtractPhase::ExtractingStages, done, total_files);
                        }
                        let Ok(data) = read_romfs_file(&chunk[idx], &dicts) else { continue };
                        let is_raw_nvn = data.len() >= 16 && {
                            let first = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
                            first >= 8 && first + 4 <= data.len() && u32::from_le_bytes(data[first..first + 4].try_into().unwrap()) == 0x98761234
                        };
                        if !data.windows(4).any(|w| w == b"BNSH" || w == b"BAHS") && !is_raw_nvn {
                            continue;
                        }
                        let mut found = Vec::new();
                        for b in bnsh_blobs(&data) {
                            found.extend(bnsh_stages(b));
                        }
                        for s in shab_blobs(&data) {
                            found.extend(shab_stages(s));
                        }
                        found.extend(raw_nvn_stages(&data));
                        *results[idx].lock().unwrap() = found;
                    });
                }
            });

            let mut st = state.lock().unwrap();
            for found in results {
                let found = found.into_inner().unwrap();
                dir_stages.fetch_add(found.len(), Ordering::Relaxed);
                for (_v, slot, ctrl, code) in found {
                    if st.seen.contains(&code) {
                        continue;
                    }
                    let weak = fnv1a64(&code[..WEAK_LEN.min(code.len())]);
                    let count = st.weak_count.entry(weak).or_insert(0);
                    let stem = if *count > 0 { format!("{:016x}_{}_s{}", weak, *count, slot) } else { format!("{:016x}_s{}", weak, slot) };
                    *count += 1;

                    st.seen.insert(code.clone());
                    dir_new.fetch_add(1, Ordering::Relaxed);
                    st.per_stage[slot] += 1;
                    st.total_stages += 1;

                    let mut code_padded = code;
                    if code_padded.len() < 0x10000 {
                        code_padded.resize(0x10000, 0);
                    }
                    let mut ctrl_padded = ctrl.clone();
                    if ctrl_padded.len() < 0x400 {
                        ctrl_padded.resize(0x400, 0);
                    }
                    std::fs::write(out_dir.join(format!("{}.code", stem)), &code_padded)?;
                    std::fs::write(out_dir.join(format!("{}.ctrl", stem)), &ctrl_padded)?;
                    if let Some(d) = ctrl_out {
                        let full_ctrl = d.join(format!("{:016x}.ctrl", weak));
                        if !full_ctrl.exists() {
                            let _ = std::fs::write(full_ctrl, &ctrl);
                        }
                    }
                }
            }
        }

        if let Some(e) = state.lock().unwrap().err.take() {
            return Err(Box::new(e));
        }

        println!("    {:10} found {} stages, {} unique new", dir_name, dir_stages.load(Ordering::Relaxed), dir_new.load(Ordering::Relaxed));
    }

    let st = state.into_inner().unwrap();
    println!("\n=== RomFS Shader Extraction Summary ===");
    println!("  Total Unique Stages Extracted: {}", st.total_stages);
    for stage in ShaderStage::ALL_PIPELINE_STAGES {
        let count = st.per_stage[stage.slot()];
        if count > 0 {
            println!("    - {:15}: {:5}", stage.snake_name(), count);
        }
    }

    let marker_content = format!(
        "romfs: {}\ntotal_stages: {}\n",
        romfs.display(),
        st.total_stages
    );
    let _ = std::fs::write(out_dir.join(".extracted"), marker_content);

    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() {
                collect_files(&p, out);
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
}
