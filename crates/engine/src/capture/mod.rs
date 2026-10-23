// Frame captures for GPU backend conformance tests: a frame of a game recorded on one backend (format.rs, record.rs)
// and run again on any backend, compared render pass by render pass (replay.rs).

pub(crate) mod format;
pub(crate) mod image;
#[cfg(nx_native)]
pub(crate) mod record;
pub mod replay;
