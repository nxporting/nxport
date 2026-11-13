// Dear ImGui on the host: a Metal renderer (macOS and iOS: the overlays) and an AppKit view that feeds it input (macOS).

#[cfg(target_os = "macos")]
pub use dear_imgui_rs as imgui;

pub mod renderer;
#[cfg(target_os = "macos")]
pub mod window;
