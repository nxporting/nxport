// NVN syncs and events: what nvnDeviceGetProcAddress hands out by name.

use super::super::super::Entry;

pub(super) static PROCS: &[Entry] = &[
    Entry("nvnSyncInitialize", func!(crate::nvn::api::hle_nvnSyncInitialize(_, _))),
    Entry("nvnSyncFinalize", func!(crate::nvn::api::hle_nvnSyncFinalize(_))),
    Entry("nvnSyncSetDebugLabel", func!(crate::nvn::api::hle_nvnSyncSetDebugLabel(_, _))),
    Entry("nvnSyncWait", func!(crate::nvn::api::hle_nvnSyncWait(_, _))),
    Entry("nvnEventInitialize", func!(crate::nvn::api::hle_nvnEventInitialize(_, _))),
    Entry("nvnEventFinalize", func!(crate::nvn::api::hle_nvnEventFinalize(_))),
    Entry("nvnEventGetValue", func!(crate::nvn::api::hle_nvnEventGetValue(_))),
    Entry("nvnEventSignal", func!(crate::nvn::api::hle_nvnEventSignal(_, _, _))),
];
