// The Metal backend's shader tooling without the backend, for builds that do not run Metal (lib.rs).

#[path = "msl_specialize.rs"]
pub mod msl_specialize;
#[path = "source_pack.rs"]
pub mod source_pack;
