// nn::swkbd.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn5swkbd10MakePresetEPNS0_14KeyboardConfigENS0_6PresetE", func!(crate::horizon::nn::swkbd_make_preset)),
    Entry("_ZN2nn5swkbd12ShowKeyboardEPNS0_6StringERKNS0_15ShowKeyboardArgE", func!(crate::horizon::nn::swkbd_show_keyboard)),
    // nn::swkbd
    Entry("_ZN2nn5swkbd25GetRequiredWorkBufferSizeEb", func!(crate::horizon::nn::swkbd_get_required_work_buffer_size)),
    Entry("_ZN2nn5swkbd27GetRequiredStringBufferSizeEv", func!(crate::horizon::nn::swkbd_get_required_string_buffer_size)),
];
