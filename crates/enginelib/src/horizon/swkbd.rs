// nn::swkbd as the game calls it: the software keyboard applet.

use super::nn::*;

// ── nn::swkbd / nn::nfp / nn::hid / nn::prepo ────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn get_required_work_buffer_size(_is_unicode: bool) -> usize {
    0x4000
}

pub(crate) unsafe extern "C" fn get_required_string_buffer_size() -> usize {
    0x2000
}

pub(crate) unsafe extern "C" fn make_preset(cfg: *mut u8, _preset: i32) {
    if !cfg.is_null() {
        std::ptr::write_bytes(cfg, 0, 0x4d0);
    }
}

pub(crate) unsafe extern "C" fn show_keyboard(out: *mut u8, arg: *const u8) -> u32 {
    const SWKBD_CANCELED: u32 = 0x29f;
    if out.is_null() {
        return SWKBD_CANCELED;
    }
    let str_buf = *(out as *const usize);
    let str_cap = *((out as *const usize).add(1));
    if str_buf == 0 || str_cap == 0 {
        return SWKBD_CANCELED;
    }

    let (_max_len, _min_len, utf8, initial_text) = if !arg.is_null() {
        let max_l = *(arg.add(0x3ac) as *const u32) as usize;
        let min_l = *(arg.add(0x3b0) as *const u32) as usize;
        let is_u8 = *arg.add(0x3bd) != 0;

        let init_off = *(arg.add(0x3c0) as *const i32);
        let init_len = *(arg.add(0x3c4) as *const i32);
        let work = *(arg.add(0x4d0) as *const usize);
        let work_size = *(arg.add(0x4d8) as *const usize);

        let mut init_str = String::new();
        if init_len > 0
            && work != 0
            && init_off >= 0
            && (init_off as usize + (init_len as usize) * 2) <= work_size
        {
            let u16_slice = std::slice::from_raw_parts(
                (work + init_off as usize) as *const u16,
                init_len as usize,
            );
            init_str = String::from_utf16_lossy(u16_slice);
        }
        (max_l, min_l, is_u8, init_str)
    } else {
        (500, 0, false, String::new())
    };

    let answer = if let Some(val) = crate::settings::get().input.keyboard_text.clone() {
        if val == "<cancel>" {
            return SWKBD_CANCELED;
        }
        val
    } else if !initial_text.is_empty() {
        initial_text
    } else {
        "Player".to_string()
    };

    println!("[+] swkbd: ShowKeyboard returning \"{}\"", answer);

    if utf8 {
        let bytes = answer.as_bytes();
        let copy_len = bytes.len().min(str_cap.saturating_sub(1));
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), str_buf as *mut u8, copy_len);
        *((str_buf + copy_len) as *mut u8) = 0;
    } else {
        let u16_vec: Vec<u16> = answer.encode_utf16().collect();
        let max_u16 = (str_cap / 2).saturating_sub(1);
        let copy_len = u16_vec.len().min(max_u16);
        std::ptr::copy_nonoverlapping(u16_vec.as_ptr(), str_buf as *mut u16, copy_len);
        *((str_buf + copy_len * 2) as *mut u16) = 0;
    }

    RESULT_SUCCESS
}
