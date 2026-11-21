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

/// iOS without the extended-address-space entitlement: 4 GB largest reservation, ~7 GB in total; the game heap
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
pub(crate) unsafe extern "C" fn set_memory_heap_size(size: usize) -> i32 {
    println!(
        "[+] HLE: nn::os::SetMemoryHeapSize({} MB / {} bytes)",
        size / (1024 * 1024),
        size
    );
    MEMORY_HEAP_SIZE.store(size, std::sync::atomic::Ordering::Relaxed);
    crate::horizon::heap_layout::set_size(size);
    0
}

pub(crate) unsafe extern "C" fn free_memory_block(addr: usize, size: usize) {
    let mut b = MEMORY_BLOCKS.lock().unwrap();
    if let Some(i) = b.iter().position(|&(a, _)| a == addr) {
        b.remove(i);
        crate::horizon::heap_layout::freed(addr);
        crate::guest::dirty_track::release(addr, size);
        crate::platform::os::free(addr as *mut u8, size);
    }
    println!(
        "[+] HLE: nn::os::FreeMemoryBlock({:#x}, {} MB)",
        addr,
        size / (1024 * 1024)
    );
}

pub(crate) unsafe extern "C" fn allocate_memory_block(out_addr: *mut usize, size: usize) -> i32 {
    let file_backed = {
        #[cfg(unix)]
        {
            crate::settings::get().guest.file_backed_memory.then(|| crate::platform::os::alloc_file_backed(size, &std::env::temp_dir()))
        }
        #[cfg(not(unix))]
        {
            None::<std::io::Result<*mut u8>>
        }
    };
    let alloc = match file_backed {
        Some(Ok(p)) => {
            println!("[+] HLE: AllocateMemoryBlock: {} MB file-backed", size >> 20);
            Ok(p)
        }
        Some(Err(e)) => {
            eprintln!("[!] HLE: AllocateMemoryBlock: file-backed memory failed ({e}); anonymous instead");
            crate::platform::os::alloc_on_demand(size)
        }
        None => crate::platform::os::alloc_on_demand(size),
    };
    let Ok(ptr) = alloc else {
        eprintln!(
            "[-] HLE: AllocateMemoryBlock failed for {} bytes",
            size
        );
        return 1;
    };
    // Fresh anonymous memory is already zero; touching it would commit the whole block (2.5 GB for TotK's heap).
    if !out_addr.is_null() {
        *out_addr = ptr as usize;
    }
    MEMORY_BLOCKS.lock().unwrap().push((ptr as usize, size));
    crate::horizon::heap_layout::allocated(ptr as usize, size);
    println!(
        "[+] HLE: nn::os::AllocateMemoryBlock({} MB) allocated at {:p}",
        size / (1024 * 1024),
        ptr
    );
    crate::guest::dirty_track::register_domain(ptr as usize, size);
    #[cfg(nx_native)]
    let focus = crate::guest::exec::crash_handler::watch_focus();
    #[cfg(nx_native)]
    if focus >= ptr as usize && focus < ptr as usize + size {
        crate::guest::exec::crash_handler::add_watchpoint(focus);
    }
    0
}

pub(crate) unsafe extern "C" fn query_memory_info(info: *mut NnMemoryInfo) {
    // Every field: games read more than the first two (Super Mario Bros. Wonder sizes its heap from them).
    if !info.is_null() {
        // Used memory as Horizon counts it: the program image, the main thread's stack and the memory heap once its size
        // is set. Super Mario Bros. Wonder aborts at boot unless what is used before its heap fits its 85 MB "Program"
        // budget, then sizes the heap as available - used - its reserve.
        let allocated: usize = MEMORY_BLOCKS.lock().unwrap().iter().map(|b| b.1).sum();
        let heap = MEMORY_HEAP_SIZE.load(std::sync::atomic::Ordering::Relaxed) as u64;
        let program = PROGRAM_SIZE.load(std::sync::atomic::Ordering::Relaxed) as u64;
        let stacks = 1u64 << 20;
        (*info).total_available_memory_size = 4 * 1024 * 1024 * 1024;
        (*info).total_used_memory_size = program + stacks + heap;
        (*info).total_memory_heap_size = heap;
        (*info).allocated_memory_heap_size = allocated as u64;
        (*info).program_size = program;
        (*info).total_thread_stack_size = stacks;
        (*info).thread_count = 1;
    }
}

pub(crate) unsafe extern "C" fn allocate_address_region(
    out_address: *mut usize,
    size: usize,
) -> i32 {
    let aligned_size = (size + REGION_ALIGN - 1) & !(REGION_ALIGN - 1);
    let aligned_addr = match region_arena_alloc(aligned_size) {
        Some(addr) => {
            if region_arena_commit(addr, aligned_size).is_err() {
                region_arena_free(addr, aligned_size);
                println!(
                    "[!] HLE: nn::os::AllocateAddressRegion(size={}) FAILED",
                    size
                );
                return -1;
            }
            addr
        }
        None => {
            // Arena exhausted (or unreserved): an aligned region anywhere. Windows reservations are already 64 KiB
            // aligned, so `addr` is the reservation's start there, as release_address_region needs.
            println!(
                "[!] HLE: address region arena exhausted; mapping {} bytes outside it",
                aligned_size
            );
            let region = crate::platform::os::reserve(crate::platform::os::At::Anywhere, aligned_size + REGION_ALIGN).and_then(|raw| {
                let addr = (raw + REGION_ALIGN - 1) & !(REGION_ALIGN - 1);
                crate::platform::os::commit(addr, aligned_size, crate::platform::os::Prot::ReadWrite).map(|()| addr)
            });
            match region {
                Ok(addr) => addr,
                Err(_) => {
                    println!(
                        "[!] HLE: nn::os::AllocateAddressRegion(size={}) FAILED",
                        size
                    );
                    return -1;
                }
            }
        }
    };
    if !out_address.is_null() {
        *out_address = aligned_addr;
    }
    address_regions()
        .lock()
        .unwrap()
        .insert(aligned_addr, aligned_size);
    println!(
        "[+] HLE: nn::os::AllocateAddressRegion(size={} bytes, aligned={}) -> 0x{:x}",
        size, aligned_size, aligned_addr
    );
    0
}

pub(crate) unsafe extern "C" fn allocate_memory_pages(address: usize, size: usize) -> i32 {
    let page_size = HOST_PAGE;
    let page_start = address & !(page_size - 1);
    let page_end = (address + size + page_size - 1) & !(page_size - 1);
    let page_len = page_end - page_start;
    // These pages are made writable and zeroed below without faulting: tell write tracking.
    crate::guest::dirty_track::before_host_write(page_start, page_len);
    if let Err(err) = crate::platform::os::protect(page_start, page_len, crate::platform::os::Prot::ReadWrite) {
        println!(
            "[!] HLE: nn::os::AllocateMemoryPages(addr=0x{:x}, size={}) FAILED: {}",
            address, size, err
        );
        return -1;
    }
    // Horizon maps fresh, zeroed pages. Guest pages are 4KB but host pages 16KB, so zero exactly the newly
    // mapped 4KB pages and never touch neighbours that are still in use.
    let mut mapped = mapped_guest_pages().lock().unwrap();
    let mut fresh = 0usize;
    for page in (address / GUEST_PAGE)..((address + size + GUEST_PAGE - 1) / GUEST_PAGE) {
        if mapped.insert(page) {
            libc::memset((page * GUEST_PAGE) as *mut libc::c_void, 0, GUEST_PAGE);
            fresh += 1;
        }
    }
    drop(mapped);
    println!(
        "[+] HLE: nn::os::AllocateMemoryPages(addr=0x{:x}, size={} bytes, {} fresh 4KB pages)",
        address, size, fresh
    );
    0
}

/// nn::os::GenerateRandomBytes(void*, size_t): bytes from the host's random source (the SDK's version seeds its
/// generator from a syscall).
pub(crate) unsafe extern "C" fn generate_random_bytes(buf: *mut u8, size: usize) {
    if !buf.is_null() && size != 0 {
        crate::platform::os::random_bytes(std::slice::from_raw_parts_mut(buf, size));
    }
}

/// SetMemoryPermission(address, size, MemoryPermission): accepted, not enforced. The pages live in the region arena,
/// where write tracking and the GPU guards set host page protection themselves (16 KiB pages, the guest's are 4 KiB);
/// a game would only notice if it relied on faulting on its own pages.
pub(crate) unsafe extern "C" fn set_memory_permission(address: usize, size: usize, permission: u32) {
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| {
        println!("[+] HLE: nn::os::SetMemoryPermission(addr=0x{address:x}, size={size}, perm={permission}): not enforced");
    });
}

pub(crate) unsafe extern "C" fn free_address_region(address: usize) {
    let size = address_regions().lock().unwrap().remove(&address);
    if let Some(size) = size {
        let mut mapped = mapped_guest_pages().lock().unwrap();
        for page in (address / GUEST_PAGE)..((address + size) / GUEST_PAGE) {
            mapped.remove(&page);
        }
        drop(mapped);
        // GPU work recorded before this free may still read the memory through zero-copy Metal buffers; unmapping it
        // now would leave those buffers pointing at nothing (GPU fault). Release it a few frames later instead.
        crate::nvn::api::guest_memory_released(address, size);
        crate::nvn::api::PENDING_UNMAPS.lock().unwrap().push((
            address,
            size,
            crate::gpu::backend().presented_frames(),
        ));
    }
    println!("[+] HLE: nn::os::FreeAddressRegion(addr=0x{:x})", address);
}

pub(crate) unsafe extern "C" fn free_memory_pages(address: usize, size: usize) {
    // Guest pages are 4KB, host pages 16KB: only hand a host page back to the kernel once all four guest
    // pages in it are free. Discarding a partially freed host page would zero live neighbouring data.
    let mut mapped = mapped_guest_pages().lock().unwrap();
    for page in (address / GUEST_PAGE)..((address + size + GUEST_PAGE - 1) / GUEST_PAGE) {
        mapped.remove(&page);
    }
    let per_host = HOST_PAGE / GUEST_PAGE;
    let first_host = address / HOST_PAGE;
    let last_host = (address + size + HOST_PAGE - 1) / HOST_PAGE;
    let mut released = 0usize;
    for host in first_host..last_host {
        let base = host * per_host;
        if (base..base + per_host).all(|p| !mapped.contains(&p)) {
            crate::platform::os::discard(host * HOST_PAGE, HOST_PAGE);
            // File-backed: the file's block goes too (madvise leaves file pages in place).
            #[cfg(target_vendor = "apple")]
            if in_region_arena(host * HOST_PAGE) {
                if let Some((fd, off)) = arena_file(host * HOST_PAGE) {
                    crate::platform::os::punch_hole(fd, off, HOST_PAGE);
                }
            }
            released += 1;
        }
    }
    drop(mapped);
    println!(
        "[+] HLE: nn::os::FreeMemoryPages(addr=0x{:x}, size={} bytes, {} host pages released)",
        address, size, released
    );
}

// 10. Stack, Core, Mutex TryLock, and ReaderWriterLock
pub(crate) unsafe extern "C" fn get_current_stack_info(
    out_stack: *mut usize,
    out_stack_size: *mut usize,
) {
    let (lo, hi) = crate::horizon::thread_stack::current_bounds().unwrap_or_else(crate::platform::os::current_thread_stack);
    if !out_stack.is_null() {
        *out_stack = lo;
    }
    if !out_stack_size.is_null() {
        *out_stack_size = hi - lo;
    }
}
