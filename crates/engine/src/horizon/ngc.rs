// nn::ngc as the game calls it: the profanity filter, which accepts every word.

use crate::horizon::nn::RESULT_SUCCESS;
// ── nn::ngc::ProfanityFilter: every word checks clean (ngc:u is not emulated; the SDK's Initialize opens it) ──────────
pub(crate) unsafe extern "C" fn ngc_ok() -> u32 {
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ngc_check_words(
    _this: *mut u8,
    results: *mut u32,
    _patterns: u32,
    _words: *const usize,
    count: usize,
) -> u32 {
    if !results.is_null() && count < 0x10000 {
        std::ptr::write_bytes(results, 0, count);
    }
    RESULT_SUCCESS
}
