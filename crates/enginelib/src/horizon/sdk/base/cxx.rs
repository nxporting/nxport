// C++ runtime.

use super::super::{Bind, Entry};

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZSt7nothrow", Bind::Data(|| &crate::horizon::cxx::STD_NOTHROW as *const _ as usize)),
    Entry("__cxa_deleted_virtual", func!(crate::horizon::cxx::hle_cxa_deleted_virtual())),
    Entry("__cxa_guard_acquire", func!(crate::horizon::cxx::hle_cxa_guard_acquire(_))),
    Entry("__cxa_guard_release", func!(crate::horizon::cxx::hle_cxa_guard_release(_))),
    Entry("__cxa_pure_virtual", func!(crate::horizon::cxx::hle_cxa_pure_virtual())),
];
