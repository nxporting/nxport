// crates/cli/src/pack.rs
// A macOS .app with the engine, a game dump (ExeFS + RomFS), its mods and its shader cache.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct PackOptions<'a> {
    pub dump_dir: &'a Path,
    pub mods_dir: Option<&'a Path>,
    pub shader_cache_dir: Option<&'a Path>,
    /// Default: target/<game title>.app.
    pub out_app: Option<&'a Path>,
    /// Settings the app starts with (a preset file), above the game's profile.
    pub settings: Option<&'a Path>,
    pub symlink_assets: bool,
}

pub fn pack_app(opts: &PackOptions) -> Result<(), Box<dyn std::error::Error>> {
    let exefs = opts.dump_dir.join("ExeFS");
    let romfs = opts.dump_dir.join("RomFS");
    if !exefs.is_dir() {
        return Err(format!("ExeFS directory not found at {}", exefs.display()).into());
    }
    let game = nxport::profile::identify(opts.dump_dir);
    let tid = game.title_id.ok_or("no program id in ExeFS/main.npdm")?;
    let game_name = game.name.clone();
    let bundle_id = nxport::profile::bundle_id(tid, game.profile);
    let version_str = game.version.map_or("1.0", |v| v.version_string).to_string();
    let default_out = PathBuf::from("target").join(format!("{}.app", game_name.replace(':', " -")));
    let out_app = opts.out_app.unwrap_or(&default_out);
    println!("[+] Packaging {} {} into {}", game_name, version_str, out_app.display());

    // Clean existing app bundle if present so stale files, symlinks, or read-only files
    // do not cause permission or packaging conflicts.
    if out_app.exists() || out_app.is_symlink() {
        clean_path(out_app)?;
    }

    // Bundle directories
    let contents = out_app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    let bundle_dump = resources.join("Dump");

    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;
    fs::create_dir_all(&bundle_dump)?;

    // Locate engine executable
    let engine_bin = locate_engine_binary()?;
    let dest_bin = macos.join("nxport");
    copy_file_robust(&engine_bin, &dest_bin)?;

    // Make sure it is executable
    let mut perms = fs::metadata(&dest_bin)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&dest_bin, perms)?;

    let frameworks = contents.join("Frameworks");
    let _ = fs::create_dir_all(&frameworks);

    // Copy or symlink Dump (ExeFS & RomFS)
    let dest_exefs = bundle_dump.join("ExeFS");
    let dest_romfs = bundle_dump.join("RomFS");

    if opts.symlink_assets {
        println!("  Linking assets (symlinks)...");
        if dest_exefs.is_symlink() || dest_exefs.exists() {
            clean_path(&dest_exefs)?;
        }
        std::os::unix::fs::symlink(fs::canonicalize(&exefs)?, &dest_exefs)?;
        if romfs.is_dir() {
            if dest_romfs.is_symlink() || dest_romfs.exists() {
                clean_path(&dest_romfs)?;
            }
            std::os::unix::fs::symlink(fs::canonicalize(&romfs)?, &dest_romfs)?;
        }
    } else {
        println!("  Copying ExeFS...");
        if dest_exefs.is_symlink() {
            clean_path(&dest_exefs)?;
        }
        copy_dir_recursive(&exefs, &dest_exefs)?;
        if romfs.is_dir() {
            println!("  Copying RomFS (this may take a few moments)...");
            if dest_romfs.is_symlink() {
                clean_path(&dest_romfs)?;
            }
            copy_dir_recursive(&romfs, &dest_romfs)?;
        }
    }

    // Copy or link Mods if provided, filtering for this game's program ID
    let mut packaged_any_mods = false;
    if let Some(mods) = opts.mods_dir {
        if mods.is_dir() {
            let dest_mods = resources.join("Mods");
            let is_single_mod = mods.join("romfs").is_dir()
                || mods.join("exefs").is_dir()
                || mods.join("ExeFS").is_dir()
                || mods.join("RomFS").is_dir();

            let matches_game = |p: &Path| -> bool {
                let npdm = [p.join("exefs/main.npdm"), p.join("ExeFS/main.npdm")]
                    .into_iter()
                    .find(|f| f.is_file());
                let target_tid = npdm.as_deref().and_then(nxport::game::npdm_program_id);
                match target_tid {
                    Some(t) if t != tid => {
                        println!("  [!] Skipping mod '{}': for program {t:016X}, not this game ({tid:016X})", p.display());
                        false
                    }
                    _ => true,
                }
            };

            if is_single_mod {
                if matches_game(mods) {
                    packaged_any_mods = true;
                    if opts.symlink_assets {
                        println!("  Linking mod directory...");
                        if dest_mods.is_symlink() || dest_mods.exists() {
                            clean_path(&dest_mods)?;
                        }
                        std::os::unix::fs::symlink(fs::canonicalize(mods)?, &dest_mods)?;
                    } else {
                        println!("  Copying mod directory...");
                        if dest_mods.is_symlink() {
                            clean_path(&dest_mods)?;
                        }
                        copy_dir_recursive(mods, &dest_mods)?;
                    }
                }
            } else {
                for entry in fs::read_dir(mods)? {
                    let entry = entry?;
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    if !matches_game(&path) {
                        continue;
                    }
                    packaged_any_mods = true;
                    let dest = dest_mods.join(entry.file_name());
                    if opts.symlink_assets {
                        if dest.is_symlink() || dest.exists() {
                            clean_path(&dest)?;
                        }
                        let _ = fs::create_dir_all(&dest_mods);
                        std::os::unix::fs::symlink(fs::canonicalize(&path)?, &dest)?;
                    } else {
                        if dest.is_symlink() {
                            clean_path(&dest)?;
                        }
                        copy_dir_recursive(&path, &dest)?;
                    }
                }
                if packaged_any_mods {
                    println!("  Included mods from {}", mods.display());
                }
            }
        }
    }

    let settings_path = resources.join("settings.toml");
    let mut app_settings = if let Some(file) = opts.settings {
        nxport::settings::load_preset(file)?
    } else {
        nxport::settings::SettingsPatch::default()
    };
    if !packaged_any_mods {
        app_settings.files.no_mods = Some(true);
    }
    if settings_path.exists() || settings_path.is_symlink() {
        let _ = ensure_writable(&settings_path);
        let _ = fs::remove_file(&settings_path);
    }
    nxport::settings::save_preset(&settings_path, &app_settings)?;
    let _ = ensure_writable(&settings_path);

    if let Some(shader_cache) = opts.shader_cache_dir.filter(|d| d.is_dir()) {
        let dest_cache = resources.join("shader_cache");
        clean_path(&dest_cache)?;
        if opts.symlink_assets {
            std::os::unix::fs::symlink(fs::canonicalize(&shader_cache)?, &dest_cache)?;
        } else {
            copy_dir_recursive(&shader_cache, &dest_cache)?;
        }
        println!("  Included shader cache from {}", shader_cache.display());
    }

    // Icon from the game's logo
    let icon = match crate::icon::logo(opts.dump_dir, tid) {
        Some(logo) => {
            let icon_path = resources.join("AppIcon.icns");
            if icon_path.exists() || icon_path.is_symlink() {
                let _ = ensure_writable(&icon_path);
                let _ = fs::remove_file(&icon_path);
            }
            crate::icon::icns(&logo, &icon_path)?;
            let _ = ensure_writable(&icon_path);
            "\n    <key>CFBundleIconFile</key>\n    <string>AppIcon</string>"
        }
        None => "",
    };

    // Write Info.plist
    let plist_content = format!(
r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>nxport</string>
    <key>CFBundleIdentifier</key>
    <string>{}</string>
    <key>CFBundleName</key>
    <string>{}</string>
    <key>CFBundleDisplayName</key>
    <string>{}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>{}</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>{}
</dict>
</plist>
"#,
        bundle_id, game_name, game_name, version_str, icon
    );
    write_file_robust(&contents.join("Info.plist"), plist_content)?;

    // Write PkgInfo
    write_file_robust(&contents.join("PkgInfo"), "APPL????")?;

    println!("[+] Wrote {}", out_app.display());

    Ok(())
}

pub(crate) fn locate_engine_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // The engine build.rs built from this tool's own source, else one next to this tool (a release).
    let candidates = [
        PathBuf::from(env!("NXPORT_ENGINE_MACOS")),
        std::env::current_exe()?.with_file_name("nxport"),
        PathBuf::from("target/release/nxport"),
    ];
    for c in &candidates {
        if c.is_file() {
            return Ok(c.clone());
        }
    }
    // Fallback: check current exe directory
    if let Ok(curr) = std::env::current_exe() {
        if let Some(dir) = curr.parent() {
            let engine = dir.join("nxport");
            if engine.is_file() {
                return Ok(engine);
            }
        }
    }
    Err("Could not find nxport release binary. Run `cargo build --release` first.".into())
}

/// Ensures that `path` has user write permissions so it can be modified, overwritten, or removed.
pub fn ensure_writable(path: &Path) -> std::io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.is_symlink() {
            let mut perms = metadata.permissions();
            let mode = perms.mode();
            let needed = if metadata.is_dir() { 0o700 } else { 0o600 };
            if mode & needed != needed {
                perms.set_mode(mode | needed);
                let _ = fs::set_permissions(path, perms);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        if let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) {
            unsafe {
                libc::chflags(c_path.as_ptr(), 0);
            }
        }
    }
    Ok(())
}

/// Recursively removes a file or directory tree, ensuring all permissions allow deletion.
/// Symlinks are removed without traversing their target.
pub fn clean_path(path: &Path) -> std::io::Result<()> {
    if !path.exists() && !path.is_symlink() {
        return Ok(());
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    if metadata.is_symlink() {
        if let Some(parent) = path.parent() {
            let _ = ensure_writable(parent);
        }
        return fs::remove_file(path);
    }

    if metadata.is_dir() {
        let _ = ensure_writable(path);
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            clean_path(&entry.path())?;
        }
        let _ = ensure_writable(path);
        if let Some(parent) = path.parent() {
            let _ = ensure_writable(parent);
        }
        fs::remove_dir(path)
    } else {
        if let Some(parent) = path.parent() {
            let _ = ensure_writable(parent);
        }
        let _ = ensure_writable(path);
        fs::remove_file(path)
    }
}

/// Copies a file robustly: unlinks or makes writable any existing destination file first,
/// uses APFS cloning where available, and ensures the destination has user write permissions.
pub fn copy_file_robust(src: &Path, dst: &Path) -> std::io::Result<u64> {
    if dst.is_symlink() || dst.is_file() {
        if let Some(parent) = dst.parent() {
            let _ = ensure_writable(parent);
        }
        let _ = ensure_writable(dst);
        let _ = fs::remove_file(dst);
    } else if dst.is_dir() {
        clean_path(dst)?;
    }

    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        if let (Ok(c_src), Ok(c_dst)) = (
            std::ffi::CString::new(src.as_os_str().as_bytes()),
            std::ffi::CString::new(dst.as_os_str().as_bytes()),
        ) {
            if unsafe { libc::clonefile(c_src.as_ptr(), c_dst.as_ptr(), 0) } == 0 {
                let _ = ensure_writable(dst);
                if let Ok(metadata) = fs::symlink_metadata(dst) {
                    return Ok(metadata.len());
                }
                return Ok(0);
            }
        }
    }

    let bytes = fs::copy(src, dst)?;
    let _ = ensure_writable(dst);
    Ok(bytes)
}

/// Writes content to a file, unlinking any existing read-only file first.
pub fn write_file_robust(path: &Path, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    if path.exists() || path.is_symlink() {
        if let Some(parent) = path.parent() {
            let _ = ensure_writable(parent);
        }
        let _ = ensure_writable(path);
        let _ = fs::remove_file(path);
    }
    fs::write(path, content)?;
    let _ = ensure_writable(path);
    Ok(())
}

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if dst.is_symlink() || dst.is_file() {
        let _ = clean_path(dst);
    }
    fs::create_dir_all(dst)?;
    let _ = ensure_writable(dst);

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else if ty.is_file() {
            copy_file_robust(&entry.path(), &dest_path)?;
        } else if ty.is_symlink() {
            if let Ok(target) = fs::read_link(entry.path()) {
                if dest_path.exists() || dest_path.is_symlink() {
                    let _ = clean_path(&dest_path);
                }
                let _ = std::os::unix::fs::symlink(&target, &dest_path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "nxport-pack-test-{}-{}-{:?}",
                name,
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = clean_path(&p);
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = clean_path(&self.0);
        }
    }

    #[test]
    fn test_ensure_writable() {
        let t = TempDir::new("writable");
        let file = t.0.join("test.txt");
        fs::write(&file, "hello").unwrap();
        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&file, perms).unwrap();
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o200, 0);

        ensure_writable(&file).unwrap();
        assert_ne!(fs::metadata(&file).unwrap().permissions().mode() & 0o200, 0);
    }

    #[test]
    fn test_copy_file_robust_overwrites_read_only() {
        let t = TempDir::new("copy_ro");
        let src = t.0.join("src.bin");
        let dst = t.0.join("dst.bin");
        fs::write(&src, "new content").unwrap();
        let mut src_perms = fs::metadata(&src).unwrap().permissions();
        src_perms.set_mode(0o444);
        fs::set_permissions(&src, src_perms).unwrap();

        fs::write(&dst, "old content").unwrap();
        let mut dst_perms = fs::metadata(&dst).unwrap().permissions();
        dst_perms.set_mode(0o444);
        fs::set_permissions(&dst, dst_perms).unwrap();

        // Standard fs::copy would fail with EACCES / PermissionDenied
        assert!(fs::copy(&src, &dst).is_err());

        // copy_file_robust succeeds and dst becomes writable
        copy_file_robust(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "new content");
        assert_ne!(fs::metadata(&dst).unwrap().permissions().mode() & 0o200, 0);
    }

    #[test]
    fn test_clean_path_removes_read_only_tree() {
        let t = TempDir::new("clean_ro");
        let sub = t.0.join("bundle").join("Contents").join("Resources");
        fs::create_dir_all(&sub).unwrap();
        let f = sub.join("main");
        fs::write(&f, "switch binary").unwrap();
        let mut perms = fs::metadata(&f).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&f, perms).unwrap();

        // Make the containing directories read-only
        let mut dir_perms = fs::metadata(&sub).unwrap().permissions();
        dir_perms.set_mode(0o555);
        fs::set_permissions(&sub, dir_perms).unwrap();

        let bundle = t.0.join("bundle");
        clean_path(&bundle).unwrap();
        assert!(!bundle.exists());
    }

    #[test]
    fn test_clean_path_preserves_symlink_targets() {
        let t = TempDir::new("clean_sym");
        let target_dir = t.0.join("target_dir");
        fs::create_dir_all(&target_dir).unwrap();
        let target_file = target_dir.join("data.bin");
        fs::write(&target_file, "precious data").unwrap();

        let link_parent = t.0.join("app").join("Dump");
        fs::create_dir_all(&link_parent).unwrap();
        let link_path = link_parent.join("RomFS");
        std::os::unix::fs::symlink(&target_dir, &link_path).unwrap();

        clean_path(&t.0.join("app")).unwrap();
        assert!(!t.0.join("app").exists());
        assert!(target_dir.exists());
        assert_eq!(fs::read_to_string(&target_file).unwrap(), "precious data");
    }

    #[test]
    fn test_copy_dir_recursive_overwrites_read_only_files() {
        let t = TempDir::new("copy_dir_ro");
        let src = t.0.join("src_exefs");
        fs::create_dir_all(&src).unwrap();
        let src_main = src.join("main");
        fs::write(&src_main, "new main").unwrap();
        let mut p = fs::metadata(&src_main).unwrap().permissions();
        p.set_mode(0o444);
        fs::set_permissions(&src_main, p).unwrap();

        let dst = t.0.join("dst_exefs");
        fs::create_dir_all(&dst).unwrap();
        let dst_main = dst.join("main");
        fs::write(&dst_main, "old main").unwrap();
        let mut p2 = fs::metadata(&dst_main).unwrap().permissions();
        p2.set_mode(0o444);
        fs::set_permissions(&dst_main, p2).unwrap();

        copy_dir_recursive(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(&dst_main).unwrap(), "new main");
        assert_ne!(fs::metadata(&dst_main).unwrap().permissions().mode() & 0o200, 0);
    }
}
