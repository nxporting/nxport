// Native arm64 execution of guest code: trap handling, code rewriting (TLS register, x18, HLE trampolines) and the
// signed ahead-of-time code images iOS needs.

pub mod aot;
pub mod crash_handler;
pub mod tls;
pub mod tpidr_rewrite;
pub mod trampoline;
