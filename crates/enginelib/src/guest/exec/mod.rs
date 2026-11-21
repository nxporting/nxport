// Running guest code. Natively on arm64: trap handling, code rewriting (TLS register, x18, HLE trampolines) and the
// signed ahead-of-time code images iOS needs; only `aot`'s registry of guest code regions and its image building are
// needed where guest code is lifted (the image is what the lifter lifts). Lifted: the guest library (`lifted`), the
// calling convention between it and the engine (`hle_abi`) and code patched after lifting (`live_patch`).

#[cfg(nx_runtime)]
pub mod aot;
#[cfg(all(nx_runtime, nx_native))]
pub mod crash_handler;
#[cfg(all(nx_runtime, nx_native))]
pub mod load_emulate;
#[cfg(all(nx_runtime, any(nx_native, windows)))]
pub mod exclusive_rewrite;
#[cfg(all(nx_runtime, any(nx_native, windows)))]
pub mod tls;
#[cfg(all(nx_runtime, any(nx_native, windows)))]
pub mod tpidr_rewrite;
#[cfg(all(nx_runtime, any(nx_native, windows)))]
pub mod trampoline;
pub mod hle_abi;
pub mod lifted;
#[cfg(all(nx_runtime, windows))]
pub mod live_patch;
