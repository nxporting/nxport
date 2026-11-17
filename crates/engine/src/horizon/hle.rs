// The nn:: libraries HLE'd at the guest's import boundary: threads and synchronisation, memory, TLS, clocks, the
// C++ runtime hooks, display (vi) and graphics bootstrap (nv), audio and codec entry points.
#![allow(non_snake_case)]

use crate::horizon::os::threads::*;
use std::ffi::CStr;








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






// 4. Horizon OS Thread & Mutex implementation (backed by thread-aware recursive lock)
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};


const _: () = assert!(std::mem::size_of::<NnThreadType>() == 0x1C0);










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










































































#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_SetPriorityOnCurrentThread(_prio: i32) {}
















 // Horizon requires 64KB alignment for address regions





















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
use std::sync::{Mutex as StdMutex, OnceLock};














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

