// C++ runtime.

use super::super::{Bind, Entry};

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZSt7nothrow", Bind::Data(|| &crate::horizon::hle::STD_NOTHROW as *const _ as usize)),
    Entry("_ZTVN10__cxxabiv117__class_type_infoE", Bind::Data(|| crate::horizon::hle::rtti_class_type_info_vtable_addr())),
    Entry("_ZTVN10__cxxabiv120__si_class_type_infoE", Bind::Data(|| crate::horizon::hle::rtti_si_class_type_info_vtable_addr())),
    Entry("__cxa_deleted_virtual", func!(crate::horizon::hle::hle_cxa_deleted_virtual)),
    Entry("__cxa_guard_acquire", func!(crate::horizon::hle::hle_cxa_guard_acquire)),
    Entry("__cxa_guard_release", func!(crate::horizon::hle::hle_cxa_guard_release)),
    Entry("__cxa_pure_virtual", func!(crate::horizon::hle::hle_cxa_pure_virtual)),
];
