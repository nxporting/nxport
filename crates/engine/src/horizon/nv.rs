// nn::nv as the game calls it: graphics memory allocators and initialisation.

#[no_mangle]
pub(crate) unsafe extern "C" fn set_graphics_allocator(
    _alloc: *const (),
    _free: *const (),
    _realloc: *const (),
    _user: *const (),
) {
    println!("[+] HLE: nv::SetGraphicsAllocator called");
}

#[no_mangle]
pub(crate) unsafe extern "C" fn initialize_graphics(mem: *mut (), size: usize) -> i32 {
    println!(
        "[+] HLE: nv::InitializeGraphics(mem={:p}, size={} MB)",
        mem,
        size / (1024 * 1024)
    );
    0
}

#[no_mangle]
pub(crate) unsafe extern "C" fn set_graphics_devtools_allocator(
    _alloc: *const (),
    _free: *const (),
    _realloc: *const (),
    _user: *const (),
) {
    println!("[+] HLE: nv::SetGraphicsDevtoolsAllocator called");
}
