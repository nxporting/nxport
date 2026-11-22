// nn::fs: RomFS and game paths mapped to the dump's files (mods layered over them), and writable save mounts.

use std::ffi::CStr;
use std::fs::File;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

mod mods;
mod romfs;
mod dirs;
mod storage;
pub(crate) mod posix;
pub use mods::*;
pub(crate) use romfs::*;
pub use dirs::*;
pub use storage::*;

use super::packed_mods::{self, ImageDirEntry, RomfsImage};

/// pread: reads at `off` (Windows also moves the file position, which nothing here uses).
/// Reads `buf.len()` bytes at `off`, or up to the end of the file: as nn::fs::ReadFile does. One pread / ReadFile may return
/// fewer bytes than asked without being at the end (a mounted image, a FUSE or WinFsp file system, a large request), and
/// the game's reads without a byte count take the rest of the buffer as file data.
fn read_at(f: &File, buf: &mut [u8], off: u64) -> std::io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(f, &mut buf[done..], off + done as u64);
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(f, &mut buf[done..], off + done as u64);
        match n {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // The caller retries a buffer fault from the start (ReadFile below).
            Err(e) => return Err(e),
        }
    }
    Ok(done)
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

/// Every mount name the game has mounted and not unmounted (nn::fs::IsMounted). The routing tables below are kept
/// separately: a path under a name stays resolvable after an unmount.
static MOUNTED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub(crate) fn note_mounted(name: &str) {
    let mut m = MOUNTED.lock().unwrap();
    if !m.iter().any(|n| n == name) {
        m.push(name.to_string());
    }
}

/// nn::fs::IsMounted(const char*), from the names the engine's mount calls recorded (the SDK's own mount table is
/// not filled by them). Unity's IL2CPP checks "rom" before opening its metadata.
pub unsafe extern "C" fn is_mounted(name: *const c_char) -> bool {
    if name.is_null() {
        return false;
    }
    let name = CStr::from_ptr(name).to_string_lossy();
    MOUNTED.lock().unwrap().iter().any(|n| *n == name)
}

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
    note_mounted(&name);
    if name != "save" && name != "data" {
        // A mounted storage's root exists on the console: TotK creates savc:/icon at mount time and saves its icons
        // there.
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
            // UltraCam's config from the mods, when the SD card has none yet.
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









































#[no_mangle]
pub unsafe extern "C" fn query_mount_rom_cache_size(out_size: *mut usize) -> i32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn mount_rom(
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
    note_mounted(&name_str);
    println!(
        "[+] HLE: nn::fs::MountRom('{}') -> mapped to '{}'",
        name_str,
        romfs_root().display()
    );
    0
}

#[no_mangle]
pub unsafe extern "C" fn open_file(
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

    // The bare path first, then the .zs compressed variant (TotK's RomFS stores its files with an extra .zs suffix).
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
    // built-in Zstandard decompressor.
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
pub unsafe extern "C" fn get_file_size(out_size: *mut i64, handle: u64) -> i32 {
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
pub unsafe extern "C" fn read_file(
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
                crate::platform::os::ensure_committed(buffer as usize, size);
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
pub unsafe extern "C" fn read_file_no_option(
    out_bytes_read: *mut usize,
    handle: u64,
    offset: i64,
    buffer: *mut u8,
    size: usize,
) -> i32 {
    read_file(
        out_bytes_read,
        handle,
        offset,
        buffer,
        size,
        std::ptr::null(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn read_file_simple(
    handle: u64,
    offset: i64,
    buffer: *mut u8,
    size: usize,
) -> i32 {
    read_file(
        std::ptr::null_mut(),
        handle,
        offset,
        buffer,
        size,
        std::ptr::null(),
    )
}

#[no_mangle]
pub unsafe extern "C" fn close_file(handle: u64) {
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
pub unsafe extern "C" fn get_entry_type(out_type: *mut i32, path: *const c_char) -> i32 {
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

    // nn::fs::DirectoryEntryType: Directory = 0, File = 1 (TotK's album code retries CreateFile forever on a save file
    // that does not read as a file).
    *out_type = if meta.is_dir() { 0 } else { 1 };
    0
}

#[no_mangle]
pub unsafe extern "C" fn commit(_name: *const c_char) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn unmount(name: *const c_char) -> i32 {
    if !name.is_null() {
        let name = CStr::from_ptr(name).to_string_lossy();
        MOUNTED.lock().unwrap().retain(|n| *n != name);
    }
    0
}


































#[no_mangle]
pub unsafe extern "C" fn write_file(
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
                crate::platform::os::ensure_committed(buffer as usize, size);
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
pub unsafe extern "C" fn flush_file(handle: u64) -> i32 {
    // Synced through a second handle to the same file, outside OPEN_FILES: a sync waits on the disk (tens of
    // milliseconds), and under the lock every other file call of the game would wait with it.
    let file = {
        let files = OPEN_FILES.lock().unwrap();
        match files.get((handle.wrapping_sub(1)) as usize) {
            Some(Some(OpenFile { data: FileData::Fd(f), .. })) => f.try_clone().ok(),
            _ => None,
        }
    };
    if let Some(f) = file {
        let _ = f.sync_all();
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn set_file_size(handle: u64, size: i64) -> i32 {
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

#[no_mangle]
pub(crate) unsafe extern "C" fn set_priority_on_current_thread(_prio: i32) {}
