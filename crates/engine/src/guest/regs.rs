// The guest's registers as HLE code reads them (docs/windows-port.md, "The guest boundary"): the caller's frame
// pointer and return address (logs, backtraces, a few call-site checks), x16 / x17 (what PLT stubs and the HLE
// trampolines leave there), the thread's thread-local region (TPIDRRO_EL0) and the counter.
//
// Natively these are the host's own registers, which still hold the guest's values in an HLE function, so
// `guest_reg!` reads them in place (a function would read its own frame). Lifted code keeps them in the Cpu of the
// host call the thread is in (crate::lifted::current); outside one they read as 0.

/// `guest_reg!(x29)`: guest register x16, x17, x29 or x30 as usize.
#[cfg(nx_native)]
#[macro_export]
macro_rules! guest_reg {
    ($r:ident) => {{
        let v: usize;
        #[allow(unused_unsafe)]
        unsafe {
            core::arch::asm!(concat!("mov {}, ", stringify!($r)), out(reg) v, options(nomem, nostack, preserves_flags))
        };
        v
    }};
}

#[cfg(not(nx_native))]
#[macro_export]
macro_rules! guest_reg {
    (x16) => {
        $crate::guest::regs::lifted_x(16)
    };
    (x17) => {
        $crate::guest::regs::lifted_x(17)
    };
    (x29) => {
        $crate::guest::regs::lifted_x(29)
    };
    (x30) => {
        $crate::guest::regs::lifted_x(30)
    };
}

#[cfg(not(nx_native))]
pub fn lifted_x(n: usize) -> usize {
    let c = crate::lifted::current();
    if c.is_null() {
        0
    } else {
        unsafe { (*c).x[n] as usize }
    }
}

/// The counter guest code reads (CNTPCT_EL0) and its frequency (CNTFRQ_EL0): the hardware's natively; lifted, the host
/// clock (crate::clock), which lifted code's counter reads must return too (lifted::Handlers::ticks).
pub fn counter() -> (u64, u64) {
    #[cfg(nx_native)]
    {
        let (t, f): (u64, u64);
        unsafe {
            core::arch::asm!("mrs {}, cntpct_el0", out(reg) t, options(nomem, nostack, preserves_flags));
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) f, options(nomem, nostack, preserves_flags));
        }
        (t, f)
    }
    #[cfg(not(nx_native))]
    {
        (crate::clock::ticks(), crate::clock::frequency())
    }
}

/// TPIDRRO_EL0: the guest thread's Horizon thread-local region (IPC buffer, the thread pointer at +0x1f8). Natively
/// that is the host thread's Darwin thread-specific data page, shared with Darwin's own slots and the engine's
/// (cpu::tls); lifted, the guest thread's own region.
pub fn tpidrro() -> usize {
    #[cfg(nx_native)]
    {
        let v: usize;
        unsafe { core::arch::asm!("mrs {}, tpidrro_el0", out(reg) v, options(nomem, nostack, preserves_flags)) };
        v
    }
    #[cfg(not(nx_native))]
    {
        let c = crate::lifted::current();
        if c.is_null() {
            0
        } else {
            unsafe { (*c).tpidrro as usize }
        }
    }
}
