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

#[path = "live.rs"]
pub mod live;

pub mod overlay {
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
    use std::sync::LazyLock;

    use crate::settings::FpsMode;

    // The settings' values until the window's menu changes them (host/display_sdl.rs).
    static UNLOCKED: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(crate::settings::get().system.fps == FpsMode::Unlocked));
    static VSYNC: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(!crate::settings::get().display.no_vsync));
    static ASYNC_SHADERS: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(crate::settings::get().graphics.async_pipelines));

    pub fn fps_mode() -> FpsMode {
        if UNLOCKED.load(Relaxed) {
            FpsMode::Unlocked
        } else {
            FpsMode::Stock30
        }
    }

    pub fn set_fps_mode(mode: FpsMode) {
        UNLOCKED.store(mode == FpsMode::Unlocked, Relaxed);
    }

    pub fn toggle_fps_mode() -> FpsMode {
        UNLOCKED.fetch_not(Relaxed);
        fps_mode()
    }

    pub fn is_vsync() -> bool {
        VSYNC.load(Relaxed)
    }

    pub fn toggle_vsync() -> bool {
        !VSYNC.fetch_not(Relaxed)
    }

    pub fn is_async_shaders() -> bool {
        ASYNC_SHADERS.load(Relaxed)
    }

    pub fn toggle_async_shaders() -> bool {
        !ASYNC_SHADERS.fetch_not(Relaxed)
    }
}
