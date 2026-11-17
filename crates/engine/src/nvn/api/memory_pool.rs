// NVN memory pools: the pool registry, released ranges, virtual mappings and address translation.

use super::*;

// ---------------------------------------------------------------------------
// NVN Memory Pool family -- real implementation, not a stub.
//
// Root cause this session: the game's own GPU memory-pool wrapper object
// (sead::gfx-style, dispatched through a C++ vtable at guest+0xCB2500/+0xCB2B88)
// never got constructed because `nvnMemoryPoolInitialize` -- which it calls
// internally via a real function pointer fetched through
// nvnDeviceGetProcAddress -- was silently falling through to
// hle_nvn_generic_stub (always returns 0 = NVN_FALSE). The game read that
// as "pool init failed" and never finished its own construction, leaving the
// wrapper's vtable pointer permanently null. Implementing this family for
// real (not just making it return success) fixes that at the source.
//
// NVNmemoryPool (0x100 bytes) and NVNmemoryPoolBuilder (0x40 bytes) are
// opaque, driver-defined storage (NVN declares them as
// `char reserved[...]`) -- the game never reads their fields directly, only
// ever hands the pointer back into other nvn* calls. That means we're free
// to define our own internal layout, as long as our own Set*/Get*/Initialize
// implementations agree with each other -- same freedom nn::vi's opaque
// Display/Layer handles already gave us.
#[repr(C)]
pub(crate) struct NvnMemoryPoolBuilder {
    device: usize,
    memory: usize,
    size: usize,
    flags: i32,
    _pad: u32,
}

#[repr(C)]
pub struct NvnMemoryPool {
    pub memory: usize,
    pub size: usize,
    pub flags: i32,
    pub _pad: u32,
    pub device: usize,
    pub owns_memory: u8, // 1 if we host-allocated `memory` ourselves (VIRTUAL pool) and must free it on Finalize
}

// Live memory pools as (base, size) host ranges. GPU addresses are host pointers, so the Metal executor
// resolves a buffer address to its pool and wraps the pool's pages as one zero-copy MTLBuffer.
/// Registered pools by base -> size. Registration removes overlapping entries, so at most one contains an address.
pub(crate) static MEMORY_POOLS: std::sync::Mutex<std::collections::BTreeMap<usize, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// NVNmemoryPool object -> base of its registration in MEMORY_POOLS.
pub(crate) static POOL_OBJECTS: std::sync::Mutex<std::collections::BTreeMap<usize, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// NVN memory pool flags by pool base (NVN_MEMORY_POOL_FLAGS: CPU_NO_ACCESS 0x1, CPU_UNCACHED 0x2, CPU_CACHED 0x4, ...).
pub(crate) static MEMORY_POOL_FLAGS: std::sync::Mutex<std::collections::BTreeMap<usize, i32>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Bumped whenever MEMORY_POOLS, MEMORY_POOL_FLAGS or VIRTUAL_MAPS change; invalidates the per-thread lookup caches
/// below (the executor resolves pools for every buffer and texture binding, and a linear scan per lookup was ~9% of the
/// presentation thread).
pub(crate) static POOLS_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) fn bump_pools_gen() {
    POOLS_GEN.fetch_add(1, std::sync::atomic::Ordering::Release);
}

/// Changes whenever a pool is registered or finalized, a virtual mapping changes, or guest memory is released or
/// unmapped: lookups derived from pools (gfx zero-copy buffer wrappers) are valid while it is unchanged.
pub fn pools_gen() -> u64 {
    POOLS_GEN.load(std::sync::atomic::Ordering::Acquire)
}

/// Per-thread cache of recently found pools: (base, size, cpu_writable); valid while the generation matches.
pub(crate) struct PoolCache {
    pub(crate) gen: u64,
    pub(crate) entries: [(usize, usize, bool); 8],
    pub(crate) next: usize,
}

/// (base, size, cpu_writable) of the pool containing `addr` (no virtual translation).
pub(crate) fn pool_info(addr: usize) -> Option<(usize, usize, bool)> {
    let gen = POOLS_GEN.load(std::sync::atomic::Ordering::Acquire);
    let hit = POOL_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.gen != gen {
            c.gen = gen;
            c.entries = [(0, 0, false); 8];
            return None;
        }
        c.entries
            .iter()
            .copied()
            .find(|&(b, s, _)| s != 0 && addr >= b && addr < b + s)
    });
    if hit.is_some() {
        return hit;
    }
    // Ordered by base: the closest pool starting at or below `addr` that still covers it. Pools can nest (Kirby makes
    // small shader pools inside the storage of a 1.4 GB pool that also holds vertex data), so the nearest one below
    // may end before `addr`.
    let (base, size) = containing_pool(addr, 1)?;
    let writable = MEMORY_POOL_FLAGS
        .lock()
        .unwrap()
        .get(&base)
        .map_or(false, |f| f & 0x6 != 0);
    POOL_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.gen == gen {
            let i = c.next;
            c.entries[i] = (base, size, writable);
            c.next = (i + 1) % 8;
        }
    });
    Some((base, size, writable))
}

/// Whether the CPU can write the memory pool containing `addr` (so data the GPU reads from it may change under it).
pub fn memory_pool_cpu_writable(addr: usize) -> bool {
    pool_info(translate_virtual(addr)).map_or(false, |p| p.2)
}

/// The registered pool range containing `addr`, if any.
/// Guest memory ranges released since the executor last looked (pool finalized / address region freed): zero-copy
/// Metal buffers wrapping them must not be reused.
pub(crate) static RELEASED_RANGES: StdMutex<Vec<(usize, usize)>> = StdMutex::new(Vec::new());

/// Address regions whose unmap is deferred until the GPU can no longer be using them: (addr, size, frame freed).
pub(crate) static PENDING_UNMAPS: StdMutex<Vec<(usize, usize, u64)>> = StdMutex::new(Vec::new());

pub(crate) fn guest_memory_released(addr: usize, size: usize) {
    unsafe { crate::guest::dirty_track::release(addr, size) };
    #[cfg(nx_native)]
    crate::gpu::metal::rt_guard::released(addr, size);
    RELEASED_RANGES.lock().unwrap().push((addr, size));
    TEX_RELEASED.lock().unwrap().push((addr, size));
    bump_pools_gen();
}

/// Released ranges for the texture cache (metal::sweep_shared_storage), separate from the buffer cache's list.
pub(crate) static TEX_RELEASED: StdMutex<Vec<(usize, usize)>> = StdMutex::new(Vec::new());

/// Ranges released since the texture cache last looked.
pub fn take_tex_released() -> Vec<(usize, usize)> {
    std::mem::take(&mut *TEX_RELEASED.lock().unwrap())
}

/// Ranges whose pages were replaced (deferred unmap) since the executor last looked.
pub(crate) static UNMAPPED_RANGES: StdMutex<Vec<(usize, usize)>> = StdMutex::new(Vec::new());

/// Ranges released since the last call (for the executor's buffer cache): (addr, size, pages replaced now).
pub fn take_released_ranges() -> Vec<(usize, usize, bool)> {
    let mut v: Vec<(usize, usize, bool)> = std::mem::take(&mut *RELEASED_RANGES.lock().unwrap())
        .into_iter()
        .map(|(a, s)| (a, s, false))
        .collect();
    v.extend(
        std::mem::take(&mut *UNMAPPED_RANGES.lock().unwrap())
            .into_iter()
            .map(|(a, s)| (a, s, true)),
    );
    v
}

/// Unmaps address regions freed at least `frames` presents ago (called by the executor at present).
pub fn flush_pending_unmaps(now: u64, frames: u64) {
    let mut pending = PENDING_UNMAPS.lock().unwrap();
    pending.retain(|&(addr, size, at)| {
        if now >= at + frames {
            unsafe { release_address_region(addr, size) };
            // The pages are replaced only now, and a zero-copy Metal buffer keeps the pages it was made over: the GPU
            // would read the old contents through it forever (checked with a standalone test). Draws encoded since the
            // free (recorded before it) can have wrapped this range again, and such a buffer would serve the region's
            // next allocation stale data (geometry streaks that accumulate as the game streams). Report it again;
            // this runs on the executor thread, before any later wrap.
            UNMAPPED_RANGES.lock().unwrap().push((addr, size));
            bump_pools_gen();
            false
        } else {
            true
        }
    });
}

pub fn find_memory_pool(addr: usize) -> Option<(usize, usize)> {
    pool_info(addr).map(|(b, s, _)| (b, s))
}

/// The innermost pool (by start) that covers all of `[addr, addr + len)`.
pub fn containing_pool(addr: usize, len: usize) -> Option<(usize, usize)> {
    MEMORY_POOLS
        .lock()
        .unwrap()
        .range(..=addr)
        .rev()
        .take(4096)
        .map(|(&b, &s)| (b, s))
        .find(|&(b, s)| addr + len.max(1) <= b + s)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderSetDefaults(builder: *mut NvnMemoryPoolBuilder) {
    record_hle_call("hle_nvnMemoryPoolBuilderSetDefaults");
    if !builder.is_null() {
        std::ptr::write_bytes(builder, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderSetDevice(
    builder: *mut NvnMemoryPoolBuilder,
    device: usize,
) {
    record_hle_call("hle_nvnMemoryPoolBuilderSetDevice");
    if !builder.is_null() {
        (*builder).device = device;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderSetStorage(
    builder: *mut NvnMemoryPoolBuilder,
    memory: usize,
    size: usize,
) {
    record_hle_call("hle_nvnMemoryPoolBuilderSetStorage");
    if !builder.is_null() {
        (*builder).memory = memory;
        (*builder).size = size;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderSetFlags(
    builder: *mut NvnMemoryPoolBuilder,
    flags: i32,
) {
    record_hle_call("hle_nvnMemoryPoolBuilderSetFlags");
    if !builder.is_null() {
        (*builder).flags = flags;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderGetDevice(
    builder: *const NvnMemoryPoolBuilder,
) -> usize {
    record_hle_call("hle_nvnMemoryPoolBuilderGetDevice");
    if builder.is_null() {
        0
    } else {
        (*builder).device
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderGetMemory(
    builder: *const NvnMemoryPoolBuilder,
) -> usize {
    record_hle_call("hle_nvnMemoryPoolBuilderGetMemory");
    if builder.is_null() {
        0
    } else {
        (*builder).memory
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderGetSize(
    builder: *const NvnMemoryPoolBuilder,
) -> usize {
    record_hle_call("hle_nvnMemoryPoolBuilderGetSize");
    if builder.is_null() {
        0
    } else {
        (*builder).size
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderGetFlags(
    builder: *const NvnMemoryPoolBuilder,
) -> i32 {
    record_hle_call("hle_nvnMemoryPoolBuilderGetFlags");
    if builder.is_null() {
        0
    } else {
        (*builder).flags
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolInitialize(
    pool: *mut NvnMemoryPool,
    builder: *const NvnMemoryPoolBuilder,
) -> u8 {
    let lr = crate::guest_reg!(x30);
    let mut fp = crate::guest_reg!(x29);
    record_hle_call("hle_nvnMemoryPoolInitialize");
    if pool.is_null() || builder.is_null() {
        return 0; // NVN_FALSE
    }
    let size = (*builder).size;
    if size == 0 {
        return 0;
    }
    let mut memory = (*builder).memory;
    let mut owns_memory = 0u8;
    if memory == 0 {
        // VIRTUAL pools (and any caller that didn't supply backing storage)
        // get real host memory of their own -- matches real NVN semantics
        // for a pool with no caller-supplied storage.
        let out = std::alloc::alloc_zeroed(owned_pool_layout(size));
        if out.is_null() {
            return 0;
        }
        memory = out as usize;
        owns_memory = 1;
    }
    std::ptr::write(
        pool,
        NvnMemoryPool {
            memory,
            size,
            flags: (*builder).flags,
            _pad: 0,
            device: (*builder).device,
            owns_memory,
        },
    );
    {
        // Pools live until nvnMemoryPoolFinalize and may overlap: Kirby creates small shader pools inside the storage of
        // a live 1.4 GB pool that also holds vertex data (lookups take the innermost covering pool). Only a pool object
        // initialized again without Finalize replaces its own earlier registration.
        let mut pools = MEMORY_POOLS.lock().unwrap();
        if let Some(old) = POOL_OBJECTS.lock().unwrap().insert(pool as usize, memory) {
            if old != memory {
                pools.remove(&old);
                MEMORY_POOL_FLAGS.lock().unwrap().remove(&old);
            }
        }
        pools.insert(memory, size);
        let mut flags = MEMORY_POOL_FLAGS.lock().unwrap();
        flags.insert(memory, (*builder).flags as i32);
        bump_pools_gen();
    }
    println!(
        "[+] NVN: MemoryPool initialized: memory={:#x} size={} flags={:#x} (host-allocated={})",
        memory,
        size,
        (*builder).flags,
        owns_memory != 0
    );
    if size >= 64 << 20 {
        // Big pools (render targets, streaming): who sizes them (mods such as UltraCam enlarge some).
        let base = crate::guest::loader::main_base();
        let mut chain = vec![format!("{:#x}", lr.wrapping_sub(base))];
        for _ in 0..10 {
            if fp < 0x1000 || fp & 7 != 0 {
                break;
            }
            let next = *(fp as *const usize);
            chain.push(format!(
                "{:#x}",
                (*((fp + 8) as *const usize)).wrapping_sub(base)
            ));
            if next <= fp {
                break;
            }
            fp = next;
        }
        println!(
            "[+] NVN:   pool of {} MB created from base+ {}",
            size >> 20,
            chain.join(" <- ")
        );
    }
    1 // NVN_TRUE
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolFinalize(pool: *mut NvnMemoryPool) {
    record_hle_call("hle_nvnMemoryPoolFinalize");
    if pool.is_null() {
        return;
    }
    let base = (*pool).memory;
    guest_memory_released(base, (*pool).size);
    POOL_OBJECTS.lock().unwrap().remove(&(pool as usize));
    MEMORY_POOLS.lock().unwrap().remove(&base);
    bump_pools_gen();
    if (*pool).owns_memory != 0 && (*pool).memory != 0 {
        std::alloc::dealloc((*pool).memory as *mut u8, owned_pool_layout((*pool).size));
    }
    std::ptr::write_bytes(pool, 0, 1);
}

/// The memory of a pool given none by the game, aligned to NVN's pool page (NVN_DEVICE_INFO_MEMORY_POOL_PAGE_SIZE).
pub(crate) fn owned_pool_layout(size: usize) -> std::alloc::Layout {
    std::alloc::Layout::from_size_align(size, 0x10000).expect("pool size")
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolMap(pool: *const NvnMemoryPool) -> usize {
    record_hle_call("hle_nvnMemoryPoolMap");
    // Real NVN only allows mapping pools without CPU_NO_ACCESS; we're always
    // host-memory-backed (no discrete VRAM to be inaccessible from), so just
    // hand back the real pointer.
    if pool.is_null() {
        0
    } else {
        (*pool).memory
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolGetBufferAddress(pool: *const NvnMemoryPool) -> u64 {
    record_hle_call("hle_nvnMemoryPoolGetBufferAddress");
    // No separate GPU virtual address space exists here -- the host pointer
    // *is* the "GPU address" by our own internal convention, consistent with
    // how our NVN implementation resolves texture/buffer storage elsewhere.
    if pool.is_null() {
        0
    } else {
        (*pool).memory as u64
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolSetDebugLabel(
    _pool: *mut NvnMemoryPool,
    _label: *const c_char,
) {
    record_hle_call("hle_nvnMemoryPoolSetDebugLabel");
    // Debug-only on real hardware too; no functional effect.
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolGetFlags(pool: *const NvnMemoryPool) -> i32 {
    record_hle_call("hle_nvnMemoryPoolGetFlags");
    if pool.is_null() {
        0
    } else {
        (*pool).flags
    }
}

/// NVNmappingRequest: maps `size` bytes of a VIRTUAL pool (at `virtual_offset`) onto PHYSICAL pool storage.
#[repr(C)]
pub struct NvnMappingRequest {
    pub physical_pool: *const NvnMemoryPool,
    pub physical_offset: i64,
    pub virtual_offset: i64,
    pub size: u64,
    pub storage_class: i32,
}

/// Virtual pool pages mapped onto physical pool memory, as (virtual start, size, physical start). Physical pool memory
/// is only 4 KB aligned, so the host pages cannot be remapped (16 KB pages); addresses are translated instead.
pub(crate) static VIRTUAL_MAPS: StdMutex<Vec<(usize, usize, usize)>> = StdMutex::new(Vec::new());

/// No virtual mappings exist (translate_virtual can skip the lock).
pub(crate) static VIRTUAL_MAPS_EMPTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// The host address backing `addr`: inside a mapped range of a VIRTUAL pool, the physical memory it maps to.
pub fn translate_virtual(addr: usize) -> usize {
    if VIRTUAL_MAPS_EMPTY.load(std::sync::atomic::Ordering::Acquire) {
        return addr;
    }
    // Each thread keeps a copy of the mappings sorted by start, refreshed when the pool generation changes (every
    // mapping change bumps it): this runs several times per draw binding on the encoding threads, and the global lock
    // plus a linear scan of every mapping showed up in their profiles. Mappings never overlap (a new one splits what
    // was there), so the binary search finds the same one the scan did.
    thread_local! {
        static MAPS: std::cell::RefCell<(u64, Vec<(usize, usize, usize)>)> = const { std::cell::RefCell::new((0, Vec::new())) };
    }
    let gen = pools_gen();
    MAPS.with(|m| {
        let mut m = m.borrow_mut();
        if m.0 != gen {
            let mut v = VIRTUAL_MAPS.lock().unwrap().clone();
            v.sort_unstable_by_key(|e| e.0);
            *m = (gen, v);
        }
        let maps = &m.1;
        let i = maps.partition_point(|&(v, _, _)| v <= addr);
        match i.checked_sub(1).map(|i| maps[i]) {
            Some((v, s, p)) if addr < v + s => p + (addr - v),
            _ => addr,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolMapVirtual(
    pool: *const NvnMemoryPool,
    num_requests: i32,
    requests: *const NvnMappingRequest,
) -> u8 {
    record_hle_call("hle_nvnMemoryPoolMapVirtual");
    if pool.is_null() || requests.is_null() || num_requests < 0 {
        return 0;
    }
    for i in 0..num_requests as usize {
        let r = &*requests.add(i);
        let (voff, size) = (r.virtual_offset as usize, r.size as usize);
        if voff + size > (*pool).size {
            return 0;
        }
        let vstart = (*pool).memory + voff;
        let mut maps = VIRTUAL_MAPS.lock().unwrap();
        // Split any existing mapping around the new range (a remap or an unmap replaces what was there).
        let mut kept = Vec::new();
        for &(v, s, p) in maps.iter() {
            if v + s <= vstart || v >= vstart + size {
                kept.push((v, s, p));
                continue;
            }
            if v < vstart {
                kept.push((v, vstart - v, p));
            }
            if v + s > vstart + size {
                let cut = vstart + size - v;
                kept.push((vstart + size, s - cut, p + cut));
            }
        }
        if !r.physical_pool.is_null() {
            kept.push((
                vstart,
                size,
                (*r.physical_pool).memory + r.physical_offset as usize,
            ));
        }
        *maps = kept;
        VIRTUAL_MAPS_EMPTY.store(maps.is_empty(), std::sync::atomic::Ordering::Release);
        bump_pools_gen();
        drop(maps);
        // Zero-copy buffers wrapping the old backing of this range are stale.
        guest_memory_released(vstart, size);
        println!(
            "[+] NVN: MapVirtual {:#x}+{:#x} -> {}",
            vstart,
            size,
            if r.physical_pool.is_null() {
                "unmapped".to_string()
            } else {
                format!(
                    "{:#x}",
                    (*r.physical_pool).memory + r.physical_offset as usize
                )
            }
        );
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolFlushMappedRange(
    _pool: *const NvnMemoryPool,
    _offset: i64,
    _size: i64,
) -> u8 {
    record_hle_call("hle_nvnMemoryPoolFlushMappedRange");
    // Apple Silicon is a unified-memory architecture -- there's no CPU/GPU
    // cache-coherency gap to flush across, unlike the Tegra X1's split
    // pools. A no-op is the semantically *correct* behavior here, not a gap.
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolInvalidateMappedRange(
    _pool: *const NvnMemoryPool,
    _offset: i64,
    _size: i64,
) -> u8 {
    record_hle_call("hle_nvnMemoryPoolInvalidateMappedRange");
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolGetSize(pool: *const NvnMemoryPool) -> usize {
    if pool.is_null() {
        0
    } else {
        (*pool).size
    }
}
