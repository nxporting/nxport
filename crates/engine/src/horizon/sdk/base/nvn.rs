// NVN bootstrap.

use super::super::{Bind, Entry};

pub(super) static TABLE: &[Entry] = &[
    Entry("nvnBootstrapLoader", func!(crate::nvn::api::hle_nvnBootstrapLoader(_))),
    Entry("nvnPrivate0", Bind::Data(|| std::ptr::addr_of_mut!(crate::horizon::hle::NVN_PRIVATE0) as usize)),
];
