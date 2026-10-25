// Building ports: shader stages from a game's RomFS, the Metal shader cache, and the apps (macOS .app, iOS .ipa with
// the signed guest code image, the Windows app folder), and guest code lifted to C for hosts without the native runtime
// (lift, compiled with zig cc on Windows).

#[cfg(target_os = "macos")]
mod batch_kernels;
pub mod extract_shaders;
#[cfg(target_os = "macos")]
mod icon;
// The Apple packaging (app bundles, signed Mach-O images, Metal shader caches through xcrun) runs on macOS hosts.
#[cfg(target_os = "macos")]
pub mod ipa;
pub mod lift;
pub mod macho;
pub use nxport::metal::msl_specialize;
#[cfg(target_os = "macos")]
pub mod pack;
#[cfg(target_os = "macos")]
pub mod pipeline_archive;
pub mod ppm2png;
mod sarc;
#[cfg(target_os = "macos")]
pub mod shader_cache_build;
pub mod tiny_kernels;
pub mod translation;
// The Windows app (an app folder with the lifted guest library) and the C compiler it is built with, on Windows hosts.
#[cfg(windows)]
pub mod winapp;
#[cfg(windows)]
pub mod zig;
