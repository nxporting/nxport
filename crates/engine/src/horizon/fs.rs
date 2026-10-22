// Horizon OS High-Level Emulation for File System (nn::fs)
// Maps RomFS and game paths directly to the local dumped assets.

use std::ffi::CStr;
use std::fs::File;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
                    let src_cfg = mod_path.join("romfs/UltraCam/maxlastbreath.ini");
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
}

static OPEN_FILES: Mutex<Vec<Option<OpenFile>>> = Mutex::new(Vec::new());

/// Enabled mods, highest priority first: (name, mod directory). A mod directory has the Ryujinx / Atmosphere layout
/// (`romfs/` replaces or adds game files; `exefs/` holds code mods). Mods come from --mods-dir (a directory of mod
/// folders, e.g. Ryujinx's mods/contents/0100f2c0115b6000), else ./mods if it exists; --no-mods disables them.
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
        let Ok(rd) = std::fs::read_dir(&root) else { return Vec::new() };
        let mut v: Vec<(String, PathBuf)> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .filter(|(name, path)| {
                let mod_npdm = [path.join("exefs/main.npdm"), path.join("ExeFS/main.npdm")]
                    .into_iter()
                    .find(|p| p.is_file());
                let target = mod_npdm.as_deref().and_then(crate::game::npdm_program_id);
                let current_tid = crate::game::title_id()
                    .or_else(|| {
                        let act = crate::profile::active();
                        (act.title_id != 0).then_some(act.title_id)
                    });
                match (target, current_tid) {
                    (Some(t), Some(game)) if t != game => {
                        println!("[!] Mods: '{name}' is for program {t:016X}, not this game ({game:016X}); skipped");
                        false
                    }
                    (Some(t), None) => {
                        println!("[!] Mods: '{name}' is for program {t:016X}, but current game program ID is unknown; skipped");
                        false
                    }
                    _ => true,
                }
            })
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path) in &v {
            println!("[+] Mods: '{}' ({}{})", name, if path.join("romfs").is_dir() { "romfs " } else { "" }, if path.join("exefs").is_dir() { "exefs" } else { "" });
        }
        v
    })
}

/// RomFS directories of the enabled mods, highest priority first.
fn mod_romfs_roots() -> &'static [PathBuf] {
    static ROOTS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| {
        mods()
            .iter()
            .map(|(_, p)| p.join("romfs"))
            .filter(|p| p.is_dir())
            .collect()
    })
}

/// A RomFS-relative path resolved through the mods first (the game's own .zs fallback applies to mod files too).
fn romfs_path(rel: &str) -> PathBuf {
    let rel = rel.trim_start_matches('/');
    for root in mod_romfs_roots() {
        let p = root.join(rel);
        let zs = PathBuf::from(format!("{}.zs", p.display()));
        if p.exists() || zs.exists() {
            return p;
        }
    }
    romfs_root().join(rel)
}

/// Every directory a RomFS-relative directory path exists in: the mods' (highest priority first), then the game's.
fn romfs_dirs(rel: &str) -> Vec<PathBuf> {
    let rel = rel.trim_start_matches('/');
    let mut v: Vec<PathBuf> = mod_romfs_roots()
        .iter()
        .map(|r| r.join(rel))
        .filter(|p| p.is_dir())
        .collect();
    v.push(romfs_root().join(rel));
    v
}

/// The RomFS-relative part of a guest path, if it names the game's RomFS.
fn romfs_rel(guest_path: &str) -> Option<&str> {
    let clean_path = guest_path.trim_start_matches('/');
    if let Some(rel) = clean_path
        .strip_prefix("rom:")
        .or_else(|| clean_path.strip_prefix("content:"))
    {
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

pub(crate) fn resolve_path(guest_path: &str) -> Option<PathBuf> {
    // Paths like "content://Sound/x" (double slash) occur; Path::join with a leading "/" would replace
    // the root entirely, so strip every leading slash after the mount name.
    let clean_path = guest_path.trim_start_matches('/');
    if let Some(rel) = clean_path.strip_prefix("rom:") {
        Some(romfs_path(rel))
    } else if let Some(rel) = clean_path.strip_prefix("content:") {
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
                let dst = std::slice::from_raw_parts_mut(buffer, size);
                let mut n = Err(std::io::ErrorKind::Other.into());
                for _ in 0..4 {
                    crate::guest::dirty_track::before_host_write(buffer as usize, size);
                    n = read_at(f, dst, offset as u64);
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
    let host_path = match resolve_path(&path_str) {
        Some(p) => p,
        None => return RESULT_PATH_NOT_FOUND,
    };

    let Ok(meta) = std::fs::metadata(&host_path) else {
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
fn system_data_dir(id: u64) -> Option<PathBuf> {
    let root = crate::settings::get()
        .files
        .system_data_dir
        .clone()
        .or_else(|| {
            // nxport/games/<title id> -> nxport/system_data
            Some(crate::profile::data_dir()?.parent()?.parent()?.join("system_data"))
        })?;
    Some(root.join(format!("{id:016x}"))).filter(|d| d.is_dir())
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
    _open_mode: i32,
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
        Some(rel) => romfs_dirs(rel),
        None => vec![host_path.clone()],
    };
    let mut read_dirs = Vec::new();
    for d in &dirs_to_read {
        if let Ok(rd) = std::fs::read_dir(d) {
            read_dirs.push(rd);
        }
    }
    if read_dirs.is_empty() {
        println!(
            "[-] HLE: nn::fs::OpenDirectory failed for '{}' -> '{}'",
            path_str,
            host_path.display()
        );
        return RESULT_PATH_NOT_FOUND;
    }
    let mut seen_names = std::collections::HashSet::new();

    let mut entries = Vec::new();
    for entry_res in read_dirs.into_iter().flatten() {
        if let Ok(entry) = entry_res {
            let file_name = entry.file_name();
            if !seen_names.insert(file_name.clone()) {
                continue;
            }
            let name_str = file_name.to_string_lossy();
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

            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    raw_entry.entry_type = 0; // DirectoryEntryType_Directory
                    raw_entry.attributes = 1;
                    raw_entry.file_size = 0;
                } else {
                    raw_entry.entry_type = 1; // DirectoryEntryType_File
                    raw_entry.attributes = 0;
                    raw_entry.file_size = meta.len() as i64;
                }
            }

            entries.push(raw_entry);
        }
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
