// nn::pl.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn2pl11BeginVrModeEv", func!(crate::horizon::nn::pl_vr_mode)),
    Entry("_ZN2nn2pl13GetSharedFontEPiPNS0_14SharedFontDataEiNS_8settings12LanguageCodeE", func!(crate::horizon::nn::pl_get_shared_font)),
    Entry("_ZN2nn2pl17GetSharedFontSizeENS0_14SharedFontTypeE", func!(crate::horizon::nn::pl_get_shared_font_size)),
    Entry("_ZN2nn2pl20GetSharedFontAddressENS0_14SharedFontTypeE", func!(crate::horizon::nn::pl_get_shared_font_address)),
    Entry("_ZN2nn2pl21RequestSharedFontLoadENS0_14SharedFontTypeE", func!(crate::horizon::nn::pl_request_shared_font_load)),
    Entry("_ZN2nn2pl22GetSharedFontLoadStateENS0_14SharedFontTypeE", func!(crate::horizon::nn::pl_get_shared_font_load_state)),
    Entry("_ZN2nn2pl9EndVrModeEv", func!(crate::horizon::nn::pl_vr_mode)),
];
