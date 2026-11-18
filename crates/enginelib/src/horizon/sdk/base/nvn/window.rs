// NVN windows: what nvnDeviceGetProcAddress hands out by name.

use super::super::super::Entry;

pub(super) static PROCS: &[Entry] = &[
    Entry("nvnWindowBuilderSetPresentInterval", func!(crate::nvn::api::hle_nvnWindowSetPresentInterval(_, _))),
    Entry("nvnWindowBuilderGetPresentInterval", func!(crate::nvn::api::hle_nvnWindowGetPresentInterval(_))),
    Entry("nvnWindowBuilderSetNumActiveTextures", func!(crate::nvn::api::hle_nvnWindowBuilderSetNumActiveTextures(_, _))),
    Entry("nvnWindowBuilderGetDevice", func!(crate::nvn::api::hle_nvnWindowBuilderGetDevice(_))),
    Entry("nvnWindowBuilderGetNumTextures", func!(crate::nvn::api::hle_nvnWindowBuilderGetNumTextures(_))),
    Entry("nvnWindowBuilderGetTexture", func!(crate::nvn::api::hle_nvnWindowBuilderGetTexture(_, _))),
    Entry("nvnWindowBuilderGetNumActiveTextures", func!(crate::nvn::api::hle_nvnWindowBuilderGetNumActiveTextures(_))),
    Entry("nvnWindowGetNumTextures", func!(crate::nvn::api::hle_nvnWindowGetNumTextures(_))),
    Entry("nvnWindowGetNumActiveTextures", func!(crate::nvn::api::hle_nvnWindowGetNumActiveTextures(_))),
    Entry("nvnWindowGetCrop", func!(crate::nvn::api::hle_nvnWindowGetCrop(_, _))),
    Entry("nvnWindowBuilderSetDefaults", func!(crate::nvn::api::hle_nvnWindowBuilderSetDefaults(_))),
    Entry("nvnWindowBuilderSetDevice", func!(crate::nvn::api::hle_nvnWindowBuilderSetDevice(_, _))),
    Entry("nvnWindowBuilderSetTextures", func!(crate::nvn::api::hle_nvnWindowBuilderSetTextures(_, _, _))),
    Entry("nvnWindowInitialize", func!(crate::nvn::api::hle_nvnWindowInitialize(_, _))),
    Entry("nvnWindowFinalize", func!(crate::nvn::api::hle_nvnWindowFinalize(_))),
    Entry("nvnWindowAcquireTexture", func!(crate::nvn::api::hle_nvnWindowAcquireTexture(_, _, _))),
    Entry("nvnWindowSetCrop", func!(crate::nvn::api::hle_nvnWindowSetCrop(_, _, _, _, _))),
    Entry("nvnWindowSetNumActiveTextures", func!(crate::nvn::api::hle_nvnWindowSetNumActiveTextures(_, _))),
];
