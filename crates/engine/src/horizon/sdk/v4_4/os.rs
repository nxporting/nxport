use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // ReleaseSemaphore without a count releases one (MK8D's sdk module has only this form; 7.3 has the counted one).
    Entry(
        "_ZN2nn2os16ReleaseSemaphoreEPNS0_13SemaphoreTypeE",
        adapt!(fn(sem: *mut ()) {
            crate::horizon::hle::hle_nn_os_ReleaseSemaphore(sem, 1)
        }),
    ),
];
