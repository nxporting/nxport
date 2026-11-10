// SDL3.dll, carried in the program (Windows): the builder is a single file that runs without it beside. The builder
// links SDL delay-loaded (its build.rs), so Windows looks for the library at its first call instead of at start, and
// load() puts it in place before that. A Windows app has its own copy beside its nxport.exe (nxport_build::winapp).

use std::path::PathBuf;

/// The library the engine is built against (vendor/SDL3).
pub const DLL: &[u8] = include_bytes!("../../../vendor/SDL3/lib/SDL3.dll");

/// Loads SDL3.dll unless the program's folder has one: from nxport's data folder, written there once per version of the
/// library. Call before anything uses SDL; the delay-load helper then finds it loaded by name.
pub fn load() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if exe.with_file_name("SDL3.dll").is_file() {
        return Ok(());
    }
    let dir = crate::config::support_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("sdl")
        .join(format!("{:016x}", DLL.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))));
    let dll = dir.join("SDL3.dll");
    if !dll.is_file() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        // Written whole, then renamed: a program started at the same time never loads half of it.
        let part: PathBuf = dir.join(format!("SDL3.dll.{}", std::process::id()));
        std::fs::write(&part, DLL).map_err(|e| format!("{}: {e}", part.display()))?;
        if std::fs::rename(&part, &dll).is_err() {
            let _ = std::fs::remove_file(&part);
        }
    }
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut core::ffi::c_void;
    }
    use std::os::windows::ffi::OsStrExt;
    let name: Vec<u16> = dll.as_os_str().encode_wide().chain([0]).collect();
    if unsafe { LoadLibraryW(name.as_ptr()) }.is_null() {
        return Err(format!("{}: {}", dll.display(), std::io::Error::last_os_error()));
    }
    Ok(())
}
