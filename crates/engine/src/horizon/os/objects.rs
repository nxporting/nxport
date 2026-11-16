// The objects nn::os functions manage (os/mod.rs): mutexes, semaphores, light and timer events, condition variables,
// message queues and reader / writer locks, by guest object address.

use super::*;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};

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
pub(crate) fn mutex_park(word: &std::sync::atomic::AtomicU32, held: u32) {
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

pub(crate) struct LightMsgQueueInner {
    pub(crate) capacity: usize,
    pub(crate) queue: VecDeque<u64>,
}

pub(crate) struct LightMsgQueueState {
    pub(crate) inner: StdMutex<LightMsgQueueInner>,
    pub(crate) not_empty: Condvar,
    pub(crate) not_full: Condvar,
}

pub(crate) static LIGHT_MQ_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<LightMsgQueueState>>>> =
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

pub(crate) static CONDVAR_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<CondVarState>>>> =
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

pub(crate) static LIGHT_EVENT_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<LightEventState>>>> =
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

pub(crate) static TIMER_EVENT_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<TimerEventState>>>> =
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
pub(crate) struct SemaphoreState {
    count: StdMutex<i32>,
    max: i32,
    cv: Condvar,
}

pub(crate) static SEMAPHORE_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<SemaphoreState>>>> =
    OnceLock::new();

pub(crate) fn semaphore_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<SemaphoreState>>> {
    SEMAPHORE_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

pub(crate) fn semaphore_new(initial: i32, max: i32) -> Arc<SemaphoreState> {
    Arc::new(SemaphoreState {
        count: StdMutex::new(initial),
        max,
        cv: Condvar::new(),
    })
}

pub(crate) unsafe fn semaphore_get(sem: *mut ()) -> Arc<SemaphoreState> {
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

// ── global sync state ────────────────────────────────────────────────────────────────────────
#[derive(Default)]
pub(crate) struct EventSt {
    pub(crate) signaled: bool,
    pub(crate) auto_clear: bool,
    /// Signals so far: a manual-clear waiter returns once one arrived after it began, even if cleared again since.
    pub(crate) signals: u64,
}

#[derive(Default)]
pub(crate) struct MsgQ {
    pub(crate) cap: usize,
    pub(crate) q: VecDeque<u64>,
}

#[derive(Default)]
pub(crate) struct RwSt {
    readers: u32,
    writer: usize, // owning thread token, 0 = none
    write_depth: u32,
}

#[derive(Default)]
pub(crate) struct Sync {
    pub(crate) events: HashMap<usize, EventSt>,
    pub(crate) holders: HashMap<usize, usize>, // holder addr -> event addr
    pub(crate) multiwaits: HashMap<usize, Vec<usize>>, // multiwait addr -> ordered holder addrs
    pub(crate) msgqs: HashMap<usize, MsgQ>,
    pub(crate) rwlocks: HashMap<usize, RwSt>,
}

pub(crate) static SYNC: Mutex<Option<Sync>> = Mutex::new(None);

pub(crate) static SYNC_CV: Condvar = Condvar::new();

pub(crate) fn lock() -> MutexGuard<'static, Option<Sync>> {
    let mut g = SYNC.lock().unwrap();
    if g.is_none() {
        *g = Some(Sync::default());
    }
    g
}

/// Waits for the event; a signaled auto-clear event is consumed. `deadline` None = forever. Returns whether it was
/// signaled. An event nothing initialised (no side-table entry) never signals.
pub(crate) fn event_wait(ev: usize, deadline: Option<std::time::Instant>) -> bool {
    let mut g = lock();
    let signals0 = g.as_ref().unwrap().events.get(&ev).map(|e| e.signals);
    loop {
        if let Some(e) = g.as_mut().unwrap().events.get_mut(&ev) {
            if e.signaled {
                if e.auto_clear {
                    e.signaled = false;
                }
                return true;
            }
            if !e.auto_clear && signals0.is_some_and(|n| n != e.signals) {
                return true;
            }
        }
        match deadline {
            None => g = SYNC_CV.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    return false;
                }
                g = SYNC_CV.wait_timeout(g, d - now).unwrap().0;
            }
        }
    }
}

/// Message queue operations with a deadline (None = block forever, Some(now) = try once). `front` jams the value at the
/// head instead of the tail. Returns false on timeout; an unknown queue fails at once.
pub(crate) fn mq_send(mq: usize, data: u64, front: bool, deadline: Option<std::time::Instant>) -> bool {
    let mut g = lock();
    loop {
        match g.as_mut().unwrap().msgqs.get_mut(&mq) {
            Some(q) if q.q.len() < q.cap => {
                if front {
                    q.q.push_front(data)
                } else {
                    q.q.push_back(data)
                }
                drop(g);
                SYNC_CV.notify_all();
                return true;
            }
            Some(_) => {}
            None => return false,
        }
        g = match deadline {
            None => SYNC_CV.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    return false;
                }
                SYNC_CV.wait_timeout(g, d - now).unwrap().0
            }
        };
    }
}

/// `peek`: read the head without removing it.
pub(crate) fn mq_receive(out: *mut u64, mq: usize, deadline: Option<std::time::Instant>, peek: bool) -> bool {
    let mut g = lock();
    loop {
        match g.as_mut().unwrap().msgqs.get_mut(&mq) {
            Some(q) => {
                let v = if peek { q.q.front().copied() } else { q.q.pop_front() };
                if let Some(v) = v {
                    if !out.is_null() {
                        unsafe { *out = v };
                    }
                    drop(g);
                    SYNC_CV.notify_all();
                    return true;
                }
            }
            None => return false,
        }
        g = match deadline {
            None => SYNC_CV.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    return false;
                }
                SYNC_CV.wait_timeout(g, d - now).unwrap().0
            }
        };
    }
}

// ── nn::os::ReaderWriterLock (real exclusion) ────────────────────────────────────────────────
pub(crate) fn rw_init(addr: usize) {
    lock()
        .as_mut()
        .unwrap()
        .rwlocks
        .insert(addr, RwSt::default());
}

pub(crate) fn rw_acquire_write(addr: usize) {
    let t = me();
    let mut g = lock();
    loop {
        let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
        if r.writer == t {
            r.write_depth += 1;
            return;
        }
        if r.writer == 0 && r.readers == 0 {
            r.writer = t;
            r.write_depth = 1;
            return;
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}

pub(crate) fn rw_try_acquire_write(addr: usize) -> bool {
    let t = me();
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.writer == t {
        r.write_depth += 1;
        true
    } else if r.writer == 0 && r.readers == 0 {
        r.writer = t;
        r.write_depth = 1;
        true
    } else {
        false
    }
}

pub(crate) fn rw_release_write(addr: usize) {
    let t = me();
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.writer == t && r.write_depth > 0 {
        r.write_depth -= 1;
        if r.write_depth == 0 {
            r.writer = 0;
            drop(g);
            SYNC_CV.notify_all();
        }
    }
}

pub(crate) fn rw_acquire_read(addr: usize) {
    let t = me();
    let mut g = lock();
    loop {
        let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
        if r.writer == 0 || r.writer == t {
            r.readers += 1;
            return;
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}

pub(crate) fn rw_try_acquire_read(addr: usize) -> bool {
    let t = me();
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.writer == 0 || r.writer == t {
        r.readers += 1;
        true
    } else {
        false
    }
}

pub(crate) fn rw_release_read(addr: usize) {
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.readers > 0 {
        r.readers -= 1;
        if r.readers == 0 {
            drop(g);
            SYNC_CV.notify_all();
        }
    }
}

pub(crate) fn rw_write_held_by_me(addr: usize) -> bool {
    let t = me();
    lock()
        .as_mut()
        .unwrap()
        .rwlocks
        .get(&addr)
        .map(|r| r.writer == t)
        .unwrap_or(false)
}
