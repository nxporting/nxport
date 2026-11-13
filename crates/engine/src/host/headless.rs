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

#[path = "menu.rs"]
pub mod menu;


pub mod overlay {
    pub use crate::perf_stats::*;
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

    pub fn set_vsync(on: bool) {
        VSYNC.store(on, Relaxed);
    }

    pub fn toggle_vsync() -> bool {
        !VSYNC.fetch_not(Relaxed)
    }

    static LOW_LATENCY: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(crate::settings::get().display.low_latency));

    /// Whether at most one finished frame may wait for the screen (settings display.low_latency; Vulkan window.rs).
    pub fn is_low_latency() -> bool {
        LOW_LATENCY.load(Relaxed)
    }

    pub fn toggle_low_latency() -> bool {
        !LOW_LATENCY.fetch_not(Relaxed)
    }

    pub fn is_async_shaders() -> bool {
        ASYNC_SHADERS.load(Relaxed)
    }

    pub fn set_async_shaders(on: bool) {
        ASYNC_SHADERS.store(on, Relaxed);
    }

    pub fn toggle_async_shaders() -> bool {
        !ASYNC_SHADERS.fetch_not(Relaxed)
    }
}
