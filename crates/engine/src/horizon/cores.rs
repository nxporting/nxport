// Guest cores: a guest thread runs game code only while it holds its core, as on the console, where a core runs one
// thread at a time. Games rely on it: per-core work buffers are written without locks by every thread pinned to that
// core. During engine code (any HLE call from game code) the core is lent: the caller takes it back on return without a
// thread switch, and another thread of that core may take it once the call has lasted the grace period, so blocking
// calls never hold one.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// Per core: when it was last lent (host ns).
static LENT_AT: [AtomicU64; CORES] = [const { AtomicU64::new(0) }; CORES];
static WAKE_LOCK: [Mutex<()>; CORES] = [const { Mutex::new(()) }; CORES];
static WAKE: [Condvar; CORES] = [const { Condvar::new() }; CORES];
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static GUEST: Cell<bool> = const { Cell::new(false) };
    static ID: u64 = NEXT_ID.fetch_add(1, Ordering::Relaxed) << 1;
    static HELD: Cell<usize> = const { Cell::new(usize::MAX) };
    /// The thread's core and the assignment generation it was read at.
    static CORE: Cell<(u64, usize)> = const { Cell::new((u64::MAX, 0)) };
}

/// The function the HLE entry stub calls with the HLE function in x16.
pub fn entry() -> *const () {
    if crate::settings::get().guest.guest_cores {
        ENABLED.store(true, Ordering::Relaxed);
        hle_glue as *const ()
    } else {
        hle_direct as *const ()
    }
}

extern "C" {
    fn clock_gettime_nsec_np(clock: u32) -> u64;
}

fn now_ns() -> u64 {
    const CLOCK_UPTIME_RAW: u32 = 8;
    unsafe { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }
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

fn acquire() {
    if !ENABLED.load(Ordering::Relaxed) || !GUEST.with(|g| g.get()) || HELD.with(|h| h.get()) != usize::MAX {
        return;
    }
    let c = my_core();
    let me = ID.with(|i| *i);
    // Back from a call the core was lent for.
    if STATE[c].compare_exchange(me | 1, me, Ordering::Acquire, Ordering::Relaxed).is_err() {
        take(c, me);
    }
    HELD.with(|h| h.set(c));
}

/// Waits for core `c`: a free core goes to whoever comes first, a lent one to any thread after the grace period, or to a
/// thread that waited STARVED_NS.
fn take(c: usize, me: u64) {
    let since = now_ns();
    loop {
        let st = STATE[c].load(Ordering::Acquire);
        let wait_ns = if st == 0 {
            if STATE[c].compare_exchange(0, me, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                break;
            }
            continue;
        } else if st & 1 != 0 {
            let age = now_ns().saturating_sub(LENT_AT[c].load(Ordering::Relaxed));
            if age >= GRACE_NS || now_ns() - since >= STARVED_NS {
                if STATE[c].compare_exchange(st, me, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                    break;
                }
                continue;
            }
            GRACE_NS - age
        } else {
            200_000
        };
        let g = WAKE_LOCK[c].lock().unwrap();
        if STATE[c].load(Ordering::Acquire) == st {
            drop(WAKE[c].wait_timeout(g, std::time::Duration::from_nanos(wait_ns.max(5_000))).unwrap());
        }
    }
}

/// Engine code: the core stays this thread's unless another thread of the core takes it after the grace period.
fn lend() {
    let c = HELD.with(|h| h.replace(usize::MAX));
    if c != usize::MAX {
        LENT_AT[c].store(now_ns(), Ordering::Relaxed);
        STATE[c].store(ID.with(|i| *i) | 1, Ordering::Release);
    }
}

fn free() {
    let c = HELD.with(|h| h.replace(usize::MAX));
    if c != usize::MAX {
        STATE[c].store(0, Ordering::Release);
        let _g = WAKE_LOCK[c].lock().unwrap();
        WAKE[c].notify_all();
    }
}

/// Runs `f` as a guest thread: on its core while in game code.
pub fn run_guest(f: impl FnOnce()) {
    GUEST.with(|g| g.set(true));
    acquire();
    f();
    free();
    GUEST.with(|g| g.set(false));
}

extern "C" fn hle_entered() {
    lend();
}

extern "C" fn hle_returning() {
    acquire();
}

/// Called by the HLE entry stub for outermost HLE calls with the function in x16: releases the core around it. Keeps
/// x29 (HLE code walks the guest frame chain from it), argument registers x0-x8 / q0-q7 and return registers x0-x1 /
/// q0-q3.
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
#[unsafe(naked)]
pub unsafe extern "C" fn hle_direct() {
    std::arch::naked_asm!("br x16")
}


