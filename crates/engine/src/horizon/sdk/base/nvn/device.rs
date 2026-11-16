// NVN the device: what nvnDeviceGetProcAddress hands out by name.

use super::super::super::Entry;

pub(super) static PROCS: &[Entry] = &[
    Entry("nvnDeviceGetTextureHandle", func!(crate::nvn::state::hle_nvnDeviceGetTextureHandle(_, _, _))),
    Entry("nvnDeviceGetTexelFetchHandle", func!(crate::nvn::state::hle_nvnDeviceGetTexelFetchHandle(_, _))),
    Entry("nvnDeviceGetImageHandle", func!(crate::nvn::state::hle_nvnDeviceGetImageHandle(_, _))),
    Entry("nvnDeviceGetSeparateTextureHandle", func!(crate::nvn::state::hle_nvnDeviceGetSeparateTextureHandle(_, _))),
    Entry("nvnDeviceGetSeparateSamplerHandle", func!(crate::nvn::state::hle_nvnDeviceGetSeparateSamplerHandle(_, _))),
    Entry("nvnDeviceInitialize", func!(crate::nvn::api::hle_nvnDeviceInitialize(_, _))),
    Entry("nvnDeviceGetInteger", func!(crate::nvn::api::hle_nvnDeviceGetInteger(_, _, _))),
    Entry("nvnDeviceBuilderSetDefaults", func!(crate::nvn::api::hle_nvnDeviceBuilderSetDefaults(_))),
    Entry("nvnDeviceBuilderSetFlags", func!(crate::nvn::api::hle_nvnDeviceBuilderSetFlags(_, _))),
    Entry("nvnDeviceRegisterFastClearColor", func!(crate::nvn::api::hle_nvnDeviceRegisterFastClearColor(_, _, _))),
    Entry("nvnDeviceGetTimestampInNanoseconds", func!(crate::nvn::api::hle_nvnDeviceGetTimestampInNanoseconds(_, _))),
    Entry("nvnDeviceSetWindowOriginMode", func!(crate::nvn::api::hle_nvnDeviceSetWindowOriginMode(_, _))),
    Entry("nvnDeviceSetDepthMode", func!(crate::nvn::api::hle_nvnDeviceSetDepthMode(_, _))),
    Entry("nvnDeviceSetDebugLabel", func!(crate::nvn::api::hle_nvn_noop_setter(_, _, _))),
    Entry("nvnDeviceGetDepthMode", func!(crate::nvn::api::hle_nvnDeviceGetDepthMode(_))),
    Entry("nvnDeviceGetWindowOriginMode", func!(crate::nvn::api::hle_nvnDeviceGetWindowOriginMode(_))),
    Entry("nvnDeviceGetCurrentTimestampInNanoseconds", func!(crate::nvn::api::hle_nvnDeviceGetCurrentTimestampInNanoseconds(_))),
    Entry("nvnDeviceBuilderGetFlags", func!(crate::nvn::api::hle_nvnDeviceBuilderGetFlags(_))),
];
