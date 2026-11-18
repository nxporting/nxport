use super::super::Entry;

pub(in crate::horizon::sdk) static TABLE: &[Entry] = &[
    // ReleaseSemaphore without a count releases one (MK8D's sdk module has only this form; 7.3 has the counted one).
    Entry(
        "_ZN2nn2os16ReleaseSemaphoreEPNS0_13SemaphoreTypeE",
        adapt!(fn(sem: *mut ()) {
            crate::horizon::os::release_semaphore(sem, 1)
        }),
    ),
];
