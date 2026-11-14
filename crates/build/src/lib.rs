// Building ports: shader stages from a game's RomFS, the Metal shader cache, and the apps (macOS .app, iOS .ipa with
// the signed guest code image, the Windows app folder), and guest code lifted to C for hosts without the native runtime
// (lift, compiled with zig cc on Windows).

// Kernels the Metal shader cache build appends (shader_cache_build), on the hosts that build it.
#[cfg(any(target_os = "macos", windows))]
mod arg_kernels;
#[cfg(any(target_os = "macos", windows))]
mod batch_kernels;
/// The app icon from the dump, for every target.
pub mod app_icon;
pub mod extract_shaders;
// The Apple packaging (app bundles, signed Mach-O images, Metal shader caches through xcrun) runs on macOS hosts.
#[cfg(any(target_os = "macos", windows))]
pub mod ipa;
/// Apple bundles zipped with their Unix modes (the .ipa; the macOS app packed on Windows).
#[cfg(any(target_os = "macos", windows))]
mod bundle_zip;
pub mod adhoc_sign;
pub mod lift;
pub mod macho;
pub mod mod_check;
pub use nxport::metal::msl_specialize;
// The macOS app, packed on any host (PARITY #1).
#[cfg(any(target_os = "macos", windows))]
pub mod pack;
pub mod pe_exports;
#[cfg(target_os = "macos")]
pub mod pipeline_archive;
pub mod ppm2png;
mod sarc;
#[cfg(any(target_os = "macos", windows))]
pub mod shader_cache_build;
pub mod tiny_kernels;
pub mod translation;
#[cfg(nx_runtime)]
pub mod vulkan_precompile;
// The engines for iOS and macOS, cross-built on Windows without Apple's tools.
#[cfg(windows)]
pub mod apple_engine;
// The Windows app (an app folder with the lifted guest library) and the C compiler it is built with, on any host.
pub mod winapp;
pub mod zig;
