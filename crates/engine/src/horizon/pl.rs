// nn::pl as the game calls it: shared fonts and VR mode.

use super::nn::*;

pub(crate) unsafe extern "C" fn request_shared_font_load(ty: i32) {
    shared_font(ty);
}

/// GetSharedFont(int* count, SharedFontData[] {type, address, size}, int capacity, LanguageCode): every font in type
/// order whatever the language (as Ryujinx); true when all are loaded.
pub(crate) unsafe extern "C" fn get_shared_font(out_count: *mut i32, out: *mut u8, capacity: i32, _language: u64) -> bool {
    let n = capacity.clamp(0, 6) as usize;
    let mut all = true;
    for ty in 0..n {
        let font = shared_font(ty as i32);
        let (addr, len) = if font.is_empty() {
            (DUMMY_SHARED_FONT.as_ptr(), DUMMY_SHARED_FONT.len())
        } else {
            (font.as_ptr(), font.len())
        };
        all &= len > 0;
        if !out.is_null() {
            let e = out.add(ty * 24);
            (e as *mut u32).write_unaligned(ty as u32);
            (e.add(8) as *mut usize).write_unaligned(addr as usize);
            (e.add(16) as *mut usize).write_unaligned(len);
        }
    }
    if !out_count.is_null() {
        *out_count = 6;
    }
    all
}

pub(crate) unsafe extern "C" fn get_shared_font_load_state(_ty: i32) -> i32 {
    1
}

pub(crate) unsafe extern "C" fn get_shared_font_size(ty: i32) -> usize {
    let len = shared_font(ty).len();
    if len == 0 {
        DUMMY_SHARED_FONT.len()
    } else {
        len
    }
}

pub(crate) unsafe extern "C" fn get_shared_font_address(ty: i32) -> *const u8 {
    let font = shared_font(ty);
    if font.is_empty() {
        DUMMY_SHARED_FONT.as_ptr()
    } else {
        font.as_ptr()
    }
}

pub(crate) unsafe extern "C" fn begin_vr_mode() {
    VR_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) unsafe extern "C" fn end_vr_mode() {
    VR_MODE.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) unsafe extern "C" fn is_vr_mode() -> bool {
    VR_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

/// nn::pl::IsRunningOnOunce: whether the console is a Switch 2. The engine presents the original Switch.
pub(crate) unsafe extern "C" fn is_running_on_ounce() -> bool {
    false
}
