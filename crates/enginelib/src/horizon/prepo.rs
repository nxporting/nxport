// nn::prepo as the game calls it: play reports, accepted and dropped.

use super::nn::*;

pub(crate) unsafe extern "C" fn zero_u32() -> u32 {
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn save_result(_this: *mut u8) -> u32 {
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn get_size(_this: *const u8) -> usize {
    0
}

/// nn::prepo::Initialize: would open the prepo service (play reports are discarded, see `lookup`).
pub(crate) unsafe extern "C" fn initialize() {}
