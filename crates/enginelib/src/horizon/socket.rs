// nn::socket as the game calls it: no network.


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
// No network (consistent with nn::socket::Initialize failing and nifm reporting no connection): every socket operation
// fails with Errno::ENetDown (100); closing / finalizing succeed; name lookups fail with EAI_FAIL
// (4). Falling through to the sdk module here would not fail cleanly: its IPC requests are written to the
// thread's Horizon IPC buffer, which is Darwin's TLS on this host (the reply port in slot 2 gets overwritten).
pub(crate) const SOCKET_ENETDOWN: i32 = 100;

thread_local! {
    pub(crate) static SOCKET_ERRNO: std::cell::Cell<i32> = const { std::cell::Cell::new(SOCKET_ENETDOWN) };
}

/// The errno the calling thread's next nn::socket::GetLastErrno returns.
pub(crate) fn set_socket_errno(e: i32) {
    SOCKET_ERRNO.with(|c| c.set(e));
}
