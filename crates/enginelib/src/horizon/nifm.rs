// nn::nifm as the game calls it: network requests, never connected.


/// nifm connection queries without a connection: NoInternetConnection (module 110, description 300, as
/// Ryujinx reports it).
pub(crate) unsafe extern "C" fn no_internet_connection(_out: *mut u8) -> u32 {
    110 | (300 << 9)
}

pub(crate) unsafe extern "C" fn cancel_network_request() {}

/// HandleNetworkRequestErrorResult: false = the error was not resolved.
pub(crate) unsafe extern "C" fn handle_network_request_error_result() -> bool {
    false
}
