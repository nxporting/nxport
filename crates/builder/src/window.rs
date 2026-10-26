// Window and Dear ImGui runner: AppKit on macOS (delegates to nxport::ui::window), SDL3 on Windows (window_sdl.rs).

#[cfg(target_os = "macos")]
pub use nxport::ui::imgui;

#[cfg(target_os = "macos")]
pub use nxport::ui::window::{snapshot, Frame, PathKind, request_path, take_path};

#[cfg(target_os = "macos")]
pub fn run(title: &str, width: f64, height: f64, frame: Frame) -> Result<(), Box<dyn std::error::Error>> {
    nxport::ui::window::run(title, width, height, frame);
    Ok(())
}

#[cfg(windows)]
pub use dear_imgui_rs as imgui;

#[cfg(windows)]
pub use crate::window_sdl::{request_path, run, snapshot, take_path, PathKind};
