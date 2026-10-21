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
    let got_slot: usize;
    std::arch::asm!("mov {}, x16", out(reg) got_slot);
    let caller_lr: usize;
    std::arch::asm!("mov {}, x17", out(reg) caller_lr);
    let guest_base = crate::cpu::crash_handler::get_guest_base();
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
    // with this template (cpu/tls.rs), as nnmusl gives each thread a copy.
    let align = align_sym.wrapping_sub(base);
    let align = if align.is_power_of_two() && align <= 0x1000 {
        align
    } else {
        16
    };
    let n = tdata_end.saturating_sub(tdata_start);
    let template = if n > 0 && n <= 0x1000 && tdata_start >= 0x10000 {
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
    crate::cpu::tls::set_module_tls_template(template, align);
    1
}

// 3. Libc / standard math pass-throughs
#[no_mangle]
pub unsafe extern "C" fn hle_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    if (dest as usize) < 0x10000 || n == 0 {
        return dest;
    }
    libc::memset(dest as *mut libc::c_void, c, n) as *mut u8
}

#[no_mangle]
pub unsafe extern "C" fn hle_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if (dest as usize) < 0x10000 || (src as usize) < 0x10000 || n == 0 {
        return dest;
    }
    libc::memcpy(dest as *mut libc::c_void, src as *const libc::c_void, n) as *mut u8
}

#[no_mangle]
pub unsafe extern "C" fn hle_memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
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
use std::sync::atomic::AtomicU32;

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
    pub tls_value_array: [u64; 32],       // 0x80 _tlsValueArray (256 bytes)
    pub thread_name_buffer: [u8; 32],     // 0x180 _threadNameBuffer
    pub name_pointer: *const c_char,      // 0x1A0 _namePointer
    pub cs_thread: [u8; 4],               // 0x1A8 _csThread (InternalCriticalSectionStorage)
    pub cv_thread: [u8; 4],               // 0x1AC _cvThread (InternalConditionVariableStorage)
    pub handle: u32,                      // 0x1B0 _handle
}

const _: () = assert!(std::mem::size_of::<NnThreadType>() == 0x1B8);

pub static NEXT_THREAD_HANDLE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x100);

thread_local! {
    static CURRENT_NN_THREAD: RefCell<*mut NnThreadType> = const { RefCell::new(std::ptr::null_mut()) };
    static THREAD_SELF_STRUCT: RefCell<NnThreadType> = RefCell::new(NnThreadType {
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
        tls_value_array: [0; 32],
        thread_name_buffer: [0; 32],
        name_pointer: std::ptr::null(),
        cs_thread: [0; 4],
        cv_thread: [0; 4],
        handle: 1,
    });
    static THREAD_TLS_VALUES: RefCell<[u64; 64]> = const { RefCell::new([0; 64]) };
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
    #[cfg(target_arch = "aarch64")]
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
    #[cfg(target_arch = "aarch64")]
    {
        let tp: *mut u8;
        std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tp);
        if (tp as usize) >= 0x10000 {
            *(tp.add(0x1f8) as *mut usize) = thread_ptr as usize;
            *(tp.add(0x1f0) as *mut usize) = thread_ptr as usize;
            let current_errno_ptr = *(tp.add(8) as *const usize);
            if current_errno_ptr >= (tp as usize) && current_errno_ptr < (tp as usize) + 0x1000 {
                *(tp.add(crate::cpu::tls::errno_save_offset()) as *mut usize) = current_errno_ptr;
            }
            let guest_tls = crate::cpu::tls::get_current_guest_tls_block();
            if guest_tls >= 0x10000 {
                *(tp.add(crate::cpu::tls::guest_tls_offset()) as *mut usize) = guest_tls;
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
}

pub unsafe fn sanitize_darwin_tpidrro() {
    #[cfg(target_arch = "aarch64")]
    {
        let tp: *mut u8;
        std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tp);
        if (tp as usize) >= 0x10000 {
            let saved = *(tp.add(crate::cpu::tls::errno_save_offset()) as *const usize);
            let current = *(tp.add(8) as *const usize);
            if saved != 0 && (current < (tp as usize) || current >= (tp as usize) + 0x1000) {
                *(tp.add(8) as *mut usize) = saved;
            }
            let guest_tls = *(tp.add(crate::cpu::tls::guest_tls_offset()) as *const usize);
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetCurrentThread() -> *mut NnThreadType {
    CURRENT_NN_THREAD.with(|cell| {
        let mut ptr = *cell.borrow();
        if ptr.is_null() {
            ptr = THREAD_SELF_STRUCT.with(|s| s.as_ptr());
            let self_pthread = libc::pthread_self();
            if (*ptr).handle == 0 {
                (*ptr).handle = 1;
            }
            // Populate the REAL host stack bounds at the REAL field offsets
            // (_stack/_stackSize), not a placeholder. See struct doc comment.
            let (lo, hi) = super::thread_stack::current_bounds().unwrap_or_else(|| {
                let top = libc::pthread_get_stackaddr_np(self_pthread) as usize;
                (top.saturating_sub(libc::pthread_get_stacksize_np(self_pthread)), top)
            });
            (*ptr).stack = lo as *mut ();
            (*ptr).original_stack = (*ptr).stack;
            (*ptr).stack_size = hi - lo;
            *cell.borrow_mut() = ptr;
        }
        setup_horizon_tpidrro(ptr);
        ptr
    })
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
    pub lock: libc::os_unfair_lock,
    pub _pad3: u32,
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_InitializeMutex(
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
        (*mutex).lock = libc::OS_UNFAIR_LOCK_INIT;
        (*mutex)._pad3 = 0;
    }
}

// The lock word lives in guest memory, so it's a plain atomic owner token rather
// than an os_unfair_lock: libplatform aborts the whole process ("os_unfair_lock is
// corrupt") if the word is ever stale, relocked by its owner, or unlocked from
// another thread, none of which should take the emulator down.
unsafe fn mutex_lock_word<'a>(mutex: *mut NnMutex) -> &'a std::sync::atomic::AtomicU32 {
    &*(&raw mut (*mutex).lock as *const std::sync::atomic::AtomicU32)
}
fn mutex_token(cur: *mut NnThreadType) -> u32 {
    (cur as usize as u32) | 1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_LockMutex(mutex: *mut NnMutex) {
    use std::sync::atomic::Ordering;
    if (mutex as usize) < 0x10000 {
        return;
    }
    let cur = hle_nn_os_GetCurrentThread();
    if (*mutex).owner_thread == cur && (*mutex).nest_count > 0 {
        if !(*mutex).is_recursive {
            // Real hardware asserts here. Treat as recursive so we can see who does it.
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                let lr: usize;
                std::arch::asm!("mov {}, x30", out(reg) lr);
                eprintln!("[!] nn::os::LockMutex: non-recursive mutex {:p} relocked by its owner (guest_lr={:#x}); treating as recursive", mutex, lr);
            }
        }
        (*mutex).nest_count += 1;
        return;
    }
    let word = mutex_lock_word(mutex);
    let tok = mutex_token(cur);
    let mut spins = 0u32;
    while word
        .compare_exchange(0, tok, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spins += 1;
        if spins < 1000 {
            std::hint::spin_loop();
        } else if spins < 1050 {
            std::thread::yield_now();
        } else {
            hle_sleep_ns(5_000);
        }
    }
    (*mutex).owner_thread = cur;
    (*mutex).nest_count = 1;
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_UnlockMutex(mutex: *mut NnMutex) {
    if (mutex as usize) < 0x10000 {
        return;
    }
    let cur = hle_nn_os_GetCurrentThread();
    if (*mutex).owner_thread != cur {
        return;
    }
    (*mutex).nest_count -= 1;
    if (*mutex).nest_count == 0 {
        (*mutex).owner_thread = std::ptr::null_mut();
        mutex_lock_word(mutex).store(0, std::sync::atomic::Ordering::Release);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TryLockMutex(mutex: *mut NnMutex) -> bool {
    use std::sync::atomic::Ordering;
    if (mutex as usize) < 0x10000 {
        return false;
    }
    let cur = hle_nn_os_GetCurrentThread();
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FinalizeMutex(mutex: *mut NnMutex) {
    if (mutex as usize) >= 0x10000 {
        (*mutex).state = 0;
        (*mutex).owner_thread = std::ptr::null_mut();
        (*mutex).nest_count = 0;
        (*mutex).lock = libc::OS_UNFAIR_LOCK_INIT;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_IsMutexLockedByCurrentThread(mutex: *const NnMutex) -> bool {
    if (mutex as usize) >= 0x10000 {
        let cur = hle_nn_os_GetCurrentThread();
        (*mutex).owner_thread == cur && (*mutex).nest_count > 0
    } else {
        false
    }
}

// 5. System clocks. The guest's system tick is the host clock (crate::clock), not the Switch's 19.2 MHz counter:
// GetSystemTick, GetSystemTickFrequency and ConvertToTimeSpan all use it, so they agree with each other and with NVN
// timestamps.
extern "C" {
    fn mach_wait_until(deadline: u64) -> libc::c_int;
}

/// --log-timestamps (with metal/mod.rs): guest-main's GPU syncs and sleeps next to the TIMESTAMP reports, in ms of the
/// same clock, to reconstruct a frame's timeline.
pub(crate) fn log_timestamps() -> bool {
    crate::settings::get().log.log_timestamps
}

/// Sleep on a guest thread without nanosleep(): libsystem's nanosleep makes a MIG call that
/// needs this thread's mach reply-port TSD slot, which the guest's use of tpidrro_el0 as
/// its Horizon TLS can clobber ("clock_get_time() failed: (ipc/send) invalid reply port").
/// mach_wait_until is a bare kernel trap with no reply port.
pub fn hle_sleep_ns(ns: u64) {
    unsafe {
        mach_wait_until(crate::clock::ticks().saturating_add(crate::clock::ns_to_ticks(ns)));
    }
}

#[no_mangle]
pub extern "C" fn hle_nn_os_GetSystemTick() -> u64 {
    crate::clock::ticks()
}

// 6. NVN Graphics Bootstrap Loader & NV / VI Subsystems
static mut DUMMY_DISPLAY: usize = 0x1000;
static mut DUMMY_LAYER: usize = 0x2000;
static mut DUMMY_WINDOW: usize = 0x3000;

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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_vi_Initialize() -> i32 {
    println!("[+] HLE: nn::vi::Initialize() called");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_vi_OpenDefaultDisplay(out_display: *mut *mut usize) -> i32 {
    println!("[+] HLE: nn::vi::OpenDefaultDisplay called");
    if !out_display.is_null() {
        *out_display = &raw mut DUMMY_DISPLAY;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_vi_CreateLayer(
    out_layer: *mut *mut usize,
    _display: *mut usize,
) -> i32 {
    println!("[+] HLE: nn::vi::CreateLayer called");
    if !out_layer.is_null() {
        *out_layer = &raw mut DUMMY_LAYER;
    }
    0
}

/// The layers are the engine's (CreateLayer): the presenter scales the game's image to the window whatever the mode.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_vi_SetLayerScalingMode(_layer: *mut usize, mode: i32) -> i32 {
    println!("[+] HLE: nn::vi::SetLayerScalingMode({mode})");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_vi_GetDisplayVsyncEvent(
    out_event: *mut u8,
    _display: *mut usize,
) -> i32 {
    println!("[+] HLE: nn::vi::GetDisplayVsyncEvent called");
    if !out_event.is_null() {
        // The vsync event is an auto-clear readable event signaled once per refresh (60Hz).
        // Register it in the SystemEvent table so Wait/TryWait work, and drive it from a pacer.
        let ev = hle_system_event_register(out_event as usize);
        nn_vi_start_vsync_pacer(ev);
    }
    0
}

fn nn_vi_start_vsync_pacer(ev: std::sync::Arc<HleSystemEvent>) {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_vi_GetNativeWindow(
    out_window: *mut *mut usize,
    _layer: *mut usize,
) -> i32 {
    println!("[+] HLE: nn::vi::GetNativeWindow called");
    if !out_window.is_null() {
        *out_window = &raw mut DUMMY_WINDOW;
    }
    0
}

// Global, always-on HLE call-history ring buffer. Every NVN HLE function
// (both the real implementations and the labeled-stub pool) reports here on
// every call, not just on resolution. Answers "what HLE functions were
// called, in order, before this crash?" directly from the crash dump instead
// of requiring a fresh live-tracing session each time -- the silent-stub gap
// and the "we don't have call-time visibility" gap were really the same
// problem, just found from two different angles this session.
const HLE_CALL_HISTORY_SIZE: usize = 4096;
// Names are mostly literals: Cow keeps recording allocation-free (a String per call was ~9% of guest-main).
static HLE_CALL_HISTORY: std::sync::Mutex<
    std::collections::VecDeque<(u64, std::borrow::Cow<'static, str>)>,
> = std::sync::Mutex::new(std::collections::VecDeque::new());
static HLE_CALL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn record_hle_call(name: impl Into<std::borrow::Cow<'static, str>>) {
    let seq = HLE_CALL_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut hist = HLE_CALL_HISTORY.lock().unwrap();
    if hist.len() >= HLE_CALL_HISTORY_SIZE {
        hist.pop_front();
    }
    hist.push_back((seq, name.into()));
}

/// Called from the crash handler. Prints the most recent HLE calls in order,
/// oldest first, ending with whatever ran immediately before the fault.
/// Prints a summary of the ring buffer (call counts, then the most recent calls in order) to stdout. Used with
/// --hle-history-at=<present number> to see what the game polls while it sits on a loading screen.

// Wait graph: which guest thread is currently blocked on which sync object, and from where. Dumped by
// summarize_hle_call_history() to tell a deadlocked worker apart from an idle one.
type BlockedEntry = (&'static str, usize, [usize; 4], std::time::Instant);
static BLOCKED: OnceLock<StdMutex<rustc_hash::FxHashMap<u64, BlockedEntry>>> = OnceLock::new();

fn blocked_table() -> &'static StdMutex<rustc_hash::FxHashMap<u64, BlockedEntry>> {
    BLOCKED.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

pub struct BlockGuard(u64);

impl BlockGuard {
    /// `fp` is x29 inside the HLE function (see report_long_wait); records the guest return-address chain.
    pub unsafe fn new(kind: &'static str, obj: usize, fp: usize) -> Self {
        let mut chain = [0usize; 4];
        let mut gfp = *(fp as *const usize);
        for slot in chain.iter_mut() {
            if gfp < 0x1_0000_0000 || gfp & 7 != 0 {
                break;
            }
            *slot = *((gfp + 8) as *const usize);
            gfp = *(gfp as *const usize);
        }
        let key = libc::pthread_self() as u64;
        blocked_table()
            .lock()
            .unwrap()
            .insert(key, (kind, obj, chain, std::time::Instant::now()));
        BlockGuard(key)
    }
}

impl Drop for BlockGuard {
    fn drop(&mut self) {
        blocked_table().lock().unwrap().remove(&self.0);
    }
}

pub fn dump_blocked_threads() {
    let base = crate::cpu::crash_handler::get_guest_base();
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
    for (kind, obj, chain, since) in rows {
        let c: Vec<String> = chain
            .iter()
            .map(|a| fmt(*a))
            .filter(|s| !s.is_empty())
            .collect();
        println!(
            "[wait-graph]   {:>6.1}s {} on {:#x} <- {}",
            since.elapsed().as_secs_f32(),
            kind,
            obj,
            c.join(" <- ")
        );
    }
}

pub fn summarize_hle_call_history() {
    dump_blocked_threads();
    let hist = HLE_CALL_HISTORY.lock().unwrap();
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
    let hist = HLE_CALL_HISTORY.lock().unwrap();
    eprintln!(
        "    HLE Call History (last {} calls, oldest first):",
        hist.len()
    );
    for (seq, name) in hist.iter() {
        eprintln!("      #{}: {}", seq, name);
    }
}

// 7. Horizon Memory & Heap Management
#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_SetCpuBoostMode(mode: u32) -> i32 {
    println!("[+] HLE: nn::oe::SetCpuBoostMode(mode={})", mode);
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SetMemoryHeapSize(size: usize) -> i32 {
    println!(
        "[+] HLE: nn::os::SetMemoryHeapSize({} MB / {} bytes)",
        size / (1024 * 1024),
        size
    );
    MEMORY_HEAP_SIZE.store(size, std::sync::atomic::Ordering::Relaxed);
    0
}

/// nn::os memory heap bookkeeping for QueryMemoryInfo: the size SetMemoryHeapSize set, and the blocks allocated from it
/// (address -> size). Super Mario Bros. Wonder sizes its main heap as heap size minus what is already allocated.
static MEMORY_HEAP_SIZE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static PROGRAM_SIZE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Bytes of the loaded program images (main and the SDK), for QueryMemoryInfo.
pub fn set_program_size(n: usize) {
    PROGRAM_SIZE.store(n, std::sync::atomic::Ordering::Relaxed);
}
static MEMORY_BLOCKS: StdMutex<Vec<(usize, usize)>> = StdMutex::new(Vec::new());

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FreeMemoryBlock(addr: usize, size: usize) {
    let mut b = MEMORY_BLOCKS.lock().unwrap();
    if let Some(i) = b.iter().position(|&(a, _)| a == addr) {
        b.remove(i);
        crate::guest::dirty_track::release(addr, size);
        libc::munmap(addr as *mut libc::c_void, size);
    }
    println!(
        "[+] HLE: nn::os::FreeMemoryBlock({:#x}, {} MB)",
        addr,
        size / (1024 * 1024)
    );
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_AllocateMemoryBlock(out_addr: *mut usize, size: usize) -> i32 {
    let ptr = libc::mmap(
        std::ptr::null_mut(),
        size,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
        -1,
        0,
    );
    if ptr == libc::MAP_FAILED {
        eprintln!(
            "[-] HLE: AllocateMemoryBlock mmap failed for {} bytes",
            size
        );
        return 1;
    }
    // Fresh anonymous memory is already zero; touching it would commit the whole block (2.5 GB for TotK's heap).
    if !out_addr.is_null() {
        *out_addr = ptr as usize;
    }
    MEMORY_BLOCKS.lock().unwrap().push((ptr as usize, size));
    println!(
        "[+] HLE: nn::os::AllocateMemoryBlock({} MB) allocated at {:p}",
        size / (1024 * 1024),
        ptr
    );
    crate::guest::dirty_track::register_domain(ptr as usize, size);
    let focus = crate::cpu::crash_handler::watch_focus();
    if focus >= ptr as usize && focus < ptr as usize + size {
        crate::cpu::crash_handler::add_watchpoint(focus);
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SetMemoryAllocatorForThreadLocal(
    _alloc: *const (),
    _free: *const (),
) {
    println!("[+] HLE: nn::os::SetMemoryAllocatorForThreadLocal called");
}

// Pool allocator for nn::mem::StandardAllocator.
// TotK passes a large mmap'd region to Initialize; we bump-allocate from it
// so that all TotK-visible objects stay in guest memory (not macOS malloc).
// Free is intentionally a no-op: the pool lives for the process lifetime and
// the TotK heap system does its own internal free-list management on top.
pub struct PoolState {
    pub base: usize,
    pub size: usize,
    pub cursor: std::sync::atomic::AtomicUsize,
}

static POOL_ALLOCATORS: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, PoolState>>> =
    OnceLock::new();

pub fn pool_allocators() -> &'static StdMutex<rustc_hash::FxHashMap<usize, PoolState>> {
    POOL_ALLOCATORS.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_mem_StandardAllocator_ctor(_this: *mut ()) {}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_mem_StandardAllocator_Initialize(
    _this: *mut (),
    addr: *mut (),
    size: usize,
    _cache: bool,
) {
    println!(
        "[+] HLE: nn::mem::StandardAllocator::Initialize(this={:p}, pool={:p}, size={} MB)",
        _this,
        addr,
        size / (1024 * 1024)
    );
    let key = _this as usize;
    {
        let mut map = pool_allocators().lock().unwrap();
        map.entry(key).or_insert_with(|| PoolState {
            base: addr as usize,
            size,
            cursor: std::sync::atomic::AtomicUsize::new(0),
        });
    }
    // Watch the page that will contain the inner-allocator object.
    // Observed across runs: inner_alloc is placed at pool_base + 0x17000920.
    // Watching that page tells us exactly which instruction writes pool_base
    // to [inner_alloc+0] instead of the correct sead vtable pointer.
    // NOTE: do NOT watch pool_base itself here — bump_alloc will memset it
    // during AllocateAligned, which would fault on a read-only page.
    let pool_base = addr as usize;
    let inner_alloc_page = (pool_base + 0x17000920) & !0xfff;
    crate::cpu::crash_handler::add_watchpoint(inner_alloc_page);
}

fn bump_alloc(this: *mut (), size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    let align = align.max(16);
    let map = pool_allocators().lock().unwrap();
    if let Some(pool) = map.get(&(this as usize)) {
        loop {
            let cur = pool.cursor.load(std::sync::atomic::Ordering::Relaxed);
            let aligned = (pool.base + cur + align - 1) & !(align - 1);
            let end = aligned + size;
            if end > pool.base + pool.size {
                eprintln!(
                    "[!] HLE: StandardAllocator pool exhausted (this={:p}, size={}, align={})",
                    this, size, align
                );
                return std::ptr::null_mut();
            }
            let new_cursor = end - pool.base;
            if pool
                .cursor
                .compare_exchange(
                    cur,
                    new_cursor,
                    std::sync::atomic::Ordering::Relaxed,
                    std::sync::atomic::Ordering::Relaxed,
                )
                .is_ok()
            {
                // Zero the allocation (mmap gives zero pages but re-used ranges may not be)
                unsafe {
                    libc::memset(aligned as *mut libc::c_void, 0, size);
                }
                return aligned as *mut u8;
            }
        }
    }
    // Fallback: no pool registered for this allocator (shouldn't happen after Initialize)
    eprintln!(
        "[!] HLE: StandardAllocator::Allocate called without prior Initialize (this={:p})",
        this
    );
    unsafe {
        let ptr = libc::malloc(size) as *mut u8;
        if !ptr.is_null() {
            libc::memset(ptr as *mut libc::c_void, 0, size);
        }
        ptr
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_mem_StandardAllocator_Allocate(
    _this: *mut (),
    size: usize,
) -> *mut u8 {
    let ptr = bump_alloc(_this, size, 16);
    println!(
        "[+] HLE: StandardAllocator::Allocate(this={:p}, size={:#x}) -> {:p}",
        _this, size, ptr
    );
    ptr
}

// Flag so we only install the pool-page watchpoint once (after first bump alloc).
static POOL_PAGE_WATCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[no_mangle]
pub unsafe extern "C" fn hle_nn_mem_StandardAllocator_AllocateAligned(
    _this: *mut (),
    size: usize,
    alignment: usize,
) -> *mut u8 {
    // x17 holds the guest LR saved by the trampoline (mov x17, x30 before blr).
    let guest_lr: usize;
    std::arch::asm!("mov {}, x17", out(reg) guest_lr, options(nomem, nostack));
    // bump_alloc zeroes the allocation with memset before returning.
    let ptr = bump_alloc(_this, size, alignment);
    println!("[+] HLE: StandardAllocator::AllocateAligned(this={:p}, size={:#x}, align={}) -> {:p} [guest_caller=0x{:x}]",
        _this, size, alignment, ptr, guest_lr);
    // After the first allocation the page is already zero (memset done).
    // Protect it now so we catch what TotK writes to it next (vtable, inner_alloc).
    if !ptr.is_null()
        && POOL_PAGE_WATCHED
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
    {
        crate::cpu::crash_handler::add_watchpoint(ptr as usize);
    }
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_mem_StandardAllocator_Free(_this: *mut (), _ptr: *mut ()) {
    // Intentional no-op: bump allocator does not support per-object free.
    // TotK's own heap layer (sead::ExpHeap) manages its free-list within the pool.
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_mem_StandardAllocator_Reallocate(
    _this: *mut (),
    ptr: *mut (),
    new_size: usize,
) -> *mut u8 {
    // Allocate new region from the pool and copy old data.
    // We don't know the old size so we copy new_size bytes (conservative for grow, truncates for shrink).
    let new_ptr = bump_alloc(_this, new_size, 16);
    if !new_ptr.is_null() && !ptr.is_null() {
        libc::memcpy(
            new_ptr as *mut libc::c_void,
            ptr as *const libc::c_void,
            new_size,
        );
    }
    new_ptr
}

// 7b. Service Manager (nn::sm) & Diagnostic Abort
#[no_mangle]
pub unsafe extern "C" fn hle_nn_sm_Initialize() -> i32 {
    println!("[+] HLE: nn::sm::Initialize() called -> returning ResultSuccess (0)");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_sm_Finalize() {
    println!("[+] HLE: nn::sm::Finalize() called");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_sm_GetServiceHandle(
    handle_out: *mut u32,
    service_name: *const libc::c_char,
    _len: usize,
) -> i32 {
    let name_str = if service_name.is_null() {
        ""
    } else {
        std::ffi::CStr::from_ptr(service_name)
            .to_str()
            .unwrap_or("?")
    };
    println!(
        "[+] HLE: nn::sm::GetServiceHandle('{}') called -> returning handle 0x5001",
        name_str
    );
    // The SDK only asks for a service handle when it is about to talk to that service itself. That IPC is not emulated:
    // its request is written to the thread's Horizon IPC buffer, which on this host is Darwin's TLS (thread-specific
    // data; the MIG reply port in slot 2 gets overwritten and later Mach calls fail). The fix is HLE of the nn:: API
    // that led here (symbolize the caller below).
    let fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp);
    // Our frame's return address is the HLE trampoline; the guest's frames follow through the frame-pointer chain.
    let mut chain = Vec::new();
    let mut f = fp;
    for _ in 0..4 {
        if f <= 0x10000 || f & 7 != 0 {
            break;
        }
        chain.push(*((f + 8) as *const usize));
        f = *(f as *const usize);
    }
    eprintln!("[!] SDK opens service '{}' itself (return addresses {:x?}): its IPC is not emulated and would corrupt this thread's TLS", name_str, chain);
    if !handle_out.is_null() {
        *handle_out = 0x5001;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_diag_IsDebuggerAttached() -> bool {
    false
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_diag_detail_Abort(res: *const u32) -> ! {
    sanitize_darwin_tpidrro();
    let code = if res.is_null() { 0 } else { *res };
    let module = code & 0x1ff;
    let desc = (code >> 9) & 0x1fff;
    eprintln!("[!] ========================================================");
    eprintln!("[!] CRITICAL ABORT: nn::diag::detail::Abort called!");
    eprintln!(
        "[!] Result code: 0x{:08X} (Module: {}, Description: {})",
        code, module, desc
    );
    let mut fp: usize;
    std::arch::asm!("mov {}, fp", out(reg) fp);
    eprintln!("[!] Call trace for Abort:");
    for frame_idx in 0..15 {
        if fp < 0x10000 || (fp & 7) != 0 {
            break;
        }
        let next_fp = *(fp as *const usize);
        let lr = *((fp + 8) as *const usize);
        let mut dl_info: libc::Dl_info = std::mem::zeroed();
        let (sname, dname) = if libc::dladdr(lr as *const libc::c_void, &mut dl_info) != 0 {
            let s = if !dl_info.dli_sname.is_null() {
                std::ffi::CStr::from_ptr(dl_info.dli_sname)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "?".into()
            };
            let f = if !dl_info.dli_fname.is_null() {
                std::ffi::CStr::from_ptr(dl_info.dli_fname)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "?".into()
            };
            (s, f)
        } else {
            ("?".into(), "?".into())
        };
        eprintln!(
            "    Frame #{:02}: FP={:#x} LR={:#x} <{} in {}>",
            frame_idx, fp, lr, sname, dname
        );
        if next_fp <= fp {
            break;
        }
        fp = next_fp;
    }
    eprintln!("[!] ========================================================");
    eprintln!("[!] Abort called — terminating process with exit code 1.");
    std::process::exit(1);
}

// 8. Operation Environment (nn::oe) and Account
#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_Initialize() {
    println!("[+] HLE: nn::oe::Initialize() called");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_SetFocusHandlingMode(mode: i32) {
    println!("[+] HLE: nn::oe::SetFocusHandlingMode(mode={})", mode);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_SetPerformanceConfiguration(mode: i32, config: i32) {
    println!(
        "[+] HLE: nn::oe::SetPerformanceConfiguration(mode={}, config={})",
        mode, config
    );
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_SetResumeNotificationEnabled(enabled: bool) {
    println!("[+] HLE: nn::oe::SetResumeNotificationEnabled({})", enabled);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_SetOperationModeChangedNotificationEnabled(enabled: bool) {
    println!(
        "[+] HLE: nn::oe::SetOperationModeChangedNotificationEnabled({})",
        enabled
    );
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_SetPerformanceModeChangedNotificationEnabled(enabled: bool) {
    println!(
        "[+] HLE: nn::oe::SetPerformanceModeChangedNotificationEnabled({})",
        enabled
    );
}

static HANDHELD_OVERRIDE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

pub fn set_handheld(handheld: bool) {
    HANDHELD_OVERRIDE.store(
        if handheld { 1 } else { 0 },
        std::sync::atomic::Ordering::Release,
    );
    crate::horizon::nn::oe_push_notification_message(0x1e); // MessageOperationModeChanged
    crate::horizon::nn::oe_push_notification_message(0x1f); // MessagePerformanceModeChanged
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_GetOperationMode() -> i32 {
    if is_handheld() {
        0 // Handheld mode
    } else {
        1 // Docked mode
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_GetPerformanceMode() -> i32 {
    if is_handheld() {
        0 // Normal / Handheld 720p
    } else {
        1 // Boosted / Docked 1080p
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_GetPerformanceConfiguration(mode: i32) -> u32 {
    let config = if mode == 1 {
        0x00010001
    } else {
        0x00020003
    };
    println!(
        "[+] HLE: nn::oe::GetPerformanceConfiguration(mode={}) -> 0x{:08x}",
        mode, config
    );
    config
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_GetCurrentFocusState() -> i32 {
    1 // InFocus
}

#[repr(C)]
pub struct NnAccountUid {
    pub high: u64,
    pub low: u64,
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_account_Initialize() -> i32 {
    println!("[+] HLE: nn::account::Initialize() called");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_account_TryOpenPreselectedUser(out_handle: *mut u128) -> bool {
    println!("[+] HLE: nn::account::TryOpenPreselectedUser called");
    if !out_handle.is_null() {
        *out_handle = 1;
    }
    true
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_account_OpenPreselectedUser(out_handle: *mut u128) -> i32 {
    println!("[+] HLE: nn::account::OpenPreselectedUser called");
    if !out_handle.is_null() {
        *out_handle = 1;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_account_GetUserId(
    out_uid: *mut NnAccountUid,
    _handle: *const (),
) -> i32 {
    println!("[+] HLE: nn::account::GetUserId called");
    if !out_uid.is_null() {
        (*out_uid).high = 0x0123456789ABCDEF;
        (*out_uid).low = 0xFEDCBA9876543210;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_account_GetLastOpenedUser(out_uid: *mut NnAccountUid) -> i32 {
    println!("[+] HLE: nn::account::GetLastOpenedUser called");
    if !out_uid.is_null() {
        (*out_uid).high = 0x0123456789ABCDEF;
        (*out_uid).low = 0xFEDCBA9876543210;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_account_GetNickname(
    out_nick: *mut [u8; 32],
    _uid: *const NnAccountUid,
) -> i32 {
    println!("[+] HLE: nn::account::GetNickname called");
    if !out_nick.is_null() {
        let name = b"Link\0";
        libc::memset(out_nick as *mut libc::c_void, 0, 32);
        libc::memcpy(
            out_nick as *mut libc::c_void,
            name.as_ptr() as *const libc::c_void,
            name.len(),
        );
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_account_CloseUser(_handle: *const ()) {
    println!("[+] HLE: nn::account::CloseUser called");
}

// 9. Threading & TLS Management
static NEXT_TLS_SLOT: AtomicU32 = AtomicU32::new(0);

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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_AllocateTlsSlot(out_slot: *mut u32, _dtor: *const ()) -> i32 {
    let slot = NEXT_TLS_SLOT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if !out_slot.is_null() {
        *out_slot = slot;
    }
    println!("[+] HLE: nn::os::AllocateTlsSlot() -> slot {}", slot);
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SetTlsValue(slot: u32, val: u64) {
    if (slot as usize) < 64 {
        THREAD_TLS_VALUES.with(|cell| {
            cell.borrow_mut()[slot as usize] = val;
        });
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetTlsValue(slot: u32) -> u64 {
    if (slot as usize) < 64 {
        THREAD_TLS_VALUES.with(|cell| cell.borrow()[slot as usize])
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FreeTlsSlot(_slot: u32) {}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SleepThread(nano: i64) {
    sanitize_darwin_tpidrro();
    if nano > 0 {
        hle_sleep_ns(nano as u64);
    }
    if nano > 1_000_000 && log_timestamps() && std::thread::current().name() == Some("guest-main") {
        let now = crate::clock::now_ms();
        println!(
            "[ts] sleep {:.1} ms ended t {:.3} ms",
            nano as f64 / 1e6,
            now
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_YieldThread() {
    std::thread::yield_now();
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_ConvertToTimeSpan(tick: i64) -> i64 {
    crate::clock::ticks_to_ns(tick.max(0) as u64) as i64
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_InitializeNpad() {
    println!("[+] HLE: nn::hid::InitializeNpad() called");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_SetSupportedNpadIdType(_npad_ids: *const u32, count: usize) {
    println!(
        "[+] HLE: nn::hid::SetSupportedNpadIdType(count={}) called",
        count
    );
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_SetSupportedNpadStyleSet(style_set: u32) {
    println!(
        "[+] HLE: nn::hid::SetSupportedNpadStyleSet(0x{:X}) called",
        style_set
    );
    HID_SUPPORTED_STYLES.store(style_set, std::sync::atomic::Ordering::Relaxed);
}
static HID_SUPPORTED_STYLES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// NpadJoyHoldType: Vertical = 0 (the default), Horizontal = 1.
static HID_HOLD_TYPE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetSupportedNpadStyleSet() -> u32 {
    HID_SUPPORTED_STYLES.load(std::sync::atomic::Ordering::Relaxed)
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetNpadJoyHoldType() -> i32 {
    HID_HOLD_TYPE.load(std::sync::atomic::Ordering::Relaxed)
}
/// NpadJoyAssignmentMode: Dual = 0 (one Npad, both Joy-Con or a full controller), which is what the input layer reports.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetNpadJoyAssignment(_id: *const u32) -> i32 {
    0
}
/// Assignment-mode and LR-assignment changes, Joy-Con swaps and the strap guide applet: one fixed controller layout.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_assignment_noop() {}
/// GetTouchScreenState<16>: {i64 samplingNumber, i32 count, reserved[4], TouchState touches[N]},
/// TouchState (0x28): {i64 deltaTime, u32 attributes (bit 0 Start, bit 1 End), i32 fingerId, i32 x, i32 y,
/// i32 diameterX, i32 diameterY, i32 rotationAngle, reserved[4]}, coordinates in the 1280x720 panel. The capacity N is
/// part of the imported symbol: only the <16> instantiation is bound to this function.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetTouchScreenState(out: *mut u8) {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| println!("[+] HLE: nn::hid::GetTouchScreenState<16> polled (window mouse / touches are the touch screen)"));
    write_touch_screen_state(out, 16);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetTouchScreenState1(out: *mut u8) {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| println!("[+] HLE: nn::hid::GetTouchScreenState<1> polled (window mouse / touches are the touch screen)"));
    write_touch_screen_state(out, 1);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetTouchScreenState5(out: *mut u8) {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| println!("[+] HLE: nn::hid::GetTouchScreenState<5> polled (window mouse / touches are the touch screen)"));
    write_touch_screen_state(out, 5);
}

/// Fills a TouchScreenState<n> from the window's contacts (host::input::touches).
unsafe fn write_touch_screen_state(out: *mut u8, n: usize) {
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
        if now.duration_since(t.began).as_millis() < 20 {
            attributes |= 1; // Start
        }
        if t.ended.is_some() {
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
/// GetSixAxisSensorState (96 bytes): no motion, identity orientation.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetSixAxisSensorState(out: *mut u8, _handle: *const u32) {
    if out.is_null() {
        return;
    }
    std::ptr::write_bytes(out, 0, 96);
    // direction (DirectionState: Float3 x, y, z) starts after deltaTime, samplingNumber and three Float3s: 16 + 36.
    let dir = out.add(52) as *mut f32;
    *dir.add(0) = 1.0;
    *dir.add(4) = 1.0;
    *dir.add(8) = 1.0;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_IsSixAxisSensorAtRest(_handle: *const u32) -> bool {
    true
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetNpadStyleSet(_npad_id: *const u32) -> u32 {
    let id = if _npad_id.is_null() {
        u32::MAX
    } else {
        *_npad_id
    };
    crate::host::input::log_hid("GetNpadStyleSet", id, 0);
    // Like a docked Switch with one Pro Controller: Npad No1 in the FullKey style, nothing else connected. Reporting
    // every id as FullKey | Handheld | JoyDual made the game read JoyDual states (without the Joy-Con connection
    // attributes), and the left stick never moved Link.
    if id == NPAD_NO1 {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetNpadStates(
    states: *mut NpadFullKeyState,
    count: i32,
    _npad_id: *const u32,
) -> i32 {
    crate::host::input::log_hid(
        "GetNpadStates(FullKey)",
        if _npad_id.is_null() {
            u32::MAX
        } else {
            *_npad_id
        },
        count,
    );
    npad_states(states, count, _npad_id, 0)
}

/// NpadId::No1.
const NPAD_NO1: u32 = 0;

/// The shared body of the GetNpadStates overloads (all state structs have the NpadFullKeyState layout). Only No1 is
/// connected; `extra_attributes` adds style-specific attribute bits (JoyDual: IsLeftConnected | IsRightConnected).
unsafe fn npad_states(
    states: *mut NpadFullKeyState,
    count: i32,
    _npad_id: *const u32,
    extra_attributes: u32,
) -> i32 {
    let connected = !_npad_id.is_null() && *_npad_id == NPAD_NO1;
    if states.is_null() || count <= 0 {
        return 0;
    }
    crate::host::input::apply_script();
    // Keyboard / controller state mapped by the user's bindings (host/input.rs).
    let (buttons, sticks) = if connected {
        crate::host::input::npad_state()
    } else {
        (0, [0; 4])
    };
    // --log-hid-buttons: every read that returns pressed buttons (which id / style path the game polls).
    static LOG_BUTTONS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if buttons != 0 && *LOG_BUTTONS.get_or_init(|| crate::settings::get().log.log_hid_buttons) {
        let fp: usize;
        std::arch::asm!("mov {}, x29", out(reg) fp);
        let lr = if fp > 0x10000 {
            *((fp + 8) as *const usize)
        } else {
            0
        };
        let _ = lr;
        println!(
            "[hid-buttons] frame {} id {:#x} buttons {:#x} attrs {:#x} callers: {}",
            crate::gpu::backend().presented_frames(),
            *_npad_id,
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


/// GetTouchScreenStates<16>(states, count): the latest state only.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetTouchScreenStates(out: *mut u8, count: i32) -> i32 {
    if count < 1 {
        return 0;
    }
    write_touch_screen_state(out, 16);
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetNpadJoyDualStates(
    states: *mut NpadFullKeyState,
    count: i32,
    npad_id: *const u32,
) -> i32 {
    crate::host::input::log_hid(
        "GetNpadStates(JoyDual)",
        if npad_id.is_null() {
            u32::MAX
        } else {
            *npad_id
        },
        count,
    );
    npad_states(states, count, npad_id, (1 << 2) | (1 << 4)) // IsLeftConnected | IsRightConnected
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetNpadHandheldStates(
    states: *mut NpadFullKeyState,
    count: i32,
    npad_id: *const u32,
) -> i32 {
    crate::host::input::log_hid(
        "GetNpadStates(Handheld)",
        if npad_id.is_null() {
            u32::MAX
        } else {
            *npad_id
        },
        count,
    );
    npad_states(states, count, npad_id, (1 << 2) | (1 << 4))
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetSixAxisSensorHandles(
    handles: *mut u32,
    count: i32,
    _npad_id: *const u32,
    style: u32,
) -> i32 {
    // Returns the number of handles obtained, which depends on the style (count is only the
    // buffer's capacity). Per style as in Ryujinx's Npad: FullKey 1, Handheld 2 (both Joy-Con), JoyDual 2, JoyLeft /
    // JoyRight 1. Returning `count` (8) made Kirby loop over eight sensors and overwrite the controller state it had
    // just read on its stack: every button press was lost.
    let sensors = if style & 0b0110 != 0 { 2 } else { 1 }; // Handheld (bit 1) / JoyDual (bit 2)
    let n = sensors.min(count.max(0));
    if !handles.is_null() {
        for i in 0..n {
            *handles.add(i as usize) = (i + 1) as u32;
        }
    }
    n
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetVibrationDeviceHandles(
    handles: *mut u32,
    count: i32,
    _npad_id: *const u32,
    style: u32,
) -> i32 {
    // Like GetSixAxisSensorHandles: the number of vibration devices the style has (one per motor: Pro Controller,
    // handheld and Joy-Con pair 2; a single Joy-Con 1), not the buffer's capacity.
    let devices = if style & 0b11000 != 0 && style & 0b00111 == 0 {
        1
    } else {
        2
    }; // JoyLeft (bit 3) / JoyRight (bit 4)
    let n = devices.min(count.max(0));
    if !handles.is_null() {
        for i in 0..n {
            *handles.add(i as usize) = (i + 1) as u32;
        }
    }
    n
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_StartSixAxisSensor(_handle: *const u32) -> i32 {
    println!("[+] HLE: nn::hid::StartSixAxisSensor called -> returning 0");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_StopSixAxisSensor(_handle: *const u32) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_InitializeVibrationDevice(_handle: *const u32) -> i32 {
    println!("[+] HLE: nn::hid::InitializeVibrationDevice called -> returning 0");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_SetNpadJoyHoldType(_hold_type: i32) {
    HID_HOLD_TYPE.store(_hold_type, std::sync::atomic::Ordering::Relaxed);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_SendVibrationValue(
    handle: *const u32,
    val: *const [f32; 4],
) -> i32 {
    // nn::hid::VibrationValue: amplitudeLow, frequencyLow, amplitudeHigh, frequencyHigh (host/rumble.rs plays it).
    if !handle.is_null() && !val.is_null() {
        send_vibration(*handle, std::ptr::read_unaligned(val));
    }
    0
}

/// The last vibration value sent to each motor handle.
static VIBRATION_SENT: StdMutex<Option<rustc_hash::FxHashMap<u32, [f32; 4]>>> = StdMutex::new(None);

fn send_vibration(handle: u32, v: [f32; 4]) {
    VIBRATION_SENT.lock().unwrap().get_or_insert_with(Default::default).insert(handle, v);
    crate::host::rumble::set(handle, v);
}

/// The value a motor is vibrating with: the last one sent to it, or the neutral value (no amplitude at the 160 / 320 Hz
/// resonance frequencies) before any.
pub(crate) unsafe extern "C" fn hid_current_vibration(out: *mut [f32; 4], handle: *const u32) -> i32 {
    if !out.is_null() {
        let sent = if handle.is_null() {
            None
        } else {
            VIBRATION_SENT.lock().unwrap().as_ref().and_then(|m| m.get(&*handle).copied())
        };
        std::ptr::write_unaligned(out, sent.unwrap_or([0.0, 160.0, 0.0, 320.0]));
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_SendVibrationValues(
    handles: *const u32,
    vals: *const [f32; 4],
    count: i32,
) -> i32 {
    if !handles.is_null() && !vals.is_null() {
        for i in 0..count.max(0) as usize {
            send_vibration(*handles.add(i), std::ptr::read_unaligned(vals.add(i)));
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_IsVibrationPermitted(out_permitted: *mut bool) -> i32 {
    if !out_permitted.is_null() {
        *out_permitted = true;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetVibrationDeviceInfo(
    info_out: *mut u32,
    _handle: *const u32,
) -> i32 {
    if !info_out.is_null() {
        *info_out = 1; // LinearResonantActuator (HD Rumble)
        *info_out.add(1) = 1; // Left
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_InitializeMouse() -> i32 {
    println!("[+] HLE: nn::hid::InitializeMouse() called -> returning 0");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_InitializeKeyboard() -> i32 {
    println!("[+] HLE: nn::hid::InitializeKeyboard() called -> returning 0");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_InitializeTouchScreen() -> i32 {
    println!("[+] HLE: nn::hid::InitializeTouchScreen() called -> returning 0");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetMouseState(state: *mut u8) {
    if !state.is_null() {
        std::ptr::write_bytes(state, 0, 64);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetMouseStates(states: *mut u8, count: i32) -> i32 {
    if !states.is_null() && count > 0 {
        std::ptr::write_bytes(states, 0, (count as usize) * 64);
    }
    count
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetKeyboardState(state: *mut u8) {
    if !state.is_null() {
        std::ptr::write_bytes(state, 0, 64);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetKeyboardStates(states: *mut u8, count: i32) -> i32 {
    if !states.is_null() && count > 0 {
        std::ptr::write_bytes(states, 0, (count as usize) * 64);
    }
    count
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_InitializeDebugPad() {
    println!("[+] HLE: nn::hid::InitializeDebugPad called");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetDebugPadState(state: *mut u8) {
    if !state.is_null() {
        std::ptr::write_bytes(state, 0, 64);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_ControllerSupportArg_SetDefault(arg: *mut u8) {
    if !arg.is_null() {
        std::ptr::write_bytes(arg, 0, 0x100);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_ShowControllerSupport(
    result_info: *mut u8,
    arg: *const u8,
) -> i32 {
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n < 3 || n % 10000 == 0 {
        let fp: usize;
        std::arch::asm!("mov {}, x29", out(reg) fp);
        let bytes = if arg.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(arg, 0x20).to_vec()
        };
        println!(
            "[+] HLE: nn::hid::ShowControllerSupport() call #{} arg={:02x?} callers: {}",
            n,
            bytes,
            guest_chain(fp)
        );
    }
    // ControllerSupportResultInfo { int8 playerCount; u8 pad[3]; NpadIdType selectedId; ... }.
    // playerCount == 0 means the user cancelled, and the game then shows the applet again, forever.
    // A real session ends with the requested player count and Npad No1 selected.
    if !result_info.is_null() {
        std::ptr::write_bytes(result_info, 0, 0x40);
        let min_players = if arg.is_null() { 1 } else { (*arg).max(1) };
        *(result_info as *mut i8) = min_players as i8;
        *(result_info.add(4) as *mut u32) = 0; // NpadId::No1
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_InitializeSevenSixAxisSensor(_buf: *mut u8, _size: usize) -> i32 {
    println!("[+] HLE: nn::hid::InitializeSevenSixAxisSensor called");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_FinalizeSevenSixAxisSensor() {
    println!("[+] HLE: nn::hid::FinalizeSevenSixAxisSensor called");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_StartSevenSixAxisSensor() -> i32 {
    println!("[+] HLE: nn::hid::StartSevenSixAxisSensor called");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_StopSevenSixAxisSensor() -> i32 {
    println!("[+] HLE: nn::hid::StopSevenSixAxisSensor called");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_IsSevenSixAxisSensorAtRest() -> bool {
    true
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_GetSevenSixAxisSensorStates(out: *mut u8, count: i32) -> i32 {
    if !out.is_null() && count > 0 {
        std::ptr::write_bytes(out, 0, (count as usize) * 0x60);
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_hid_ResetSevenSixAxisSensorTimestamp() {
    println!("[+] HLE: nn::hid::ResetSevenSixAxisSensorTimestamp called");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_SetUserInactivityDetectionTimeExtended(_extended: bool) {}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_CreateThread(
    thread: *mut NnThreadType,
    entry: unsafe extern "C" fn(*mut ()),
    arg: *mut (),
    stack: *mut (),
    stack_size: usize,
    priority: i32,
) -> i32 {
    println!(
        "[+] HLE: nn::os::CreateThread(thread={:p}, stack={:p}, size={}, prio={})",
        thread, stack, stack_size, priority
    );
    if !stack.is_null() && stack_size > 0 {
        libc::memset(stack as *mut libc::c_void, 0, stack_size);
    }
    if !thread.is_null() {
        (*thread).handle = NEXT_THREAD_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SetThreadName(thread: *mut NnThreadType, name: *const c_char) {
    if !thread.is_null() && !name.is_null() {
        let name_str = CStr::from_ptr(name).to_string_lossy();
        println!(
            "[+] HLE: nn::os::SetThreadName(thread={:p}, name='{}')",
            thread, name_str
        );
        let bytes = name_str.as_bytes();
        let len = bytes.len().min(31);
        libc::memcpy(
            (*thread).thread_name_buffer.as_mut_ptr() as *mut libc::c_void,
            bytes.as_ptr() as *const libc::c_void,
            len,
        );
        (*thread).thread_name_buffer[len] = 0;
        (*thread).name_pointer = (*thread).thread_name_buffer.as_ptr() as *const c_char;
    }
}

/// nn::os::SetThreadNamePointer / GetThreadNamePointer: the name is the caller's string, kept by pointer.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SetThreadNamePointer(
    thread: *mut NnThreadType,
    name: *const c_char,
) {
    let t = if thread.is_null() {
        hle_nn_os_GetCurrentThread() as *mut NnThreadType
    } else {
        thread
    };
    if !t.is_null() {
        (*t).name_pointer = name;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetThreadNamePointer(
    thread: *const NnThreadType,
) -> *const c_char {
    let t = if thread.is_null() {
        hle_nn_os_GetCurrentThread() as *const NnThreadType
    } else {
        thread
    };
    if t.is_null() {
        std::ptr::null()
    } else {
        (*t).name_pointer
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_StartThread(thread: *mut NnThreadType) {
    if thread.is_null() {
        return;
    }
    let name_bytes = &(*thread).thread_name_buffer;
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
            crate::cpu::tls::mark_guest_thread();
            println!("[+] HLE: Guest thread '{}' starting execution", name_str);
            // The threads on the frame's critical path (all of our Metal encoding runs on the Presentation Thread)
            // ask for performance cores: with the default QoS macOS may run them on efficiency cores.
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
            crate::cpu::crash_handler::ensure_altstack();
            // Every guest thread reads tpidr_el0 directly for thread_local /
            // SDK per-thread state -- must be installed before any guest code
            // (including code reached indirectly through HLE calls below) runs
            // on this OS thread. See cpu/tls.rs.
            crate::cpu::tls::install_guest_tls();
            crate::cpu::trampoline::init_worker_thread_x18();
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
            if (*thread_ptr).handle == 0 {
                (*thread_ptr).handle =
                    NEXT_THREAD_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            setup_horizon_tpidrro(thread_ptr);
            super::thread_stack::run(stack_lo, stack_hi, entry_usize, arg_usize);
            println!("[+] HLE: Guest thread '{}' returned", name_str);
            (*thread_ptr).state = THREAD_STATE_EXITED;
            let (done, cv) = &*exit;
            *done.lock().unwrap_or_else(|e| e.into_inner()) = true;
            cv.notify_all();
        })
        .expect("failed to spawn guest thread");
}

// nn::os::ThreadType::State.
const THREAD_STATE_NOT_INITIALIZED: u8 = 0;
const THREAD_STATE_INITIALIZED: u8 = 1;
const THREAD_STATE_STARTED: u8 = 3;
const THREAD_STATE_EXITED: u8 = 4;

type ExitSignal = std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>;
static THREAD_EXITS: std::sync::Mutex<Option<std::collections::HashMap<usize, ExitSignal>>> =
    std::sync::Mutex::new(None);

/// The exit flag of a started guest thread (`fresh` replaces any previous one when the object is started again).
fn thread_exit_signal(thread: *mut NnThreadType, fresh: bool) -> Option<ExitSignal> {
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
unsafe fn join_guest_thread(thread: *mut NnThreadType) {
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
pub unsafe extern "C" fn hle_nn_os_WaitThread(thread: *mut NnThreadType) {
    if !thread.is_null() {
        join_guest_thread(thread);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TryWaitThread(thread: *mut NnThreadType) -> bool {
    match thread_exit_signal(thread, false) {
        Some(sig) => *sig.0.lock().unwrap_or_else(|e| e.into_inner()),
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_DestroyThread(thread: *mut NnThreadType) {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_SetPriorityOnCurrentThread(_prio: i32) {}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetThreadId(thread: *const NnThreadType) -> u64 {
    if !thread.is_null() {
        (*thread).handle as u64
    } else {
        1
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_ChangeThreadPriority(thread: *mut NnThreadType, prio: i32) {
    if !thread.is_null() {
        (*thread).base_priority = prio;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetThreadPriority(thread: *const NnThreadType) -> i32 {
    if !thread.is_null() {
        (*thread).base_priority
    } else {
        16
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetThreadAvailableCoreMask() -> u64 {
    0b111
}

// Per-thread core assignment. TotK's task system dispatches per core (0..2), so a thread must
// report the core its affinity selected: GetCurrentCoreNumber used to return a constant 1 for
// every thread, which left per-core task queues stranded (worker threads waited forever).
static THREAD_CORES: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, i32>>> = OnceLock::new();
const DEFAULT_CORE: i32 = 1;
/// Bumped whenever a thread's core changes (cores.rs caches each thread's core against it).
pub static CORES_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn thread_cores() -> &'static StdMutex<rustc_hash::FxHashMap<usize, i32>> {
    THREAD_CORES.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

/// Core number the calling guest thread runs on (nn::os::GetCurrentCoreNumber).
pub fn current_core() -> u32 {
    let t = unsafe { hle_nn_os_GetCurrentThread() as usize };
    thread_cores()
        .lock()
        .unwrap()
        .get(&t)
        .copied()
        .unwrap_or(DEFAULT_CORE) as u32
}

/// nn::os::CreateThread(..., priority, idealCore): the 6-argument version plus the core the thread
/// starts on, which GetCurrentCoreNumber reports (see current_core).
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_CreateThreadWithCore(
    thread: *mut NnThreadType,
    entry: unsafe extern "C" fn(*mut ()),
    arg: *mut (),
    stack: *mut (),
    stack_size: usize,
    priority: i32,
    ideal_core: i32,
) -> i32 {
    let r = hle_nn_os_CreateThread(thread, entry, arg, stack, stack_size, priority);
    if r == 0 && ideal_core >= 0 && !thread.is_null() {
        thread_cores()
            .lock()
            .unwrap()
            .insert(thread as usize, ideal_core);
        CORES_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    r
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SetThreadCoreMask(
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
        hle_nn_os_GetCurrentThread() as usize
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
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetThreadCoreMask(
    out_core: *mut i32,
    out_mask: *mut u64,
    thread: *const NnThreadType,
) {
    let t = if thread.is_null() {
        hle_nn_os_GetCurrentThread() as usize
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_QueryMemoryInfo(info: *mut NnMemoryInfo) {
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

// nn::os::AllocateAddressRegion / AllocateMemoryPages / FreeAddressRegion / FreeMemoryPages: Horizon's
// reserve-then-commit virtual memory, used directly (bypassing StandardAllocator) by the game's page allocators.
// Regions are mapped read/write up front; AllocateMemoryPages zeroes the guest pages it commits. A side table
// tracks each region's size, since FreeAddressRegion takes only an address.
static ADDRESS_REGIONS: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, usize>>> = OnceLock::new();

const GUEST_PAGE: usize = 0x1000;
const HOST_PAGE: usize = 0x4000;

/// 4KB guest pages currently mapped through nn::os::AllocateMemoryPages (page number = address / 4KB).
fn mapped_guest_pages() -> &'static StdMutex<std::collections::HashSet<usize>> {
    static PAGES: OnceLock<StdMutex<std::collections::HashSet<usize>>> = OnceLock::new();
    PAGES.get_or_init(|| StdMutex::new(std::collections::HashSet::new()))
}

fn address_regions() -> &'static StdMutex<rustc_hash::FxHashMap<usize, usize>> {
    ADDRESS_REGIONS.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

/// Horizon hands out address regions from a dedicated part of the address space, never interleaved with the
/// memory heap. TotK's page-allocator heap (vtable main+0x45b43c8) depends on that: its isInclude() is just
/// [lowest chunk, highest chunk end), so with regions from plain mmap() one chunk could land below the memory heap
/// and another above it, the heap then claimed ordinary heap pointers, and sead::HeapMgr::findContainHeap routed
/// their frees to it (the intermittent crash at main+0xAFCEB4). All regions now come from one reserved arena.
/// Largest arena tried first; smaller ones after it (iOS apps without the extended-virtual-addressing entitlement get
/// far less address space than macOS processes).
#[cfg(not(target_os = "ios"))]
const REGION_ARENA_SIZES: &[usize] = &[16 << 30, 8 << 30, 4 << 30, 2 << 30];
/// iPhone 16e without the entitlement (probe, 2026-10-08): 4 GB largest reservation, ~7 GB in total; the game heap
/// (AllocateMemoryBlock, 3.2 GB for Kirby) must still fit next to the arena.
#[cfg(target_os = "ios")]
const REGION_ARENA_SIZES: &[usize] = &[2 << 30, 1 << 30];
const REGION_ALIGN: usize = 0x10000; // Horizon requires 64KB alignment for address regions

struct RegionArena {
    base: usize,
    size: usize,
    /// Free ranges: start -> size (coalesced, 64KB granular).
    free: std::collections::BTreeMap<usize, usize>,
}

fn region_arena() -> &'static StdMutex<RegionArena> {
    static ARENA: OnceLock<StdMutex<RegionArena>> = OnceLock::new();
    ARENA.get_or_init(|| {
        let (raw, size) = REGION_ARENA_SIZES
            .iter()
            .map(|&size| {
                let raw = unsafe {
                    libc::mmap(
                        std::ptr::null_mut(),
                        size + REGION_ALIGN,
                        libc::PROT_NONE,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                        -1,
                        0,
                    )
                };
                (raw, size)
            })
            .find(|(raw, _)| *raw != libc::MAP_FAILED)
            .unwrap_or((libc::MAP_FAILED, 0));
        let mut free = std::collections::BTreeMap::new();
        let base = if raw == libc::MAP_FAILED {
            println!(
                "[!] HLE: could not reserve the address region arena; regions fall back to mmap"
            );
            0
        } else {
            let base = (raw as usize + REGION_ALIGN - 1) & !(REGION_ALIGN - 1);
            free.insert(base, size);
            println!(
                "[+] HLE: address region arena reserved at {:#x}..{:#x} ({} GB)",
                base,
                base + size,
                size >> 30
            );
            crate::guest::dirty_track::register_domain(base, size);
            base
        };
        StdMutex::new(RegionArena { base, size, free })
    })
}

fn in_region_arena(addr: usize) -> bool {
    let a = region_arena().lock().unwrap();
    a.base != 0 && addr >= a.base && addr < a.base + a.size
}

/// First fit; `size` is 64KB aligned.
fn region_arena_alloc(size: usize) -> Option<usize> {
    let mut a = region_arena().lock().unwrap();
    let (&start, &len) = a.free.iter().find(|(_, &len)| len >= size)?;
    a.free.remove(&start);
    if len > size {
        a.free.insert(start + size, len - size);
    }
    Some(start)
}

fn region_arena_free(addr: usize, size: usize) {
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
        // Replace with a fresh PROT_NONE reservation: drops the pages but keeps the arena contiguous.
        libc::mmap(
            addr as *mut libc::c_void,
            size,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | libc::MAP_FIXED,
            -1,
            0,
        );
        region_arena_free(addr, size);
    } else {
        libc::munmap(addr as *mut libc::c_void, size);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_AllocateAddressRegion(
    out_address: *mut usize,
    size: usize,
) -> i32 {
    let aligned_size = (size + REGION_ALIGN - 1) & !(REGION_ALIGN - 1);
    let aligned_addr = match region_arena_alloc(aligned_size) {
        Some(addr) => {
            let p = libc::mmap(
                addr as *mut libc::c_void,
                aligned_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE | libc::MAP_FIXED,
                -1,
                0,
            );
            if p == libc::MAP_FAILED {
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
            // Arena exhausted (or unreserved): an aligned mapping anywhere, as before.
            println!(
                "[!] HLE: address region arena exhausted; mapping {} bytes outside it",
                aligned_size
            );
            let total_alloc = aligned_size + REGION_ALIGN;
            let raw = libc::mmap(
                std::ptr::null_mut(),
                total_alloc,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            );
            if raw == libc::MAP_FAILED {
                println!(
                    "[!] HLE: nn::os::AllocateAddressRegion(size={}) FAILED",
                    size
                );
                return -1;
            }
            let raw_addr = raw as usize;
            let aligned_addr = (raw_addr + REGION_ALIGN - 1) & !(REGION_ALIGN - 1);
            let pre_pad = aligned_addr - raw_addr;
            if pre_pad > 0 {
                libc::munmap(raw, pre_pad);
            }
            let post_pad = total_alloc - pre_pad - aligned_size;
            if post_pad > 0 {
                libc::munmap((aligned_addr + aligned_size) as *mut libc::c_void, post_pad);
            }
            aligned_addr
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_AllocateMemoryPages(address: usize, size: usize) -> i32 {
    let page_size = HOST_PAGE;
    let page_start = address & !(page_size - 1);
    let page_end = (address + size + page_size - 1) & !(page_size - 1);
    let page_len = page_end - page_start;
    // These pages are made writable and zeroed below without faulting: tell write tracking.
    crate::guest::dirty_track::before_host_write(page_start, page_len);
    let res = libc::mprotect(
        page_start as *mut libc::c_void,
        page_len,
        libc::PROT_READ | libc::PROT_WRITE,
    );
    if res != 0 {
        let err = std::io::Error::last_os_error();
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FreeAddressRegion(address: usize) {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FreeMemoryPages(address: usize, size: usize) {
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
            libc::madvise(
                (host * HOST_PAGE) as *mut libc::c_void,
                HOST_PAGE,
                libc::MADV_FREE_REUSABLE,
            );
            released += 1;
        }
    }
    drop(mapped);
    println!(
        "[+] HLE: nn::os::FreeMemoryPages(addr=0x{:x}, size={} bytes, {} host pages released)",
        address, size, released
    );
}

#[no_mangle]
pub unsafe extern "C" fn hle_strcmp(s1: *const c_char, s2: *const c_char) -> i32 {
    libc::strcmp(s1, s2)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetSystemTickFrequency() -> u64 {
    crate::clock::frequency()
}

// 10. Stack, Core, Mutex TryLock, and ReaderWriterLock
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetCurrentStackInfo(
    out_stack: *mut usize,
    out_stack_size: *mut usize,
) {
    let (lo, hi) = super::thread_stack::current_bounds().unwrap_or_else(|| {
        let me = libc::pthread_self();
        let hi = libc::pthread_get_stackaddr_np(me) as usize;
        (hi - libc::pthread_get_stacksize_np(me), hi)
    });
    if !out_stack.is_null() {
        *out_stack = lo;
    }
    if !out_stack_size.is_null() {
        *out_stack_size = hi - lo;
    }
}

#[repr(C)]
pub struct NnBitArray {
    pub bits: *mut u32,
    pub bit_count: i32,
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_util_BitArray_reset(this: *mut NnBitArray) {
    if !this.is_null() && !(*this).bits.is_null() && (*this).bit_count > 0 {
        let words = ((*this).bit_count as usize + 31) / 32;
        libc::memset((*this).bits as *mut libc::c_void, 0, words * 4);
    }
}

// Reader-writer lock: real exclusion, tracked in a side table keyed by the lock's address
// (horizon/nn.rs).
#[repr(C)]
pub struct NnReaderWriterLock {
    pub lock_state: u32,
    pub thread_id: u64,
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_InitializeReaderWriterLock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::nn::rw_init(rwlock as usize);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FinalizeReaderWriterLock(_rwlock: *mut NnReaderWriterLock) {}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_AcquireWriteLock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::nn::rw_acquire_write(rwlock as usize);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_ReleaseWriteLock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::nn::rw_release_write(rwlock as usize);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_AcquireReadLock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::nn::rw_acquire_read(rwlock as usize);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_ReleaseReadLock(rwlock: *mut NnReaderWriterLock) {
    if !rwlock.is_null() {
        crate::horizon::nn::rw_release_read(rwlock as usize);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_IsWriteLockHeldByCurrentThread(
    rwlock: *const NnReaderWriterLock,
) -> bool {
    !rwlock.is_null() && crate::horizon::nn::rw_write_held_by_me(rwlock as usize)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_oe_GetDesiredLanguage() -> u64 {
    u64::from_le_bytes(*b"en-US\0\0\0")
}

// nn::os::LightMessageQueue: a bounded, blocking SPSC/MPMC queue. Real hardware backs
// it with the guest-supplied buffer; we keep the actual
// ring storage host-side, keyed by the guest queue pointer, and use a real Condvar so
// Receive blocks instead of busy-spinning (this was causing VibrationThread to spin at
// ~1M calls/sec on an unbound GOT stub before these were wired up).
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex as StdMutex, OnceLock};

struct LightMsgQueueInner {
    capacity: usize,
    queue: VecDeque<u64>,
}

struct LightMsgQueueState {
    inner: StdMutex<LightMsgQueueInner>,
    not_empty: Condvar,
    not_full: Condvar,
}

static LIGHT_MQ_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<LightMsgQueueState>>>> =
    OnceLock::new();

fn light_mq_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<LightMsgQueueState>>> {
    LIGHT_MQ_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

fn light_mq_new(capacity: usize) -> Arc<LightMsgQueueState> {
    Arc::new(LightMsgQueueState {
        inner: StdMutex::new(LightMsgQueueInner {
            capacity: capacity.max(1),
            queue: VecDeque::new(),
        }),
        not_empty: Condvar::new(),
        not_full: Condvar::new(),
    })
}

unsafe fn light_mq_get(mq: *mut ()) -> Arc<LightMsgQueueState> {
    let key = mq as usize;
    let mut table = light_mq_table().lock().unwrap();
    table.entry(key).or_insert_with(|| light_mq_new(64)).clone()
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_InitializeLightMessageQueue(
    mq: *mut (),
    _buf: *mut u64,
    count: usize,
) {
    light_mq_table()
        .lock()
        .unwrap()
        .insert(mq as usize, light_mq_new(count));
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FinalizeLightMessageQueue(mq: *mut ()) {
    light_mq_table().lock().unwrap().remove(&(mq as usize));
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SendLightMessageQueue(mq: *mut (), data: u64) {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    while inner.queue.len() >= inner.capacity {
        inner = state.not_full.wait(inner).unwrap();
    }
    inner.queue.push_back(data);
    state.not_empty.notify_all();
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TrySendLightMessageQueue(mq: *mut (), data: u64) -> bool {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    if inner.queue.len() >= inner.capacity {
        return false;
    }
    inner.queue.push_back(data);
    state.not_empty.notify_all();
    true
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_JamLightMessageQueue(mq: *mut (), data: u64) {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    while inner.queue.len() >= inner.capacity {
        inner = state.not_full.wait(inner).unwrap();
    }
    inner.queue.push_front(data);
    state.not_empty.notify_all();
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TryJamLightMessageQueue(mq: *mut (), data: u64) -> bool {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    if inner.queue.len() >= inner.capacity {
        return false;
    }
    inner.queue.push_front(data);
    state.not_empty.notify_all();
    true
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_ReceiveLightMessageQueue(out: *mut u64, mq: *mut ()) {
    let fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp);
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
    if !out.is_null() {
        *out = v;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TryReceiveLightMessageQueue(out: *mut u64, mq: *mut ()) -> bool {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TryPeekLightMessageQueue(out: *mut u64, mq: *const ()) -> bool {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_PeekLightMessageQueue(out: *mut u64, mq: *const ()) {
    let state = light_mq_get(mq as *mut ());
    let mut inner = state.inner.lock().unwrap();
    while inner.queue.is_empty() {
        inner = state.not_empty.wait(inner).unwrap();
    }
    if !out.is_null() {
        *out = inner.queue[0];
    }
}

/// Send (or jam at the head) with a timeout; false if the queue stayed full.
unsafe fn light_mq_timed_put(mq: *mut (), data: u64, front: bool, timeout_ns: i64) -> bool {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TimedSendLightMessageQueue(mq: *mut (), data: u64, timeout_ns: i64) -> bool {
    light_mq_timed_put(mq, data, false, timeout_ns)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TimedJamLightMessageQueue(mq: *mut (), data: u64, timeout_ns: i64) -> bool {
    light_mq_timed_put(mq, data, true, timeout_ns)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TimedReceiveLightMessageQueue(out: *mut u64, mq: *mut (), timeout_ns: i64) -> bool {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TimedPeekLightMessageQueue(
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
struct CondVarState {
    generation: StdMutex<u64>,
    cv: Condvar,
}

static CONDVAR_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<CondVarState>>>> =
    OnceLock::new();

fn condvar_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<CondVarState>>> {
    CONDVAR_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

fn condvar_new() -> Arc<CondVarState> {
    Arc::new(CondVarState {
        generation: StdMutex::new(0),
        cv: Condvar::new(),
    })
}

unsafe fn condvar_get(cv: *mut ()) -> Arc<CondVarState> {
    let key = cv as usize;
    let mut table = condvar_table().lock().unwrap();
    table.entry(key).or_insert_with(condvar_new).clone()
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_InitializeConditionVariable(cv: *mut ()) {
    condvar_table()
        .lock()
        .unwrap()
        .insert(cv as usize, condvar_new());
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FinalizeConditionVariable(cv: *mut ()) {
    condvar_table().lock().unwrap().remove(&(cv as usize));
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SignalConditionVariable(cv: *mut ()) {
    let state = condvar_get(cv);
    {
        let mut gen = state.generation.lock().unwrap();
        *gen = gen.wrapping_add(1);
    }
    state.cv.notify_all();
}

/// Signal already wakes every waiter (generation counter), so Broadcast is the same operation.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_BroadcastConditionVariable(cv: *mut ()) {
    hle_nn_os_SignalConditionVariable(cv);
}

/// nn::os::TimedWaitConditionVariable: returns ConditionVariableStatus (Timeout = 0, NoTimeout = 1).
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TimedWaitConditionVariable(
    cv: *mut (),
    mutex: *mut NnMutex,
    timeout_ns: i64,
) -> i32 {
    let state = condvar_get(cv);
    let seen_gen = *state.generation.lock().unwrap();
    hle_nn_os_UnlockMutex(mutex);
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
    hle_nn_os_LockMutex(mutex);
    if timed_out {
        0
    } else {
        1
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_WaitConditionVariable(cv: *mut (), mutex: *mut NnMutex) {
    let fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp);
    let state = condvar_get(cv);
    let seen_gen = *state.generation.lock().unwrap();

    // Real nn::os semantics: atomically release the (already-held) guest
    // mutex and block until signaled, then re-acquire it before returning.
    // The guest mutex isn't the same lock as our internal Condvar's, so we
    // can't do this in one atomic step the way Rust's Condvar::wait(guard)
    // does with its own Mutex -- release, wait on our own generation-counted
    // condvar, then re-lock, matching the same net effect.
    hle_nn_os_UnlockMutex(mutex);
    {
        let _blocked = BlockGuard::new("CondVar", cv as usize, fp);
        let guard = state.generation.lock().unwrap();
        let _final_guard = state.cv.wait_while(guard, |gen| *gen == seen_gen).unwrap();
    }
    hle_nn_os_LockMutex(mutex);
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
struct LightEventState {
    signaled: StdMutex<bool>,
    auto_clear: std::sync::atomic::AtomicBool,
    cv: Condvar,
    /// Signals so far, counted under `signaled`'s lock: a manual-clear event signaled and cleared again before a
    /// waiter woke still releases that waiter (it was waiting when the signal came), as on the console.
    signals: std::sync::atomic::AtomicU64,
}

static LIGHT_EVENT_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<LightEventState>>>> =
    OnceLock::new();

fn light_event_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<LightEventState>>> {
    LIGHT_EVENT_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

fn light_event_new(initially_signaled: bool, clear_mode: i32) -> Arc<LightEventState> {
    Arc::new(LightEventState {
        signaled: StdMutex::new(initially_signaled),
        auto_clear: std::sync::atomic::AtomicBool::new(clear_mode == 1),
        cv: Condvar::new(),
        signals: std::sync::atomic::AtomicU64::new(0),
    })
}

unsafe fn light_event_get(ev: *mut ()) -> Arc<LightEventState> {
    let key = ev as usize;
    let mut table = light_event_table().lock().unwrap();
    table
        .entry(key)
        .or_insert_with(|| light_event_new(false, 0))
        .clone()
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_InitializeLightEvent(
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FinalizeLightEvent(ev: *mut ()) {
    light_event_table().lock().unwrap().remove(&(ev as usize));
}


#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SignalLightEvent(ev: *mut ()) {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_ClearLightEvent(ev: *mut ()) {
    let state = light_event_get(ev);
    *state.signaled.lock().unwrap() = false;
}

/// Debug aid: report (once per wait) a blocking nn::os wait that has not completed after 5s,
/// with the guest call site, so whole-process stalls can be attributed to a specific wait.
pub(crate) fn guest_chain(fp: usize) -> String {
    let base = crate::cpu::crash_handler::get_guest_base();
    let fmt = |a: usize| {
        if base != 0 && a >= base && a < base + 0x5000000 {
            format!("guest+{:#x}", a - base)
        } else {
            format!("{:#x}", a)
        }
    };
    let mut chain = Vec::new();
    unsafe {
        let mut gfp = *(fp as *const usize);
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
    let base = crate::cpu::crash_handler::get_guest_base();
    let fmt = |a: usize| {
        if base != 0 && a >= base && a < base + 0x5000000 {
            format!("guest+{:#x}", a - base)
        } else {
            format!("{:#x}", a)
        }
    };
    let mut chain = Vec::new();
    unsafe {
        let rec = fp as *const usize;
        let mut gfp = *rec;
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_WaitLightEvent(ev: *mut ()) {
    let fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp);
    let state = light_event_get(ev);
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TryWaitLightEvent(ev: *mut ()) -> bool {
    let state = light_event_get(ev);
    let mut guard = state.signaled.lock().unwrap();
    let was = *guard;
    if was && state.auto_clear.load(std::sync::atomic::Ordering::Relaxed) {
        *guard = false;
    }
    was
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TimedWaitLightEvent(ev: *mut (), timeout_ns: i64) -> bool {
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

// ── nn::os::TimerEvent ─────────────────────────────────────────────────────────
//
// TimerEvent is a timed synchronization primitive supporting one-shot and
// periodic intervals with automatic or manual clear semantics.
// Native Switch implementations use Horizon kernel condition variables
// and critical sections which issue Switch SVCs (svc 0x1c, 0x1d) and check
// thread handles, causing aborts on Darwin. This HLE provides complete
// standard condition variable and deadline based blocking.

struct TimerEventInner {
    clear_mode: u8,
    signaled: bool,
    timer_mode: u8, // 0 = stopped, 1 = one-shot, 2 = periodic
    next_wake: Option<std::time::Instant>,
    first_time_ns: i64,
    interval_ns: i64,
}

struct TimerEventState {
    inner: StdMutex<TimerEventInner>,
    cv: Condvar,
}

static TIMER_EVENT_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<TimerEventState>>>> =
    OnceLock::new();

fn timer_event_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<TimerEventState>>> {
    TIMER_EVENT_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

fn timer_event_new(clear_mode: u8) -> Arc<TimerEventState> {
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

unsafe fn timer_event_get(ev: *mut ()) -> Arc<TimerEventState> {
    let key = ev as usize;
    let mut table = timer_event_table().lock().unwrap();
    table
        .entry(key)
        .or_insert_with(|| timer_event_new(1))
        .clone()
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_InitializeTimerEvent(ev: *mut (), clear_mode: i32) {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FinalizeTimerEvent(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x10) = 0; // _state = 0
    timer_event_table().lock().unwrap().remove(&(ev as usize));
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_StartOneShotTimerEvent(ev: *mut (), first_time: i64) {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_StartPeriodicTimerEvent(
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_StopTimerEvent(ev: *mut ()) {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_SignalTimerEvent(ev: *mut ()) {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_ClearTimerEvent(ev: *mut ()) {
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_WaitTimerEvent(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp);
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

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TryWaitTimerEvent(ev: *mut ()) -> bool {
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

unsafe fn semaphore_init(sem: *mut (), initial: i32, max: i32) {
    semaphore_table()
        .lock()
        .unwrap()
        .insert(sem as usize, semaphore_new(initial, max));
}

unsafe fn semaphore_finalize(sem: *mut ()) {
    semaphore_table().lock().unwrap().remove(&(sem as usize));
}

unsafe fn semaphore_acquire(sem: *mut (), fp: usize) {
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

unsafe fn semaphore_try_acquire(sem: *mut (), timeout_ns: i64) -> bool {
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

unsafe fn semaphore_count(sem: *const ()) -> i32 {
    *semaphore_get(sem as *mut ()).count.lock().unwrap()
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TryAcquireSemaphore(sem: *mut ()) -> bool {
    semaphore_try_acquire(sem, 0)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TimedAcquireSemaphore(sem: *mut (), timeout_ns: i64) -> bool {
    semaphore_try_acquire(sem, timeout_ns)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_GetCurrentSemaphoreCount(sem: *const ()) -> i32 {
    semaphore_count(sem)
}

unsafe fn semaphore_release(sem: *mut (), release_count: i32) {
    let state = semaphore_get(sem);
    {
        let mut count = state.count.lock().unwrap();
        *count = (*count + release_count).min(state.max);
    }
    state.cv.notify_all();
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_InitializeSemaphore(sem: *mut (), initial: i32, max: i32) {
    semaphore_init(sem, initial, max);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FinalizeSemaphore(sem: *mut ()) {
    semaphore_finalize(sem);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_AcquireSemaphore(sem: *mut ()) {
    let fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp);
    semaphore_acquire(sem, fp);
}


#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_ReleaseSemaphore(sem: *mut (), count: i32) {
    semaphore_release(sem, count);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_InitializeLightSemaphore(sem: *mut (), initial: i32, max: i32) {
    semaphore_init(sem, initial, max);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_FinalizeLightSemaphore(sem: *mut ()) {
    semaphore_finalize(sem);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_AcquireLightSemaphore(sem: *mut ()) {
    let fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp);
    semaphore_acquire(sem, fp);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_ReleaseLightSemaphore(sem: *mut ()) {
    semaphore_release(sem, 1);
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
const NN_RESULT_SUCCESS: u32 = 0;

fn audio_align_up(v: usize, a: usize) -> usize {
    (v + a - 1) & !(a - 1)
}

/// nn::audio::GetAudioRendererWorkBufferSize(const AudioRendererParameter&).
/// The buffer only backs the audren process's state, which we don't run, so
/// this just needs to be a valid 4KB-aligned size that scales with the params.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_GetAudioRendererWorkBufferSize(param: *const i32) -> usize {
    record_hle_call("hle_nn_audio_GetAudioRendererWorkBufferSize");
    // AudioRendererParameter: sampleRate, sampleCount, mixBufferCount, subMixCount,
    // voiceCount, sinkCount, effectCount, ...
    let (mix, sub, voice, sink, fx) = if param.is_null() {
        (24, 8, 96, 4, 16)
    } else {
        let g = |i: usize| (*param.add(i)).max(0) as usize;
        (g(2), g(3), g(4), g(5), g(6))
    };
    audio_align_up(
        0x40000 + voice * 0x800 + sub * 0x1000 + mix * 0x400 + sink * 0x400 + fx * 0x2000,
        0x1000,
    )
}

static NN_AUDIO_RENDERER_STATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// ── nn::os::SystemEvent (kernel readable-event handle) ──────────────────────
// SystemEventType state normally comes from an svc-created kernel event; the
// sdk module TryWait/WaitSystemEvent abort (nn::diag::detail::AbortImpl) on an
// event they didn't initialize. Events handed out by our HLE (audio renderer,
// audio device notification, ...) are tracked here by address instead, with
// auto-clear semantics, and Wait/TryWait/Destroy are HLE over this registry.
struct HleSystemEvent {
    signaled: StdMutex<bool>,
    cv: std::sync::Condvar,
}
static HLE_SYSTEM_EVENTS: StdMutex<Vec<(usize, std::sync::Arc<HleSystemEvent>)>> =
    StdMutex::new(Vec::new());

pub fn hle_system_event_register_addr(addr: usize) {
    hle_system_event_register(addr);
}
/// Registers an HLE system event at `addr` that is already signaled (an operation that completed at once).
pub fn hle_system_event_register_signaled(addr: usize) {
    let ev = hle_system_event_register(addr);
    *ev.signaled.lock().unwrap() = true;
    ev.cv.notify_all();
}

fn hle_system_event_register(addr: usize) -> std::sync::Arc<HleSystemEvent> {
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
fn hle_system_event_lookup(addr: usize) -> Option<std::sync::Arc<HleSystemEvent>> {
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
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TryWaitSystemEvent(event: *mut ()) -> u8 {
    record_hle_call("hle_nn_os_TryWaitSystemEvent");
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
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_TimedWaitSystemEvent(event: *mut (), timeout_ns: i64) -> bool {
    record_hle_call("hle_nn_os_TimedWaitSystemEvent");
    if crate::settings::get().log.log_sysevent_waits {
        static SEEN: StdMutex<Vec<usize>> = StdMutex::new(Vec::new());
        let mut seen = SEEN.lock().unwrap();
        if !seen.contains(&(event as usize)) {
            seen.push(event as usize);
            let fp: usize;
            std::arch::asm!("mov {}, x29", out(reg) fp);
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
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_WaitSystemEvent(event: *mut ()) {
    record_hle_call("hle_nn_os_WaitSystemEvent");
    let fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp);
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
#[no_mangle]
pub unsafe extern "C" fn hle_nn_os_DestroySystemEvent(event: *mut ()) {
    record_hle_call("hle_nn_os_DestroySystemEvent");
    HLE_SYSTEM_EVENTS
        .lock()
        .unwrap()
        .retain(|(a, _)| *a != event as usize);
}

/// audren signals the renderer's event once per audio frame (240 samples @ 48kHz = 5ms).
fn nn_audio_start_frame_pacer(ev: std::sync::Arc<HleSystemEvent>) {
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

/// nn::audio::OpenAudioRenderer(AudioRendererHandle*, SystemEvent*, const AudioRendererParameter&, void*, size_t)
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_OpenAudioRenderer(
    handle: *mut usize,
    event: *mut (),
    param: *const (),
    _work: *mut (),
    _work_size: usize,
) -> u32 {
    record_hle_call("hle_nn_audio_OpenAudioRenderer");
    if !event.is_null() {
        let ev = hle_system_event_register(event as usize);
        nn_audio_start_frame_pacer(ev);
    }
    if !handle.is_null() {
        *handle = crate::horizon::audio::renderer::open(param as *const u32);
    }
    NN_AUDIO_RENDERER_STATE.store(1, std::sync::atomic::Ordering::SeqCst);
    println!("[+] HLE: nn::audio::OpenAudioRenderer -> audren service object (timing only, no audio output)");
    NN_RESULT_SUCCESS
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_StartAudioRenderer(_handle: usize) -> u32 {
    record_hle_call("hle_nn_audio_StartAudioRenderer");
    NN_AUDIO_RENDERER_STATE.store(2, std::sync::atomic::Ordering::SeqCst);
    crate::horizon::audio::renderer::set_running(true);
    NN_RESULT_SUCCESS
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_StopAudioRenderer(_handle: usize) {
    record_hle_call("hle_nn_audio_StopAudioRenderer");
    NN_AUDIO_RENDERER_STATE.store(1, std::sync::atomic::Ordering::SeqCst);
    crate::horizon::audio::renderer::set_running(false);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_CloseAudioRenderer(_handle: usize) {
    record_hle_call("hle_nn_audio_CloseAudioRenderer");
    NN_AUDIO_RENDERER_STATE.store(0, std::sync::atomic::Ordering::SeqCst);
}

// AudioDeviceName is `char name[256]`.
unsafe fn audio_write_device_name(out: *mut u8) {
    let name = b"AudioStereoJackOutput\0";
    std::ptr::write_bytes(out, 0, 256);
    std::ptr::copy_nonoverlapping(name.as_ptr(), out, name.len());
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_GetActiveAudioOutputDeviceName(out: *mut u8) {
    record_hle_call("hle_nn_audio_GetActiveAudioOutputDeviceName");
    if !out.is_null() {
        audio_write_device_name(out);
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_ListAudioOutputDeviceName(out: *mut u8, count: i32) -> i32 {
    record_hle_call("hle_nn_audio_ListAudioOutputDeviceName");
    if out.is_null() || count < 1 {
        return 0;
    }
    audio_write_device_name(out);
    1
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_GetActiveAudioDeviceChannelCountForOutput() -> i32 {
    record_hle_call("hle_nn_audio_GetActiveAudioDeviceChannelCountForOutput");
    2
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_SetAudioDeviceOutputVolume(_name: *const u8, _volume: f32) {
    record_hle_call("hle_nn_audio_SetAudioDeviceOutputVolume");
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_audio_AcquireAudioDeviceNotificationForOutput(event: *mut ()) {
    record_hle_call("hle_nn_audio_AcquireAudioDeviceNotificationForOutput");
    // Signaled only on output-device changes (headphones plugged in, ...); never fires here.
    if !event.is_null() {
        hle_system_event_register(event as usize);
    }
}

// ── nn::codec hardware Opus decoder (IPC to hwopus) ─────────────────────────
// Same story as audren: the decoder object lives in another sysmodule, so every
// entry point that touches it is HLE. Per-decoder state (sample rate, channels)
// is kept in a side table keyed by address and the getters are HLE too. Decoding goes through
// libopus (horizon/audio/opus.rs), or produces silence with correct accounting without it;
// nn::codec::GetOpusPacketSize is pure CPU and stays in the sdk module.
static HLE_OPUS_DECODERS: StdMutex<Vec<(usize, i32, i32)>> = StdMutex::new(Vec::new()); // (addr, rate, channels)

#[no_mangle]
pub unsafe extern "C" fn hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize(
    _rate: i32,
    channels: i32,
    _streams: i32,
) -> usize {
    record_hle_call("hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize");
    audio_align_up(0x10000 + 0x4000 * channels.max(1) as usize, 0x1000)
}
/// GetHardwareOpusDecoderWorkBufferSize(sampleRate, channelCount): the overload without the stream count.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize2(
    rate: i32,
    channels: i32,
) -> usize {
    hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize(rate, channels, 1)
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_codec_InitializeHardwareOpusDecoder(
    dec: *mut (),
    rate: i32,
    channels: i32,
    _work: *mut (),
    _work_size: usize,
    _extra: i32,
) -> u32 {
    record_hle_call("hle_nn_codec_InitializeHardwareOpusDecoder");
    if !dec.is_null() {
        if OPUS_HANDLE.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            OPUS_HANDLE.store(
                Box::into_raw(Box::new(OPUS_HANDLE_VTABLE.as_ptr() as usize)) as usize,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        std::ptr::write(
            dec as *mut OpusDecoderType,
            OpusDecoderType {
                handle: OPUS_HANDLE.load(std::sync::atomic::Ordering::Relaxed),
                buffer: _work as usize,
                size: _work_size,
                sample_rate: rate,
                channel_count: channels,
                perf: 0,
                reset_requested: false,
            },
        );
    }
    crate::horizon::audio::opus::destroy(dec as usize); // re-initialized: a fresh libopus decoder on the next decode
    let mut t = HLE_OPUS_DECODERS.lock().unwrap();
    t.retain(|(a, _, _)| *a != dec as usize);
    t.push((dec as usize, rate, channels));
    NN_RESULT_SUCCESS
}
/// nn::codec::HardwareOpusDecoder::Finalize(this): the class holds its HardwareOpusDecoderType at this+8 (its other
/// methods are short thunks onto the C API with this+8 and run natively).
#[no_mangle]
pub unsafe extern "C" fn hle_nn_codec_HardwareOpusDecoder_Finalize(this: *mut u8) {
    hle_nn_codec_FinalizeHardwareOpusDecoder(this.add(8) as *mut ());
}
/// Stand-in for the decoder's service object (HardwareOpusDecoderType::_handle): code that releases it calls
/// through its vtable, so every entry is a no-op.
unsafe extern "C" fn opus_handle_noop() -> u32 {
    0
}
static OPUS_HANDLE_VTABLE: [unsafe extern "C" fn() -> u32; 16] = [opus_handle_noop; 16];
static OPUS_HANDLE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// HardwareOpusDecoderType: _handle, _buffer, _size, _sampleRate, _channelCount,
/// _perf, _isResetRequested. Inline getters read these fields.
#[repr(C)]
struct OpusDecoderType {
    handle: usize,
    buffer: usize,
    size: usize,
    sample_rate: i32,
    channel_count: i32,
    perf: i64,
    reset_requested: bool,
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_codec_FinalizeHardwareOpusDecoder(dec: *mut ()) {
    record_hle_call("hle_nn_codec_FinalizeHardwareOpusDecoder");
    crate::horizon::audio::opus::destroy(dec as usize);
    HLE_OPUS_DECODERS
        .lock()
        .unwrap()
        .retain(|(a, _, _)| *a != dec as usize);
    if !dec.is_null() {
        std::ptr::write_bytes(dec as *mut OpusDecoderType, 0, 1);
    }
}
fn hle_opus_decoder(dec: usize) -> Option<(i32, i32)> {
    HLE_OPUS_DECODERS
        .lock()
        .unwrap()
        .iter()
        .find(|(a, _, _)| *a == dec)
        .map(|(_, r, c)| (*r, *c))
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_codec_GetHardwareOpusDecoderSampleRate(dec: *const ()) -> i32 {
    record_hle_call("hle_nn_codec_GetHardwareOpusDecoderSampleRate");
    hle_opus_decoder(dec as usize).map(|(r, _)| r).unwrap_or(0)
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_codec_GetHardwareOpusDecoderChannelCount(dec: *const ()) -> i32 {
    record_hle_call("hle_nn_codec_GetHardwareOpusDecoderChannelCount");
    hle_opus_decoder(dec as usize).map(|(_, c)| c).unwrap_or(0)
}
#[no_mangle]
pub unsafe extern "C" fn hle_nn_codec_ResetHardwareOpusDecoder(dec: *mut ()) {
    record_hle_call("hle_nn_codec_ResetHardwareOpusDecoder");
    crate::horizon::audio::opus::reset(dec as usize);
}
/// Samples per channel in one Opus packet at `rate`, from the TOC byte (RFC 6716 §3.1).
fn opus_packet_samples(pkt: &[u8], rate: i32) -> usize {
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
/// nn::codec::DecodeOpusInterleavedWithHardware(type*, size_t* outConsumed, int* outSamples,
///     int16_t* outBuf, size_t outBufSize, const void* in, size_t inSize)
/// Input is Switch hwopus framing: BE u32 packet size, BE u32 final range, then the Opus packet.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_codec_DecodeOpusInterleavedWithHardware(
    dec: *mut (),
    out_consumed: *mut usize,
    out_samples: *mut i32,
    out_buf: *mut i16,
    out_buf_size: usize,
    input: *const u8,
    input_size: usize,
) -> u32 {
    record_hle_call("hle_nn_codec_DecodeOpusInterleavedWithHardware");
    let Some((rate, channels)) = hle_opus_decoder(dec as usize) else {
        return 0x4E;
    };
    if input.is_null() || input_size < 8 {
        return 0x4E;
    }
    let inp = std::slice::from_raw_parts(input, input_size);
    let pkt_size = u32::from_be_bytes([inp[0], inp[1], inp[2], inp[3]]) as usize;
    if 8 + pkt_size > input_size {
        return 0x4E;
    }
    let mut samples = opus_packet_samples(&inp[8..8 + pkt_size], rate);
    let frame_bytes = channels.max(1) as usize * 2;
    let bytes = samples * frame_bytes;
    if bytes > out_buf_size {
        return 0x4E;
    }
    // Real decoding through libopus (horizon/audio/opus.rs); silence with the same accounting when it is unavailable or fails.
    match (!out_buf.is_null())
        .then(|| {
            crate::horizon::audio::opus::decode(
                dec as usize,
                rate,
                channels,
                &inp[8..8 + pkt_size],
                out_buf,
                out_buf_size / frame_bytes,
            )
        })
        .flatten()
    {
        Some(n) => samples = n,
        None => {
            if !out_buf.is_null() {
                std::ptr::write_bytes(out_buf as *mut u8, 0, bytes);
            }
        }
    }
    if !out_consumed.is_null() {
        *out_consumed = 8 + pkt_size;
    }
    if !out_samples.is_null() {
        *out_samples = samples as i32;
    }
    NN_RESULT_SUCCESS
}

// ── Guest-main sampler (--sample-main=<seconds>) ──────────────────────────────────────────────
// Suspends the guest-main thread every few ms, reads its register state and walks the frame-pointer chain, then
// prints the hottest guest call chains. Shows which guest function the main loop is actually running/waiting in.
static SAMPLE_TARGET: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

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
    let base = crate::cpu::crash_handler::get_guest_base();
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

#[no_mangle]
pub extern "C" fn hle_nn_os_GetCurrentCoreNumber() -> u32 {
    current_core()
}
