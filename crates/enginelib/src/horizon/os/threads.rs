// Guest threads: the nn::os thread object (NnThreadType) and its tail fields, handles, the current thread's state, the
// Darwin TLS a guest thread runs with, exit signals and joins, and core assignment.

use std::ffi::CStr;
use super::*;
use std::sync::atomic::AtomicU32;
use std::os::raw::c_char;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::cell::RefCell;

// Layout of nn::os::ThreadType (0x1B8 bytes in current titles; no `_lockHistory` field). Guest code reads its fields
// directly (sead's stack checks read the stack and its size), so the layout must be the SDK's.
#[repr(C, align(8))]
pub(crate) struct NnThreadType {
    pub _all_threads_list_node: [u8; 16], // 0x00 TypedStorage<IntrusiveListNode,16,8>
    pub _multi_wait_object_list: [u8; 16], // 0x10 TypedStorage<MultiWaitObjectList,16,8>
    pub _reserved: [usize; 4],            // 0x20
    pub state: u8,                        // 0x40 _state
    pub stack_is_aliased: bool,           // 0x41 _stackIsAliased
    pub auto_registered: bool,            // 0x42 _autoRegistered
    pub suspend_count: u8,                // 0x43 _suspendCount
    pub base_priority: i32,               // 0x44 _basePriority
    pub original_stack: *mut (),          // 0x48 _originalStack
    pub stack: *mut (),                   // 0x50 _stack
    pub stack_size: usize,                // 0x58 _stackSize
    pub argument: *mut (),                // 0x60 _argument
    pub thread_function: Option<unsafe extern "C" fn(*mut ())>, // 0x68 _threadFunction
    pub current_fiber: *mut (),           // 0x70 _currentFiber
    pub initial_fiber: *mut (),           // 0x78 _initialFiber
    /// 0x80 in the newest layout: TLS values (0x100), name buffer (0x20), name pointer, lock, condition variable,
    /// handle; older SDKs place them sdk::thread_tail_shift() bytes later (`ThreadTail`). Room for the longest layout.
    pub _tail: [u8; 0x140],
}
const _: () = assert!(std::mem::size_of::<NnThreadType>() == 0x1C0);

/// The thread object's fields after its fiber pointers, at the game's SDK's offsets.
pub(crate) struct ThreadTail(usize);

impl ThreadTail {
    pub(crate) fn of(t: *const NnThreadType) -> Self {
        ThreadTail(t as usize + 0x80 + crate::horizon::sdk::thread_tail_shift())
    }
    pub(crate) fn name_buffer(&self) -> *mut u8 {
        (self.0 + 0x100) as *mut u8
    }
    pub(crate) fn name_pointer(&self) -> *mut *const c_char {
        (self.0 + 0x120) as *mut *const c_char
    }
    pub(crate) fn handle(&self) -> *mut u32 {
        (self.0 + 0x130) as *mut u32
    }
}

pub(crate) static NEXT_THREAD_HANDLE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x100);

thread_local! {
    pub(crate) static CURRENT_NN_THREAD: RefCell<*mut NnThreadType> = const { RefCell::new(std::ptr::null_mut()) };
    pub(crate) static THREAD_SELF_STRUCT: RefCell<NnThreadType> = RefCell::new(NnThreadType {
        _all_threads_list_node: [0; 16],
        _multi_wait_object_list: [0; 16],
        _reserved: [0; 4],
        state: 1,
        stack_is_aliased: false,
        auto_registered: false,
        suspend_count: 0,
        base_priority: 16,
        original_stack: std::ptr::null_mut(),
        stack: std::ptr::null_mut(),
        stack_size: 8 * 1024 * 1024,
        argument: std::ptr::null_mut(),
        thread_function: None,
        current_fiber: std::ptr::null_mut(),
        initial_fiber: std::ptr::null_mut(),
        _tail: [0; 0x140],
    });
    pub(crate) static THREAD_TLS_VALUES: RefCell<[u64; 64]> = const { RefCell::new([0; 64]) };
}

/// Snapshot the Darwin-owned TSD slots for this thread (no-op after the first call).
pub(crate) unsafe fn capture_darwin_tsd_shadow() {
    #[cfg(nx_native)]
    {
        if TSD_SHADOW.with(|c| c.get().is_some()) {
            return;
        }
        let tp: *mut u8;
        std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tp);
        if (tp as usize) >= 0x10000 {
            let v = [
                *(tp.add(0x10) as *const usize),
                *(tp.add(0x18) as *const usize),
                *(tp.add(0x20) as *const usize),
                *(tp.add(0x28) as *const usize),
            ];
            TSD_SHADOW.with(|c| c.set(Some(v)));
        }
    }
}

pub(crate) unsafe fn setup_horizon_tpidrro(thread_ptr: *mut NnThreadType) {
    capture_darwin_tsd_shadow();
    #[cfg(nx_native)]
    {
        let tp: *mut u8;
        std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tp);
        if (tp as usize) >= 0x10000 {
            *(tp.add(0x1f8) as *mut usize) = thread_ptr as usize;
            *(tp.add(0x1f0) as *mut usize) = thread_ptr as usize;
            let current_errno_ptr = *(tp.add(8) as *const usize);
            if current_errno_ptr >= (tp as usize) && current_errno_ptr < (tp as usize) + 0x1000 {
                *(tp.add(crate::guest::exec::tls::errno_save_offset()) as *mut usize) = current_errno_ptr;
            }
            let guest_tls = crate::guest::exec::tls::get_current_guest_tls_block();
            if guest_tls >= 0x10000 {
                *(tp.add(crate::guest::exec::tls::guest_tls_offset()) as *mut usize) = guest_tls;
                std::arch::asm!("msr tpidr_el0, {0}", in(reg) guest_tls);
            }
        }
        let tpidr: *mut u8;
        std::arch::asm!("mrs {}, tpidr_el0", out(reg) tpidr);
        if (tpidr as usize) >= 0x10000 {
            *(tpidr.add(0x1f8) as *mut usize) = thread_ptr as usize;
            *(tpidr.add(0x1f0) as *mut usize) = thread_ptr as usize;
        }
    }
    // Lifted: the same thread pointers in the thread's own region and TLS block, which its Cpu's tpidrro / tpidr hold.
    #[cfg(not(nx_native))]
    {
        let tlr = crate::guest::tls::current_fake_tlr();
        for p in [crate::guest::tls::thread_block(), tlr].into_iter().filter(|&p| p != 0) {
            *((p + 0x1f8) as *mut usize) = thread_ptr as usize;
            *((p + 0x1f0) as *mut usize) = thread_ptr as usize;
        }
    }
}

pub(crate) unsafe fn sanitize_darwin_tpidrro() {
    #[cfg(nx_native)]
    {
        let tp: *mut u8;
        std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tp);
        if (tp as usize) >= 0x10000 {
            let saved = *(tp.add(crate::guest::exec::tls::errno_save_offset()) as *const usize);
            let current = *(tp.add(8) as *const usize);
            if saved != 0 && (current < (tp as usize) || current >= (tp as usize) + 0x1000) {
                *(tp.add(8) as *mut usize) = saved;
            }
            let guest_tls = *(tp.add(crate::guest::exec::tls::guest_tls_offset()) as *const usize);
            if guest_tls >= 0x10000 {
                std::arch::asm!("msr tpidr_el0, {0}", in(reg) guest_tls);
            }
            // Only the mach thread-self port (+0x18) is immutable; the other slots (e.g. +0x10, the
            // MIG reply port) legitimately change at runtime, so they are never "restored".
            if let Some(shadow) = TSD_SHADOW.with(|c| c.get()) {
                let slot = tp.add(0x18) as *mut usize;
                if *slot != shadow[1] {
                    eprintln!("[!] Darwin TSD thread-self slot clobbered by guest ({:#x} -> restoring {:#x})", *slot, shadow[1]);
                    *slot = shadow[1];
                }
            }
        }
    }
}

// nn::os::ThreadType::State.
pub(crate) const THREAD_STATE_NOT_INITIALIZED: u8 = 0;

pub(crate) const THREAD_STATE_INITIALIZED: u8 = 1;

pub(crate) const THREAD_STATE_STARTED: u8 = 3;

pub(crate) const THREAD_STATE_EXITED: u8 = 4;

pub(crate) type ExitSignal = std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>;

pub(crate) static THREAD_EXITS: std::sync::Mutex<Option<std::collections::HashMap<usize, ExitSignal>>> =
    std::sync::Mutex::new(None);

/// The exit flag of a started guest thread (`fresh` replaces any previous one when the object is started again).
pub(crate) fn thread_exit_signal(thread: *mut NnThreadType, fresh: bool) -> Option<ExitSignal> {
    let mut g = THREAD_EXITS.lock().unwrap_or_else(|e| e.into_inner());
    let map = g.get_or_insert_with(std::collections::HashMap::new);
    if fresh {
        let s: ExitSignal =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        map.insert(thread as usize, s.clone());
        return Some(s);
    }
    map.get(&(thread as usize)).cloned()
}

/// Blocks until a started thread has returned from its entry function (WaitThread, and DestroyThread
/// for a thread in the Started state). The game relies on it: Kirby's movie player frees the decoder
/// and extractor right after joining the threads that use them.
pub(crate) unsafe fn join_guest_thread(thread: *mut NnThreadType) {
    let Some(sig) = thread_exit_signal(thread, false) else {
        return;
    };
    let (done, cv) = &*sig;
    let mut d = done.lock().unwrap_or_else(|e| e.into_inner());
    while !*d {
        d = cv.wait(d).unwrap_or_else(|e| e.into_inner());
    }
}

// Per-thread core assignment. TotK's task system dispatches per core (0..2), so a thread must
// report the core its affinity selected (a constant core would strand per-core task queues).
pub(crate) static THREAD_CORES: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, i32>>> = OnceLock::new();

pub(crate) const DEFAULT_CORE: i32 = 1;

pub(crate) fn thread_cores() -> &'static StdMutex<rustc_hash::FxHashMap<usize, i32>> {
    THREAD_CORES.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

/// Core number the calling guest thread runs on (nn::os::GetCurrentCoreNumber).
pub(crate) fn current_core() -> u32 {
    let t = unsafe { crate::horizon::os::get_current_thread() as usize };
    thread_cores()
        .lock()
        .unwrap()
        .get(&t)
        .copied()
        .unwrap_or(DEFAULT_CORE) as u32
}
thread_local! {
    // Darwin pthread TSD slots 2..5 (mach thread port, MIG reply port, ...) as first seen on
    // this thread. The guest treats tpidrro_el0 as its Horizon TLS (IPC buffer in the first
    // 0x100 bytes), so its writes can clobber them; libsystem then fails with e.g.
    // "clock_get_time() failed: (ipc/send) invalid reply port" inside nanosleep.
    static TSD_SHADOW: std::cell::Cell<Option<[usize; 4]>> = const { std::cell::Cell::new(None) };
}
pub(crate) static NEXT_TLS_SLOT: AtomicU32 = AtomicU32::new(0);
pub(crate) unsafe extern "C" fn get_current_thread() -> *mut NnThreadType {
    CURRENT_NN_THREAD.with(|cell| {
        let mut ptr = *cell.borrow();
        if ptr.is_null() {
            ptr = THREAD_SELF_STRUCT.with(|s| s.as_ptr());
            let handle = ThreadTail::of(ptr).handle();
            if *handle == 0 {
                *handle = 1;
            }
            // The thread's real stack bounds, at the SDK's field offsets (_stack / _stackSize; see the struct).
            let (lo, hi) = crate::horizon::thread_stack::current_bounds().unwrap_or_else(crate::platform::os::current_thread_stack);
            (*ptr).stack = lo as *mut ();
            (*ptr).original_stack = (*ptr).stack;
            (*ptr).stack_size = hi - lo;
            *cell.borrow_mut() = ptr;
        }
        setup_horizon_tpidrro(ptr);
        ptr
    })
}

pub(crate) unsafe extern "C" fn is_mutex_locked_by_current_thread(mutex: *const NnMutex) -> bool {
    if (mutex as usize) >= 0x10000 {
        let cur = crate::horizon::os::get_current_thread();
        (*mutex).owner_thread == cur && (*mutex).nest_count > 0
    } else {
        false
    }
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
    // processor: with all of them busy that is another process, for its whole quantum (milliseconds a yield). The game's
    // threads that are ready get a processor from the scheduler anyway; on an idle machine SwitchToThread returns at
    // once, in about the time these pauses take.
    for _ in 0..8 {
        std::hint::spin_loop();
    }
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
    LIVE_THREADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        // libraries, and compiling a render pipeline (synchronous, on the calling thread) can take a thread past 512 KiB.
        .stack_size(8 << 20)
        .spawn(move || {
            crate::guest::tls::mark_guest_thread();
            println!("[+] HLE: Guest thread '{}' starting execution", name_str);
            // Every guest thread asks for performance cores: the console runs game threads on identical cores, and with
            // the default QoS macOS puts some on efficiency cores, where a thread holding a spinlock (Breath of the
            // Wild's job queue) runs slowly while others spin on it.
            #[cfg(target_vendor = "apple")]
            {
                extern "C" {
                    fn pthread_set_qos_class_self_np(qos: u32, relpri: i32) -> i32;
                }
                const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
                pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
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
                crate::horizon::thread_stack::allocate(1 << 20)
            };
            let handle = ThreadTail::of(thread_ptr).handle();
            if *handle == 0 {
                *handle = NEXT_THREAD_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            setup_horizon_tpidrro(thread_ptr);
            crate::horizon::thread_stack::run(stack_lo, stack_hi, entry_usize, arg_usize);
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

/// Threads the game created and has not destroyed (the main thread not included).
static LIVE_THREADS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// The process's thread limit: Ryujinx's (KernelInit: LimitableResource.Thread 800).
const THREAD_LIMIT: i32 = 800;

/// nn::os::GetFreeThreadCount(): threads the process may still create, the limit minus the threads in use (the main
/// thread included).
pub(crate) unsafe extern "C" fn get_free_thread_count() -> i32 {
    (THREAD_LIMIT - 1 - LIVE_THREADS.load(std::sync::atomic::Ordering::Relaxed)).max(0)
}

pub(crate) unsafe extern "C" fn destroy_thread(thread: *mut NnThreadType) {
    if !thread.is_null() {
        if (*thread).state != THREAD_STATE_NOT_INITIALIZED {
            LIVE_THREADS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
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

pub(crate) unsafe extern "C" fn is_write_lock_held_by_current_thread(
    rwlock: *const NnReaderWriterLock,
) -> bool {
    !rwlock.is_null() && crate::horizon::os::objects::rw_write_held_by_me(rwlock as usize)
}

pub(crate) extern "C" fn get_current_core_number() -> u32 {
    current_core()
}

/// nn::os::GetCurrentFiber: ThreadType::_currentFiber, which the SDK's native SwitchToFiber maintains.
pub(crate) unsafe extern "C" fn get_current_fiber() -> *mut u8 {
    let t = crate::horizon::os::get_current_thread();
    if t.is_null() {
        std::ptr::null_mut()
    } else {
        (*t).current_fiber as *mut u8
    }
}

/// Whether the calling thread is the game's main thread (the one that runs nnMain: startup.rs names it guest-main).
pub(crate) fn on_guest_main() -> bool {
    thread_local! {
        static MAIN: bool = std::thread::current().name() == Some("guest-main");
    }
    MAIN.with(|m| *m)
}
