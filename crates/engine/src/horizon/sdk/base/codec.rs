// nn::codec.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn5codec19HardwareOpusDecoder8FinalizeEv", func!(crate::horizon::hle::hle_nn_codec_HardwareOpusDecoder_Finalize(_))),
    Entry("_ZN2nn5codec19HardwareOpusDecoderC1Ei", func!(crate::horizon::hle::hle_nn_codec_HardwareOpusDecoder_Ctor(_, _))),
    Entry("_ZN2nn5codec24ResetHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_ResetHardwareOpusDecoder(_))),
    Entry("_ZN2nn5codec27FinalizeHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_FinalizeHardwareOpusDecoder(_))),
    Entry("_ZN2nn5codec29InitializeHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeEiiPvmi", func!(crate::horizon::hle::hle_nn_codec_InitializeHardwareOpusDecoder(_, _, _, _, _, _))),
    Entry("_ZN2nn5codec30HardwareOpusMultiStreamDecoderC1Ei", func!(crate::horizon::hle::hle_nn_codec_HardwareOpusMultiStreamDecoder_Ctor(_, _))),
    Entry("_ZN2nn5codec32GetHardwareOpusDecoderSampleRateEPKNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusDecoderSampleRate(_))),
    Entry("_ZN2nn5codec33DecodeOpusInterleavedWithHardwareEPNS0_23HardwareOpusDecoderTypeEPmPiPsmPKvm", func!(crate::horizon::hle::hle_nn_codec_DecodeOpusInterleavedWithHardware(_, _, _, _, _, _, _))),
    Entry("_ZN2nn5codec34GetHardwareOpusDecoderChannelCountEPKNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusDecoderChannelCount(_))),
    Entry("_ZN2nn5codec35ResetHardwareOpusMultiStreamDecoderEPNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_ResetHardwareOpusMultiStreamDecoder(_))),
    Entry("_ZN2nn5codec36GetHardwareOpusDecoderWorkBufferSizeEii", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize2(_, _))),
    Entry("_ZN2nn5codec36GetHardwareOpusDecoderWorkBufferSizeEiii", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize(_, _, _))),
    Entry("_ZN2nn5codec38FinalizeHardwareOpusMultiStreamDecoderEPNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_FinalizeHardwareOpusMultiStreamDecoder(_))),
    Entry("_ZN2nn5codec40InitializeHardwareOpusMultiStreamDecoderEPNS0_34HardwareOpusMultiStreamDecoderTypeEiiiiPKhPvmi", func!(crate::horizon::hle::hle_nn_codec_InitializeHardwareOpusMultiStreamDecoder(_, _, _, _, _, _, _, _, _))),
    Entry("_ZN2nn5codec43GetHardwareOpusMultiStreamDecoderSampleRateEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusMultiStreamDecoderSampleRate(_))),
    Entry("_ZN2nn5codec44DecodeOpusMultiStreamInterleavedWithHardwareEPNS0_34HardwareOpusMultiStreamDecoderTypeEPmPiPsmPKvm", func!(crate::horizon::hle::hle_nn_codec_DecodeOpusMultiStreamInterleavedWithHardware(_, _, _, _, _, _, _))),
    Entry("_ZN2nn5codec45GetHardwareOpusMultiStreamDecoderChannelCountEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusMultiStreamDecoderChannelCount(_))),
    Entry("_ZN2nn5codec47GetHardwareOpusMultiStreamDecoderProcessingTimeEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusMultiStreamDecoderProcessingTime(_))),
    Entry("_ZN2nn5codec47GetHardwareOpusMultiStreamDecoderWorkBufferSizeEiiii", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusMultiStreamDecoderWorkBufferSize(_, _, _, _))),
    Entry("_ZN2nn5codec47GetHardwareOpusMultiStreamDecoderWorkBufferSizeEiiiii", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusMultiStreamDecoderWorkBufferSize5(_, _, _, _, _))),
    Entry("_ZN2nn5codec49GetHardwareOpusMultiStreamDecoderTotalStreamCountEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusMultiStreamDecoderTotalStreamCount(_))),
    Entry("_ZN2nn5codec50GetHardwareOpusMultiStreamDecoderStereoStreamCountEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::hle::hle_nn_codec_GetHardwareOpusMultiStreamDecoderStereoStreamCount(_))),
];
