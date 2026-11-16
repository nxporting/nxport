// nn::socket as the game calls it: no network.

use super::nn::*;

pub(crate) unsafe extern "C" fn fail() -> i32 {
    -1
}

pub(crate) unsafe extern "C" fn get_last_errno() -> i32 {
    SOCKET_ERRNO.with(|c| c.get())
}

pub(crate) unsafe extern "C" fn get_addr_info(
    _node: *const u8,
    _svc: *const u8,
    _hints: *const u8,
    out: *mut *mut u8,
) -> i32 {
    if !out.is_null() {
        *out = std::ptr::null_mut();
    }
    4 // EAI_FAIL
}

pub(crate) unsafe extern "C" fn free_addr_info(_ai: *mut u8) {}
