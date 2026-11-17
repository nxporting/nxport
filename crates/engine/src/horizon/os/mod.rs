// nn::os as the game calls it: threads, synchronization (mutexes, events, semaphores, condition variables, message
// queues), TLS, memory and time.

use std::sync::Mutex;
use crate::horizon::vi::*;
use crate::horizon::os::objects::*;
use crate::horizon::call_log::*;
use crate::horizon::os::memory::*;
use crate::horizon::hle::*;
pub(crate) mod objects;
pub(crate) mod memory;
pub(crate) mod threads;
use threads::*;
use std::collections::{HashMap, VecDeque};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::Mutex as StdMutex;

pub(crate) unsafe extern "C" fn get_current_thread() -> *mut NnThreadType {
    CURRENT_NN_THREAD.with(|cell| {
        let mut ptr = *cell.borrow();
        if ptr.is_null() {
            ptr = THREAD_SELF_STRUCT.with(|s| s.as_ptr());
            let handle = ThreadTail::of(ptr).handle();
            if *handle == 0 {
                *handle = 1;
            }
            // Populate the REAL host stack bounds at the REAL field offsets
            // (_stack/_stackSize), not a placeholder. See struct doc comment.
            let (lo, hi) = super::thread_stack::current_bounds().unwrap_or_else(crate::platform::os::current_thread_stack);
            (*ptr).stack = lo as *mut ();
            (*ptr).original_stack = (*ptr).stack;
            (*ptr).stack_size = hi - lo;
            *cell.borrow_mut() = ptr;
        }
        setup_horizon_tpidrro(ptr);
        ptr
    })
}

pub(crate) unsafe extern "C" fn initialize_mutex(
    mutex: *mut NnMutex,
    recursive: bool,
    level: i32,
) {
    if (mutex as usize) >= 0x10000 {
        (*mutex).state = 1;
        (*mutex).is_recursive = recursive;
        (*mutex)._pad1 = [0; 2];
        (*mutex).lock_level = level;
        (*mutex).nest_count = 0;
        (*mutex)._pad2 = [0; 4];
        (*mutex).owner_thread = std::ptr::null_mut();
        (*mutex).lock = 0;
        (*mutex)._pad3 = 0;
    }
}

pub(crate) unsafe extern "C" fn lock_mutex(mutex: *mut NnMutex) {
    use std::sync::atomic::Ordering;
    if (mutex as usize) < 0x10000 {
        return;
    }
    let cur = crate::horizon::os::get_current_thread();
    if (*mutex).owner_thread == cur && (*mutex).nest_count > 0 {
        if !(*mutex).is_recursive {
            // Real hardware asserts here. Treat as recursive so we can see who does it.
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                let lr = crate::guest_reg!(x30);
                eprintln!("[!] nn::os::LockMutex: non-recursive mutex {:p} relocked by its owner (guest_lr={:#x}); treating as recursive", mutex, lr);
            }
        }
        (*mutex).nest_count += 1;
        return;
    }
    let word = mutex_lock_word(mutex);
    let tok = mutex_token(cur);
    if word.compare_exchange(0, tok, Ordering::Acquire, Ordering::Relaxed).is_err() {
        mutex_lock_contended(word, tok);
    }
    (*mutex).owner_thread = cur;
    (*mutex).nest_count = 1;
}

pub(crate) unsafe extern "C" fn unlock_mutex(mutex: *mut NnMutex) {
    if (mutex as usize) < 0x10000 {
        return;
    }
    let cur = crate::horizon::os::get_current_thread();
    if (*mutex).owner_thread != cur {
        return;
    }
    (*mutex).nest_count -= 1;
    if (*mutex).nest_count == 0 {
        (*mutex).owner_thread = std::ptr::null_mut();
        let word = mutex_lock_word(mutex);
        if word.swap(0, std::sync::atomic::Ordering::Release) & MUTEX_WAITERS != 0 {
            mutex_unpark(word);
        }
    }
}

pub(crate) unsafe extern "C" fn try_lock_mutex(mutex: *mut NnMutex) -> bool {
    use std::sync::atomic::Ordering;
    if (mutex as usize) < 0x10000 {
        return false;
    }
    let cur = crate::horizon::os::get_current_thread();
    if (*mutex).is_recursive && (*mutex).owner_thread == cur && (*mutex).nest_count > 0 {
        (*mutex).nest_count += 1;
        return true;
    }
    if mutex_lock_word(mutex)
        .compare_exchange(0, mutex_token(cur), Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        (*mutex).owner_thread = cur;
        (*mutex).nest_count = 1;
        true
    } else {
        false
    }
}

pub(crate) unsafe extern "C" fn finalize_mutex(mutex: *mut NnMutex) {
    if (mutex as usize) >= 0x10000 {
        (*mutex).state = 0;
        (*mutex).owner_thread = std::ptr::null_mut();
        (*mutex).nest_count = 0;
        (*mutex).lock = 0;
    }
}

pub(crate) unsafe extern "C" fn is_mutex_locked_by_current_thread(mutex: *const NnMutex) -> bool {
    if (mutex as usize) >= 0x10000 {
        let cur = crate::horizon::os::get_current_thread();
        (*mutex).owner_thread == cur && (*mutex).nest_count > 0
    } else {
        false
    }
}

pub(crate) extern "C" fn get_system_tick() -> u64 {
    crate::platform::clock::ticks()
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

pub(crate) unsafe extern "C" fn set_memory_allocator_for_thread_local(
    _alloc: *const (),
    _free: *const (),
) {
    println!("[+] HLE: nn::os::SetMemoryAllocatorForThreadLocal called");
}

pub(crate) unsafe extern "C" fn allocate_tls_slot(out_slot: *mut u32, _dtor: *const ()) -> i32 {
    let slot = NEXT_TLS_SLOT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if !out_slot.is_null() {
        *out_slot = slot;
    }
    println!("[+] HLE: nn::os::AllocateTlsSlot() -> slot {}", slot);
    0
}

pub(crate) unsafe extern "C" fn set_tls_value(slot: u32, val: u64) {
    if (slot as usize) < 64 {
        THREAD_TLS_VALUES.with(|cell| {
            cell.borrow_mut()[slot as usize] = val;
        });
    }
}

pub(crate) unsafe extern "C" fn get_tls_value(slot: u32) -> u64 {
    if (slot as usize) < 64 {
        THREAD_TLS_VALUES.with(|cell| cell.borrow()[slot as usize])
    } else {
        0
    }
}

pub(crate) unsafe extern "C" fn free_tls_slot(_slot: u32) {}

pub(crate) unsafe extern "C" fn sleep_thread(nano: i64) {
    sanitize_darwin_tpidrro();
    if nano > 0 {
        hle_sleep_ns(nano as u64);
    }
    if nano > 1_000_000 && log_timestamps() && std::thread::current().name() == Some("guest-main") {
        let now = crate::platform::clock::now_ms();
        println!(
            "[ts] sleep {:.1} ms ended t {:.3} ms",
            nano as f64 / 1e6,
            now
        );
    }
}

pub(crate) unsafe extern "C" fn yield_thread() {
    // A few pauses, not SwitchToThread. The console yields to the game's own ready threads on that core, and TotK's job
    // threads poll with yields hundreds of thousands of times a second. Windows yields to whatever is ready on the
    // processor: with all of them busy that is another process, for its whole quantum (~10 ms a yield, ~1 fps). The
    // game's threads that are ready get a processor from the scheduler anyway; an idle machine's SwitchToThread
    // returned at once (0.1-0.2 us), about what these pauses take.
    for _ in 0..8 {
        std::hint::spin_loop();
    }
}

pub(crate) unsafe extern "C" fn convert_to_time_span(tick: i64) -> i64 {
    crate::platform::clock::ticks_to_ns(tick.max(0) as u64) as i64
}

pub(crate) unsafe extern "C" fn create_thread(
    thread: *mut NnThreadType,
    entry: unsafe extern "C" fn(*mut ()),
    arg: *mut (),
    stack: *mut (),
    stack_size: usize,
    priority: i32,
) -> i32 {
    println!(
        "[+] HLE: nn::os::CreateThread(thread={:p}, entry={:#x}, arg={:p}, stack={:p}, size={}, prio={})",
        thread, entry as usize, arg, stack, stack_size, priority
    );
    if !stack.is_null() && stack_size > 0 {
        libc::memset(stack as *mut libc::c_void, 0, stack_size);
    }
    if !thread.is_null() {
        *ThreadTail::of(thread).handle() = NEXT_THREAD_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        (*thread).state = THREAD_STATE_INITIALIZED;
        (*thread).base_priority = priority;
        (*thread).original_stack = stack;
        (*thread).stack = stack;
        (*thread).stack_size = stack_size;
        (*thread).thread_function = Some(entry);
        (*thread).argument = arg;
        // nn::os::CreateThread starts every thread on its own stack, not on a fiber.
        (*thread).current_fiber = std::ptr::null_mut();
        (*thread).initial_fiber = std::ptr::null_mut();
    }
    0
}

pub(crate) unsafe extern "C" fn set_thread_name(thread: *mut NnThreadType, name: *const c_char) {
    if !thread.is_null() && !name.is_null() {
        let name_str = CStr::from_ptr(name).to_string_lossy();
        println!(
            "[+] HLE: nn::os::SetThreadName(thread={:p}, name='{}')",
            thread, name_str
        );
        let bytes = name_str.as_bytes();
        let len = bytes.len().min(31);
        let tail = ThreadTail::of(thread);
        libc::memcpy(tail.name_buffer() as *mut libc::c_void, bytes.as_ptr() as *const libc::c_void, len);
        *tail.name_buffer().add(len) = 0;
        *tail.name_pointer() = tail.name_buffer() as *const c_char;
    }
}

/// nn::os::SetThreadNamePointer / GetThreadNamePointer: the name is the caller's string, kept by pointer.
pub(crate) unsafe extern "C" fn set_thread_name_pointer(
    thread: *mut NnThreadType,
    name: *const c_char,
) {
    let t = if thread.is_null() {
        crate::horizon::os::get_current_thread() as *mut NnThreadType
    } else {
        thread
    };
    if !t.is_null() {
        *ThreadTail::of(t).name_pointer() = name;
    }
}

pub(crate) unsafe extern "C" fn get_thread_name_pointer(
    thread: *const NnThreadType,
) -> *const c_char {
    let t = if thread.is_null() {
        crate::horizon::os::get_current_thread() as *const NnThreadType
    } else {
        thread
    };
    if t.is_null() {
        std::ptr::null()
    } else {
        *ThreadTail::of(t).name_pointer()
    }
}

pub(crate) unsafe extern "C" fn start_thread(thread: *mut NnThreadType) {
    if thread.is_null() {
        return;
    }
    let name_bytes = std::slice::from_raw_parts(ThreadTail::of(thread).name_buffer(), 32);
    let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(32);
    let name_str = String::from_utf8_lossy(&name_bytes[..name_end]).to_string();
    println!(
        "[+] HLE: nn::os::StartThread(thread={:p}, name='{}')",
        thread, name_str
    );

    (*thread).state = THREAD_STATE_STARTED;

    let entry = match (*thread).thread_function {
        Some(f) => f,
        None => {
            println!("    [!] StartThread: no entry_fn set, not spawning thread");
            return;
        }
    };
    let arg = (*thread).argument;
    let exit = thread_exit_signal(thread, true).unwrap();

    // Cast raw pointers to usize to make them Send (guest memory is shared globally)
    let thread_usize = thread as usize;
    let arg_usize = arg as usize;
    let entry_usize = entry as usize;
    let guest_stack = ((*thread).stack as usize, (*thread).stack_size);

    // Spawn a real OS thread. The guest binary is natively compiled AArch64 code
    // so we can call the entry function pointer directly from a Rust thread.
    std::thread::Builder::new()
        .name(name_str.clone())
        // The thread's own stack is where its HLE calls run (thread_stack.rs): they call into Metal and other system
        // libraries, and compiling a render pipeline (synchronous, on the calling thread) took the Presentation Thread
        // past 512 KiB in long play sessions.
        .stack_size(8 << 20)
        .spawn(move || {
            crate::guest::tls::mark_guest_thread();
            println!("[+] HLE: Guest thread '{}' starting execution", name_str);
            // The threads on the frame's critical path (all of our Metal encoding runs on the Presentation Thread)
            // ask for performance cores: with the default QoS macOS may run them on efficiency cores.
            #[cfg(target_vendor = "apple")]
            if name_str == "Presentation Thread" || name_str == "guest-main" {
                extern "C" {
                    fn pthread_set_qos_class_self_np(qos: u32, relpri: i32) -> i32;
                }
                const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
                let r = pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
                println!(
                    "[+] HLE: thread '{}' QoS user-interactive (rc {})",
                    name_str, r
                );
            }
            #[cfg(nx_native)]
            crate::guest::exec::crash_handler::ensure_altstack();
            // Every guest thread reads tpidr_el0 directly for thread_local /
            // SDK per-thread state -- must be installed before any guest code
            // (including code reached indirectly through HLE calls below) runs
            // on this OS thread. See guest/exec/tls.rs.
            #[cfg(nx_native)]
            {
                crate::guest::exec::tls::install_guest_tls();
                crate::guest::exec::trampoline::init_worker_thread_x18();
            }
            let thread_ptr = thread_usize as *mut NnThreadType;
            // Initialize this thread's TLS self-pointer
            CURRENT_NN_THREAD.with(|cell| {
                *cell.borrow_mut() = thread_ptr;
            });
            // Guest code runs on the game's stack buffer, as on Horizon (thread_stack.rs).
            let (stack_lo, stack_hi) = if guest_stack.0 != 0 && guest_stack.1 != 0 {
                (guest_stack.0, guest_stack.0 + guest_stack.1)
            } else {
                super::thread_stack::allocate(1 << 20)
            };
            let handle = ThreadTail::of(thread_ptr).handle();
            if *handle == 0 {
                *handle = NEXT_THREAD_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            setup_horizon_tpidrro(thread_ptr);
            super::thread_stack::run(stack_lo, stack_hi, entry_usize, arg_usize);
            println!("[+] HLE: Guest thread '{}' ({:p}) returned", name_str, thread_ptr);
            (*thread_ptr).state = THREAD_STATE_EXITED;
            let (done, cv) = &*exit;
            *done.lock().unwrap_or_else(|e| e.into_inner()) = true;
            cv.notify_all();
        })
        .expect("failed to spawn guest thread");
}

pub(crate) unsafe extern "C" fn wait_thread(thread: *mut NnThreadType) {
    if !thread.is_null() {
        join_guest_thread(thread);
    }
}

pub(crate) unsafe extern "C" fn try_wait_thread(thread: *mut NnThreadType) -> bool {
    match thread_exit_signal(thread, false) {
        Some(sig) => *sig.0.lock().unwrap_or_else(|e| e.into_inner()),
        None => false,
    }
}

pub(crate) unsafe extern "C" fn destroy_thread(thread: *mut NnThreadType) {
    if !thread.is_null() {
        join_guest_thread(thread);
        THREAD_EXITS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(std::collections::HashMap::new)
            .remove(&(thread as usize));
        (*thread).state = THREAD_STATE_NOT_INITIALIZED;
    }
}

pub(crate) unsafe extern "C" fn get_thread_id(thread: *const NnThreadType) -> u64 {
    if !thread.is_null() {
        *ThreadTail::of(thread).handle() as u64
    } else {
        1
    }
}

pub(crate) unsafe extern "C" fn change_thread_priority(thread: *mut NnThreadType, prio: i32) {
    if !thread.is_null() {
        (*thread).base_priority = prio;
    }
}

pub(crate) unsafe extern "C" fn get_thread_priority(thread: *const NnThreadType) -> i32 {
    if !thread.is_null() {
        (*thread).base_priority
    } else {
        16
    }
}

pub(crate) unsafe extern "C" fn get_thread_available_core_mask() -> u64 {
    0b111
}

/// nn::os::CreateThread(..., priority, idealCore): the 6-argument version plus the core the thread
/// starts on, which GetCurrentCoreNumber reports (see current_core).
pub(crate) unsafe extern "C" fn create_thread_with_core(
    thread: *mut NnThreadType,
    entry: unsafe extern "C" fn(*mut ()),
    arg: *mut (),
    stack: *mut (),
    stack_size: usize,
    priority: i32,
    ideal_core: i32,
) -> i32 {
    let r = crate::horizon::os::create_thread(thread, entry, arg, stack, stack_size, priority);
    if r == 0 && ideal_core >= 0 && !thread.is_null() {
        thread_cores()
            .lock()
            .unwrap()
            .insert(thread as usize, ideal_core);
        CORES_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    r
}

pub(crate) unsafe extern "C" fn set_thread_core_mask(
    thread: *mut NnThreadType,
    ideal_core: i32,
    affinity: u64,
) {
    // ideal_core < 0 (CoreId_Default / "any"): run on the lowest core the affinity allows.
    let core = if ideal_core >= 0 {
        ideal_core
    } else if affinity != 0 {
        affinity.trailing_zeros() as i32
    } else {
        DEFAULT_CORE
    };
    let t = if thread.is_null() {
        crate::horizon::os::get_current_thread() as usize
    } else {
        thread as usize
    };
    thread_cores().lock().unwrap().insert(t, core);
    CORES_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    println!(
        "[+] HLE: nn::os::SetThreadCoreMask(thread={:#x}, ideal={}, affinity={:#x}) -> core {}",
        t, ideal_core, affinity, core
    );
}

/// nn::os::GetThreadCoreMask(int* idealCore, Bit64* affinity, const ThreadType*): the core recorded by CreateThread /
/// SetThreadCoreMask (current_core's table); the affinity is that core alone.
pub(crate) unsafe extern "C" fn get_thread_core_mask(
    out_core: *mut i32,
    out_mask: *mut u64,
    thread: *const NnThreadType,
) {
    let t = if thread.is_null() {
        crate::horizon::os::get_current_thread() as usize
    } else {
        thread as usize
    };
    let core = thread_cores()
        .lock()
        .unwrap()
        .get(&t)
        .copied()
        .unwrap_or(DEFAULT_CORE);
    if !out_core.is_null() {
        *out_core = core;
    }
    if !out_mask.is_null() {
        *out_mask = 1u64 << core;
    }
}

pub(crate) unsafe extern "C" fn query_memory_info(info: *mut NnMemoryInfo) {
    // Every field: games read more than the first two (only those used to be written; the rest was stack garbage, and
    // Super Mario Bros. Wonder then asked for a negative-size heap block).
    if !info.is_null() {
        // Used memory as Horizon counts it: the program image, the main thread's stack and the memory heap once its size
        // is set. Super Mario Bros. Wonder aborts at boot unless what is used before its heap fits its 85 MB "Program"
        // budget (the old flat 512 MB did not), then sizes the heap as available - used - its reserve.
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

pub(crate) unsafe extern "C" fn get_system_tick_frequency() -> u64 {
    crate::platform::clock::frequency()
}

// 10. Stack, Core, Mutex TryLock, and ReaderWriterLock
pub(crate) unsafe extern "C" fn get_current_stack_info(
    out_stack: *mut usize,
    out_stack_size: *mut usize,
) {
    let (lo, hi) = super::thread_stack::current_bounds().unwrap_or_else(crate::platform::os::current_thread_stack);
    if !out_stack.is_null() {
        *out_stack = lo;
    }
    if !out_stack_size.is_null() {
        *out_stack_size = hi - lo;
    }
}

pub(crate) unsafe extern "C" fn initialize_reader_writer_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_init(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn finalize_reader_writer_lock(_rwlock: *mut NnReaderWriterLock) {}

pub(crate) unsafe extern "C" fn acquire_write_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_acquire_write(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn release_write_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_release_write(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn acquire_read_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_acquire_read(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn release_read_lock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::os::objects::rw_release_read(rwlock as usize);
    }
}

pub(crate) unsafe extern "C" fn is_write_lock_held_by_current_thread(
    rwlock: *const NnReaderWriterLock,
) -> bool {
    !rwlock.is_null() && crate::horizon::os::objects::rw_write_held_by_me(rwlock as usize)
}

pub(crate) unsafe extern "C" fn initialize_light_message_queue(
    mq: *mut (),
    _buf: *mut u64,
    count: usize,
) {
    light_mq_table()
        .lock()
        .unwrap()
        .insert(mq as usize, light_mq_new(count));
}

pub(crate) unsafe extern "C" fn finalize_light_message_queue(mq: *mut ()) {
    light_mq_table().lock().unwrap().remove(&(mq as usize));
}

pub(crate) unsafe extern "C" fn send_light_message_queue(mq: *mut (), data: u64) {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    while inner.queue.len() >= inner.capacity {
        inner = state.not_full.wait(inner).unwrap();
    }
    inner.queue.push_back(data);
    state.not_empty.notify_all();
}

pub(crate) unsafe extern "C" fn try_send_light_message_queue(mq: *mut (), data: u64) -> bool {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    if inner.queue.len() >= inner.capacity {
        return false;
    }
    inner.queue.push_back(data);
    state.not_empty.notify_all();
    true
}

pub(crate) unsafe extern "C" fn jam_light_message_queue(mq: *mut (), data: u64) {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    while inner.queue.len() >= inner.capacity {
        inner = state.not_full.wait(inner).unwrap();
    }
    inner.queue.push_front(data);
    state.not_empty.notify_all();
}

pub(crate) unsafe extern "C" fn try_jam_light_message_queue(mq: *mut (), data: u64) -> bool {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    if inner.queue.len() >= inner.capacity {
        return false;
    }
    inner.queue.push_front(data);
    state.not_empty.notify_all();
    true
}

pub(crate) unsafe extern "C" fn receive_light_message_queue(out: *mut u64, mq: *mut ()) {
    let fp = crate::guest_reg!(x29);
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    let _blocked = if inner.queue.is_empty() {
        Some(BlockGuard::new("MsgQueueRecv", mq as usize, fp))
    } else {
        None
    };
    while inner.queue.is_empty() {
        inner = state.not_empty.wait(inner).unwrap();
    }
    let v = inner.queue.pop_front().unwrap();
    state.not_full.notify_one();
    drop(inner);
    if !out.is_null() {
        *out = v;
    }
    present_paced_wait();
}

pub(crate) unsafe extern "C" fn try_receive_light_message_queue(out: *mut u64, mq: *mut ()) -> bool {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    match inner.queue.pop_front() {
        Some(v) => {
            state.not_full.notify_one();
            if !out.is_null() {
                *out = v;
            }
            true
        }
        None => false,
    }
}

pub(crate) unsafe extern "C" fn try_peek_light_message_queue(out: *mut u64, mq: *const ()) -> bool {
    let state = light_mq_get(mq as *mut ());
    let inner = state.inner.lock().unwrap();
    match inner.queue.front() {
        Some(&v) => {
            if !out.is_null() {
                *out = v;
            }
            true
        }
        None => false,
    }
}

pub(crate) unsafe extern "C" fn peek_light_message_queue(out: *mut u64, mq: *const ()) {
    let state = light_mq_get(mq as *mut ());
    let mut inner = state.inner.lock().unwrap();
    while inner.queue.is_empty() {
        inner = state.not_empty.wait(inner).unwrap();
    }
    if !out.is_null() {
        *out = inner.queue[0];
    }
}

pub(crate) unsafe extern "C" fn timed_send_light_message_queue(mq: *mut (), data: u64, timeout_ns: i64) -> bool {
    light_mq_timed_put(mq, data, false, timeout_ns)
}

pub(crate) unsafe extern "C" fn timed_jam_light_message_queue(mq: *mut (), data: u64, timeout_ns: i64) -> bool {
    light_mq_timed_put(mq, data, true, timeout_ns)
}

pub(crate) unsafe extern "C" fn timed_receive_light_message_queue(out: *mut u64, mq: *mut (), timeout_ns: i64) -> bool {
    let state = light_mq_get(mq);
    let inner = state.inner.lock().unwrap();
    let (mut inner, _) = state
        .not_empty
        .wait_timeout_while(inner, std::time::Duration::from_nanos(timeout_ns.max(0) as u64), |i| i.queue.is_empty())
        .unwrap();
    match inner.queue.pop_front() {
        Some(v) => {
            state.not_full.notify_one();
            if !out.is_null() {
                *out = v;
            }
            true
        }
        None => false,
    }
}

pub(crate) unsafe extern "C" fn timed_peek_light_message_queue(
    out: *mut u64,
    mq: *const (),
    timeout_ns: i64,
) -> bool {
    let state = light_mq_get(mq as *mut ());
    let mut inner = state.inner.lock().unwrap();
    if inner.queue.is_empty() && timeout_ns > 0 {
        let (guard, _timed_out) = state
            .not_empty
            .wait_timeout(inner, std::time::Duration::from_nanos(timeout_ns as u64))
            .unwrap();
        inner = guard;
    }
    match inner.queue.front() {
        Some(&v) => {
            if !out.is_null() {
                *out = v;
            }
            true
        }
        None => false,
    }
}

pub(crate) unsafe extern "C" fn initialize_condition_variable(cv: *mut ()) {
    condvar_table()
        .lock()
        .unwrap()
        .insert(cv as usize, condvar_new());
}

pub(crate) unsafe extern "C" fn finalize_condition_variable(cv: *mut ()) {
    condvar_table().lock().unwrap().remove(&(cv as usize));
}

pub(crate) unsafe extern "C" fn signal_condition_variable(cv: *mut ()) {
    let state = condvar_get(cv);
    {
        let mut gen = state.generation.lock().unwrap();
        *gen = gen.wrapping_add(1);
    }
    state.cv.notify_all();
}

/// Signal already wakes every waiter (generation counter), so Broadcast is the same operation.
pub(crate) unsafe extern "C" fn broadcast_condition_variable(cv: *mut ()) {
    crate::horizon::os::signal_condition_variable(cv);
}

/// nn::os::TimedWaitConditionVariable: returns ConditionVariableStatus (Timeout = 0, NoTimeout = 1).
pub(crate) unsafe extern "C" fn timed_wait_condition_variable(
    cv: *mut (),
    mutex: *mut NnMutex,
    timeout_ns: i64,
) -> i32 {
    let state = condvar_get(cv);
    let seen_gen = *state.generation.lock().unwrap();
    crate::horizon::os::unlock_mutex(mutex);
    let timed_out = {
        let guard = state.generation.lock().unwrap();
        let timeout = std::time::Duration::from_nanos(timeout_ns.max(0) as u64);
        state
            .cv
            .wait_timeout_while(guard, timeout, |gen| *gen == seen_gen)
            .unwrap()
            .1
            .timed_out()
    };
    crate::horizon::os::lock_mutex(mutex);
    if timed_out {
        0
    } else {
        1
    }
}

pub(crate) unsafe extern "C" fn wait_condition_variable(cv: *mut (), mutex: *mut NnMutex) {
    let fp = crate::guest_reg!(x29);
    let state = condvar_get(cv);
    let seen_gen = *state.generation.lock().unwrap();

    // Real nn::os semantics: atomically release the (already-held) guest
    // mutex and block until signaled, then re-acquire it before returning.
    // The guest mutex isn't the same lock as our internal Condvar's, so we
    // can't do this in one atomic step the way Rust's Condvar::wait(guard)
    // does with its own Mutex -- release, wait on our own generation-counted
    // condvar, then re-lock, matching the same net effect.
    crate::horizon::os::unlock_mutex(mutex);
    {
        let _blocked = BlockGuard::new("CondVar", cv as usize, fp);
        let guard = state.generation.lock().unwrap();
        let _final_guard = state.cv.wait_while(guard, |gen| *gen == seen_gen).unwrap();
    }
    crate::horizon::os::lock_mutex(mutex);
}

pub(crate) unsafe extern "C" fn initialize_light_event(
    ev: *mut (),
    initially_signaled: bool,
    clear_mode: i32,
) {
    // In place when the object already has state: a thread may already be waiting on it (a wait before the
    // initialization creates the state lazily), and a replaced state would never see the signals meant for it.
    let mut table = light_event_table().lock().unwrap();
    match table.get(&(ev as usize)) {
        Some(st) => {
            let st = st.clone();
            drop(table);
            st.auto_clear
                .store(clear_mode == 1, std::sync::atomic::Ordering::Relaxed);
            let mut g = st.signaled.lock().unwrap();
            *g = initially_signaled;
            if initially_signaled {
                st.signals.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                st.cv.notify_all();
            }
        }
        None => {
            table.insert(ev as usize, light_event_new(initially_signaled, clear_mode));
        }
    }
}

pub(crate) unsafe extern "C" fn finalize_light_event(ev: *mut ()) {
    light_event_table().lock().unwrap().remove(&(ev as usize));
}

pub(crate) unsafe extern "C" fn signal_light_event(ev: *mut ()) {
    let state = light_event_get(ev);
    {
        let mut g = state.signaled.lock().unwrap();
        *g = true;
        state.signals.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if state.auto_clear.load(std::sync::atomic::Ordering::Relaxed) {
        state.cv.notify_one();
    } else {
        state.cv.notify_all();
    }
}

pub(crate) unsafe extern "C" fn clear_light_event(ev: *mut ()) {
    let state = light_event_get(ev);
    *state.signaled.lock().unwrap() = false;
}

pub(crate) unsafe extern "C" fn wait_light_event(ev: *mut ()) {
    let fp = crate::guest_reg!(x29);
    let state = light_event_get(ev);
    light_event_spin(&state);
    let mut guard = state.signaled.lock().unwrap();
    let _blocked = if *guard {
        None
    } else {
        Some(BlockGuard::new("LightEvent", ev as usize, fp))
    };
    let started = std::time::Instant::now();
    let mut reported = false;
    // Manual clear: a signal while waiting releases the waiter even if the event was cleared again since.
    let manual = !state.auto_clear.load(std::sync::atomic::Ordering::Relaxed);
    let signals0 = state.signals.load(std::sync::atomic::Ordering::Relaxed);
    while !*guard && !(manual && state.signals.load(std::sync::atomic::Ordering::Relaxed) != signals0) {
        let (g, t) = state
            .cv
            .wait_timeout(guard, std::time::Duration::from_secs(5))
            .unwrap();
        guard = g;
        if t.timed_out() && !*guard && !reported && !(manual && state.signals.load(std::sync::atomic::Ordering::Relaxed) != signals0) {
            reported = true;
            report_long_wait("WaitLightEvent", ev as usize, fp, started.elapsed());
        }
    }
    if state.auto_clear.load(std::sync::atomic::Ordering::Relaxed) {
        *guard = false;
    }
}

pub(crate) unsafe extern "C" fn try_wait_light_event(ev: *mut ()) -> bool {
    let state = light_event_get(ev);
    let mut guard = state.signaled.lock().unwrap();
    let was = *guard;
    if was && state.auto_clear.load(std::sync::atomic::Ordering::Relaxed) {
        *guard = false;
    }
    was
}

pub(crate) unsafe extern "C" fn timed_wait_light_event(ev: *mut (), timeout_ns: i64) -> bool {
    let state = light_event_get(ev);
    let guard = state.signaled.lock().unwrap();
    let manual = !state.auto_clear.load(std::sync::atomic::Ordering::Relaxed);
    let signals0 = state.signals.load(std::sync::atomic::Ordering::Relaxed);
    let (mut guard, timeout_result) = state
        .cv
        .wait_timeout_while(
            guard,
            std::time::Duration::from_nanos(timeout_ns.max(0) as u64),
            |signaled| {
                !*signaled && !(manual && state.signals.load(std::sync::atomic::Ordering::Relaxed) != signals0)
            },
        )
        .unwrap();
    let was_signaled = !timeout_result.timed_out();
    if was_signaled && state.auto_clear.load(std::sync::atomic::Ordering::Relaxed) {
        *guard = false;
    }
    was_signaled
}

pub(crate) unsafe extern "C" fn initialize_timer_event(ev: *mut (), clear_mode: i32) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    libc::memset(p as *mut libc::c_void, 0, 0x40);
    *p.add(0x10) = 1; // _state = Initialized
    *p.add(0x11) = clear_mode as u8; // _clearMode
    *p.add(0x12) = 0; // _signaled = false
    *p.add(0x13) = 0; // _timerMode = 0 (stopped)

    timer_event_table()
        .lock()
        .unwrap()
        .insert(ev as usize, timer_event_new(clear_mode as u8));
}

pub(crate) unsafe extern "C" fn finalize_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x10) = 0; // _state = 0
    timer_event_table().lock().unwrap().remove(&(ev as usize));
}

pub(crate) unsafe extern "C" fn start_one_shot_timer_event(ev: *mut (), first_time: i64) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x13) = 1; // _timerMode = 1
    *(p.add(0x28) as *mut i64) = first_time;
    *(p.add(0x30) as *mut i64) = 0;

    let first_dur = std::time::Duration::from_nanos(first_time.max(0) as u64);
    let target = std::time::Instant::now() + first_dur;

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.timer_mode = 1;
        inner.first_time_ns = first_time;
        inner.interval_ns = 0;
        inner.next_wake = Some(target);
        inner.signaled = false;
    }
    state.cv.notify_all();
}

pub(crate) unsafe extern "C" fn start_periodic_timer_event(
    ev: *mut (),
    first_time: i64,
    interval: i64,
) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x13) = 2; // _timerMode = 2
    *(p.add(0x28) as *mut i64) = first_time;
    *(p.add(0x30) as *mut i64) = interval;

    let first_dur = std::time::Duration::from_nanos(first_time.max(0) as u64);
    let target = std::time::Instant::now() + first_dur;

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.timer_mode = 2;
        inner.first_time_ns = first_time;
        inner.interval_ns = interval;
        inner.next_wake = Some(target);
        inner.signaled = false;
    }
    state.cv.notify_all();
}

pub(crate) unsafe extern "C" fn stop_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x13) = 0; // _timerMode = 0
    *p.add(0x12) = 0; // _signaled = 0

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.timer_mode = 0;
        inner.next_wake = None;
        inner.signaled = false;
    }
    state.cv.notify_all();
}

pub(crate) unsafe extern "C" fn signal_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x12) = 1;

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.signaled = true;
    }
    state.cv.notify_all();
}

pub(crate) unsafe extern "C" fn clear_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x12) = 0;

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.signaled = false;
    }
}

pub(crate) unsafe extern "C" fn wait_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let fp = crate::guest_reg!(x29);
    let _blocked = BlockGuard::new("TimerEvent", ev as usize, fp);
    let p = ev as *mut u8;
    let state = timer_event_get(ev);
    let mut inner = state.inner.lock().unwrap();
    loop {
        if inner.signaled {
            if inner.clear_mode == 1 {
                inner.signaled = false;
                *p.add(0x12) = 0;
            }
            break;
        }
        match inner.timer_mode {
            0 => {
                inner = state.cv.wait(inner).unwrap();
            }
            1 => {
                if let Some(target) = inner.next_wake {
                    let now = std::time::Instant::now();
                    if now >= target {
                        inner.timer_mode = 0;
                        *p.add(0x13) = 0;
                        if inner.clear_mode == 1 {
                            inner.signaled = false;
                            *p.add(0x12) = 0;
                        }
                        break;
                    }
                    let timeout = target - now;
                    let (new_inner, timeout_res) = state.cv.wait_timeout(inner, timeout).unwrap();
                    inner = new_inner;
                    if timeout_res.timed_out() {
                        inner.timer_mode = 0;
                        *p.add(0x13) = 0;
                        if inner.clear_mode == 1 {
                            inner.signaled = false;
                            *p.add(0x12) = 0;
                        }
                        break;
                    }
                } else {
                    inner = state.cv.wait(inner).unwrap();
                }
            }
            2 => {
                if let Some(target) = inner.next_wake {
                    let now = std::time::Instant::now();
                    if now >= target {
                        let interval = std::time::Duration::from_nanos(
                            inner.interval_ns.max(1_000_000) as u64,
                        );
                        let mut next = target + interval;
                        while next <= now {
                            next += interval;
                        }
                        inner.next_wake = Some(next);
                        if inner.clear_mode == 1 {
                            inner.signaled = false;
                            *p.add(0x12) = 0;
                        }
                        break;
                    }
                    let timeout = target - now;
                    let (new_inner, timeout_res) = state.cv.wait_timeout(inner, timeout).unwrap();
                    inner = new_inner;
                    if timeout_res.timed_out() {
                        let interval = std::time::Duration::from_nanos(
                            inner.interval_ns.max(1_000_000) as u64,
                        );
                        let now = std::time::Instant::now();
                        let mut next = target + interval;
                        while next <= now {
                            next += interval;
                        }
                        inner.next_wake = Some(next);
                        if inner.clear_mode == 1 {
                            inner.signaled = false;
                            *p.add(0x12) = 0;
                        }
                        break;
                    }
                } else {
                    inner = state.cv.wait(inner).unwrap();
                }
            }
            _ => break,
        }
    }
}

pub(crate) unsafe extern "C" fn try_wait_timer_event(ev: *mut ()) -> bool {
    if (ev as usize) < 0x10000 {
        return false;
    }
    let p = ev as *mut u8;
    let state = timer_event_get(ev);
    let mut inner = state.inner.lock().unwrap();
    if inner.signaled {
        if inner.clear_mode == 1 {
            inner.signaled = false;
            *p.add(0x12) = 0;
        }
        return true;
    }
    if let Some(target) = inner.next_wake {
        let now = std::time::Instant::now();
        if now >= target {
            if inner.timer_mode == 1 {
                inner.timer_mode = 0;
                *p.add(0x13) = 0;
            } else if inner.timer_mode == 2 {
                let interval =
                    std::time::Duration::from_nanos(inner.interval_ns.max(1_000_000) as u64);
                let mut next = target + interval;
                while next <= now {
                    next += interval;
                }
                inner.next_wake = Some(next);
            }
            if inner.clear_mode == 1 {
                inner.signaled = false;
                *p.add(0x12) = 0;
            }
            return true;
        }
    }
    false
}

pub(crate) unsafe extern "C" fn try_acquire_semaphore(sem: *mut ()) -> bool {
    semaphore_try_acquire(sem, 0)
}

pub(crate) unsafe extern "C" fn timed_acquire_semaphore(sem: *mut (), timeout_ns: i64) -> bool {
    semaphore_try_acquire(sem, timeout_ns)
}

pub(crate) unsafe extern "C" fn get_current_semaphore_count(sem: *const ()) -> i32 {
    semaphore_count(sem)
}

pub(crate) unsafe extern "C" fn initialize_semaphore(sem: *mut (), initial: i32, max: i32) {
    semaphore_init(sem, initial, max);
}

pub(crate) unsafe extern "C" fn finalize_semaphore(sem: *mut ()) {
    semaphore_finalize(sem);
}

pub(crate) unsafe extern "C" fn acquire_semaphore(sem: *mut ()) {
    let fp = crate::guest_reg!(x29);
    semaphore_acquire(sem, fp);
}

pub(crate) unsafe extern "C" fn release_semaphore(sem: *mut (), count: i32) {
    semaphore_release(sem, count);
}

pub(crate) unsafe extern "C" fn initialize_light_semaphore(sem: *mut (), initial: i32, max: i32) {
    semaphore_init(sem, initial, max);
}

pub(crate) unsafe extern "C" fn finalize_light_semaphore(sem: *mut ()) {
    semaphore_finalize(sem);
}

pub(crate) unsafe extern "C" fn acquire_light_semaphore(sem: *mut ()) {
    let fp = crate::guest_reg!(x29);
    semaphore_acquire(sem, fp);
}

pub(crate) unsafe extern "C" fn release_light_semaphore(sem: *mut ()) {
    semaphore_release(sem, 1);
}

pub(crate) unsafe extern "C" fn try_wait_system_event(event: *mut ()) -> u8 {
    record_hle_call("os::try_wait_system_event");
    match hle_system_event_lookup(event as usize) {
        Some(ev) => std::mem::replace(&mut *ev.signaled.lock().unwrap(), false) as u8,
        None => {
            static SEEN: StdMutex<Vec<usize>> = StdMutex::new(Vec::new());
            let mut seen = SEEN.lock().unwrap();
            if !seen.contains(&(event as usize)) {
                seen.push(event as usize);
                eprintln!(
                    "[!] nn::os::TryWaitSystemEvent on unregistered event {:p} -> false",
                    event
                );
            }
            0
        }
    }
}

/// nn::os::TimedWaitSystemEvent(SystemEventType*, TimeSpan): WaitSystemEvent with a timeout (nanoseconds); true when
/// the event was signaled (and, as an auto-clear event, consumed).
pub(crate) unsafe extern "C" fn timed_wait_system_event(event: *mut (), timeout_ns: i64) -> bool {
    record_hle_call("os::timed_wait_system_event");
    if crate::settings::get().log.log_sysevent_waits {
        static SEEN: StdMutex<Vec<usize>> = StdMutex::new(Vec::new());
        let mut seen = SEEN.lock().unwrap();
        if !seen.contains(&(event as usize)) {
            seen.push(event as usize);
            let fp = crate::guest_reg!(x29);
            println!(
                "[sysevent] TimedWait {:p} timeout {} ns registered={} callers: {}",
                event,
                timeout_ns,
                hle_system_event_lookup(event as usize).is_some(),
                guest_chain(fp)
            );
        }
    }
    let timeout = std::time::Duration::from_nanos(timeout_ns.max(0) as u64);
    match hle_system_event_lookup(event as usize) {
        Some(ev) => {
            let g = ev.signaled.lock().unwrap();
            let (mut g, _) = ev.cv.wait_timeout_while(g, timeout, |s| !*s).unwrap();
            std::mem::replace(&mut *g, false)
        }
        None => {
            static SEEN: StdMutex<Vec<usize>> = StdMutex::new(Vec::new());
            let mut seen = SEEN.lock().unwrap();
            if !seen.contains(&(event as usize)) {
                seen.push(event as usize);
                eprintln!(
                    "[!] nn::os::TimedWaitSystemEvent on unregistered event {:p} -> times out",
                    event
                );
            }
            drop(seen);
            hle_sleep_ns(timeout.as_nanos().min(u64::MAX as u128) as u64);
            false
        }
    }
}

pub(crate) unsafe extern "C" fn wait_system_event(event: *mut ()) {
    record_hle_call("os::wait_system_event");
    let fp = crate::guest_reg!(x29);
    match hle_system_event_lookup(event as usize) {
        Some(ev) => {
            let mut g = ev.signaled.lock().unwrap();
            let _blocked = if *g {
                None
            } else {
                Some(BlockGuard::new("SystemEvent", event as usize, fp))
            };
            while !*g {
                g = ev.cv.wait(g).unwrap();
            }
            *g = false;
        }
        None => {
            // An event nothing initialised through HLE. Log once per address and rate-limit instead of
            // spinning: callers loop on this, and returning instantly pegs a core and floods the log.
            static SEEN: StdMutex<Vec<usize>> = StdMutex::new(Vec::new());
            let mut seen = SEEN.lock().unwrap();
            if !seen.contains(&(event as usize)) {
                seen.push(event as usize);
                eprintln!("[!] nn::os::WaitSystemEvent on unregistered event {:p} (returning after a short sleep from now on)", event);
            }
            drop(seen);
            hle_sleep_ns(5_000_000);
        }
    }
}

pub(crate) unsafe extern "C" fn destroy_system_event(event: *mut ()) {
    record_hle_call("os::destroy_system_event");
    HLE_SYSTEM_EVENTS
        .lock()
        .unwrap()
        .retain(|(a, _)| *a != event as usize);
}

pub(crate) extern "C" fn get_current_core_number() -> u32 {
    current_core()
}

// ── nn::os::Event ────────────────────────────────────────────────────────────────────────────
// EventClearMode: 0 = ManualClear, 1 = AutoClear.
pub(crate) unsafe extern "C" fn initialize_event(ev: *mut u8, initially_signaled: bool, clear_mode: i32) {
    let mut g = lock();
    g.as_mut().unwrap().events.insert(
        ev as usize,
        EventSt {
            signaled: initially_signaled,
            auto_clear: clear_mode == 1,
            signals: 0,
        },
    );
}

pub(crate) unsafe extern "C" fn finalize_event(ev: *mut u8) {
    lock().as_mut().unwrap().events.remove(&(ev as usize));
}

pub(crate) unsafe extern "C" fn signal_event(ev: *mut u8) {
    let mut g = lock();
    if let Some(e) = g.as_mut().unwrap().events.get_mut(&(ev as usize)) {
        e.signaled = true;
        e.signals += 1;
    }
    drop(g);
    SYNC_CV.notify_all();
}

pub(crate) unsafe extern "C" fn clear_event(ev: *mut u8) {
    if let Some(e) = lock().as_mut().unwrap().events.get_mut(&(ev as usize)) {
        e.signaled = false;
    }
}

pub(crate) unsafe extern "C" fn wait_event(ev: *mut u8) {
    event_wait(ev as usize, None);
}

pub(crate) unsafe extern "C" fn try_wait_event(ev: *mut u8) -> bool {
    event_wait(ev as usize, Some(std::time::Instant::now()))
}

pub(crate) unsafe extern "C" fn timed_wait_event(ev: *mut u8, timeout_ns: i64) -> bool {
    event_wait(
        ev as usize,
        Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64)),
    )
}

// ── nn::os::MultiWait ────────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn initialize_multiwait(mw: *mut u8) {
    lock()
        .as_mut()
        .unwrap()
        .multiwaits
        .insert(mw as usize, Vec::new());
}

pub(crate) unsafe extern "C" fn finalize_multiwait(mw: *mut u8) {
    lock().as_mut().unwrap().multiwaits.remove(&(mw as usize));
}

pub(crate) unsafe extern "C" fn initialize_multiwait_holder_event(holder: *mut u8, ev: *mut u8) {
    lock()
        .as_mut()
        .unwrap()
        .holders
        .insert(holder as usize, ev as usize);
}

pub(crate) unsafe extern "C" fn finalize_multiwait_holder(holder: *mut u8) {
    let mut g = lock();
    let s = g.as_mut().unwrap();
    s.holders.remove(&(holder as usize));
    for v in s.multiwaits.values_mut() {
        v.retain(|h| *h != holder as usize);
    }
}

pub(crate) unsafe extern "C" fn link_multiwait_holder(mw: *mut u8, holder: *mut u8) {
    let mut g = lock();
    g.as_mut()
        .unwrap()
        .multiwaits
        .entry(mw as usize)
        .or_default()
        .push(holder as usize);
    drop(g);
    SYNC_CV.notify_all(); // a waiter may be blocked on a list that just grew
}

pub(crate) unsafe extern "C" fn set_multiwait_holder_user_data(holder: *mut u8, data: usize) {
    HOLDER_USER_DATA
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(holder as usize, data);
}

pub(crate) unsafe extern "C" fn get_multiwait_holder_user_data(holder: *const u8) -> usize {
    HOLDER_USER_DATA
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&(holder as usize)).copied())
        .unwrap_or(0)
}

pub(crate) unsafe extern "C" fn unlink_multiwait_holder(holder: *mut u8) {
    let mut g = lock();
    for v in g.as_mut().unwrap().multiwaits.values_mut() {
        v.retain(|h| *h != holder as usize);
    }
}

/// Blocks until any linked holder's event is signaled; returns that holder (auto-clear events are consumed).
pub(crate) unsafe extern "C" fn wait_any(mw: *mut u8) -> *mut u8 {
    let mut g = lock();
    loop {
        let s = g.as_mut().unwrap();
        let holders: Vec<usize> = s
            .multiwaits
            .get(&(mw as usize))
            .cloned()
            .unwrap_or_default();
        for h in holders {
            if let Some(&ev) = s.holders.get(&h) {
                if let Some(e) = s.events.get_mut(&ev) {
                    if e.signaled {
                        if e.auto_clear {
                            e.signaled = false;
                        }
                        return h as *mut u8;
                    }
                }
            }
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}

// ── nn::os::MessageQueue ─────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn initialize_message_queue(mq: *mut u8, _buf: *mut u64, count: usize) {
    lock().as_mut().unwrap().msgqs.insert(
        mq as usize,
        MsgQ {
            cap: count.max(1),
            q: VecDeque::new(),
        },
    );
}

pub(crate) unsafe extern "C" fn finalize_message_queue(mq: *mut u8) {
    lock().as_mut().unwrap().msgqs.remove(&(mq as usize));
}

pub(crate) unsafe extern "C" fn send_message_queue(mq: *mut u8, data: u64) {
    let mut g = lock();
    loop {
        let s = g.as_mut().unwrap();
        match s.msgqs.get_mut(&(mq as usize)) {
            Some(q) if q.q.len() >= q.cap => {}
            Some(q) => {
                q.q.push_back(data);
                drop(g);
                SYNC_CV.notify_all();
                return;
            }
            None => return,
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}

pub(crate) unsafe extern "C" fn receive_message_queue(out: *mut u64, mq: *mut u8) {
    let mut g = lock();
    loop {
        let s = g.as_mut().unwrap();
        match s.msgqs.get_mut(&(mq as usize)) {
            Some(q) => {
                if let Some(v) = q.q.pop_front() {
                    if !out.is_null() {
                        *out = v;
                    }
                    drop(g);
                    SYNC_CV.notify_all();
                    return;
                }
            }
            None => return,
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}

pub(crate) unsafe extern "C" fn try_send_message_queue(mq: *mut u8, data: u64) -> bool {
    mq_send(mq as usize, data, false, Some(std::time::Instant::now()))
}

pub(crate) unsafe extern "C" fn jam_message_queue(mq: *mut u8, data: u64) {
    mq_send(mq as usize, data, true, None);
}

pub(crate) unsafe extern "C" fn try_receive_message_queue(out: *mut u64, mq: *mut u8) -> bool {
    mq_receive(out, mq as usize, Some(std::time::Instant::now()), false)
}

pub(crate) unsafe extern "C" fn timed_receive_message_queue(
    out: *mut u64,
    mq: *mut u8,
    timeout_ns: i64,
) -> bool {
    mq_receive(
        out,
        mq as usize,
        Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64)),
        false,
    )
}

pub(crate) unsafe extern "C" fn peek_message_queue(out: *mut u64, mq: *const u8) {
    mq_receive(out, mq as usize, None, true);
}

pub(crate) unsafe extern "C" fn try_peek_message_queue(out: *mut u64, mq: *const u8) -> bool {
    mq_receive(out, mq as usize, Some(std::time::Instant::now()), true)
}

pub(crate) unsafe extern "C" fn timed_peek_message_queue(out: *mut u64, mq: *const u8, timeout_ns: i64) -> bool {
    mq_receive(out, mq as usize, deadline_ns(timeout_ns), true)
}

pub(crate) unsafe extern "C" fn try_jam_message_queue(mq: *mut u8, data: u64) -> bool {
    mq_send(mq as usize, data, true, Some(std::time::Instant::now()))
}

pub(crate) unsafe extern "C" fn timed_send_message_queue(mq: *mut u8, data: u64, timeout_ns: i64) -> bool {
    mq_send(mq as usize, data, false, deadline_ns(timeout_ns))
}

pub(crate) unsafe extern "C" fn timed_jam_message_queue(mq: *mut u8, data: u64, timeout_ns: i64) -> bool {
    mq_send(mq as usize, data, true, deadline_ns(timeout_ns))
}

pub(crate) unsafe extern "C" fn try_acquire_write_lock(rw: *mut u8) -> bool {
    rw_try_acquire_write(rw as usize)
}

pub(crate) unsafe extern "C" fn try_acquire_read_lock(rw: *mut u8) -> bool {
    rw_try_acquire_read(rw as usize)
}

// ── misc nn::os ──────────────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn set_dying_message_region(_addr: usize, _size: usize) {}

/// nn::os::GetCurrentFiber: ThreadType::_currentFiber, which the SDK's native SwitchToFiber maintains.
pub(crate) unsafe extern "C" fn get_current_fiber() -> *mut u8 {
    let t = crate::horizon::os::get_current_thread();
    if t.is_null() {
        std::ptr::null_mut()
    } else {
        (*t).current_fiber as *mut u8
    }
}

/// nn::os::ConvertToTick(TimeSpan) -> Tick, consistent with GetSystemTick (host mach ticks).
pub(crate) unsafe extern "C" fn convert_to_tick(ns: i64) -> i64 {
    crate::platform::clock::ns_to_ticks(ns.max(0) as u64) as i64
}


/// MultiWaitHolder user data: an opaque value the caller attaches to a holder.
pub(crate) static HOLDER_USER_DATA: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);

pub(crate) fn deadline_ns(timeout_ns: i64) -> Option<std::time::Instant> {
    Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64))
}

/// Data-cache maintenance (FlushDataCache etc.): the sdk module's versions execute `dc`/`mrs ctr_el0`
/// sequences that Darwin traps (SIGILL). Apple Silicon memory is coherent for our purposes, so no-op.
pub(crate) unsafe extern "C" fn cache_noop(_addr: *const u8, _size: usize) {}
