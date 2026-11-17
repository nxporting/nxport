// nn::audio as the game calls it: opening and running audio renderers, and the output device.

use crate::horizon::call_log::*;
use crate::horizon::hle::*;

pub(crate) fn align_up(v: usize, a: usize) -> usize {
    (v + a - 1) & !(a - 1)
}

/// nn::audio::GetAudioRendererWorkBufferSize(const AudioRendererParameter&).
/// The buffer only backs the audren process's state, which we don't run, so
/// this just needs to be a valid 4KB-aligned size that scales with the params.
pub(crate) unsafe extern "C" fn get_audio_renderer_work_buffer_size(param: *const i32) -> usize {
    record_hle_call("audio::get_audio_renderer_work_buffer_size");
    // AudioRendererParameter: sampleRate, sampleCount, mixBufferCount, subMixCount,
    // voiceCount, sinkCount, effectCount, ...
    let (mix, sub, voice, sink, fx) = if param.is_null() {
        (24, 8, 96, 4, 16)
    } else {
        let g = |i: usize| (*param.add(i)).max(0) as usize;
        (g(2), g(3), g(4), g(5), g(6))
    };
    crate::horizon::audio::api::align_up(
        0x40000 + voice * 0x800 + sub * 0x1000 + mix * 0x400 + sink * 0x400 + fx * 0x2000,
        0x1000,
    )
}

/// nn::audio::OpenAudioRenderer(AudioRendererHandle*, SystemEvent*, const AudioRendererParameter&, void*, size_t)
pub(crate) unsafe extern "C" fn open_audio_renderer(
    handle: *mut usize,
    event: *mut (),
    param: *const (),
    _work: *mut (),
    _work_size: usize,
) -> u32 {
    record_hle_call("audio::open_audio_renderer");
    if !event.is_null() {
        let ev = hle_system_event_register(event as usize);
        nn_audio_start_frame_pacer(ev);
    }
    if !handle.is_null() {
        *handle = crate::horizon::audio::renderer::open(param as *const u32);
        if crate::horizon::sdk::audio_handle_state() {
            *handle.add(1) = crate::horizon::audio::api::renderer_state(param as *const u32, event as u64);
        }
    }
    NN_AUDIO_RENDERER_STATE.store(1, std::sync::atomic::Ordering::SeqCst);
    println!("[+] HLE: nn::audio::OpenAudioRenderer -> audren service object (timing only, no audio output)");
    NN_RESULT_SUCCESS
}

/// The client state block SDK 20.5's OpenAudioRenderer (BotW 1.9.0 sdk+0x221580) puts in the handle's second word, filled
/// as it fills it: +0x08 / +0x0c / +0x10 sample rate, sample count and mix buffer count from the parameter; +0x14 1;
/// +0x18 the rendering time limit, 100 (%); +0x1c / +0x20 the parameter's words at +0x2c / +0x30; +0x24 the voice drop
/// parameter, 1.0; +0x00 the system event; the rest 0. Lives as long as the process (one renderer).
pub(crate) unsafe fn renderer_state(param: *const u32, event: u64) -> usize {
    let s: &'static mut [u32; 12] = Box::leak(Box::new([0u32; 12]));
    // +0x00: the system event (the five-argument OpenAudioRenderer stores it there after the four-argument one).
    s[0] = event as u32;
    s[1] = (event >> 32) as u32;
    let p = |i: usize| if param.is_null() { 0 } else { *param.add(i) };
    s[2] = p(0);
    s[3] = p(1);
    s[4] = p(2);
    s[5] = 1;
    s[6] = 100;
    s[7] = p(0x2c / 4);
    s[8] = p(0x30 / 4);
    s[9] = 1.0f32.to_bits();
    s.as_mut_ptr() as usize
}

pub(crate) unsafe extern "C" fn start_audio_renderer(_handle: usize) -> u32 {
    record_hle_call("audio::start_audio_renderer");
    NN_AUDIO_RENDERER_STATE.store(2, std::sync::atomic::Ordering::SeqCst);
    crate::horizon::audio::renderer::set_running(true);
    NN_RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn stop_audio_renderer(_handle: usize) {
    record_hle_call("audio::stop_audio_renderer");
    NN_AUDIO_RENDERER_STATE.store(1, std::sync::atomic::Ordering::SeqCst);
    crate::horizon::audio::renderer::set_running(false);
}

pub(crate) unsafe extern "C" fn close_audio_renderer(_handle: usize) {
    record_hle_call("audio::close_audio_renderer");
    NN_AUDIO_RENDERER_STATE.store(0, std::sync::atomic::Ordering::SeqCst);
}

// AudioDeviceName is `char name[256]`.
pub(crate) unsafe fn write_device_name(out: *mut u8) {
    let name = b"AudioStereoJackOutput\0";
    std::ptr::write_bytes(out, 0, 256);
    std::ptr::copy_nonoverlapping(name.as_ptr(), out, name.len());
}

pub(crate) unsafe extern "C" fn get_active_audio_output_device_name(out: *mut u8) {
    record_hle_call("audio::get_active_audio_output_device_name");
    if !out.is_null() {
        crate::horizon::audio::api::write_device_name(out);
    }
}

pub(crate) unsafe extern "C" fn list_audio_output_device_name(out: *mut u8, count: i32) -> i32 {
    record_hle_call("audio::list_audio_output_device_name");
    if out.is_null() || count < 1 {
        return 0;
    }
    crate::horizon::audio::api::write_device_name(out);
    1
}

pub(crate) unsafe extern "C" fn get_active_audio_device_channel_count_for_output() -> i32 {
    record_hle_call("audio::get_active_audio_device_channel_count_for_output");
    2
}

pub(crate) unsafe extern "C" fn set_audio_device_output_volume(_name: *const u8, _volume: f32) {
    record_hle_call("audio::set_audio_device_output_volume");
}

pub(crate) unsafe extern "C" fn acquire_audio_device_notification_for_output(event: *mut ()) {
    record_hle_call("audio::acquire_audio_device_notification_for_output");
    // Signaled only on output-device changes (headphones plugged in, ...); never fires here.
    if !event.is_null() {
        hle_system_event_register(event as usize);
    }
}
