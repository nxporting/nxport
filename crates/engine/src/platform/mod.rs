//! Host operating system primitives: memory, faults, threads and the clock.
#[cfg(nx_runtime)]
pub(crate) mod clock;
pub mod os;
