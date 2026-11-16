// nn::ec as the game calls it: shop service accessors, reported as unavailable.

use super::nn::*;

pub(crate) unsafe extern "C" fn initialize_for_shop_service_accessors(_work: *mut u8, _size: usize) -> u32 {
    0
}

pub(crate) unsafe extern "C" fn finalize_for_shop_service_accessors() {}

pub(crate) unsafe extern "C" fn accessor_initialize(_this: *mut u8, _service: *const u8) -> u32 {
    0
}

pub(crate) unsafe extern "C" fn accessor_finalize(_this: *mut u8) {}

pub(crate) unsafe extern "C" fn accessor_request(
    _this: *mut u8,
    _resp: *mut u8,
    _uid: *const u8,
    _method: i32,
    _path: *const u8,
    _post: *const u8,
    _timeout: i64,
) -> u32 {
    EC_INTERNET_REQUEST_NOT_ACCEPTED
}

pub(crate) unsafe extern "C" fn response_try_wait(_this: *mut u8) -> bool {
    true
}

pub(crate) unsafe extern "C" fn response_cancel(_this: *mut u8) {}

pub(crate) unsafe extern "C" fn response_get_size(_this: *mut u8, out: *mut usize) -> u32 {
    if !out.is_null() {
        *out = 0;
    }
    EC_INTERNET_REQUEST_NOT_ACCEPTED
}

pub(crate) unsafe extern "C" fn response_get(_this: *mut u8, _buf: *mut u8, _size: usize) -> u32 {
    EC_INTERNET_REQUEST_NOT_ACCEPTED
}

/// Opens the eShop product page applet; nothing to show.
pub(crate) unsafe extern "C" fn show_shop_product_details(ns_uid: u64, _user: *const u8) {
    println!(
        "[+] HLE: nn::ec::ShowShopProductDetails(nsUid={:#x}): no eShop",
        ns_uid
    );
}
