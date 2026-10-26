// nn::ngc.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn3ngc15ProfanityFilter10InitializeEPvmb", func!(crate::horizon::nn::ngc_ok())),
    Entry("_ZN2nn3ngc15ProfanityFilter19CheckProfanityWordsEPjjPPKDsm", func!(crate::horizon::nn::ngc_check_words(_, _, _, _, _))),
    Entry("_ZN2nn3ngc15ProfanityFilter19CheckProfanityWordsEPjjPPKcm", func!(crate::horizon::nn::ngc_check_words(_, _, _, _, _))),
    Entry("_ZN2nn3ngc15ProfanityFilter19CheckProfanityWordsEPjjPPKtm", func!(crate::horizon::nn::ngc_check_words(_, _, _, _, _))),
    // nn::ngc
    Entry("_ZN2nn3ngc15ProfanityFilterC1Ev", func!(crate::horizon::nn::friends_void())),
    Entry("_ZN2nn3ngc15ProfanityFilterD1Ev", func!(crate::horizon::nn::friends_void())),
];
