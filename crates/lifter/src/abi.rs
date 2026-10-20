// The guest CPU state as the runtime sees it: runtime/cpu.h's `Cpu`, field for field.

/// One guest thread's registers (runtime/cpu.h).
#[repr(C, align(16))]
pub struct Cpu {
    pub x: [u64; 32],
    pub sp: u64,
    pub pc: u64,
    pub nzcv: u64,
    pub tpidr: u64,
    pub tpidrro: u64,
    pub excl_addr: u64,
    pub excl_val: u64,
    pub thread: *mut core::ffi::c_void,
    pub fpcr: u64,
    pub fpsr: u64,
    pub excl_val_hi: u64,
    pub _pad: [u8; 512 - 344],
    pub v: [u128; 32],
}

/// Generated functions and the dispatcher: `void f(Cpu *)`.
pub type GuestFn = unsafe extern "C" fn(*mut Cpu);

/// The return address host-to-guest calls set (rt_dispatch returns to the host on it).
pub const RETURN_TO_HOST: u64 = 0;

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::offset_of;

    #[test]
    fn layout_matches_cpu_h() {
        assert_eq!(offset_of!(Cpu, sp), 256);
        assert_eq!(offset_of!(Cpu, pc), 264);
        assert_eq!(offset_of!(Cpu, nzcv), 272);
        assert_eq!(offset_of!(Cpu, tpidr), 280);
        assert_eq!(offset_of!(Cpu, tpidrro), 288);
        assert_eq!(offset_of!(Cpu, excl_addr), 296);
        assert_eq!(offset_of!(Cpu, thread), 312);
        assert_eq!(offset_of!(Cpu, fpcr), 320);
        assert_eq!(offset_of!(Cpu, excl_val_hi), 336);
        assert_eq!(offset_of!(Cpu, v), 512);
        assert_eq!(core::mem::size_of::<Cpu>(), 1024);
    }
}
