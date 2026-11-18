// nn::account as the game calls it: user accounts and the preselected user.

use super::nn::*;

pub(crate) unsafe extern "C" fn initialize() -> i32 {
    println!("[+] HLE: nn::account::Initialize() called");
    0
}

pub(crate) unsafe extern "C" fn try_open_preselected_user(out_handle: *mut u128) -> bool {
    println!("[+] HLE: nn::account::TryOpenPreselectedUser called");
    if !out_handle.is_null() {
        *out_handle = 1;
    }
    true
}

pub(crate) unsafe extern "C" fn open_preselected_user(out_handle: *mut u128) -> i32 {
    println!("[+] HLE: nn::account::OpenPreselectedUser called");
    if !out_handle.is_null() {
        *out_handle = 1;
    }
    0
}

pub(crate) unsafe extern "C" fn get_user_id(
    out_uid: *mut NnAccountUid,
    _handle: *const (),
) -> i32 {
    println!("[+] HLE: nn::account::GetUserId called");
    if !out_uid.is_null() {
        (*out_uid).high = 0x0123456789ABCDEF;
        (*out_uid).low = 0xFEDCBA9876543210;
    }
    0
}

pub(crate) unsafe extern "C" fn get_last_opened_user(out_uid: *mut NnAccountUid) -> i32 {
    println!("[+] HLE: nn::account::GetLastOpenedUser called");
    if !out_uid.is_null() {
        (*out_uid).high = 0x0123456789ABCDEF;
        (*out_uid).low = 0xFEDCBA9876543210;
    }
    0
}

pub(crate) unsafe extern "C" fn get_nickname(
    out_nick: *mut [u8; 32],
    _uid: *const NnAccountUid,
) -> i32 {
    println!("[+] HLE: nn::account::GetNickname called");
    if !out_nick.is_null() {
        let name = b"Link\0";
        libc::memset(out_nick as *mut libc::c_void, 0, 32);
        libc::memcpy(
            out_nick as *mut libc::c_void,
            name.as_ptr() as *const libc::c_void,
            name.len(),
        );
    }
    0
}

pub(crate) unsafe extern "C" fn close_user(_handle: *const ()) {
    println!("[+] HLE: nn::account::CloseUser called");
}

pub(crate) unsafe extern "C" fn get_user_existence(out: *mut bool, _uid: *const u8) -> u32 {
    if !out.is_null() {
        *out = true;
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn list_users(out_count: *mut i32, out: *mut [u64; 2], max: i32) -> u32 {
    let n = if max > 0 && !out.is_null() {
        *out = USER_UID;
        1
    } else {
        0
    };
    if !out_count.is_null() {
        *out_count = n;
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn get_user_count(out: *mut i32) -> u32 {
    if !out.is_null() {
        *out = 1;
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn open_user(out_handle: *mut u128, _uid: *const u8) -> u32 {
    if !out_handle.is_null() {
        *out_handle = 1; // as TryOpenPreselectedUser
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn show_user_selector(
    out_uid: *mut [u64; 2],
    _settings: *const u8,
) -> u32 {
    if !out_uid.is_null() {
        *out_uid = USER_UID;
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn out_false(out: *mut bool) -> u32 {
    if !out.is_null() {
        *out = false;
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn load_profile_image(out_size: *mut usize) -> u32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    124 | (100 << 9) // an account-module failure: the user has no profile image
}

pub(crate) unsafe extern "C" fn nsa_unavailable_1(_a: *mut u8) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}

pub(crate) unsafe extern "C" fn nsa_unavailable_2(_a: *mut u8, _b: *const u8) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}

pub(crate) unsafe extern "C" fn nsa_unavailable_4(_a: *mut u8, _b: *const u8, _c: *mut u8, _d: usize) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}

pub(crate) unsafe extern "C" fn load_nsa_id_token_cache(
    out_size: *mut usize,
    _buf: *mut u8,
    _size: usize,
    _user: *const u8,
) -> u32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    ACCOUNT_NSA_UNAVAILABLE
}

pub(crate) unsafe extern "C" fn async_has_done(_this: *mut u8, out: *mut bool) -> u32 {
    if !out.is_null() {
        *out = true;
    }
    0
}

pub(crate) unsafe extern "C" fn async_get_result(_this: *mut u8) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}

pub(crate) unsafe extern "C" fn async_cancel(_this: *mut u8) -> u32 {
    0
}

pub(crate) unsafe extern "C" fn async_get_system_event(_this: *mut u8, out_event: *mut u8) -> u32 {
    if !out_event.is_null() {
        crate::horizon::os::objects::hle_system_event_register_signaled(out_event as usize);
    }
    0
}
#[repr(C)]
pub(crate) struct NnAccountUid {
    pub high: u64,
    pub low: u64,
}
// ── nn::account: one local user (the uid TryOpenPreselectedUser / GetUserId report, horizon/hle.rs), no network account ──────
pub(crate) const USER_UID: [u64; 2] = [0x0123456789ABCDEF, 0xFEDCBA9876543210];

// nn::account network service account: the user has none linked / no network,
// ResultNetworkServiceAccountUnavailable (module 124, description 200). An AsyncContext the
// failed calls never started reports done with that result; its event is signaled.
pub(crate) const ACCOUNT_NSA_UNAVAILABLE: u32 = 124 | (200 << 9);
