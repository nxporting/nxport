// Guest cores: a guest thread runs game code only while it holds its core, as on the console, where a core runs one
// thread at a time. Games rely on it: per-core work buffers are written without locks by every thread pinned to that
// core. Engine code (any HLE call from game code) runs without the core, so blocking calls never hold one.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

const CORES: usize = 4;

static ENABLED: AtomicBool = AtomicBool::new(false);
static BUSY: [Mutex<bool>; CORES] = [const { Mutex::new(false) }; CORES];
static FREED: [Condvar; CORES] = [const { Condvar::new() }; CORES];

thread_local! {
    static GUEST: Cell<bool> = const { Cell::new(false) };
    static HELD: Cell<usize> = const { Cell::new(usize::MAX) };
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

fn acquire() {
    if !ENABLED.load(Ordering::Relaxed) || !GUEST.with(|g| g.get()) || HELD.with(|h| h.get()) != usize::MAX {
        return;
    }
    let c = (super::hle::current_core() as usize).min(CORES - 1);
    let mut busy = BUSY[c].lock().unwrap();
    while *busy {
        busy = FREED[c].wait(busy).unwrap();
    }
    *busy = true;
    HELD.with(|h| h.set(c));
}

fn release() {
    let c = HELD.with(|h| h.replace(usize::MAX));
    if c != usize::MAX {
        *BUSY[c].lock().unwrap() = false;
        FREED[c].notify_one();
    }
}

/// Runs `f` as a guest thread: on its core while in game code.
pub fn run_guest(f: impl FnOnce()) {
    GUEST.with(|g| g.set(true));
    acquire();
    f();
    release();
    GUEST.with(|g| g.set(false));
}

extern "C" fn hle_entered() {
    release();
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
