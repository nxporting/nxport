// Native arm64 execution of guest code: trap handling, code rewriting (TLS register, x18, HLE trampolines) and the
// signed ahead-of-time code images iOS needs. Only `aot`'s registry of guest code regions and its image building are
// needed where guest code is lifted (the image is what the lifter lifts).

pub mod aot;
#[cfg(nx_native)]
pub mod crash_handler;
#[cfg(nx_native)]
pub mod exclusive_rewrite;
#[cfg(nx_native)]
pub mod tls;
#[cfg(nx_native)]
pub mod tpidr_rewrite;
#[cfg(nx_native)]
pub mod trampoline;
