// Building ports: shader stages from a game's RomFS, the Metal shader cache, and the apps (macOS .app, iOS .ipa with
// the signed guest code image, the Windows app folder), and guest code lifted to C for hosts without the native runtime
// (lift, compiled with zig cc on Windows).

pub mod lift;
pub use nxport::metal::msl_specialize;
pub mod ppm2png;

pub mod game_data;
pub mod packaging;
pub mod shaders;
pub use shaders::extract_shaders;
#[cfg(target_os = "macos")]
pub use shaders::pipeline_archive;
#[cfg(any(target_os = "macos", windows))]
pub use shaders::shader_cache_build;
pub use shaders::tiny_kernels;
pub use shaders::translation;
#[cfg(nx_runtime)]
pub use shaders::vulkan_precompile;
#[cfg(any(target_os = "macos", windows))]
use shaders::arg_kernels;
#[cfg(any(target_os = "macos", windows))]
use shaders::batch_kernels;
pub use packaging::app_icon;
#[cfg(any(target_os = "macos", windows))]
pub use packaging::ipa;
pub use packaging::adhoc_sign;
pub use packaging::macho;
#[cfg(any(target_os = "macos", windows))]
pub use packaging::pack;
pub use packaging::pe_exports;
#[cfg(windows)]
pub use packaging::apple_engine;
pub use packaging::winapp;
pub use packaging::zig;
#[cfg(any(target_os = "macos", windows))]
use packaging::bundle_zip;
pub use game_data::mod_check;
use game_data::sarc;
