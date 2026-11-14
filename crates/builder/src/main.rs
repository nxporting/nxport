// nxport-builder: turns a game dump into a native app, and runs the steps of one as commands (cli.rs).

// A window, not a console program: Explorer opens no console window with it (app.rs attaches to a parent's console).
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
#[cfg(any(target_os = "macos", windows))]
mod apple;
mod cli;
#[cfg(any(target_os = "macos", windows))]
mod ios;
mod log;
mod window;
#[cfg(windows)]
mod window_sdl;
// The Windows app's steps (on any host) and the Windows host's .ipa steps.
mod windows;

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    {
        // Linked delay-loaded (build.rs): SDL3.dll is put in place before anything uses it.
        if let Err(e) = nxport::sdl_library::load() {
            // Said by Windows itself: SDL's message box is what is missing.
            extern "system" {
                fn MessageBoxW(hwnd: *mut core::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
            }
            let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
            let text = wide(&format!("SDL3.dll could not be loaded: {e}"));
            unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), wide("nxport").as_ptr(), 0x10) };
            return std::process::ExitCode::FAILURE;
        }
        // A Windows app's nxport.exe is this program (nxport_build::winapp::pack), so the engine an app runs is always
        // the one that lifted its code: it runs the game by that name, or beside the app's files.
        let exe = std::env::current_exe().unwrap_or_default();
        let app = exe.with_file_name("settings.toml").is_file() && exe.with_file_name("guest.dll").is_file();
        if app || exe.file_stem().is_some_and(|s| s.eq_ignore_ascii_case("nxport")) {
            return nxport::main();
        }
    }
    match app::main() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
