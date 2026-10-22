// C library.

use super::super::{Bind, Entry};

pub(super) static TABLE: &[Entry] = &[
    Entry("__errno_location", func!(libc::__error())),
    Entry("__nnmusl_init_dso", func!(crate::horizon::hle::hle_nnmusl_init_dso(_, _, _, _, _, _, _, _))),
    Entry("__syscall_cond_broadcast", func!(crate::guest::module_loader::sys_cond_broadcast(_))),
    Entry("__syscall_cond_signal", func!(crate::guest::module_loader::sys_cond_signal(_))),
    Entry("__syscall_cond_wait", func!(crate::guest::module_loader::sys_cond_wait(_, _, _))),
    Entry("__syscall_cond_wait_recursive", func!(crate::guest::module_loader::sys_cond_wait_recursive(_, _, _))),
    Entry("__syscall_exit", func!(crate::guest::module_loader::sys_exit(_))),
    Entry("__syscall_getreent", func!(crate::guest::module_loader::sys_getreent())),
    Entry("__syscall_lock_acquire", func!(crate::guest::module_loader::sys_lock_acquire(_))),
    Entry("__syscall_lock_acquire_recursive", func!(crate::guest::module_loader::sys_lock_acquire_recursive(_))),
    Entry("__syscall_lock_release", func!(crate::guest::module_loader::sys_lock_release(_))),
    Entry("__syscall_lock_release_recursive", func!(crate::guest::module_loader::sys_lock_release_recursive(_))),
    Entry("__syscall_lock_try_acquire", func!(crate::guest::module_loader::sys_lock_try_acquire(_))),
    Entry("__syscall_lock_try_acquire_recursive", func!(crate::guest::module_loader::sys_lock_try_acquire_recursive(_))),
    Entry("__syscall_tls_create", func!(crate::guest::module_loader::sys_tls_create(_, _))),
    Entry("__syscall_tls_delete", func!(crate::guest::module_loader::sys_tls_delete(_))),
    Entry("__syscall_tls_get", func!(crate::guest::module_loader::sys_tls_get(_))),
    Entry("__syscall_tls_set", func!(crate::guest::module_loader::sys_tls_set(_, _))),
    Entry("clock", func!(super::super::clock())),
    Entry("fflush", func!(libc::fflush(_))),
    Entry("memcmp", func!(libc::memcmp(_, _, _))),
    Entry("memcpy", func!(crate::horizon::hle::hle_memcpy(_, _, _))),
    Entry("memmove", func!(crate::horizon::hle::hle_memmove(_, _, _))),
    Entry("memset", func!(crate::horizon::hle::hle_memset(_, _, _))),
    Entry("printf", Bind::Raw(super::super::Fp(libc::printf as *const ()))),
    // glslc* are WEAK imports (STB_WEAK): retail has no glslc, so they stay null and nn::gfx's GlslcDll::Initialize()
    // fails exactly as on hardware. Binding HLE stubs here made the game believe the shader compiler existed.
    Entry("stdout", Bind::Data(|| std::ptr::addr_of_mut!(super::super::__stdoutp) as usize)),
    Entry("strcmp", func!(crate::horizon::hle::hle_strcmp(_, _))),
    Entry("strlen", func!(libc::strlen(_))),
    Entry("time", func!(libc::time(_))),
];
