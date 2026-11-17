// Guest memory as nn::os and the kernel calls see it: the address regions handed out, the region arena they come from,
// the memory blocks and heap sizes the game queries.

use std::sync::{Mutex as StdMutex, OnceLock};

/// nn::os memory heap bookkeeping for QueryMemoryInfo: the size SetMemoryHeapSize set, and the blocks allocated from it
/// (address -> size). Super Mario Bros. Wonder sizes its main heap as heap size minus what is already allocated.
pub(crate) static MEMORY_HEAP_SIZE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(crate) static PROGRAM_SIZE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Bytes of the loaded program images (main and the SDK), for QueryMemoryInfo.
pub(crate) fn set_program_size(n: usize) {
    PROGRAM_SIZE.store(n, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) static MEMORY_BLOCKS: StdMutex<Vec<(usize, usize)>> = StdMutex::new(Vec::new());

#[repr(C)]
/// nn::os::MemoryInfo.
pub(crate) struct NnMemoryInfo {
    pub total_available_memory_size: u64,
    pub total_used_memory_size: u64,
    pub total_memory_heap_size: u64,
    pub allocated_memory_heap_size: u64,
    pub program_size: u64,
    pub total_thread_stack_size: u64,
    pub thread_count: i32,
}

// nn::os::AllocateAddressRegion / AllocateMemoryPages / FreeAddressRegion / FreeMemoryPages: Horizon's
// reserve-then-commit virtual memory, used directly (bypassing StandardAllocator) by the game's page allocators.
// Regions are mapped read/write up front; AllocateMemoryPages zeroes the guest pages it commits. A side table
// tracks each region's size, since FreeAddressRegion takes only an address.
pub(crate) static ADDRESS_REGIONS: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, usize>>> = OnceLock::new();

pub(crate) const GUEST_PAGE: usize = 0x1000;

pub(crate) const HOST_PAGE: usize = 0x4000;

/// 4KB guest pages currently mapped through nn::os::AllocateMemoryPages (page number = address / 4KB).
pub(crate) fn mapped_guest_pages() -> &'static StdMutex<std::collections::HashSet<usize>> {
    static PAGES: OnceLock<StdMutex<std::collections::HashSet<usize>>> = OnceLock::new();
    PAGES.get_or_init(|| StdMutex::new(std::collections::HashSet::new()))
}

pub(crate) fn address_regions() -> &'static StdMutex<rustc_hash::FxHashMap<usize, usize>> {
    ADDRESS_REGIONS.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

/// Horizon hands out address regions from a dedicated part of the address space, never interleaved with the
/// memory heap. TotK's page-allocator heap (TotK 1.1.2 vtable main+0x45b43c8) depends on that: its isInclude() is just
/// [lowest chunk, highest chunk end), so with regions from plain mmap() one chunk could land below the memory heap
/// and another above it, the heap then claimed ordinary heap pointers, and sead::HeapMgr::findContainHeap routed
/// their frees to it (the intermittent crash at TotK 1.1.2 main+0xAFCEB4). All regions now come from one reserved arena.
/// Largest arena tried first; smaller ones after it (iOS apps without the extended-virtual-addressing entitlement get
/// far less address space than macOS processes).
#[cfg(not(target_os = "ios"))]
pub(crate) const REGION_ARENA_SIZES: &[usize] = &[16 << 30, 8 << 30, 4 << 30, 2 << 30];

/// iPhone 16e without the entitlement (probe, 2026-10-08): 4 GB largest reservation, ~7 GB in total; the game heap
/// (AllocateMemoryBlock, 3.2 GB for Kirby) must still fit next to the arena.
#[cfg(target_os = "ios")]
pub(crate) const REGION_ARENA_SIZES: &[usize] = &[2 << 30, 1 << 30];

pub(crate) const REGION_ALIGN: usize = 0x10000; // Horizon requires 64KB alignment for address regions

pub(crate) struct RegionArena {
    base: usize,
    size: usize,
    /// --file-backed-memory: the arena's pages come from this sparse file, at their offset from `base`.
    #[cfg(target_vendor = "apple")]
    file: Option<std::fs::File>,
    /// Free ranges: start -> size (coalesced, 64KB granular).
    free: std::collections::BTreeMap<usize, usize>,
}

pub(crate) fn region_arena() -> &'static StdMutex<RegionArena> {
    static ARENA: OnceLock<StdMutex<RegionArena>> = OnceLock::new();
    ARENA.get_or_init(|| {
        let reserved = REGION_ARENA_SIZES.iter().find_map(|&size| {
            unsafe { crate::platform::os::reserve(crate::platform::os::At::Anywhere, size + REGION_ALIGN) }.ok().map(|raw| (raw, size))
        });
        let mut free = std::collections::BTreeMap::new();
        let (base, size) = if let Some((raw, size)) = reserved {
            let base = (raw + REGION_ALIGN - 1) & !(REGION_ALIGN - 1);
            free.insert(base, size);
            println!(
                "[+] HLE: address region arena reserved at {:#x}..{:#x} ({} GB)",
                base,
                base + size,
                size >> 30
            );
            crate::guest::dirty_track::register_domain(base, size);
            (base, size)
        } else {
            println!("[!] HLE: could not reserve the address region arena; regions are reserved one by one");
            (0, 0)
        };
        #[cfg(target_vendor = "apple")]
        let file = (size != 0 && crate::settings::get().guest.file_backed_memory)
            // The arena is a reservation; regions in use are what the disk must hold. 2 GB covers TotK's.
            .then(|| crate::platform::os::sparse_file_with_room(size, 3 << 30, &std::env::temp_dir()))
            .and_then(|f| f.map_err(|e| eprintln!("[!] HLE: file-backed region arena failed ({e}); anonymous instead")).ok());
        StdMutex::new(RegionArena {
            base,
            size,
            free,
            #[cfg(target_vendor = "apple")]
            file,
        })
    })
}

pub(crate) fn in_region_arena(addr: usize) -> bool {
    let a = region_arena().lock().unwrap();
    a.base != 0 && addr >= a.base && addr < a.base + a.size
}

/// The arena's file and `addr`'s offset in it (--file-backed-memory).
#[cfg(target_vendor = "apple")]
pub(crate) fn arena_file(addr: usize) -> Option<(i32, usize)> {
    use std::os::unix::io::AsRawFd;
    let a = region_arena().lock().unwrap();
    a.file.as_ref().map(|f| (f.as_raw_fd(), addr - a.base))
}

/// Commits a range of the arena: from its file when it has one.
pub(crate) unsafe fn region_arena_commit(addr: usize, size: usize) -> std::io::Result<()> {
    #[cfg(target_vendor = "apple")]
    if let Some((fd, off)) = arena_file(addr) {
        return crate::platform::os::map_file(addr, size, fd, off);
    }
    crate::platform::os::commit(addr, size, crate::platform::os::Prot::ReadWrite)
}

/// First fit; `size` is 64KB aligned.
pub(crate) fn region_arena_alloc(size: usize) -> Option<usize> {
    let mut a = region_arena().lock().unwrap();
    let (&start, &len) = a.free.iter().find(|(_, &len)| len >= size)?;
    a.free.remove(&start);
    if len > size {
        a.free.insert(start + size, len - size);
    }
    Some(start)
}

pub(crate) fn region_arena_free(addr: usize, size: usize) {
    let mut a = region_arena().lock().unwrap();
    let (mut start, mut len) = (addr, size);
    if let Some((&prev, &plen)) = a.free.range(..addr).next_back() {
        if prev + plen == addr {
            a.free.remove(&prev);
            start = prev;
            len += plen;
        }
    }
    if let Some(&nlen) = a.free.get(&(addr + size)) {
        a.free.remove(&(addr + size));
        len += nlen;
    }
    a.free.insert(start, len);
}

/// Returns a freed region's memory to the system (and its addresses to the arena).
pub(crate) unsafe fn release_address_region(addr: usize, size: usize) {
    if in_region_arena(addr) {
        // Back to reserved: drops the pages but keeps the arena contiguous.
        crate::platform::os::decommit(addr, size);
        // File-backed: the pages read zero when the range is handed out again.
        #[cfg(target_vendor = "apple")]
        if let Some((fd, off)) = arena_file(addr) {
            crate::platform::os::punch_hole(fd, off, size);
        }
        region_arena_free(addr, size);
    } else {
        crate::platform::os::release(addr, size);
    }
}
