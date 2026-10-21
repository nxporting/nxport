// Stacks for guest code.
//
// On Horizon a thread runs on the stack buffer the game passed to CreateThread, and kernel calls do not use it. Games
// rely on both. Super Mario Odyssey's sead decides whether a string must be copied to the heap by checking that it lies
// in [ThreadType stack, + the stack size the game asked for), and keeps pointers to strings it judged not to be on the
// stack. So guest code runs on the game's own buffer, and HLE calls switch to the host thread's stack (the HLE entry
// stub, guest/loader.rs write_hle_entry_stub): our frames are far deeper than the SDK's and do not fit the game's
// buffers, which Horizon sizes for its thin syscall layer.

use std::cell::Cell;

thread_local! {
    static BOUNDS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

/// The guest stack the calling thread runs guest code on (lo, hi), if any.
pub fn current_bounds() -> Option<(usize, usize)> {
    BOUNDS.try_with(|b| b.get()).ok().filter(|b| b.1 != 0)
}

// call_on_stack(arg, entry, sp, hle_slot): entry(arg) with sp as its stack. The caller's stack below this frame becomes
// the stack HLE calls use (*hle_slot) until entry returns.
std::arch::global_asm!(
    ".p2align 2",
    ".globl _nxport_call_on_stack",
    "_nxport_call_on_stack:",
    "stp x29, x30, [sp, #-16]!",
    "mov x29, sp",
    "str x29, [x3]",
    "mov sp, x2",
    "stp x3, x29, [sp, #-16]!",
    "blr x1",
    "ldp x3, x9, [sp], #16",
    "str xzr, [x3]",
    "mov sp, x9",
    "ldp x29, x30, [sp], #16",
    "ret",
);

extern "C" {
    fn nxport_call_on_stack(arg: usize, entry: usize, sp: usize, hle_slot: *mut usize);
}

/// A stack of `size` bytes for guest code that has no buffer from the game (the main thread's, which Horizon's
/// kernel allocates). It lives as long as the process. Returns (lo, hi).
pub fn allocate(size: usize) -> (usize, usize) {
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };
    let size = size.next_multiple_of(page);
    let base = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            page + size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        )
    };
    assert!(base != libc::MAP_FAILED, "guest stack: mmap of {size:#x} bytes failed");
    let base = base as usize;
    unsafe { libc::mprotect(base as *mut _, page, libc::PROT_NONE) };
    (base + page, base + page + size)
}

/// Runs guest code `entry(arg)` on the stack [lo, hi).
pub unsafe fn run(lo: usize, hi: usize, entry: usize, arg: usize) {
    BOUNDS.with(|b| b.set((lo, hi)));
    let tsd: *mut u8;
    std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tsd);
    super::cores::run_guest(|| {
        nxport_call_on_stack(arg, entry, hi & !15, tsd.add(crate::cpu::tls::hle_stack_offset()) as *mut usize)
    });
    BOUNDS.with(|b| b.set((0, 0)));
}
