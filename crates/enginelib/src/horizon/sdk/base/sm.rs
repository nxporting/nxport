// nn::sm.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2sm10InitializeEv", func!(crate::horizon::sm::initialize())),
    Entry("_ZN2nn2sm16GetServiceHandleEPNS_3svc6HandleEPKcm", func!(crate::horizon::sm::get_service_handle(_, _, _))),
    Entry("_ZN2nn2sm8FinalizeEv", func!(crate::horizon::sm::finalize())),
];
