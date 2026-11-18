// nn::audio.

use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    Entry("_ZN2nn5audio17OpenAudioRendererEPNS0_19AudioRendererHandleEPNS_2os11SystemEventERKNS0_22AudioRendererParameterEPvm", func!(crate::horizon::audio::api::open_audio_renderer(_, _, _, _, _))),
    Entry("_ZN2nn5audio17StopAudioRendererENS0_19AudioRendererHandleE", func!(crate::horizon::audio::api::stop_audio_renderer(_))),
    Entry("_ZN2nn5audio18CloseAudioRendererENS0_19AudioRendererHandleE", func!(crate::horizon::audio::api::close_audio_renderer(_))),
    Entry("_ZN2nn5audio18StartAudioRendererENS0_19AudioRendererHandleE", func!(crate::horizon::audio::api::start_audio_renderer(_))),
    Entry("_ZN2nn5audio19ListAudioDeviceNameEPNS0_15AudioDeviceNameEi", func!(crate::horizon::audio::api::list_audio_output_device_name(_, _))),
    Entry("_ZN2nn5audio21GetActiveChannelCountEv", func!(crate::horizon::audio::api::get_active_audio_device_channel_count_for_output())),
    // RequestUpdateAudioRenderer runs natively: it serializes the config and calls the IAudioRenderer service object
    // that OpenAudioRenderer hands out (horizon/audio/renderer.rs), then parses the statuses back.
    Entry("_ZN2nn5audio24GetActiveAudioDeviceNameEPNS0_15AudioDeviceNameE", func!(crate::horizon::audio::api::get_active_audio_output_device_name(_))),
    Entry("_ZN2nn5audio25ListAudioOutputDeviceNameEPNS0_15AudioDeviceNameEi", func!(crate::horizon::audio::api::list_audio_output_device_name(_, _))),
    Entry("_ZN2nn5audio26SetAudioDeviceOutputVolumeEPKNS0_15AudioDeviceNameEf", func!(crate::horizon::audio::api::set_audio_device_output_volume(_, _))),
    // RequestUpdateAudioRenderer runs natively: it serializes the config and calls the IAudioRenderer service object
    // that OpenAudioRenderer hands out (horizon/audio/renderer.rs), then parses the statuses back.
    Entry("_ZN2nn5audio30GetActiveAudioOutputDeviceNameEPNS0_15AudioDeviceNameE", func!(crate::horizon::audio::api::get_active_audio_output_device_name(_))),
    // nn::audio renderer / output-device entry points: IPC to audren, so HLE.
    Entry("_ZN2nn5audio30GetAudioRendererWorkBufferSizeERKNS0_22AudioRendererParameterE", func!(crate::horizon::audio::api::get_audio_renderer_work_buffer_size(_))),
    Entry("_ZN2nn5audio36AcquireAudioDeviceSwitchNotificationEPNS_2os11SystemEventE", func!(crate::horizon::audio::api::acquire_audio_device_notification_for_output(_))),
    Entry("_ZN2nn5audio39AcquireAudioDeviceNotificationForOutputEPNS_2os11SystemEventE", func!(crate::horizon::audio::api::acquire_audio_device_notification_for_output(_))),
    Entry("_ZN2nn5audio3dsp14FlushDataCacheEPKvm", func!(crate::horizon::os::cache_noop(_, _))),
    Entry("_ZN2nn5audio3dsp19InvalidateDataCacheEPvm", func!(crate::horizon::os::cache_noop(_, _))),
    Entry("_ZN2nn5audio41GetActiveAudioDeviceChannelCountForOutputEv", func!(crate::horizon::audio::api::get_active_audio_device_channel_count_for_output())),
];
