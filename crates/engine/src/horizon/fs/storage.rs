// nn::fs storage: save data, cache storage, system data and temporary storage mounts, and creating, deleting,
// renaming and sizing files and directories in writable mounts.

use super::*;

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

// ── Save-data write path ────────────────────────────────────────────────────
pub(crate) unsafe fn save_host_path(path: *const c_char) -> Option<(String, PathBuf)> {
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

pub(crate) unsafe extern "C" fn fs_is_save_data_existing(_uid: *const u8) -> bool {
    true // saves live in the save directory (horizon/fs.rs); EnsureSaveData / MountSaveData create it
}
