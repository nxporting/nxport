// Per-guest-thread TPIDR_EL0 (ELF/musl thread-pointer) setup.
//
// The compiled TotK binary reads `tpidr_el0` directly (812 call sites) to reach
// both compiler-emitted C++ `thread_local` storage and per-thread OS runtime
// state, at fixed positive byte offsets baked into the code as immediates
// (e.g. `mrs x21, tpidr_el0` / `ldr x9, [x21, #0x10]`, `add x0, x21, #0x980`).
//
// On Darwin (Apple Silicon), the XNU kernel overwrites `tpidr_el0` with the kernel's
// internal thread ID on every syscall and preemptive context switch.
// We preserve each thread's guest TLS block in `CURRENT_GUEST_TLS_BLOCK` and in
// `tpidrro_el0 + guest_tls_offset()` (a pthread key TSD slot, which Darwin preserves across context switches).
// We re-prime `tpidr_el0` on HLE entries/returns, and the signal handler transparently
// recovers any fault caused by a kernel thread ID in a base register.

use std::cell::Cell;

const TLS_BLOCK_SIZE: usize = 0x4000; // empirically observed max offset 0x1000, generous margin
/// Offset in the guest TLS block of the pointer to the thread's fake Horizon thread-local region.
pub const FAKE_TLR_SLOT: usize = 0x3ff8;

/// The current thread's fake Horizon thread-local region (IPC buffer), or 0 before install_guest_tls.
pub fn current_fake_tlr() -> usize {
    let block = CURRENT_GUEST_TLS_BLOCK.with(|c| c.get());
    if block == 0 {
        0
    } else {
        unsafe { *((block + FAKE_TLR_SLOT) as *const usize) }
    }
}

thread_local! {
    pub static CURRENT_GUEST_TLS_BLOCK: Cell<usize> = const { Cell::new(0) };
}

/// Byte offset from tpidrro_el0 (Darwin's TSD array) of the slot holding this thread's guest TLS block, read by every
/// rewritten `mrs Xt, tpidr_el0`. It is a pthread key created for the purpose: the fixed slot 60 used before is
/// Foundation's TSD key (__PTK_FRAMEWORK_FOUNDATION_KEY0, libpthread tsd_private.h), so Foundation code on a guest
/// thread could replace the guest's thread pointer (crashes reading [thread pointer + 0x10] with a pointer of 3).
pub fn guest_tls_offset() -> usize {
    static OFF: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| tsd_key_offset("guest TLS", GUEST_TLS_KEY))
}

/// Fixed TSD keys: their offsets are baked into patched guest code, which must be the same on every launch (AOT
/// images, cpu/aot.rs). pthread_key_create hands out the lowest free key, so keys are created until these come up.
const GUEST_TLS_KEY: libc::pthread_key_t = 400;
const ERRNO_SAVE_KEY: libc::pthread_key_t = 401;
const X18_KEY: libc::pthread_key_t = 402;
const HLE_STACK_KEY: libc::pthread_key_t = 403;

/// Byte offset (from tpidrro_el0) of the slot holding the guest's x18 when x18 uses are rewritten (cpu/trampoline.rs).
pub fn x18_slot_offset() -> usize {
    static OFF: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| tsd_key_offset("guest x18", X18_KEY))
}

/// The pthread key of the x18 slot (pthread_setspecific).
pub fn x18_key() -> libc::pthread_key_t {
    x18_slot_offset();
    X18_KEY
}

/// Byte offset (from tpidrro_el0) of the slot holding the stack HLE calls switch to (horizon/thread_stack.rs), 0 when
/// they stay on the caller's stack.
pub fn hle_stack_offset() -> usize {
    static OFF: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| tsd_key_offset("HLE stack", HLE_STACK_KEY))
}

/// Byte offset (from tpidrro_el0) of the slot where horizon::hle::setup_horizon_tpidrro saves Darwin's errno pointer.
pub fn errno_save_offset() -> usize {
    static OFF: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| tsd_key_offset("errno save", ERRNO_SAVE_KEY))
}

/// A new pthread key's TSD slot as a byte offset; on Darwin a key indexes the TSD array tpidrro_el0 points to. Must
/// fit an `ldr Xt, [Xn, #imm]` (offset <= 32760).
fn tsd_key_offset(what: &str, want: libc::pthread_key_t) -> usize {
    let mut key: libc::pthread_key_t = 0;
    let mut r;
    loop {
        r = unsafe { libc::pthread_key_create(&mut key, None) };
        if r != 0 || key >= want {
            break;
        }
    }
    assert!(
        r == 0 && key == want,
        "pthread key for the {} slot: wanted {}, got {} (error {})",
        what,
        want,
        key,
        r
    );
    let off = key as usize * 8;
    assert!(off <= 32760);
    println!(
        "[+] TLS: {} slot = TSD key {} (tpidrro_el0 + {:#x})",
        what, key, off
    );
    off
}

/// The main module's TLS initialization image (.tdata) and its offset from the thread pointer (AArch64 variant I: the
/// 16-byte TCB, rounded up to the segment's alignment), set by __nnmusl_init_dso (horizon/hle.rs).
static TLS_TEMPLATE: std::sync::Mutex<(Vec<u8>, usize)> = std::sync::Mutex::new((Vec::new(), 16));

pub fn set_module_tls_template(image: Vec<u8>, align: usize) {
    let off = (16 + align.max(1) - 1) / align.max(1) * align.max(1);
    let mut t = TLS_TEMPLATE.lock().unwrap();
    *t = (image, off);
    // Blocks made before the module registered (the loader thread's) get it too.
    let block = CURRENT_GUEST_TLS_BLOCK.with(|c| c.get());
    if block != 0 {
        unsafe { apply_template(block, &t) };
    }
}

unsafe fn apply_template(block: usize, t: &(Vec<u8>, usize)) {
    if !t.0.is_empty() && t.1 + t.0.len() <= 0x1f0 {
        std::ptr::copy_nonoverlapping(t.0.as_ptr(), (block + t.1) as *mut u8, t.0.len());
    }
}

pub fn get_current_guest_tls_block() -> usize {
    let mut block = CURRENT_GUEST_TLS_BLOCK.with(|c| c.get());
    if block == 0 {
        #[cfg(target_arch = "aarch64")]
        {
            let tp: *mut u8;
            unsafe {
                std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tp);
            }
            if (tp as usize) >= 0x10000 {
                block = unsafe { *(tp.add(guest_tls_offset()) as *const usize) };
            }
        }
    }
    if block == 0 {
        block = unsafe { install_guest_tls() };
    }
    block
}

/// Allocate a zeroed thread-local block for the calling OS thread and install
/// it as that thread's tpidr_el0. Must be called on each thread (main thread
/// included) before any guest code runs on it. Leaked intentionally: lifetime
/// matches the OS thread's own lifetime, which for this engine is the process.
pub unsafe fn install_guest_tls() -> usize {
    let mut block = CURRENT_GUEST_TLS_BLOCK.with(|c| c.get());
    if block == 0 {
        let layout = std::alloc::Layout::from_size_align(TLS_BLOCK_SIZE, 0x1000).unwrap();
        let ptr = std::alloc::alloc_zeroed(layout);
        *(ptr.add(0x1f0) as *mut usize) = ptr as usize;
        // A Horizon thread-local region (0x200 bytes, IPC message buffer at +0) for modules that read tpidrro_el0
        // themselves (exlaunch mods; see guest/module_loader.rs): on macOS tpidrro_el0 is Darwin's TSD, which must not be
        // written. Those reads are rewritten to load this pointer.
        let tlr =
            std::alloc::alloc_zeroed(std::alloc::Layout::from_size_align(0x200, 0x200).unwrap());
        *(ptr.add(FAKE_TLR_SLOT) as *mut usize) = tlr as usize;
        apply_template(ptr as usize, &TLS_TEMPLATE.lock().unwrap());
        block = ptr as usize;
        CURRENT_GUEST_TLS_BLOCK.with(|c| c.set(block));
    }

    #[cfg(target_arch = "aarch64")]
    {
        std::arch::asm!("msr tpidr_el0, {0}", in(reg) block);
        let tp: *mut u8;
        std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tp);
        if (tp as usize) >= 0x10000 {
            *(tp.add(guest_tls_offset()) as *mut usize) = block;
        }
    }
    block
}

thread_local! {
    static GUEST_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Called first on every thread that runs guest code (guest-main and the threads the guest creates).
pub fn mark_guest_thread() {
    GUEST_THREAD.with(|g| g.set(true));
}

/// Whether this thread runs guest code. Signal-safe.
pub fn is_guest_thread() -> bool {
    GUEST_THREAD.with(|g| g.get())
}
