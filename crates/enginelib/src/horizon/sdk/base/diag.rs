// nn::diag.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn4diag12GetBacktraceEPmi", func!(crate::horizon::diag::get_backtrace(_, _))),
    // nn::diag / nn::detail
    Entry("_ZN2nn4diag13GetSymbolNameEPcmm", func!(crate::horizon::diag::get_symbol_name(_, _, _))),
    Entry("_ZN2nn4diag18IsDebuggerAttachedEv", func!(crate::horizon::diag::is_debugger_attached())),
    Entry("_ZN2nn4diag6detail18IsDebuggerAttachedEv", func!(crate::horizon::diag::is_debugger_attached())),
    Entry("_ZN2nn4diag6detail5AbortEPKNS_6ResultE", func!(crate::horizon::diag::detail_abort(_))),
    Entry("_ZN2nn4diag6detail7PutImplERKNS0_11LogMetaDataEPKcm", func!(crate::horizon::diag::put_impl(_, _, _))),
    Entry("_ZN2nn4diag6detail16PrintDebugStringEPKcm", func!(crate::horizon::diag::print_debug_string(_, _))),
    Entry("_ZN2nn4diag6detail16PrintDebugStringEPKc", func!(crate::horizon::diag::print_debug_string_z(_))),
    Entry("_ZN2nn4diag6detail9AbortImplEv", func!(crate::horizon::diag::abort_impl_noargs())),
];
