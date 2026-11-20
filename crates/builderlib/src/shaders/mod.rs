// Kernels the Metal shader cache build appends (shader_cache_build), on the hosts that build it.
#[cfg(any(target_os = "macos", windows))]
pub(crate) mod arg_kernels;
#[cfg(any(target_os = "macos", windows))]
pub(crate) mod batch_kernels;
pub mod extract_shaders;
#[cfg(target_os = "macos")]
pub mod pipeline_archive;
#[cfg(any(target_os = "macos", windows))]
pub mod shader_cache_build;
pub use nxport_metalshaderlib::tiny_kernels;
pub mod translation;
#[cfg(nx_runtime)]
pub mod vulkan_precompile;
