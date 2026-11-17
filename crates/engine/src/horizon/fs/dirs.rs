// nn::fs directories: opening, reading and counting entries.

use super::*;

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

pub(crate) struct OpenDir {
    #[allow(dead_code)]
    path: String,
    entries: Vec<DirectoryEntryRaw>,
    read_index: usize,
}

pub(crate) static OPEN_DIRS: Mutex<Vec<Option<OpenDir>>> = Mutex::new(Vec::new());

#[no_mangle]
pub unsafe extern "C" fn hle_nn_fs_OpenDirectory(
    out_handle: *mut u64,
    path: *const c_char,
    open_mode: i32,
) -> i32 {
    crate::horizon::os::threads::sanitize_darwin_tpidrro();
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
