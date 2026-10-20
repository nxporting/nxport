// nn::diag.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn4diag12GetBacktraceEPmi", func!(crate::horizon::nn::diag_get_backtrace)),
    // nn::diag / nn::detail
    Entry("_ZN2nn4diag13GetSymbolNameEPcmm", func!(crate::horizon::nn::diag_get_symbol_name)),
    Entry("_ZN2nn4diag18IsDebuggerAttachedEv", func!(crate::horizon::hle::hle_nn_diag_IsDebuggerAttached)),
    Entry("_ZN2nn4diag6detail18IsDebuggerAttachedEv", func!(crate::horizon::hle::hle_nn_diag_IsDebuggerAttached)),
    Entry("_ZN2nn4diag6detail5AbortEPKNS_6ResultE", func!(crate::horizon::hle::hle_nn_diag_detail_Abort)),
    Entry("_ZN2nn4diag6detail6PutImplERKNS0_11LogMetaDataEPKcm", func!(crate::horizon::nn::diag_put_impl)),
    Entry("_ZN2nn4diag6detail9AbortImplEv", func!(crate::horizon::nn::diag_abort_impl_noargs)),
];
