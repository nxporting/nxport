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
    let game_name = game.profile.map_or_else(|| format!("Switch title {tid:016X}"), |p| p.name.to_string());
    let bundle_id = nxport::profile::bundle_id(tid, game.profile);
    let version_str = game.version.map_or("1.0", |v| v.version_string).to_string();
    let default_out = PathBuf::from("target").join(format!("{}.app", game_name.replace(':', " -")));
    let out_app = opts.out_app.unwrap_or(&default_out);
    println!("[+] Packaging {} {} into {}", game_name, version_str, out_app.display());

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
    fs::copy(&engine_bin, &dest_bin)?;

    // Make sure it is executable
    let mut perms = fs::metadata(&dest_bin)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&dest_bin, perms)?;

    // Copy native sass2spv dylib for dynamic runtime shader translation
    let frameworks = contents.join("Frameworks");
    let _ = fs::create_dir_all(&frameworks);
    for candidate in &[
        engine_bin.with_file_name("sass2spv.dylib"),
        engine_bin.with_file_name("libsass2spv.dylib"),
        PathBuf::from("target/native_sass2spv/sass2spv.dylib"),
        PathBuf::from("target/native_sass2spv/libsass2spv.dylib"),
    ] {
        if candidate.is_file() {
            let _ = fs::copy(candidate, macos.join("sass2spv.dylib"));
            let _ = fs::copy(candidate, macos.join("libsass2spv.dylib"));
            let _ = fs::copy(candidate, frameworks.join("sass2spv.dylib"));
            let _ = fs::copy(candidate, frameworks.join("libsass2spv.dylib"));
            break;
        }
    }

    // Copy movie_shim dylib for in-game video playback
    for candidate in &[
        engine_bin.with_file_name("movie_shim.dylib"),
        PathBuf::from("target/movie_shim.dylib"),
        PathBuf::from("movie_shim.dylib"),
    ] {
        if candidate.is_file() {
            let _ = fs::copy(candidate, macos.join("movie_shim.dylib"));
            let _ = fs::copy(candidate, frameworks.join("movie_shim.dylib"));
            break;
        }
    }

    // Copy or symlink Dump (ExeFS & RomFS)
    let dest_exefs = bundle_dump.join("ExeFS");
    let dest_romfs = bundle_dump.join("RomFS");

    if opts.symlink_assets {
        println!("  Linking assets (symlinks)...");
        if !dest_exefs.exists() {
            std::os::unix::fs::symlink(fs::canonicalize(&exefs)?, &dest_exefs)?;
        }
        if romfs.is_dir() && !dest_romfs.exists() {
            std::os::unix::fs::symlink(fs::canonicalize(&romfs)?, &dest_romfs)?;
        }
    } else {
        println!("  Copying ExeFS...");
        copy_dir_recursive(&exefs, &dest_exefs)?;
        if romfs.is_dir() {
            println!("  Copying RomFS (this may take a few moments)...");
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
                        if !dest_mods.exists() {
                            std::os::unix::fs::symlink(fs::canonicalize(mods)?, &dest_mods)?;
                        }
                    } else {
                        println!("  Copying mod directory...");
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
                        if !dest.exists() {
                            let _ = fs::create_dir_all(&dest_mods);
                            std::os::unix::fs::symlink(fs::canonicalize(&path)?, &dest)?;
                        }
                    } else {
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
    nxport::settings::save_preset(&settings_path, &app_settings)?;

    if let Some(shader_cache) = opts.shader_cache_dir.filter(|d| d.is_dir()) {
        let dest_cache = resources.join("shader_cache");
        if dest_cache.is_symlink() {
            let _ = fs::remove_file(&dest_cache);
        } else if dest_cache.is_dir() {
            let _ = fs::remove_dir_all(&dest_cache);
        }
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
            crate::icon::icns(&logo, &resources.join("AppIcon.icns"))?;
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
    fs::write(contents.join("Info.plist"), plist_content)?;

    // Write PkgInfo
    fs::write(contents.join("PkgInfo"), "APPL????")?;

    println!("[+] Wrote {}", out_app.display());

    Ok(())
}

fn locate_engine_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
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

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}
