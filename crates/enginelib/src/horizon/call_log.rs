// The HLE call log: the last calls of every thread (record_hle_call, kept for crash and hang reports), threads blocked
// in HLE waits, and reports of long waits with their guest call chains.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex as StdMutex, OnceLock};

// The HLE call history, always on: every NVN HLE function (implemented ones and the labelled stubs) records each call,
// so crash and hang reports list the calls made before them.
pub(crate) const HLE_CALL_HISTORY_SIZE: usize = 4096;

// Each thread records into a ring of its own, with plain stores: a ring for all threads has its lock contended by every
// guest thread making HLE calls, and even a lock per thread (or a shared sequence counter) costs a few percent of the
// busy time of the game's busiest threads.
// Readers merge the rings by sequence number (on x86-64 the time stamp counter, synchronized across cores) and skip a
// slot written while they read it. Names are 'static (literals,
// and the rare formatted name interned once), so a slot is a sequence number and a str's pointer and length.
pub(crate) const HLE_CALL_RING_SIZE: usize = 1024;

pub(crate) struct HleCallSlot {
    seq: AtomicU64, // 0: empty or being written
    ptr: AtomicUsize,
    len: AtomicUsize,
}

pub(crate) struct HleCallRing {
    next: AtomicUsize,
    slots: [HleCallSlot; HLE_CALL_RING_SIZE],
}

pub(crate) static HLE_CALL_RINGS: StdMutex<Vec<&'static HleCallRing>> = StdMutex::new(Vec::new());

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub(crate) static HLE_CALL_SEQ: AtomicU64 = AtomicU64::new(1);

thread_local! {
    // Rings outlive their threads (HLE calls from TLS destructors still record), so the slot needs no destructor.
    static HLE_CALL_RING: std::cell::Cell<Option<&'static HleCallRing>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn intern_hle_call_name(name: String) -> &'static str {
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

pub(crate) fn record_hle_call(name: impl Into<std::borrow::Cow<'static, str>>) {
    let name: &'static str = match name.into() {
        std::borrow::Cow::Borrowed(n) => n,
        std::borrow::Cow::Owned(n) => intern_hle_call_name(n),
    };
    if crate::coverage::on() {
        crate::coverage::note(crate::coverage::Kind::Call, name);
    }
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
    let i = ring.next.load(Ordering::Relaxed);
    #[cfg(target_arch = "x86_64")]
    let seq = unsafe { core::arch::x86_64::_rdtsc() };
    // The virtual counter (synchronized across cores) with the ring position below it: a thread's calls within one tick
    // stay in order. A shared atomic counter would move its cache line between every thread making HLE calls.
    #[cfg(target_arch = "aarch64")]
    let seq = {
        let cnt: u64;
        unsafe { core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nomem, nostack)) };
        (cnt << 10) | i as u64 | 1 << 63
    };
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    let seq = HLE_CALL_SEQ.fetch_add(1, Ordering::Relaxed);
    ring.next.store((i + 1) % HLE_CALL_RING_SIZE, Ordering::Relaxed);
    let slot = &ring.slots[i];
    slot.seq.store(0, Ordering::Relaxed);
    std::sync::atomic::fence(Ordering::Release);
    slot.ptr.store(name.as_ptr() as usize, Ordering::Relaxed);
    slot.len.store(name.len(), Ordering::Relaxed);
    slot.seq.store(seq, Ordering::Release);
}

/// The last HLE_CALL_HISTORY_SIZE recorded calls of all threads, oldest first.
pub(crate) fn hle_call_history() -> Vec<(u64, std::borrow::Cow<'static, str>)> {
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

// Wait graph: which guest thread is currently blocked on which sync object, and from where. Dumped by
// summarize_hle_call_history() to tell a deadlocked worker apart from an idle one.
pub(crate) type BlockedEntry = (&'static str, usize, [usize; 4], std::time::Instant, std::thread::Thread);

pub(crate) static BLOCKED: OnceLock<StdMutex<rustc_hash::FxHashMap<u64, BlockedEntry>>> = OnceLock::new();

pub(crate) fn blocked_table() -> &'static StdMutex<rustc_hash::FxHashMap<u64, BlockedEntry>> {
    BLOCKED.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

/// The guest frame pointer of the function that called an HLE function, from `fp` = x29 as the HLE function sees it:
/// natively its own frame record, which saved the guest's; lifted, the guest's x29 itself (HLE functions are host
/// calls that leave the guest registers as they were).
pub(crate) unsafe fn first_guest_fp(fp: usize) -> usize {
    if cfg!(nx_native) {
        if frame_ok(fp) {
            *(fp as *const usize)
        } else {
            0
        }
    } else {
        fp
    }
}

/// Whether `a` can be a frame record of the calling thread: on its guest stack, or on its host stack above the current
/// stack pointer. x29 is not always a frame pointer (code built without frame pointers keeps data in it), so the frame
/// walks below check every record before reading it.
pub(crate) fn frame_ok(a: usize) -> bool {
    if a & 7 != 0 || a < 0x1_0000_0000 {
        return false;
    }
    if let Some((lo, hi)) = crate::horizon::thread_stack::current_bounds() {
        if a >= lo && a + 16 <= hi {
            return true;
        }
    }
    let sp: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags))
    };
    #[cfg(not(target_arch = "aarch64"))]
    {
        let x = 0u8;
        sp = &x as *const u8 as usize;
    }
    a >= sp && a < sp + (16 << 20)
}

pub(crate) struct BlockGuard(u64);

impl BlockGuard {
    /// `fp` is x29 inside the HLE function (see report_long_wait); records the guest return-address chain.
    pub unsafe fn new(kind: &'static str, obj: usize, fp: usize) -> Self {
        let mut chain = [0usize; 4];
        let mut gfp = first_guest_fp(fp);
        for slot in chain.iter_mut() {
            if !frame_ok(gfp) {
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

pub(crate) fn dump_blocked_threads() {
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

/// Prints a summary of the ring buffer (call counts, then the most recent calls in order) to stdout. Used with
/// --hle-history-at=<present number> to see what the game polls while it sits on a loading screen.
pub(crate) fn summarize_hle_call_history() {
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

/// Called from the crash handler. Prints the most recent HLE calls in order,
/// oldest first, ending with whatever ran immediately before the fault.
pub(crate) fn dump_hle_call_history() {
    let hist = hle_call_history();
    eprintln!(
        "    HLE Call History (last {} calls, oldest first):",
        hist.len()
    );
    for (seq, name) in hist.iter() {
        eprintln!("      #{}: {}", seq, name);
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
            if !frame_ok(gfp) {
                break;
            }
            chain.push(fmt(*((gfp + 8) as *const usize)));
            gfp = *(gfp as *const usize);
        }
    }
    chain.join(" <- ")
}

/// When the game last presented a frame (platform clock ns; 0: never), for report_long_wait.
static LAST_PRESENT_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The game is being closed: its threads stop with it, so waits from then on are not stalls.
static QUITTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn note_quit() {
    QUITTING.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The game presented a frame (nvnQueuePresentTexture).
pub(crate) fn note_present() {
    LAST_PRESENT_NS.store(crate::platform::clock::now_ns(), std::sync::atomic::Ordering::Relaxed);
}

/// A wait that has lasted `waited`. "[STALL]" only when no frame was presented since it began (the game froze); a
/// thread waiting long while frames go on (a worker with no work during a load) is "[wait]".
pub(crate) fn report_long_wait(what: &str, obj: usize, fp: usize, waited: std::time::Duration) {
    if QUITTING.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let began = crate::platform::clock::now_ns().saturating_sub(waited.as_nanos() as u64);
    let last = LAST_PRESENT_NS.load(std::sync::atomic::Ordering::Relaxed);
    // Before the first frame the game is starting (caches, files): long waits are expected there, and a start that never
    // shows a frame is seen as such (no present at all).
    let frames_meanwhile = last > began || last == 0;
    let tag = if frames_meanwhile { "[wait]" } else { "[STALL]" };
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
            if !frame_ok(gfp) {
                break;
            }
            chain.push(fmt(*((gfp + 8) as *const usize)));
            gfp = *(gfp as *const usize);
        }
    }
    eprintln!(
        "{} {} on {:#x} blocked {}s{} (guest callers: {})",
        tag,
        what,
        obj,
        waited.as_secs(),
        if last == 0 { ", before the first frame" } else if frames_meanwhile { ", frames kept coming" } else { ", no frame presented meanwhile" },
        chain.join(" <- ")
    );
}
// ── Guest-main sampler (--sample-main=<seconds>) ──────────────────────────────────────────────
// Suspends the guest-main thread every few ms, reads its register state and walks the frame-pointer chain, then
// prints the hottest guest call chains. Shows which guest function the main loop is actually running/waiting in.
#[cfg(nx_native)]
pub(crate) static SAMPLE_TARGET: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(nx_native)]
pub(crate) fn register_main_sample_target() {
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
pub(crate) fn sample_main_thread() {
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
