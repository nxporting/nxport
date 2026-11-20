// The Metal shader cache's MSL tooling, shared by the engine (stages translated at run time) and the builder (the
// cache it builds on any host): the specializer that post-processes the translator's MSL, the compute versions of
// stages for tiny passes, and the cache's source pack.

pub mod msl_specialize;
pub mod source_pack;
pub mod tiny_kernels;
