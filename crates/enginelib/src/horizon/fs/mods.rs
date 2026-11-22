// Mods in Atmosphère layout: discovering them, their exefs (patches, plugins, replaced modules) and romfs layers, and
// seeding the SD card from them.

use super::*;

/// A mod's `romfs`, `exefs` or `cheats` folder, whatever its case: mods ship both `romfs` and `RomFS`, and iOS's file
/// system is case sensitive.
pub fn mod_subdir(dir: &Path, name: &str) -> PathBuf {
    let exact = dir.join(name);
    if exact.is_dir() {
        return exact;
    }
    std::fs::read_dir(dir)
        .ok()
        .and_then(|rd| rd.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name) && e.path().is_dir()))
        .map_or(exact, |e| e.path())
}

/// A file directly in a mod's folder, whatever its case: `romfs.bin`, `exefs.nsp`.
pub(crate) fn mod_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.is_file() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name) && e.path().is_file()).map(|e| e.path())
}

/// Whether the mod in `dir` has its code packed: Atmosphere's exefs.nsp.
pub fn has_exefs_nsp(dir: &Path) -> bool {
    mod_file(dir, "exefs.nsp").is_some()
}

pub fn is_mod_dir(dir: &Path) -> bool {
    ["romfs", "exefs", "cheats"].iter().any(|d| mod_subdir(dir, d).is_dir()) || mod_file(dir, "romfs.bin").is_some() || has_exefs_nsp(dir)
}

/// Folders mods are packaged in on the way to their contents: `atmosphere/contents/<title id>/`, Ryujinx's
/// `mods/contents/<title id>/`, or a bare `<title id>/`.
pub(crate) fn is_layout_dir(name: &str) -> bool {
    ["atmosphere", "contents", "mods"].iter().any(|d| name.eq_ignore_ascii_case(d)) || title_dir(name).is_some()
}

pub(crate) fn title_dir(name: &str) -> Option<u64> {
    (name.len() == 16).then(|| u64::from_str_radix(name, 16).ok()).flatten()
}

pub(crate) fn sorted_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// The mods under `root`: `root` itself if it is one, else the folders in it, through the folders mods are packaged
/// in (is_layout_dir). A folder named for another title is skipped. Other folders inside a mod are not looked into:
/// a download's variants would all turn on at once.
pub fn discover_mods(root: &Path, title: Option<u64>) -> Vec<(String, PathBuf)> {
    let name_of = |p: &Path| p.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    if is_mod_dir(root) {
        return vec![(name_of(root), root.to_path_buf())];
    }
    let mut out = Vec::new();
    for (name, path) in sorted_dirs(root) {
        if let (Some(t), Some(game)) = (title_dir(&name), title) {
            if t != game {
                println!("[!] Mods: '{name}' is for another program, not this game ({game:016X}); skipped");
                continue;
            }
        }
        if is_layout_dir(&name) {
            out.extend(discover_mods(&path, title));
        } else if is_mod_dir(&path) {
            out.push((name, path));
        } else if sorted_dirs(&path).iter().any(|(n, _)| is_layout_dir(n)) {
            // A download holding the layout: named for its own folder, which is what decides its priority.
            for (i, (_, p)) in discover_mods(&path, title).into_iter().enumerate() {
                out.push((if i == 0 { name.clone() } else { format!("{name} {i}") }, p));
            }
        } else {
            if name != "exefs_patches" {
                println!("[!] Mods: '{name}' has no romfs, exefs or cheats folder; ignored");
            }
        }
    }
    out
}

/// What one mod says of an ExeFS file.
#[derive(Debug, PartialEq)]
pub(crate) enum ExefsEntry {
    File(PathBuf),
    /// `<name>.stub`: the file is removed from what lies below.
    Stub,
    Absent,
}

/// As Atmosphere's SdCardRedirectionCodeFileSystem: a file in the mod's exefs folder, else its stub, else the exefs.nsp's
/// file (`unpacked`, a folder).
pub(crate) fn exefs_entry(exefs: &Path, unpacked: Option<&Path>, name: &str) -> ExefsEntry {
    let loose = exefs.join(name);
    if loose.is_file() {
        ExefsEntry::File(loose)
    } else if exefs.join(format!("{name}.stub")).is_file() {
        ExefsEntry::Stub
    } else {
        match unpacked.map(|d| d.join(name)).filter(|p| p.is_file()) {
            Some(p) => ExefsEntry::File(p),
            None => ExefsEntry::Absent,
        }
    }
}

/// The exefs.nsp files of the enabled mods, unpacked once: (mod directory, folder). Modules are loaded from paths, and
/// these are small. Under the game's data directory, named for the file's path, size and time, so a changed file is
/// unpacked again and a mod's folder may be read-only (an iOS app's). Nothing is written without an exefs.nsp.
pub(crate) fn unpacked_nsps() -> &'static [(PathBuf, PathBuf)] {
    static DIRS: std::sync::OnceLock<Vec<(PathBuf, PathBuf)>> = std::sync::OnceLock::new();
    DIRS.get_or_init(|| {
        use std::hash::{Hash, Hasher};
        let root = crate::profile::data_subdir("mods_unpacked").unwrap_or_else(|| std::env::temp_dir().join("nxport_mods_unpacked"));
        let mut out = Vec::new();
        for (name, dir) in mods() {
            let Some(nsp) = mod_file(dir, "exefs.nsp") else { continue };
            let unpack = || -> Result<PathBuf, String> {
                let meta = nsp.metadata().map_err(|e| e.to_string())?;
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (&nsp, meta.len(), meta.modified().ok()).hash(&mut h);
                let key = format!("{:016x}", h.finish());
                let dest = root.join(&key);
                if !dest.is_dir() {
                    let tmp = root.join(format!("{key}.{}.tmp", std::process::id()));
                    let _ = std::fs::remove_dir_all(&tmp);
                    packed_mods::unpack_pfs0(&nsp, &tmp)?;
                    // Another process may have got there first: its copy is the same.
                    if std::fs::rename(&tmp, &dest).is_err() && !dest.is_dir() {
                        return Err(format!("cannot create {}", dest.display()));
                    }
                    let _ = std::fs::remove_dir_all(&tmp);
                }
                Ok(dest)
            };
            match unpack() {
                Ok(d) => out.push((dir.clone(), d)),
                Err(e) => println!("[!] Mods: '{name}': exefs.nsp not used: {e}"),
            }
        }
        // What older versions of the mods unpacked.
        if !out.is_empty() {
            for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
                if !out.iter().any(|(_, d)| *d == e.path()) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        out
    })
}

pub(crate) fn mod_exefs_entry(dir: &Path, name: &str) -> ExefsEntry {
    let unpacked = unpacked_nsps().iter().find(|(d, _)| d == dir).map(|(_, u)| u.as_path());
    exefs_entry(&mod_subdir(dir, "exefs"), unpacked, name)
}

/// The file a mod in `dir` holds as ExeFS `name`, from its folder or its exefs.nsp.
pub fn mod_exefs_file(dir: &Path, name: &str) -> Option<PathBuf> {
    match mod_exefs_entry(dir, name) {
        ExefsEntry::File(p) => Some(p),
        _ => None,
    }
}

/// Whether the mod in `dir` holds a plugin module (`subsdk0`..`subsdk9`).
pub fn mod_has_plugin(dir: &Path) -> bool {
    (0..10).any(|i| mod_exefs_file(dir, &format!("subsdk{i}")).is_some())
}

/// Whether the game's own ExeFS file is out: a mod has a stub for it, or an exefs.nsp, which stands for the whole ExeFS.
pub(crate) fn game_exefs_removed(entries: &[ExefsEntry], any_nsp: bool) -> bool {
    any_nsp || entries.iter().any(|e| *e == ExefsEntry::Stub)
}

/// Whether the game's own module `name` stays out of the module map. Mods' modules of that name still load: they add up.
pub fn game_module_removed(name: &str) -> bool {
    let entries: Vec<ExefsEntry> = mods().iter().map(|(_, d)| mod_exefs_entry(d, name)).collect();
    game_exefs_removed(&entries, !unpacked_nsps().is_empty())
}

/// The file to load as ExeFS `name` (`main`, `sdk`), the mods' first, highest priority first; None if it was removed.
pub fn exefs_code_file(name: &str) -> Option<PathBuf> {
    for (m, d) in mods() {
        match mod_exefs_entry(d, name) {
            ExefsEntry::File(p) => {
                println!("[+] Mods: '{m}' replaces exefs/{name}");
                return Some(p);
            }
            ExefsEntry::Stub => {
                println!("[!] Mods: '{m}' removes exefs/{name}");
                return None;
            }
            ExefsEntry::Absent => {}
        }
    }
    if !unpacked_nsps().is_empty() {
        println!("[!] Mods: an exefs.nsp replaces the ExeFS and holds no {name}");
        return None;
    }
    Some(crate::game::exefs_file(name))
}

/// Atmosphere's patch sets under `root`: `[.../]exefs_patches/<set>/`, up to two folders down.
pub fn patch_sets(root: &Path, depth: u32) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for (name, path) in sorted_dirs(root) {
        if name.eq_ignore_ascii_case("exefs_patches") {
            out.extend(sorted_dirs(&path));
        } else if depth > 0 {
            out.extend(patch_sets(&path, depth - 1));
        }
    }
    out
}

/// Every folder of IPS and pchtxt files: the mods' exefs folders, then the patch sets.
pub fn exefs_patch_dirs() -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = mods().iter().map(|(n, d)| (n.clone(), mod_subdir(d, "exefs"))).collect();
    if crate::settings::get().files.no_mods {
        return v;
    }
    for root in mod_roots() {
        v.extend(patch_sets(root, 2));
    }
    v
}

/// The folders mods come from: --mods-dir (an app's built-in mods), else the game's data directory's mods folder. An
/// app with built-in mods takes the data directory's too, for mods added after the build.
pub fn mod_roots() -> &'static [PathBuf] {
    static ROOTS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| {
        let s = crate::settings::get();
        let data = crate::profile::data_subdir("mods").filter(|d| d.is_dir());
        let mut roots: Vec<PathBuf> = match s.files.mods_dir.clone() {
            Some(own) if crate::config::app_resources().is_some() => {
                [Some(own.clone()), data.filter(|d| *d != own)].into_iter().flatten().collect()
            }
            Some(own) => vec![own],
            None => data.into_iter().collect(),
        };
        // The game profile's own mods (patches only), in a folder of the engine's.
        if let Some(dir) = crate::profile::data_subdir("bundled_mods").filter(|d| crate::profile::write_bundled_mods(d)) {
            roots.push(dir);
        }
        roots
    })
}

/// Whether `path` lies in an app's built-in mods (lifted with the game's code on Windows), not one added after the build.
pub fn built_in_mod(path: &Path) -> bool {
    crate::config::app_resources().is_some()
        && crate::settings::get().files.mods_dir.as_ref().is_some_and(|own| path.starts_with(own))
}

/// Files a host's file manager leaves in folders, which a game listing a RomFS directory must not see.
pub(crate) fn is_host_junk(name: &str) -> bool {
    name == ".DS_Store" || name.starts_with("._") || name.eq_ignore_ascii_case("Thumbs.db") || name.eq_ignore_ascii_case("desktop.ini")
}

/// Names the game mounted its RomFS under (nn::fs::MountRom), besides the usual ones.
pub(crate) static ROM_MOUNTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The RomFS-relative part of "<rom mount>:/rest", if the path names a RomFS mount.
pub(crate) fn rom_mount_rel(clean_path: &str) -> Option<&str> {
    let (name, rest) = clean_path.split_once(':')?;
    (name == "rom" || name == "content" || ROM_MOUNTS.lock().unwrap().iter().any(|m| m == name)).then_some(rest)
}

/// Enabled mods, highest priority first: (name, mod directory). A mod directory has the Ryujinx / Atmosphere layout
/// (`romfs/` replaces or adds game files; `exefs/` holds code mods; `cheats/` Atmosphère cheats, guest/cheats). Mods
/// come from --mods-dir (a directory of mod folders, one mod, or Atmosphere's or Ryujinx's tree: discover_mods), else
/// ./mods if it exists; --no-mods disables them.
/// Priority is alphabetical (that is why mods use names like "!!!TOTK Optimizer"). A mod whose exefs/main.npdm names
/// another program is made for another game and skipped.
pub fn mods() -> &'static [(String, PathBuf)] {
    static MODS: std::sync::OnceLock<Vec<(String, PathBuf)>> = std::sync::OnceLock::new();
    MODS.get_or_init(|| {
        if crate::settings::get().files.no_mods {
            return Vec::new();
        }
        let mut v = all_mods();
        let disabled = &crate::settings::get().files.disabled_mods;
        v.retain(|(name, _)| {
            let off = disabled.contains(name);
            if off {
                println!("[+] Mods: '{name}' turned off");
            }
            !off
        });
        for (name, path) in &v {
            let parts: Vec<&str> = ["romfs", "exefs", "cheats"].into_iter().filter(|d| mod_subdir(path, d).is_dir()).collect();
            println!("[+] Mods: '{}' ({})", name, parts.join(" "));
            for packed in ["romfs.bin", "exefs.nsp"] {
                if mod_file(path, packed).is_some() {
                    println!("[+] Mods: '{name}': {packed}");
                }
            }
        }
        v
    })
}

/// Every mod in the mod folders, turned off or not, by name (the order of priority: alphabetical).
pub fn all_mods() -> Vec<(String, PathBuf)> {
    // A mod's main.npdm does not say which game it is for: Atmosphere overwrites its program ids with the running
    // program's (ldr_meta.cpp), and exlaunch's template names a fixed one. Which folder it is in decides (discover_mods).
    let mut v: Vec<(String, PathBuf)> = mod_roots().iter().flat_map(|r| discover_mods(r, crate::game::title_id())).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Puts the folders a mod carries for the SD card (UltraCam/TOTK/Config) on the game's SD card, for the mod's code to
/// find. A file already there stays: the mod or the player may have changed it, and higher priority mods come first.
pub fn seed_sd_card() {
    let sd = Path::new(save_root()).join("mount_sd");
    for (name, dir) in mods() {
        match seed_from_mod(dir, &sd) {
            Ok(0) => {}
            Ok(n) => println!("[+] Mods: '{name}': {n} files put on the SD card"),
            Err(e) => println!("[!] Mods: '{name}': its SD card files: {e}"),
        }
    }
}

pub(crate) fn seed_from_mod(dir: &Path, sd: &Path) -> std::io::Result<usize> {
    const NOT_FOR_SD: [&str; 3] = ["exefs", "romfs", "cheats"];
    let mut copied = 0;
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name();
        let wanted = !NOT_FOR_SD.iter().any(|n| name.to_string_lossy().eq_ignore_ascii_case(n));
        if wanted && entry.path().is_dir() {
            copied += copy_missing(&entry.path(), &sd.join(&name))?;
        }
    }
    Ok(copied)
}

pub(crate) fn copy_missing(from: &Path, to: &Path) -> std::io::Result<usize> {
    let mut copied = 0;
    for entry in std::fs::read_dir(from)?.flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copied += copy_missing(&entry.path(), &target)?;
        } else if !target.exists() {
            std::fs::create_dir_all(to)?;
            std::fs::copy(entry.path(), &target)?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// One source of RomFS files of a mod.
pub(crate) enum RomfsLayer {
    Dir(PathBuf),
    Image(RomfsImage),
}

/// The enabled mods' RomFS sources, highest priority first. Within a mod the folder comes before its romfs.bin, as
/// Atmosphere's layered RomFS builder adds them, and the first to add a path keeps it.
pub(crate) fn mod_romfs_layers() -> &'static [RomfsLayer] {
    static LAYERS: std::sync::OnceLock<Vec<RomfsLayer>> = std::sync::OnceLock::new();
    LAYERS.get_or_init(|| {
        let mut v = Vec::new();
        for (name, dir) in mods() {
            let root = mod_subdir(dir, "romfs");
            if root.is_dir() {
                v.push(RomfsLayer::Dir(root));
            }
            if let Some(bin) = mod_file(dir, "romfs.bin") {
                match RomfsImage::open(&bin) {
                    Ok(i) => v.push(RomfsLayer::Image(i)),
                    Err(e) => println!("[!] Mods: '{name}': romfs.bin not used: {e}"),
                }
            }
        }
        v
    })
}

#[cfg(test)]
mod mod_layout_tests {
    use super::*;

    const TOTK: u64 = 0x0100f2c0115b6000;

    fn tree(tag: &str, dirs: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nxport-mods-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in dirs {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        root
    }

    fn names(root: &Path) -> Vec<String> {
        let v = discover_mods(root, Some(TOTK));
        let names = v.iter().map(|(n, _)| n.clone()).collect();
        let _ = std::fs::remove_dir_all(root);
        names
    }

    #[test]
    fn a_folder_of_mods() {
        assert_eq!(names(&tree("list", &["b/romfs", "a/exefs", "c/RomFS"])), ["a", "b", "c"]);
    }

    #[test]
    fn the_root_is_one_mod() {
        let r = tree("one", &["romfs/Pack", "exefs"]);
        let got = discover_mods(&r, Some(TOTK));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, r);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn atmosphere_and_ryujinx_trees() {
        assert_eq!(names(&tree("atm", &["atmosphere/contents/0100F2C0115B6000/romfs"])), ["0100F2C0115B6000"]);
        assert_eq!(names(&tree("ryu", &["mods/contents/0100f2c0115b6000/A/romfs", "mods/contents/0100f2c0115b6000/B/exefs"])), ["A", "B"]);
    }

    #[test]
    fn a_download_holding_the_tree_keeps_its_name() {
        assert_eq!(names(&tree("dl", &["!First/atmosphere/contents/0100f2c0115b6000/romfs", "Second/romfs"])), ["!First", "Second"]);
    }

    #[test]
    fn other_titles_are_skipped() {
        assert_eq!(names(&tree("other", &["atmosphere/contents/01006f8002326000/romfs", "atmosphere/contents/0100f2c0115b6000/romfs"])), ["0100f2c0115b6000"]);
    }

    #[test]
    fn variants_inside_a_mod_do_not_all_turn_on() {
        assert!(names(&tree("var", &["Mod/Option A/romfs", "Mod/Option B/romfs"])).is_empty());
    }

    #[test]
    fn patch_sets_are_found_where_atmosphere_keeps_them() {
        let r = tree("sets", &["atmosphere/exefs_patches/60fps", "exefs_patches/other", "Mod/romfs/exefs_patches_not"]);
        let got: Vec<String> = patch_sets(&r, 2).into_iter().map(|x| x.0).collect();
        let _ = std::fs::remove_dir_all(&r);
        assert_eq!(got, ["60fps", "other"]);
    }

    #[test]
    fn sd_card_gets_a_mods_other_folders_and_keeps_its_files() {
        let r = tree("sdseed", &["m/UltraCam/Config", "m/exefs", "m/RomFS", "sd/UltraCam/Config"]);
        std::fs::write(r.join("m/UltraCam/Config/new.ini"), "mod").unwrap();
        std::fs::write(r.join("m/UltraCam/Config/kept.ini"), "mod").unwrap();
        std::fs::write(r.join("m/exefs/subsdk3"), "code").unwrap();
        std::fs::write(r.join("m/RomFS/file"), "data").unwrap();
        std::fs::write(r.join("m/loose.txt"), "text").unwrap();
        std::fs::write(r.join("sd/UltraCam/Config/kept.ini"), "player").unwrap();
        assert_eq!(seed_from_mod(&r.join("m"), &r.join("sd")).unwrap(), 1);
        let read = |p: &str| std::fs::read_to_string(r.join("sd").join(p)).unwrap();
        assert_eq!(read("UltraCam/Config/new.ini"), "mod");
        assert_eq!(read("UltraCam/Config/kept.ini"), "player");
        assert!(!r.join("sd/exefs").exists() && !r.join("sd/RomFS").exists() && !r.join("sd/loose.txt").exists());
        assert_eq!(seed_from_mod(&r.join("m"), &r.join("sd")).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn mod_subdir_finds_any_case() {
        let r = tree("case", &["RomFS"]);
        assert!(mod_subdir(&r, "romfs").is_dir());
        assert!(!mod_subdir(&r, "exefs").is_dir());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn packed_forms_make_a_mod() {
        let r = tree("packed", &["a", "b", "c", "d"]);
        std::fs::write(r.join("a/exefs.nsp"), []).unwrap();
        std::fs::write(r.join("b/ROMFS.BIN"), []).unwrap();
        std::fs::write(r.join("c/other.bin"), []).unwrap();
        let got: Vec<String> = discover_mods(&r, Some(TOTK)).into_iter().map(|x| x.0).collect();
        assert_eq!(got, ["a", "b"]);
        assert!(has_exefs_nsp(&r.join("a")) && !has_exefs_nsp(&r.join("b")));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn exefs_file_then_stub_then_nsp() {
        let r = tree("exefs", &["exefs", "out"]);
        std::fs::write(r.join("nsp"), packed_mods::tests::pfs0(&[("main", b"nsp main"), ("sdk", b"nsp sdk"), ("subsdk0", b"nsp 0")])).unwrap();
        packed_mods::unpack_pfs0(&r.join("nsp"), &r.join("out")).unwrap();
        let (x, out) = (r.join("exefs"), r.join("out"));
        std::fs::write(x.join("main"), b"loose main").unwrap();
        std::fs::write(x.join("main.stub"), b"").unwrap();
        std::fs::write(x.join("sdk.stub"), b"").unwrap();
        // A file beats its stub; a stub hides the nsp's file; the nsp's file stands otherwise.
        assert_eq!(exefs_entry(&x, Some(&out), "main"), ExefsEntry::File(x.join("main")));
        assert_eq!(exefs_entry(&x, Some(&out), "sdk"), ExefsEntry::Stub);
        assert_eq!(exefs_entry(&x, Some(&out), "subsdk0"), ExefsEntry::File(out.join("subsdk0")));
        assert_eq!(std::fs::read(out.join("subsdk0")).unwrap(), b"nsp 0");
        assert_eq!(exefs_entry(&x, Some(&out), "subsdk1"), ExefsEntry::Absent);
        assert_eq!(exefs_entry(&x, None, "subsdk0"), ExefsEntry::Absent);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn the_game_keeps_a_module_unless_stubbed_or_replaced() {
        assert!(!game_exefs_removed(&[ExefsEntry::Absent, ExefsEntry::File("x".into())], false));
        assert!(game_exefs_removed(&[ExefsEntry::Absent, ExefsEntry::Stub], false));
        assert!(game_exefs_removed(&[ExefsEntry::Absent], true));
        assert!(!game_exefs_removed(&[], false));
    }

    #[test]
    fn romfs_layers_merge_by_priority() {
        let r = tree("layers", &["high/Pack", "low/Pack", "game/Pack", "game/Gone"]);
        std::fs::write(r.join("high/Pack/both"), b"folder").unwrap();
        std::fs::write(r.join("low/Pack/low_only"), b"low").unwrap();
        std::fs::write(r.join("game/Pack/both"), b"the game").unwrap();
        std::fs::write(r.join("game/Pack/game_only"), b"game").unwrap();
        std::fs::write(r.join("image.bin"), packed_mods::tests::romfs(&[("Pack/both", b"image"), ("Pack/in_image", b"12345"), ("Pack/low_only", b"image too"), ("Pack/z.zs", b"zz"), ("New/f", b"f")])).unwrap();
        let image = RomfsLayer::Image(RomfsImage::open(&r.join("image.bin")).unwrap());
        let layers = [RomfsLayer::Dir(r.join("high")), image, RomfsLayer::Dir(r.join("low"))];

        // The folder above the image wins; the image wins over the folder below it.
        assert!(image_entry_in(&layers, "Pack/both").is_none());
        let bytes = std::fs::read(r.join("image.bin")).unwrap();
        let in_image = |rel: &str| match image_entry_in(&layers, rel) {
            Some(ImageEntry::File(_, off, size, zs)) => (bytes[off as usize..][..size as usize].to_vec(), zs),
            _ => panic!("{rel} is not a file of the image"),
        };
        assert_eq!(in_image("/Pack/low_only"), (b"image too".to_vec(), false));
        assert_eq!(in_image("Pack/in_image"), (b"12345".to_vec(), false));
        // The game's .zs fallback, and the .zs asked for by name.
        assert_eq!(in_image("Pack/z"), (b"zz".to_vec(), true));
        assert_eq!(in_image("Pack/z.zs"), (b"zz".to_vec(), false));
        assert!(matches!(image_entry_in(&layers, "New"), Some(ImageEntry::Dir)));
        // What only the game has stays with the host paths.
        assert!(image_entry_in(&layers, "Pack/game_only").is_none());
        assert!(image_entry_in(&[], "Pack/both").is_none());

        let listing = |rel: &str| {
            let mut v = merged_listing(&romfs_dirs_in(&layers, &r.join("game"), rel)).unwrap();
            v.sort();
            v
        };
        let sizes: Vec<(String, u64)> = listing("Pack").into_iter().map(|(n, m)| (n, m.unwrap().1)).collect();
        let want = [("both", 6), ("game_only", 4), ("in_image", 5), ("low_only", 9), ("z.zs", 2)];
        assert_eq!(sizes, want.map(|(n, s)| (n.to_string(), s)));
        assert_eq!(listing("/").iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Gone", "New", "Pack"]);
        assert!(merged_listing(&romfs_dirs_in(&layers, &r.join("game"), "Nowhere")).is_none());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn rom_mounts_by_name() {
        assert_eq!(rom_mount_rel("rom:/a"), Some("/a"));
        assert_eq!(rom_mount_rel("content:/a"), Some("/a"));
        assert_eq!(rom_mount_rel("zz_test:/a"), None);
        ROM_MOUNTS.lock().unwrap().push("zz_test".into());
        assert_eq!(rom_mount_rel("zz_test:/a"), Some("/a"));
        assert_eq!(rom_mount_rel("no/mount"), None);
    }
}
