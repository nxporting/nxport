// nn::err.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // nn::err (ErrorCode overload)
    Entry("_ZN2nn3err9ShowErrorENS0_9ErrorCodeE", func!(crate::horizon::nn::err_show_error_code)),
    Entry("_ZN2nn3err9ShowErrorENS_6ResultE", func!(crate::horizon::nn::err_show_error)),
];
