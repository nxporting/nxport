// The host platform: window or view, input devices, audio output, haptics.

pub mod audio;
#[cfg(target_os = "macos")]
pub mod controls_ui;
#[cfg(target_os = "macos")]
pub mod display;
#[cfg(target_os = "ios")]
#[path = "display_ios.rs"]
pub mod display;
pub mod input;
#[cfg(target_os = "macos")]
pub mod menu;
#[cfg(target_os = "ios")]
pub mod ios_env;
pub mod rumble;
pub mod thread_stats;
// Dear ImGui: its Metal renderer on both (the overlays), its AppKit window on macOS.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub mod ui;
pub mod overlay;
pub mod live;
