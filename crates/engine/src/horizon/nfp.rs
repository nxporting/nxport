// nn::nfp as the game calls it: amiibo, with no reader present.

use crate::horizon::os::objects::*;
use super::nn::*;

pub(crate) unsafe extern "C" fn initialize() -> u32 {
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn finalize() {}

pub(crate) unsafe extern "C" fn get_state() -> i32 {
    0
}

pub(crate) unsafe extern "C" fn attach_event(ev: *mut u8) {
    if !ev.is_null() {
        hle_system_event_register_addr(ev as usize);
    }
}

pub(crate) unsafe extern "C" fn attach_event_with_handle(ev: *mut u8, _handle: *const u8) -> u32 {
    if !ev.is_null() {
        hle_system_event_register_addr(ev as usize);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn list_devices(_handles: *mut u8, out_count: *mut i32, _max: i32) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn get_device_state(_handle: *const u8) -> i32 {
    0
}

pub(crate) unsafe extern "C" fn fail1(_a: *const u8) -> u32 {
    NFP_RESULT_NOT_AVAILABLE
}

pub(crate) unsafe extern "C" fn fail2(_a: *mut u8, _b: *const u8) -> u32 {
    NFP_RESULT_NOT_AVAILABLE
}

pub(crate) unsafe extern "C" fn mount(_h: *const u8, _m: i32, _t: i32) -> u32 {
    NFP_RESULT_NOT_AVAILABLE
}
