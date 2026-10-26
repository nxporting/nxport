// nn::ssl.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // nn::ssl
    Entry("_ZN2nn3ssl10InitializeEv", func!(crate::horizon::nn::ssl_ok())),
    Entry("_ZN2nn3ssl7Context15ImportServerPkiEPmPKcjNS0_17CertificateFormatE", func!(crate::horizon::nn::ssl_ok())),
    Entry("_ZN2nn3ssl7Context6CreateENS1_10SslVersionE", func!(crate::horizon::nn::ssl_ok())),
];
