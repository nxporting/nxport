// nn::codec.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn5codec19HardwareOpusDecoder8FinalizeEv", func!(crate::horizon::codec::hardware_opus_decoder_finalize(_))),
    Entry("_ZN2nn5codec19HardwareOpusDecoderC1Ei", func!(crate::horizon::codec::hardware_opus_decoder_ctor(_, _))),
    Entry("_ZN2nn5codec24ResetHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::codec::reset_hardware_opus_decoder(_))),
    Entry("_ZN2nn5codec27FinalizeHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::codec::finalize_hardware_opus_decoder(_))),
    Entry("_ZN2nn5codec29InitializeHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeEiiPvmi", func!(crate::horizon::codec::initialize_hardware_opus_decoder(_, _, _, _, _, _))),
    Entry("_ZN2nn5codec30HardwareOpusMultiStreamDecoderC1Ei", func!(crate::horizon::codec::hardware_opus_multi_stream_decoder_ctor(_, _))),
    Entry("_ZN2nn5codec32GetHardwareOpusDecoderSampleRateEPKNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::codec::get_hardware_opus_decoder_sample_rate(_))),
    Entry("_ZN2nn5codec33DecodeOpusInterleavedWithHardwareEPNS0_23HardwareOpusDecoderTypeEPmPiPsmPKvm", func!(crate::horizon::codec::decode_opus_interleaved_with_hardware(_, _, _, _, _, _, _))),
    Entry("_ZN2nn5codec34GetHardwareOpusDecoderChannelCountEPKNS0_23HardwareOpusDecoderTypeE", func!(crate::horizon::codec::get_hardware_opus_decoder_channel_count(_))),
    Entry("_ZN2nn5codec35ResetHardwareOpusMultiStreamDecoderEPNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::codec::reset_hardware_opus_multi_stream_decoder(_))),
    Entry("_ZN2nn5codec36GetHardwareOpusDecoderWorkBufferSizeEii", func!(crate::horizon::codec::get_hardware_opus_decoder_work_buffer_size2(_, _))),
    Entry("_ZN2nn5codec36GetHardwareOpusDecoderWorkBufferSizeEiii", func!(crate::horizon::codec::get_hardware_opus_decoder_work_buffer_size(_, _, _))),
    Entry("_ZN2nn5codec38FinalizeHardwareOpusMultiStreamDecoderEPNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::codec::finalize_hardware_opus_multi_stream_decoder(_))),
    Entry("_ZN2nn5codec40InitializeHardwareOpusMultiStreamDecoderEPNS0_34HardwareOpusMultiStreamDecoderTypeEiiiiPKhPvmi", func!(crate::horizon::codec::initialize_hardware_opus_multi_stream_decoder(_, _, _, _, _, _, _, _, _))),
    Entry("_ZN2nn5codec43GetHardwareOpusMultiStreamDecoderSampleRateEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::codec::get_hardware_opus_multi_stream_decoder_sample_rate(_))),
    Entry("_ZN2nn5codec44DecodeOpusMultiStreamInterleavedWithHardwareEPNS0_34HardwareOpusMultiStreamDecoderTypeEPmPiPsmPKvm", func!(crate::horizon::codec::decode_opus_multi_stream_interleaved_with_hardware(_, _, _, _, _, _, _))),
    Entry("_ZN2nn5codec45GetHardwareOpusMultiStreamDecoderChannelCountEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::codec::get_hardware_opus_multi_stream_decoder_channel_count(_))),
    Entry("_ZN2nn5codec47GetHardwareOpusMultiStreamDecoderProcessingTimeEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::codec::get_hardware_opus_multi_stream_decoder_processing_time(_))),
    Entry("_ZN2nn5codec47GetHardwareOpusMultiStreamDecoderWorkBufferSizeEiiii", func!(crate::horizon::codec::get_hardware_opus_multi_stream_decoder_work_buffer_size(_, _, _, _))),
    Entry("_ZN2nn5codec47GetHardwareOpusMultiStreamDecoderWorkBufferSizeEiiiii", func!(crate::horizon::codec::get_hardware_opus_multi_stream_decoder_work_buffer_size5(_, _, _, _, _))),
    Entry("_ZN2nn5codec49GetHardwareOpusMultiStreamDecoderTotalStreamCountEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::codec::get_hardware_opus_multi_stream_decoder_total_stream_count(_))),
    Entry("_ZN2nn5codec50GetHardwareOpusMultiStreamDecoderStereoStreamCountEPKNS0_34HardwareOpusMultiStreamDecoderTypeE", func!(crate::horizon::codec::get_hardware_opus_multi_stream_decoder_stereo_stream_count(_))),
];
