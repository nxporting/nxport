// nn::crypto.

/// nn::crypto::GenerateCryptographicallyRandomBytes(void*, size_t): the SDK fills the buffer through the csrng service,
/// whose IPC is not emulated (Super Mario Party Jamboree calls it from its first static constructor: std::random_device
/// -> getentropy -> here). The host's secure random source gives the same guarantee.
pub(crate) unsafe extern "C" fn generate_cryptographically_random_bytes(buf: *mut u8, size: usize) {
    if !buf.is_null() && size != 0 {
        crate::platform::os::random_bytes(std::slice::from_raw_parts_mut(buf, size));
    }
}
