// HLE of the low-level lock primitives: nn::os::detail::InternalCriticalSectionImplByHorizon and
// InternalConditionVariableImplByHorizon (used across internal modules).
//
// The native versions take the lock with an atomic compare-and-swap of the owner's thread handle, read
// from the Horizon TLS, and fall back to kernel calls under contention (svcArbitrateLock/Unlock,
// svcWaitProcessWideKeyAtomic/SignalProcessWideKey). macOS ignores an svc's immediate and runs whichever
// BSD syscall number happens to be in x16, so a contended native lock either kills the process with
// SIGSYS or performs an arbitrary host syscall. Here ownership is tracked per host thread and contention
// blocks on a host condition variable. The guest lock word still holds the owner's handle (0 when free)
// so code that inspects it inline sees the native encoding.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

struct Tables {
    /// critical section address -> owning host thread
    owners: HashMap<usize, u64>,
    /// condition variable address -> signal generation
    generations: HashMap<usize, u64>,
}

static TABLES: Mutex<Option<Tables>> = Mutex::new(None);
static WAKE: Condvar = Condvar::new();
static NEXT_TID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static TID: u64 = NEXT_TID.fetch_add(1, Ordering::Relaxed);
}

fn me() -> u64 {
    TID.with(|t| *t)
}

fn tables() -> std::sync::MutexGuard<'static, Option<Tables>> {
    let mut g = TABLES.lock().unwrap();
    if g.is_none() {
        *g = Some(Tables {
            owners: HashMap::new(),
            generations: HashMap::new(),
        });
    }
    g
}

/// The value the native code stores in a held lock word: the owner's nn::os thread handle.
unsafe fn guest_handle() -> u32 {
    let tp: usize;
    std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tp);
    if tp >= 0x10000 {
        let thread = *((tp + 0x1f8) as *const usize);
        if thread >= 0x10000 {
            let h = *((thread + 0x1b0) as *const u32);
            if h != 0 {
                return h;
            }
        }
    }
    1
}

unsafe fn word<'a>(cs: usize) -> &'a AtomicU32 {
    &*(cs as *const AtomicU32)
}

/// Takes the lock if its guest word is free (0). The word is the real lock, so guest code that locks it
/// inline stays mutually exclusive with these functions. Must be called with the tables locked.
unsafe fn try_take(t: &mut Tables, cs: usize, tid: u64) -> bool {
    if word(cs)
        .compare_exchange(0, guest_handle(), Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        t.owners.insert(cs, tid);
        true
    } else {
        false
    }
}

unsafe fn enter_locked(
    mut g: std::sync::MutexGuard<'static, Option<Tables>>,
    cs: usize,
) -> std::sync::MutexGuard<'static, Option<Tables>> {
    let tid = me();
    if g.as_ref().unwrap().owners.get(&cs) == Some(&tid) {
        static WARNED: AtomicU32 = AtomicU32::new(0);
        if WARNED.fetch_add(1, Ordering::Relaxed) < 3 {
            eprintln!("[!] sdk_sync: critical section {:#x} re-entered by its owner (real hardware would deadlock)", cs);
        }
        return g;
    }
    while !try_take(g.as_mut().unwrap(), cs, tid) {
        // Inline lockers release the word without notifying WAKE, so also re-check periodically.
        g = WAKE
            .wait_timeout(g, std::time::Duration::from_micros(200))
            .unwrap()
            .0;
    }
    g
}

fn leave_locked(t: &mut Tables, cs: usize) {
    if t.owners.get(&cs) == Some(&me()) {
        t.owners.remove(&cs);
        unsafe { word(cs).store(0, Ordering::Release) };
        WAKE.notify_all();
    } else if t.owners.contains_key(&cs) || unsafe { word(cs).load(Ordering::Acquire) } != 0 {
        static WARNED: AtomicU32 = AtomicU32::new(0);
        if WARNED.fetch_add(1, Ordering::Relaxed) < 3 {
            eprintln!(
                "[!] sdk_sync: critical section {:#x} left by a thread that does not own it",
                cs
            );
        }
    }
}

unsafe extern "C" fn cs_enter(cs: usize) {
    drop(enter_locked(tables(), cs));
}

unsafe extern "C" fn cs_try_enter(cs: usize) -> bool {
    let tid = me();
    let mut g = tables();
    let t = g.as_mut().unwrap();
    if t.owners.get(&cs) == Some(&tid) {
        return false; // native TryEnter fails on a held word, including one held by the caller
    }
    try_take(t, cs, tid)
}

unsafe extern "C" fn cs_leave(cs: usize) {
    leave_locked(tables().as_mut().unwrap(), cs);
}

unsafe extern "C" fn cs_is_locked_by_current_thread(cs: usize) -> bool {
    tables().as_ref().unwrap().owners.get(&cs) == Some(&me())
}

unsafe extern "C" fn cv_signal(cv: usize) {
    let mut g = tables();
    *g.as_mut().unwrap().generations.entry(cv).or_insert(0) += 1;
    WAKE.notify_all();
}

unsafe extern "C" fn cv_broadcast(cv: usize) {
    cv_signal(cv);
}

/// Releases `cs`, waits for a signal on `cv` (or the deadline), then re-acquires `cs`. Returns true when
/// signaled. Spurious wakeups are allowed by the API, so a signal meant for another waiter may wake this
/// one too.
unsafe fn cv_wait_until(cv: usize, cs: usize, deadline: Option<std::time::Instant>) -> bool {
    let mut g = tables();
    let gen0 = *g.as_mut().unwrap().generations.entry(cv).or_insert(0);
    leave_locked(g.as_mut().unwrap(), cs);
    let mut signaled = true;
    loop {
        if *g.as_ref().unwrap().generations.get(&cv).unwrap_or(&0) != gen0 {
            break;
        }
        match deadline {
            None => g = WAKE.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    signaled = false;
                    break;
                }
                g = WAKE.wait_timeout(g, d - now).unwrap().0;
            }
        }
    }
    drop(enter_locked(g, cs));
    signaled
}

unsafe extern "C" fn cv_wait(cv: usize, cs: usize) {
    cv_wait_until(cv, cs, None);
}

/// `helper` is a nn::os::detail::TimeoutHelper: the absolute deadline in CNTPCT ticks.
unsafe extern "C" fn cv_timed_wait(cv: usize, cs: usize, helper: *const u64) -> u32 {
    let target = if helper.is_null() { 0 } else { *helper };
    let now_tick: u64;
    std::arch::asm!("mrs {}, cntpct_el0", out(reg) now_tick); // system timer ticks for deadline
    let freq: u64;
    std::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq);
    let left = target.saturating_sub(now_tick);
    if left == 0 {
        return 0;
    }
    let ns = (left as u128 * 1_000_000_000 / freq.max(1) as u128).min(u64::MAX as u128) as u64;
    let deadline = std::time::Instant::now().checked_add(std::time::Duration::from_nanos(ns));
    cv_wait_until(cv, cs, deadline) as u32
}

/// (mangled symbol, replacement) pairs, patched over the module's own definitions at startup.
pub fn interpositions() -> Vec<(&'static str, usize)> {
    vec![
        ("_ZN2nn2os6detail36InternalCriticalSectionImplByHorizon5EnterEv", cs_enter as *const () as usize),
        ("_ZN2nn2os6detail36InternalCriticalSectionImplByHorizon8TryEnterEv", cs_try_enter as *const () as usize),
        ("_ZN2nn2os6detail36InternalCriticalSectionImplByHorizon5LeaveEv", cs_leave as *const () as usize),
        ("_ZNK2nn2os6detail36InternalCriticalSectionImplByHorizon23IsLockedByCurrentThreadEv", cs_is_locked_by_current_thread as *const () as usize),
        ("_ZN2nn2os6detail38InternalConditionVariableImplByHorizon6SignalEv", cv_signal as *const () as usize),
        ("_ZN2nn2os6detail38InternalConditionVariableImplByHorizon9BroadcastEv", cv_broadcast as *const () as usize),
        ("_ZN2nn2os6detail38InternalConditionVariableImplByHorizon4WaitEPNS1_23InternalCriticalSectionE", cv_wait as *const () as usize),
        ("_ZN2nn2os6detail38InternalConditionVariableImplByHorizon9TimedWaitEPNS1_23InternalCriticalSectionERKNS1_13TimeoutHelperE", cv_timed_wait as *const () as usize),
    ]
}
