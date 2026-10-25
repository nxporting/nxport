// The host platform where guest code is lifted (Windows, docs/windows-port.md): SDL3 gives it a window, the keyboard,
// game controllers and rumble (host/display_sdl.rs, host/rumble_sdl.rs); input goes through the bindings the native
// hosts use (host/input.rs). Audio is the native hosts' module, whose output device is SDL3's here. --headless runs
// without the window, as natively. The functions the runtime calls, with the native host modules' signatures.

#[path = "audio.rs"]
pub mod audio;

#[path = "display_sdl.rs"]
pub mod display;

#[path = "input.rs"]
pub mod input;

#[path = "rumble_sdl.rs"]
pub mod rumble;

pub mod overlay {
    use crate::settings::FpsMode;

    /// The settings' values: there is no overlay to change them.
    pub fn fps_mode() -> FpsMode {
        crate::settings::get().system.fps
    }

    pub fn is_vsync() -> bool {
        !crate::settings::get().display.no_vsync
    }
}
