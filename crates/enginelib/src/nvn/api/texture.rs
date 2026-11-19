// NVN textures, texture builders and views: layouts, snapshots and the storage index, texel reads and writes.

use super::*;

// ---------------------------------------------------------------------------
// NVN Texture family. NVNtexture and NVNtextureBuilder are both 0x80 bytes.
// Storage size, alignment and view offsets come from nvn_layout (Tegra block-linear,
// validated against the toolchain's BNTX output): the game memcpy's pre-swizzled
// data to the offsets reported here, so they must be exact. An earlier flat
// 4-bytes-per-pixel estimate asked the allocator for ~700MB for one texture.
#[repr(C)]
pub struct NvnTextureBuilder {
    pub device: usize,
    pub target: i32,
    pub width: i32,
    pub height: i32,
    pub depth: i32,
    pub levels: i32,
    pub format: i32,
    pub flags: i32,
    pub samples: i32,
    pub stride: i64,
    pub pool: usize,
    pub pool_offset: i64,
    /// nvnTextureBuilderSetSwizzle (NVN_TEXTURE_SWIZZLE_*: 0 zero, 1 one, 2..5 r, g, b, a).
    pub swizzle: [u8; 4],
    /// nvnTextureBuilderSetDepthStencilMode (0 depth, 1 stencil); only reported back, not yet used for sampling.
    pub depth_stencil_mode: i32,
    /// nvnTextureBuilderSetPackagedTextureLayout: the texture packager's layout descriptor (NVNpackagedTextureLayout,
    /// 8 bytes) for packaged texture data, and whether one was set.
    pub packaged_layout: [u8; 8],
    pub has_packaged_layout: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NvnTexture {
    pub device: usize,
    pub target: i32,
    pub width: i32,
    pub height: i32,
    pub depth: i32,
    pub levels: i32,
    pub format: i32,
    pub flags: i32,
    pub samples: i32,
    pub stride: i64,
    pub pool: usize,
    pub pool_offset: i64,
    pub storage: usize,
    pub size: usize,
    /// The builder's swizzle; a view's replaces it (see metal::translated::texture_for_binding).
    pub swizzle: [u8; 4],
    pub _reserved: [u8; 44],
}

pub(crate) fn builder_layout_params(b: &NvnTextureBuilder) -> crate::nvn::layout::TextureParams {
    crate::nvn::layout::TextureParams {
        target: b.target,
        width: b.width,
        height: b.height,
        depth: b.depth,
        levels: b.levels,
        format: b.format,
        flags: b.flags,
        samples: b.samples,
        stride: b.stride,
    }
}

pub(crate) fn texture_layout_params(t: &NvnTexture) -> crate::nvn::layout::TextureParams {
    crate::nvn::layout::TextureParams {
        target: t.target,
        width: t.width,
        height: t.height,
        depth: t.depth,
        levels: t.levels,
        format: t.format,
        flags: t.flags,
        samples: t.samples,
        stride: t.stride,
    }
}

pub(crate) unsafe fn texture_builder_storage_size(b: *const NvnTextureBuilder) -> usize {
    if b.is_null() {
        return 0;
    }
    crate::nvn::layout::storage_size(&builder_layout_params(&*b))
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetPackagedTextureLayout(
    b: *mut NvnTextureBuilder,
    layout: *const [u8; 8],
) {
    record_hle_call("hle_nvnTextureBuilderSetPackagedTextureLayout");
    if b.is_null() {
        return;
    }
    if layout.is_null() {
        (*b).has_packaged_layout = 0;
    } else {
        (*b).packaged_layout = *layout;
        (*b).has_packaged_layout = 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetDefaults(b: *mut NvnTextureBuilder) {
    record_hle_call("hle_nvnTextureBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
        (*b).levels = 1;
        (*b).depth = 1;
        (*b).samples = 1;
        (*b).swizzle = [2, 3, 4, 5];
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetDevice(b: *mut NvnTextureBuilder, device: usize) {
    record_hle_call("hle_nvnTextureBuilderSetDevice");
    if !b.is_null() {
        (*b).device = device;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetTarget(b: *mut NvnTextureBuilder, target: i32) {
    record_hle_call("hle_nvnTextureBuilderSetTarget");
    if !b.is_null() {
        (*b).target = target;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetWidth(b: *mut NvnTextureBuilder, width: i32) {
    record_hle_call("hle_nvnTextureBuilderSetWidth");
    if !b.is_null() {
        (*b).width = width;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetHeight(b: *mut NvnTextureBuilder, height: i32) {
    record_hle_call("hle_nvnTextureBuilderSetHeight");
    if !b.is_null() {
        (*b).height = height;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetDepth(b: *mut NvnTextureBuilder, depth: i32) {
    record_hle_call("hle_nvnTextureBuilderSetDepth");
    if !b.is_null() {
        (*b).depth = depth;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetLevels(b: *mut NvnTextureBuilder, levels: i32) {
    record_hle_call("hle_nvnTextureBuilderSetLevels");
    if !b.is_null() {
        (*b).levels = levels;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetFormat(b: *mut NvnTextureBuilder, format: i32) {
    record_hle_call("hle_nvnTextureBuilderSetFormat");
    if !b.is_null() {
        (*b).format = format;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetFlags(b: *mut NvnTextureBuilder, flags: i32) {
    record_hle_call("hle_nvnTextureBuilderSetFlags");
    if !b.is_null() {
        (*b).flags = flags;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSamples(b: *mut NvnTextureBuilder, samples: i32) {
    record_hle_call("hle_nvnTextureBuilderSetSamples");
    if !b.is_null() {
        (*b).samples = samples;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetDepthStencilMode(
    _b: *mut NvnTextureBuilder,
    _mode: i32,
) {
    if !_b.is_null() {
        (*_b).depth_stencil_mode = _mode;
    }
    record_hle_call("hle_nvnTextureBuilderSetDepthStencilMode");
    // Selects which aspect (depth vs stencil) a combined depth/stencil
    // texture samples as -- affects shader reads, not storage layout, so
    // irrelevant until real Metal translation exists (same as SetSwizzle).
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSwizzle(
    b: *mut NvnTextureBuilder,
    r: i32,
    g: i32,
    bl: i32,
    a: i32,
) {
    record_hle_call("hle_nvnTextureBuilderSetSwizzle");
    if !b.is_null() {
        (*b).swizzle = [r as u8, g as u8, bl as u8, a as u8];
    }
    log_swizzle("SetSwizzle", -1, [r as u8, g as u8, bl as u8, a as u8]);
    if [r, g, bl, a].iter().any(|&c| !(0..=5).contains(&c))
        && crate::settings::get().log.log_swizzle
    {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 3 {
            let fp = crate::guest_reg!(x29);
            println!("[swizzle] out-of-range SetSwizzle({}, {}, {}, {}) raw args {:#x} {:#x} {:#x} {:#x} callers {}", r, g, bl, a, r as u32, g as u32, bl as u32, a as u32, guest_chain(fp));
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetStride(b: *mut NvnTextureBuilder, stride: i64) {
    record_hle_call("hle_nvnTextureBuilderSetStride");
    if !b.is_null() {
        (*b).stride = stride;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetStorage(
    b: *mut NvnTextureBuilder,
    pool: usize,
    offset: i64,
) {
    record_hle_call("hle_nvnTextureBuilderSetStorage");
    if !b.is_null() {
        (*b).pool = pool;
        (*b).pool_offset = offset;
    }
}

#[no_mangle]
/// nvnTexture(Builder)GetZCullStorageSize: memory for nvnCommandBufferSave/RestoreZCullData. ZCull (the hardware's
/// coarse depth-culling cache) has no Metal counterpart and those commands are not executed, so nothing is needed.
pub unsafe extern "C" fn hle_nvnZCullStorageSize(_obj: *const u8) -> usize {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetStorageSize(b: *const NvnTextureBuilder) -> usize {
    record_hle_call("hle_nvnTextureBuilderGetStorageSize");
    texture_builder_storage_size(b)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetStorageAlignment(
    b: *const NvnTextureBuilder,
) -> usize {
    record_hle_call("hle_nvnTextureBuilderGetStorageAlignment");
    crate::nvn::layout::storage_alignment(if b.is_null() { 0 } else { (*b).flags })
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetStorageClass() -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetStorageClass");
    0 // NVN_STORAGE_CLASS_POOL -- backed by an explicit memory pool, not device-internal storage
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureInitialize(
    tex: *mut NvnTexture,
    builder: *const NvnTextureBuilder,
) -> u8 {
    let lr = crate::guest_reg!(x30);
    let base = crate::guest::loader::main_base();
    let rel_lr = if lr >= base && base != 0 {
        lr - base
    } else {
        lr
    };
    // --log-tex-init: every texture initialization (with size and storage); off by default (TotK re-initializes
    // texture objects constantly).
    static LOG_INIT: OnceLock<bool> = OnceLock::new();
    let log_init = *LOG_INIT.get_or_init(|| crate::settings::get().log.log_tex_init);
    record_hle_call("hle_nvnTextureInitialize");
    if tex.is_null() || builder.is_null() {
        return 0;
    }
    let fp = crate::guest_reg!(x29);
    crate::profile::texture_initialized((*builder).width.max(0) as u32, (*builder).height.max(0) as u32, fp);
    if log_init {
        let b = &*builder;
        // Up to eight more callers through the guest frame pointers (main-relative where they are in main).
        let mut chain = String::new();
        let mut f = fp;
        for _ in 0..8 {
            if f == 0 || !crate::profile::readable(f, 16) {
                break;
            }
            let ret = *((f + 8) as *const usize);
            chain += &format!(" {:#x}", if ret >= base && base != 0 { ret - base } else { ret });
            let next = *(f as *const usize);
            if next <= f {
                break;
            }
            f = next;
        }
        println!("[tex-init] tex={:p} {}x{}x{} lv{} fmt {} flags {:#x} target {} pool {:#x}+{:#x} size {:#x} layout {} caller=base+{:#x}{} thread {}", tex, b.width, b.height, b.depth, b.levels, b.format, b.flags, b.target, b.pool, b.pool_offset, texture_builder_storage_size(builder),
            if b.has_packaged_layout != 0 { format!("{:02x?}", b.packaged_layout) } else { "-".into() }, rel_lr, chain, std::thread::current().name().unwrap_or("?"));
    }
    let storage = if (*builder).pool != 0 {
        let pool = &*((*builder).pool as *const NvnMemoryPool);
        translate_virtual(pool.memory + (*builder).pool_offset as usize)
    } else {
        0
    };
    std::ptr::write(
        tex,
        NvnTexture {
            device: (*builder).device,
            target: (*builder).target,
            width: (*builder).width,
            height: (*builder).height,
            depth: (*builder).depth.max(1),
            levels: (*builder).levels.max(1),
            format: (*builder).format,
            flags: (*builder).flags,
            samples: (*builder).samples.max(1),
            stride: (*builder).stride,
            pool: (*builder).pool,
            pool_offset: (*builder).pool_offset,
            storage,
            size: texture_builder_storage_size(builder),
            swizzle: if (*builder).swizzle == [0; 4] {
                [2, 3, 4, 5]
            } else {
                (*builder).swizzle
            },
            _reserved: [0; 44],
        },
    );
    log_swizzle("Initialize", (*tex).format, (*tex).swizzle);
    snapshot_texture(tex as usize, std::ptr::read(tex));
    // A pending upload belonged to the object's previous texture (the game wrote / flushed texels into that one):
    // TotK re-initializes texture objects for other textures constantly, render targets included, and a mark carried
    // over made the next resolve upload guest memory over a render target the GPU had just drawn (one-frame magenta
    // tiles over an unlit scene, 2026-10-13). The new texture's initial contents come from its creation path.
    DIRTY_TEXTURES.lock().unwrap().remove(&(tex as usize));
    1
}

/// Debug: --log-swizzle prints each distinct (call, format, swizzle) once.
pub(crate) fn log_swizzle(what: &str, format: i32, sw: [u8; 4]) {
    static ON: OnceLock<bool> = OnceLock::new();
    if !*ON.get_or_init(|| crate::settings::get().log.log_swizzle) {
        return;
    }
    static SEEN: StdMutex<Vec<(String, i32, [u8; 4])>> = StdMutex::new(Vec::new());
    let k = (what.to_string(), format, sw);
    let mut seen = SEEN.lock().unwrap();
    if !seen.contains(&k) {
        println!("[swizzle] {} format {} swizzle {:?}", what, format, sw);
        seen.push(k);
    }
}

// What the executor needs of an NVNtexture, captured when the game initializes it. The game may finalize or
// re-initialize the object after recording a command that names it (real NVN copies what it needs at record time),
// so the executor must not read the live struct. A new Initialize at the same address bumps the generation, which
// makes the executor drop its Metal texture and build a new one.
pub(crate) static TEX_SNAPSHOTS: std::sync::Mutex<Option<rustc_hash::FxHashMap<usize, (u64, NvnTexture)>>> =
    std::sync::Mutex::new(None);

pub(crate) static TEX_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Bumped after every change to TEX_SNAPSHOTS or POOL_TEXTURES: a texture binding resolved while it had some value still
/// resolves the same way while it keeps it (metal::translated::bound_texture's fast path). Read before the lookups it
/// guards, bumped after the change: a race only costs a slow-path check.
pub(crate) static TEX_TABLE_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn tex_table_epoch() -> u64 {
    TEX_TABLE_EPOCH.load(std::sync::atomic::Ordering::Acquire)
}

pub(crate) fn bump_tex_table_epoch() {
    TEX_TABLE_EPOCH.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

pub(crate) fn snapshot_texture(addr: usize, t: NvnTexture) {
    // The state being replaced goes into the history before the new one is published: a recorded command resolved in
    // between (another thread encodes while the game re-initializes the object) found neither and took the object's new
    // texture (BotW's scene composite drew with the half-size depth target its object had just become: the scene shrunk
    // into a corner for a frame, ~10 times a minute in headless runs).
    let prev = texture_snapshot(addr);
    if let Some((og, ot)) = prev {
        remember_snapshot(og, ot);
    }
    let gen = TEX_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let old = TEX_SNAPSHOTS
        .lock()
        .unwrap()
        .get_or_insert_with(Default::default)
        .insert(addr, (gen, t));
    // Another thread replaced it meanwhile: that state too.
    if let Some((og, ot)) = old {
        if prev.map(|(g, _)| g) != Some(og) {
            remember_snapshot(og, ot);
        }
    }
    bump_tex_table_epoch();
    let mut idx = TEX_BY_STORAGE.lock().unwrap();
    let idx = idx.get_or_insert_with(Default::default);
    if let Some((_, o)) = old {
        unindex_texture(idx, addr, o.storage);
        log_index_change(o.storage, texture_span(&o));
    }
    if t.storage > 0x10000 {
        idx.entry(t.storage).or_default().push(addr);
        log_index_change(t.storage, texture_span(&t));
        // Bound for the alias search (a container starts at most its own size below a texture it contains).
        let span = texture_span(&t);
        MAX_TEXTURE_SPAN.fetch_max(span, std::sync::atomic::Ordering::Relaxed);
        if span > SMALL_TEXTURE_SPAN {
            TEX_LARGE_BY_STORAGE.lock().unwrap().get_or_insert_with(Default::default).entry(t.storage).or_default().push(addr);
        }
    }
}

/// Recent changes of the live-texture index: (sequence, start, end) of the memory each indexed or unindexed texture
/// covers. Backends caching alias resolutions drop the entries these touch (index_changes_since).
pub(crate) static INDEX_CHANGES: std::sync::Mutex<(u64, std::collections::VecDeque<(u64, usize, usize)>)> =
    std::sync::Mutex::new((0, std::collections::VecDeque::new()));

pub(crate) const INDEX_CHANGES_KEPT: usize = 1 << 14;

pub(crate) fn log_index_change(storage: usize, span: usize) {
    if storage <= 0x10000 {
        return;
    }
    let mut g = INDEX_CHANGES.lock().unwrap();
    g.0 += 1;
    let seq = g.0;
    if g.1.len() == INDEX_CHANGES_KEPT {
        g.1.pop_front();
    }
    g.1.push_back((seq, storage, storage + span.max(1)));
}

/// The live-texture index changes after sequence `since` as memory ranges, and the current sequence; None when some
/// of them are no longer kept (everything must be considered changed).
pub fn index_changes_since(since: u64, out: &mut Vec<(usize, usize)>) -> (u64, bool) {
    let g = INDEX_CHANGES.lock().unwrap();
    let complete = g.1.front().is_none_or(|f| f.0 <= since + 1) || g.0 == since;
    out.extend(g.1.iter().filter(|c| c.0 > since).map(|c| (c.1, c.2)));
    (g.0, complete)
}

/// Memory a texture covers: its declared size or its layout's, the larger.
pub(crate) fn texture_span(t: &NvnTexture) -> usize {
    (t.size as u64).max(crate::nvn::layout::compute_layout(&texture_layout_params(t)).total) as usize
}

/// textures_reaching scans TEX_BY_STORAGE this far below an address; larger textures are also indexed in
/// TEX_LARGE_BY_STORAGE (few), scanned over the whole MAX_TEXTURE_SPAN.
pub(crate) const SMALL_TEXTURE_SPAN: usize = 4 << 20;

pub(crate) static TEX_LARGE_BY_STORAGE: std::sync::Mutex<Option<std::collections::BTreeMap<usize, Vec<usize>>>> =
    std::sync::Mutex::new(None);

/// Snapshots replaced at their address, by generation (bounded): commands recorded while an object described one
/// texture still name that texture after the game re-initializes or reuses the object (resolve_texture_ref).
pub(crate) const SNAPSHOT_HISTORY: usize = 1 << 16;

/// Generations are keyed by their low 32 bits, as commands record them.
pub(crate) struct SnapshotHistory {
    pub(crate) by_gen: rustc_hash::FxHashMap<u32, NvnTexture>,
    pub(crate) order: std::collections::VecDeque<u32>,
    /// Stable objects standing in for a replaced snapshot, by generation (their addresses are executor texture keys).
    pub(crate) copies: rustc_hash::FxHashMap<u32, Box<NvnTexture>>,
}

pub(crate) static TEX_HISTORY: std::sync::Mutex<Option<SnapshotHistory>> = std::sync::Mutex::new(None);

pub(crate) fn remember_snapshot(gen: u64, t: NvnTexture) {
    let mut h = TEX_HISTORY.lock().unwrap();
    let h = h.get_or_insert_with(|| SnapshotHistory {
        by_gen: Default::default(),
        order: Default::default(),
        copies: Default::default(),
    });
    h.by_gen.insert(gen as u32, t);
    h.order.push_back(gen as u32);
    if h.order.len() > SNAPSHOT_HISTORY {
        if let Some(g) = h.order.pop_front() {
            h.by_gen.remove(&g);
            if let Some(c) = h.copies.remove(&g) {
                let addr = &*c as *const NvnTexture as usize;
                if let Some(m) = TEX_SNAPSHOTS.lock().unwrap().as_mut() {
                    m.remove(&addr);
                }
            }
        }
    }
}

/// The snapshot generation a command naming `addr` records (0: none).
pub fn texture_ref_gen(addr: usize) -> u32 {
    texture_snapshot(addr).map_or(0, |(g, _)| g as u32)
}

/// The texture object a recorded reference (`addr` as captured with generation `gen`) names now: `addr` while it still
/// describes that texture, else a stable copy of the recorded description. Super Mario Odyssey records command lists once
/// and calls them every frame while the NVNtexture structs they named are re-initialized for other textures (its mip
/// chain and volume passes then targeted levels the new textures do not have).
pub fn resolve_texture_ref(addr: usize, gen: u32) -> usize {
    if addr == 0 || gen == 0 || texture_snapshot(addr).map_or(true, |(g, _)| g as u32 == gen) {
        return addr;
    }
    let mut h = TEX_HISTORY.lock().unwrap();
    let Some(h) = h.as_mut() else { return addr };
    let Some(&state) = h.by_gen.get(&gen) else { return addr };
    if let Some(c) = h.copies.get(&gen) {
        return &**c as *const NvnTexture as usize;
    }
    let b = Box::new(state);
    let copy = &*b as *const NvnTexture as usize;
    let g = TEX_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    TEX_SNAPSHOTS.lock().unwrap().get_or_insert_with(Default::default).insert(copy, (g, state));
    h.copies.insert(gen, b);
    bump_tex_table_epoch();
    copy
}

/// Largest memory size (declared or by layout) of any texture indexed so far.
pub(crate) static MAX_TEXTURE_SPAN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn max_texture_span() -> usize {
    MAX_TEXTURE_SPAN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Live (initialized, not finalized) NVNtextures by storage address, for resolving aliases: several texture
/// objects over the same GPU memory (mip-level or layer views, format reinterpretations) must share one
/// executor texture or rendering through one is invisible through the others.
pub(crate) static TEX_BY_STORAGE: std::sync::Mutex<Option<std::collections::BTreeMap<usize, Vec<usize>>>> =
    std::sync::Mutex::new(None);

pub(crate) fn unindex_texture(
    idx: &mut std::collections::BTreeMap<usize, Vec<usize>>,
    addr: usize,
    storage: usize,
) {
    for idx in [Some(idx), TEX_LARGE_BY_STORAGE.lock().unwrap().as_mut()].into_iter().flatten() {
        if let Some(v) = idx.get_mut(&storage) {
            v.retain(|a| *a != addr);
            if v.is_empty() {
                idx.remove(&storage);
            }
        }
    }
}

/// Live textures whose storage starts within `span` bytes at or below `storage`: (object address, generation, desc).
pub fn textures_near_storage(storage: usize, span: usize) -> Vec<(usize, u64, NvnTexture)> {
    let addrs: Vec<usize> = {
        let idx = TEX_BY_STORAGE.lock().unwrap();
        let Some(idx) = idx.as_ref() else {
            return Vec::new();
        };
        idx.range(storage.saturating_sub(span)..=storage)
            .flat_map(|(_, v)| v.iter().copied())
            .collect()
    };
    let snaps = TEX_SNAPSHOTS.lock().unwrap();
    let Some(snaps) = snaps.as_ref() else {
        return Vec::new();
    };
    addrs
        .into_iter()
        .filter_map(|a| snaps.get(&a).map(|(g, t)| (a, *g, *t)))
        .collect()
}

/// textures_near_storage without the textures whose declared size ends at or below `storage`.
pub fn textures_reaching(storage: usize, span: usize) -> Vec<(usize, u64, NvnTexture)> {
    let addrs: Vec<usize> = {
        let idx = TEX_BY_STORAGE.lock().unwrap();
        let Some(idx) = idx.as_ref() else {
            return Vec::new();
        };
        let mut a: Vec<usize> = idx
            .range(storage.saturating_sub(span.min(SMALL_TEXTURE_SPAN))..=storage)
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        if span > SMALL_TEXTURE_SPAN {
            if let Some(large) = TEX_LARGE_BY_STORAGE.lock().unwrap().as_ref() {
                let below = storage.saturating_sub(SMALL_TEXTURE_SPAN);
                a.extend(large.range(storage.saturating_sub(span)..below).flat_map(|(_, v)| v.iter().copied()));
            }
        }
        a
    };
    let snaps = TEX_SNAPSHOTS.lock().unwrap();
    let Some(snaps) = snaps.as_ref() else {
        return Vec::new();
    };
    addrs
        .iter()
        .filter_map(|a| snaps.get(a).map(|(g, t)| (*a, *g, t)))
        .filter(|(_, _, t)| t.size == 0 || t.storage + t.size > storage)
        .map(|(a, g, t)| (a, g, *t))
        .collect()
}

/// Called when a command naming the NVNtexture at `addr` is recorded. Real NVN reads the texture then; the executor
/// reads its snapshot later. Games copy initialized NVNtexture structs (they hold their descriptor inline) and bind the
/// copies, which never went through nvnTextureInitialize: the copy is snapshotted here, before the game frees or reuses
/// its memory (Super Mario Odyssey's opening demo: a 1600x900 target read back as garbage dimensions, gigabytes of
/// layout). A new copy at an address that held another texture gets a new generation.
pub fn capture_texture(addr: usize) {
    if addr < 0x10000 {
        return;
    }
    let live = unsafe { std::ptr::read(addr as *const NvnTexture) };
    let same = |a: &NvnTexture, b: &NvnTexture| {
        (a.target, a.width, a.height, a.depth, a.levels, a.format, a.flags, a.samples, a.stride, a.storage, a.size, a.swizzle)
            == (b.target, b.width, b.height, b.depth, b.levels, b.format, b.flags, b.samples, b.stride, b.storage, b.size, b.swizzle)
    };
    match texture_snapshot(addr) {
        Some((_, snap)) if same(&snap, &live) => {}
        // A finalized object (zeroed) keeps its snapshot: a command naming it is the game's business.
        _ if live.width <= 0 || live.height <= 0 => {}
        _ => snapshot_texture(addr, live),
    }
}

/// (generation, texture description) of the NVNtexture most recently initialized at `addr`.
pub fn texture_snapshot(addr: usize) -> Option<(u64, NvnTexture)> {
    TEX_SNAPSHOTS.lock().unwrap().as_ref()?.get(&addr).copied()
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureFinalize(tex: *mut NvnTexture) {
    record_hle_call("hle_nvnTextureFinalize");
    // The snapshot stays (recorded commands may still name this object), but it no longer anchors aliases. It gets a
    // new generation: the game may free the object while pool slots registered from it stay in use (NVN copied the
    // descriptor into the pool), and those slots must resolve to their registered state, not to whatever reuses the
    // memory (Super Mario Odyssey's opening demo read heap data as texture dimensions and allocated gigabytes).
    if let Some((og, t)) = texture_snapshot(tex as usize) {
        if let Some(idx) = TEX_BY_STORAGE.lock().unwrap().as_mut() {
            unindex_texture(idx, tex as usize, t.storage);
            log_index_change(t.storage, texture_span(&t));
        }
        // Commands recorded before the finalize name the old generation (BotW initializes one object for each of its
        // water maps, records a pass into it and finalizes it again: without this every pass drew into the last map).
        // Remembered before the new generation is published (snapshot_texture).
        remember_snapshot(og, t);
        let gen = TEX_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(s) = TEX_SNAPSHOTS.lock().unwrap().as_mut() {
            s.insert(tex as usize, (gen, t));
        }
        bump_tex_table_epoch();
    }
    if !tex.is_null() {
        std::ptr::write_bytes(tex, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetWidth(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetWidth");
    if tex.is_null() {
        0
    } else {
        (*tex).width
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetHeight(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetHeight");
    if tex.is_null() {
        0
    } else {
        (*tex).height
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetTextureAddress(tex: *const NvnTexture) -> u64 {
    record_hle_call("hle_nvnTextureGetTextureAddress");
    if tex.is_null() {
        0
    } else {
        (*tex).storage as u64
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetDepth(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetDepth");
    if tex.is_null() {
        0
    } else {
        (*tex).depth
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetLevels(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetLevels");
    if tex.is_null() {
        0
    } else {
        (*tex).levels
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetFormat(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetFormat");
    if tex.is_null() {
        0
    } else {
        (*tex).format
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetFlags(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetFlags");
    if tex.is_null() {
        0
    } else {
        (*tex).flags
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetSamples(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetSamples");
    if tex.is_null() {
        0
    } else {
        (*tex).samples
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetTarget(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetTarget");
    if tex.is_null() {
        0
    } else {
        (*tex).target
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetStride(tex: *const NvnTexture) -> i64 {
    record_hle_call("hle_nvnTextureGetStride");
    if tex.is_null() {
        0
    } else {
        (*tex).stride
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetStorageSize(tex: *const NvnTexture) -> usize {
    record_hle_call("hle_nvnTextureGetStorageSize");
    if tex.is_null() {
        0
    } else {
        (*tex).size
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetStorageClass(_tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetStorageClass");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetMemoryPool(tex: *const NvnTexture) -> usize {
    record_hle_call("hle_nvnTextureGetMemoryPool");
    if tex.is_null() {
        0
    } else {
        (*tex).pool
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetMemoryOffset(tex: *const NvnTexture) -> i64 {
    record_hle_call("hle_nvnTextureGetMemoryOffset");
    if tex.is_null() {
        0
    } else {
        (*tex).pool_offset
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetViewOffset(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
) -> isize {
    record_hle_call("hle_nvnTextureGetViewOffset");
    if tex.is_null() {
        return 0;
    }
    let layout = crate::nvn::layout::compute_layout(&texture_layout_params(&*tex));
    let (lvl, layer) = if view.is_null() {
        (0, 0)
    } else {
        ((*view).min_level as u32, (*view).min_layer as u32)
    };
    crate::nvn::layout::view_offset(&layout, lvl, layer) as isize
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetDebugID(_tex: *const NvnTexture) -> u32 {
    record_hle_call("hle_nvnTextureGetDebugID");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetSwizzle(
    tex: *const NvnTexture,
    r: *mut i32,
    g: *mut i32,
    b: *mut i32,
    a: *mut i32,
) {
    record_hle_call("hle_nvnTextureGetSwizzle");
    // Four separate outputs. Writing all four through `r` as an array left g/b/a holding stale stack bytes:
    // TotK copies a texture's swizzle into a new texture this way, and the garbage (e.g. B = 156, G = ONE) turned
    // albedo textures cyan in Hateno.
    let sw = if tex.is_null() {
        [2, 3, 4, 5]
    } else {
        (*tex).swizzle
    };
    for (p, v) in [r, g, b, a].into_iter().zip(sw) {
        if !p.is_null() {
            *p = v as i32;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetDepthStencilMode(_tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetDepthStencilMode");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetRawStorageClass(_tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetRawStorageClass");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetSparseTileLayout(
    _tex: *const NvnTexture,
    _layout: *mut (),
) -> u8 {
    record_hle_call("hle_nvnTextureGetSparseTileLayout");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureSetDebugLabel(_tex: *mut NvnTexture, _label: *const c_char) {
    record_hle_call("hle_nvnTextureSetDebugLabel");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureCompare(a: *const NvnTexture, b: *const NvnTexture) -> u8 {
    record_hle_call("hle_nvnTextureCompare");
    // NVN compares what the two objects describe, not where they are: BotW rebuilds a texture into a new object and keeps
    // its texture pool slot only when the two compare equal (Savage Wind #16: comparing addresses freed the slot each
    // time, and cuttable tree trunks then drew whatever texture took it).
    if a == b {
        return 1;
    }
    if a.is_null() || b.is_null() {
        return 0;
    }
    let (a, b) = (&*a, &*b);
    (a.target == b.target
        && a.format == b.format
        && a.flags == b.flags
        && a.width == b.width
        && a.height == b.height
        && a.depth == b.depth
        && a.levels == b.levels
        && a.samples == b.samples
        && a.swizzle == b.swizzle
        && a.stride == b.stride
        && a.storage == b.storage
        && a.pool == b.pool
        && a.pool_offset == b.pool_offset) as u8
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSize1D(b: *mut NvnTextureBuilder, width: i32) {
    record_hle_call("hle_nvnTextureBuilderSetSize1D");
    if !b.is_null() {
        (*b).width = width;
        (*b).height = 1;
        (*b).depth = 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSize2D(
    b: *mut NvnTextureBuilder,
    width: i32,
    height: i32,
) {
    record_hle_call("hle_nvnTextureBuilderSetSize2D");
    if !b.is_null() {
        (*b).width = width;
        (*b).height = height;
        (*b).depth = 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSize3D(
    b: *mut NvnTextureBuilder,
    width: i32,
    height: i32,
    depth: i32,
) {
    record_hle_call("hle_nvnTextureBuilderSetSize3D");
    if !b.is_null() {
        (*b).width = width;
        (*b).height = height;
        (*b).depth = depth;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetDevice(b: *const NvnTextureBuilder) -> usize {
    record_hle_call("hle_nvnTextureBuilderGetDevice");
    if b.is_null() {
        0
    } else {
        (*b).device
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetTarget(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetTarget");
    if b.is_null() {
        0
    } else {
        (*b).target
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetWidth(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetWidth");
    if b.is_null() {
        0
    } else {
        (*b).width
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetHeight(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetHeight");
    if b.is_null() {
        0
    } else {
        (*b).height
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetDepth(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetDepth");
    if b.is_null() {
        0
    } else {
        (*b).depth
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetLevels(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetLevels");
    if b.is_null() {
        0
    } else {
        (*b).levels
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetFormat(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetFormat");
    if b.is_null() {
        0
    } else {
        (*b).format
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetFlags(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetFlags");
    if b.is_null() {
        0
    } else {
        (*b).flags
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetSamples(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetSamples");
    if b.is_null() {
        0
    } else {
        (*b).samples
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetStride(b: *const NvnTextureBuilder) -> i64 {
    record_hle_call("hle_nvnTextureBuilderGetStride");
    if b.is_null() {
        0
    } else {
        (*b).stride
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetMemoryPool(b: *const NvnTextureBuilder) -> usize {
    record_hle_call("hle_nvnTextureBuilderGetMemoryPool");
    if b.is_null() {
        0
    } else {
        (*b).pool
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetMemoryOffset(b: *const NvnTextureBuilder) -> i64 {
    record_hle_call("hle_nvnTextureBuilderGetMemoryOffset");
    if b.is_null() {
        0
    } else {
        (*b).pool_offset
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetViewOffset(
    b: *const NvnTextureBuilder,
    view: *const NvnTextureView,
) -> isize {
    record_hle_call("hle_nvnTextureBuilderGetViewOffset");
    if b.is_null() {
        return 0;
    }
    let layout = crate::nvn::layout::compute_layout(&builder_layout_params(&*b));
    let (lvl, layer) = if view.is_null() {
        (0, 0)
    } else {
        ((*view).min_level as u32, (*view).min_layer as u32)
    };
    crate::nvn::layout::view_offset(&layout, lvl, layer) as isize
}

// ---------------------------------------------------------------------------
// NVN Texture View family. NVNtextureView must be 0x10 bytes (or smaller): the game
// builds views in a stack slot directly followed by a live container (count / heap /
// array pointer at view+0x10, initialised *before* SetDefaults/SetLevels/SetLayers run
// in the per-subresource loop at guest+0x86cc60). Earlier 0x28/0x20-byte layouts
// clobbered that container and crashed its release at guest+0x18B7B38.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvnTextureView {
    pub min_level: u16,
    pub num_levels: u16,
    pub min_layer: u16,
    pub num_layers: u16,
    pub format: u16,
    pub target: u8,
    pub depth_stencil_mode: u8,
    pub swizzle: [u8; 4],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetDefaults(view: *mut NvnTextureView) {
    record_hle_call("hle_nvnTextureViewSetDefaults");
    if !view.is_null() {
        std::ptr::write_bytes(view, 0, 1);
        (*view).swizzle = [2, 3, 4, 5];
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetLevels(
    view: *mut NvnTextureView,
    min_level: i32,
    num_levels: i32,
) {
    record_hle_call("hle_nvnTextureViewSetLevels");
    if !view.is_null() {
        (*view).min_level = min_level as u16;
        (*view).num_levels = num_levels as u16;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetLayers(
    view: *mut NvnTextureView,
    min_layer: i32,
    num_layers: i32,
) {
    record_hle_call("hle_nvnTextureViewSetLayers");
    if !view.is_null() {
        (*view).min_layer = min_layer as u16;
        (*view).num_layers = num_layers as u16;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetFormat(view: *mut NvnTextureView, format: i32) {
    record_hle_call("hle_nvnTextureViewSetFormat");
    if !view.is_null() {
        (*view).format = format as u16;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetSwizzle(
    view: *mut NvnTextureView,
    r: i32,
    g: i32,
    b: i32,
    a: i32,
) {
    record_hle_call("hle_nvnTextureViewSetSwizzle");
    if !view.is_null() {
        (*view).swizzle = [r as u8, g as u8, b as u8, a as u8];
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetDepthStencilMode(
    view: *mut NvnTextureView,
    mode: i32,
) {
    record_hle_call("hle_nvnTextureViewSetDepthStencilMode");
    if !view.is_null() {
        (*view).depth_stencil_mode = mode as u8;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetTarget(view: *mut NvnTextureView, target: i32) {
    record_hle_call("hle_nvnTextureViewSetTarget");
    if !view.is_null() {
        (*view).target = target as u8;
    }
}

// The texture view getters return whether the view specifies the attribute and write its value (the default when it
// does not) through their pointers. The view's 16 bytes leave no room to record which attributes were set, so target,
// depth/stencil mode and swizzle count as specified when they differ from the defaults.
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetLevels(
    view: *const NvnTextureView,
    min_level: *mut i32,
    num_levels: *mut i32,
) -> bool {
    record_hle_call("hle_nvnTextureViewGetLevels");
    let (min, num) = view.as_ref().map_or((0, 0), |v| (v.min_level as i32, v.num_levels as i32));
    if !min_level.is_null() {
        *min_level = min;
    }
    if !num_levels.is_null() {
        *num_levels = num;
    }
    num != 0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetLayers(
    view: *const NvnTextureView,
    min_layer: *mut i32,
    num_layers: *mut i32,
) -> bool {
    record_hle_call("hle_nvnTextureViewGetLayers");
    let (min, num) = view.as_ref().map_or((0, 0), |v| (v.min_layer as i32, v.num_layers as i32));
    if !min_layer.is_null() {
        *min_layer = min;
    }
    if !num_layers.is_null() {
        *num_layers = num;
    }
    num != 0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetFormat(view: *const NvnTextureView, format: *mut i32) -> bool {
    record_hle_call("hle_nvnTextureViewGetFormat");
    let f = view.as_ref().map_or(0, |v| v.format as i32); // 0: NVN_FORMAT_NONE
    if !format.is_null() {
        *format = f;
    }
    f != 0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetSwizzle(
    view: *const NvnTextureView,
    r: *mut i32,
    g: *mut i32,
    b: *mut i32,
    a: *mut i32,
) -> bool {
    record_hle_call("hle_nvnTextureViewGetSwizzle");
    // SetDefaults stores the identity swizzle (R, G, B, A); a view never given defaults holds ZERO.
    const IDENTITY: [u8; 4] = [2, 3, 4, 5];
    let sw = view.as_ref().map_or([0; 4], |v| v.swizzle);
    for (p, v) in [r, g, b, a].into_iter().zip(sw) {
        if !p.is_null() {
            *p = v as i32;
        }
    }
    sw != IDENTITY && sw != [0; 4]
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetDepthStencilMode(view: *const NvnTextureView, mode: *mut i32) -> bool {
    record_hle_call("hle_nvnTextureViewGetDepthStencilMode");
    let m = view.as_ref().map_or(0, |v| v.depth_stencil_mode as i32); // 0: DEPTH
    if !mode.is_null() {
        *mode = m;
    }
    m != 0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetTarget(view: *const NvnTextureView, target: *mut i32) -> bool {
    record_hle_call("hle_nvnTextureViewGetTarget");
    let t = view.as_ref().map_or(0, |v| v.target as i32); // 0: TARGET_1D
    if !target.is_null() {
        *target = t;
    }
    t != 0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewCompare(
    a: *const NvnTextureView,
    b: *const NvnTextureView,
) -> u8 {
    record_hle_call("hle_nvnTextureViewCompare");
    if a.is_null() || b.is_null() {
        return (a == b) as u8;
    }
    ((*a).target == (*b).target
        && (*a).format == (*b).format
        && (*a).min_level == (*b).min_level
        && (*a).num_levels == (*b).num_levels
        && (*a).min_layer == (*b).min_layer
        && (*a).num_layers == (*b).num_layers
        && (*a).swizzle == (*b).swizzle
        && (*a).depth_stencil_mode == (*b).depth_stencil_mode) as u8
}

// ── CPU texel access (nvnTextureWriteTexels & co) ───────────────────────────────────────────
// NVNcopyRegion: { xoffset, yoffset, zoffset, width, height, depth } (6 ints). Data is linear:
// tightly packed rows of `width` texels (blocks for compressed formats), slices back to back.
#[repr(C)]
pub struct NvnCopyRegion {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub w: i32,
    pub h: i32,
    pub d: i32,
}

// Textures the guest wrote texels into after the executor may have created their Metal copy; the executor
// re-uploads them from guest storage on next use (see metal/mod.rs, `texture`).
/// A set: texture() asks for every texture it resolves, and entries of textures never resolved again stay (the Vec this
/// was made the linear search ~6% of the encoding thread while walking).
pub(crate) static DIRTY_TEXTURES: std::sync::LazyLock<std::sync::Mutex<rustc_hash::FxHashSet<usize>>> =
    std::sync::LazyLock::new(Default::default);

/// Bumped by every mark_texture_dirty (lets the executor's binding memo notice pending re-uploads without a lock).
pub(crate) static DIRTY_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn texture_dirty_gen() -> u64 {
    DIRTY_GEN.load(std::sync::atomic::Ordering::Acquire)
}

pub fn mark_texture_dirty(tex: usize) {
    // Insert first: a reader that sees the new generation must also find the entry (memos record the generation and
    // skip take_texture_dirty while it is unchanged).
    DIRTY_TEXTURES.lock().unwrap().insert(tex);
    DIRTY_GEN.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// Whether `tex` has an upload pending (without taking it).
pub fn texture_dirty_pending(tex: usize) -> bool {
    DIRTY_TEXTURES.lock().unwrap().contains(&tex)
}

/// True (once) if the guest wrote into `tex` since the executor last looked.
pub fn take_texture_dirty(tex: usize) -> bool {
    DIRTY_TEXTURES.lock().unwrap().remove(&tex)
}

/// Writes one full mip level (layer 0) of tightly packed texels into the texture's guest memory (render-target
/// write-back, metal::writeback_pass).
/// `t` is the texture's recorded state (the object's memory may hold something else by now).
pub unsafe fn guest_write_level(t: &NvnTexture, level: usize, data: &[u8]) {
    let w = ((t.width as usize) >> level).max(1) as i32;
    let h = ((t.height as usize) >> level).max(1) as i32;
    let region = NvnCopyRegion {
        x: 0,
        y: 0,
        z: 0,
        w,
        h,
        d: 1,
    };
    let mut view: NvnTextureView = std::mem::zeroed();
    view.min_level = level as _;
    view.num_levels = 1;
    // Not marked dirty: the Metal texture already holds these texels (they were read from it), and the upload a dirty
    // mark triggers replaced newer GPU output with this copy (one-frame magenta tiles, 2026-10-13).
    texture_write_texels(t, &view, &region, data.as_ptr(), false);
}

/// `mark`: the texture's Metal copy is stale afterwards (the game wrote texels); false when the texels came from it.
pub(crate) unsafe fn texture_write_texels(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *const u8,
    mark: bool,
) {
    if tex.is_null() || region.is_null() || data.is_null() || (*tex).storage == 0 {
        return;
    }
    if mark {
        mark_texture_dirty(tex as usize);
    }
    let t = &*tex;
    let r = &*region;
    let params = texture_layout_params(t);
    let layout = crate::nvn::layout::compute_layout(&params);
    let (level, layer) = if view.is_null() {
        (0usize, 0u32)
    } else {
        ((*view).min_level as usize, (*view).min_layer as u32)
    };
    let Some(info) = crate::nvn::layout::format_info(t.format) else {
        return;
    };
    let slice_data_bytes = (((r.w.max(0) as u32 + info.block_w - 1) / info.block_w) as usize)
        * (((r.h.max(0) as u32 + info.block_h - 1) / info.block_h) as usize)
        * info.bytes as usize;
    let is_3d = t.target == crate::nvn::layout::NVN_TARGET_3D;
    for z in 0..r.d.max(1) {
        let (slice_layer, z_off) = if is_3d {
            let lvl_sz = layout.level_sizes.get(level).copied().unwrap_or(0) as usize;
            let depth_l = ((t.depth.max(1) as usize) >> level).max(1);
            (layer, (r.z.max(0) as usize + z as usize) * lvl_sz / depth_l)
        } else {
            (layer + (r.z.max(0) + z) as u32, 0)
        };
        let base = layout.layer_stride as usize * slice_layer as usize + z_off;
        let src = data.add(z as usize * slice_data_bytes);
        if !crate::nvn::layout::write_texels_2d(
            (t.storage) as *mut u8,
            base,
            &params,
            level,
            r.x,
            r.y,
            r.w,
            r.h,
            src,
        ) {
            eprintln!(
                "[!] nvnTextureWriteTexels: unsupported layout (target {} format {})",
                t.target, t.format
            );
            return;
        }
    }
}

/// Bytes of one tightly packed row and one slice of `region` (blocks for compressed formats).
pub(crate) unsafe fn texel_region_pitches(t: &NvnTexture, r: &NvnCopyRegion) -> Option<(usize, usize)> {
    let info = crate::nvn::layout::format_info(t.format)?;
    let row =
        ((r.w.max(0) as u32 + info.block_w - 1) / info.block_w) as usize * info.bytes as usize;
    let rows = ((r.h.max(0) as u32 + info.block_h - 1) / info.block_h) as usize;
    Some((row, row * rows))
}

/// nvnTextureReadTexels: the inverse of texture_write_texels, from the texture's (block-linear) storage into linear
/// `data`. TotK copies baked lighting (TexToGo/RBake_*) this way into its lightmap atlas; left unimplemented, the
/// atlas stayed zero and lit surfaces as fully occluded.
pub(crate) unsafe fn texture_read_texels(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *mut u8,
) -> bool {
    if tex.is_null() || region.is_null() || data.is_null() || (*tex).storage == 0 {
        return false;
    }
    let t = &*tex;
    let r = &*region;
    let params = texture_layout_params(t);
    let layout = crate::nvn::layout::compute_layout(&params);
    let (level, layer) = if view.is_null() {
        (0usize, 0u32)
    } else {
        ((*view).min_level as usize, (*view).min_layer as u32)
    };
    let Some((_, slice_data_bytes)) = texel_region_pitches(t, r) else {
        return false;
    };
    if t.target == crate::nvn::layout::NVN_TARGET_3D {
        eprintln!(
            "[!] nvnTextureReadTexels: 3D textures not supported (format {})",
            t.format
        );
        return false;
    }
    for z in 0..r.d.max(1) {
        let base = layout.layer_stride as usize * (layer + (r.z.max(0) + z) as u32) as usize;
        let dst = data.add(z as usize * slice_data_bytes);
        if !crate::nvn::layout::read_texels_2d(
            t.storage as *const u8,
            base,
            &params,
            level,
            r.x,
            r.y,
            r.w,
            r.h,
            dst,
        ) {
            eprintln!(
                "[!] nvnTextureReadTexels: unsupported layout (target {} format {})",
                t.target, t.format
            );
            return false;
        }
    }
    true
}

#[no_mangle]
pub(crate) unsafe fn watch_size_matches(t: &NvnTexture) -> bool {
    crate::settings::get()
        .trace
        .watch_size
        .is_some_and(|s| s.is(t.width, t.height))
}

/// Debug (--watch-size=WxH): logs texel reads / writes on textures of that size.
pub(crate) unsafe fn watch_texels(op: &str, tex: *const NvnTexture, view: *const NvnTextureView) {
    let w = crate::settings::get()
        .trace
        .watch_size
        .map(|s| (s.w as i32, s.h as i32));
    if let (Some((ww, wh)), false) = (w, tex.is_null()) {
        let t = &*tex;
        if t.width == ww && t.height == wh {
            println!(
                "[watch-size] {} texture {:p} storage {:#x} fmt {} levels {} view {:?}",
                op,
                tex,
                t.storage,
                t.format,
                t.levels,
                (!view.is_null()).then(|| *view)
            );
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureReadTexels(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *mut u8,
) {
    record_hle_call("hle_nvnTextureReadTexels");
    watch_texels("ReadTexels", tex, view);
    texture_read_texels(tex, view, region, data);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureReadTexelsStrided(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *mut u8,
    row_stride: isize,
    image_stride: isize,
) {
    record_hle_call("hle_nvnTextureReadTexelsStrided");
    watch_texels("ReadTexelsStrided", tex, view);
    if tex.is_null() || region.is_null() || data.is_null() {
        return;
    }
    let Some((row, slice)) = texel_region_pitches(&*tex, &*region) else {
        return;
    };
    let r = &*region;
    let (rs, is) = (
        if row_stride > 0 {
            row_stride as usize
        } else {
            row
        },
        if image_stride > 0 {
            image_stride as usize
        } else {
            slice
        },
    );
    if rs == row && is == slice {
        texture_read_texels(tex, view, region, data);
        return;
    }
    // Read tightly packed, then scatter rows to the caller's strides.
    let depth = r.d.max(1) as usize;
    let mut tmp = vec![0u8; slice * depth];
    if !texture_read_texels(tex, view, region, tmp.as_mut_ptr()) {
        return;
    }
    let rows = if row > 0 { slice / row } else { 0 };
    for z in 0..depth {
        for y in 0..rows {
            std::ptr::copy_nonoverlapping(
                tmp.as_ptr().add(z * slice + y * row),
                data.add(z * is + y * rs),
                row,
            );
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureWriteTexels(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *const u8,
) {
    record_hle_call("hle_nvnTextureWriteTexels");
    watch_texels("WriteTexels", tex, view);
    if !tex.is_null() && !region.is_null() && !data.is_null() && watch_size_matches(&*tex) {
        let r = &*region;
        let n = (r.w.max(0) * r.h.max(0) * 4) as usize;
        let nz = std::slice::from_raw_parts(data, n)
            .iter()
            .filter(|&&b| b != 0)
            .count();
        println!(
            "[watch-size]   data {:p} region {}x{} at ({}, {}): {} of {} bytes nonzero",
            data, r.w, r.h, r.x, r.y, nz, n
        );
    }
    texture_write_texels(tex, view, region, data, true);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureWriteTexelsStrided(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *const u8,
    row_stride: isize,
    image_stride: isize,
) {
    record_hle_call("hle_nvnTextureWriteTexelsStrided");
    watch_texels("WriteTexelsStrided", tex, view);
    if tex.is_null() || region.is_null() || data.is_null() {
        return;
    }
    let Some((row, slice)) = texel_region_pitches(&*tex, &*region) else {
        return;
    };
    let (rs, is) = (
        if row_stride > 0 {
            row_stride as usize
        } else {
            row
        },
        if image_stride > 0 {
            image_stride as usize
        } else {
            slice
        },
    );
    if rs == row && is == slice {
        texture_write_texels(tex, view, region, data, true);
        return;
    }
    // Gather the caller's strided rows into a tightly packed copy first.
    let depth = (*region).d.max(1) as usize;
    let rows = if row > 0 { slice / row } else { 0 };
    let mut tmp = vec![0u8; slice * depth];
    for z in 0..depth {
        for y in 0..rows {
            std::ptr::copy_nonoverlapping(
                data.add(z * is + y * rs),
                tmp.as_mut_ptr().add(z * slice + y * row),
                row,
            );
        }
    }
    texture_write_texels(tex, view, region, tmp.as_ptr(), true);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureFlushTexels(
    tex: *const NvnTexture,
    _view: *const NvnTextureView,
    _region: *const NvnCopyRegion,
) {
    record_hle_call("hle_nvnTextureFlushTexels"); // unified memory: nothing to flush, but the game may have written storage directly
    if !tex.is_null() {
        mark_texture_dirty(tex as usize);
    }
}

/// the packaged-data pointer is for the Windows reference implementation's conversion; "ignored on NX".
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetPackagedTextureData(
    _builder: *mut u8,
    _data: *const u8,
) {
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureInvalidateTexels(
    _tex: *const NvnTexture,
    _view: *const NvnTextureView,
    _region: *const NvnCopyRegion,
) {
    record_hle_call("hle_nvnTextureInvalidateTexels");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetSwizzle(
    b: *const NvnTextureBuilder,
    r: *mut i32,
    g: *mut i32,
    bl: *mut i32,
    a: *mut i32,
) {
    if b.is_null() {
        return;
    }
    // SetDefaults zeroes the swizzle; NVN's default is identity (R, G, B, A = 2..5).
    let sw = (*b).swizzle;
    let ident = sw == [0; 4];
    for (i, out) in [r, g, bl, a].into_iter().enumerate() {
        if !out.is_null() {
            *out = if ident { 2 + i as i32 } else { sw[i] as i32 };
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetDepthStencilMode(
    b: *const NvnTextureBuilder,
) -> i32 {
    if b.is_null() {
        0
    } else {
        (*b).depth_stencil_mode
    }
}
