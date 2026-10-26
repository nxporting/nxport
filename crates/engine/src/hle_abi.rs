// The calling convention between guest code and the engine's HLE functions.
//
// HLE functions are ordinary `extern "C"` Rust functions. On arm64 hosts guest code calls them directly through an
// arm64 trampoline (GuestMemorySpace::make_hle_trampoline): guest and host share AAPCS64, so x0-x7 / v0-v7 already
// are the arguments. A host with another calling convention (x86-64, running lifted guest code: docs/windows-port.md)
// has the guest's registers in a `Cpu` and needs each function's signature to call it. `Hle` keeps both: the
// function's address, and an adapter generated from its type that reads the arguments out of a `Cpu` in AAPCS64 order
// and writes the result back.
//
// Code that hands out an HLE function's address (to a trampoline, a symbol table, a vtable) takes it through `hle`,
// which records the function's adapter under its address; the parameter count is written as placeholders:
//     hle(hle_nn_sm_GetServiceHandle as unsafe extern "C" fn(_, _, _) -> _)
// A runtime that cannot call the address directly finds the adapter with `adapter`. Native trampolines check that one
// exists (GuestMemorySpace::make_hle_trampoline), so a native run shows any binding that would be missing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use nxport_lifter::abi::Cpu;

/// Guest arguments in AAPCS64 order: integers and pointers from x0-x7, floats from v0-v7, the rest from the guest
/// stack (8-byte slots).
pub struct Args<'a> {
    cpu: &'a Cpu,
    int: usize,
    fp: usize,
    stack: usize,
}

impl<'a> Args<'a> {
    pub fn new(cpu: &'a Cpu) -> Self {
        Args { cpu, int: 0, fp: 0, stack: 0 }
    }

    fn stack_slot(&mut self) -> u64 {
        // Guest addresses are host addresses.
        let v = unsafe { *((self.cpu.sp as usize + 8 * self.stack) as *const u64) };
        self.stack += 1;
        v
    }

    /// The next integer-class argument.
    pub fn int(&mut self) -> u64 {
        if self.int < 8 {
            self.int += 1;
            self.cpu.x[self.int - 1]
        } else {
            self.stack_slot()
        }
    }

    /// The next floating-point argument (the low bits of the V register).
    pub fn fp(&mut self) -> u128 {
        if self.fp < 8 {
            self.fp += 1;
            self.cpu.v[self.fp - 1]
        } else {
            self.stack_slot() as u128
        }
    }
}

/// A type an HLE function can take as an argument.
pub trait Arg: Sized {
    fn take(a: &mut Args) -> Self;
}

/// A type an HLE function can return.
pub trait Ret {
    fn put(self, cpu: &mut Cpu);
}

macro_rules! int_abi {
    ($($t:ty),*) => {$(
        impl Arg for $t {
            fn take(a: &mut Args) -> Self {
                a.int() as $t
            }
        }
        impl Ret for $t {
            fn put(self, cpu: &mut Cpu) {
                cpu.x[0] = self as u64;
            }
        }
    )*};
}
int_abi!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

impl Arg for bool {
    fn take(a: &mut Args) -> Self {
        a.int() as u8 != 0
    }
}
impl Ret for bool {
    fn put(self, cpu: &mut Cpu) {
        cpu.x[0] = self as u64;
    }
}

impl<T> Arg for *const T {
    fn take(a: &mut Args) -> Self {
        a.int() as usize as *const T
    }
}
impl<T> Ret for *const T {
    fn put(self, cpu: &mut Cpu) {
        cpu.x[0] = self as usize as u64;
    }
}
impl<T> Arg for *mut T {
    fn take(a: &mut Args) -> Self {
        a.int() as usize as *mut T
    }
}
impl<T> Ret for *mut T {
    fn put(self, cpu: &mut Cpu) {
        cpu.x[0] = self as usize as u64;
    }
}

impl Arg for f32 {
    fn take(a: &mut Args) -> Self {
        f32::from_bits(a.fp() as u32)
    }
}
impl Ret for f32 {
    fn put(self, cpu: &mut Cpu) {
        cpu.v[0] = self.to_bits() as u128;
    }
}
impl Arg for f64 {
    fn take(a: &mut Args) -> Self {
        f64::from_bits(a.fp() as u64)
    }
}
impl Ret for f64 {
    fn put(self, cpu: &mut Cpu) {
        cpu.v[0] = self.to_bits() as u128;
    }
}

impl Ret for () {
    fn put(self, _cpu: &mut Cpu) {}
}

// Guest function pointers (callbacks) are guest addresses: the HLE function gets the address, as on arm64 hosts where
// it calls it directly. A runtime running lifted code calls such pointers through guest::call (docs/windows-port.md).
macro_rules! guest_fn_arg {
    ($($a:ident),*) => {
        impl<R, $($a),*> Arg for unsafe extern "C" fn($($a),*) -> R {
            fn take(a: &mut Args) -> Self {
                let p = a.int() as usize;
                assert!(p != 0, "null guest function pointer");
                unsafe { std::mem::transmute::<usize, Self>(p) }
            }
        }
        impl<R, $($a),*> Arg for Option<unsafe extern "C" fn($($a),*) -> R> {
            fn take(a: &mut Args) -> Self {
                unsafe { std::mem::transmute::<usize, Self>(a.int() as usize) }
            }
        }
    };
}
guest_fn_arg!();
guest_fn_arg!(A0);
guest_fn_arg!(A0, A1);
guest_fn_arg!(A0, A1, A2);
guest_fn_arg!(A0, A1, A2, A3);

/// A function pointer type HLE functions can have: `unsafe extern "C" fn(A0, .., An) -> R`, n <= 10.
pub trait HleFn: Copy + 'static {
    fn addr(self) -> *const ();
    /// # Safety
    /// `p` is the address of a function of this type.
    unsafe fn from_addr(p: *const ()) -> Self;
    /// Calls the function with the guest's arguments and stores its result, then returns to the guest's caller.
    /// # Safety
    /// As calling the function itself.
    unsafe fn call(self, cpu: &mut Cpu);
}

macro_rules! hle_fn {
    ($($a:ident),*) => {
        // Functions that do not return (aborts): nothing to write back.
        impl<$($a: Arg + 'static),*> HleFn for unsafe extern "C" fn($($a),*) -> ! {
            fn addr(self) -> *const () {
                self as *const ()
            }
            unsafe fn from_addr(p: *const ()) -> Self {
                std::mem::transmute::<*const (), Self>(p)
            }
            #[allow(non_snake_case)]
            unsafe fn call(self, cpu: &mut Cpu) {
                let ($($a,)*) = {
                    let mut _args = Args::new(cpu);
                    ($($a::take(&mut _args),)*)
                };
                self($($a),*)
            }
        }
        impl<R: Ret + 'static, $($a: Arg + 'static),*> HleFn for unsafe extern "C" fn($($a),*) -> R {
            fn addr(self) -> *const () {
                self as *const ()
            }
            unsafe fn from_addr(p: *const ()) -> Self {
                std::mem::transmute::<*const (), Self>(p)
            }
            #[allow(non_snake_case)]
            unsafe fn call(self, cpu: &mut Cpu) {
                let ($($a,)*) = {
                    let mut _args = Args::new(cpu);
                    ($($a::take(&mut _args),)*)
                };
                self($($a),*).put(cpu);
                cpu.pc = cpu.x[30];
            }
        }
    };
}
hle_fn!();
hle_fn!(A0);
hle_fn!(A0, A1);
hle_fn!(A0, A1, A2);
hle_fn!(A0, A1, A2, A3);
hle_fn!(A0, A1, A2, A3, A4);
hle_fn!(A0, A1, A2, A3, A4, A5);
hle_fn!(A0, A1, A2, A3, A4, A5, A6);
hle_fn!(A0, A1, A2, A3, A4, A5, A6, A7);
hle_fn!(A0, A1, A2, A3, A4, A5, A6, A7, A8);
hle_fn!(A0, A1, A2, A3, A4, A5, A6, A7, A8, A9);

/// An HLE function: its address (what native trampolines call) and its adapter (what a lifted runtime calls).
#[derive(Clone, Copy, Debug)]
pub struct Hle {
    addr: *const (),
    adapter: unsafe fn(*const (), &mut Cpu),
}

// Function addresses and adapters are plain code pointers.
unsafe impl Send for Hle {}
unsafe impl Sync for Hle {}

unsafe fn call_through<F: HleFn>(p: *const (), cpu: &mut Cpu) {
    F::from_addr(p).call(cpu)
}

impl Hle {
    pub fn of<F: HleFn>(f: F) -> Hle {
        Hle { addr: f.addr(), adapter: call_through::<F> }
    }

    /// `of` for static tables: `addr` is `f`'s address, taken by the caller (`HleFn::addr` cannot run at compile
    /// time). `func!` in horizon::sdk passes both from the same path.
    pub const fn of_const<F: HleFn>(_f: F, addr: *const ()) -> Hle {
        Hle { addr, adapter: call_through::<F> }
    }

    /// The function's address.
    pub fn addr(self) -> *const () {
        self.addr
    }

    /// Calls the function with the arguments in `cpu` and stores its result there, setting `cpu.pc` to the return
    /// address (x30) as a `ret` would.
    /// # Safety
    /// As calling the function itself; guest memory the arguments point to must be mapped.
    pub unsafe fn call(self, cpu: &mut Cpu) {
        (self.adapter)(self.addr, cpu)
    }
}

impl<F: HleFn> From<F> for Hle {
    fn from(f: F) -> Hle {
        Hle::of(f)
    }
}

fn registry() -> &'static Mutex<HashMap<usize, Hle>> {
    static R: OnceLock<Mutex<HashMap<usize, Hle>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// The address of HLE function `f`, its adapter recorded for `adapter`.
pub fn hle<F: HleFn>(f: F) -> *const () {
    record(Hle::of(f))
}

/// The address of `h`, its adapter recorded for `adapter`.
pub fn record(h: Hle) -> *const () {
    registry().lock().unwrap().insert(h.addr as usize, h);
    h.addr
}

/// The HLE function at `addr`, if one was handed out through `hle`.
pub fn adapter(addr: *const ()) -> Option<Hle> {
    registry().lock().unwrap().get(&(addr as usize)).copied()
}

fn entries() -> &'static Mutex<HashMap<u64, usize>> {
    static E: OnceLock<Mutex<HashMap<u64, usize>>> = OnceLock::new();
    E.get_or_init(Default::default)
}

/// Records that guest address `entry` (an HLE trampoline) enters HLE function `f`.
pub fn bind_entry(entry: u64, f: *const ()) {
    entries().lock().unwrap().insert(entry, f as usize);
}

/// The HLE function guest code reaches by branching to `addr`: through a trampoline (`bind_entry`), or the function's
/// own address.
pub fn entry(addr: u64) -> Option<Hle> {
    let f = entries().lock().unwrap().get(&addr).copied().unwrap_or(addr as usize);
    adapter(f as *const ())
}

/// `hle!(f(_, _))`: the address of HLE function `f` (two parameters), its adapter recorded; the same as
/// `hle(f as unsafe extern "C" fn(_, _) -> _)`.
#[macro_export]
macro_rules! hle {
    ($($f:ident)::+ ($($a:tt),*)) => {
        $crate::hle_abi::hle($($f)::+ as unsafe extern "C" fn($($a),*) -> _)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu() -> Box<Cpu> {
        // Cpu is plain data: all zeros is a valid state.
        unsafe { Box::new(std::mem::zeroed()) }
    }

    unsafe extern "C" fn add3(a: u32, b: i64, c: u8) -> i64 {
        a as i64 + b + c as i64
    }

    unsafe extern "C" fn mixed(a: f32, n: u64, b: f64) -> f64 {
        a as f64 * n as f64 + b
    }

    unsafe extern "C" fn many(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64, i: u64, j: u64) -> u64 {
        a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + 10 * j
    }

    static mut SEEN: usize = 0;
    unsafe extern "C" fn store(p: *const u8) {
        SEEN = p as usize;
    }

    unsafe extern "C" fn half(x: f32) -> f32 {
        x / 2.0
    }

    extern "C" fn is_zero(x: u32) -> bool {
        x == 0
    }

    #[test]
    fn integer_arguments_and_return() {
        let mut c = cpu();
        c.x[0] = 0xffff_ffff_0000_0007; // only w0 is the u32
        c.x[1] = (-10i64) as u64;
        c.x[2] = 0x1ff; // only the low byte is the u8
        c.x[30] = 0x7000_1234;
        unsafe { Hle::of(add3 as unsafe extern "C" fn(_, _, _) -> _).call(&mut c) };
        assert_eq!(c.x[0] as i64, 7 - 10 + 0xff);
        assert_eq!(c.pc, 0x7000_1234);
    }

    #[test]
    fn float_and_integer_registers_count_separately() {
        let mut c = cpu();
        c.v[0] = 1.5f32.to_bits() as u128;
        c.x[0] = 4;
        c.v[1] = 0.25f64.to_bits() as u128;
        unsafe { Hle::of(mixed as unsafe extern "C" fn(_, _, _) -> _).call(&mut c) };
        assert_eq!(f64::from_bits(c.v[0] as u64), 6.25);
        c.v[0] = 0xdead_beef_0000_0000_0000_0000 | 6.25f32.to_bits() as u128;
        unsafe { Hle::of(half as unsafe extern "C" fn(_) -> _).call(&mut c) };
        assert_eq!(c.v[0], 3.125f32.to_bits() as u128); // the upper bits of v0 are cleared
    }

    #[test]
    fn arguments_past_x7_come_from_the_stack() {
        let mut c = cpu();
        for i in 0..8 {
            c.x[i] = 1;
        }
        let stack = [100u64, 1000];
        c.sp = stack.as_ptr() as u64;
        unsafe { Hle::of(many as unsafe extern "C" fn(_, _, _, _, _, _, _, _, _, _) -> _).call(&mut c) };
        assert_eq!(c.x[0], 36 + 9 * 100 + 10 * 1000);
    }

    #[test]
    fn pointers_unit_and_safe_functions() {
        let mut c = cpu();
        c.x[0] = 0x1234_5678;
        unsafe { Hle::of(store as unsafe extern "C" fn(_)).call(&mut c) };
        assert_eq!(unsafe { SEEN }, 0x1234_5678);
        assert_eq!(c.x[0], 0x1234_5678); // a function returning nothing leaves x0 alone
        c.x[0] = 0xffff_ffff_0000_0000; // w0 = 0
        let h = Hle::of(is_zero as unsafe extern "C" fn(_) -> _);
        assert_eq!(h.addr(), is_zero as *const ());
        unsafe { h.call(&mut c) };
        assert_eq!(c.x[0], 1);
    }

    #[test]
    fn adapters_built_at_compile_time() {
        static ADD3: Hle = Hle::of_const(add3 as unsafe extern "C" fn(_, _, _) -> _, add3 as *const ());
        assert_eq!(record(ADD3), add3 as *const ());
        let mut c = cpu();
        c.x[0] = 1;
        c.x[1] = 2;
        c.x[2] = 3;
        unsafe { adapter(add3 as *const ()).expect("recorded").call(&mut c) };
        assert_eq!(c.x[0], 6);
    }

    #[test]
    fn registry_finds_adapters_by_address() {
        let p = hle(add3 as unsafe extern "C" fn(_, _, _) -> _);
        assert_eq!(p, add3 as *const ());
        let mut c = cpu();
        c.x[0] = 1;
        c.x[1] = 2;
        c.x[2] = 3;
        unsafe { adapter(p).expect("registered").call(&mut c) };
        assert_eq!(c.x[0], 6);
    }

    #[test]
    fn entries_lead_to_their_functions() {
        let trampoline = 0x7000_0123_4560;
        bind_entry(trampoline, hle(add3 as unsafe extern "C" fn(_, _, _) -> _));
        assert_eq!(entry(trampoline).map(Hle::addr), Some(add3 as *const ()));
        assert_eq!(entry(add3 as *const () as u64).map(Hle::addr), Some(add3 as *const ())); // its own address too
        assert!(entry(0x7000_0123_4570).is_none());
    }
}
