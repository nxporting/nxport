// NVN texture and sampler pools: registered textures and samplers by pool slot, active pools.

use super::*;

// ---------------------------------------------------------------------------
// NVN TexturePool & SamplerPool family. NVNtexturePool and NVNsamplerPool
// are 0x20 (32) bytes each. They manage descriptor free-lists for shader
// access. Initialize MUST return 1 (NVN_TRUE), and GetSize returns descriptor count.
#[repr(C)]
pub struct NvnTexturePool {
    pub memory_pool: usize,
    pub memory_offset: usize,
    pub num_descriptors: i32,
    pub _pad: [u8; 12],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolInitialize(
    pool: *mut NvnTexturePool,
    memory_pool: usize,
    offset: usize,
    num_descriptors: i32,
) -> u8 {
    record_hle_call("hle_nvnTexturePoolInitialize");
    if pool.is_null() {
        return 0;
    }
    std::ptr::write_bytes(pool, 0, 1);
    (*pool).memory_pool = memory_pool;
    (*pool).memory_offset = offset;
    (*pool).num_descriptors = num_descriptors;
    println!("[+] NVN: Initialized TexturePool at {:p}, memory_pool: 0x{:x}, offset: 0x{:x}, num_descriptors: {}",
        pool, memory_pool, offset, num_descriptors);
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolSetDebugLabel(
    _pool: *mut NvnTexturePool,
    _label: *const c_char,
) {
    record_hle_call("hle_nvnTexturePoolSetDebugLabel");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolFinalize(pool: *mut NvnTexturePool) {
    record_hle_call("hle_nvnTexturePoolFinalize");
    if !pool.is_null() {
        std::ptr::write_bytes(pool, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolGetMemoryPool(pool: *const NvnTexturePool) -> usize {
    record_hle_call("hle_nvnTexturePoolGetMemoryPool");
    if pool.is_null() {
        0
    } else {
        (*pool).memory_pool
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolGetMemoryOffset(pool: *const NvnTexturePool) -> usize {
    record_hle_call("hle_nvnTexturePoolGetMemoryOffset");
    if pool.is_null() {
        0
    } else {
        (*pool).memory_offset
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolGetSize(pool: *const NvnTexturePool) -> i32 {
    record_hle_call("hle_nvnTexturePoolGetSize");
    if pool.is_null() {
        0
    } else {
        (*pool).num_descriptors
    }
}

/// Texture pool entries: texture address and the view as it was at registration. NVN writes the descriptor when
/// RegisterTexture is called, so the view is copied: games pass views on the stack and reuse that memory.
/// A texture pool slot. Real NVN writes the texture's descriptor into the pool at RegisterTexture time, so
/// re-initializing the NVNtexture afterwards (TotK's texture streamer reuses objects for other textures) does not
/// affect draws that use the slot until the game registers again. Resolving slots to the live object made materials
/// sample whatever texture the object held later (stair edges drawn with the gloom-crystal texture, Zelda's hair
/// green). The slot therefore remembers the object's generation and state at registration: while the object is
/// unchanged it resolves to the object itself (the executor's fast, cached path); once the object has been
/// re-initialized it resolves to a private copy of the registered state. The view is copied too: games pass views on
/// the stack and reuse that memory.
pub(crate) struct PoolSlot {
    pub(crate) obj: usize,
    pub(crate) gen: u64,
    pub(crate) state: NvnTexture,
    pub(crate) view: Option<NvnTextureView>,
    /// Created on first use after the object changed; not in the storage index (it is not a texture object and must
    /// not join alias groups); the executor finds its Metal texture through the description cache.
    pub(crate) copy: Option<Box<NvnTexture>>,
}

/// Each texture or sampler pool is its own namespace of ids: a game's and a mod's may both use id 0x101. A pool is known
/// by its descriptor memory, as on the console, and numbered as first seen, so with one pool slots keep their plain ids.
/// A slot's key is the pool's number above the id's 20 bits (the id bits of a texture handle).
pub(crate) static TEXTURE_POOLS: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());

pub(crate) static SAMPLER_POOLS: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());

/// The pools commands executed last bound (nvnCommandBufferSetTexturePool, SetSamplerPool); handles resolve in them.
pub(crate) static ACTIVE_TEXTURE_POOL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) static ACTIVE_SAMPLER_POOL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) fn pool_number(pools: &std::sync::Mutex<Vec<(usize, usize)>>, memory: (usize, usize)) -> u32 {
    let mut pools = pools.lock().unwrap();
    if let Some(i) = pools.iter().position(|p| *p == memory) {
        return i as u32;
    }
    pools.push(memory);
    pools.len() as u32 - 1
}

/// The number of a texture pool object, for Cmd::SetTexturePool.
pub unsafe fn texture_pool_number(pool: *const NvnTexturePool) -> u32 {
    if pool.is_null() {
        return 0;
    }
    pool_number(&TEXTURE_POOLS, ((*pool).memory_pool, (*pool).memory_offset))
}

/// The number of a sampler pool object, for Cmd::SetSamplerPool.
pub unsafe fn sampler_pool_number(pool: *const NvnSamplerPool) -> u32 {
    if pool.is_null() {
        return 0;
    }
    pool_number(&SAMPLER_POOLS, ((*pool).memory_pool, (*pool).memory_offset))
}

/// Cmd::SetTexturePool ran. Whether that changed the pool handles resolve in.
pub fn set_active_texture_pool(pool: u32) -> bool {
    ACTIVE_TEXTURE_POOL.swap(pool, std::sync::atomic::Ordering::Relaxed) != pool
}

/// Cmd::SetSamplerPool ran. Whether that changed the pool handles resolve in.
pub fn set_active_sampler_pool(pool: u32) -> bool {
    ACTIVE_SAMPLER_POOL.swap(pool, std::sync::atomic::Ordering::Relaxed) != pool
}

/// Bits above a texture handle's 32 that name the pools it resolves in now, for caches keyed by handle.
pub fn active_pools_bits() -> u64 {
    use std::sync::atomic::Ordering::Relaxed;
    (ACTIVE_TEXTURE_POOL.load(Relaxed) as u64) << 32 | (ACTIVE_SAMPLER_POOL.load(Relaxed) as u64) << 48
}

/// The key of texture slot `id` in the active pool: what pool_changes_since reports.
pub fn active_texture_slot(id: i32) -> i32 {
    pool_slot_key(ACTIVE_TEXTURE_POOL.load(std::sync::atomic::Ordering::Relaxed), id)
}

pub(crate) static POOL_TEXTURES: std::sync::Mutex<Option<rustc_hash::FxHashMap<i32, PoolSlot>>> =
    std::sync::Mutex::new(None);

pub fn lookup_pool_texture(id: i32) -> Option<(usize, Option<NvnTextureView>)> {
    let id = active_texture_slot(id);
    let mut map = POOL_TEXTURES.lock().unwrap();
    let slot = map.as_mut()?.get_mut(&id)?;
    if slot.obj == 0 {
        return Some((0, slot.view));
    }
    // Always the state registered (NVN wrote the descriptor into the pool then), as a stable copy: the live object is read
    // again later (when a draw sampling it is planned), and game threads re-initialize transient objects meanwhile.
    // Breath of the Wild sampled its 800x450 RGBA16F target through an object that had become another texture for a
    // frame: the scene lit magenta.
    if slot.copy.is_none() {
        let b = Box::new(slot.state);
        let addr = &*b as *const NvnTexture as usize;
        let gen = TEX_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        TEX_SNAPSHOTS
            .lock()
            .unwrap()
            .get_or_insert_with(Default::default)
            .insert(addr, (gen, slot.state));
        super::texture::COPY_SOURCE.lock().unwrap().insert(addr, slot.gen);
        slot.copy = Some(b);
        bump_tex_table_epoch();
    }
    Some((
        &**slot.copy.as_ref().unwrap() as *const NvnTexture as usize,
        slot.view,
    ))
}

/// Records a RegisterTexture / RegisterImage.
pub(crate) unsafe fn register_pool_slot(id: i32, texture: *const NvnTexture, view: Option<NvnTextureView>) {
    // NVN copies the descriptor into the pool here; objects the game copied (never initialized through us) are captured
    // like the ones commands name.
    capture_texture(texture as usize);
    let (gen, state) = if texture.is_null() {
        (0, std::mem::zeroed())
    } else {
        texture_snapshot(texture as usize).unwrap_or_else(|| (0, std::ptr::read(texture)))
    };
    // Debug: --log-pool-reg logs every texture pool registration.
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *LOG.get_or_init(|| crate::settings::get().log.log_pool_reg) {
        println!(
            "[poolreg] id={:#x} tex={:p} gen={} {}x{} fmt={} storage={:#x}",
            id, texture, gen, state.width, state.height, state.format, state.storage
        );
    }
    let mut map = POOL_TEXTURES.lock().unwrap();
    let map = map.get_or_insert_with(Default::default);
    if let Some(old) = map.get(&id) {
        if old.obj == texture as usize
            && old.gen == gen
            && old.view.map(view_bytes) == view.map(view_bytes)
        {
            return; // re-registration of the same state: keep the slot (and any copy)
        }
    }
    // The slot's previous copy goes with it.
    if let Some(c) = map.get(&id).and_then(|old| old.copy.as_ref()) {
        let addr = &**c as *const NvnTexture as usize;
        if let Some(m) = TEX_SNAPSHOTS.lock().unwrap().as_mut() {
            m.remove(&addr);
        }
        super::texture::COPY_SOURCE.lock().unwrap().remove(&addr);
    }
    map.insert(
        id,
        PoolSlot {
            obj: texture as usize,
            gen,
            state,
            view,
            copy: None,
        },
    );
    log_pool_change(id);
    bump_tex_table_epoch();
}

/// Recent texture pool registrations that changed a slot: (sequence, id; -1: every slot). A slot resolves to the state
/// it was registered with until then (PoolSlot), so backends keeping resolutions across submits drop only the ones these
/// name (pool_changes_since), where tex_table_epoch changes with every texture initialized anywhere (most frames).
pub(crate) static POOL_CHANGES: std::sync::Mutex<(u64, std::collections::VecDeque<(u64, i32)>)> =
    std::sync::Mutex::new((0, std::collections::VecDeque::new()));

pub(crate) const POOL_CHANGES_KEPT: usize = 1 << 14;

pub(crate) fn log_pool_change(id: i32) {
    let mut g = POOL_CHANGES.lock().unwrap();
    g.0 += 1;
    let seq = g.0;
    if g.1.len() == POOL_CHANGES_KEPT {
        g.1.pop_front();
    }
    g.1.push_back((seq, id));
}

/// The texture pool slots registered again after sequence `since` (-1: all of them), and the current sequence; false
/// when some changes are no longer kept (every slot must be considered changed).
pub fn pool_changes_since(since: u64, out: &mut Vec<i32>) -> (u64, bool) {
    let g = POOL_CHANGES.lock().unwrap();
    let complete = g.1.front().is_none_or(|f| f.0 <= since + 1) || g.0 == since;
    out.extend(g.1.iter().filter(|c| c.0 > since).map(|c| c.1));
    (g.0, complete)
}

pub(crate) fn view_bytes(v: NvnTextureView) -> [u8; std::mem::size_of::<NvnTextureView>()] {
    unsafe { std::mem::transmute(v) }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolRegisterTexture(
    pool: *const NvnTexturePool,
    id: i32,
    texture: *const NvnTexture,
    view: *const (),
) {
    record_hle_call("hle_nvnTexturePoolRegisterTexture");
    let view = (!view.is_null()).then(|| *(view as *const NvnTextureView));
    register_pool_slot(pool_slot_key(texture_pool_number(pool), id), texture, view);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolRegisterImage(
    pool: *const NvnTexturePool,
    id: i32,
    texture: *const NvnTexture,
    view: *const (),
) {
    record_hle_call("hle_nvnTexturePoolRegisterImage");
    let view = (!view.is_null()).then(|| *(view as *const NvnTextureView));
    register_pool_slot(pool_slot_key(texture_pool_number(pool), id), texture, view);
}

#[repr(C)]
pub struct NvnSamplerPool {
    pub memory_pool: usize,
    pub memory_offset: usize,
    pub num_descriptors: i32,
    pub _pad: [u8; 12],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolInitialize(
    pool: *mut NvnSamplerPool,
    memory_pool: usize,
    offset: usize,
    num_descriptors: i32,
) -> u8 {
    record_hle_call("hle_nvnSamplerPoolInitialize");
    if pool.is_null() {
        return 0;
    }
    std::ptr::write_bytes(pool, 0, 1);
    (*pool).memory_pool = memory_pool;
    (*pool).memory_offset = offset;
    (*pool).num_descriptors = num_descriptors;
    println!("[+] NVN: Initialized SamplerPool at {:p}, memory_pool: 0x{:x}, offset: 0x{:x}, num_descriptors: {}",
        pool, memory_pool, offset, num_descriptors);
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolSetDebugLabel(
    _pool: *mut NvnSamplerPool,
    _label: *const c_char,
) {
    record_hle_call("hle_nvnSamplerPoolSetDebugLabel");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolFinalize(pool: *mut NvnSamplerPool) {
    record_hle_call("hle_nvnSamplerPoolFinalize");
    if !pool.is_null() {
        std::ptr::write_bytes(pool, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolGetMemoryPool(pool: *const NvnSamplerPool) -> usize {
    record_hle_call("hle_nvnSamplerPoolGetMemoryPool");
    if pool.is_null() {
        0
    } else {
        (*pool).memory_pool
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolGetMemoryOffset(pool: *const NvnSamplerPool) -> usize {
    record_hle_call("hle_nvnSamplerPoolGetMemoryOffset");
    if pool.is_null() {
        0
    } else {
        (*pool).memory_offset
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolGetSize(pool: *const NvnSamplerPool) -> i32 {
    record_hle_call("hle_nvnSamplerPoolGetSize");
    if pool.is_null() {
        0
    } else {
        (*pool).num_descriptors
    }
}

/// Sampler state as registered in a sampler pool (NVN enum values).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SamplerDesc {
    pub min_filter: i32,
    pub mag_filter: i32,
    pub wrap_s: i32,
    pub wrap_t: i32,
    pub wrap_r: i32,
    pub compare_mode: i8,
    pub compare_func: i8,
    pub max_anisotropy: f32,
    pub lod_min: f32,
    pub lod_max: f32,
    pub lod_bias: f32,
    pub border_color: [f32; 4],
}

pub(crate) static POOL_SAMPLERS: std::sync::OnceLock<
    std::sync::Mutex<rustc_hash::FxHashMap<i32, SamplerDesc>>,
> = std::sync::OnceLock::new();

/// Sampler registered under `id` (handle bits 20.. of an NVNtextureHandle).
pub fn lookup_pool_sampler(id: i32) -> Option<SamplerDesc> {
    let id = pool_slot_key(ACTIVE_SAMPLER_POOL.load(std::sync::atomic::Ordering::Relaxed), id);
    POOL_SAMPLERS.get()?.lock().unwrap().get(&id).copied()
}

/// Changes at every sampler registration: lookups memoized per thread (gfx sampler states) are valid while unchanged.
pub(crate) static SAMPLERS_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn samplers_gen() -> u64 {
    SAMPLERS_GEN.load(std::sync::atomic::Ordering::Acquire)
}

/// The active pools changed, for backends memoizing resolutions by handle alone.
pub fn active_pools_changed() {
    SAMPLERS_GEN.fetch_add(1, std::sync::atomic::Ordering::Release);
    bump_tex_table_epoch();
}

pub(crate) fn register_pool_sampler(id: i32, d: SamplerDesc) {
    // Bumped after the insert below as well, so a lookup that read the old generation cannot keep the old sampler.
    struct Bump;
    impl Drop for Bump {
        fn drop(&mut self) {
            SAMPLERS_GEN.fetch_add(1, std::sync::atomic::Ordering::Release);
        }
    }
    let _bump = Bump;
    POOL_SAMPLERS
        .get_or_init(|| std::sync::Mutex::new(rustc_hash::FxHashMap::default()))
        .lock()
        .unwrap()
        .insert(id, d);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolRegisterSampler(
    pool: *const NvnSamplerPool,
    id: i32,
    sampler: *const NvnSampler,
) {
    record_hle_call("hle_nvnSamplerPoolRegisterSampler");
    if sampler.is_null() {
        return;
    }
    let s = &*sampler;
    register_pool_sampler(
        pool_slot_key(sampler_pool_number(pool), id),
        SamplerDesc {
            min_filter: s.min_filter,
            mag_filter: s.mag_filter,
            wrap_s: s.wrap_s,
            wrap_t: s.wrap_t,
            wrap_r: s.wrap_r,
            compare_mode: s.compare_mode,
            compare_func: s.compare_func,
            max_anisotropy: s.max_anisotropy,
            lod_min: s.lod_min,
            lod_max: s.lod_max,
            lod_bias: s.lod_bias,
            border_color: s.border_color,
        },
    );
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolRegisterSamplerBuilder(
    pool: *const NvnSamplerPool,
    id: i32,
    builder: *const NvnSamplerBuilder,
) {
    record_hle_call("hle_nvnSamplerPoolRegisterSamplerBuilder");
    if builder.is_null() {
        return;
    }
    let b = &*builder;
    register_pool_sampler(
        pool_slot_key(sampler_pool_number(pool), id),
        SamplerDesc {
            min_filter: b.min_filter,
            mag_filter: b.mag_filter,
            wrap_s: b.wrap_s,
            wrap_t: b.wrap_t,
            wrap_r: b.wrap_r,
            compare_mode: b.compare_mode,
            compare_func: b.compare_func,
            max_anisotropy: b.max_anisotropy,
            lod_min: b.lod_min,
            lod_max: b.lod_max,
            lod_bias: b.lod_bias,
            border_color: b.border_color,
        },
    );
}

#[cfg(test)]
mod pool_namespace_tests {
    use super::*;

    #[test]
    fn pool_slot_keys_keep_plain_ids_in_the_first_pool_and_differ_between_pools() {
        assert_eq!(pool_slot_key(0, 0x101), 0x101);
        assert_ne!(pool_slot_key(1, 0x101), 0x101);
        assert_ne!(pool_slot_key(1, 0x101), pool_slot_key(2, 0x101));
        assert_eq!(pool_slot_key(1, 0x101) & 0xFFFFF, 0x101);
    }

    #[test]
    fn a_pool_is_its_descriptor_memory() {
        let pools = std::sync::Mutex::new(Vec::new());
        assert_eq!(pool_number(&pools, (0x1000, 0x20)), 0);
        assert_eq!(pool_number(&pools, (0x2000, 0)), 1);
        // Another object over the same memory is the same pool.
        assert_eq!(pool_number(&pools, (0x1000, 0x20)), 0);
    }

    #[test]
    fn samplers_of_two_pools_do_not_replace_each_other() {
        let mut a: SamplerDesc = unsafe { std::mem::zeroed() };
        let mut b: SamplerDesc = unsafe { std::mem::zeroed() };
        a.max_anisotropy = 1.0;
        b.max_anisotropy = 2.0;
        register_pool_sampler(pool_slot_key(0, 0x101), a);
        register_pool_sampler(pool_slot_key(1, 0x101), b);
        set_active_sampler_pool(1);
        assert_eq!(lookup_pool_sampler(0x101).unwrap().max_anisotropy, 2.0);
        set_active_sampler_pool(0);
        assert_eq!(lookup_pool_sampler(0x101).unwrap().max_anisotropy, 1.0);
    }
}
