// nn::crypto.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[Entry(
    "_ZN2nn6crypto36GenerateCryptographicallyRandomBytesEPvm",
    func!(crate::horizon::crypto::generate_cryptographically_random_bytes(_, _)),
)];
