// nn::audio.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn5audio17OpenAudioRendererEPNS0_19AudioRendererHandleEPNS_2os11SystemEventERKNS0_22AudioRendererParameterEPvm", func!(crate::horizon::hle::hle_nn_audio_OpenAudioRenderer)),
    Entry("_ZN2nn5audio17StopAudioRendererENS0_19AudioRendererHandleE", func!(crate::horizon::hle::hle_nn_audio_StopAudioRenderer)),
    Entry("_ZN2nn5audio18CloseAudioRendererENS0_19AudioRendererHandleE", func!(crate::horizon::hle::hle_nn_audio_CloseAudioRenderer)),
    Entry("_ZN2nn5audio18StartAudioRendererENS0_19AudioRendererHandleE", func!(crate::horizon::hle::hle_nn_audio_StartAudioRenderer)),
    Entry("_ZN2nn5audio19ListAudioDeviceNameEPNS0_15AudioDeviceNameEi", func!(crate::horizon::hle::hle_nn_audio_ListAudioOutputDeviceName)),
    Entry("_ZN2nn5audio21GetActiveChannelCountEv", func!(crate::horizon::hle::hle_nn_audio_GetActiveAudioDeviceChannelCountForOutput)),
    // RequestUpdateAudioRenderer runs natively: it serializes the config and calls the IAudioRenderer service object
    // that OpenAudioRenderer hands out (horizon/audio/renderer.rs), then parses the statuses back.
    Entry("_ZN2nn5audio24GetActiveAudioDeviceNameEPNS0_15AudioDeviceNameE", func!(crate::horizon::hle::hle_nn_audio_GetActiveAudioOutputDeviceName)),
    Entry("_ZN2nn5audio25ListAudioOutputDeviceNameEPNS0_15AudioDeviceNameEi", func!(crate::horizon::hle::hle_nn_audio_ListAudioOutputDeviceName)),
    Entry("_ZN2nn5audio26SetAudioDeviceOutputVolumeEPKNS0_15AudioDeviceNameEf", func!(crate::horizon::hle::hle_nn_audio_SetAudioDeviceOutputVolume)),
    // RequestUpdateAudioRenderer runs natively: it serializes the config and calls the IAudioRenderer service object
    // that OpenAudioRenderer hands out (horizon/audio/renderer.rs), then parses the statuses back.
    Entry("_ZN2nn5audio30GetActiveAudioOutputDeviceNameEPNS0_15AudioDeviceNameE", func!(crate::horizon::hle::hle_nn_audio_GetActiveAudioOutputDeviceName)),
    // nn::audio renderer / output-device entry points: IPC to audren, so HLE.
    Entry("_ZN2nn5audio30GetAudioRendererWorkBufferSizeERKNS0_22AudioRendererParameterE", func!(crate::horizon::hle::hle_nn_audio_GetAudioRendererWorkBufferSize)),
    Entry("_ZN2nn5audio36AcquireAudioDeviceSwitchNotificationEPNS_2os11SystemEventE", func!(crate::horizon::hle::hle_nn_audio_AcquireAudioDeviceNotificationForOutput)),
    Entry("_ZN2nn5audio39AcquireAudioDeviceNotificationForOutputEPNS_2os11SystemEventE", func!(crate::horizon::hle::hle_nn_audio_AcquireAudioDeviceNotificationForOutput)),
    Entry("_ZN2nn5audio3dsp14FlushDataCacheEPKvm", func!(crate::horizon::nn::cache_noop)),
    Entry("_ZN2nn5audio3dsp19InvalidateDataCacheEPvm", func!(crate::horizon::nn::cache_noop)),
    Entry("_ZN2nn5audio41GetActiveAudioDeviceChannelCountForOutputEv", func!(crate::horizon::hle::hle_nn_audio_GetActiveAudioDeviceChannelCountForOutput)),
];
