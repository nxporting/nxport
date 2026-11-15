// Guest cores: a guest thread runs game code only while it holds its core, as on the console, where a core runs one
// thread at a time: the best ready thread (lowest priority number; equal ones in turn). Games rely on it: per-core work
// buffers are written without locks by every thread pinned to that core, and work a thread starts for a worse thread of
// its own core is not picked up until the starter blocks (Breath of the Wild 1.0.0: the core-0 ActorCreate thread ran a
// job before the main thread had set the object it reads).
//
// During engine code (any HLE call or svc from game code) the core is lent: the caller takes it back on return without
// a thread switch, unless a better thread of that core is waiting, which takes a lent core at once (Horizon would have
// preempted the caller). Other waiters take a lent core once the call has lasted the grace period, so blocking calls
// never hold one. Game code itself has no preemption points: a better thread waits for the holder's next engine call.
//
// Native code enters the engine through the HLE entry stub (`entry`); lifted code through lifted.rs (`engine`).

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

const CORES: usize = 4;
/// How long a thread in engine code keeps first claim on its core: quick calls (a lock, a TLS read) return to the core
/// without a thread switch; a thread on that core waiting longer than this takes it (the caller is blocked).
const GRACE_NS: u64 = 30_000;
/// A thread that has waited this long takes a lent core even while its holder keeps returning within the grace period
/// (a thread polling through short engine calls).
const STARVED_NS: u64 = 1_000_000;

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Per core: 0 free, else the holder's id << 1, with bit 0 set while the holder is in engine code (lent).
static STATE: [AtomicU64; CORES] = [const { AtomicU64::new(0) }; CORES];
/// Per core: when it was last lent (ns since START).
static LENT_AT: [AtomicU64; CORES] = [const { AtomicU64::new(0) }; CORES];
/// Per core: the holder's priority (the guest's nn::os priority: lower is better).
static HOLDER_PRIO: [AtomicI32; CORES] = [const { AtomicI32::new(i32::MAX) }; CORES];
/// Per core: the best priority among its waiters, i32::MAX with none (read without the lock on every return).
static BEST_WAIT: [AtomicI32; CORES] = [const { AtomicI32::new(i32::MAX) }; CORES];
/// Per core: its waiters (priority, ticket, id), under the core's wake lock.
static WAITERS: [Mutex<Vec<(i32, u64, u64)>>; CORES] = [const { Mutex::new(Vec::new()) }; CORES];
static WAKE: [Condvar; CORES] = [const { Condvar::new() }; CORES];
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TICKET: AtomicU64 = AtomicU64::new(0);
/// Bumped when a guest thread's priority changes (nn::os::ChangeThreadPriority).
pub static PRIO_GEN: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static GUEST: Cell<bool> = const { Cell::new(false) };
    static ID: u64 = NEXT_ID.fetch_add(1, Ordering::Relaxed) << 1;
    static HELD: Cell<usize> = const { Cell::new(usize::MAX) };
    /// The thread's core and the assignment generation it was read at.
    static CORE: Cell<(u64, usize)> = const { Cell::new((u64::MAX, 0)) };
    /// The thread's priority and the generation it was read at.
    static PRIO: Cell<(u64, i32)> = const { Cell::new((u64::MAX, 0)) };
    /// Engine calls in progress on this thread (lifted code: only the outermost one lends the core).
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Turns core scheduling on (settings: guest.guest_cores), before any guest thread runs.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// The function the HLE entry stub calls with the HLE function in x16.
#[cfg(nx_native)]
pub fn entry() -> *const () {
    if crate::settings::get().guest.guest_cores {
        enable();
        hle_glue as *const ()
    } else {
        hle_direct as *const ()
    }
}

fn now_ns() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64
}

fn my_core() -> usize {
    let g = super::hle::CORES_GEN.load(Ordering::Relaxed);
    let (cg, c) = CORE.with(|c| c.get());
    if cg == g {
        return c;
    }
    let c = (super::hle::current_core() as usize).min(CORES - 1);
    CORE.with(|cell| cell.set((g, c)));
    c
}

fn my_prio() -> i32 {
    let g = PRIO_GEN.load(Ordering::Relaxed);
    let (pg, p) = PRIO.with(|p| p.get());
    if pg == g {
        return p;
    }
    let p = unsafe { super::hle::hle_nn_os_GetThreadPriority(super::hle::hle_nn_os_GetCurrentThread()) };
    PRIO.with(|cell| cell.set((g, p)));
    p
}

fn acquire() {
    if !ENABLED.load(Ordering::Relaxed) || !GUEST.with(|g| g.get()) || HELD.with(|h| h.get()) != usize::MAX {
        return;
    }
    let c = my_core();
    let me = ID.with(|i| *i);
    let p = my_prio();
    // Back from a call the core was lent for, with no better thread waiting for it.
    if BEST_WAIT[c].load(Ordering::Acquire) >= p
        && STATE[c].compare_exchange(me | 1, me, Ordering::Acquire, Ordering::Relaxed).is_ok()
    {
        HELD.with(|h| h.set(c));
        return;
    }
    take(c, me, p);
    HOLDER_PRIO[c].store(p, Ordering::Relaxed);
    HELD.with(|h| h.set(c));
}

fn best_of(w: &[(i32, u64, u64)]) -> Option<(i32, u64, u64)> {
    w.iter().copied().min_by_key(|&(p, t, _)| (p, t))
}

/// Waits for core `c` as a waiter of priority `p`: the best waiter (priority, then arrival) takes a free core, a lent
/// one whose holder is worse than it, or a lent one after the grace period; a waiter starved STARVED_NS takes a lent one.
fn take(c: usize, me: u64, p: i32) {
    let since = now_ns();
    let mut w = WAITERS[c].lock().unwrap();
    w.push((p, NEXT_TICKET.fetch_add(1, Ordering::Relaxed), me));
    BEST_WAIT[c].store(best_of(&w).map_or(i32::MAX, |b| b.0), Ordering::Release);
    loop {
        let first = best_of(&w).is_some_and(|b| b.2 == me);
        let st = STATE[c].load(Ordering::Acquire);
        let wait_ns = if st == 0 {
            if first && STATE[c].compare_exchange(0, me, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                break;
            }
            200_000
        } else if st & 1 != 0 {
            let age = now_ns().saturating_sub(LENT_AT[c].load(Ordering::Relaxed));
            let better = p < HOLDER_PRIO[c].load(Ordering::Relaxed);
            let starved = now_ns() - since >= STARVED_NS;
            if (first && (better || age >= GRACE_NS)) || starved {
                if STATE[c].compare_exchange(st, me, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                    break;
                }
                continue;
            }
            if first { GRACE_NS - age.min(GRACE_NS) } else { 200_000 }
        } else {
            200_000
        };
        w = WAKE[c].wait_timeout(w, std::time::Duration::from_nanos(wait_ns.max(5_000))).unwrap().0;
    }
    w.retain(|&(_, _, id)| id != me);
    BEST_WAIT[c].store(best_of(&w).map_or(i32::MAX, |b| b.0), Ordering::Release);
    // The next best waiter re-reads its turn.
    WAKE[c].notify_all();
}

fn wake_waiters(c: usize) {
    if BEST_WAIT[c].load(Ordering::Acquire) != i32::MAX {
        let _g = WAITERS[c].lock().unwrap();
        WAKE[c].notify_all();
    }
}

/// Engine code: the core stays this thread's unless another thread of the core takes it (see the top of the file).
fn lend() {
    let c = HELD.with(|h| h.replace(usize::MAX));
    if c != usize::MAX {
        LENT_AT[c].store(now_ns(), Ordering::Relaxed);
        STATE[c].store(ID.with(|i| *i) | 1, Ordering::Release);
        wake_waiters(c);
    }
}

fn free() {
    let c = HELD.with(|h| h.replace(usize::MAX));
    if c != usize::MAX {
        STATE[c].store(0, Ordering::Release);
        let _g = WAITERS[c].lock().unwrap();
        WAKE[c].notify_all();
    }
}

/// Runs `f` as a guest thread: on its core while in game code.
pub fn run_guest<R>(f: impl FnOnce() -> R) -> R {
    GUEST.with(|g| g.set(true));
    acquire();
    let r = f();
    free();
    GUEST.with(|g| g.set(false));
    r
}

/// Engine code called from lifted game code (an HLE call or svc): the core is lent around the outermost one.
#[inline]
pub fn engine<R>(f: impl FnOnce() -> R) -> R {
    if !ENABLED.load(Ordering::Relaxed) {
        return f();
    }
    let outer = DEPTH.with(|d| {
        let n = d.get();
        d.set(n + 1);
        n == 0
    });
    if outer {
        lend();
    }
    let r = f();
    DEPTH.with(|d| d.set(d.get() - 1));
    if outer {
        acquire();
    }
    r
}

#[cfg(nx_native)]
extern "C" fn hle_entered() {
    lend();
}

#[cfg(nx_native)]
extern "C" fn hle_returning() {
    acquire();
}

/// Called by the HLE entry stub for outermost HLE calls with the function in x16: releases the core around it. Keeps
/// x29 (HLE code walks the guest frame chain from it), argument registers x0-x8 / q0-q7 and return registers x0-x1 /
/// q0-q3.
#[cfg(nx_native)]
#[unsafe(naked)]
pub unsafe extern "C" fn hle_glue() {
    std::arch::naked_asm!(
        "sub sp, sp, #0x120",
        "stp x29, x30, [sp]",
        "stp x0, x1, [sp, #0x10]",
        "stp x2, x3, [sp, #0x20]",
        "stp x4, x5, [sp, #0x30]",
        "stp x6, x7, [sp, #0x40]",
        "stp x8, x16, [sp, #0x50]",
        "stp q0, q1, [sp, #0x60]",
        "stp q2, q3, [sp, #0x80]",
        "stp q4, q5, [sp, #0xa0]",
        "stp q6, q7, [sp, #0xc0]",
        "bl {entered}",
        "ldp x0, x1, [sp, #0x10]",
        "ldp x2, x3, [sp, #0x20]",
        "ldp x4, x5, [sp, #0x30]",
        "ldp x6, x7, [sp, #0x40]",
        "ldp x8, x16, [sp, #0x50]",
        "ldp q0, q1, [sp, #0x60]",
        "ldp q2, q3, [sp, #0x80]",
        "ldp q4, q5, [sp, #0xa0]",
        "ldp q6, q7, [sp, #0xc0]",
        "blr x16",
        "stp x0, x1, [sp, #0x10]",
        "stp q0, q1, [sp, #0x60]",
        "stp q2, q3, [sp, #0x80]",
        "bl {returning}",
        "ldp x0, x1, [sp, #0x10]",
        "ldp q0, q1, [sp, #0x60]",
        "ldp q2, q3, [sp, #0x80]",
        "ldp x29, x30, [sp]",
        "add sp, sp, #0x120",
        "ret",
        entered = sym hle_entered,
        returning = sym hle_returning,
    )
}

/// The same entry without core scheduling.
#[cfg(nx_native)]
#[unsafe(naked)]
pub unsafe extern "C" fn hle_direct() {
    std::arch::naked_asm!("br x16")
}
