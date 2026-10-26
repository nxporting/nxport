// nn::sm.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2sm10InitializeEv", func!(crate::horizon::hle::hle_nn_sm_Initialize())),
    Entry("_ZN2nn2sm16GetServiceHandleEPNS_3svc6HandleEPKcm", func!(crate::horizon::hle::hle_nn_sm_GetServiceHandle(_, _, _))),
    Entry("_ZN2nn2sm8FinalizeEv", func!(crate::horizon::hle::hle_nn_sm_Finalize())),
];
