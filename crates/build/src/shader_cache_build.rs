// crates/cli/src/shader_cache_build.rs
// `nxport-cli shaders build`: the ahead-of-time shader cache for the Metal backend (port of the former
// tools/build_shader_cache.py).
//
// Input : directories of stage dumps (NAME_sN.code, optional NAME_sN.ctrl), written by the engine
//         (--dump-shaders=<dir>, missing-stage dumps) or extracted from RomFS (`nxport-cli shaders extract`). The index
//         covers exactly the stages in the given directories.
// Output: <out>/shaders_NNNN.metallib   libraries of LINK_CHUNK stages each, entry point `s_<hash>`
//         <out>/index.txt               lookup index + resource bindings, parsed by crates/engine/src/shader_cache.rs
//         <out>/failures.txt            stages not translated, with the phase and message
//         <out>/work/                   per-stage MSL and AIR, state.json (what the next build can reuse)
//
// Pipeline per stage: SASS -(sass2spv)-> MSL (or SPIR-V -(spirv-cross)-> MSL with --spirv-cross) -(msl_specialize)-> MSL
// with function-constant typed inputs/outputs -(xcrun metal)-> AIR, then the AIRs are linked into metallibs in chunks.
//
// Stage identity. The dump window is 64 KB, but the real stage is shorter and its tail is whatever followed it in
// memory, so the hash covers only [0, PREAMBLE + decoded size). The engine does not know the size, so the index is
// two-level: a weak key (hash of the first WEAK_LEN bytes) selects candidate sizes, then the engine hashes exactly
// `size` bytes and compares against the full hash. A miss just means "no translated shader".
//
// The cache is derived from the game's code: keep it out of version control.

use crate::extract_shaders::fnv1a64;
use crate::msl_specialize::{early_fragment_tests, specialize, uses_cbuf_limits};
use regex::Regex;
use sass2spv::ShaderStage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

const PREAMBLE: usize = 0x30; // NVN stage data: 0x12345678 preamble, then the 0x50-byte SPH, then code
const WEAK_LEN: usize = 0x130;
const MAX_RSS_MB: u64 = 3072; // a tool that grows past this is killed rather than allowed to push the machine into swap
const LINK_CHUNK: usize = 50; // AIR files per metallib; one giant link of ~2,800 shaders used >100 GB of memory
const BATCH: usize = 256; // stages per decompiler process
const COMPILE_BATCH: usize = 24; // MSL files per Metal compiler launch (startup, not compilation, dominated)

fn xcross_stage(stage: ShaderStage) -> &'static str {
    stage.xcross_arg()
}

/// One of the 256 work subdirectories: the first two hex digits of the name, else (names such as r3_<hex>_s0) a hash of it.
fn shard_prefix(key: &str) -> String {
    let clean = key.strip_prefix("s_").unwrap_or(key);
    match clean.get(..2) {
        Some(p) if p.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) => p.to_string(),
        _ => format!("{:02x}", fnv1a64(key.as_bytes()) as u8),
    }
}

fn shard_dir(base: &Path, key: &str) -> PathBuf {
    base.join(shard_prefix(key))
}

fn stage_metal_path(work: &Path, tag: &str) -> PathBuf {
    let sharded = shard_dir(work, tag).join(format!("{tag}.metal"));
    if sharded.exists() {
        sharded
    } else {
        let flat = work.join(format!("{tag}.metal"));
        if flat.exists() {
            flat
        } else {
            sharded
        }
    }
}

fn entry_air_path(work: &Path, entry: &str) -> PathBuf {
    let sharded = shard_dir(work, entry).join(format!("{entry}.air"));
    if sharded.exists() {
        sharded
    } else {
        let flat = work.join(format!("{entry}.air"));
        if flat.exists() {
            flat
        } else {
            sharded
        }
    }
}

fn collect_metal_files(src: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(src) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Ok(sub) = std::fs::read_dir(&p) {
                    for s in sub.flatten() {
                        if s.path().extension().is_some_and(|x| x == "metal") {
                            out.push(s.path());
                        }
                    }
                }
            } else if p.extension().is_some_and(|x| x == "metal") {
                out.push(p);
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderBuildPhase {
    Decompiling,
    TranslatingToMetal,
    CompilingMetal,
    LinkingLibraries,
}

impl ShaderBuildPhase {
    pub const ALL: [ShaderBuildPhase; 4] = [
        ShaderBuildPhase::Decompiling,
        ShaderBuildPhase::TranslatingToMetal,
        ShaderBuildPhase::CompilingMetal,
        ShaderBuildPhase::LinkingLibraries,
    ];

    pub const NUM_PHASES: usize = 4;

    #[inline]
    pub fn phase_index(self) -> usize {
        match self {
            Self::Decompiling => 0,
            Self::TranslatingToMetal => 1,
            Self::CompilingMetal => 2,
            Self::LinkingLibraries => 3,
        }
    }

    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Decompiling => "Decompiling shaders",
            Self::TranslatingToMetal => "Translating to Metal",
            Self::CompilingMetal => "Compiling Metal shaders",
            Self::LinkingLibraries => "Linking shader libraries",
        }
    }
}

impl std::fmt::Display for ShaderBuildPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IosShaderBuildPhase {
    CompilingShaders,
    LinkingLibraries,
}

impl IosShaderBuildPhase {
    pub const ALL: [IosShaderBuildPhase; 2] = [
        IosShaderBuildPhase::CompilingShaders,
        IosShaderBuildPhase::LinkingLibraries,
    ];

    pub const NUM_PHASES: usize = 2;

    #[inline]
    pub fn phase_index(self) -> usize {
        match self {
            Self::CompilingShaders => 0,
            Self::LinkingLibraries => 1,
        }
    }

    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CompilingShaders => "Compiling iOS shaders",
            Self::LinkingLibraries => "Linking iOS shader libraries",
        }
    }
}

impl std::fmt::Display for IosShaderBuildPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub struct Options {
    pub dump_dirs: Vec<PathBuf>,
    pub out_dir: PathBuf,
    /// Shared control sections by weak hash (engine: --dump-ctrl); stages with their own <stem>.ctrl do not need it.
    pub ctrl_dir: Option<PathBuf>,
    pub jobs: usize,
    /// Only write each stage's specialized MSL here (named by entry point) and stop; no Metal compile.
    pub msl_out: Option<PathBuf>,
    /// Only rewrite <out>/index.txt from the last build's state.
    pub index_only: bool,
    /// Progress reporting callback: (phase, done_items, total_items)
    pub on_progress: Option<Arc<dyn Fn(ShaderBuildPhase, usize, usize) + Send + Sync>>,
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs `cmd` with captured output, killing it if its resident set exceeds `max_rss_mb` or timeout.
fn run(cmd: Command, max_rss_mb: u64) -> Result<Output, String> {
    run_timeout(cmd, max_rss_mb, 60)
}

struct ChildGuard(u32, std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.1.load(std::sync::atomic::Ordering::Relaxed) {
            unsafe {
                libc::kill(self.0 as i32, libc::SIGKILL);
            }
        }
    }
}

fn run_timeout(mut cmd: Command, max_rss_mb: u64, timeout_secs: u64) -> Result<Output, String> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{name}: {e}"))?;
    let pid = child.id();
    let exited = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _guard = ChildGuard(pid, exited.clone());
    let done = std::sync::Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let killed = std::sync::Arc::new(Mutex::new(None::<u64>));
    let timed_out = std::sync::Arc::new(Mutex::new(false));
    let watcher = {
        let (done, killed, timed_out) = (done.clone(), killed.clone(), timed_out.clone());
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let (lock, cv) = &*done;
            let mut finished = lock.lock().unwrap();
            loop {
                // Poll every half second, but wake at once when the child exits.
                let (g, _) = cv.wait_timeout(finished, std::time::Duration::from_millis(500)).unwrap();
                finished = g;
                if *finished {
                    return;
                }
                if start.elapsed().as_secs() >= timeout_secs {
                    *timed_out.lock().unwrap() = true;
                    unsafe { libc::kill(pid as i32, libc::SIGKILL); }
                    return;
                }
                let rss = Command::new("ps").args(["-o", "rss=", "-p", &pid.to_string()]).output().ok();
                let kb = rss.and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok()).unwrap_or(0);
                if kb / 1024 > max_rss_mb {
                    *killed.lock().unwrap() = Some(kb / 1024);
                    unsafe { libc::kill(pid as i32, libc::SIGKILL); }
                    return;
                }
            }
        })
    };
    let out = child.wait_with_output().map_err(|e| format!("{name}: {e}"));
    exited.store(true, std::sync::atomic::Ordering::Relaxed);
    *done.0.lock().unwrap() = true;
    done.1.notify_all();
    let _ = watcher.join();
    if *timed_out.lock().unwrap() {
        return Err(format!("{name} timed out after {timeout_secs}s"));
    }
    if let Some(mb) = *killed.lock().unwrap() {
        return Err(format!("{name} exceeded {max_rss_mb} MB (reached {mb} MB)"));
    }
    let out = out?;
    Ok(Output {
        status: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

/// The stage's full GLSLC control section, or None. The per-stage <stem>.ctrl beside the code comes first (full sections
/// from the extractor and the engine's miss dumps; older dumps kept only 0x400 bytes), then <ctrl_dir>/<weak>.ctrl. The
/// latter holds the section of whichever stage with that weak hash was seen first, so it is not used for the
/// extractor's _<n> variants (same first WEAK_LEN bytes, different code): a variant read with another one's cbuf 1
/// offset gets bogus jump tables, and the decompiler crashed (IndexOutOfRange) on them.
fn stage_ctrl(code: &Path, head: &[u8], ctrl_dir: Option<&Path>) -> Option<Vec<u8>> {
    let valid = |d: &[u8]| d.len() >= 0x768 && u32_at(d, 0) == 0x9876_1234;
    if let Ok(own) = std::fs::read(code.with_extension("ctrl")) {
        if valid(&own) {
            return Some(own);
        }
    }
    let stem = code.file_stem()?.to_string_lossy().into_owned();
    let parts: Vec<&str> = stem.split('_').collect();
    if parts.len() == 3 && !parts[1].is_empty() && parts[1].bytes().all(|b| b.is_ascii_digit()) {
        return None; // <weak>_<n>_s<stage>: a variant
    }
    let shared = std::fs::read(ctrl_dir?.join(format!("{:016x}.ctrl", fnv1a64(head)))).ok()?;
    valid(&shared).then_some(shared)
}

fn head_of(code: &Path) -> Vec<u8> {
    let d = std::fs::read(code).unwrap_or_default();
    d[..WEAK_LEN.min(d.len())].to_vec()
}

/// GLSLC control section +0x718 bit 0: the stage requested early fragment tests (NVN sets EarlyZForce).
fn early_tests_flag(code: &Path, ctrl_dir: Option<&Path>) -> bool {
    stage_ctrl(code, &head_of(code), ctrl_dir).is_some_and(|d| d[0x718] & 1 != 0)
}

/// Tab-separated extra decompiler inputs from the stage's GLSLC control section: the constant bank (cbuf 1) offset at
/// +0x700, or -1 to let sass2spv find it heuristically (the decompiler reads indirect-branch jump tables from there; wrong
/// tables build bogus control flow), then for compute stages the workgroup size (+0x748/+0x74c/+0x750) and shared /
/// local memory sizes. Which of +0x754 / +0x760 / +0x710 is shared and which local memory is not pinned down yet, so
/// both get the largest (over-allocating is harmless).
fn ctrl_fields(code: &Path, ctrl_dir: Option<&Path>) -> String {
    let head = head_of(code);
    let Some(d) = stage_ctrl(code, &head, ctrl_dir) else { return "-1".into() };
    let mut fields = vec![u32_at(&d, 0x700)];
    let magic = if head.len() >= 4 { u32_at(&head, 0) } else { 0 };
    // Compute stages: TotK's carry a 0x12345679 preamble; older GLSLC output (Kirby) has none, the code starts at once.
    // Graphics stages always start with the 0x12345678 preamble, and only compute control sections give a workgroup size.
    if magic == 0x1234_5679 || (magic != 0x1234_5678 && u32_at(&d, 0x748) > 0) {
        let mem = (u32_at(&d, 0x754).max(u32_at(&d, 0x760)).max(u32_at(&d, 0x710)) + 15) & !15;
        fields.extend([u32_at(&d, 0x748), u32_at(&d, 0x74C), u32_at(&d, 0x750), mem, mem]);
    }
    fields.iter().map(|f| f.to_string()).collect::<Vec<_>>().join("\t")
}

enum Translated {
    Ok { stage: ShaderStage, size: usize },
    Failed(String, String),
}

/// Translates `paths` to MSL (<stem>.msl) or, with --spirv-cross, SPIR-V (<stem>.spv), plus <stem>.spv.json, into
/// `spv_dir`; {stem: result}.
fn translate_batch(paths: &[PathBuf], spv_dir: &Path, o: &Options) -> HashMap<String, Translated> {
    let msl = !crate::translation::get().spirv_cross;
    let target = if msl { sass2spv::Target::Msl(sass2spv::MSL_ENTRY_PLACEHOLDER.into()) } else { sass2spv::Target::Spirv };
    let mut out = HashMap::new();
    for p in paths {
        let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
        let fields: Vec<i32> = ctrl_fields(p, o.ctrl_dir.as_deref()).split('\t').map(|f| f.parse().unwrap_or(-1)).collect();
        let compute = (fields.len() > 1).then(|| [fields[1], fields[2], fields[3], fields[4]]);
        let res = std::fs::read(p).map_err(|e| e.to_string()).and_then(|code| {
            std::panic::catch_unwind(|| sass2spv::translate_stage_to(&code, if compute.is_some() { 3 } else { 0 }, fields[0], compute, &target))
                .unwrap_or_else(|_| Err("translator panicked".into()))
        });
        let res = match res {
            Ok(prog) if !prog.binary_code.is_empty() || !prog.msl.is_empty() => {
                let (file, bytes) = if msl { (format!("{stem}.msl"), prog.msl.as_bytes()) } else { (format!("{stem}.spv"), &prog.binary_code[..]) };
                match std::fs::write(spv_dir.join(file), bytes)
                    .and_then(|_| std::fs::write(spv_dir.join(format!("{stem}.spv.json")), sass2spv::manifest_json(&prog)))
                {
                    // Size is counted from the code start: 0x30 (graphics, after the preamble) or 0x100 (compute).
                    Ok(()) => {
                        let extra = if prog.info.stage == ShaderStage::Compute { 0x100 } else { 0x30 } - PREAMBLE;
                        Translated::Ok { stage: prog.info.stage, size: prog.size as usize + extra }
                    }
                    Err(e) => Translated::Failed("write".into(), e.to_string()),
                }
            }
            Ok(_) => Translated::Failed("sass2spv".into(), "empty binary".into()),
            Err(e) => Translated::Failed("sass2spv".into(), e.lines().next().unwrap_or("").chars().take(120).collect()),
        };
        out.insert(stem, res);
    }
    out
}

fn metal_bin() -> &'static str {
    static B: OnceLock<String> = OnceLock::new();
    B.get_or_init(|| {
        let mut c = Command::new("xcrun");
        c.args(["-sdk", "macosx", "-f", "metal"]);
        run(c, MAX_RSS_MB).map(|o| o.stdout.trim().to_string()).unwrap_or_else(|_| "metal".into())
    })
}

fn entry_line_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^(?:fragment|vertex|kernel) .*$").unwrap())
}

/// {Metal buffer index: bytes} for constant buffers a stage reads only at literal vec4 indices: it never reads past the
/// returned length, so the engine copies only that much. Buffers indexed dynamically or passed whole to helper functions
/// are left out (the engine then copies the full bound range).
pub fn cbuf_extents(msl: &str) -> BTreeMap<u32, usize> {
    static PARAM: OnceLock<Regex> = OnceLock::new();
    let param = PARAM.get_or_init(|| Regex::new(r"constant _\w+& (\w+) \[\[buffer\((\d+)\)\]\]").unwrap());
    let mut out = BTreeMap::new();
    let Some(m) = entry_line_re().find(msl) else { return out };
    // Only the stage's own code: the tiny-pass kernels appended after it (tiny_kernels::COMMON onwards) pass every
    // buffer whole to a copy of the body, which read as a whole-buffer use and dropped every tiny-capable stage's extents.
    let body = &msl[m.end()..];
    let body = [crate::tiny_kernels::MARKER, crate::batch_kernels::MARKER].iter().filter_map(|m| body.find(m)).min().map_or(body, |e| &body[..e]);
    for c in param.captures_iter(m.as_str()) {
        let use_re = Regex::new(&format!(r"\b{}\b", regex::escape(&c[1]))).unwrap();
        let mut top = Some(0usize);
        for u in use_re.find_iter(body) {
            // A literal read `NAME._m0[<digits>]` not followed by a word character or '[' (the original's lookahead).
            let rest = &body[u.end()..];
            let lit = rest.strip_prefix("._m0[").and_then(|r| {
                let n = r.bytes().take_while(|b| b.is_ascii_digit()).count();
                let after = r[n..].strip_prefix(']')?;
                let next_ok = after.bytes().next().is_none_or(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'['));
                (n > 0 && next_ok).then(|| r[..n].parse::<usize>().ok()).flatten()
            });
            match lit {
                Some(i) => top = top.map(|t| t.max(i + 1)),
                None => {
                    top = None; // dynamic index or a whole-buffer use
                    break;
                }
            }
        }
        if let Some(t) = top {
            out.insert(c[2].parse().unwrap(), t * 16);
        }
    }
    out
}

/// Metal buffer indices of storage buffers the stage never writes (spirv-cross declares them `const device`): the
/// engine's early-fence guard may write-protect their memory while GPU work reads it (index line `sr`).
pub fn readonly_sbuffers(msl: &str) -> Vec<u32> {
    static P: OnceLock<Regex> = OnceLock::new();
    let p = P.get_or_init(|| Regex::new(r"const device _\w+& \w+ \[\[buffer\((\d+)\)\]\]").unwrap());
    let Some(m) = entry_line_re().find(msl) else { return Vec::new() };
    let mut v: Vec<u32> = p.captures_iter(m.as_str()).map(|c| c[1].parse().unwrap()).collect();
    v.sort_unstable();
    v
}

/// The translator's MSL (or spirv-cross's, with --spirv-cross) + MSL specialization for a stage the decompiler translated: (record, tag) or (phase, message).
fn prepare_stage(code: &Path, work: &Path, spv_dir: &Path, stage: ShaderStage, size: usize, o: &Options) -> Result<(StageRecord, String), (String, String)> {
    let data = std::fs::read(code).map_err(|e| ("read".to_string(), e.to_string()))?;
    let tag = code.file_stem().unwrap().to_string_lossy().into_owned();
    let spv = spv_dir.join(format!("{tag}.spv"));
    let manifest_path = spv_dir.join(format!("{tag}.spv.json"));
    let total = PREAMBLE + size;
    let full = fnv1a64(&data[..total.min(data.len())]);
    let weak = fnv1a64(&data[..WEAK_LEN.min(data.len())]);
    let entry = format!("s_{full:016x}");
    let msl_path = shard_dir(work, &tag).join(format!("{tag}.metal"));
    let raw = if !crate::translation::get().spirv_cross {
        let src = spv_dir.join(format!("{tag}.msl"));
        let raw = std::fs::read_to_string(&src).map_err(|e| ("msl".to_string(), e.to_string()))?;
        let _ = std::fs::remove_file(&src);
        raw.replace(sass2spv::MSL_ENTRY_PLACEHOLDER, &entry)
    } else {
        let mut cmd = Command::new("spirv-cross");
        cmd.args(["--msl", "--msl-version", "30000", "--msl-decoration-binding", "--rename-entry-point", "main", &entry])
            .arg(xcross_stage(stage))
            .arg(&spv)
            .arg("--output")
            .arg(&msl_path);
        let r = run_timeout(cmd, MAX_RSS_MB, 15).map_err(|e| {
            let _ = std::fs::remove_file(&spv);
            let _ = std::fs::remove_file(&manifest_path);
            ("spirv-cross".to_string(), e)
        })?;
        if r.status != Some(0) {
            let _ = std::fs::remove_file(&spv);
            let _ = std::fs::remove_file(&manifest_path);
            let msg = if r.stderr.is_empty() { r.stdout } else { r.stderr };
            return Err(("spirv-cross".into(), msg.chars().take(120).collect::<String>().trim().to_string()));
        }
        let raw = std::fs::read_to_string(&msl_path).map_err(|e| {
            let _ = std::fs::remove_file(&spv);
            let _ = std::fs::remove_file(&manifest_path);
            ("spirv-cross".to_string(), e.to_string())
        })?;
        raw
    };
    let (mut text, attrs, colors, varyings) = specialize(&raw);
    // --early-tests: honour the early-fragment-tests flag. Off by default: with it TotK's tile classification
    // (fragment shaders marking tiles in a storage buffer) loses marks and most of the screen goes unlit
    // (cause not found). -13% GPU time in gameplay.
    if stage == ShaderStage::Fragment && crate::translation::get().early_tests && early_tests_flag(code, o.ctrl_dir.as_deref()) {
        text = early_fragment_tests(&text); // last: earlier passes insert declarations before the entry point
    }
    // Compute versions of the stage for the engine's tiny-pass path: appended, the stage itself is
    // unchanged. --no-tiny-kernels: none.
    let tiny = if crate::translation::get().no_tiny_kernels {
        None
    } else {
        match stage {
            ShaderStage::Fragment => crate::tiny_kernels::fragment_kernel(&text),
            ShaderStage::Vertex => crate::tiny_kernels::vertex_kernel(&text),
            _ => None,
        }
    };
    // Batched variants (crate::batch_kernels), from the stage's own code; appended after the tiny kernels.
    // --no-batch-kernels: none.
    let batch = if crate::translation::get().no_batch_kernels {
        None
    } else {
        match stage {
            ShaderStage::Vertex => crate::batch_kernels::batch_variant(&text, "vertex"),
            ShaderStage::Fragment => crate::batch_kernels::batch_variant(&text, "fragment"),
            _ => None,
        }
    };
    if let Some(k) = &tiny {
        text.push('\n');
        text.push_str(k);
    }
    if let Some((k, _)) = &batch {
        text.push_str(k);
    }
    std::fs::write(&msl_path, &text).map_err(|e| {
        let _ = std::fs::remove_file(&spv);
        let _ = std::fs::remove_file(&manifest_path);
        ("write".to_string(), e.to_string())
    })?;
    let manifest: sass2spv::ShaderManifest = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or_else(|| {
            let _ = std::fs::remove_file(&spv);
            let _ = std::fs::remove_file(&manifest_path);
            ("sass2spv".to_string(), "no binding manifest".to_string())
        })?;
    // Flush the intermediate .spv and .spv.json as soon as the stage is translated!
    let _ = std::fs::remove_file(&spv);
    let _ = std::fs::remove_file(&manifest_path);
    let local = (stage == ShaderStage::Compute).then(|| {
        let f: Vec<u32> = ctrl_fields(code, o.ctrl_dir.as_deref()).split('\t').filter_map(|s| s.parse().ok()).collect();
        if f.len() >= 4 { f[1..4].to_vec() } else { vec![1, 1, 1] }
    });
    let cbext = cbuf_extents(&text);
    let rec = StageRecord {
        weak,
        weaks: Vec::new(),
        size: total,
        full,
        stage,
        entry,
        manifest,
        attrs,
        varyings,
        colors,
        local,
        cbext,
        sbro: readonly_sbuffers(&text),
        cl: uses_cbuf_limits(&text),
        tk: tiny.is_some(),
        tkl: tiny.as_deref().and_then(crate::tiny_kernels::implicit_tag),
        tkc: tiny.as_deref().is_some_and(crate::tiny_kernels::applies_clears),
        tb: batch.as_ref().map(|(_, b)| b.clone()),
    };
    Ok((rec, tag))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageRecord {
    pub weak: u64,
    #[serde(default)]
    pub weaks: Vec<u64>,
    pub size: usize,
    pub full: u64,
    pub stage: ShaderStage,
    pub entry: String,
    pub manifest: sass2spv::ShaderManifest,
    #[serde(default)]
    pub attrs: Vec<u32>,
    #[serde(default)]
    pub varyings: Vec<u32>,
    #[serde(default)]
    pub colors: Vec<u32>,
    #[serde(default)]
    pub local: Option<Vec<u32>>,
    #[serde(default)]
    pub cbext: BTreeMap<u32, usize>,
    #[serde(default)]
    pub sbro: Vec<u32>,
    #[serde(default)]
    pub cl: bool,
    #[serde(default)]
    pub tk: bool,
    #[serde(default)]
    pub tkl: Option<String>,
    #[serde(default)]
    pub tkc: bool,
    #[serde(default)]
    pub tb: Option<Vec<u32>>,
}

/// Compiles the .metal files of `tags` in one Metal compiler launch: {tag: None | (phase, message)}; each AIR is renamed
/// after its entry point in its sharded directory.
fn compile_batch(items: &[(String, String)], work: &Path) -> HashMap<String, Option<(String, String)>> {
    let mut cmd = Command::new(metal_bin());
    // -fpreserve-invariance: [[invariant]] positions (msl_specialize) compute identically in every vertex shader.
    cmd.arg("-fpreserve-invariance").arg("-c").current_dir(work);
    for (tag, _) in items {
        cmd.arg(stage_metal_path(work, tag));
    }
    let r = run(cmd, MAX_RSS_MB);
    let stderr = match &r {
        Ok(o) => o.stderr.clone(),
        Err(e) => e.clone(),
    };
    let mut errors: HashMap<String, String> = HashMap::new();
    for line in stderr.lines() {
        if line.contains("error:") {
            let owner = Path::new(line.split(':').next().unwrap_or("")).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            errors.entry(owner).or_insert_with(|| line.rsplit("error:").next().unwrap_or("").trim().chars().take(120).collect());
        }
    }
    let first = stderr.trim().lines().next().unwrap_or("compiler produced no output").chars().take(120).collect::<String>();
    items
        .iter()
        .map(|(tag, entry)| {
            let air = work.join(format!("{tag}.air"));
            let dest = shard_dir(work, entry).join(format!("{entry}.air"));
            let res = if air.exists() && std::fs::rename(&air, &dest).is_ok() {
                None
            } else {
                Some(("metal".to_string(), errors.get(tag).cloned().unwrap_or_else(|| first.clone())))
            };
            (tag.clone(), res)
        })
        .collect()
}

/// Hash of everything that changes what a stage translates to: the decompiler, the MSL passes, the tool flags.
fn fingerprint() -> String {
    let mut bytes = Vec::new();
    bytes.extend(sass2spv::SOURCE_HASH.as_bytes());
    bytes.extend(include_str!("../../engine/src/metal/msl_specialize.rs").as_bytes());
    bytes.extend(include_str!("tiny_kernels.rs").as_bytes());
    bytes.extend(include_str!("batch_kernels.rs").as_bytes());
    bytes.extend(crate::translation::fingerprint().as_bytes());
    bytes.extend(b"spirv-cross --msl-version 30000 --msl-decoration-binding");
    // Which control section a stage is translated with (stage_ctrl): per-stage sections, no borrowing between variants.
    bytes.extend(b"ctrl v2");
    format!("rust:{:016x}", fnv1a64(&bytes))
}

/// Every weak hash a stage was seen with. The weak hash covers WEAK_LEN bytes, which for a stage smaller than that
/// include whatever followed it in memory when it was dumped, so identical tiny stages (same full hash) dumped at
/// different addresses have different weak hashes; the engine looks stages up by the weak hash of its own copy.
fn record_weaks(r: &StageRecord) -> Vec<u64> {
    let mut v = vec![r.weak];
    v.extend(r.weaks.iter().copied().filter(|&w| w != r.weak));
    v
}

/// Keeps one record per full hash and remembers each weak hash it was seen with (see record_weaks).
fn add_record(records: &mut BTreeMap<u64, StageRecord>, rec: StageRecord, weak: u64) {
    let full = rec.full;
    let kept = records.entry(full).or_insert(rec);
    if weak != kept.weak && !kept.weaks.contains(&weak) {
        kept.weaks.push(weak);
    }
}

/// What a failed translation depended on besides the toolchain: the stage's code and its control-section inputs. A
/// recorded failure is reused only while both are unchanged (a stage dumped again with its full control section is
/// retried).
fn fail_key(code: &Path, data: &[u8], o: &Options) -> String {
    format!("{:016x}:{}", fnv1a64(data), ctrl_fields(code, o.ctrl_dir.as_deref()))
}

type Known = (HashMap<u64, Vec<StageRecord>>, HashMap<u64, Vec<String>>);

#[derive(Serialize, Deserialize)]
struct StateFile {
    fingerprint: String,
    records: Vec<StageRecord>,
    failures: HashMap<u64, Vec<String>>,
}

fn load_state(path: &Path, fp: &str) -> Known {
    let Some(st) = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str::<StateFile>(&s).ok()) else {
        return Default::default();
    };
    if st.fingerprint != fp {
        println!("[+] toolchain changed since the last build: translating everything again");
        return Default::default();
    }
    let mut by_weak: HashMap<u64, Vec<StageRecord>> = HashMap::new();
    for r in st.records {
        for w in record_weaks(&r) {
            by_weak.entry(w).or_default().push(r.clone());
        }
    }
    (by_weak, st.failures)
}

fn save_state(path: &Path, fp: &str, records: &[&StageRecord], failures: &HashMap<u64, Vec<String>>) -> std::io::Result<()> {
    let file = StateFile {
        fingerprint: fp.to_string(),
        records: records.iter().map(|&r| r.clone()).collect(),
        failures: failures.clone(),
    };
    std::fs::write(path, serde_json::to_string(&file)?)
}

fn write_index(path: &Path, records: &[&StageRecord]) -> std::io::Result<()> {
    use std::fmt::Write;
    let mut f = String::from("# nxport shader cache index v1\n");
    let mut entries: Vec<(u64, &StageRecord)> = records.iter().flat_map(|r| record_weaks(r).into_iter().map(move |w| (w, *r))).collect();
    entries.sort_by_key(|(w, r)| (*w, r.size as u64, r.full));
    let nums = |v: &[u32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
    for (w, r) in entries {
        let mf = &r.manifest;
        let inst = if mf.uses_instance_id { 1 } else { 0 };
        let _ = writeln!(f, "S {w:016x} {} {:016x} {} {inst}", r.size, r.full, r.stage);
        if !r.attrs.is_empty() {
            let _ = writeln!(f, "va {}", nums(&r.attrs));
        }
        if !r.colors.is_empty() {
            let _ = writeln!(f, "co {}", nums(&r.colors));
        }
        if !r.varyings.is_empty() {
            let _ = writeln!(f, "vy {}", nums(&r.varyings));
        }
        if let Some(local) = &r.local {
            if !local.is_empty() {
                let _ = writeln!(f, "ls {}", nums(local));
            }
        }
        for c in &mf.cbuffers {
            let _ = writeln!(f, "cb {} {}", c.binding, c.slot);
        }
        for (b, n) in &r.cbext {
            let _ = writeln!(f, "cx {b} {n}");
        }
        for b in &mf.sbuffers {
            let _ = writeln!(f, "sb {} {} {} {}", b.binding, b.slot, b.cb_slot, b.cb_offset);
        }
        for b in &r.sbro {
            let _ = writeln!(f, "sr {b}");
        }
        if r.cl {
            f.push_str("cl\n");
        }
        // The stage has a compute version (tkv_<entry> / tkf_<entry>).
        // Fragment kernels also list the texture bindings they sample with implicit LOD (exact only for one level).
        if r.tk {
            match &r.tkl {
                Some(l) => { let _ = writeln!(f, "tk {}", if l.is_empty() { "-" } else { l }); }
                None => f.push_str("tk\n"),
            }
            if r.tkc {
                f.push_str("tkc\n");
            }
        }
        // Batched variant tb_<entry> (crate::batch_kernels): its address table's constant-buffer bindings in order.
        if let Some(b) = &r.tb {
            f.push_str(&format!("tb {}\n", b.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ")));
        }
        for t in &mf.textures {
            let _ = writeln!(f, "tex {} {} {} {}", t.binding, t.cbuf_slot, t.handle_index, t.texture_type.replace(", ", "+").replace(' ', ""));
        }
        for img in &mf.images {
            let _ = writeln!(f, "img {} {} {} {}", img.binding, img.cbuf_slot, img.handle_index, img.texture_type.replace(", ", "+").replace(' ', ""));
        }
        f.push_str("E\n");
    }
    std::fs::write(path, f)
}

/// Runs `f` over `items` on `jobs` threads, results in input order.
fn parallel_progress<T: Sync, R: Send>(
    items: &[T],
    jobs: usize,
    on_progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    f: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let total = items.len();
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..total).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..jobs.max(1) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let r = f(&items[i]);
                results.lock().unwrap()[i] = Some(r);
                let completed = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if let Some(cb) = on_progress {
                    cb(completed, total);
                }
            });
        }
    });
    results.into_inner().unwrap().into_iter().map(|r| r.unwrap()).collect()
}

fn parallel<T: Sync, R: Send>(items: &[T], jobs: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    parallel_progress(items, jobs, None, f)
}

pub fn build(o: &Options) -> Result<(), String> {
    let out_dir = std::fs::canonicalize(&o.out_dir).or_else(|_| {
        std::fs::create_dir_all(&o.out_dir)?;
        std::fs::canonicalize(&o.out_dir)
    }).map_err(|e| format!("{}: {e}", o.out_dir.display()))?;
    let work = out_dir.join("work");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    for i in 0x00..=0xff {
        let _ = std::fs::create_dir_all(work.join(format!("{i:02x}")));
    }
    let state_path = work.join("state.json");

    if o.index_only {
        let mut st: StateFile = serde_json::from_str(&std::fs::read_to_string(&state_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        // Text-derived metadata is recomputed from the MSL the build kept (work/<stem>.metal), so a fix to its analysis
        // reaches an existing cache without retranslating: the constant-buffer extents (`cx`).
        let mut ext_by_entry: HashMap<String, BTreeMap<u32, usize>> = HashMap::new();
        for m in collect_metal_files(&work) {
            let Ok(text) = std::fs::read_to_string(&m) else { continue };
            let Some(m) = entry_line_re().find(&text) else { continue };
            let Some(name) = m.as_str().split_whitespace().nth(2).map(|n| n.split('(').next().unwrap_or(n).to_string()) else { continue };
            ext_by_entry.insert(name, cbuf_extents(&text));
        }
        let mut changed = 0;
        for r in &mut st.records {
            if let Some(ext) = ext_by_entry.get(&r.entry) {
                if &r.cbext != ext {
                    r.cbext = ext.clone();
                    changed += 1;
                }
            }
        }
        if changed > 0 {
            std::fs::write(&state_path, serde_json::to_string(&st).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }
        println!("[+] constant-buffer extents recomputed from {} kept MSL files: {} stages changed", ext_by_entry.len(), changed);
        let records: Vec<&StageRecord> = st.records.iter().collect();
        write_index(&out_dir.join("index.txt"), &records).map_err(|e| e.to_string())?;
        println!("[+] wrote {} ({} stages)", out_dir.join("index.txt").display(), records.len());
        return Ok(());
    }

    // Tools run with cwd=work, so every path handed to them must be absolute.
    let mut codes: Vec<PathBuf> = Vec::new();
    for d in &o.dump_dirs {
        let d = std::fs::canonicalize(d).map_err(|e| format!("{}: {e}", d.display()))?;
        for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "code") {
                codes.push(e.path());
            }
        }
    }
    codes.sort();
    let mut stems: Vec<_> = codes.iter().map(|c| c.file_stem().unwrap().to_owned()).collect();
    stems.sort();
    stems.dedup();
    if stems.len() != codes.len() {
        return Err("two dump directories contain a stage with the same file name".into());
    }
    println!("[+] {} stages", codes.len());

    let t0 = std::time::Instant::now();
    let fp = fingerprint();
    let (known, known_fail) = load_state(&state_path, &fp);
    let mut records: BTreeMap<u64, StageRecord> = BTreeMap::new();
    let mut failures: Vec<(String, String, String)> = Vec::new();
    let mut fail_by_weak: HashMap<u64, Vec<String>> = HashMap::new();

    // Stages already translated by an earlier build need no work (the same two-level check the engine uses).
    let mut todo: Vec<(PathBuf, u64)> = Vec::new();
    let mut code_of: HashMap<u64, PathBuf> = HashMap::new();
    let mut reused = 0;
    for code in &codes {
        let data = std::fs::read(code).map_err(|e| e.to_string())?;
        let weak = fnv1a64(&data[..WEAK_LEN.min(data.len())]);
        code_of.insert(weak, code.clone());
        let hit = known.get(&weak).and_then(|rs| {
            rs.iter().find(|r| {
                let size = r.size;
                let entry = &r.entry;
                let air = entry_air_path(&work, entry);
                fnv1a64(&data[..size.min(data.len())]) == r.full && air.exists()
            })
        });
        if let Some(hit) = hit {
            add_record(&mut records, hit.clone(), weak);
            reused += 1;
        } else if known_fail.get(&weak).is_some_and(|f| f.len() == 3 && f[2] == fail_key(code, &data, o)) {
            let f = &known_fail[&weak];
            failures.push((code.file_name().unwrap().to_string_lossy().into_owned(), f[0].clone(), f[1].clone()));
            fail_by_weak.insert(weak, f.clone());
            reused += 1;
        } else {
            todo.push((code.clone(), weak));
        }
    }
    println!("[+] {reused} stages reused from the previous build, {} to translate", todo.len());

    // Phase 1: decompile in batches.
    let spv_dir = work.join("spv");
    std::fs::create_dir_all(&spv_dir).map_err(|e| e.to_string())?;
    let batches: Vec<Vec<PathBuf>> = todo.chunks(BATCH).map(|b| b.iter().map(|(c, _)| c.clone()).collect()).collect();
    let mut translated: HashMap<String, Translated> = HashMap::new();
    let total_stages = todo.len();
    if let Some(cb) = &o.on_progress {
        cb(ShaderBuildPhase::Decompiling, 0, total_stages);
    }
    let p_cb1 = o.on_progress.as_ref().map(|cb| {
        let cb = cb.clone();
        move |done_batches: usize, _: usize| {
            let done_stages = (done_batches * BATCH).min(total_stages);
            cb(ShaderBuildPhase::Decompiling, done_stages, total_stages);
        }
    });
    for r in parallel_progress(&batches, o.jobs, p_cb1.as_ref().map(|f| f as &(dyn Fn(usize, usize) + Sync)), |b| translate_batch(b, &spv_dir, o)) {
        translated.extend(r);
    }
    let ok = translated.values().filter(|t| matches!(t, Translated::Ok { .. })).count();
    println!("[+] decompiler done: {ok} of {} stages translated ({:.0}s)", todo.len(), t0.elapsed().as_secs_f64());

    // Phase 2a: MSL specialization per stage (after spirv-cross with --spirv-cross).
    let name_of = |c: &Path| c.file_name().unwrap().to_string_lossy().into_owned();
    let mut jobs: Vec<(PathBuf, u64, ShaderStage, usize)> = Vec::new();
    for (code, weak) in &todo {
        match translated.get(&code.file_stem().unwrap().to_string_lossy().into_owned()) {
            None => {
                let e = ("sass2spv".to_string(), "no result from the decompiler batch".to_string());
                failures.push((name_of(code), e.0.clone(), e.1.clone()));
                fail_by_weak.insert(*weak, vec![e.0, e.1]);
            }
            Some(Translated::Failed(p, m)) => {
                failures.push((name_of(code), p.clone(), m.clone()));
                fail_by_weak.insert(*weak, vec![p.clone(), m.clone()]);
            }
            Some(Translated::Ok { stage, size }) => jobs.push((code.clone(), *weak, *stage, *size)),
        }
    }
    let mut prepared: Vec<(PathBuf, u64, StageRecord, String)> = Vec::new();
    if let Some(cb) = &o.on_progress {
        cb(ShaderBuildPhase::TranslatingToMetal, 0, jobs.len());
    }
    let p_cb2 = o.on_progress.as_ref().map(|cb| {
        let cb = cb.clone();
        move |done, total| cb(ShaderBuildPhase::TranslatingToMetal, done, total)
    });
    for ((code, weak, _, _), res) in jobs.iter().zip(parallel_progress(&jobs, o.jobs, p_cb2.as_ref().map(|f| f as &(dyn Fn(usize, usize) + Sync)), |(c, _, st, sz)| prepare_stage(c, &work, &spv_dir, *st, *sz, o))) {
        match res {
            Ok((rec, tag)) => prepared.push((code.clone(), *weak, rec, tag)),
            Err((p, m)) => {
                failures.push((name_of(code), p.clone(), m.clone()));
                fail_by_weak.insert(*weak, vec![p, m]);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&spv_dir);
    if let Some(msl_out) = &o.msl_out {
        std::fs::create_dir_all(msl_out).map_err(|e| e.to_string())?;
        for (_, _, rec, tag) in &prepared {
            let src = stage_metal_path(&work, tag);
            let _ = std::fs::rename(src, msl_out.join(format!("{}.metal", rec.entry)));
        }
        println!("[+] wrote {} MSL files to {}", prepared.len(), msl_out.display());
        return Ok(());
    }
    println!("[+] MSL prepared for {} stages ({:.0}s), compiling Metal in batches of {COMPILE_BATCH}", prepared.len(), t0.elapsed().as_secs_f64());

    // Phase 2b: Metal compile, many files per launch.
    let groups: Vec<Vec<(String, String)>> = prepared
        .chunks(COMPILE_BATCH)
        .map(|g| g.iter().map(|(_, _, rec, tag)| (tag.clone(), rec.entry.clone())).collect())
        .collect();
    let total_stages = prepared.len();
    let mut compiled: HashMap<String, Option<(String, String)>> = HashMap::new();
    if let Some(cb) = &o.on_progress {
        cb(ShaderBuildPhase::CompilingMetal, 0, total_stages);
    }
    let p_cb3 = o.on_progress.as_ref().map(|cb| {
        let cb = cb.clone();
        move |done_batches: usize, _: usize| {
            let done_stages = (done_batches * COMPILE_BATCH).min(total_stages);
            cb(ShaderBuildPhase::CompilingMetal, done_stages, total_stages);
        }
    });
    for r in parallel_progress(&groups, o.jobs, p_cb3.as_ref().map(|f| f as &(dyn Fn(usize, usize) + Sync)), |g| compile_batch(g, &work)) {
        compiled.extend(r);
    }
    // A stage whose appended tiny-pass kernels (tiny_kernels.rs) do not compile is compiled again without them: the
    // kernels must never cost the stage itself.
    let mut no_tiny: std::collections::HashSet<String> = std::collections::HashSet::new();
    let retry: Vec<(String, String)> = prepared.iter().filter_map(|(_, _, rec, tag)| {
        compiled.get(tag).cloned().flatten()?;
        let path = stage_metal_path(&work, tag);
        let text = std::fs::read_to_string(&path).ok()?;
        let cut = text.find(crate::tiny_kernels::MARKER)?;
        std::fs::write(&path, &text[..cut]).ok()?;
        Some((tag.clone(), rec.entry.clone()))
    }).collect();
    if !retry.is_empty() {
        let n = retry.len();
        let groups: Vec<Vec<(String, String)>> = retry.chunks(COMPILE_BATCH).map(|g| g.to_vec()).collect();
        let mut fixed = 0;
        for r in parallel(&groups, o.jobs, |g| compile_batch(g, &work)) {
            for (tag, res) in r {
                if res.is_none() {
                    fixed += 1;
                    no_tiny.insert(tag.clone());
                }
                compiled.insert(tag, res);
            }
        }
        println!("[+] {} stages failed with their tiny-pass kernels; {} compile without them", n, fixed);
    }
    for (code, weak, mut rec, tag) in prepared {
        if no_tiny.contains(&tag) {
            rec.tk = false;
        }
        match compiled.get(&tag).cloned().flatten() {
            None => add_record(&mut records, rec, weak), // identical stages dumped at different addresses
            Some((p, m)) => {
                failures.push((name_of(&code), p.clone(), m.clone()));
                fail_by_weak.insert(weak, vec![p, m]);
            }
        }
    }
    for (w, v) in fail_by_weak.iter_mut() {
        if v.len() == 2 {
            if let Some(c) = code_of.get(w) {
                let data = std::fs::read(c).unwrap_or_default();
                v.push(fail_key(c, &data, o));
            }
        }
    }
    let recs: Vec<&StageRecord> = records.values().collect();
    save_state(&state_path, &fp, &recs, &fail_by_weak).map_err(|e| e.to_string())?;

    println!("[+] Metal compile done ({:.0}s), linking libraries", t0.elapsed().as_secs_f64());
    let mut by_entry: Vec<&StageRecord> = records.values().collect();
    by_entry.sort_by(|a, b| a.entry.cmp(&b.entry));
    let airs: Vec<PathBuf> = by_entry.iter().map(|r| entry_air_path(&work, &r.entry)).collect();
    let total_libs = airs.len().div_ceil(LINK_CHUNK);
    let chunks: Vec<(usize, &[PathBuf], PathBuf)> = airs
        .chunks(LINK_CHUNK)
        .enumerate()
        .map(|(i, chunk)| (i, chunk, out_dir.join(format!("shaders_{i:04}.metallib"))))
        .collect();

    // Clean up any extra metallibs from previous runs that had more chunks
    for e in std::fs::read_dir(&out_dir).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with("shaders_") && n.ends_with(".metallib") {
            if let Some(idx_str) = n.strip_prefix("shaders_").and_then(|s| s.strip_suffix(".metallib")) {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if idx >= total_libs {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
    }

    // Determine which chunks need linking
    let mut dirty_indices: Vec<usize> = Vec::new();
    let mut lib_bytes: u64 = 0;
    for (i, chunk, lib) in &chunks {
        let is_dirty = match std::fs::metadata(lib) {
            Ok(m) if m.len() > 0 => {
                let lib_mtime = m.modified().ok();
                chunk.iter().any(|air| {
                    match std::fs::metadata(air).and_then(|am| am.modified()) {
                        Ok(air_mtime) => lib_mtime.map_or(true, |lm| air_mtime > lm),
                        Err(_) => true,
                    }
                })
            }
            _ => true,
        };
        if is_dirty {
            dirty_indices.push(*i);
        } else if let Ok(m) = std::fs::metadata(lib) {
            lib_bytes += m.len();
        }
    }

    if dirty_indices.is_empty() {
        println!("[+] {} shader libraries already up to date ({} KB), skipping link", total_libs, lib_bytes / 1024);
        if let Some(cb) = &o.on_progress {
            cb(ShaderBuildPhase::LinkingLibraries, total_libs, total_libs);
        }
    } else {
        println!("[+] linking {} of {} shader libraries in parallel", dirty_indices.len(), total_libs);
        let done_count = std::sync::atomic::AtomicUsize::new(total_libs - dirty_indices.len());
        if let Some(cb) = &o.on_progress {
            cb(ShaderBuildPhase::LinkingLibraries, done_count.load(std::sync::atomic::Ordering::Relaxed), total_libs);
        }
        let dirty_chunks: Vec<&(usize, &[PathBuf], PathBuf)> = dirty_indices.iter().map(|&i| &chunks[i]).collect();

        let link_res: Result<Vec<u64>, String> = parallel(&dirty_chunks, o.jobs, |(i, chunk, lib)| {
            let mut cmd = Command::new("xcrun");
            cmd.args(["-sdk", "macosx", "metallib"]).args(*chunk).arg("-o").arg(lib);
            let r = run_timeout(cmd, MAX_RSS_MB, 180)?;
            if r.status != Some(0) {
                return Err(format!("metallib link failed for chunk {i}: {}", r.stderr.chars().take(500).collect::<String>()));
            }
            let size = std::fs::metadata(lib).map(|m| m.len()).unwrap_or(0);
            let completed = done_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if let Some(cb) = &o.on_progress {
                cb(ShaderBuildPhase::LinkingLibraries, completed, total_libs);
            }
            Ok(size)
        }).into_iter().collect();

        for sz in link_res? {
            lib_bytes += sz;
        }
    }
    write_index(&out_dir.join("index.txt"), &recs).map_err(|e| e.to_string())?;
    println!("[+] {} unique stages -> {} libraries ({} KB)", records.len(), airs.len().div_ceil(LINK_CHUNK), lib_bytes / 1024);

    let mut by: HashMap<(String, String), usize> = HashMap::new();
    for (_, p, m) in &failures {
        *by.entry((p.clone(), m.chars().take(70).collect())).or_default() += 1;
    }
    let mut by: Vec<_> = by.into_iter().collect();
    by.sort_by(|a, b| b.1.cmp(&a.1));
    println!("[-] {} stages not translated:", failures.len());
    for ((p, m), n) in by.iter().take(10) {
        println!("      {n:5}  {p}: {m}");
    }
    let text: String = failures.iter().map(|(n, p, m)| format!("{n}\t{p}\t{m}\n")).collect();
    std::fs::write(out_dir.join("failures.txt"), text).map_err(|e| e.to_string())?;
    Ok(())
}

/// `nxport-cli shaders build-ios CACHE OUT`: the iOS flavour of a macOS cache. The same index and failure list, with the
/// metallibs compiled for iOS from the specialized MSL the macOS build keeps in CACHE/work. The engine finds functions by
/// entry name in whatever libraries it loads, so the grouping into libraries need not match the macOS build.
/// Incremental: an .air is rebuilt only when its .metal is newer. MSL 3.0 (spirv-cross --msl-version 30000) needs iOS 16.
pub fn build_ios(
    cache: &Path,
    out: &Path,
    jobs: usize,
    simulator: bool,
    on_progress: Option<&(dyn Fn(IosShaderBuildPhase, usize, usize) + Sync)>,
) -> Result<(), String> {
    const IOS_LINK_CHUNK: usize = 150; // fewer files open on iOS than macOS's 50 per library (iOS limits open files)
    const MIN_IOS: &str = "16.0";
    let src = std::fs::canonicalize(cache.join("work")).map_err(|_| format!("{}/work missing: build the macOS cache first (nxport-cli shaders build)", cache.display()))?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let out = std::fs::canonicalize(out).map_err(|e| e.to_string())?;
    let work = out.join("work_ios");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    for i in 0x00..=0xff {
        let _ = std::fs::create_dir_all(work.join(format!("{i:02x}")));
    }
    for f in ["index.txt", "failures.txt"] {
        if cache.join(f).exists() {
            std::fs::copy(cache.join(f), out.join(f)).map_err(|e| e.to_string())?;
        }
    }
    let sdk = if simulator { "iphonesimulator" } else { "iphoneos" };
    let min_flag = if simulator { format!("-mios-simulator-version-min={MIN_IOS}") } else { format!("-mios-version-min={MIN_IOS}") };
    let mut c = Command::new("xcrun");
    c.args(["-sdk", sdk, "-f", "metal"]);
    let metal = run(c, MAX_RSS_MB)?.stdout.trim().to_string();
    // The stages the index uses (entry s_<full hash>), each from its newest MSL file: the work dir also keeps MSL of
    // stages from earlier builds, and several files can carry the same entry (the macOS build keeps one .air per entry).
    let index = std::fs::read_to_string(cache.join("index.txt")).map_err(|e| e.to_string())?;
    let wanted: std::collections::HashSet<String> =
        index.lines().filter(|l| l.starts_with("S ")).filter_map(|l| l.split_whitespace().nth(3)).map(|h| format!("s_{h}")).collect();
    let entry_re = Regex::new(r"(?m)^(?:fragment|vertex|kernel) .*?\b(s_[0-9a-f]{16})\(").unwrap();
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut by_entry: BTreeMap<String, PathBuf> = BTreeMap::new();
    for m in collect_metal_files(&src) {
        let text = String::from_utf8_lossy(&std::fs::read(&m).unwrap_or_default()).into_owned();
        if let Some(c) = entry_re.captures(&text) {
            let entry = c[1].to_string();
            if wanted.contains(&entry) {
                let newer = by_entry.get(&entry).is_none_or(|cur| mtime(&m) > mtime(cur));
                if newer {
                    by_entry.insert(entry, m);
                }
            }
        }
    }
    let todo: Vec<(String, PathBuf)> = by_entry
        .iter()
        .filter(|(entry, m)| {
            let air = entry_air_path(&work, entry);
            !air.exists() || mtime(&air) < mtime(m)
        })
        .map(|(e, m)| (e.clone(), m.clone()))
        .collect();
    println!("[+] {} stages in the index, {} with MSL, {} to compile for iOS", wanted.len(), by_entry.len(), todo.len());

    let total_stages = todo.len();
    let batches: Vec<&[(String, PathBuf)]> = todo.chunks(COMPILE_BATCH).collect();
    if let Some(cb) = on_progress {
        cb(IosShaderBuildPhase::CompilingShaders, 0, total_stages);
    }
    let p_cb_ios = on_progress.map(|cb| {
        move |done_batches: usize, _: usize| {
            let done_stages = (done_batches * COMPILE_BATCH).min(total_stages);
            cb(IosShaderBuildPhase::CompilingShaders, done_stages, total_stages);
        }
    });
    let results = parallel_progress(&batches, jobs, p_cb_ios.as_ref().map(|f| f as &(dyn Fn(usize, usize) + Sync)), |items| {
        // The compiler writes <tag>.air into the work dir; renamed to <entry>.air like the macOS build.
        let mut cmd = Command::new(&metal);
        cmd.arg(&min_flag).arg("-fpreserve-invariance").arg("-c").args(items.iter().map(|(_, m)| m)).current_dir(&work);
        let stderr = match run(cmd, MAX_RSS_MB) {
            Ok(o) => o.stderr,
            Err(e) => e,
        };
        let bad: Vec<String> = items
            .iter()
            .filter(|(entry, m)| {
                let air = work.join(format!("{}.air", m.file_stem().unwrap().to_string_lossy()));
                let dest = shard_dir(&work, entry).join(format!("{entry}.air"));
                !(air.exists() && std::fs::rename(&air, &dest).is_ok())
            })
            .map(|(e, _)| e.clone())
            .collect();
        (bad, stderr)
    });
    let mut failed: Vec<String> = Vec::new();
    for (bad, err) in results {
        if !bad.is_empty() && failed.len() < 5 {
            println!("[!] {} failed in a batch: {:?}", bad.len(), err.trim().lines().take(2).collect::<Vec<_>>());
        }
        failed.extend(bad);
    }
    let airs: Vec<PathBuf> = by_entry.keys().map(|e| entry_air_path(&work, e)).filter(|p| p.exists()).collect();
    let total_libs = airs.len().div_ceil(IOS_LINK_CHUNK);
    let chunks: Vec<(usize, &[PathBuf], PathBuf)> = airs
        .chunks(IOS_LINK_CHUNK)
        .enumerate()
        .map(|(i, chunk)| (i, chunk, out.join(format!("shaders_{i:04}.metallib"))))
        .collect();

    for e in std::fs::read_dir(&out).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with("shaders_") && n.ends_with(".metallib") {
            if let Some(idx_str) = n.strip_prefix("shaders_").and_then(|s| s.strip_suffix(".metallib")) {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if idx >= total_libs {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
    }

    let mut dirty_indices: Vec<usize> = Vec::new();
    let mut size: u64 = 0;
    for (i, chunk, lib) in &chunks {
        let is_dirty = match std::fs::metadata(lib) {
            Ok(m) if m.len() > 0 => {
                let lib_mtime = m.modified().ok();
                chunk.iter().any(|air| {
                    match std::fs::metadata(air).and_then(|am| am.modified()) {
                        Ok(air_mtime) => lib_mtime.map_or(true, |lm| air_mtime > lm),
                        Err(_) => true,
                    }
                })
            }
            _ => true,
        };
        if is_dirty {
            dirty_indices.push(*i);
        } else if let Ok(m) = std::fs::metadata(lib) {
            size += m.len();
        }
    }

    if dirty_indices.is_empty() {
        println!("[+] {} iOS shader libraries already up to date ({} MB), skipping link", total_libs, size >> 20);
        if let Some(cb) = on_progress {
            cb(IosShaderBuildPhase::LinkingLibraries, total_libs, total_libs);
        }
    } else {
        println!("[+] linking {} of {} iOS shader libraries in parallel", dirty_indices.len(), total_libs);
        let done_count = std::sync::atomic::AtomicUsize::new(total_libs - dirty_indices.len());
        if let Some(cb) = on_progress {
            cb(IosShaderBuildPhase::LinkingLibraries, done_count.load(std::sync::atomic::Ordering::Relaxed), total_libs);
        }
        let dirty_chunks: Vec<&(usize, &[PathBuf], PathBuf)> = dirty_indices.iter().map(|&i| &chunks[i]).collect();

        let link_res: Result<Vec<u64>, String> = parallel(&dirty_chunks, jobs, |(i, chunk, lib)| {
            let mut cmd = Command::new("xcrun");
            cmd.args(["-sdk", sdk, "metallib"]).args(*chunk).arg("-o").arg(lib);
            let r = run_timeout(cmd, MAX_RSS_MB, 180)?;
            if r.status != Some(0) {
                return Err(format!("metallib link failed for chunk {i}: {}", r.stderr.chars().take(500).collect::<String>()));
            }
            let sz = std::fs::metadata(lib).map(|m| m.len()).unwrap_or(0);
            let completed = done_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if let Some(cb) = on_progress {
                cb(IosShaderBuildPhase::LinkingLibraries, completed, total_libs);
            }
            Ok(sz)
        }).into_iter().collect();

        for sz in link_res? {
            size += sz;
        }
    }
    println!("[+] {} stages in {total_libs} iOS metallibs ({} MB); {} failed to compile for iOS", airs.len(), size >> 20, failed.len());
    for f in failed.iter().take(20) {
        println!("    {f}");
    }
    Ok(())
}
