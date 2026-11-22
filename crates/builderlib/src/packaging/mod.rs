/// The app icon from the dump, for every target.
pub mod app_icon;
// The Apple packaging (app bundles, signed Mach-O images, Metal shader caches through xcrun) runs on macOS hosts.
#[cfg(any(target_os = "macos", windows))]
pub mod ipa;
/// Apple bundles zipped with their Unix modes (the .ipa; the macOS app packed on Windows).
#[cfg(any(target_os = "macos", windows))]
pub(crate) mod bundle_zip;
pub mod adhoc_sign;
pub mod macho;
// The macOS app, packed on any host.
#[cfg(any(target_os = "macos", windows))]
pub mod pack;
pub mod pe_exports;
// The engines for iOS and macOS, cross-built on Windows without Apple's tools.
#[cfg(windows)]
pub mod apple_engine;
// The Windows app (an app folder with the lifted guest library) and the C compiler it is built with, on any host.
pub mod winapp;
pub mod zig;
