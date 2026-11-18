// NVN programs: what nvnDeviceGetProcAddress hands out by name.

use super::super::super::Entry;

pub(super) static PROCS: &[Entry] = &[
    Entry("nvnProgramInitialize", func!(crate::nvn::api::hle_nvnProgramInitialize(_, _))),
    Entry("nvnProgramFinalize", func!(crate::nvn::api::hle_nvnProgramFinalize(_))),
    Entry("nvnProgramSetDebugLabel", func!(crate::nvn::api::hle_nvnProgramSetDebugLabel(_, _))),
    Entry("nvnProgramSetShaders", func!(crate::nvn::api::hle_nvnProgramSetShaders(_, _, _))),
    Entry("nvnProgramSetShadersExt", func!(crate::nvn::api::hle_nvnProgramSetShadersExt(_, _, _))),
    Entry("nvnProgramSetSubroutineLinkage", func!(crate::nvn::api::hle_nvnProgramSetSubroutineLinkage(_, _, _))),
    Entry("nvnProgramSetSampleShading", func!(crate::nvn::api::hle_nvnProgramSetSampleShading(_, _))),
];
