// nn::bcat as the game calls it: delivery cache (online content), reported as empty.


pub(crate) unsafe extern "C" fn initialize() {}

pub(crate) unsafe extern "C" fn mount_delivery_cache_storage() -> u32 {
    0
}

pub(crate) unsafe extern "C" fn unmount_delivery_cache_storage() {}

pub(crate) unsafe extern "C" fn enumerate_delivery_cache_directory(
    out_count: *mut i32,
    _names: *mut u8,
    _count: i32,
) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    0
}

pub(crate) unsafe extern "C" fn request_sync_delivery_cache(_progress: *mut u8) -> u32 {
    BCAT_INTERNET_REQUEST_NOT_ACCEPTED
}

pub(crate) unsafe extern "C" fn progress_update(_this: *mut u8) {}

pub(crate) unsafe extern "C" fn progress_get_status(_this: *const u8) -> i32 {
    9
}

pub(crate) unsafe extern "C" fn progress_get_result(_this: *const u8) -> u32 {
    BCAT_INTERNET_REQUEST_NOT_ACCEPTED
}

pub(crate) unsafe extern "C" fn open_not_found(_this: *mut u8, _name: *const u8) -> u32 {
    BCAT_NOT_FOUND
}

pub(crate) unsafe extern "C" fn file_read(
    _this: *mut u8,
    out_size: *mut usize,
    _offset: i64,
    _buf: *mut u8,
    _size: usize,
) -> u32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    BCAT_NOT_OPENED
}

pub(crate) unsafe extern "C" fn directory_read(
    _this: *mut u8,
    out_count: *mut i32,
    _entries: *mut u8,
    _count: i32,
) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    BCAT_NOT_OPENED
}

pub(crate) unsafe extern "C" fn close(_this: *mut u8) {}
// nn::bcat (data distribution / delivery cache): a console that never went online. The storage mounts and
// is empty, sync requests fail with no Internet connection, files and directories are not found.
// Result codes use module 122 (result = module | description << 9).
pub(crate) const BCAT_NOT_FOUND: u32 = 122 | (2 << 9);

pub(crate) const BCAT_NOT_OPENED: u32 = 122 | (7 << 9);

pub(crate) const BCAT_INTERNET_REQUEST_NOT_ACCEPTED: u32 = 122 | (8 << 9);
