// iOS apps (.ipa): the engine for iOS, the game's code patched and signed ahead of time (iOS runs only signed code),
// the iOS shader cache, mods, and the settings the app starts with. Signed ad hoc; sideloading tools re-sign it.

use std::fs;
use std::path::{Path, PathBuf};

use nxport::settings::profiles::{self, Platform};
use nxport::settings::{self, Settings, SettingsPatch};

use crate::macho;

pub const GUEST_INSTALL_NAME: &str = "@rpath/NxportGuest.framework/NxportGuest";

const EXECUTABLE: &str = "nxport";

/// The settings the app runs `dump` with: the iOS profile, the game's iOS profile, the app's settings (`edits`), and
/// what the app's own environment sets (x18 trampolines, a phone's default handheld mode, no window at image build).
pub fn app_settings(dump: &Path, mods: Option<&Path>, edits: SettingsPatch) -> Settings {
    let mut layers = vec![profiles::platform(Platform::IOS)];
    let dump_id = nxport::profile::identify(dump);
    if let Some(p) = dump_id.profile {
        layers.extend(nxport::profile::layers(p, Platform::IOS));
    }
    layers.push(edits);
    let mut env = SettingsPatch::default();
    env.files.game_dir = Some(dump.to_path_buf());
    let has_matching_mods = mods.is_some_and(|m| {
        if !m.is_dir() {
            return false;
        }
        let Some(tid) = dump_id.title_id else { return true };
        let is_single = m.join("romfs").is_dir() || m.join("exefs").is_dir() || m.join("ExeFS").is_dir() || m.join("RomFS").is_dir();
        if is_single {
            mod_matches_tid(m, tid)
        } else {
            fs::read_dir(m).map_or(false, |rd| {
                rd.flatten().any(|e| e.path().is_dir() && mod_matches_tid(&e.path(), tid))
            })
        }
    });
    if let Some(m) = mods.filter(|_| has_matching_mods) {
        env.files.mods_dir = Some(m.to_path_buf());
    } else {
        env.files.no_mods = Some(true);
    }
    env.system.handheld = Some(true);
    env.display.headless = Some(true);
    env.guest.x18_trampolines = Some(true);
    env.files.no_miss_dump = Some(true);
    layers.push(env);
    settings::resolve(layers)
}

pub struct Options<'a> {
    pub dump: &'a Path,
    pub mods: Option<&'a Path>,
    /// The iOS shader cache (shaders build-ios); without it the app translates shaders as the game loads them.
    pub cache: Option<&'a Path>,
    pub out_dir: &'a Path,
    /// The settings the app starts with, above its iOS and game profiles.
    pub edits: SettingsPatch,
    /// Put the game's ExeFS/ and RomFS/ into the app; without it they go into the app's Documents/Game.
    pub bundle_game: bool,
    /// Request the increased memory limit and extended virtual addressing.
    pub entitlements: bool,
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpaBuildPhase {
    PatchingCode,
    SigningGuestImage,
    AssemblingApp,
    AddingGameFiles,
    SigningApp,
    WritingIpa,
}

impl IpaBuildPhase {
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PatchingCode => "Patching the game's code for iOS",
            Self::SigningGuestImage => "Signing the code image",
            Self::AssemblingApp => "Assembling the app",
            Self::AddingGameFiles => "Adding the game files",
            Self::SigningApp => "Signing the app",
            Self::WritingIpa => "Writing the .ipa",
        }
    }

    #[inline]
    pub fn default_fraction(self) -> f32 {
        match self {
            Self::PatchingCode => 0.1,
            Self::SigningGuestImage => 0.3,
            Self::AssemblingApp => 0.5,
            Self::AddingGameFiles => 0.6,
            Self::SigningApp => 0.8,
            Self::WritingIpa => 0.9,
        }
    }
}

impl std::fmt::Display for IpaBuildPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Builds the .ipa into `out_dir`; returns its path. `log` gets an event per step.
pub fn build(o: &Options, log: &dyn Fn(IpaBuildPhase)) -> Result<PathBuf, String> {
    let id = nxport::profile::identify(o.dump);
    let tid = id.title_id.ok_or("no program id in ExeFS/main.npdm")?;
    let name = id.name.clone();
    let bundle_id = o.bundle_id.clone().unwrap_or_else(|| nxport::profile::bundle_id(tid, id.profile));
    let engine = ios_artifact("nxport").ok_or("the engine for iOS is missing (ios/nxport next to this program)")?;

    log(IpaBuildPhase::PatchingCode);
    let s = app_settings(o.dump, o.mods, o.edits.clone());
    let image = nxport::guest_image(s.clone())?;

    let work = o.out_dir.join(format!(".{}.ipa.work", sanitize(&name)));
    let _ = fs::remove_dir_all(&work);
    let app = work.join("Payload").join(format!("{EXECUTABLE}.app"));
    let fw = app.join("Frameworks/NxportGuest.framework");
    mkdir(&fw)?;
    mkdir(&app.join("shader_cache"))?;

    log(IpaBuildPhase::SigningGuestImage);
    macho::write_guest_dylib(&image, &fw.join("NxportGuest"), macho::Platform::IOS, GUEST_INSTALL_NAME)?;
    write(&fw.join("Info.plist"), &framework_plist(&bundle_id))?;

    log(IpaBuildPhase::AssemblingApp);
    copy(&engine, &app.join(EXECUTABLE))?;
    set_executable(&app.join(EXECUTABLE))?;
    if let Some(shim) = ios_artifact("movie_shim.dylib") {
        println!("  Including movie shim from {}", shim.display());
        copy(&shim, &app.join("Frameworks/movie_shim.dylib"))?;
        macho::sign(&app.join("Frameworks/movie_shim.dylib"), None)?;
    } else {
        println!("  [!] Warning: movie_shim.dylib not found; in-game movies will have no tracks");
    }
    let mut packaged_any_mods = false;
    if let Some(m) = o.mods {
        if m.is_dir() {
            let dest_mods = app.join("mods");
            let is_single = m.join("romfs").is_dir() || m.join("exefs").is_dir() || m.join("ExeFS").is_dir() || m.join("RomFS").is_dir();
            if is_single {
                if mod_matches_tid(m, tid) {
                    packaged_any_mods = true;
                    copy_tree(m, &dest_mods)?;
                } else {
                    println!("  [!] Skipping mod '{}': target program does not match {tid:016X}", m.display());
                }
            } else if let Ok(rd) = fs::read_dir(m) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.is_dir() && mod_matches_tid(&path, tid) {
                        packaged_any_mods = true;
                        copy_tree(&path, &dest_mods.join(entry.file_name()))?;
                    } else if path.is_dir() {
                        println!("  [!] Skipping mod '{}': target program does not match {tid:016X}", entry.file_name().to_string_lossy());
                    }
                }
            }
        }
    }
    if o.bundle_game {
        log(IpaBuildPhase::AddingGameFiles);
        mkdir(&app.join("Game"))?;
        copy_tree(&o.dump.join("ExeFS"), &app.join("Game/ExeFS"))?;
        copy_tree(&o.dump.join("RomFS"), &app.join("Game/RomFS"))?;
    }
    for e in o.cache.map(fs::read_dir).transpose().map_err(|e| format!("shader cache: {e}"))?.into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n == "index.txt"
            || n == "failures.txt"
            || n == "pipelines.txt"
            || (n.starts_with("shaders_") && n.ends_with(".metallib"))
            || (n.starts_with("pipelines") && n.ends_with(".metalar"))
        {
            copy(&e.path(), &app.join("shader_cache").join(&n))?;
        }
    }
    let font_src = o.edits.files.shared_fonts.as_deref()
        .map(PathBuf::from)
        .or_else(|| {
            let d = o.dump.join("fonts");
            d.is_dir().then_some(d)
        })
        .or_else(|| {
            let d = o.dump.join("RomFS/fonts");
            d.is_dir().then_some(d)
        })
        .or_else(|| {
            let d = o.dump.join("romfs/fonts");
            d.is_dir().then_some(d)
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/nxport/fonts")).filter(|d| d.is_dir())
        })
        .or_else(|| {
            std::env::current_exe().ok().and_then(|exe| {
                let macos = exe.parent()?;
                (macos.file_name()? == "MacOS").then(|| macos.parent().map(|c| c.join("Resources/fonts"))).flatten()
            }).filter(|d| d.is_dir())
        })
        .or_else(|| {
            let d = PathBuf::from("Resources/fonts");
            d.is_dir().then_some(d)
        })
        .or_else(|| {
            let d = PathBuf::from("fonts");
            d.is_dir().then_some(d)
        });

    if let Some(src) = font_src {
        if src.is_dir() {
            println!("  Including shared fonts from {}", src.display());
            copy_tree(&src, &app.join("fonts"))?;
        }
    }
    let sys_data_src = o.edits.files.system_data_dir.as_deref()
        .map(PathBuf::from)
        .or_else(|| {
            let d = o.dump.join("system_data");
            d.is_dir().then_some(d)
        })
        .or_else(|| {
            let d = o.dump.join("RomFS/system_data");
            d.is_dir().then_some(d)
        })
        .or_else(|| {
            let d = o.dump.join("romfs/system_data");
            d.is_dir().then_some(d)
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/nxport/system_data")).filter(|d| d.is_dir())
        })
        .or_else(|| {
            std::env::current_exe().ok().and_then(|exe| {
                let macos = exe.parent()?;
                (macos.file_name()? == "MacOS").then(|| macos.parent().map(|c| c.join("Resources/system_data"))).flatten()
            }).filter(|d| d.is_dir())
        })
        .or_else(|| {
            let d = PathBuf::from("Resources/system_data");
            d.is_dir().then_some(d)
        })
        .or_else(|| {
            let d = PathBuf::from("system_data");
            d.is_dir().then_some(d)
        });

    if let Some(src) = sys_data_src {
        if src.is_dir() {
            println!("  Including system data from {}", src.display());
            copy_tree(&src, &app.join("system_data"))?;
        }
    }
    if let Ok(entries) = fs::read_dir(o.dump) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    if name.len() == 16 && u64::from_str_radix(name, 16).is_ok() {
                        let _ = copy_tree(&p, &app.join("system_data").join(name));
                    }
                }
            }
        }
    }
    let mut edits = o.edits.clone();
    if !packaged_any_mods {
        edits.files.no_mods = Some(true);
    }
    if app.join("fonts").is_dir() {
        edits.files.shared_fonts = Some(PathBuf::from("fonts"));
    }
    if app.join("system_data").is_dir() {
        edits.files.system_data_dir = Some(PathBuf::from("system_data"));
    }
    settings::save_preset(&app.join("settings.toml"), &edits)?;
    let icons = icons(o.dump, tid, &app)?;
    let aspect = s.display.launch_aspect.map_or_else(|| "off".to_string(), |a| a.to_string());
    write(&app.join("Info.plist"), &app_plist(&name, &bundle_id, icons, &aspect))?;
    mkdir(&app.join("Settings.bundle/en.lproj"))?;
    write(&app.join("Settings.bundle/en.lproj/Root.strings"), "")?;
    write(&app.join("Settings.bundle/Root.plist"), include_str!("ipa_files/Settings.plist"))?;

    log(IpaBuildPhase::SigningApp);
    macho::sign(&fw, None)?;
    if o.entitlements {
        let ent = work.join("entitlements.plist");
        write(&ent, ENTITLEMENTS)?;
        write(&app.join("Entitlements.plist"), ENTITLEMENTS)?;
        write(&app.join("archived-expanded-entitlements.xcent"), ENTITLEMENTS)?;
        macho::sign(&app, Some(&ent))?;
    } else {
        macho::sign(&app, None)?;
    }

    log(IpaBuildPhase::WritingIpa);
    let ipa = o.out_dir.join(format!("{}.ipa", sanitize(&name)));
    zip_payload(&work.join("Payload"), &ipa)?;
    let _ = fs::remove_dir_all(&work);
    Ok(ipa)
}

/// An iOS artifact: the one build.rs built next to the iOS engine from this program's own source, else ios/<name> next
/// to this program (a release).
fn ios_artifact(name: &str) -> Option<PathBuf> {
    let built = Path::new(env!("NXPORT_ENGINE_IOS")).with_file_name(name);
    let mut candidates = vec![
        built,
        PathBuf::from("target/engines/aarch64-apple-ios/release").join(name),
        PathBuf::from("target/aarch64-apple-ios/release").join(name),
        PathBuf::from("tools/ios").join(name),
        PathBuf::from("target/ios").join(name),
        PathBuf::from("target/ios/work/Payload/Kirby.app/Frameworks").join(name),
        PathBuf::from("target/work/Payload/TotK.app/Frameworks").join(name),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("ios").join(name));
            candidates.push(parent.join(name));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn sanitize(name: &str) -> String {
    name.replace([':', '/'], " -")
}

fn mkdir(p: &Path) -> Result<(), String> {
    fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn write(p: &Path, s: &str) -> Result<(), String> {
    fs::write(p, s).map_err(|e| format!("{}: {e}", p.display()))
}

/// A copy-on-write clone (APFS), else a copy.
fn copy(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
    let _ = fs::remove_file(to);
    if unsafe { libc::clonefile(c(from).as_ptr(), c(to).as_ptr(), 0) } == 0 {
        return Ok(());
    }
    fs::copy(from, to).map(|_| ()).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    mkdir(to)?;
    for e in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
        let (src, n) = (e.path(), e.file_name());
        if n == ".DS_Store" {
            continue;
        }
        if src.is_dir() {
            copy_tree(&src, &to.join(&n))?;
        } else {
            copy(&src, &to.join(&n))?;
        }
    }
    Ok(())
}

fn set_executable(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).map_err(|e| format!("{}: {e}", p.display()))
}

/// App icons from the game's logo in the dump; the plist entries, or none without one.
fn icons(dump: &Path, title_id: u64, app: &Path) -> Result<String, String> {
    let Some(src) = crate::icon::logo(dump, title_id) else { return Ok(String::new()) };
    for (file, px) in [("AppIcon60x60@2x.png", 120), ("AppIcon60x60@3x.png", 180), ("AppIcon76x76@2x~ipad.png", 152), ("AppIcon83.5x83.5@2x~ipad.png", 167)] {
        crate::icon::scaled_png(&src, px, &app.join(file))?;
    }
    Ok("<key>CFBundleIcons</key><dict><key>CFBundlePrimaryIcon</key><dict><key>CFBundleIconFiles</key><array><string>AppIcon60x60</string></array></dict></dict>\
<key>CFBundleIcons~ipad</key><dict><key>CFBundlePrimaryIcon</key><dict><key>CFBundleIconFiles</key><array><string>AppIcon60x60</string><string>AppIcon76x76</string><string>AppIcon83.5x83.5</string></array></dict></dict>".into())
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

const PLIST_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
"#;

fn framework_plist(bundle_id: &str) -> String {
    format!(
        "{PLIST_HEAD}<key>CFBundleExecutable</key><string>NxportGuest</string>
<key>CFBundleIdentifier</key><string>{}.guest</string>
<key>CFBundleName</key><string>NxportGuest</string>
<key>CFBundlePackageType</key><string>FMWK</string>
<key>CFBundleShortVersionString</key><string>1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleSupportedPlatforms</key><array><string>iPhoneOS</string></array>
<key>MinimumOSVersion</key><string>16.0</string>
</dict></plist>
",
        xml(bundle_id)
    )
}

fn app_plist(name: &str, bundle_id: &str, icons: String, aspect: &str) -> String {
    format!(
        "{PLIST_HEAD}<key>CFBundleExecutable</key><string>{EXECUTABLE}</string>
<key>CFBundleIdentifier</key><string>{id}</string>
<key>CFBundleName</key><string>{name}</string>
<key>CFBundleDisplayName</key><string>{name}</string>
{icons}
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleSupportedPlatforms</key><array><string>iPhoneOS</string></array>
<key>MinimumOSVersion</key><string>16.0</string>
<key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
<key>UIRequiredDeviceCapabilities</key><array><string>arm64</string><string>metal</string></array>
<key>UILaunchScreen</key><dict/>
<key>UIStatusBarHidden</key><true/>
<key>UIRequiresFullScreen</key><true/>
<key>UIFileSharingEnabled</key><true/>
<key>LSSupportsOpeningDocumentsInPlace</key><true/>
<key>UISupportedInterfaceOrientations</key><array><string>UIInterfaceOrientationLandscapeLeft</string><string>UIInterfaceOrientationLandscapeRight</string></array>
<key>GCSupportsControllerUserInteraction</key><true/>
<key>LSApplicationCategoryType</key><string>public.app-category.games</string>
<key>GCSupportsGameMode</key><true/>
<key>LSSupportsGameMode</key><true/>
<key>NxportDisplayAspect</key><string>{aspect}</string>
<key>GCSupportedGameControllers</key><array><dict><key>ProfileName</key><string>ExtendedGamepad</string></dict></array>
</dict></plist>
",
        id = xml(bundle_id),
        name = xml(name),
        aspect = xml(aspect),
    )
}

// Game Mode (iOS 18+) applies to apps in the games category that opt in (LSSupportsGameMode above).
const ENTITLEMENTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>com.apple.developer.kernel.increased-memory-limit</key><true/>
<key>com.apple.developer.kernel.extended-virtual-addressing</key><true/>
</dict></plist>
"#;

/// The .ipa: Payload/ zipped; game files stored as they are (already compressed, and large), the rest deflated.
fn zip_payload(payload: &Path, ipa: &Path) -> Result<(), String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", ipa.display());
    let _ = fs::remove_file(ipa);
    let file = fs::File::create(ipa).map_err(|e| err(&e))?;
    let mut z = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let base = payload.parent().unwrap();
    let mut stack = vec![payload.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rel = dir.strip_prefix(base).unwrap().to_string_lossy().into_owned();
        z.add_directory(format!("{rel}/"), SimpleFileOptions::default()).map_err(|e| err(&e))?;
        let mut entries: Vec<_> = fs::read_dir(&dir).map_err(|e| err(&e))?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            use std::os::unix::fs::PermissionsExt;
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            let stored = rel.contains(".app/Game/");
            let mode = fs::metadata(&p).map_err(|e| err(&e))?.permissions().mode();
            let opts = SimpleFileOptions::default()
                .compression_method(if stored { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated })
                .unix_permissions(mode & 0o777)
                .large_file(fs::metadata(&p).map(|m| m.len() >= u32::MAX as u64).unwrap_or(false));
            z.start_file(rel, opts).map_err(|e| err(&e))?;
            let mut f = fs::File::open(&p).map_err(|e| err(&e))?;
            std::io::copy(&mut f, &mut z).map_err(|e| err(&e))?;
        }
    }
    z.finish().map_err(|e| err(&e))?.flush().map_err(|e| err(&e))?;
    Ok(())
}

fn mod_matches_tid(p: &Path, tid: u64) -> bool {
    let npdm = [p.join("exefs/main.npdm"), p.join("ExeFS/main.npdm")]
        .into_iter()
        .find(|f| f.is_file());
    let target_tid = npdm.as_deref().and_then(nxport::game::npdm_program_id);
    match target_tid {
        Some(t) if t != tid => false,
        _ => true,
    }
}
