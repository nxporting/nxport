// Horizon OS High-Level Emulation for File System (nn::fs)
// Maps RomFS and game paths directly to the local dumped assets.

use std::ffi::CStr;
use std::fs::File;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::packed_mods::{self, ImageDirEntry, RomfsImage};

/// pread: reads at `off` (Windows also moves the file position, which nothing here uses).
fn read_at(f: &File, buf: &mut [u8], off: u64) -> std::io::Result<usize> {
    #[cfg(unix)]
    return std::os::unix::fs::FileExt::read_at(f, buf, off);
    #[cfg(windows)]
    return std::os::windows::fs::FileExt::seek_read(f, buf, off);
}

/// pwrite, as `read_at`.
fn write_at(f: &File, buf: &[u8], off: u64) -> std::io::Result<usize> {
    #[cfg(unix)]
    return std::os::unix::fs::FileExt::write_at(f, buf, off);
    #[cfg(windows)]
    return std::os::windows::fs::FileExt::seek_write(f, buf, off);
}

/// The OS could not write the destination buffer (EFAULT; Windows: ERROR_NOACCESS).
fn is_buffer_fault(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    return e.raw_os_error() == Some(libc::EFAULT);
    #[cfg(windows)]
    return e.raw_os_error() == Some(998);
}

fn romfs_root() -> &'static std::path::Path {
    crate::game::romfs()
}
// Writable save-data mounts ("save:", "data:") are backed by this host directory.

/// Host directory behind the "save:" / "data:" mounts: the game's data directory's save/ unless --save-dir names one.
fn default_save_dir() -> String {
    let dir = crate::profile::data_subdir("save").unwrap_or_else(|| "save".into());
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[!] save directory {}: {e}", dir.display());
    }
    dir.to_string_lossy().into_owned()
}

fn save_root() -> &'static str {
    static ROOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        if let Some(dir) = &crate::settings::get().files.save_dir {
            return dir.display().to_string();
        }
        default_save_dir()
    })
}

// nn::fs Results (module 2): raw = module | (description << 9).
const RESULT_PATH_NOT_FOUND: i32 = 0x202; // 2002-0001
const RESULT_PATH_ALREADY_EXISTS: i32 = 0x402; // 2002-0002
const RESULT_TARGET_NOT_FOUND: i32 = 2 | (1002 << 9); // 2002-1002

/// Mount names the guest registered through the writable mount calls (MountSaveData & co). "save"/"data" are
/// always writable; any other name the game mounts (e.g. "prsv") gets its own subdirectory of the save root.
static EXTRA_MOUNTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub(crate) unsafe fn register_writable_mount(kind: &str, name: *const c_char) {
    if name.is_null() {
        return;
    }
    let name = std::ffi::CStr::from_ptr(name)
        .to_string_lossy()
        .into_owned();
    println!("[+] HLE: nn::fs::{} name='{}'", kind, name);
    if name != "save" && name != "data" {
        // A mounted storage's root exists on the console. Without it TotK's CreateDirectory('savc:/icon') at mount time
        // failed, and saving the game later retried CreateFile('savc:/icon/...png') forever (the save hung).
        let root = Path::new(save_root()).join(format!("mount_{}", name));
        if let Err(e) = std::fs::create_dir_all(&root) {
            println!(
                "[!] HLE: nn::fs::{}: cannot create the mount root {}: {}",
                kind,
                root.display(),
                e
            );
        }
        if name == "sd" {
            // Seed UltraCam config if present in mods/
            let target_cfg = root.join("UltraCam/TOTK/Config/maxlastbreath.ini");
            if !target_cfg.exists() {
                for (_, mod_path) in mods() {
                    let src_cfg = mod_subdir(mod_path, "romfs").join("UltraCam/maxlastbreath.ini");
                    if src_cfg.exists() {
                        if let Some(parent) = target_cfg.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Ok(_) = std::fs::copy(&src_cfg, &target_cfg) {
                            println!(
                                "[+] HLE: seeded UltraCam config to {}",
                                target_cfg.display()
                            );
                        }
                        break;
                    }
                }
            }
        }
        let mut mounts = EXTRA_MOUNTS.lock().unwrap();
        if !mounts.contains(&name) {
            mounts.push(name);
        }
    }
}

/// Read-only mounts of host directories (system data): mount name -> directory.
static READ_ONLY_MOUNTS: Mutex<Vec<(String, PathBuf)>> = Mutex::new(Vec::new());

/// Splits "<mount>:/rest" into (directory, rest) for a read-only mount.
fn read_only_mount_of(guest_path: &str) -> Option<(PathBuf, String)> {
    let p = guest_path.trim_start_matches('/');
    let (name, rest) = p.split_once(':')?;
    READ_ONLY_MOUNTS
        .lock()
        .unwrap()
        .iter()
        .find(|(m, _)| m == name)
        .map(|(_, dir)| (dir.clone(), rest.to_string()))
}

/// Splits "<extra mount>:/rest" into (mount name, rest) when the mount was registered as writable.
fn extra_mount_of(guest_path: &str) -> Option<(String, String)> {
    let p = guest_path.trim_start_matches('/');
    let (name, rest) = p.split_once(':')?;
    EXTRA_MOUNTS
        .lock()
        .unwrap()
        .iter()
        .any(|m| m == name)
        .then(|| (name.to_string(), rest.to_string()))
}

fn is_save_path(guest_path: &str) -> bool {
    let p = guest_path.trim_start_matches('/');
    p.starts_with("save:")
        || p.starts_with("data:")
        || p.starts_with("sd:")
        || extra_mount_of(guest_path).is_some()
}

fn errno_to_result(err: &std::io::Error) -> i32 {
    match err.kind() {
        std::io::ErrorKind::AlreadyExists => RESULT_PATH_ALREADY_EXISTS,
        _ => RESULT_PATH_NOT_FOUND,
    }
}

enum FileData {
    Fd(File),
    Buf(Vec<u8>),
}

struct OpenFile {
    data: FileData,
    #[allow(dead_code)]
    path: String,
    size: i64,
    /// Where it starts in `data`'s file: non-zero for a file inside a mod's romfs.bin (opened read-only, so writes fail).
    base: u64,
}

static OPEN_FILES: Mutex<Vec<Option<OpenFile>>> = Mutex::new(Vec::new());

/// A mod's `romfs` or `exefs` folder, whatever its case: mods ship both `romfs` and `RomFS`, and iOS's file system is
/// case sensitive.
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
fn mod_file(dir: &Path, name: &str) -> Option<PathBuf> {
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
    mod_subdir(dir, "romfs").is_dir() || mod_subdir(dir, "exefs").is_dir() || mod_file(dir, "romfs.bin").is_some() || has_exefs_nsp(dir)
}

/// Folders mods are packaged in on the way to their contents: `atmosphere/contents/<title id>/`, Ryujinx's
/// `mods/contents/<title id>/`, or a bare `<title id>/`.
fn is_layout_dir(name: &str) -> bool {
    ["atmosphere", "contents", "mods"].iter().any(|d| name.eq_ignore_ascii_case(d)) || title_dir(name).is_some()
}

fn title_dir(name: &str) -> Option<u64> {
    (name.len() == 16).then(|| u64::from_str_radix(name, 16).ok()).flatten()
}

fn sorted_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
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
                println!("[!] Mods: '{name}' has no romfs or exefs folder; ignored");
            }
        }
    }
    out
}

/// What one mod says of an ExeFS file.
#[derive(Debug, PartialEq)]
enum ExefsEntry {
    File(PathBuf),
    /// `<name>.stub`: the file is removed from what lies below.
    Stub,
    Absent,
}

/// As Atmosphere's SdCardRedirectionCodeFileSystem: a file in the mod's exefs folder, else its stub, else the exefs.nsp's
/// file (`unpacked`, a folder).
fn exefs_entry(exefs: &Path, unpacked: Option<&Path>, name: &str) -> ExefsEntry {
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
fn unpacked_nsps() -> &'static [(PathBuf, PathBuf)] {
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

fn mod_exefs_entry(dir: &Path, name: &str) -> ExefsEntry {
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
fn game_exefs_removed(entries: &[ExefsEntry], any_nsp: bool) -> bool {
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
    if let Some(root) = crate::settings::get().files.mods_dir.clone().or_else(|| crate::profile::data_subdir("mods")) {
        v.extend(patch_sets(&root, 2));
    }
    v
}

/// Files a host's file manager leaves in folders, which a game listing a RomFS directory must not see.
fn is_host_junk(name: &str) -> bool {
    name == ".DS_Store" || name.starts_with("._") || name.eq_ignore_ascii_case("Thumbs.db") || name.eq_ignore_ascii_case("desktop.ini")
}

/// Names the game mounted its RomFS under (nn::fs::MountRom), besides the usual ones.
static ROM_MOUNTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The RomFS-relative part of "<rom mount>:/rest", if the path names a RomFS mount.
fn rom_mount_rel(clean_path: &str) -> Option<&str> {
    let (name, rest) = clean_path.split_once(':')?;
    (name == "rom" || name == "content" || ROM_MOUNTS.lock().unwrap().iter().any(|m| m == name)).then_some(rest)
}

/// Enabled mods, highest priority first: (name, mod directory). A mod directory has the Ryujinx / Atmosphere layout
/// (`romfs/` replaces or adds game files; `exefs/` holds code mods). Mods come from --mods-dir (a directory of mod
/// folders, one mod, or Atmosphere's or Ryujinx's tree: discover_mods), else ./mods if it exists; --no-mods disables them.
/// Priority is alphabetical (that is why mods use names like "!!!TOTK Optimizer"). A mod whose exefs/main.npdm names
/// another program is made for another game and skipped.
pub fn mods() -> &'static [(String, PathBuf)] {
    static MODS: std::sync::OnceLock<Vec<(String, PathBuf)>> = std::sync::OnceLock::new();
    MODS.get_or_init(|| {
        if crate::settings::get().files.no_mods {
            return Vec::new();
        }
        let Some(root) = crate::settings::get().files.mods_dir.clone().or_else(|| crate::profile::data_subdir("mods")) else {
            return Vec::new();
        };
        // A mod's main.npdm does not say which game it is for: Atmosphere overwrites its program ids with the running
        // program's (ldr_meta.cpp), and exlaunch's template names a fixed one. Which folder it is in decides (discover_mods).
        let mut v = discover_mods(&root, crate::game::title_id());
        v.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path) in &v {
            println!("[+] Mods: '{}' ({}{})", name, if mod_subdir(path, "romfs").is_dir() { "romfs " } else { "" }, if mod_subdir(path, "exefs").is_dir() { "exefs" } else { "" });
            for packed in ["romfs.bin", "exefs.nsp"] {
                if mod_file(path, packed).is_some() {
                    println!("[+] Mods: '{name}': {packed}");
                }
            }
        }
        v
    })
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

fn seed_from_mod(dir: &Path, sd: &Path) -> std::io::Result<usize> {
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

fn copy_missing(from: &Path, to: &Path) -> std::io::Result<usize> {
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
enum RomfsLayer {
    Dir(PathBuf),
    Image(RomfsImage),
}

/// The enabled mods' RomFS sources, highest priority first. Within a mod the folder comes before its romfs.bin, as
/// Atmosphere's layered RomFS builder adds them, and the first to add a path keeps it.
fn mod_romfs_layers() -> &'static [RomfsLayer] {
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

/// What a RomFS path finds in a romfs.bin.
enum ImageEntry<'a> {
    Dir,
    /// The image, where the file lies in it, its size, and whether the path found its `.zs` (OpenFile's fallback).
    File(&'a RomfsImage, u64, u64, bool),
}

/// `rel` in the first of `layers` that has it, if that is an image. Otherwise the host paths below apply.
fn image_entry_in<'a>(layers: &'a [RomfsLayer], rel: &str) -> Option<ImageEntry<'a>> {
    let rel = rel.trim_start_matches('/');
    let zs = format!("{rel}.zs");
    for layer in layers {
        match layer {
            RomfsLayer::Dir(root) => {
                if root.join(rel).exists() || root.join(&zs).exists() {
                    return None;
                }
            }
            RomfsLayer::Image(i) => {
                if i.dir(rel).is_some() {
                    return Some(ImageEntry::Dir);
                }
                if let Some((off, size)) = i.file(rel) {
                    return Some(ImageEntry::File(i, off, size, false));
                }
                if let Some((off, size)) = i.file(&zs) {
                    return Some(ImageEntry::File(i, off, size, true));
                }
            }
        }
    }
    None
}

/// What a guest path finds in a mod's romfs.bin. None without such files, whatever the path.
fn image_entry(guest_path: &str) -> Option<ImageEntry<'static>> {
    let layers = mod_romfs_layers();
    if !layers.iter().any(|l| matches!(l, RomfsLayer::Image(_))) {
        return None;
    }
    image_entry_in(layers, romfs_rel(guest_path)?)
}

/// A RomFS-relative path resolved through the mods first (the game's own .zs fallback applies to mod files too). Host
/// paths only: what a romfs.bin holds is found by image_entry first. (The movie player opens files by host path, so it
/// does not see a movie in a romfs.bin.)
fn romfs_path(rel: &str) -> PathBuf {
    let rel = rel.trim_start_matches('/');
    for layer in mod_romfs_layers() {
        let RomfsLayer::Dir(root) = layer else { continue };
        let p = root.join(rel);
        let zs = PathBuf::from(format!("{}.zs", p.display()));
        if p.exists() || zs.exists() {
            return p;
        }
    }
    romfs_root().join(rel)
}

/// A folder to list, in a RomFS merge or on its own.
enum ListedDir<'a> {
    Host(PathBuf),
    Image(&'a [ImageDirEntry]),
}

/// Everywhere a RomFS-relative directory path exists in `layers` (highest priority first), then `game`'s.
fn romfs_dirs_in<'a>(layers: &'a [RomfsLayer], game: &Path, rel: &str) -> Vec<ListedDir<'a>> {
    let rel = rel.trim_start_matches('/');
    let mut v: Vec<ListedDir> = layers
        .iter()
        .filter_map(|l| match l {
            RomfsLayer::Dir(r) => Some(r.join(rel)).filter(|p| p.is_dir()).map(ListedDir::Host),
            RomfsLayer::Image(i) => i.dir(rel).map(ListedDir::Image),
        })
        .collect();
    v.push(ListedDir::Host(game.join(rel)));
    v
}

/// What `dirs` hold, the first of a name winning: (name, whether a folder and size, if known). None if none could be read.
fn merged_listing(dirs: &[ListedDir]) -> Option<Vec<(String, Option<(bool, u64)>)>> {
    let (mut found, mut seen, mut out) = (false, std::collections::HashSet::new(), Vec::new());
    for d in dirs {
        match d {
            ListedDir::Host(p) => {
                let Ok(rd) = std::fs::read_dir(p) else { continue };
                found = true;
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if seen.insert(name.clone()) {
                        out.push((name, e.metadata().ok().map(|m| (m.is_dir(), m.len()))));
                    }
                }
            }
            ListedDir::Image(entries) => {
                found = true;
                for e in *entries {
                    if seen.insert(e.name.clone()) {
                        out.push((e.name.clone(), Some((e.is_dir, e.size))));
                    }
                }
            }
        }
    }
    found.then_some(out)
}

/// The RomFS-relative part of a guest path, if it names the game's RomFS.
fn romfs_rel(guest_path: &str) -> Option<&str> {
    let clean_path = guest_path.trim_start_matches('/');
    if let Some(rel) = rom_mount_rel(clean_path) {
        return Some(rel);
    }
    if clean_path.starts_with("data:")
        || clean_path.starts_with("save:")
        || clean_path.starts_with("sd:")
        || extra_mount_of(clean_path).is_some()
        || read_only_mount_of(clean_path).is_some()
    {
        return None;
    }
    Some(clean_path)
}

/// resolve_path for a reader that needs a real file (the movie decoder): a file in a romfs.bin is copied out once, to the system's/// temporary folder, as nothing but this one reader needs it there.
pub(crate) fn resolve_to_file(guest_path: &str) -> Option<PathBuf> {
    let Some(ImageEntry::File(image, off, size, zs)) = image_entry(guest_path) else {
        return resolve_path(guest_path);
    };
    use std::hash::{Hash, Hasher};
    use std::io::{Read, Seek};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let changed = std::fs::metadata(&image.path).and_then(|m| m.modified()).ok();
    (&image.path, off, size, changed).hash(&mut h);
    let name = Path::new(guest_path).file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let out = std::env::temp_dir().join(format!("nxport-{:016x}-{name}", h.finish()));
    if !out.is_file() {
        let copied = (|| -> std::io::Result<()> {
            let mut f = File::open(&image.path)?;
            f.seek(std::io::SeekFrom::Start(off))?;
            let tmp = out.with_extension("part");
            let mut to = File::create(&tmp)?;
            if zs {
                zstd::stream::copy_decode(f.take(size), &mut to)?;
            } else {
                std::io::copy(&mut f.take(size), &mut to)?;
            }
            drop(to);
            std::fs::rename(&tmp, &out)
        })();
        if let Err(e) = copied {
            println!("[!] Mods: {guest_path} from a romfs.bin: {e}");
            return None;
        }
    }
    Some(out)
}

pub(crate) fn resolve_path(guest_path: &str) -> Option<PathBuf> {
    // Paths like "content://Sound/x" (double slash) occur; Path::join with a leading "/" would replace
    // the root entirely, so strip every leading slash after the mount name.
    let clean_path = guest_path.trim_start_matches('/');
    if let Some(rel) = rom_mount_rel(clean_path) {
        Some(romfs_path(rel))
    } else if let Some(rel) = clean_path
        .strip_prefix("data:")
        .or_else(|| clean_path.strip_prefix("save:"))
    {
        Some(Path::new(save_root()).join(rel.trim_start_matches('/')))
    } else if let Some(rel) = clean_path.strip_prefix("sd:") {
        Some(
            Path::new(save_root())
                .join("mount_sd")
                .join(rel.trim_start_matches('/')),
        )
    } else if let Some((dir, rel)) = read_only_mount_of(clean_path) {
        Some(dir.join(rel.trim_start_matches('/')))
    } else if let Some((mount, rel)) = extra_mount_of(clean_path) {
        Some(
            Path::new(save_root())
                .join(format!("mount_{}", mount))
                .join(rel.trim_start_matches('/')),
        )
    } else {
        Some(romfs_path(clean_path))
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_QueryMountRomCacheSize(out_size: *mut usize) -> i32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_MountRom(
    name: *const c_char,
    _cache_buf: *mut (),
    _cache_size: usize,
) -> i32 {
    let name_str = if !name.is_null() {
        CStr::from_ptr(name).to_string_lossy().into_owned()
    } else {
        "unknown".to_string()
    };
    {
        let mut mounts = ROM_MOUNTS.lock().unwrap();
        if !mounts.contains(&name_str) {
            mounts.push(name_str.clone());
        }
    }
    println!(
        "[+] HLE: nn::fs::MountRom('{}') -> mapped to '{}'",
        name_str,
        romfs_root().display()
    );
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_OpenFile(
    out_handle: *mut u64,
    path: *const c_char,
    mode: i32,
) -> i32 {
    if path.is_null() || out_handle.is_null() {
        return RESULT_PATH_NOT_FOUND;
    }
    let path_str = CStr::from_ptr(path).to_string_lossy();
    let host_path = match resolve_path(&path_str) {
        Some(p) => p,
        None => {
            eprintln!(
                "[-] HLE: nn::fs::OpenFile failed to resolve path: '{}'",
                path_str
            );
            return RESULT_PATH_NOT_FOUND;
        }
    };

    if is_save_path(&path_str) {
        // Save data is plain host files: no .zs fallback, writable when OpenMode_Write (2) is set.
        let f = match std::fs::OpenOptions::new().read(true).write(mode & 2 != 0).open(&host_path) {
            Ok(f) => f,
            Err(err) => {
                log_fs_miss(
                    "OpenFile(save)",
                    &path_str,
                    &format!("mode={} -> {}", mode, err),
                );
                return errno_to_result(&err);
            }
        };
        let size = f.metadata().map_or(0, |m| m.len() as i64);
        let mut files = OPEN_FILES.lock().unwrap();
        files.push(Some(OpenFile {
            data: FileData::Fd(f),
            path: path_str.to_string(),
            size,
            base: 0,
        }));
        *out_handle = files.len() as u64;
        println!(
            "[+] HLE: nn::fs::OpenFile (save) handle={} '{}' mode={} ({} bytes)",
            files.len(),
            path_str,
            mode,
            size
        );
        return 0;
    }

    match image_entry(&path_str) {
        Some(ImageEntry::Dir) => return 0x00202602, // ResultTargetIsDirectory, as below
        Some(ImageEntry::File(image, off, size, zs)) => return open_image_file(out_handle, &path_str, image, off, size, zs),
        None => {}
    }

    // Try the bare path first, then fall back to the .zs compressed variant
    // (TotK's RomFS dump stores all files with an extra .zs suffix).
    let candidates = [
        host_path.clone(),
        host_path.with_extension(
            host_path
                .extension()
                .map(|e| format!("{}.zs", e.to_string_lossy()))
                .unwrap_or_else(|| "zs".to_string()),
        ),
    ];

    // Checked before opening: Windows cannot open a directory as a file at all.
    if host_path.is_dir() {
        println!(
            "[-] HLE: nn::fs::OpenFile rejected directory: '{}' -> ResultTargetIsDirectory (0x00202602)",
            path_str
        );
        return 0x00202602;
    }

    let mut opened = Err(std::io::ErrorKind::NotFound.into());
    let mut chosen_path = host_path.clone();
    for candidate in &candidates {
        opened = File::open(candidate);
        if opened.is_ok() {
            chosen_path = candidate.clone();
            break;
        }
    }
    let f = match opened {
        Ok(f) => f,
        Err(err) => {
            println!(
                "[-] HLE: nn::fs::OpenFile NOT FOUND: '{}' -> '{}' ({err})",
                path_str,
                host_path.display()
            );
            return RESULT_PATH_NOT_FOUND; // PathNotFound Result
        }
    };

    // Decompress only when we did a transparent fallback: the game requested a
    // plain file but on disk it only exists as a .zs archive.  If the game
    // explicitly asked for the .zs file it will decompress it itself via its
    // built-in Zstandard decompressor (0xa7e768).
    let used_zs_fallback =
        chosen_path != candidates[0] && chosen_path.extension().map(|e| e == "zs").unwrap_or(false);

    let (file_data, size) = if used_zs_fallback {
        let mut decompressed = Vec::new();
        match zstd::stream::copy_decode(f, &mut decompressed) {
            Ok(_) => {
                let sz = decompressed.len() as i64;
                println!(
                    "[+] HLE: nn::fs::OpenFile (zstd) '{}' -> '{}' (decompressed {} bytes)",
                    path_str,
                    chosen_path.display(),
                    sz
                );
                (FileData::Buf(decompressed), sz)
            }
            Err(e) => {
                eprintln!(
                    "[-] HLE: nn::fs::OpenFile zstd decode failed for '{}': {e}",
                    chosen_path.display()
                );
                return RESULT_PATH_NOT_FOUND;
            }
        }
    } else {
        let sz = f.metadata().map_or(0, |m| m.len() as i64);
        (FileData::Fd(f), sz)
    };

    let mut files = OPEN_FILES.lock().unwrap();
    let handle_idx = files.len();
    files.push(Some(OpenFile {
        data: file_data,
        path: path_str.to_string(),
        size,
        base: 0,
    }));

    let handle_id = (handle_idx + 1) as u64;
    *out_handle = handle_id;
    println!(
        "[+] HLE: nn::fs::OpenFile handle={} path='{}' -> '{}' ({} bytes)",
        handle_id,
        path_str,
        chosen_path.display(),
        size
    );
    0
}

/// OpenFile of a file in a mod's romfs.bin, with its fallback: a path that found only `<path>.zs` reads decompressed.
unsafe fn open_image_file(out_handle: *mut u64, path: &str, image: &RomfsImage, off: u64, size: u64, zs: bool) -> i32 {
    let opened = File::open(&image.path).and_then(|mut f| {
        if !zs {
            return Ok((FileData::Fd(f), size, off));
        }
        use std::io::{Read, Seek};
        f.seek(std::io::SeekFrom::Start(off))?;
        let mut out = Vec::new();
        zstd::stream::copy_decode(f.take(size), &mut out)?;
        let n = out.len() as u64;
        Ok((FileData::Buf(out), n, 0))
    });
    let (data, size, base) = match opened {
        Ok(o) => o,
        Err(e) => {
            println!("[-] HLE: nn::fs::OpenFile '{path}' in {}: {e}", image.path.display());
            return RESULT_PATH_NOT_FOUND;
        }
    };
    let mut files = OPEN_FILES.lock().unwrap();
    files.push(Some(OpenFile { data, path: path.to_string(), size: size as i64, base }));
    *out_handle = files.len() as u64;
    println!("[+] HLE: nn::fs::OpenFile handle={} path='{path}' -> {}+{off:#x} ({size} bytes)", files.len(), image.path.display());
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_GetFileSize(out_size: *mut i64, handle: u64) -> i32 {
    if handle == 0 || out_size.is_null() {
        return RESULT_PATH_NOT_FOUND;
    }
    let idx = (handle - 1) as usize;
    let files = OPEN_FILES.lock().unwrap();
    if let Some(Some(entry)) = files.get(idx) {
        *out_size = entry.size;
        0
    } else {
        eprintln!("[-] HLE: nn::fs::GetFileSize invalid handle {}", handle);
        RESULT_PATH_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_ReadFile(
    out_bytes_read: *mut usize,
    handle: u64,
    offset: i64,
    buffer: *mut u8,
    size: usize,
    _read_option: *const (),
) -> i32 {
    if handle == 0 || buffer.is_null() {
        return RESULT_PATH_NOT_FOUND;
    }
    let idx = (handle - 1) as usize;
    let files = OPEN_FILES.lock().unwrap();
    if let Some(Some(entry)) = files.get(idx) {
        let bytes_read = match &entry.data {
            FileData::Fd(f) => {
                // The kernel cannot write write-tracked (read-only) pages: unprotect them first, and retry if the
                // executor re-protected them in between (a buffer fault).
                // A file inside a romfs.bin ends at its size, not the image's end.
                let size = if entry.base != 0 { size.min((entry.size - offset).max(0) as usize) } else { size };
                crate::os::ensure_committed(buffer as usize, size);
                let dst = std::slice::from_raw_parts_mut(buffer, size);
                let mut n = Err(std::io::ErrorKind::Other.into());
                for _ in 0..4 {
                    crate::guest::dirty_track::before_host_write(buffer as usize, size);
                    n = read_at(f, dst, entry.base + offset as u64);
                    if !n.as_ref().is_err_and(is_buffer_fault) {
                        break;
                    }
                }
                match n {
                    Ok(n) => n,
                    Err(err) => {
                        eprintln!(
                            "[-] HLE: nn::fs::ReadFile error reading handle {}: {err}",
                            handle
                        );
                        return RESULT_PATH_NOT_FOUND;
                    }
                }
            }
            FileData::Buf(buf) => {
                let off = offset as usize;
                if off >= buf.len() {
                    0
                } else {
                    let available = buf.len() - off;
                    let n = size.min(available);
                    std::ptr::copy_nonoverlapping(buf.as_ptr().add(off), buffer, n);
                    n
                }
            }
        };
        if !out_bytes_read.is_null() {
            *out_bytes_read = bytes_read;
        }
        println!(
            "[+] HLE: nn::fs::ReadFile handle={} '{}' off={} sz={} -> read={} buf={:p}",
            handle, entry.path, offset, size, bytes_read, buffer
        );
        // Verify ZSTD magic for .zs files (0xFD2FB528 little-endian = 28 B5 2F FD) at offset 0
        if entry.path.ends_with(".zs") && bytes_read >= 4 && offset == 0 {
            let magic = unsafe { *(buffer as *const u32) };
            if magic == 0xFD2FB528 {
                println!("[+]   ZSTD magic OK @ {:p}", buffer);
            } else {
                eprintln!(
                    "[-]   ZSTD magic MISSING @ {:p}: got 0x{:08x}",
                    buffer, magic
                );
            }
        }
        0
    } else {
        eprintln!("[-] HLE: nn::fs::ReadFile invalid handle {}", handle);
        RESULT_PATH_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_ReadFile_no_option(
    out_bytes_read: *mut usize,
    handle: u64,
    offset: i64,
    buffer: *mut u8,
    size: usize,
) -> i32 {
    hle_nn_fs_ReadFile(
        out_bytes_read,
        handle,
        offset,
        buffer,
        size,
        std::ptr::null(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_ReadFile_simple(
    handle: u64,
    offset: i64,
    buffer: *mut u8,
    size: usize,
) -> i32 {
    hle_nn_fs_ReadFile(
        std::ptr::null_mut(),
        handle,
        offset,
        buffer,
        size,
        std::ptr::null(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_CloseFile(handle: u64) {
    if handle == 0 {
        return;
    }
    let idx = (handle - 1) as usize;
    let mut files = OPEN_FILES.lock().unwrap();
    if let Some(slot) = files.get_mut(idx) {
        // Dropping the entry closes its file.
        slot.take();
    }
}

/// Logs a failed filesystem lookup once per (operation, path), then every 5000th repeat, so a thread that polls for a
/// file that never appears is visible without flooding the log.
fn log_fs_miss(op: &str, path: &str, detail: &str) {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashMap<(String, String), u64>>> = OnceLock::new();
    let mut seen = SEEN
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    let n = seen.entry((op.to_string(), path.to_string())).or_insert(0);
    *n += 1;
    if *n == 1 || *n % 5000 == 0 {
        println!("[fs-miss] {} '{}' {} (x{})", op, path, detail, n);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_GetEntryType(out_type: *mut i32, path: *const c_char) -> i32 {
    if path.is_null() || out_type.is_null() {
        return RESULT_PATH_NOT_FOUND;
    }
    let path_str = CStr::from_ptr(path).to_string_lossy();
    match image_entry(&path_str) {
        Some(ImageEntry::Dir) => {
            *out_type = 0;
            return 0;
        }
        Some(ImageEntry::File(..)) => {
            *out_type = 1;
            return 0;
        }
        None => {}
    }
    let host_path = match resolve_path(&path_str) {
        Some(p) => p,
        None => return RESULT_PATH_NOT_FOUND,
    };

    // As OpenFile: a file may be stored with an extra .zs suffix.
    let zs = PathBuf::from(format!("{}.zs", host_path.display()));
    let Ok(meta) = std::fs::metadata(&host_path).or_else(|_| std::fs::metadata(&zs)) else {
        log_fs_miss(
            "GetEntryType",
            &path_str,
            &format!("-> host '{}'", host_path.display()),
        );
        return RESULT_PATH_NOT_FOUND;
    };

    // nn::fs::DirectoryEntryType: Directory = 0, File = 1. Reporting 1/2 made an existing save file
    // look like "not a file": TotK's album code then retried CreateFile on it forever (loading never finished).
    *out_type = if meta.is_dir() { 0 } else { 1 };
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_Commit(_name: *const c_char) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_Unmount(_name: *const c_char) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_MountSaveData(name: *const c_char, _uid: *const ()) -> i32 {
    register_writable_mount("MountSaveData", name);
    let _ = std::fs::create_dir_all(save_root());
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_MountSaveDataReadOnly(
    name: *const c_char,
    _app_id: u64,
    _uid: *const (),
) -> i32 {
    register_writable_mount("MountSaveDataReadOnly", name);
    0
}

/// System data archives (ncm::SystemDataId) come with the console's firmware: <system data dir>/<id as 16 hex
/// digits>/ holds an archive's files (--system-data-dir, default: the data directory's system_data/).
pub(crate) fn system_data_dir(id: u64) -> Option<PathBuf> {
    let id_hex = format!("{id:016x}");
    let mut candidate_roots: Vec<PathBuf> = Vec::new();

    if let Some(dir) = crate::settings::get().files.system_data_dir.clone() {
        candidate_roots.push(dir.clone());
        if dir.is_relative() {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(parent) = exe.parent() {
                    candidate_roots.push(parent.join(&dir));
                    if let Some(grandparent) = parent.parent() {
                        candidate_roots.push(grandparent.join(&dir));
                    }
                }
            }
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidate_roots.push(parent.join("system_data"));
            candidate_roots.push(parent.join("Resources/system_data"));
            if let Some(grandparent) = parent.parent() {
                candidate_roots.push(grandparent.join("Resources/system_data"));
                candidate_roots.push(grandparent.join("system_data"));
            }
        }
    }

    if let Some(game_dir) = &crate::settings::get().files.game_dir {
        candidate_roots.push(game_dir.join("system_data"));
        candidate_roots.push(game_dir.join("RomFS/system_data"));
    }

    if let Some(app_data) = crate::profile::data_dir() {
        if let Some(nxport_dir) = app_data.parent().and_then(|p| p.parent()) {
            candidate_roots.push(nxport_dir.join("system_data"));
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home);
        candidate_roots.push(p.join("Library/Application Support/nxport/system_data"));
        candidate_roots.push(p.join("Documents/system_data"));
        candidate_roots.push(p.join("Documents/Game/system_data"));
    }

    candidate_roots.push(PathBuf::from("system_data"));

    for root in candidate_roots {
        let target = root.join(&id_hex);
        if target.is_dir() {
            return Some(target);
        }
    }
    None
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_QueryMountSystemDataCacheSize(out_size: *mut u64, _id: u64) -> i32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_MountSystemData(name: *const c_char, id: u64, _cache: *mut u8, _size: usize) -> i32 {
    if name.is_null() {
        return RESULT_TARGET_NOT_FOUND;
    }
    let name = std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned();
    let Some(dir) = system_data_dir(id) else {
        println!("[!] HLE: nn::fs::MountSystemData('{name}', {id:016x}): no system data {id:016x} (--system-data-dir)");
        return RESULT_TARGET_NOT_FOUND;
    };
    println!("[+] HLE: nn::fs::MountSystemData('{name}', {id:016x}) -> {}", dir.display());
    let mut m = READ_ONLY_MOUNTS.lock().unwrap();
    m.retain(|(n, _)| *n != name);
    m.push((name, dir));
    0
}

/// Device save data (shared by the console's users); MountDeviceSaveData(name, ApplicationId) mounts another
/// application's, which here is this one's.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_MountDeviceSaveData(name: *const c_char, _app_id: u64) -> i32 {
    register_writable_mount("MountDeviceSaveData", name);
    let _ = std::fs::create_dir_all(save_root());
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_IsSaveDataExisting(_app_id: u64, _uid: *const ()) -> bool {
    true
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_MountTemporaryStorage(name: *const c_char) -> i32 {
    register_writable_mount("MountTemporaryStorage", name);
    0
}

// ── Cache storage (nn::fs::*CacheStorage*) ──────────────────────────────────
// Cache storage lives in the fs sysmodule, so the game's sdk module entry points abort
// without a session. A fresh console has no cache storage: the list is empty,
// and Create/Mount/Delete succeed like the save-data mounts above.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_OpenCacheStorageList(out_handle: *mut u64) -> i32 {
    if !out_handle.is_null() {
        *out_handle = 1;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_ReadCacheStorageList(
    out_count: *mut i32,
    _out_infos: *mut (),
    _handle: u64,
    _count: i32,
) -> i32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_CloseCacheStorageList(_handle: u64) {}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_MountCacheStorage(name: *const c_char, _index: i32) -> i32 {
    register_writable_mount("MountCacheStorage", name);
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_CreateCacheStorage(
    _index: i32,
    _size: i64,
    _journal_size: i64,
) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_DeleteCacheStorage(_index: i32) -> i32 {
    0
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct DirectoryEntryRaw {
    pub name: [u8; 0x301],
    pub attributes: u8,
    pub pad1: [u8; 2],
    pub entry_type: u8, // nn::fs::DirectoryEntryType: 0 = directory, 1 = file
    pub pad2: [u8; 3],
    pub file_size: i64,
}

struct OpenDir {
    #[allow(dead_code)]
    path: String,
    entries: Vec<DirectoryEntryRaw>,
    read_index: usize,
}

static OPEN_DIRS: Mutex<Vec<Option<OpenDir>>> = Mutex::new(Vec::new());

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_OpenDirectory(
    out_handle: *mut u64,
    path: *const c_char,
    open_mode: i32,
) -> i32 {
    crate::horizon::hle::sanitize_darwin_tpidrro();
    if path.is_null() || out_handle.is_null() {
        return RESULT_PATH_NOT_FOUND;
    }
    let path_str = CStr::from_ptr(path).to_string_lossy();
    let host_path = match resolve_path(&path_str) {
        Some(p) => p,
        None => return RESULT_PATH_NOT_FOUND,
    };

    // RomFS directories merge the mods' contents with the game's (a mod's entry wins over the game's of the same name).
    let dirs_to_read = match romfs_rel(&path_str) {
        Some(rel) => romfs_dirs_in(mod_romfs_layers(), romfs_root(), rel),
        None => vec![ListedDir::Host(host_path.clone())],
    };
    let Some(listing) = merged_listing(&dirs_to_read) else {
        println!(
            "[-] HLE: nn::fs::OpenDirectory failed for '{}' -> '{}'",
            path_str,
            host_path.display()
        );
        return RESULT_PATH_NOT_FOUND;
    };

    let mut entries = Vec::new();
    for (name_str, meta) in listing {
        if is_host_junk(&name_str) {
            continue;
        }
        let name_bytes = name_str.as_bytes();

        let mut raw_entry = DirectoryEntryRaw {
            name: [0; 0x301],
            attributes: 0,
            pad1: [0; 2],
            entry_type: 1, // nn::fs::DirectoryEntryType_File
            pad2: [0; 3],
            file_size: 0,
        };

        let len = name_bytes.len().min(0x300);
        raw_entry.name[..len].copy_from_slice(&name_bytes[..len]);
        raw_entry.name[len] = 0;

        if let Some((is_dir, len)) = meta {
            // OpenDirectoryMode: bit 0 directories, bit 1 files.
            if open_mode & 3 != 0 && open_mode & if is_dir { 1 } else { 2 } == 0 {
                continue;
            }
            if is_dir {
                raw_entry.entry_type = 0; // DirectoryEntryType_Directory
                raw_entry.attributes = 1;
                raw_entry.file_size = 0;
            } else {
                raw_entry.entry_type = 1; // DirectoryEntryType_File
                raw_entry.attributes = 0;
                raw_entry.file_size = len as i64;
            }
        }

        entries.push(raw_entry);
    }

    let mut dirs = OPEN_DIRS.lock().unwrap();
    let handle_idx = dirs.len();
    dirs.push(Some(OpenDir {
        path: path_str.to_string(),
        entries,
        read_index: 0,
    }));

    let handle_id = (handle_idx + 1) as u64;
    *out_handle = handle_id;
    println!(
        "[+] HLE: nn::fs::OpenDirectory handle={} path='{}' (found {} entries)",
        handle_id,
        path_str,
        dirs[handle_idx].as_ref().unwrap().entries.len()
    );
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_ReadDirectory(
    out_count: *mut i64,
    out_entries: *mut DirectoryEntryRaw,
    handle: u64,
    max_entries: i64,
) -> i32 {
    if handle == 0 || out_count.is_null() || out_entries.is_null() || max_entries <= 0 {
        return RESULT_PATH_NOT_FOUND;
    }
    let idx = (handle - 1) as usize;
    let mut dirs = OPEN_DIRS.lock().unwrap();
    if let Some(Some(dir)) = dirs.get_mut(idx) {
        let remaining = dir.entries.len().saturating_sub(dir.read_index);
        let count_to_read = remaining.min(max_entries as usize);

        for i in 0..count_to_read {
            let entry = dir.entries[dir.read_index + i];
            *out_entries.add(i) = entry;
        }

        dir.read_index += count_to_read;
        *out_count = count_to_read as i64;
        0
    } else {
        eprintln!("[-] HLE: nn::fs::ReadDirectory invalid handle {}", handle);
        RESULT_PATH_NOT_FOUND
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_CloseDirectory(handle: u64) {
    if handle == 0 {
        return;
    }
    let idx = (handle - 1) as usize;
    let mut dirs = OPEN_DIRS.lock().unwrap();
    if let Some(slot) = dirs.get_mut(idx) {
        *slot = None;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_GetDirectoryEntryCount(out_count: *mut i64, handle: u64) -> i32 {
    if handle == 0 || out_count.is_null() {
        return RESULT_PATH_NOT_FOUND;
    }
    let idx = (handle - 1) as usize;
    let dirs = OPEN_DIRS.lock().unwrap();
    if let Some(Some(dir)) = dirs.get(idx) {
        *out_count = dir.entries.len() as i64;
        0
    } else {
        RESULT_PATH_NOT_FOUND
    }
}

// ── Save-data write path ────────────────────────────────────────────────────
unsafe fn save_host_path(path: *const c_char) -> Option<(String, PathBuf)> {
    if path.is_null() {
        return None;
    }
    let guest = CStr::from_ptr(path).to_string_lossy().into_owned();
    if !is_save_path(&guest) {
        eprintln!("[-] HLE: nn::fs write op on read-only path '{}'", guest);
        return None;
    }
    let host = resolve_path(&guest)?;
    Some((guest, host))
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_CreateDirectory(path: *const c_char) -> i32 {
    let Some((guest, host)) = save_host_path(path) else {
        return RESULT_PATH_NOT_FOUND;
    };
    if let Err(err) = std::fs::create_dir(&host) {
        log_fs_miss(
            "CreateDirectory",
            &guest,
            &format!("-> {} (host '{}')", err, host.display()),
        );
        return errno_to_result(&err);
    }
    println!("[+] HLE: nn::fs::CreateDirectory '{}'", guest);
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_CreateFile(path: *const c_char, size: i64) -> i32 {
    let Some((guest, host)) = save_host_path(path) else {
        return RESULT_PATH_NOT_FOUND;
    };
    let f = match std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&host) {
        Ok(f) => f,
        Err(err) => {
            log_fs_miss(
                "CreateFile",
                &guest,
                &format!("size={} -> {} (host '{}')", size, err, host.display()),
            );
            return errno_to_result(&err);
        }
    };
    if size > 0 {
        let _ = f.set_len(size as u64);
    }
    println!("[+] HLE: nn::fs::CreateFile '{}' size={}", guest, size);
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_DeleteFile(path: *const c_char) -> i32 {
    let Some((guest, host)) = save_host_path(path) else {
        return RESULT_PATH_NOT_FOUND;
    };
    if let Err(err) = std::fs::remove_file(&host) {
        return errno_to_result(&err);
    }
    println!("[+] HLE: nn::fs::DeleteFile '{}'", guest);
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_DeleteDirectoryRecursively(path: *const c_char) -> i32 {
    let Some((guest, host)) = save_host_path(path) else {
        return RESULT_PATH_NOT_FOUND;
    };
    match std::fs::remove_dir_all(&host) {
        Ok(_) => {
            println!("[+] HLE: nn::fs::DeleteDirectoryRecursively '{}'", guest);
            0
        }
        Err(e) => errno_to_result(&e),
    }
}

/// nn::fs::CleanDirectoryRecursively: deletes everything inside the directory and keeps the directory itself.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_CleanDirectoryRecursively(path: *const c_char) -> i32 {
    let Some((guest, host)) = save_host_path(path) else {
        return RESULT_PATH_NOT_FOUND;
    };
    let entries = match std::fs::read_dir(&host) {
        Ok(e) => e,
        Err(e) => return errno_to_result(&e),
    };
    for e in entries.flatten() {
        let p = e.path();
        let r = if p.is_dir() {
            std::fs::remove_dir_all(&p)
        } else {
            std::fs::remove_file(&p)
        };
        if let Err(err) = r {
            return errno_to_result(&err);
        }
    }
    println!("[+] HLE: nn::fs::CleanDirectoryRecursively '{}'", guest);
    0
}

/// nn::fs::EnsureSaveData(Uid): the user's save data is a host directory that mounting creates; nothing to do.
#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_EnsureSaveData(_uid: *const u8) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_GetSaveDataSizeMax(size: *mut i64, journal: *mut i64) -> i32 {
    if !size.is_null() {
        *size = 256 << 20; // 256 MB data
    }
    if !journal.is_null() {
        *journal = 256 << 20; // 256 MB journal
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_GetCacheStorageSize(size: *mut i64, journal: *mut i64, _index: i32) -> i32 {
    if !size.is_null() {
        *size = 256 << 20;
    }
    if !journal.is_null() {
        *journal = 256 << 20;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_GetFreeSpaceSize(out: *mut i64, _path: *const c_char) -> i32 {
    if !out.is_null() {
        *out = 256 << 20;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_ExtendSaveData(_uid: *const (), _size: i64, _journal: i64) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_RenameFile(
    old_path: *const c_char,
    new_path: *const c_char,
) -> i32 {
    let (Some((_, from)), Some((_, to))) = (save_host_path(old_path), save_host_path(new_path))
    else {
        return RESULT_PATH_NOT_FOUND;
    };
    if let Err(err) = std::fs::rename(&from, &to) {
        return errno_to_result(&err);
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_WriteFile(
    handle: u64,
    offset: i64,
    buffer: *const u8,
    size: usize,
    _option: *const (),
) -> i32 {
    if handle == 0 || (buffer.is_null() && size != 0) {
        return RESULT_PATH_NOT_FOUND;
    }
    let mut files = OPEN_FILES.lock().unwrap();
    match files.get_mut((handle - 1) as usize) {
        Some(Some(entry)) => match &entry.data {
            FileData::Fd(f) => {
                crate::os::ensure_committed(buffer as usize, size);
                let src = if size == 0 { &[][..] } else { std::slice::from_raw_parts(buffer, size) };
                if write_at(f, src, offset as u64).ok() != Some(size) {
                    eprintln!(
                        "[-] HLE: nn::fs::WriteFile short/failed write on handle {}",
                        handle
                    );
                    return RESULT_PATH_NOT_FOUND;
                }
                entry.size = entry.size.max(offset + size as i64);
                0
            }
            FileData::Buf(_) => RESULT_PATH_NOT_FOUND,
        },
        _ => RESULT_PATH_NOT_FOUND,
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_FlushFile(handle: u64) -> i32 {
    let files = OPEN_FILES.lock().unwrap();
    if let Some(Some(OpenFile {
        data: FileData::Fd(f),
        ..
    })) = files.get((handle.wrapping_sub(1)) as usize)
    {
        let _ = f.sync_all();
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_SetFileSize(handle: u64, size: i64) -> i32 {
    let mut files = OPEN_FILES.lock().unwrap();
    match files.get_mut((handle.wrapping_sub(1)) as usize) {
        Some(Some(entry)) => match &entry.data {
            FileData::Fd(f) => {
                if f.set_len(size as u64).is_err() {
                    return RESULT_PATH_NOT_FOUND;
                }
                entry.size = size;
                0
            }
            FileData::Buf(_) => RESULT_PATH_NOT_FOUND,
        },
        _ => RESULT_PATH_NOT_FOUND,
    }
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
