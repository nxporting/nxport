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

/// The game's own settings: what was changed while it ran (host/live.rs), in its data directory. A layer above the app's
/// settings (the builder's) and below the command line.
pub fn game_settings_file(title_id: u64) -> Option<PathBuf> {
    Some(support_dir()?.join("games").join(format!("{title_id:016x}")).join("settings.toml"))
}

pub fn game_settings(title_id: u64) -> Option<crate::settings::SettingsPatch> {
    let file = game_settings_file(title_id)?;
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

/// Keeps a change made while the game runs in the game's settings file (the running game's; nothing without one).
#[cfg(nx_runtime)]
pub fn remember(change: impl FnOnce(&mut crate::settings::SettingsPatch)) {
    let Some(tid) = crate::game::title_id() else { return };
    let Some(file) = game_settings_file(tid) else { return };
    let mut p = game_settings(tid).unwrap_or_default();
    change(&mut p);
    if let Err(e) = std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string()).and_then(|_| crate::settings::save_preset(&file, &p)) {
        eprintln!("[!] settings: {e}");
    }
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
        // What the app's mods change (nxport_build::lift::lift_overlay).
        p.files.guest_overlay = Some(resources.join("overlay").join("guest_overlay.dll")).filter(|l| l.is_file());
        if p.files.guest_overlay.is_some() {
            p.guest.live_patches = Some(true);
        }
    }
    p
}

/// Inside a macOS .app started from Finder (no terminal): the log goes to the game's data directory (log.txt, the previous
/// session's kept as log.prev.txt), so a frame captured in play keeps its trace.
#[cfg(target_os = "macos")]
pub fn app_log() {
    if !app_without_terminal() {
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

/// Inside a macOS .app started from Finder: no terminal shows its output. Decided on the first call, before app_log
/// sends the output to a file.
#[cfg(target_os = "macos")]
fn app_without_terminal() -> bool {
    static NONE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *NONE.get_or_init(|| {
        let in_bundle = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.file_name()? == "MacOS")).unwrap_or(false);
        in_bundle && unsafe { libc::isatty(1) } == 0
    })
}

/// An alert any thread can show (CoreFoundation's user notification: no NSApplication needed), waiting until it is closed.
#[cfg(target_os = "macos")]
fn alert(title: &str, message: &str) {
    type CFStringRef = *const std::ffi::c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithBytes(alloc: *const std::ffi::c_void, bytes: *const u8, len: isize, encoding: u32, external: u8) -> CFStringRef;
        fn CFRelease(cf: *const std::ffi::c_void);
        fn CFUserNotificationDisplayAlert(
            timeout: f64,
            flags: usize,
            icon: *const std::ffi::c_void,
            sound: *const std::ffi::c_void,
            localization: *const std::ffi::c_void,
            header: CFStringRef,
            message: CFStringRef,
            default_button: CFStringRef,
            alternate_button: CFStringRef,
            other_button: CFStringRef,
            response: *mut usize,
        ) -> i32;
    }
    const UTF8: u32 = 0x0800_0100;
    const STOP_ALERT_LEVEL: usize = 0;
    unsafe {
        let cf = |t: &str| CFStringCreateWithBytes(std::ptr::null(), t.as_ptr(), t.len() as isize, UTF8, 0);
        let (h, m) = (cf(title), cf(message));
        let mut response = 0usize;
        let n = std::ptr::null();
        CFUserNotificationDisplayAlert(0.0, STOP_ALERT_LEVEL, n, n, n, h, m, n, n, n, &mut response);
        CFRelease(h);
        CFRelease(m);
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
        // An app started with a console window (nxport.exe is a console program): the window goes when the game ends or
        // crashes, so what it shows is copied to the log too.
        if app_resources().is_some() {
            unsafe { tee_console_to_log() };
        }
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

#[cfg(windows)]
extern "system" {
    fn GetFileType(handle: *mut std::ffi::c_void) -> u32;
    fn CreatePipe(read: *mut *mut std::ffi::c_void, write: *mut *mut std::ffi::c_void, attrs: *const std::ffi::c_void, size: u32) -> i32;
    fn ReadFile(h: *mut std::ffi::c_void, buf: *mut u8, n: u32, read: *mut u32, ov: *mut std::ffi::c_void) -> i32;
    fn WriteFile(h: *mut std::ffi::c_void, buf: *const u8, n: u32, written: *mut u32, ov: *mut std::ffi::c_void) -> i32;
    fn PeekNamedPipe(
        h: *mut std::ffi::c_void,
        buf: *mut u8,
        n: u32,
        read: *mut u32,
        avail: *mut u32,
        left: *mut u32,
    ) -> i32;
}

/// The pipe the console's output goes through while it is copied to the log (read end), and whether the copying thread is
/// writing a chunk out.
#[cfg(windows)]
static TEE: std::sync::OnceLock<(usize, std::sync::atomic::AtomicBool)> = std::sync::OnceLock::new();

/// Standard output and error go into a pipe; a thread writes what comes out of it both to the console and to log.txt in
/// the game's data directory (the previous session's kept as log.prev.txt). Only for a real console (a pipe or file the
/// caller gave keeps it alone).
#[cfg(windows)]
unsafe fn tee_console_to_log() {
    use std::os::windows::io::IntoRawHandle;
    const FILE_TYPE_CHAR: u32 = 2;
    let console = GetStdHandle(STD_OUTPUT_HANDLE);
    if console.is_null() || GetFileType(console) != FILE_TYPE_CHAR {
        return;
    }
    let Some(dir) = crate::profile::data_dir() else { return };
    let _ = std::fs::create_dir_all(dir);
    let log = dir.join("log.txt");
    let _ = std::fs::rename(&log, dir.join("log.prev.txt"));
    let Ok(file) = std::fs::File::create(&log) else { return };
    let file = file.into_raw_handle() as usize;
    let (mut r, mut w) = (std::ptr::null_mut(), std::ptr::null_mut());
    if CreatePipe(&mut r, &mut w, std::ptr::null(), 1 << 16) == 0 {
        return;
    }
    let tee = TEE.get_or_init(|| (r as usize, std::sync::atomic::AtomicBool::new(false)));
    let console = console as usize;
    let _ = std::thread::Builder::new().name("log copy".into()).spawn(move || {
        let busy = &tee.1;
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let mut n = 0u32;
            if ReadFile(tee.0 as *mut _, buf.as_mut_ptr(), buf.len() as u32, &mut n, std::ptr::null_mut()) == 0 || n == 0 {
                return;
            }
            busy.store(true, std::sync::atomic::Ordering::Release);
            let mut done = 0u32;
            WriteFile(console as *mut _, buf.as_ptr(), n, &mut done, std::ptr::null_mut());
            WriteFile(file as *mut _, buf.as_ptr(), n, &mut done, std::ptr::null_mut());
            busy.store(false, std::sync::atomic::Ordering::Release);
        }
    });
    SetStdHandle(STD_OUTPUT_HANDLE, w);
    SetStdHandle(STD_ERROR_HANDLE, w);
}

/// Before the process ends (a crash report written): waits until the copying thread has written out everything sent to
/// the console. A safety net of two seconds keeps a stuck write from holding the process.
pub fn drain_log() {
    #[cfg(windows)]
    unsafe {
        let Some(tee) = TEE.get() else { return };
        let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < end {
            let mut avail = 0u32;
            let ok = PeekNamedPipe(tee.0 as *mut _, std::ptr::null_mut(), 0, std::ptr::null_mut(), &mut avail, std::ptr::null_mut());
            if ok == 0 || (avail == 0 && !tee.1.load(std::sync::atomic::Ordering::Acquire)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// Why the game could not start or stopped, shown in a message box by an app started from Explorer or Finder (nothing
/// shows its output). Elsewhere nothing more: the error is printed.
pub fn app_error(message: &str) {
    #[cfg(windows)]
    if app_without_output() {
        let _ = sdl3::messagebox::show_simple_message_box(sdl3::messagebox::MessageBoxFlag::ERROR, "nxport", message, None);
    }
    #[cfg(target_os = "macos")]
    if app_without_terminal() {
        alert(crate::profile::app_title(), message);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
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
