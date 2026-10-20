// Building ports: shader stages from a game's RomFS, the Metal shader cache, and the apps (macOS .app, iOS .ipa with
// the signed guest code image), and guest code lifted to C for hosts without the native runtime (lift).

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
pub mod ppm2png;
mod sarc;
#[cfg(target_os = "macos")]
pub mod shader_cache_build;
pub mod tiny_kernels;
pub mod translation;
