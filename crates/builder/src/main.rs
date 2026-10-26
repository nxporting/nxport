// nxport-builder: turns a game dump into a native app.

// A window, not a console program: Explorer opens no console window with it (app.rs attaches to a parent's console).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
#[cfg(target_os = "macos")]
mod apple;
mod log;
mod window;
#[cfg(windows)]
mod window_sdl;
#[cfg(windows)]
mod windows;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::main()
}
