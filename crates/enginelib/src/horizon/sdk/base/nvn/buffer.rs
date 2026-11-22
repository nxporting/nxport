// NVN buffers: what nvnDeviceGetProcAddress hands out by name.

use super::super::super::Entry;

pub(super) static PROCS: &[Entry] = &[
    Entry("nvnBufferBuilderSetDefaults", func!(crate::nvn::api::hle_nvnBufferBuilderSetDefaults(_))),
    Entry("nvnBufferBuilderSetDevice", func!(crate::nvn::api::hle_nvnBufferBuilderSetDevice(_, _))),
    Entry("nvnBufferBuilderSetStorage", func!(crate::nvn::api::hle_nvnBufferBuilderSetStorage(_, _, _, _))),
    Entry("nvnBufferBuilderGetSize", func!(crate::nvn::api::hle_nvnBufferBuilderGetSize(_))),
    Entry("nvnBufferBuilderGetMemoryOffset", func!(crate::nvn::api::hle_nvnBufferBuilderGetMemoryOffset(_))),
    Entry("nvnBufferBuilderGetMemoryPool", func!(crate::nvn::api::hle_nvnBufferBuilderGetMemoryPool(_))),
    Entry("nvnBufferInitialize", func!(crate::nvn::api::hle_nvnBufferInitialize(_, _))),
    Entry("nvnBufferFinalize", func!(crate::nvn::api::hle_nvnBufferFinalize(_))),
    Entry("nvnBufferGetAddress", func!(crate::nvn::api::hle_nvnBufferGetAddress(_))),
    Entry("nvnBufferMap", func!(crate::nvn::api::hle_nvnBufferMap(_))),
    Entry("nvnBufferGetSize", func!(crate::nvn::api::hle_nvnBufferGetSize(_))),
    Entry("nvnBufferFlushMappedRange", func!(crate::nvn::api::hle_nvnBufferFlushMappedRange(_, _, _))),
    Entry("nvnBufferInvalidateMappedRange", func!(crate::nvn::api::hle_nvnBufferInvalidateMappedRange(_, _, _))),
    Entry("nvnBufferBuilderGetDevice", func!(crate::nvn::api::hle_nvnBufferBuilderGetDevice(_))),
    Entry("nvnBufferGetMemoryPool", func!(crate::nvn::api::hle_nvnBufferGetMemoryPool(_))),
    Entry("nvnBufferGetMemoryOffset", func!(crate::nvn::api::hle_nvnBufferGetMemoryOffset(_))),
    Entry("nvnBufferSetDebugLabel", func!(crate::nvn::api::hle_nvn_noop_setter(_, _, _))),
];
