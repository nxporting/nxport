// Guest threads: the nn::os thread object (NnThreadType) and its tail fields, handles, the current thread's state, the
// Darwin TLS a guest thread runs with, exit signals and joins, and core assignment.

use std::sync::atomic::AtomicU32;
use std::os::raw::c_char;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::cell::RefCell;

// Layout of nn::os::ThreadType (0x1B8 bytes in current titles; no `_lockHistory` field).
//
// This replaced a struct that was 1056 bytes with a completely invented field
// order/layout against a real 440-byte type. Every one of our HLE functions
// only ever touched it through our own field names, so those specific writes
// stayed harmlessly in-bounds -- but ANY authentic, unmodified guest code
// that reads a ThreadType field *directly* (e.g. sead's stack-guard checks,
// which are routine, expected defensive bookkeeping, not a bug) would read
// whatever happened to sit at that byte offset in our fictional layout
// instead of the real field. That's a plausible source of exactly the kind
// of "everything I can verify is correct, yet the live state doesn't match"
// symptom this session kept running into elsewhere in the engine.
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
// report the core its affinity selected: GetCurrentCoreNumber used to return a constant 1 for
// every thread, which left per-core task queues stranded (worker threads waited forever).
pub(crate) static THREAD_CORES: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, i32>>> = OnceLock::new();

pub(crate) const DEFAULT_CORE: i32 = 1;

/// Bumped whenever a thread's core changes (cores.rs caches each thread's core against it).
pub(crate) static CORES_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
// 9. Threading & TLS Management
pub(crate) static NEXT_TLS_SLOT: AtomicU32 = AtomicU32::new(0);
