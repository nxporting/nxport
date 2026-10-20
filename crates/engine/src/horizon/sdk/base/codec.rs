// nn::codec.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn5codec19HardwareOpusDecoder8FinalizeEv", func!(crate::horizon::hle::hle_nn_codec_HardwareOpusDecoder_Finalize)),
    Entry("_ZN2nn5codec24ResetHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_ResetHardwareOpusDecoder)),
    Entry("_ZN2nn5codec27FinalizeHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_FinalizeHardwareOpusDecoder)),
    Entry("_ZN2nn5codec29InitializeHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeEiiPvmi", func!(crate::horizon::hle::hle_nn_codec_InitializeHardwareOpusDecoder)),
    Entry("_ZN2nn5codec32GetHardwareOpusDecoderSampleRateEPKNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusDecoderSampleRate)),
    Entry("_ZN2nn5codec33DecodeOpusInterleavedWithHardwareEPNS0_23HardwareOpusDecoderTypeEPmPiPsmPKvm", func!(crate::horizon::hle::hle_nn_codec_DecodeOpusInterleavedWithHardware)),
    Entry("_ZN2nn5codec34GetHardwareOpusDecoderChannelCountEPKNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusDecoderChannelCount)),
    Entry("_ZN2nn5codec36GetHardwareOpusDecoderWorkBufferSizeEii", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize2)),
    // nn::codec hardware Opus decoder: IPC to hwopus, so HLE.
    Entry("_ZN2nn5codec36GetHardwareOpusDecoderWorkBufferSizeEiii", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize)),
];
