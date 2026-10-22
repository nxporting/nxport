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

// The guest TLS blocks themselves are guest::tls (they exist on every CPU path).
pub use crate::guest::tls::{is_guest_thread, mark_guest_thread, CURRENT_GUEST_TLS_BLOCK, FAKE_TLR_SLOT};

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

/// Installs the calling OS thread's guest TLS block (guest::tls::thread_block) as its tpidr_el0 and in its TSD slot.
/// Must be called on each thread (main thread included) before any guest code runs on it.
pub unsafe fn install_guest_tls() -> usize {
    let block = crate::guest::tls::thread_block();

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

