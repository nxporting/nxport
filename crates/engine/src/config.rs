// Settings that come from where the program is installed, and derived display settings.

use std::path::PathBuf;

/// nxport's directory in the platform's application data, where the games' data directories (saves, caches) and the
/// builder's files go: ~/Library/Application Support/nxport on Apple hosts, %APPDATA%\nxport on Windows.
pub fn support_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    return Some(PathBuf::from(std::env::var_os("APPDATA")?).join("nxport"));
    #[cfg(not(windows))]
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/nxport"))
}

/// The settings file an app carries (the builder writes it): in a Windows app folder, beside the program.
#[cfg(windows)]
const APP_SETTINGS: &str = "settings.toml";

/// Where an app keeps what it carries: a macOS .app's Contents/Resources; on Windows the folder the builder made (the
/// program's own directory, when it holds the app's settings file). None when the program is not in an app.
fn app_resources() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    #[cfg(windows)]
    return dir.join(APP_SETTINGS).is_file().then(|| dir.to_path_buf());
    #[cfg(not(windows))]
    (dir.file_name()? == "MacOS").then(|| dir.parent().map(|c| c.join("Resources")))?
}

/// Inside an app (a macOS .app, a Windows app folder): the dump, mods and shader cache it carries, and on Windows its
/// lifted guest library (a platform-environment layer).
pub fn app_bundle_layer() -> crate::settings::SettingsPatch {
    let mut p = crate::settings::SettingsPatch::default();
    let Some(resources) = app_resources() else {
        return p;
    };
    let dir = |names: &[&str]| names.iter().map(|n| resources.join(n)).find(|d| d.is_dir());
    p.files.game_dir = dir(&["Dump"])
        .filter(|d| d.join("ExeFS").is_dir() || d.join("exefs").is_dir())
        .or_else(|| (resources.join("ExeFS").is_dir() || resources.join("exefs").is_dir()).then(|| resources.clone()));
    p.files.mods_dir = dir(&["Mods", "mods"]);
    if p.files.mods_dir.is_none() {
        p.files.no_mods = Some(true);
    }
    p.files.shader_cache = dir(&["shader_cache"]);
    p.files.shared_fonts = dir(&["fonts"]);
    // The game's code lifted and compiled with the app's settings (its modules.txt beside it).
    #[cfg(windows)]
    {
        p.files.guest_library = Some(resources.join("guest.dll")).filter(|l| l.is_file());
    }
    p
}

/// Inside a macOS .app started from Finder (no terminal): the log goes to the game's data directory (log.txt, the previous
/// session's kept as log.prev.txt), so a frame captured in play keeps its trace.
#[cfg(target_os = "macos")]
pub fn app_log() {
    let in_bundle = std::env::current_exe()
        .ok()
        .and_then(|e| Some(e.parent()?.file_name()? == "MacOS"))
        .unwrap_or(false);
    if !in_bundle || unsafe { libc::isatty(1) } != 0 {
        return;
    }
    let Some(dir) = crate::profile::data_dir() else { return };
    let _ = std::fs::create_dir_all(dir);
    let log = dir.join("log.txt");
    let _ = std::fs::rename(&log, dir.join("log.prev.txt"));
    if let Ok(f) = std::fs::File::create(&log) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::dup2(f.as_raw_fd(), 1);
            libc::dup2(f.as_raw_fd(), 2);
        }
        std::mem::forget(f);
    }
}

#[cfg(windows)]
const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
#[cfg(windows)]
const STD_ERROR_HANDLE: u32 = -12i32 as u32;
#[cfg(windows)]
extern "system" {
    fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
    fn SetStdHandle(which: u32, handle: *mut std::ffi::c_void) -> i32;
}

/// Inside a Windows app folder, started without output handles: a GUI program (the builder marks the app's nxport.exe
/// one) started from Explorer, with no console to print to. Decided on the first call, before app_log sets a log file.
#[cfg(windows)]
fn app_without_output() -> bool {
    static NONE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *NONE.get_or_init(|| app_resources().is_some() && unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }.is_null())
}

/// Inside a Windows app folder started from Explorer: the log goes to the game's data directory (log.txt, the previous
/// session's kept as log.prev.txt), as from a macOS .app started from Finder. Started with its output going somewhere
/// (a terminal's pipe, a file), it keeps that.
#[cfg(windows)]
pub fn app_log() {
    use std::os::windows::io::IntoRawHandle;
    if !app_without_output() {
        return;
    }
    let Some(dir) = crate::profile::data_dir() else { return };
    let _ = std::fs::create_dir_all(dir);
    let log = dir.join("log.txt");
    let _ = std::fs::rename(&log, dir.join("log.prev.txt"));
    // Rust's standard output looks its handle up on every write, so setting it redirects println! and eprintln!.
    if let Ok(f) = std::fs::File::create(&log) {
        let h = f.into_raw_handle();
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, h);
            SetStdHandle(STD_ERROR_HANDLE, h);
        }
    }
}

/// Why the game could not start, shown in a message box by a Windows app started from Explorer (nothing shows its
/// output). Elsewhere nothing more: the error is printed.
pub fn app_error(message: &str) {
    #[cfg(windows)]
    if app_without_output() {
        let _ = sdl3::messagebox::show_simple_message_box(sdl3::messagebox::MessageBoxFlag::ERROR, "nxport", message, None);
    }
    #[cfg(not(windows))]
    let _ = message;
}

/// Inside an app: the settings it was built with (settings.toml in its resources), a layer above the game's profile.
pub fn app_bundle_settings() -> Option<crate::settings::SettingsPatch> {
    let file = app_resources()?.join("settings.toml");
    if !file.is_file() {
        return None;
    }
    match crate::settings::load_preset(&file) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("[!] {}: {e}", file.display());
            None
        }
    }
}

/// The aspect (width / height) to present the game's picture at: the screen shape asked for (--aspect) when the game's
/// profile made the game render at it, else None (the game's own output aspect, letterboxed to the screen).
#[cfg(nx_native)]
pub fn display_aspect() -> Option<f64> {
    crate::profile::rendered_aspect()
}
