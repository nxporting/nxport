// The nn:: libraries HLE'd at the guest's import boundary: threads and synchronisation, memory, TLS, clocks, the
// C++ runtime hooks, display (vi) and graphics bootstrap (nv), audio and codec entry points.
#![allow(non_snake_case)]

use std::ffi::CStr;
use std::os::raw::c_char;

// 1. C++ ABI Guards
//
// __cxa_guard_acquire/release gate C++11 thread-safe static-local
// initialization (Meyer's singletons, `static Foo& instance() { static Foo
// f; return f; }`, extremely common in sead:: manager classes). The Itanium
// ABI *requires* this to be safe under real concurrent access: if two
// threads race to initialize the same static local, exactly one must run
// the constructor while the other blocks until it's done. The previous
// implementation was a plain unsynchronized read-then-branch with no
// locking at all -- with two genuinely concurrent OS threads (main +
// VibrationThread), both could observe *guard == 0 at once, both would be
// told to proceed, and both would run the constructor: a double-init race
// that can leave a "singleton" partially or doubly constructed. That
// produces exactly the class of symptom this session kept running into --
// an object whose vtable and relocations are all individually correct, yet
// whose live state doesn't match what a single-threaded reading of the code
// would predict.
struct GuardState {
    in_progress: StdMutex<bool>,
    cv: Condvar,
}

static GUARD_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<GuardState>>>> =
    OnceLock::new();

fn guard_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<GuardState>>> {
    GUARD_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

unsafe fn guard_state_for(guard: *mut u8) -> Arc<GuardState> {
    let key = guard as usize;
    let mut table = guard_table().lock().unwrap();
    table
        .entry(key)
        .or_insert_with(|| {
            Arc::new(GuardState {
                in_progress: StdMutex::new(false),
                cv: Condvar::new(),
            })
        })
        .clone()
}

#[no_mangle]
pub unsafe extern "C" fn hle_cxa_guard_acquire(guard: *mut u8) -> i32 {
    if guard.is_null() {
        return 0;
    }
    if *guard != 0 {
        return 0; // fast path: already fully initialized
    }
    let state = guard_state_for(guard);
    let mut in_progress = state.in_progress.lock().unwrap();
    loop {
        if *guard != 0 {
            return 0; // finished by whoever held in_progress while we waited for the lock
        }
        if !*in_progress {
            *in_progress = true;
            return 1; // this thread does the initialization
        }
        // another thread is currently constructing this static local --
        // block until it calls release(), then re-check rather than racing.
        in_progress = state.cv.wait(in_progress).unwrap();
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_cxa_guard_release(guard: *mut u8) {
    if guard.is_null() {
        return;
    }
    *guard = 1;
    let state = guard_state_for(guard);
    {
        let mut in_progress = state.in_progress.lock().unwrap();
        *in_progress = false;
    }
    state.cv.notify_all();
}

#[no_mangle]
pub unsafe extern "C" fn hle_cxa_atexit(_func: *const (), _arg: *const (), _dso: *const ()) -> i32 {
    0
}

// 2. Musl Libc init stub & Diagnostic Stub
#[no_mangle]
pub unsafe extern "C" fn hle_default_stub() -> i64 {
    let got_slot = crate::guest_reg!(x16);
    let caller_lr = crate::guest_reg!(x17);
    let guest_base = crate::guest::loader::main_base();
    let got_virtual = if got_slot >= guest_base {
        got_slot - guest_base
    } else {
        got_slot
    };
    let lr_virtual = if caller_lr >= guest_base {
        caller_lr - guest_base
    } else {
        caller_lr
    };
    println!(
        "[!] HLE UNIMPLEMENTED GOT STUB: GOT slot 0x{:08X}, caller LR=0x{:08X}",
        got_virtual, lr_virtual
    );
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nnmusl_init_dso(
    base: usize,
    _a1: usize,
    tdata_start: usize,
    tdata_end: usize,
    align_sym: usize,
    _a5: usize,
    tbss_start: usize,
    tbss_end: usize,
) -> i32 {
    // The module's DT_INIT passes its TLS segment: x2..x3 the initialized part (.tdata), x4 its alignment (an absolute
    // linker symbol that arrives relocated: module base + alignment; x0 is the base), x6..x7 the
    // zero-filled part (.tbss) (TotK: no .tdata, 8 bytes of .tbss; Super Mario Bros. Wonder: 16 bytes of .tdata, an
    // owner sentinel of -1 its allocator checks before using a per-thread arena). Every guest thread's block starts
    // with this template (guest/exec/tls.rs), as nnmusl gives each thread a copy.
    let align = align_sym.wrapping_sub(base);
    let align = if align.is_power_of_two() && align <= 0x1000 {
        align
    } else {
        16
    };
    let n = tdata_end.saturating_sub(tdata_start);
    // The block holds at most 0x1f0 bytes of it (guest/tls.rs reports a larger segment), so no more is read.
    let template = if n > 0 && n <= 0x1f0 && tdata_start >= 0x10000 {
        std::slice::from_raw_parts(tdata_start as *const u8, n).to_vec()
    } else {
        Vec::new()
    };
    println!(
        "[+] HLE: __nnmusl_init_dso: TLS template {} bytes initialized + {} zeroed, alignment {}",
        template.len(),
        tbss_end.saturating_sub(tbss_start),
        align
    );
    crate::guest::tls::set_module_tls_template(template, align, n + tbss_end.saturating_sub(tbss_start));
    1
}

// 3. Libc / standard math pass-throughs
#[no_mangle]
pub unsafe extern "C" fn hle_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    #[cfg(nx_native)]
    let _copy = crate::metal::rt_guard::guest_copy_scope();
    if (dest as usize) < 0x10000 || n == 0 {
        return dest;
    }
    libc::memset(dest as *mut libc::c_void, c, n) as *mut u8
}

#[no_mangle]
pub unsafe extern "C" fn hle_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    #[cfg(nx_native)]
    let _copy = crate::metal::rt_guard::guest_copy_scope();
    if (dest as usize) < 0x10000 || (src as usize) < 0x10000 || n == 0 {
        return dest;
    }
    libc::memcpy(dest as *mut libc::c_void, src as *const libc::c_void, n) as *mut u8
}

#[no_mangle]
pub unsafe extern "C" fn hle_memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    #[cfg(nx_native)]
    let _copy = crate::metal::rt_guard::guest_copy_scope();
    if (dest as usize) < 0x10000 || (src as usize) < 0x10000 || n == 0 {
        return dest;
    }
    libc::memmove(dest as *mut libc::c_void, src as *const libc::c_void, n) as *mut u8
}

#[no_mangle]
pub unsafe extern "C" fn hle_strlen(s: *const c_char) -> usize {
    if (s as usize) < 0x10000 {
        return 0;
    }
    libc::strlen(s)
}

// 4. Horizon OS Thread & Mutex implementation (backed by thread-aware recursive lock)
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

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
pub struct NnThreadType {
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
        ThreadTail(t as usize + 0x80 + super::sdk::thread_tail_shift())
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

pub static NEXT_THREAD_HANDLE: std::sync::atomic::AtomicU32 =
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

thread_local! {
    // Darwin pthread TSD slots 2..5 (mach thread port, MIG reply port, ...) as first seen on
    // this thread. The guest treats tpidrro_el0 as its Horizon TLS (IPC buffer in the first
    // 0x100 bytes), so its writes can clobber them; libsystem then fails with e.g.
    // "clock_get_time() failed: (ipc/send) invalid reply port" inside nanosleep.
    static TSD_SHADOW: std::cell::Cell<Option<[usize; 4]>> = const { std::cell::Cell::new(None) };
}

/// Snapshot the Darwin-owned TSD slots for this thread (no-op after the first call).
pub unsafe fn capture_darwin_tsd_shadow() {
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

pub unsafe fn setup_horizon_tpidrro(thread_ptr: *mut NnThreadType) {
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

pub unsafe fn sanitize_darwin_tpidrro() {
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


#[repr(C)]
pub struct NnMutex {
    pub state: u8,
    pub is_recursive: bool,
    pub _pad1: [u8; 2],
    pub lock_level: i32,
    pub nest_count: i32,
    pub _pad2: [u8; 4],
    pub owner_thread: *mut NnThreadType,
    pub lock: u32,
    pub _pad3: u32,
}


// The lock word lives in guest memory, so it's a plain atomic owner token rather
// than an os_unfair_lock: libplatform aborts the whole process ("os_unfair_lock is
// corrupt") if the word is ever stale, relocked by its owner, or unlocked from
// another thread, none of which should take the emulator down.
pub(crate) unsafe fn mutex_lock_word<'a>(mutex: *mut NnMutex) -> &'a std::sync::atomic::AtomicU32 {
    &*(&raw mut (*mutex).lock as *const std::sync::atomic::AtomicU32)
}
pub(crate) fn mutex_token(cur: *mut NnThreadType) -> u32 {
    (cur as usize as u32 & !MUTEX_WAITERS) | 1
}
/// Set in a held lock word while a thread is (or may be) parked on it: the unlock then wakes one.
pub(crate) const MUTEX_WAITERS: u32 = 2;

/// Blocks while `word` still holds `held` (or returns early: callers re-check). Windows parks the thread on the
/// address; elsewhere a short sleep stands in for it.
fn mutex_park(word: &std::sync::atomic::AtomicU32, held: u32) {
    #[cfg(windows)]
    {
        #[link(name = "synchronization")]
        extern "system" {
            fn WaitOnAddress(addr: *const std::ffi::c_void, cmp: *const std::ffi::c_void, size: usize, ms: u32) -> i32;
        }
        unsafe {
            WaitOnAddress(
                word as *const _ as *const std::ffi::c_void,
                &held as *const u32 as *const std::ffi::c_void,
                4,
                u32::MAX,
            )
        };
    }
    #[cfg(not(windows))]
    {
        let _ = (word, held);
        hle_sleep_ns(5_000);
    }
}
pub(crate) fn mutex_unpark(word: &std::sync::atomic::AtomicU32) {
    #[cfg(windows)]
    {
        #[link(name = "synchronization")]
        extern "system" {
            fn WakeByAddressSingle(addr: *const std::ffi::c_void);
        }
        unsafe { WakeByAddressSingle(word as *const _ as *const std::ffi::c_void) };
    }
    #[cfg(not(windows))]
    let _ = word;
}



/// LockMutex once the first compare-and-swap failed. TotK's waits for a held mutex are a couple of microseconds, so a
/// short spin takes most; then the thread parks on the lock word. It used to spin, yield and sleep-spin instead, which
/// with every processor busy kept a core while the owner sat preempted in the ready queue behind it.
#[cold]
pub(crate) fn mutex_lock_contended(word: &std::sync::atomic::AtomicU32, tok: u32) {
    use std::sync::atomic::Ordering;
    for _ in 0..1000 {
        std::hint::spin_loop();
        if word.load(Ordering::Relaxed) == 0
            && word.compare_exchange(0, tok, Ordering::Acquire, Ordering::Relaxed).is_ok()
        {
            return;
        }
    }
    // A thread that parked once takes the lock with MUTEX_WAITERS set: others may still be parked behind it.
    let mut w = word.load(Ordering::Relaxed);
    loop {
        if w == 0 {
            match word.compare_exchange(0, tok | MUTEX_WAITERS, Ordering::Acquire, Ordering::Relaxed) {
                Ok(_) => return,
                Err(x) => {
                    w = x;
                    continue;
                }
            }
        }
        if w & MUTEX_WAITERS == 0 {
            if let Err(x) = word.compare_exchange(w, w | MUTEX_WAITERS, Ordering::Relaxed, Ordering::Relaxed) {
                w = x;
                continue;
            }
            w |= MUTEX_WAITERS;
        }
        mutex_park(word, w);
        w = word.load(Ordering::Relaxed);
    }
}




// 5. System clocks. The guest's system tick is the host clock (crate::platform::clock), not the Switch's 19.2 MHz counter:
// GetSystemTick, GetSystemTickFrequency and ConvertToTimeSpan all use it, so they agree with each other and with NVN
// timestamps.

/// --log-timestamps (with metal/mod.rs): guest-main's GPU syncs and sleeps next to the TIMESTAMP reports, in ms of the
/// same clock, to reconstruct a frame's timeline.
pub(crate) fn log_timestamps() -> bool {
    crate::settings::get().log.log_timestamps
}

/// Sleep on a guest thread without nanosleep(): libsystem's nanosleep makes a MIG call that
/// needs this thread's mach reply-port TSD slot, which the guest's use of tpidrro_el0 as
/// its Horizon TLS can clobber ("clock_get_time() failed: (ipc/send) invalid reply port").
/// mach_wait_until is a bare kernel trap with no reply port (crate::platform::clock::sleep_ns).
pub fn hle_sleep_ns(ns: u64) {
    crate::platform::clock::sleep_ns(ns);
}


// 6. NVN Graphics Bootstrap Loader & NV / VI Subsystems
pub(crate) static mut DUMMY_DISPLAY: usize = 0x1000;
pub(crate) static mut DUMMY_LAYER: usize = 0x2000;
pub(crate) static mut DUMMY_WINDOW: usize = 0x3000;

#[no_mangle]
pub unsafe extern "C" fn hle_nv_SetGraphicsAllocator(
    _alloc: *const (),
    _free: *const (),
    _realloc: *const (),
    _user: *const (),
) {
    println!("[+] HLE: nv::SetGraphicsAllocator called");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nv_InitializeGraphics(mem: *mut (), size: usize) -> i32 {
    println!(
        "[+] HLE: nv::InitializeGraphics(mem={:p}, size={} MB)",
        mem,
        size / (1024 * 1024)
    );
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nv_SetGraphicsDevtoolsAllocator(
    _alloc: *const (),
    _free: *const (),
    _realloc: *const (),
    _user: *const (),
) {
    println!("[+] HLE: nv::SetGraphicsDevtoolsAllocator called");
}






pub(crate) fn nn_vi_start_vsync_pacer(ev: std::sync::Arc<HleSystemEvent>) {
    static EVENTS: StdMutex<Vec<std::sync::Arc<HleSystemEvent>>> = StdMutex::new(Vec::new());
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    EVENTS.lock().unwrap().push(ev);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("hle-vsync".into())
        .spawn(|| {
            let period = std::time::Duration::from_nanos(16_666_667);
            let mut next = std::time::Instant::now() + period;
            loop {
                let now = std::time::Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                }
                next += period;
                for e in EVENTS.lock().unwrap().iter() {
                    hle_system_event_signal(e);
                }
            }
        })
        .expect("spawn vsync pacer");
}


// Global, always-on HLE call-history ring buffer. Every NVN HLE function
// (both the real implementations and the labeled-stub pool) reports here on
// every call, not just on resolution. Answers "what HLE functions were
// called, in order, before this crash?" directly from the crash dump instead
// of requiring a fresh live-tracing session each time -- the silent-stub gap
// and the "we don't have call-time visibility" gap were really the same
// problem, just found from two different angles this session.
const HLE_CALL_HISTORY_SIZE: usize = 4096;
// Each thread records into a ring of its own, with plain stores: one ring for all threads had its lock contended by
// every guest thread making HLE calls (2.4% of all samples in TotK's gameplay on the lifted runtime), and a lock per
// thread (and a shared sequence counter) still cost 4-5% of the busy time of guest-main and the module workers.
// Readers merge the rings by sequence number (on x86-64 the time stamp counter, synchronized across cores) and skip a
// slot written while they read it. Names are 'static (literals,
// and the rare formatted name interned once), so a slot is a sequence number and a str's pointer and length.
const HLE_CALL_RING_SIZE: usize = 1024;
struct HleCallSlot {
    seq: AtomicU64, // 0: empty or being written
    ptr: AtomicUsize,
    len: AtomicUsize,
}
struct HleCallRing {
    next: AtomicUsize,
    slots: [HleCallSlot; HLE_CALL_RING_SIZE],
}
static HLE_CALL_RINGS: StdMutex<Vec<&'static HleCallRing>> = StdMutex::new(Vec::new());
#[cfg(not(target_arch = "x86_64"))]
static HLE_CALL_SEQ: AtomicU64 = AtomicU64::new(1);
thread_local! {
    // Rings outlive their threads (HLE calls from TLS destructors still record), so the slot needs no destructor.
    static HLE_CALL_RING: std::cell::Cell<Option<&'static HleCallRing>> = const { std::cell::Cell::new(None) };
}

fn intern_hle_call_name(name: String) -> &'static str {
    static NAMES: StdMutex<Option<std::collections::HashSet<&'static str>>> = StdMutex::new(None);
    let mut names = NAMES.lock().unwrap();
    let names = names.get_or_insert_with(Default::default);
    match names.get(name.as_str()) {
        Some(&n) => n,
        None => {
            let n: &'static str = Box::leak(name.into_boxed_str());
            names.insert(n);
            n
        }
    }
}

pub fn record_hle_call(name: impl Into<std::borrow::Cow<'static, str>>) {
    let name: &'static str = match name.into() {
        std::borrow::Cow::Borrowed(n) => n,
        std::borrow::Cow::Owned(n) => intern_hle_call_name(n),
    };
    let ring = HLE_CALL_RING.with(|r| {
        r.get().unwrap_or_else(|| {
            let ring: &'static HleCallRing = Box::leak(Box::new(HleCallRing {
                next: AtomicUsize::new(0),
                slots: std::array::from_fn(|_| HleCallSlot {
                    seq: AtomicU64::new(0),
                    ptr: AtomicUsize::new(0),
                    len: AtomicUsize::new(0),
                }),
            }));
            HLE_CALL_RINGS.lock().unwrap().push(ring);
            r.set(Some(ring));
            ring
        })
    });
    #[cfg(target_arch = "x86_64")]
    let seq = unsafe { core::arch::x86_64::_rdtsc() };
    #[cfg(not(target_arch = "x86_64"))]
    let seq = HLE_CALL_SEQ.fetch_add(1, Ordering::Relaxed);
    let i = ring.next.load(Ordering::Relaxed);
    ring.next.store((i + 1) % HLE_CALL_RING_SIZE, Ordering::Relaxed);
    let slot = &ring.slots[i];
    slot.seq.store(0, Ordering::Relaxed);
    std::sync::atomic::fence(Ordering::Release);
    slot.ptr.store(name.as_ptr() as usize, Ordering::Relaxed);
    slot.len.store(name.len(), Ordering::Relaxed);
    slot.seq.store(seq, Ordering::Release);
}

/// The last HLE_CALL_HISTORY_SIZE recorded calls of all threads, oldest first.
fn hle_call_history() -> Vec<(u64, std::borrow::Cow<'static, str>)> {
    let mut all = Vec::new();
    for ring in HLE_CALL_RINGS.lock().unwrap().iter() {
        for slot in &ring.slots {
            let seq = slot.seq.load(Ordering::Acquire);
            let (ptr, len) = (slot.ptr.load(Ordering::Relaxed), slot.len.load(Ordering::Relaxed));
            std::sync::atomic::fence(Ordering::Acquire);
            if seq != 0 && slot.seq.load(Ordering::Relaxed) == seq {
                // The pointer and length of a 'static str, stored together under `seq`.
                let name: &'static str =
                    unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr as *const u8, len)) };
                all.push((seq, std::borrow::Cow::Borrowed(name)));
            }
        }
    }
    all.sort_unstable_by_key(|e| e.0);
    all.drain(..all.len().saturating_sub(HLE_CALL_HISTORY_SIZE));
    all
}

/// Called from the crash handler. Prints the most recent HLE calls in order,
/// oldest first, ending with whatever ran immediately before the fault.
/// Prints a summary of the ring buffer (call counts, then the most recent calls in order) to stdout. Used with
/// --hle-history-at=<present number> to see what the game polls while it sits on a loading screen.

// Wait graph: which guest thread is currently blocked on which sync object, and from where. Dumped by
// summarize_hle_call_history() to tell a deadlocked worker apart from an idle one.
type BlockedEntry = (&'static str, usize, [usize; 4], std::time::Instant, std::thread::Thread);
static BLOCKED: OnceLock<StdMutex<rustc_hash::FxHashMap<u64, BlockedEntry>>> = OnceLock::new();

fn blocked_table() -> &'static StdMutex<rustc_hash::FxHashMap<u64, BlockedEntry>> {
    BLOCKED.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

/// The guest frame pointer of the function that called an HLE function, from `fp` = x29 as the HLE function sees it:
/// natively its own frame record, which saved the guest's; lifted, the guest's x29 itself (HLE functions are host
/// calls that leave the guest registers as they were).
unsafe fn first_guest_fp(fp: usize) -> usize {
    if cfg!(nx_native) {
        *(fp as *const usize)
    } else {
        fp
    }
}

pub struct BlockGuard(u64);

impl BlockGuard {
    /// `fp` is x29 inside the HLE function (see report_long_wait); records the guest return-address chain.
    pub unsafe fn new(kind: &'static str, obj: usize, fp: usize) -> Self {
        let mut chain = [0usize; 4];
        let mut gfp = first_guest_fp(fp);
        for slot in chain.iter_mut() {
            if gfp < 0x1_0000_0000 || gfp & 7 != 0 {
                break;
            }
            *slot = *((gfp + 8) as *const usize);
            gfp = *(gfp as *const usize);
        }
        let key = crate::platform::os::current_thread_id();
        blocked_table()
            .lock()
            .unwrap()
            .insert(key, (kind, obj, chain, std::time::Instant::now(), std::thread::current()));
        BlockGuard(key)
    }
}

impl Drop for BlockGuard {
    fn drop(&mut self) {
        blocked_table().lock().unwrap().remove(&self.0);
    }
}

pub fn dump_blocked_threads() {
    let base = crate::guest::loader::main_base();
    let fmt = |a: usize| {
        if a == 0 {
            String::new()
        } else if base != 0 && a >= base && a < base + 0x5000000 {
            format!("guest+{:#x}", a - base)
        } else {
            format!("{:#x}", a)
        }
    };
    let table = blocked_table().lock().unwrap();
    let mut rows: Vec<_> = table.values().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.3.elapsed()));
    println!(
        "[wait-graph] {} threads currently blocked in HLE waits:",
        rows.len()
    );
    for (kind, obj, chain, since, thread) in rows {
        let c: Vec<String> = chain
            .iter()
            .map(|a| fmt(*a))
            .filter(|s| !s.is_empty())
            .collect();
        println!(
            "[wait-graph]   {:>6.1}s {:<28} {} on {:#x} <- {}",
            since.elapsed().as_secs_f32(),
            thread.name().unwrap_or("?"),
            kind,
            obj,
            c.join(" <- ")
        );
    }
}

pub fn summarize_hle_call_history() {
    dump_blocked_threads();
    let hist = hle_call_history();
    // Command recording and texture/buffer plumbing outnumber everything else; the wait condition is among the rest.
    let interesting = |n: &str| {
        !(n.starts_with("nvnCommandBuffer")
            || n.starts_with("hle_nvnTexture")
            || n.starts_with("hle_nvnBuffer")
            || n.starts_with("hle_nvnMemoryPool"))
    };
    let mut counts: rustc_hash::FxHashMap<&str, usize> = rustc_hash::FxHashMap::default();
    for (_, name) in hist.iter().filter(|(_, n)| interesting(n)) {
        *counts.entry(name.as_ref()).or_insert(0) += 1;
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    println!("[hle-history] last {} HLE calls, by count:", hist.len());
    for (name, n) in v.iter().take(40) {
        println!("[hle-history]   {:5} x {}", n, name);
    }
    println!("[hle-history] most recent 80 calls, oldest first:");
    let recent: Vec<_> = hist.iter().filter(|(_, n)| interesting(n)).collect();
    for (seq, name) in recent.iter().skip(recent.len().saturating_sub(80)) {
        println!("[hle-history]   #{} {}", seq, name);
    }
}

pub fn dump_hle_call_history() {
    let hist = hle_call_history();
    eprintln!(
        "    HLE Call History (last {} calls, oldest first):",
        hist.len()
    );
    for (seq, name) in hist.iter() {
        eprintln!("      #{}: {}", seq, name);
    }
}



/// nn::os memory heap bookkeeping for QueryMemoryInfo: the size SetMemoryHeapSize set, and the blocks allocated from it
/// (address -> size). Super Mario Bros. Wonder sizes its main heap as heap size minus what is already allocated.
pub(crate) static MEMORY_HEAP_SIZE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) static PROGRAM_SIZE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Bytes of the loaded program images (main and the SDK), for QueryMemoryInfo.
pub fn set_program_size(n: usize) {
    PROGRAM_SIZE.store(n, std::sync::atomic::Ordering::Relaxed);
}
pub(crate) static MEMORY_BLOCKS: StdMutex<Vec<(usize, usize)>> = StdMutex::new(Vec::new());




// Pool allocator for nn::mem::StandardAllocator.
// TotK passes a large mmap'd region to Initialize; we bump-allocate from it
// so that all TotK-visible objects stay in guest memory (not macOS malloc).
// Free is intentionally a no-op: the pool lives for the process lifetime and
// the TotK heap system does its own internal free-list management on top.







// Flag so we only install the pool-page watchpoint once (after first bump alloc).















static HANDHELD_OVERRIDE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

pub fn set_handheld(handheld: bool) {
    HANDHELD_OVERRIDE.store(
        if handheld { 1 } else { 0 },
        std::sync::atomic::Ordering::Release,
    );
    crate::horizon::oe::push_notification_message(0x1e); // MessageOperationModeChanged
    crate::horizon::oe::push_notification_message(0x1f); // MessagePerformanceModeChanged
    #[cfg(target_os = "macos")]
    crate::host::display::update_handheld_drawable(handheld);
    if handheld {
        println!("[+] HLE: Handheld mode active (720p target)");
    } else {
        println!("[+] HLE: Docked mode active (1080p target)");
    }
}

pub fn is_handheld() -> bool {
    match HANDHELD_OVERRIDE.load(std::sync::atomic::Ordering::Acquire) {
        0 => false,
        1 => true,
        _ => {
            static HANDHELD_ENV: OnceLock<bool> = OnceLock::new();
            *HANDHELD_ENV.get_or_init(|| {
                let h = crate::settings::get().system.handheld;
                if h {
                    println!(
                        "[+] HLE: Handheld mode active (720p target; --docked for docked 1080p)"
                    );
                } else {
                    println!(
                        "[+] HLE: Docked mode active (1080p target; --handheld for handheld 720p)"
                    );
                }
                h
            })
        }
    }
}





#[repr(C)]
pub struct NnAccountUid {
    pub high: u64,
    pub low: u64,
}








// 9. Threading & TLS Management
pub(crate) static NEXT_TLS_SLOT: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
/// nn::os::MemoryInfo.
pub struct NnMemoryInfo {
    pub total_available_memory_size: u64,
    pub total_used_memory_size: u64,
    pub total_memory_heap_size: u64,
    pub allocated_memory_heap_size: u64,
    pub program_size: u64,
    pub total_thread_stack_size: u64,
    pub thread_count: i32,
}








#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct NpadFullKeyState {
    pub sampling_number: i64,
    pub buttons: u64,
    pub analog_stick_l_x: i32,
    pub analog_stick_l_y: i32,
    pub analog_stick_r_x: i32,
    pub analog_stick_r_y: i32,
    pub attributes: u32,
    pub _reserved: u32,
}

static SAMPLING_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);



pub(crate) static HID_SUPPORTED_STYLES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// NpadJoyHoldType: Vertical = 0 (the default), Horizontal = 1.
pub(crate) static HID_HOLD_TYPE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);



/// Fills a TouchScreenState<n> from the window's contacts (host::input::touches).
pub(crate) unsafe fn write_touch_screen_state(out: *mut u8, n: usize) {
    if out.is_null() {
        return;
    }
    std::ptr::write_bytes(out, 0, 16 + n * 0x28);
    static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let now = std::time::Instant::now();
    // The touch panel is sampled at ~200 Hz on the console: the sampling number advances with time.
    let sampling = now.duration_since(*T0.get_or_init(|| now)).as_millis() as i64 / 5;
    *(out as *mut i64) = sampling;
    let touches = crate::host::input::touches();
    let count = touches.len().min(n);
    *(out.add(8) as *mut i32) = count as i32;
    for (i, t) in touches.iter().take(count).enumerate() {
        let p = out.add(16 + i * 0x28);
        let mut attributes = 0u32;
        if t.fresh {
            attributes |= 1; // Start
        }
        if t.ended {
            attributes |= 2; // End
        }
        *(p as *mut i64) = 0;
        *(p.add(8) as *mut u32) = attributes;
        *(p.add(12) as *mut i32) = t.finger_id;
        *(p.add(16) as *mut i32) = t.x;
        *(p.add(20) as *mut i32) = t.y;
        *(p.add(24) as *mut i32) = 15; // diameterX / Y: a fingertip
        *(p.add(28) as *mut i32) = 15;
    }
}
/// SixAxisSensorState (96 bytes): 200 Hz accelerometer + gyroscope samples.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SixAxisSensorState {
    pub delta_ns: i64,
    pub sampling_number: i64,
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub angle: [f32; 3],
    pub direction: [f32; 9],
    pub attributes: u32,
    pub _pad: u32,
}

impl Default for SixAxisSensorState {
    fn default() -> Self {
        Self {
            delta_ns: 5_000_000, // 5 ms (200 Hz)
            sampling_number: 0,
            acceleration: [0.0, 0.0, -1.0], // Gravity vector: 1G downwards
            angular_velocity: [0.0, 0.0, 0.0],
            angle: [0.0, 0.0, 0.0],
            direction: [
                1.0, 0.0, 0.0,
                0.0, 1.0, 0.0,
                0.0, 0.0, 1.0,
            ],
            attributes: 1, // SixAxisSensorAttribute_IsConnected
            _pad: 0,
        }
    }
}

/// The first player's controller motion (host/input.rs), when it has a gyroscope; otherwise the default: flat and still.
pub(crate) fn with_motion(state: &mut SixAxisSensorState) {
    if let Some((acc, w, angle, dir)) = crate::host::input::motion_state(0) {
        state.acceleration = acc;
        state.angular_velocity = w;
        state.angle = angle;
        state.direction = dir;
    }
}






/// The shared body of the GetNpadStates overloads (all state structs have the NpadFullKeyState layout).
/// Supports up to 8 players (NpadId No1..No8, plus Handheld mapped to player 0);
/// `extra_attributes` adds style-specific attribute bits (JoyDual: IsLeftConnected | IsRightConnected).
pub(crate) unsafe fn npad_states(
    states: *mut NpadFullKeyState,
    count: i32,
    _npad_id: *const u32,
    extra_attributes: u32,
) -> i32 {
    let id = if _npad_id.is_null() {
        u32::MAX
    } else {
        *_npad_id
    };
    let player = match id {
        0..=7 => id as usize,
        0x20 => 0, // Handheld maps to player 0
        _ => usize::MAX,
    };
    let connected = player < crate::host::input::MAX_PLAYERS
        && crate::host::input::is_player_connected(player);
    if states.is_null() || count <= 0 {
        return 0;
    }
    if player == 0 {
        crate::host::input::before_npad_read();
    }
    // Keyboard / controller state mapped by the user's bindings (host/input.rs).
    let (buttons, sticks) = if connected {
        crate::host::input::player_npad_state(player)
    } else {
        (0, [0; 4])
    };
    // --log-hid-buttons: every read that returns pressed buttons (which id / style path the game polls).
    static LOG_BUTTONS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if buttons != 0 && *LOG_BUTTONS.get_or_init(|| crate::settings::get().log.log_hid_buttons) {
        let fp = crate::guest_reg!(x29);
        let lr = if fp > 0x10000 {
            *((fp + 8) as *const usize)
        } else {
            0
        };
        let _ = lr;
        println!(
            "[hid-buttons] frame {} id {:#x} player {} buttons {:#x} attrs {:#x} callers: {}",
            crate::gpu::backend().presented_frames(),
            id,
            player,
            buttons,
            1 | extra_attributes,
            guest_chain(fp)
        );
    }
    let n = count.min(1) as usize;
    for i in 0..n {
        let cur = states.add(i);
        std::ptr::write(
            cur,
            NpadFullKeyState {
                sampling_number: SAMPLING_COUNTER
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                buttons: buttons as _,
                analog_stick_l_x: sticks[0],
                analog_stick_l_y: sticks[1],
                analog_stick_r_x: sticks[2],
                analog_stick_r_y: sticks[3],
                attributes: if connected { 1 | extra_attributes } else { 0 }, // IsConnected
                _reserved: 0,
            },
        );
    }
    n as i32
}












/// The last vibration value sent to each motor handle.
pub(crate) static VIBRATION_SENT: StdMutex<Option<rustc_hash::FxHashMap<u32, [f32; 4]>>> = StdMutex::new(None);

pub(crate) fn send_vibration(handle: u32, v: [f32; 4]) {
    VIBRATION_SENT.lock().unwrap().get_or_insert_with(Default::default).insert(handle, v);
    crate::host::rumble::set(handle, v);
}






























// nn::os::ThreadType::State.
pub(crate) const THREAD_STATE_NOT_INITIALIZED: u8 = 0;
pub(crate) const THREAD_STATE_INITIALIZED: u8 = 1;
pub(crate) const THREAD_STATE_STARTED: u8 = 3;
pub(crate) const THREAD_STATE_EXITED: u8 = 4;

type ExitSignal = std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>;
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




#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_SetPriorityOnCurrentThread(_prio: i32) {}





// Per-thread core assignment. TotK's task system dispatches per core (0..2), so a thread must
// report the core its affinity selected: GetCurrentCoreNumber used to return a constant 1 for
// every thread, which left per-core task queues stranded (worker threads waited forever).
static THREAD_CORES: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, i32>>> = OnceLock::new();
pub(crate) const DEFAULT_CORE: i32 = 1;
/// Bumped whenever a thread's core changes (cores.rs caches each thread's core against it).
pub static CORES_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn thread_cores() -> &'static StdMutex<rustc_hash::FxHashMap<usize, i32>> {
    THREAD_CORES.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

/// Core number the calling guest thread runs on (nn::os::GetCurrentCoreNumber).
pub fn current_core() -> u32 {
    let t = unsafe { crate::horizon::os::get_current_thread() as usize };
    thread_cores()
        .lock()
        .unwrap()
        .get(&t)
        .copied()
        .unwrap_or(DEFAULT_CORE) as u32
}





// nn::os::AllocateAddressRegion / AllocateMemoryPages / FreeAddressRegion / FreeMemoryPages: Horizon's
// reserve-then-commit virtual memory, used directly (bypassing StandardAllocator) by the game's page allocators.
// Regions are mapped read/write up front; AllocateMemoryPages zeroes the guest pages it commits. A side table
// tracks each region's size, since FreeAddressRegion takes only an address.
static ADDRESS_REGIONS: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, usize>>> = OnceLock::new();

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
const REGION_ARENA_SIZES: &[usize] = &[16 << 30, 8 << 30, 4 << 30, 2 << 30];
/// iPhone 16e without the entitlement (probe, 2026-10-08): 4 GB largest reservation, ~7 GB in total; the game heap
/// (AllocateMemoryBlock, 3.2 GB for Kirby) must still fit next to the arena.
#[cfg(target_os = "ios")]
const REGION_ARENA_SIZES: &[usize] = &[2 << 30, 1 << 30];
pub(crate) const REGION_ALIGN: usize = 0x10000; // Horizon requires 64KB alignment for address regions

struct RegionArena {
    base: usize,
    size: usize,
    /// --file-backed-memory: the arena's pages come from this sparse file, at their offset from `base`.
    #[cfg(target_vendor = "apple")]
    file: Option<std::fs::File>,
    /// Free ranges: start -> size (coalesced, 64KB granular).
    free: std::collections::BTreeMap<usize, usize>,
}

fn region_arena() -> &'static StdMutex<RegionArena> {
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





#[no_mangle]
pub unsafe extern "C" fn hle_strcmp(s1: *const c_char, s2: *const c_char) -> i32 {
    libc::strcmp(s1, s2)
}

/// printf, formatted here: the guest passes variadic arguments in registers (AAPCS64), where neither the host's C
/// library on Apple (variadics on the stack) nor a call through an adapter (hle_abi) would find them. Integer, string
/// and pointer conversions take x1..x7 in turn; floating-point ones (passed in v registers) print as "?".
#[no_mangle]
pub unsafe extern "C" fn hle_printf(fmt: *const c_char, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64, a7: u64) -> i32 {
    if fmt.is_null() {
        return 0;
    }
    let out = format_printf(std::ffi::CStr::from_ptr(fmt).to_bytes(), &[a1, a2, a3, a4, a5, a6, a7]);
    print!("{}", String::from_utf8_lossy(&out));
    out.len() as i32
}

fn format_printf(fmt: &[u8], args: &[u64]) -> Vec<u8> {
    let mut args = args.iter().copied();
    let mut out = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' {
            out.push(fmt[i]);
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        // Flags, width, precision and length are skipped (a '*' takes an argument).
        let mut long = false;
        while i < fmt.len() && b"-+ #0123456789.*hlLqjzt".contains(&fmt[i]) {
            match fmt[i] {
                b'*' => drop(args.next()),
                b'l' | b'L' | b'q' | b'j' | b'z' | b't' => long = true,
                _ => {}
            }
            i += 1;
        }
        let Some(&c) = fmt.get(i) else { break };
        i += 1;
        let v = |a: Option<u64>| a.unwrap_or(0);
        let s = match c {
            b'%' => "%".to_string(),
            b'd' | b'i' => if long { (v(args.next()) as i64).to_string() } else { (v(args.next()) as i32).to_string() },
            b'u' => if long { v(args.next()).to_string() } else { (v(args.next()) as u32).to_string() },
            b'x' => if long { format!("{:x}", v(args.next())) } else { format!("{:x}", v(args.next()) as u32) },
            b'X' => if long { format!("{:X}", v(args.next())) } else { format!("{:X}", v(args.next()) as u32) },
            b'o' => if long { format!("{:o}", v(args.next())) } else { format!("{:o}", v(args.next()) as u32) },
            b'p' => format!("{:#x}", v(args.next())),
            b'c' => char::from(v(args.next()) as u8).to_string(),
            b's' => match v(args.next()) {
                0 => "(null)".to_string(),
                p => String::from_utf8_lossy(unsafe { std::ffi::CStr::from_ptr(p as *const c_char) }.to_bytes()).into_owned(),
            },
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => "?".to_string(),
            _ => String::from_utf8_lossy(&fmt[start..i]).into_owned(),
        };
        out.extend_from_slice(s.as_bytes());
    }
    out
}

#[cfg(test)]
mod printf_tests {
    #[test]
    fn formats_integers_strings_and_unknowns() {
        let s = b"str\0";
        let out = super::format_printf(b"%d %5u %lx %s %c %% %f %y", &[(-3i32) as u32 as u64, 7, 0xabc, s.as_ptr() as u64, b'z' as u64]);
        assert_eq!(String::from_utf8(out).unwrap(), "-3 7 abc str z % ? %y");
    }
}





// Reader-writer lock: real exclusion, tracked in a side table keyed by the lock's address
// (horizon/nn.rs).
#[repr(C)]
pub struct NnReaderWriterLock {
    pub lock_state: u32,
    pub thread_id: u64,
}









// nn::os::LightMessageQueue: a bounded, blocking SPSC/MPMC queue. Real hardware backs
// it with the guest-supplied buffer; we keep the actual
// ring storage host-side, keyed by the guest queue pointer, and use a real Condvar so
// Receive blocks instead of busy-spinning (this was causing VibrationThread to spin at
// ~1M calls/sec on an unbound GOT stub before these were wired up).
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex as StdMutex, OnceLock};

pub(crate) struct LightMsgQueueInner {
    pub(crate) capacity: usize,
    pub(crate) queue: VecDeque<u64>,
}

pub(crate) struct LightMsgQueueState {
    pub(crate) inner: StdMutex<LightMsgQueueInner>,
    pub(crate) not_empty: Condvar,
    pub(crate) not_full: Condvar,
}

static LIGHT_MQ_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<LightMsgQueueState>>>> =
    OnceLock::new();

pub(crate) fn light_mq_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<LightMsgQueueState>>> {
    LIGHT_MQ_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

pub(crate) fn light_mq_new(capacity: usize) -> Arc<LightMsgQueueState> {
    Arc::new(LightMsgQueueState {
        inner: StdMutex::new(LightMsgQueueInner {
            capacity: capacity.max(1),
            queue: VecDeque::new(),
        }),
        not_empty: Condvar::new(),
        not_full: Condvar::new(),
    })
}

pub(crate) unsafe fn light_mq_get(mq: *mut ()) -> Arc<LightMsgQueueState> {
    let key = mq as usize;
    let mut table = light_mq_table().lock().unwrap();
    table.entry(key).or_insert_with(|| light_mq_new(64)).clone()
}








static PRESENT_PACED: StdMutex<Vec<&'static str>> = StdMutex::new(Vec::new());

/// Light message queue receives on threads with this name return only after the next present (at most 20 ms later).
pub fn pace_to_present(thread_name: &'static str) {
    PRESENT_PACED.lock().unwrap().push(thread_name);
}

pub(crate) fn present_paced_wait() {
    thread_local! {
        static PACED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    }
    let paced = PACED.with(|p| {
        if let Some(v) = p.get() {
            return v;
        }
        let v = (|| {
            let names = PRESENT_PACED.lock().unwrap();
            if names.is_empty() {
                return false;
            }
            // The name the game gave this thread (nn::os::SetThreadName), the same on every host; the host thread's own
            // name is set differently per platform.
            let p = unsafe { crate::horizon::os::get_thread_name_pointer(std::ptr::null()) };
            let name = if p.is_null() { &[][..] } else { unsafe { CStr::from_ptr(p) }.to_bytes() };
            names.iter().any(|n| n.as_bytes() == name)
        })();
        p.set(Some(v));
        v
    });
    if !paced {
        return;
    }
    let Some(g) = crate::gpu::try_backend() else { return };
    let f0 = g.presented_frames();
    let until = std::time::Instant::now() + std::time::Duration::from_millis(20);
    while g.presented_frames() == f0 && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
}




/// Send (or jam at the head) with a timeout; false if the queue stayed full.
pub(crate) unsafe fn light_mq_timed_put(mq: *mut (), data: u64, front: bool, timeout_ns: i64) -> bool {
    let state = light_mq_get(mq);
    let inner = state.inner.lock().unwrap();
    let (mut inner, _) = state
        .not_full
        .wait_timeout_while(inner, std::time::Duration::from_nanos(timeout_ns.max(0) as u64), |i| {
            i.queue.len() >= i.capacity
        })
        .unwrap();
    if inner.queue.len() >= inner.capacity {
        return false;
    }
    if front {
        inner.queue.push_front(data);
    } else {
        inner.queue.push_back(data);
    }
    state.not_empty.notify_all();
    true
}





// nn::os::ConditionVariable: was previously a complete no-op pair --
// SignalConditionVariable did nothing at all, and WaitConditionVariable never
// actually waited for a signal (just unlock/yield-once/relock regardless of
// whether the awaited condition was ever set). Any guest code using the
// standard `while (!condition) WaitConditionVariable(&cv, &mutex);` pattern
// got no real synchronization: it would spin, see whatever state happened to
// be there, and proceed -- exactly the shape of bug that produces
// "impossible" cross-thread state without there being a genuine memory-
// corruption bug anywhere. Implemented the same way as LightMessageQueue
// earlier: a real Condvar keyed by the guest cv pointer, in a side table
// since the actual wait/notify state doesn't need to live in guest memory.
pub(crate) struct CondVarState {
    pub(crate) generation: StdMutex<u64>,
    pub(crate) cv: Condvar,
}

static CONDVAR_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<CondVarState>>>> =
    OnceLock::new();

pub(crate) fn condvar_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<CondVarState>>> {
    CONDVAR_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

pub(crate) fn condvar_new() -> Arc<CondVarState> {
    Arc::new(CondVarState {
        generation: StdMutex::new(0),
        cv: Condvar::new(),
    })
}

pub(crate) unsafe fn condvar_get(cv: *mut ()) -> Arc<CondVarState> {
    let key = cv as usize;
    let mut table = condvar_table().lock().unwrap();
    table.entry(key).or_insert_with(condvar_new).clone()
}







// nn::os::LightEvent: a simple signaled/not-signaled event with manual or
// auto clear-on-wait semantics (EventClearMode_ManualClear = 0,
// EventClearMode_AutoClear = 1). Previously a complete no-op across
// Initialize/Signal/Wait, with Finalize/Clear/TimedWait not even bound to
// GOT -- so any producer/consumer pair coordinating through one (e.g. a
// worker thread signaling "data ready" to a decompression thread) never
// actually blocked or woke correctly: Wait returned immediately regardless
// of real state, letting a consumer run ahead and read memory its producer
// hadn't written yet. That's exactly the shape of the wild-garbage-pointer
// crash observed on the native ZstdDecompThread once TLS setup stopped
// masking it. Implemented the same way as LightMessageQueue/ConditionVariable:
// real state (signaled bool + clear mode) in a side table keyed by the guest
// event pointer, with a Condvar for real blocking.
pub(crate) struct LightEventState {
    pub(crate) signaled: StdMutex<bool>,
    pub(crate) auto_clear: std::sync::atomic::AtomicBool,
    pub(crate) cv: Condvar,
    /// Signals so far, counted under `signaled`'s lock: a manual-clear event signaled and cleared again before a
    /// waiter woke still releases that waiter (it was waiting when the signal came), as on the console.
    pub(crate) signals: std::sync::atomic::AtomicU64,
}

static LIGHT_EVENT_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<LightEventState>>>> =
    OnceLock::new();

pub(crate) fn light_event_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<LightEventState>>> {
    LIGHT_EVENT_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

pub(crate) fn light_event_new(initially_signaled: bool, clear_mode: i32) -> Arc<LightEventState> {
    Arc::new(LightEventState {
        signaled: StdMutex::new(initially_signaled),
        auto_clear: std::sync::atomic::AtomicBool::new(clear_mode == 1),
        cv: Condvar::new(),
        signals: std::sync::atomic::AtomicU64::new(0),
    })
}

/// Spins up to LIGHT_EVENT_SPIN for a signal before a wait blocks: TotK's job threads wait tens of thousands of times a
/// second, mostly for tens of microseconds, about what a sleep and a wake-up cost on Windows. A tuning number: it trades
/// CPU time spinning against wake-up latency; the game sees the same signal at the same point either way.
pub(crate) fn light_event_spin(state: &LightEventState) {
    const LIGHT_EVENT_SPIN: std::time::Duration = std::time::Duration::from_micros(30);
    let s0 = state.signals.load(std::sync::atomic::Ordering::Relaxed);
    if state.signaled.try_lock().map_or(true, |g| *g) {
        return;
    }
    let t0 = std::time::Instant::now();
    while state.signals.load(std::sync::atomic::Ordering::Relaxed) == s0 {
        for _ in 0..32 {
            std::hint::spin_loop();
        }
        if t0.elapsed() >= LIGHT_EVENT_SPIN {
            return;
        }
    }
}

pub(crate) unsafe fn light_event_get(ev: *mut ()) -> Arc<LightEventState> {
    let key = ev as usize;
    let mut table = light_event_table().lock().unwrap();
    table
        .entry(key)
        .or_insert_with(|| light_event_new(false, 0))
        .clone()
}






/// Debug aid: report (once per wait) a blocking nn::os wait that has not completed after 5s,
/// with the guest call site, so whole-process stalls can be attributed to a specific wait.
pub(crate) fn guest_chain(fp: usize) -> String {
    let base = crate::guest::loader::main_base();
    let fmt = |a: usize| {
        if base != 0 && a >= base && a < base + 0x5000000 {
            format!("guest+{:#x}", a - base)
        } else {
            format!("{:#x}", a)
        }
    };
    let mut chain = Vec::new();
    unsafe {
        let mut gfp = first_guest_fp(fp);
        for _ in 0..6 {
            if gfp < 0x1_0000_0000 || gfp & 7 != 0 {
                break;
            }
            chain.push(fmt(*((gfp + 8) as *const usize)));
            gfp = *(gfp as *const usize);
        }
    }
    chain.join(" <- ")
}

pub fn report_long_wait(what: &str, obj: usize, fp: usize, waited: std::time::Duration) {
    // `fp` is x29 inside the HLE function: its frame record's saved x29 is the guest's FP (the
    // trampoline doesn't change it), so [guest_fp + 8] is the return address of the guest function
    // that called the wait, followed up the guest FP chain.
    let base = crate::guest::loader::main_base();
    let fmt = |a: usize| {
        if base != 0 && a >= base && a < base + 0x5000000 {
            format!("guest+{:#x}", a - base)
        } else {
            format!("{:#x}", a)
        }
    };
    let mut chain = Vec::new();
    unsafe {
        let mut gfp = first_guest_fp(fp);
        for _ in 0..6 {
            if gfp < 0x1_0000_0000 || gfp & 7 != 0 {
                break;
            }
            chain.push(fmt(*((gfp + 8) as *const usize)));
            gfp = *(gfp as *const usize);
        }
    }
    eprintln!(
        "[STALL] {} on {:#x} blocked {}s (guest callers: {})",
        what,
        obj,
        waited.as_secs(),
        chain.join(" <- ")
    );
}




// ── nn::os::TimerEvent ─────────────────────────────────────────────────────────
//
// TimerEvent is a timed synchronization primitive supporting one-shot and
// periodic intervals with automatic or manual clear semantics.
// Native Switch implementations use Horizon kernel condition variables
// and critical sections which issue Switch SVCs (svc 0x1c, 0x1d) and check
// thread handles, causing aborts on Darwin. This HLE provides complete
// standard condition variable and deadline based blocking.

pub(crate) struct TimerEventInner {
    pub(crate) clear_mode: u8,
    pub(crate) signaled: bool,
    pub(crate) timer_mode: u8, // 0 = stopped, 1 = one-shot, 2 = periodic
    pub(crate) next_wake: Option<std::time::Instant>,
    pub(crate) first_time_ns: i64,
    pub(crate) interval_ns: i64,
}

pub(crate) struct TimerEventState {
    pub(crate) inner: StdMutex<TimerEventInner>,
    pub(crate) cv: Condvar,
}

static TIMER_EVENT_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<TimerEventState>>>> =
    OnceLock::new();

pub(crate) fn timer_event_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<TimerEventState>>> {
    TIMER_EVENT_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

pub(crate) fn timer_event_new(clear_mode: u8) -> Arc<TimerEventState> {
    Arc::new(TimerEventState {
        inner: StdMutex::new(TimerEventInner {
            clear_mode,
            signaled: false,
            timer_mode: 0,
            next_wake: None,
            first_time_ns: 0,
            interval_ns: 0,
        }),
        cv: Condvar::new(),
    })
}

pub(crate) unsafe fn timer_event_get(ev: *mut ()) -> Arc<TimerEventState> {
    let key = ev as usize;
    let mut table = timer_event_table().lock().unwrap();
    table
        .entry(key)
        .or_insert_with(|| timer_event_new(1))
        .clone()
}










// nn::os::Semaphore / nn::os::LightSemaphore: a real counting semaphore
// (Acquire blocks while count == 0, then decrements; Release adds N up to
// max). Previously a complete no-op for the (bound) Semaphore family, and
// InitializeLightSemaphore/AcquireLightSemaphore/ReleaseLightSemaphore/
// FinalizeLightSemaphore weren't even bound to GOT -- so a producer/consumer
// pair coordinating a bounded work queue through one (exactly the pattern
// TotK's DecompThread/MeshCodecDecompresionThread/ZstdDecompThread use to
// hand off compressed chunks) never actually blocked: Acquire returned
// immediately with whatever count happened to be there, letting a consumer
// run past data its producer hadn't produced yet. Same side-table + Condvar
// approach as LightEvent/ConditionVariable above.
struct SemaphoreState {
    count: StdMutex<i32>,
    max: i32,
    cv: Condvar,
}

static SEMAPHORE_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<SemaphoreState>>>> =
    OnceLock::new();

fn semaphore_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<SemaphoreState>>> {
    SEMAPHORE_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

fn semaphore_new(initial: i32, max: i32) -> Arc<SemaphoreState> {
    Arc::new(SemaphoreState {
        count: StdMutex::new(initial),
        max,
        cv: Condvar::new(),
    })
}

unsafe fn semaphore_get(sem: *mut ()) -> Arc<SemaphoreState> {
    let key = sem as usize;
    let mut table = semaphore_table().lock().unwrap();
    table
        .entry(key)
        .or_insert_with(|| semaphore_new(0, i32::MAX))
        .clone()
}

pub(crate) unsafe fn semaphore_init(sem: *mut (), initial: i32, max: i32) {
    semaphore_table()
        .lock()
        .unwrap()
        .insert(sem as usize, semaphore_new(initial, max));
}

pub(crate) unsafe fn semaphore_finalize(sem: *mut ()) {
    semaphore_table().lock().unwrap().remove(&(sem as usize));
}

pub(crate) unsafe fn semaphore_acquire(sem: *mut (), fp: usize) {
    let state = semaphore_get(sem);
    let mut count = state.count.lock().unwrap();
    let _blocked = if *count > 0 {
        None
    } else {
        Some(BlockGuard::new("Semaphore", sem as usize, fp))
    };
    count = state.cv.wait_while(count, |c| *c <= 0).unwrap();
    *count -= 1;
}

pub(crate) unsafe fn semaphore_try_acquire(sem: *mut (), timeout_ns: i64) -> bool {
    let state = semaphore_get(sem);
    let count = state.count.lock().unwrap();
    let (mut count, _) = state
        .cv
        .wait_timeout_while(count, std::time::Duration::from_nanos(timeout_ns.max(0) as u64), |c| *c <= 0)
        .unwrap();
    if *count <= 0 {
        return false;
    }
    *count -= 1;
    true
}

pub(crate) unsafe fn semaphore_count(sem: *const ()) -> i32 {
    *semaphore_get(sem as *mut ()).count.lock().unwrap()
}




pub(crate) unsafe fn semaphore_release(sem: *mut (), release_count: i32) {
    let state = semaphore_get(sem);
    {
        let mut count = state.count.lock().unwrap();
        *count = (*count + release_count).min(state.max);
    }
    state.cv.notify_all();
}










// 12. Symbol-based (R_AARCH64_ABS64 / R_AARCH64_GLOB_DAT) relocation targets.
//
// The loader previously only applied R_AARCH64_RELATIVE relocations. The other
// 1,444 entries in the binary's .rela.dyn are symbol-based, resolving against
// *external* symbols the linker expected some other module to provide -- on a
// real Switch, the C++ runtime (libc++abi-equivalent, linked in via `sdk`).
// Two symbols alone account for 1,344 of those (95%): `__cxa_pure_virtual` and
// `__cxa_deleted_virtual`, the Itanium C++ ABI's standard vtable-slot fillers
// for any pure-virtual or deleted method a derived class doesn't override --
// completely routine in any real C++ binary. Leaving them unresolved meant
// those vtable slots sat at raw zero instead of a safe handler, so calling an
// unoverridden method (an expected, frequent occurrence, not a bug in the
// guest code) crashed the whole engine on a null function pointer.
//
// These are called with whatever "this"/args the original virtual call used;
// since we don't know the real signature at the call site, they take no
// declared params and just report and return, matching the rest of this
// file's "log the unimplemented case, then let execution continue" approach.
#[no_mangle]
pub unsafe extern "C" fn hle_cxa_pure_virtual() {
    println!("[!] __cxa_pure_virtual called (unoverridden pure virtual method)");
}

#[no_mangle]
pub unsafe extern "C" fn hle_cxa_deleted_virtual() {
    println!("[!] __cxa_deleted_virtual called (deleted virtual method)");
}

// Minimal RTTI vtable stand-ins for __cxxabiv1::__class_type_info /
// __si_class_type_info. A real implementation of typeid/dynamic_cast needs
// these to behave correctly, but the common failure mode we're guarding
// against is simply "something calls through this vtable" -- so this is a
// small block of safe no-op slots (bigger than either class actually needs)
// rather than a faithful RTTI implementation.
#[no_mangle]
pub unsafe extern "C" fn hle_rtti_stub_slot() -> i64 {
    0
}

const RTTI_VTABLE_LEN: usize = 12;
static RTTI_CLASS_TYPE_INFO_VTABLE: [unsafe extern "C" fn() -> i64; RTTI_VTABLE_LEN] =
    [hle_rtti_stub_slot; RTTI_VTABLE_LEN];
static RTTI_SI_CLASS_TYPE_INFO_VTABLE: [unsafe extern "C" fn() -> i64; RTTI_VTABLE_LEN] =
    [hle_rtti_stub_slot; RTTI_VTABLE_LEN];

pub fn rtti_class_type_info_vtable_addr() -> usize {
    RTTI_CLASS_TYPE_INFO_VTABLE.as_ptr() as usize
}
pub fn rtti_si_class_type_info_vtable_addr() -> usize {
    RTTI_SI_CLASS_TYPE_INFO_VTABLE.as_ptr() as usize
}

pub static STD_NOTHROW: u8 = 0;
pub static mut NVN_PRIVATE0: [u64; 64] = [0; 64];
// Placeholder FILE* for the "stdout" data symbol; guest code using it just
// needs a stable, non-null, distinct address, not real stdio interop.

// ── nn::audio renderer (IPC to audren:u) ────────────────────────────────────
// The renderer lifecycle, work-buffer sizing and output-device queries all end
// in HIPC calls to the audio sysmodule, so the sdk module's code can't run here
// (it crashed in CreateAudioRendererManagerByHipc -> AllocateFromExpHeap with a
// null heap). The pure-CPU parts of nn::audio (config/mixer/voice bookkeeping,
// Set*/Get*/Acquire*/Add* on AudioRendererConfig) still run in the sdk module
// against the work buffers the game hands us; only the IPC edge is replaced.
// There's no audio backend yet: updates are accepted and nothing is mixed.
pub(crate) const NN_RESULT_SUCCESS: u32 = 0;



pub(crate) static NN_AUDIO_RENDERER_STATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// ── nn::os::SystemEvent (kernel readable-event handle) ──────────────────────
// SystemEventType state normally comes from an svc-created kernel event; the
// sdk module TryWait/WaitSystemEvent abort (nn::diag::detail::AbortImpl) on an
// event they didn't initialize. Events handed out by our HLE (audio renderer,
// audio device notification, ...) are tracked here by address instead, with
// auto-clear semantics, and Wait/TryWait/Destroy are HLE over this registry.
pub(crate) struct HleSystemEvent {
    pub(crate) signaled: StdMutex<bool>,
    pub(crate) cv: std::sync::Condvar,
}
pub(crate) static HLE_SYSTEM_EVENTS: StdMutex<Vec<(usize, std::sync::Arc<HleSystemEvent>)>> =
    StdMutex::new(Vec::new());

pub fn hle_system_event_register_addr(addr: usize) {
    hle_system_event_register(addr);
}
pub fn hle_system_event_signal_addr(addr: usize) {
    if let Some(ev) = hle_system_event_lookup(addr) {
        hle_system_event_signal(&ev);
    }
}
/// Registers an HLE system event at `addr` that is already signaled (an operation that completed at once).
pub fn hle_system_event_register_signaled(addr: usize) {
    let ev = hle_system_event_register(addr);
    *ev.signaled.lock().unwrap() = true;
    ev.cv.notify_all();
}

pub(crate) fn hle_system_event_register(addr: usize) -> std::sync::Arc<HleSystemEvent> {
    let ev = std::sync::Arc::new(HleSystemEvent {
        signaled: StdMutex::new(false),
        cv: std::sync::Condvar::new(),
    });
    let mut v = HLE_SYSTEM_EVENTS.lock().unwrap();
    v.retain(|(a, _)| *a != addr);
    v.push((addr, ev.clone()));
    ev
}
// nn::os::SystemEvent (class) wraps a SystemEventType; accept a pointer to either,
// i.e. the registered address or a small offset into it.
pub(crate) fn hle_system_event_lookup(addr: usize) -> Option<std::sync::Arc<HleSystemEvent>> {
    HLE_SYSTEM_EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter(|(a, _)| addr >= *a && addr - *a < 0x40)
        .min_by_key(|(a, _)| addr - *a)
        .map(|(_, e)| e.clone())
}
fn hle_system_event_signal(ev: &HleSystemEvent) {
    *ev.signaled.lock().unwrap() = true;
    ev.cv.notify_all();
}

/// audren signals the renderer's event once per audio frame (240 samples @ 48kHz = 5ms).
pub(crate) fn nn_audio_start_frame_pacer(ev: std::sync::Arc<HleSystemEvent>) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("hle-audren-pacer".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(5));
            if NN_AUDIO_RENDERER_STATE.load(std::sync::atomic::Ordering::SeqCst) == 2 {
                hle_system_event_signal(&ev);
            }
        })
        .expect("spawn audren pacer");
}



// ── nn::codec hardware Opus decoder (IPC to hwopus) ─────────────────────────
// Same story as audren: the decoder object lives in another sysmodule, so every
// entry point that touches it is HLE. Per-decoder state (sample rate, channels)
// is kept in a side table keyed by address and the getters are HLE too. Decoding goes through
// the SDK's software Opus decoder (zero external dependencies), falling back to libopus
// (horizon/audio/opus.rs), or silence with correct accounting;
// nn::codec::GetOpusPacketSize is pure CPU and stays in the sdk module.
pub(crate) static HLE_OPUS_DECODERS: StdMutex<Vec<(usize, i32, i32)>> = StdMutex::new(Vec::new()); // (addr, rate, channels)
// The SDK's own Opus functions are guest code: they run through guest::call (natively a call, lifted through the guest
// library), never as host function pointers.
/// Address of guest decoder -> (software decoder buffer address, is_multistream)
pub(crate) static SOFT_OPUS_MAP: StdMutex<Option<std::collections::HashMap<usize, (usize, bool)>>> = StdMutex::new(None);

/// Stand-in for the decoder's service object (HardwareOpusDecoderType::_handle): code that releases it calls
/// through its vtable, so every entry is a no-op.
unsafe extern "C" fn opus_handle_noop() -> u32 {
    0
}
pub(crate) static OPUS_HANDLE_VTABLE: [unsafe extern "C" fn() -> u32; 16] = [opus_handle_noop; 16];
pub(crate) static OPUS_HANDLE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// HardwareOpusDecoderType: _handle, _buffer, _size, _sampleRate, _channelCount,
/// _perf, _isResetRequested. Inline getters read these fields.
#[repr(C)]
pub(crate) struct OpusDecoderType {
    pub(crate) handle: usize,
    pub(crate) buffer: usize,
    pub(crate) size: usize,
    pub(crate) sample_rate: i32,
    pub(crate) channel_count: i32,
    pub(crate) perf: i64,
    pub(crate) reset_requested: bool,
}
pub(crate) fn hle_opus_decoder(dec: usize) -> Option<(i32, i32)> {
    HLE_OPUS_DECODERS
        .lock()
        .unwrap()
        .iter()
        .find(|(a, _, _)| *a == dec)
        .map(|(_, r, c)| (*r, *c))
}
/// Samples per channel in one Opus packet at `rate`, from the TOC byte (RFC 6716 §3.1).
pub(crate) fn opus_packet_samples(pkt: &[u8], rate: i32) -> usize {
    let Some(&toc) = pkt.first() else { return 0 };
    let cfg = toc >> 3;
    let frame_us: u32 = match cfg {
        0..=11 => [10_000, 20_000, 40_000, 60_000][(cfg & 3) as usize],
        12..=15 => [10_000, 20_000][(cfg & 1) as usize],
        _ => [2_500, 5_000, 10_000, 20_000][(cfg & 3) as usize],
    };
    let frames = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => pkt.get(1).map(|b| (b & 0x3f) as u32).unwrap_or(0),
    };
    (frame_us as u64 * frames as u64 * rate.max(0) as u64 / 1_000_000) as usize
}













// ── Guest-main sampler (--sample-main=<seconds>) ──────────────────────────────────────────────
// Suspends the guest-main thread every few ms, reads its register state and walks the frame-pointer chain, then
// prints the hottest guest call chains. Shows which guest function the main loop is actually running/waiting in.
#[cfg(nx_native)]
static SAMPLE_TARGET: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(nx_native)]
pub fn register_main_sample_target() {
    if crate::settings::get().debug.sample_main.is_some() {
        extern "C" {
            fn pthread_mach_thread_np(t: libc::pthread_t) -> u32;
        }
        SAMPLE_TARGET.store(
            unsafe { pthread_mach_thread_np(libc::pthread_self()) },
            std::sync::atomic::Ordering::SeqCst,
        );
        std::thread::Builder::new()
            .name("main-sampler".into())
            .spawn(sample_main_thread)
            .ok();
    }
}

#[cfg(nx_native)]
fn sample_main_thread() {
    extern "C" {
        fn thread_suspend(t: u32) -> i32;
        fn thread_resume(t: u32) -> i32;
        fn thread_get_state(t: u32, flavor: i32, state: *mut u64, count: *mut u32) -> i32;
    }
    let secs: u64 = crate::settings::get().debug.sample_main.unwrap_or_default();
    // Let the game get going first.
    let delay: u64 = crate::settings::get().debug.sample_delay;
    std::thread::sleep(std::time::Duration::from_secs(delay));
    let target = SAMPLE_TARGET.load(std::sync::atomic::Ordering::SeqCst);
    let base = crate::guest::loader::main_base();
    let in_guest = |a: usize| base != 0 && a >= base && a < base + 0x5000000;
    let mut hist: rustc_hash::FxHashMap<Vec<usize>, u32> = rustc_hash::FxHashMap::default();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    let mut total = 0u32;
    while std::time::Instant::now() < end {
        let mut st = [0u64; 68];
        let mut count = 68u32;
        let mut chain = Vec::new();
        unsafe {
            if thread_suspend(target) == 0 {
                if thread_get_state(target, 6, st.as_mut_ptr(), &mut count) == 0 {
                    // x[0..29], fp=st[29], lr=st[30], sp=st[31], pc=st[32]
                    let (pc, lr, mut fp) = (st[32] as usize, st[30] as usize, st[29] as usize);
                    chain.push(pc);
                    chain.push(lr);
                    for _ in 0..10 {
                        if fp < 0x1_0000_0000 || fp & 7 != 0 {
                            break;
                        }
                        let ret = *((fp + 8) as *const usize);
                        chain.push(ret);
                        fp = *(fp as *const usize);
                    }
                }
                thread_resume(target);
            }
        }
        if !chain.is_empty() {
            // Keep only guest frames: HLE/engine frames add noise.
            let guest: Vec<usize> = chain
                .iter()
                .copied()
                .filter(|a| in_guest(*a))
                .map(|a| a - base)
                .take(6)
                .collect();
            *hist.entry(guest).or_insert(0) += 1;
            total += 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut rows: Vec<_> = hist.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    println!(
        "[main-sample] {} samples of guest-main, hottest guest chains (innermost first):",
        total
    );
    for (chain, n) in rows.iter().take(15) {
        let c: Vec<String> = chain.iter().map(|a| format!("{:#x}", a)).collect();
        println!("[main-sample]   {:>5} {}", n, c.join(" <- "));
    }
}

