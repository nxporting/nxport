// A Windows app: a folder that runs the game when nxport.exe in it is started without arguments. The engine finds what
// the folder carries itself (nxport::config::app_bundle_layer, as a macOS app finds its Contents/Resources):
//
//   nxport.exe                 the engine, built from the same source as the builder (the lifted code must match the
//                              code that source patches)
//   settings.toml              the settings it was built with; it marks the folder as an app
//   guest.dll, modules.txt     the game's code, lifted and compiled with those settings (lift, compile)
//   Dump/ExeFS, Dump/RomFS     the game files, when they are copied in; otherwise settings.toml names the dump
//   Mods/                      the mods for this game, when there are any
//   shader_cache/pipelines.txt the render pipelines to compile at the first draw (the shared list at build time)
//
// Saves and the caches the engine fills go to the game's data directory (%APPDATA%\nxport\games\<title id>).

use std::path::{Path, PathBuf};

pub struct Options<'a> {
    pub dump: &'a Path,
    /// A mod, or a directory of mods; those for other programs are left out.
    pub mods: Option<&'a Path>,
    pub app: &'a Path,
    /// The settings above the game's profile.
    pub edits: nxport::settings::SettingsPatch,
    /// Copy the game files in; otherwise the app reads them from `dump`.
    pub bundle_game: bool,
    pub pipeline_list: Option<&'a Path>,
}

/// The engine the app runs: built beside this program (cargo builds both into target/<profile>, a release ships both).
pub fn engine() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut candidates = vec![
        exe.with_file_name("nxport.exe"),
    ];
    for ancestor in exe.ancestors() {
        candidates.push(ancestor.join("nxport.exe"));
        candidates.push(ancestor.join("release").join("nxport.exe"));
        candidates.push(ancestor.join("debug").join("nxport.exe"));
    }
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| "nxport.exe is missing beside the builder (cargo build --release -p nxport)".into())
}

/// Lays out the app folder, all but the guest library: the engine, the settings, mods, the game files and the pipeline
/// list. Returns the directory the app's mods are in, if it has any (the code is lifted with them).
pub fn pack(o: &Options) -> Result<Option<PathBuf>, String> {
    let tid = nxport::profile::identify(o.dump).title_id.ok_or("no program id in ExeFS/main.npdm")?;
    mkdir(o.app)?;
    println!("  Copying the engine");
    copy(&engine()?, &o.app.join("nxport.exe"))?;
    // libopus, when one ships with the builder: the engine loads opus.dll from its own directory first.
    for lib in ["opus.dll", "libopus-0.dll"] {
        if let Some(l) = std::env::current_exe().ok().map(|e| e.with_file_name(lib)).filter(|l| l.is_file()) {
            copy(&l, &o.app.join(lib))?;
        }
    }

    let mods = o.app.join("Mods");
    let _ = std::fs::remove_dir_all(&mods);
    let mut any_mods = false;
    if let Some(m) = o.mods.filter(|m| m.is_dir()) {
        let single = ["romfs", "exefs", "RomFS", "ExeFS"].iter().any(|d| m.join(d).is_dir());
        let candidates: Vec<(PathBuf, PathBuf)> = if single {
            vec![(m.to_path_buf(), mods.clone())]
        } else {
            let entries = std::fs::read_dir(m).map_err(|e| format!("{}: {e}", m.display()))?;
            entries.flatten().filter(|e| e.path().is_dir()).map(|e| (e.path(), mods.join(e.file_name()))).collect()
        };
        for (src, dest) in candidates {
            if for_program(&src, tid) {
                println!("  Copying mod {}", src.display());
                copy_tree(&src, &dest)?;
                any_mods = true;
            } else {
                println!("  [!] Skipping mod '{}': for another program than {tid:016X}", src.display());
            }
        }
    }

    let mut settings = o.edits.clone();
    if !any_mods {
        settings.files.no_mods = Some(true);
    }
    if o.bundle_game {
        println!("  Copying the game files (ExeFS, RomFS)");
        for d in ["ExeFS", "RomFS"] {
            if o.dump.join(d).is_dir() {
                copy_tree(&o.dump.join(d), &o.app.join("Dump").join(d))?;
            }
        }
    } else {
        let dump = std::fs::canonicalize(o.dump).map_err(|e| format!("{}: {e}", o.dump.display()))?;
        settings.files.game_dir = Some(plain(dump));
    }
    nxport::settings::save_preset(&o.app.join("settings.toml"), &settings)?;

    let cache = o.app.join("shader_cache");
    let _ = std::fs::remove_dir_all(&cache);
    if let Some(list) = o.pipeline_list.filter(|l| l.is_file()) {
        mkdir(&cache)?;
        copy(list, &cache.join("pipelines.txt"))?;
    }
    Ok(any_mods.then_some(mods))
}

/// Whether a mod is for program `tid` (one that names no program is taken for any).
fn for_program(m: &Path, tid: u64) -> bool {
    let npdm = [m.join("exefs/main.npdm"), m.join("ExeFS/main.npdm")].into_iter().find(|f| f.is_file());
    npdm.as_deref().and_then(nxport::game::npdm_program_id).is_none_or(|t| t == tid)
}

/// A path without the `\\?\` prefix canonicalize gives on Windows, which some readers of settings files do not take.
fn plain(p: PathBuf) -> PathBuf {
    let s = p.display().to_string();
    s.strip_prefix(r"\\?\").map_or(p, PathBuf::from)
}

fn mkdir(d: &Path) -> Result<(), String> {
    std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

/// Copies a directory tree, leaving files that are already there (same size, not older): rebuilding an app does not
/// copy the game files again.
fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    mkdir(dst)?;
    for e in std::fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))?.flatten() {
        let (from, to) = (e.path(), dst.join(e.file_name()));
        if from.is_dir() {
            copy_tree(&from, &to)?;
            continue;
        }
        let (Ok(a), b) = (from.metadata(), to.metadata()) else { continue };
        let same = b.is_ok_and(|b| b.len() == a.len() && b.modified().ok() >= a.modified().ok());
        if !same {
            copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_executable_is_found() {
        let eng = engine();
        assert!(eng.is_ok(), "engine executable not found: {:?}", eng);
        assert!(eng.unwrap().is_file());
    }

    #[test]
    fn pack_creates_app_folder() {
        let dump = Path::new(r"C:\Users\Greenlord\Downloads\sdfdsf");
        if !dump.is_dir() {
            return;
        }
        let out_dir = std::env::temp_dir().join(format!("nxport-winapp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out_dir);
        let opts = Options {
            dump,
            mods: None,
            app: &out_dir,
            edits: nxport::settings::SettingsPatch::default(),
            bundle_game: false,
            pipeline_list: None,
        };
        let res = pack(&opts);
        assert!(res.is_ok(), "pack failed: {:?}", res);
        assert!(out_dir.join("nxport.exe").is_file(), "nxport.exe missing");
        assert!(out_dir.join("settings.toml").is_file(), "settings.toml missing");
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
