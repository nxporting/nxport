// C library.

use super::super::{Bind, Entry};

pub(super) static TABLE: &[Entry] = &[
    Entry("__errno_location", func!(libc::__error)),
    Entry("__nnmusl_init_dso", func!(crate::horizon::hle::hle_nnmusl_init_dso)),
    Entry("__syscall_cond_broadcast", func!(crate::guest::module_loader::sys_cond_broadcast)),
    Entry("__syscall_cond_signal", func!(crate::guest::module_loader::sys_cond_signal)),
    Entry("__syscall_cond_wait", func!(crate::guest::module_loader::sys_cond_wait)),
    Entry("__syscall_cond_wait_recursive", func!(crate::guest::module_loader::sys_cond_wait_recursive)),
    Entry("__syscall_exit", func!(crate::guest::module_loader::sys_exit)),
    Entry("__syscall_getreent", func!(crate::guest::module_loader::sys_getreent)),
    Entry("__syscall_lock_acquire", func!(crate::guest::module_loader::sys_lock_acquire)),
    Entry("__syscall_lock_acquire_recursive", func!(crate::guest::module_loader::sys_lock_acquire_recursive)),
    Entry("__syscall_lock_release", func!(crate::guest::module_loader::sys_lock_release)),
    Entry("__syscall_lock_release_recursive", func!(crate::guest::module_loader::sys_lock_release_recursive)),
    Entry("__syscall_lock_try_acquire", func!(crate::guest::module_loader::sys_lock_try_acquire)),
    Entry("__syscall_lock_try_acquire_recursive", func!(crate::guest::module_loader::sys_lock_try_acquire_recursive)),
    Entry("__syscall_tls_create", func!(crate::guest::module_loader::sys_tls_create)),
    Entry("__syscall_tls_delete", func!(crate::guest::module_loader::sys_tls_delete)),
    Entry("__syscall_tls_get", func!(crate::guest::module_loader::sys_tls_get)),
    Entry("__syscall_tls_set", func!(crate::guest::module_loader::sys_tls_set)),
    Entry("clock", func!(super::super::clock)),
    Entry("fflush", func!(libc::fflush)),
    Entry("memcmp", func!(libc::memcmp)),
    Entry("memcpy", func!(crate::horizon::hle::hle_memcpy)),
    Entry("memmove", func!(crate::horizon::hle::hle_memmove)),
    Entry("memset", func!(crate::horizon::hle::hle_memset)),
    Entry("printf", func!(libc::printf)),
    // glslc* are WEAK imports (STB_WEAK): retail has no glslc, so they stay null and nn::gfx's GlslcDll::Initialize()
    // fails exactly as on hardware. Binding HLE stubs here made the game believe the shader compiler existed.
    Entry("stdout", Bind::Data(|| std::ptr::addr_of_mut!(super::super::__stdoutp) as usize)),
    Entry("strcmp", func!(crate::horizon::hle::hle_strcmp)),
    Entry("strlen", func!(libc::strlen)),
    Entry("time", func!(libc::time)),
];
