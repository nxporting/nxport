// The guest process image: executable formats, module loading and relocation, code mods, and write tracking of guest
// memory.

pub mod dirty_track;
pub mod loader;
pub mod mod_patches;
pub mod module_loader;
pub mod nso;
pub mod regs;
pub mod tls;
pub mod sdk_loader;

/// Calls the guest function at `addr` with up to eight integer or pointer arguments, on the calling thread's current
/// stack, and returns its x0 (unspecified for a function that returns nothing). The engine enters guest code through
/// here (module initialisers, constructors, nninitStartup, svc threads) or through horizon::thread_stack::run (guest
/// threads on their own stacks), so a runtime that runs lifted guest code has these two places to implement
/// (docs/windows-port.md).
/// # Safety
/// `addr` is guest code that takes the given arguments.
#[cfg(nx_native)]
pub unsafe fn call(addr: usize, args: &[u64]) -> u64 {
    use std::mem::transmute;
    type F0 = unsafe extern "C" fn() -> u64;
    type F1 = unsafe extern "C" fn(u64) -> u64;
    type F2 = unsafe extern "C" fn(u64, u64) -> u64;
    type F3 = unsafe extern "C" fn(u64, u64, u64) -> u64;
    type F4 = unsafe extern "C" fn(u64, u64, u64, u64) -> u64;
    type F8 = unsafe extern "C" fn(u64, u64, u64, u64, u64, u64, u64, u64) -> u64;
    // Guest code is arm64 code called with AAPCS64: integer arguments in x0-x7, the result in x0. Arguments past the
    // ones the function takes are ignored by it.
    match *args {
        [] => transmute::<usize, F0>(addr)(),
        [a] => transmute::<usize, F1>(addr)(a),
        [a, b] => transmute::<usize, F2>(addr)(a, b),
        [a, b, c] => transmute::<usize, F3>(addr)(a, b, c),
        [a, b, c, d] => transmute::<usize, F4>(addr)(a, b, c, d),
        _ => {
            assert!(args.len() <= 8, "guest::call: {} arguments", args.len());
            let mut r = [0u64; 8];
            r[..args.len()].copy_from_slice(args);
            transmute::<usize, F8>(addr)(r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7])
        }
    }
}

/// `call` on a lifted host: through the calling thread's lifted CPU state, which does not exist yet
/// (docs/windows-port.md, step 5).
#[cfg(not(nx_native))]
pub unsafe fn call(addr: usize, _args: &[u64]) -> u64 {
    unimplemented!("guest call {addr:#x}: the lifted runtime has no per-thread state yet");
}
