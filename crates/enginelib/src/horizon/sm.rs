// nn::sm as the game calls it: service handles.


pub(crate) unsafe extern "C" fn initialize() -> i32 {
    println!("[+] HLE: nn::sm::Initialize() called -> returning ResultSuccess (0)");
    0
}

pub(crate) unsafe extern "C" fn finalize() {
    println!("[+] HLE: nn::sm::Finalize() called");
}

pub(crate) unsafe extern "C" fn get_service_handle(
    handle_out: *mut u32,
    service_name: *const libc::c_char,
    _len: usize,
) -> i32 {
    let name_str = if service_name.is_null() {
        ""
    } else {
        std::ffi::CStr::from_ptr(service_name)
            .to_str()
            .unwrap_or("?")
    };
    println!(
        "[+] HLE: nn::sm::GetServiceHandle('{}') called -> returning handle 0x5001",
        name_str
    );
    // The SDK only asks for a service handle when it is about to talk to that service itself. That IPC is not emulated:
    // its request is written to the thread's Horizon IPC buffer, which on this host is Darwin's TLS (thread-specific
    // data; the MIG reply port in slot 2 gets overwritten and later Mach calls fail). The fix is HLE of the nn:: API
    // that led here (symbolize the caller below).
    let fp = crate::guest_reg!(x29);
    // Our frame's return address is the HLE trampoline; the guest's frames follow through the frame-pointer chain.
    let mut chain = Vec::new();
    let mut f = fp;
    for _ in 0..4 {
        if f <= 0x10000 || f & 7 != 0 {
            break;
        }
        chain.push(*((f + 8) as *const usize));
        f = *(f as *const usize);
    }
    eprintln!("[!] SDK opens service '{}' itself (return addresses {:x?}): its IPC is not emulated and would corrupt this thread's TLS", name_str, chain);
    if !handle_out.is_null() {
        *handle_out = 0x5001;
    }
    0
}
