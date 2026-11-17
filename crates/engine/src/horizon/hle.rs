// The nn:: libraries HLE'd at the guest's import boundary: threads and synchronisation, memory, TLS, clocks, the
// C++ runtime hooks, display (vi) and graphics bootstrap (nv), audio and codec entry points.
#![allow(non_snake_case)]

use crate::horizon::os::threads::*;








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


const _: () = assert!(std::mem::size_of::<NnThreadType>() == 0x1C0);




















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

















/// Called from the crash handler. Prints the most recent HLE calls in order,
/// oldest first, ending with whatever ran immediately before the fault.
/// Prints a summary of the ring buffer (call counts, then the most recent calls in order) to stdout. Used with
/// --hle-history-at=<present number> to see what the game polls while it sits on a loading screen.

















// Pool allocator for nn::mem::StandardAllocator.
// TotK passes a large mmap'd region to Initialize; we bump-allocate from it
// so that all TotK-visible objects stay in guest memory (not macOS malloc).
// Free is intentionally a no-op: the pool lives for the process lifetime and
// the TotK heap system does its own internal free-list management on top.







// Flag so we only install the pool-page watchpoint once (after first bump alloc).
























































































































 // Horizon requires 64KB alignment for address regions






























// nn::os::LightMessageQueue: a bounded, blocking SPSC/MPMC queue. Real hardware backs
// it with the guest-supplied buffer; we keep the actual
// ring storage host-side, keyed by the guest queue pointer, and use a real Condvar so
// Receive blocks instead of busy-spinning (this was causing VibrationThread to spin at
// ~1M calls/sec on an unbound GOT stub before these were wired up).




















































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










 // (addr, rate, channels)

















