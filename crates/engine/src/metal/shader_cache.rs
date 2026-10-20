// Lookup side of the AOT shader cache built by `nxport-cli shaders build` (crates/cli/src/shader_cache_build.rs).
//
// <cache>/index.txt maps a stage's code bytes to a translated Metal function (`s_<hash>` in
// <cache>/shaders.metallib) plus the resource bindings the translated function expects. A stage is
// identified by a two-level hash: a weak hash of its first WEAK_LEN bytes selects candidate sizes, then
// the exact `size` bytes are hashed and compared (the stage's real length isn't known to the engine, and
// whatever follows it in memory must not influence the key).
//
// The cache directory comes from --shader-cache (default ./shader_cache). With no cache every lookup
// misses and nothing is drawn.

use objc2_foundation::NSString;
use objc2_metal::MTLDevice;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const WEAK_LEN: usize = 0x130;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageKind {
    Vertex,
    TessControl,
    TessEval,
    Geometry,
    Fragment,
    Compute,
    Other,
}

#[derive(Clone, Debug)]
pub struct TexBinding {
    pub binding: u32,
    /// Word index of the handle within that constant buffer.
    pub handle_index: u32,
    pub ty: String,
    /// `ty` declares a depth-compare sampler ("+Shadow"), precomputed (checked for every binding of every draw).
    pub shadow: bool,
    /// Texture type `ty` declares (MTLTextureType raw value), precomputed for the same reason.
    pub declared: usize,
}

#[derive(Clone, Debug)]
pub struct CachedStage {
    pub kind: StageKind,
    pub entry: String,
    /// Bytes from the start of the stage data (0x30 preamble + 0x50 SPH + code) to the end of the code.
    pub code_end: usize,
    /// Vertex attribute locations the stage reads (function constant id == location).
    pub attrs: Vec<u32>,
    /// Fragment color outputs the stage writes (function constant id == 32 + location).
    pub colors: Vec<u32>,
    /// Varying (user(locnN)) locations a vertex stage writes / a fragment stage reads. A fragment input the paired
    /// vertex stage does not write is switched off with function constant 48 + N (it reads zero).
    pub varyings: Vec<u32>,
    /// (Metal buffer index, guest constant buffer slot)
    pub cbuffers: Vec<(u32, u32)>,
    /// (Metal buffer index, bytes): constant buffers the stage reads only at literal offsets below `bytes` (index
    /// line `cx`). Others may be read anywhere in their declared 64 KB.
    pub cbuf_extent: Vec<(u32, u32)>,
    pub textures: Vec<TexBinding>,
    pub images: Vec<TexBinding>,
    /// Storage buffers: (Metal buffer index, storage slot, cbuf holding the descriptor, descriptor offset in words).
    pub sbuffers: Vec<(u32, u32, u32, u32)>,
    /// Metal buffer indices of storage buffers the stage never writes (index line `sr`; spirv-cross's `const device`).
    /// Caches built before the line existed have none: every storage buffer then counts as written.
    pub sb_readonly: Vec<u32>,
    /// The stage clamps computed constant buffer reads to the bound sizes the engine writes into the driver cbuf (index
    /// line `cl`; msl_specialize.bound_cbuf_reads): it needs binding 17 even when it reads nothing else there.
    pub cbuf_limits: bool,
    /// The stage has a compute version for the tiny-pass path (index line `tk`): tkv_<entry> for a
    /// vertex stage, tkf_<entry> for a fragment stage.
    pub tiny_kernel: bool,
    /// Fragment kernels: Metal texture bindings sampled with implicit LOD (index `tk <list>`; `-` none). None: unknown
    /// (older caches), every sampled texture must then have one level.
    pub tiny_implicit: Option<Vec<u32>>,
    /// Fragment kernels that apply folded clears (index line `tkc`).
    pub tiny_clears: bool,
    /// The stage has a batched variant tb_<entry> (index line `tb`, crates/cli/src/batch_kernels.rs): the constant
    /// buffer bindings its address table holds, in order.
    pub batch_cbufs: Option<Vec<u32>>,
    /// Compute workgroup size (the kernel does not declare it; Metal takes it at dispatch).
    pub local_size: [u32; 3],
    /// Fragment stage only: its SPH says it can discard (KillsPixels), writes depth or sample mask, or stores to
    /// memory. Read from the stage's own header at lookup (the hash covers it, so it is fixed per entry).
    pub fs_effects: OnceLock<bool>,
}

impl CachedStage {
    /// Bytes of the constant buffer at Metal index `binding` the stage can read: its literal-offset extent, else `max`.
    pub fn cbuf_read_len(&self, binding: u32, max: usize) -> usize {
        self.cbuf_extent
            .iter()
            .find(|&&(b, _)| b == binding)
            .map_or(max, |&(_, n)| (n as usize).min(max))
    }
}

struct Candidate {
    size: usize,
    full: u64,
    stage: usize,
}

struct Index {
    weak: HashMap<u64, Vec<Candidate>>,
    stages: Vec<CachedStage>,
}

static INDEX: OnceLock<Option<Index>> = OnceLock::new();

pub fn cache_dir() -> std::path::PathBuf {
    static D: OnceLock<std::path::PathBuf> = OnceLock::new();
    D.get_or_init(|| {
        if let Some(dir) = &crate::settings::get().files.shader_cache {
            return dir.clone();
        }
        crate::profile::data_subdir("shader_cache").unwrap_or_else(|| "shader_cache".into())
    })
    .clone()
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn parse_kind(s: &str) -> StageKind {
    match s {
        "Vertex" => StageKind::Vertex,
        "TessellationControl" => StageKind::TessControl,
        "TessellationEvaluation" => StageKind::TessEval,
        "Geometry" => StageKind::Geometry,
        "Fragment" => StageKind::Fragment,
        "Compute" => StageKind::Compute,
        _ => StageKind::Other,
    }
}

fn parse_index(text: &str) -> Index {
    let mut idx = Index {
        weak: HashMap::new(),
        stages: Vec::new(),
    };
    let mut cur: Option<(u64, usize, u64, CachedStage)> = None;
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.as_slice() {
            ["S", weak, size, full, stage, _instance_id] => {
                let (Ok(weak), Ok(size), Ok(full)) = (
                    u64::from_str_radix(weak, 16),
                    size.parse::<usize>(),
                    u64::from_str_radix(full, 16),
                ) else {
                    continue;
                };
                cur = Some((
                    weak,
                    size,
                    full,
                    CachedStage {
                        kind: parse_kind(stage),
                        entry: format!("s_{:016x}", full),
                        code_end: size,
                        attrs: Vec::new(),
                        colors: Vec::new(),
                        varyings: Vec::new(),
                        cbuffers: Vec::new(),
                        cbuf_extent: Vec::new(),
                        textures: Vec::new(),
                        images: Vec::new(),
                        sbuffers: Vec::new(),
                        sb_readonly: Vec::new(),
                        cbuf_limits: false,
                        tiny_kernel: false,
                        tiny_implicit: None,
                        tiny_clears: false,
                        batch_cbufs: None,
                        local_size: [1, 1, 1],
                        fs_effects: OnceLock::new(),
                    },
                ));
            }
            ["va", locs @ ..] => {
                if let Some((.., st)) = cur.as_mut() {
                    st.attrs = locs.iter().filter_map(|l| l.parse().ok()).collect();
                }
            }
            ["ls", x, y, z] => {
                if let (Some((.., st)), Ok(x), Ok(y), Ok(z)) =
                    (cur.as_mut(), x.parse(), y.parse(), z.parse())
                {
                    st.local_size = [x, y, z];
                }
            }
            ["vy", locs @ ..] => {
                if let Some((.., st)) = cur.as_mut() {
                    st.varyings = locs.iter().filter_map(|l| l.parse().ok()).collect();
                }
            }
            ["co", locs @ ..] => {
                if let Some((.., st)) = cur.as_mut() {
                    st.colors = locs.iter().filter_map(|l| l.parse().ok()).collect();
                }
            }
            ["cl"] => {
                if let Some((.., st)) = cur.as_mut() {
                    st.cbuf_limits = true;
                }
            }
            ["tk"] => {
                if let Some((.., st)) = cur.as_mut() {
                    st.tiny_kernel = true;
                }
            }
            ["tb", bindings @ ..] => {
                if let Some((.., st)) = cur.as_mut() {
                    st.batch_cbufs = Some(bindings.iter().filter_map(|b| b.parse().ok()).collect());
                }
            }
            ["tkc"] => {
                if let Some((.., st)) = cur.as_mut() {
                    st.tiny_clears = true;
                }
            }
            ["tk", list] => {
                if let Some((.., st)) = cur.as_mut() {
                    st.tiny_kernel = true;
                    st.tiny_implicit = Some(if *list == "-" {
                        Vec::new()
                    } else {
                        list.split(',').filter_map(|b| b.parse().ok()).collect()
                    });
                }
            }
            ["sr", binding] => {
                if let (Some((.., st)), Ok(b)) = (cur.as_mut(), binding.parse()) {
                    st.sb_readonly.push(b);
                }
            }
            ["sb", binding, slot, cb_slot, cb_off] => {
                if let (Some((.., st)), Ok(b), Ok(s), Ok(c), Ok(o)) = (
                    cur.as_mut(),
                    binding.parse(),
                    slot.parse(),
                    cb_slot.parse(),
                    cb_off.parse(),
                ) {
                    st.sbuffers.push((b, s, c, o));
                }
            }
            ["cx", binding, bytes] => {
                if let (Some((.., st)), Ok(b), Ok(n)) =
                    (cur.as_mut(), binding.parse(), bytes.parse())
                {
                    st.cbuf_extent.push((b, n));
                }
            }
            ["cb", binding, slot] => {
                if let (Some((.., st)), Ok(b), Ok(s)) =
                    (cur.as_mut(), binding.parse(), slot.parse())
                {
                    st.cbuffers.push((b, s));
                }
            }
            [kind @ ("tex" | "img"), binding, cbuf_slot, handle, ty] => {
                if let (Some((.., st)), Ok(binding), Ok(_), Ok(handle_index)) = (
                    cur.as_mut(),
                    binding.parse(),
                    cbuf_slot.parse::<i32>(),
                    handle.parse(),
                ) {
                    let tb = TexBinding {
                        binding,
                        handle_index,
                        ty: ty.to_string(),
                        shadow: ty.contains("+Shadow"),
                        declared: declared_texture_type(ty),
                    };
                    if *kind == "tex" {
                        st.textures.push(tb)
                    } else {
                        st.images.push(tb)
                    }
                }
            }
            ["E"] => {
                if let Some((weak, size, full, st)) = cur.take() {
                    idx.stages.push(st);
                    let stage = idx.stages.len() - 1;
                    idx.weak
                        .entry(weak)
                        .or_default()
                        .push(Candidate { size, full, stage });
                }
            }
            _ => {}
        }
    }
    idx
}

fn index() -> Option<&'static Index> {
    INDEX
        .get_or_init(|| {
            let path = cache_dir().join("index.txt");
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let idx = parse_index(&text);
                    println!(
                        "[+] Shader cache: {} stages from {}",
                        idx.stages.len(),
                        path.display()
                    );
                    Some(idx)
                }
                Err(_) => {
                    println!(
                        "[!] Shader cache: {} not found; {}",
                        path.display(),
                        if crate::settings::get().graphics.dynamic_shaders {
                            "shaders are translated as the game loads them"
                        } else {
                            "nothing is drawn"
                        }
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// Stages translated at run time are kept for later starts in <app support>/translated_shaders/<program>/<build>/:
/// index.txt (records in the cache index format) and <entry>.metal (the specialized source, compiled again when the game
/// first uses the stage). <program> is the executable's path (an installed app and development builds keep their own),
/// <build> the engine's size and time and the translator's source hash: a rebuilt engine or translator starts over, and
/// removes the folders of its earlier builds.
fn runtime_dir() -> Option<&'static std::path::Path> {
    static D: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    D.get_or_init(|| {
        if !crate::settings::get().graphics.dynamic_shaders {
            return None;
        }
        let exe = std::env::current_exe().ok()?;
        let base = crate::profile::data_dir()?
            .join("translated_shaders")
            .join(format!("{:016x}", fnv1a64(exe.as_os_str().as_encoded_bytes())));
        let mut id = Vec::new();
        let m = std::fs::metadata(&exe).ok()?;
        let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        id.extend(m.len().to_le_bytes());
        id.extend(t.to_le_bytes());
        id.extend(sass2spv::SOURCE_HASH.as_bytes());
        let name = format!("{:016x}", fnv1a64(&id));
        for e in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            if e.file_name() != name.as_str() && e.path().is_dir() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        let d = base.join(name);
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    })
    .as_deref()
}

/// The stages kept from earlier starts (runtime_dir), read once.
fn runtime_index() -> Option<&'static Index> {
    static R: OnceLock<Option<Index>> = OnceLock::new();
    R.get_or_init(|| {
        let text = std::fs::read_to_string(runtime_dir()?.join("index.txt")).ok()?;
        let idx = parse_index(&text);
        println!("[+] Shader cache: {} stages translated on earlier starts", idx.stages.len());
        Some(idx)
    })
    .as_ref()
}

/// Entries whose library has been compiled (from either source); full hashes already written to runtime_dir.
static COMPILED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
static SAVED: OnceLock<Mutex<std::collections::HashSet<u64>>> = OnceLock::new();

/// Compiles specialized MSL as the cache build does (compiler defaults, -fpreserve-invariance).
fn compile_msl(
    src: &str,
    entry: &str,
) -> Option<objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLLibrary>>> {
    let device = super::device()?;
    let options = objc2_metal::MTLCompileOptions::new();
    options.setPreserveInvariance(true);
    match device.newLibraryWithSource_options_error(&NSString::from_str(src), Some(&options)) {
        Ok(l) => Some(l),
        Err(e) => {
            eprintln!("[dynamic-shader] Metal compile error for {entry}: {}", e.localizedDescription());
            None
        }
    }
}

/// A stage kept from an earlier start, with its library compiled on first use.
fn runtime_stage(st: &'static CachedStage) -> Option<&'static CachedStage> {
    let compiled = COMPILED.get_or_init(Default::default);
    if compiled.lock().unwrap().contains(&st.entry) {
        return Some(st);
    }
    let src = std::fs::read_to_string(runtime_dir()?.join(format!("{}.metal", st.entry))).ok()?;
    let lib = compile_msl(&src, &st.entry)?;
    super::translated::register_dynamic_library(&st.entry, lib);
    compiled.lock().unwrap().insert(st.entry.clone());
    Some(st)
}

/// Writes a stage translated at run time to runtime_dir: its source, then its index record (in that order, so a record
/// never names a missing source).
fn save_runtime_stage(weak: u64, full: u64, st: &CachedStage, msl: &str) {
    use std::fmt::Write;
    let Some(dir) = runtime_dir() else { return };
    let saved = SAVED.get_or_init(|| {
        Mutex::new(runtime_index().map_or_else(Default::default, |i| {
            i.weak.values().flatten().map(|c| c.full).collect()
        }))
    });
    let mut saved = saved.lock().unwrap();
    if !saved.insert(full) || std::fs::write(dir.join(format!("{}.metal", st.entry)), msl).is_err() {
        return;
    }
    let nums = |v: &[u32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
    let mut r = format!("S {weak:016x} {} {full:016x} {} 0\n", st.code_end, stage_kind_str(st.kind));
    for (tag, v) in [("va", &st.attrs), ("co", &st.colors), ("vy", &st.varyings)] {
        if !v.is_empty() {
            let _ = writeln!(r, "{tag} {}", nums(v));
        }
    }
    if st.kind == StageKind::Compute {
        let _ = writeln!(r, "ls {}", nums(&st.local_size));
    }
    for (b, slot) in &st.cbuffers {
        let _ = writeln!(r, "cb {b} {slot}");
    }
    for (b, n) in &st.cbuf_extent {
        let _ = writeln!(r, "cx {b} {n}");
    }
    if st.cbuf_limits {
        r.push_str("cl\n");
    }
    for b in &st.sb_readonly {
        let _ = writeln!(r, "sr {b}");
    }
    for (b, s, c, o) in &st.sbuffers {
        let _ = writeln!(r, "sb {b} {s} {c} {o}");
    }
    for (tag, list) in [("tex", &st.textures), ("img", &st.images)] {
        for t in list {
            let _ = writeln!(r, "{tag} {} 0 {} {}", t.binding, t.handle_index, t.ty);
        }
    }
    r.push_str("E\n");
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("index.txt")) {
        let _ = f.write_all(r.as_bytes());
    }
}

pub static DYNAMIC_STAGES: OnceLock<Mutex<HashMap<u64, &'static CachedStage>>> = OnceLock::new();
static DYNAMIC_STAGES_BY_HASH: OnceLock<Mutex<HashMap<u64, &'static CachedStage>>> =
    OnceLock::new();

/// Resolves a stage by the code bytes at `data` (a host pointer to the NVN stage data).
///
/// # Safety
/// `data` must point at readable guest memory covering at least the stage's real size (the executor
/// already reads a 64 KB window there for dumping).
pub unsafe fn lookup(data: u64) -> Option<&'static CachedStage> {
    if let Some(stages) = DYNAMIC_STAGES.get() {
        if let Some(&st) = stages.lock().unwrap().get(&data) {
            return Some(st);
        }
    }
    if data < 0x1_0000_0000 {
        return None;
    }
    let weak = fnv1a64(std::slice::from_raw_parts(data as *const u8, WEAK_LEN));
    let find = |idx: &'static Index| -> Option<&'static CachedStage> {
        idx.weak.get(&weak)?.iter().find_map(|c| {
            let bytes = std::slice::from_raw_parts(data as *const u8, c.size);
            (fnv1a64(bytes) == c.full).then(|| &idx.stages[c.stage])
        })
    };
    let st = match index().and_then(find) {
        Some(st) => st,
        None => runtime_stage(runtime_index().and_then(find)?)?,
    };
    if st.kind == StageKind::Fragment {
        st.fs_effects.get_or_init(|| fragment_effects(data));
    }
    Some(st)
}

/// A stage of the cache index by its entry name (`s_<hash>`).
pub fn by_entry(entry: &str) -> Option<&'static CachedStage> {
    static M: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    let idx = index()?;
    // The first of duplicate entries, as lookup finds it (pipeline keys hold the stage's address).
    let m = M.get_or_init(|| {
        let mut m = HashMap::new();
        for (i, s) in idx.stages.iter().enumerate() {
            m.entry(s.entry.as_str()).or_insert(i);
        }
        m
    });
    m.get(entry).map(|&i| &idx.stages[i])
}

fn stage_kind_str(k: StageKind) -> &'static str {
    match k {
        StageKind::Vertex => "Vertex",
        StageKind::Fragment => "Fragment",
        StageKind::Compute => "Compute",
        _ => "Other",
    }
}

/// Translates and compiles a missing guest shader stage synchronously into a Metal library.
pub unsafe fn lookup_or_translate_dynamic(
    data: u64,
    control: u64,
    stage_index: usize,
) -> Option<&'static CachedStage> {
    if data < 0x1_0000_0000 || !crate::settings::get().graphics.dynamic_shaders {
        return None;
    }
    if let Some(stages) = DYNAMIC_STAGES.get() {
        if let Some(&st) = stages.lock().unwrap().get(&data) {
            return Some(st);
        }
    }

    let magic = *(data as *const u32);
    let stage = if magic == 0x12345679 {
        sass2spv::ShaderStage::Compute
    } else if magic == 0x12345678 {
        let sph_word0 = *((data + 0x30) as *const u32);
        let sph_type = (sph_word0 >> 10) & 0xF;
        sass2spv::ShaderStage::from_sph_type(sph_type).unwrap_or(if stage_index == 1 {
            sass2spv::ShaderStage::Fragment
        } else {
            sass2spv::ShaderStage::Vertex
        })
    } else if stage_index == 1 {
        sass2spv::ShaderStage::Fragment
    } else {
        sass2spv::ShaderStage::Vertex
    };

    let cb1 = if control >= 0x1_0000_0000 && *(control as *const u32) == 0x9876_1234 {
        let ctrl_size = *((control + 0x20) as *const u32) as u64;
        if ctrl_size >= 0x708 {
            let off = *((control + 0x700) as *const u32) as i32;
            if off > 0 && off <= 0x10_0000 {
                off
            } else {
                -1
            }
        } else {
            -1
        }
    } else {
        -1
    };

    // Compute stages: workgroup size and shared memory from the control section, as the cache build reads them (which of
    // +0x754 / +0x760 / +0x710 is shared and which local memory is not pinned down: both get the largest).
    let ctrl_u32 = |off: u64| *((control + off) as *const u32);
    let compute_fields = (stage == sass2spv::ShaderStage::Compute
        && control >= 0x1_0000_0000
        && ctrl_u32(0) == 0x9876_1234
        && ctrl_u32(0x20) >= 0x764)
        .then(|| {
            let mem = (ctrl_u32(0x754).max(ctrl_u32(0x760)).max(ctrl_u32(0x710)) + 15) & !15;
            [ctrl_u32(0x748) as i32, ctrl_u32(0x74c) as i32, ctrl_u32(0x750) as i32, mem as i32]
        });

    let stage_hint = match stage {
        sass2spv::ShaderStage::Compute => 3,
        sass2spv::ShaderStage::Fragment => 1,
        sass2spv::ShaderStage::Vertex => 2,
        _ => 0,
    };

    // Stages near the end of a mapping: a fixed 64 KiB window would read past it.
    let code_slice = std::slice::from_raw_parts(data as *const u8, crate::cpu::crash_handler::readable_len(data as usize, 0x10000));
    let target = sass2spv::Target::Msl(sass2spv::MSL_ENTRY_PLACEHOLDER.into());
    let (msl, info, size) = match sass2spv::translate_stage_to(code_slice, stage_hint, cb1, compute_fields, &target) {
        Ok(p) if !p.msl.is_empty() => {
            let info = p.manifest();
            (p.msl, info, p.size as usize)
        }
        Ok(_) => {
            eprintln!("[dynamic-shader] sass2spv produced no MSL at 0x{data:x}");
            return None;
        }
        Err(e) => {
            eprintln!("[dynamic-shader] sass2spv error at 0x{data:x}: {e}");
            return None;
        }
    };

    let total = if magic == 0x12345679 {
        0x100 + size
    } else if magic == 0x12345678 {
        0x30 + size
    } else {
        size
    };
    let full = fnv1a64(&code_slice[..total.min(code_slice.len())]);

    if let Some(by_hash) = DYNAMIC_STAGES_BY_HASH.get() {
        if let Some(&existing) = by_hash.lock().unwrap().get(&full) {
            DYNAMIC_STAGES
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .unwrap()
                .insert(data, existing);
            return Some(existing);
        }
    }

    let entry = format!("s_{full:016x}");
    let msl_raw = msl.replace(sass2spv::MSL_ENTRY_PLACEHOLDER, &entry);
    let (specialized, attrs, colors, varyings) = super::msl_specialize::specialize(&msl_raw);

    let lib = compile_msl(&specialized, &entry)?;
    super::translated::register_dynamic_library(&entry, lib);
    COMPILED.get_or_init(Default::default).lock().unwrap().insert(entry.clone());

    let cbuffers: Vec<(u32, u32)> = info
        .cbuffers
        .iter()
        .map(|c| (c.binding, c.slot.max(0) as u32))
        .collect();
    let textures: Vec<TexBinding> = info
        .textures
        .iter()
        .map(|t| {
            let ty = t.texture_type.replace(", ", "+").replace(',', "+").replace(' ', "");
            TexBinding {
                binding: t.binding,
                handle_index: t.handle_index.max(0) as u32,
                shadow: ty.contains("Shadow"),
                declared: declared_texture_type(&ty),
                ty,
            }
        })
        .collect();
    let images: Vec<TexBinding> = info
        .images
        .iter()
        .map(|im| {
            let ty = im.texture_type.replace(", ", "+").replace(',', "+").replace(' ', "");
            TexBinding {
                binding: im.binding,
                handle_index: im.handle_index.max(0) as u32,
                shadow: false,
                declared: declared_texture_type(&ty),
                ty,
            }
        })
        .collect();
    let sbuffers: Vec<(u32, u32, u32, u32)> = info
        .sbuffers
        .iter()
        .map(|sb| {
            (
                sb.binding,
                sb.slot.max(0) as u32,
                sb.cb_slot.max(0) as u32,
                sb.cb_offset.max(0) as u32,
            )
        })
        .collect();

    let p = regex::Regex::new(r"const device _\w+& \w+ \[\[buffer\((\d+)\)\]\]").unwrap();
    let mut sb_readonly: Vec<u32> = p
        .captures_iter(&specialized)
        .map(|c| c[1].parse().unwrap())
        .collect();
    sb_readonly.sort_unstable();

    let cbuf_limits = super::msl_specialize::uses_cbuf_limits(&specialized);
    let cbuf_extent: Vec<(u32, u32)> = super::msl_specialize::cbuf_extents(&specialized)
        .into_iter()
        .map(|(b, sz)| (b, sz as u32))
        .collect();

    let stage_kind = match stage {
        sass2spv::ShaderStage::Vertex => StageKind::Vertex,
        sass2spv::ShaderStage::Fragment => StageKind::Fragment,
        sass2spv::ShaderStage::Compute => StageKind::Compute,
        _ => StageKind::Other,
    };

    let cached = Box::leak(Box::new(CachedStage {
        kind: stage_kind,
        entry,
        code_end: total,
        attrs,
        colors,
        varyings,
        cbuffers,
        cbuf_extent,
        textures,
        images,
        sbuffers,
        sb_readonly,
        cbuf_limits,
        tiny_kernel: false,
        tiny_implicit: None,
        tiny_clears: false,
        batch_cbufs: None,
        local_size: compute_fields.map_or([1, 1, 1], |c| [c[0].max(1) as u32, c[1].max(1) as u32, c[2].max(1) as u32]),
        fs_effects: OnceLock::new(),
    }));

    if cached.kind == StageKind::Fragment {
        cached.fs_effects.get_or_init(|| fragment_effects(data));
    }
    if code_slice.len() >= WEAK_LEN {
        save_runtime_stage(fnv1a64(&code_slice[..WEAK_LEN]), full, cached, &specialized);
    }

    let static_ref: &'static CachedStage = &*cached;
    DYNAMIC_STAGES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(data, static_ref);
    DYNAMIC_STAGES_BY_HASH
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(full, static_ref);
    println!(
        "[dynamic-shader] Registered dynamic {} stage 0x{:x} -> {}",
        stage_kind_str(stage_kind),
        data,
        static_ref.entry
    );
    Some(static_ref)
}

/// Whether a fragment stage does more than write colors, from its shader program header (0x30 past the stage data's
/// preamble; bit layout as Ryujinx ShaderHeader.cs): common word 0 bit 15 KillsPixels, bit 16 DoesGlobalStore, bit 26
/// DoesLoadOrStore; PS word 19 bit 0 OmapSampleMask, bit 1 OmapDepth.
unsafe fn fragment_effects(data: u64) -> bool {
    let sph = (data + 0x30) as *const u32;
    let (w0, omap) = (*sph, *sph.add(19));
    w0 & (1 << 15 | 1 << 16 | 1 << 26) != 0 || omap & 3 != 0
}

/// MTLTextureType raw value of a shader texture declaration ("Texture2D+Array", "TextureCube", ...).
pub fn declared_texture_type(ty: &str) -> usize {
    // MTLTextureType: 1D 0, 1DArray 1, 2D 2, 2DArray 3, 2DMultisample 4, Cube 5, CubeArray 6, 3D 7, 2DMultisampleArray 8.
    let norm = ty.replace(", ", "+").replace(',', "+").replace(' ', "");
    let base = norm.split('+').next().unwrap_or("Texture2D");
    let array = norm.contains("Array");
    let ms = norm.contains("Multisample");
    match (base, array, ms) {
        ("Texture1D", false, _) => 0,
        ("Texture1D", true, _) => 1,
        ("Texture3D", ..) => 7,
        ("TextureCube", false, _) => 5,
        ("TextureCube", true, _) => 6,
        (_, false, true) => 4,
        (_, true, true) => 8,
        (_, true, false) => 3,
        _ => 2,
    }
}
