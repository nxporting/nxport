// nn::friends as the game calls it: no friends, no presence.

use super::nn::*;

// ── nn::friends: an offline user with an empty friend list (the SDK's own Initialize opens friend:u itself, whose IPC
// is not emulated: Super Mario Bros. Wonder aborted there at boot) ──────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn void() {}

pub(crate) unsafe extern "C" fn ok() -> u32 {
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn get_system_event(event: *mut ()) -> u32 {
    if !event.is_null() {
        crate::horizon::os::objects::hle_system_event_register_signaled(event as usize);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn out_true(out: *mut bool) -> u32 {
    if !out.is_null() {
        *out = true;
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn get_friend_list(out_count: *mut i32) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn get_play_history_key(out_key: *mut u8) -> u32 {
    if !out_key.is_null() {
        std::ptr::write_bytes(out_key, 0, 0x40); // PlayHistoryRegistrationKey
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn returns_false() -> bool {
    false
}

pub(crate) unsafe extern "C" fn zero() -> u64 {
    0
}

pub(crate) unsafe extern "C" fn empty_ref() -> *const u8 {
    FRIENDS_EMPTY.as_ptr()
}

pub(crate) unsafe extern "C" fn not_found() -> u32 {
    121 | (1 << 9) // a friends-module failure: there is no friend to have an image
}
pub(crate) static FRIENDS_EMPTY: [u8; 0x100] = [0; 0x100];
