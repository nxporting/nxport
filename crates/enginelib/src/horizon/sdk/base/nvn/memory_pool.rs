// NVN memory pools: what nvnDeviceGetProcAddress hands out by name.

use super::super::super::Entry;

pub(super) static PROCS: &[Entry] = &[
    Entry("nvnMemoryPoolBuilderSetDefaults", func!(crate::nvn::api::hle_nvnMemoryPoolBuilderSetDefaults(_))),
    Entry("nvnMemoryPoolBuilderSetDevice", func!(crate::nvn::api::hle_nvnMemoryPoolBuilderSetDevice(_, _))),
    Entry("nvnMemoryPoolBuilderSetStorage", func!(crate::nvn::api::hle_nvnMemoryPoolBuilderSetStorage(_, _, _))),
    Entry("nvnMemoryPoolBuilderSetFlags", func!(crate::nvn::api::hle_nvnMemoryPoolBuilderSetFlags(_, _))),
    Entry("nvnMemoryPoolBuilderGetDevice", func!(crate::nvn::api::hle_nvnMemoryPoolBuilderGetDevice(_))),
    Entry("nvnMemoryPoolBuilderGetMemory", func!(crate::nvn::api::hle_nvnMemoryPoolBuilderGetMemory(_))),
    Entry("nvnMemoryPoolBuilderGetSize", func!(crate::nvn::api::hle_nvnMemoryPoolBuilderGetSize(_))),
    Entry("nvnMemoryPoolBuilderGetFlags", func!(crate::nvn::api::hle_nvnMemoryPoolBuilderGetFlags(_))),
    Entry("nvnMemoryPoolInitialize", func!(crate::nvn::api::hle_nvnMemoryPoolInitialize(_, _))),
    Entry("nvnMemoryPoolFinalize", func!(crate::nvn::api::hle_nvnMemoryPoolFinalize(_))),
    Entry("nvnMemoryPoolMap", func!(crate::nvn::api::hle_nvnMemoryPoolMap(_))),
    Entry("nvnMemoryPoolGetBufferAddress", func!(crate::nvn::api::hle_nvnMemoryPoolGetBufferAddress(_))),
    Entry("nvnMemoryPoolSetDebugLabel", func!(crate::nvn::api::hle_nvnMemoryPoolSetDebugLabel(_, _))),
    Entry("nvnMemoryPoolFlushMappedRange", func!(crate::nvn::api::hle_nvnMemoryPoolFlushMappedRange(_, _, _))),
    Entry("nvnMemoryPoolInvalidateMappedRange", func!(crate::nvn::api::hle_nvnMemoryPoolInvalidateMappedRange(_, _, _))),
    Entry("nvnMemoryPoolGetFlags", func!(crate::nvn::api::hle_nvnMemoryPoolGetFlags(_))),
    Entry("nvnMemoryPoolMapVirtual", func!(crate::nvn::api::hle_nvnMemoryPoolMapVirtual(_, _, _))),
    Entry("nvnMemoryPoolGetSize", func!(crate::nvn::api::hle_nvnMemoryPoolGetSize(_))),
];
