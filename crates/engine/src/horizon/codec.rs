// nn::codec as the game calls it: the hardware Opus decoder.

use std::sync::Mutex as StdMutex;
use crate::horizon::call_log::*;
use super::hle::*;

pub(crate) unsafe extern "C" fn get_hardware_opus_decoder_work_buffer_size(
    rate: i32,
    channels: i32,
    _streams: i32,
) -> usize {
    record_hle_call("codec::get_hardware_opus_decoder_work_buffer_size");
    if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec28GetOpusDecoderWorkBufferSizeEii") {
        return crate::guest::call(sym, &[rate as u64, channels as u64]) as usize;
    }
    crate::horizon::audio::api::align_up(0x10000 + 0x4000 * channels.max(1) as usize, 0x1000)
}

/// GetHardwareOpusDecoderWorkBufferSize(sampleRate, channelCount): the overload without the stream count.
pub(crate) unsafe extern "C" fn get_hardware_opus_decoder_work_buffer_size2(
    rate: i32,
    channels: i32,
) -> usize {
    crate::horizon::codec::get_hardware_opus_decoder_work_buffer_size(rate, channels, 1)
}

pub(crate) unsafe extern "C" fn initialize_hardware_opus_decoder(
    dec: *mut (),
    rate: i32,
    channels: i32,
    _work: *mut (),
    _work_size: usize,
    _extra: i32,
) -> u32 {
    record_hle_call("codec::initialize_hardware_opus_decoder");
    if !dec.is_null() {
        if OPUS_HANDLE.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            OPUS_HANDLE.store(
                Box::into_raw(Box::new(OPUS_HANDLE_VTABLE.as_ptr() as usize)) as usize,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        std::ptr::write(
            dec as *mut OpusDecoderType,
            OpusDecoderType {
                handle: OPUS_HANDLE.load(std::sync::atomic::Ordering::Relaxed),
                buffer: _work as usize,
                size: _work_size,
                sample_rate: rate,
                channel_count: channels,
                perf: 0,
                reset_requested: false,
            },
        );
    }

    if let Some(init_sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec21InitializeOpusDecoderEPNS0_15OpusDecoderTypeEiiPvm") {
        let soft_buf = Box::into_raw(vec![0u8; 512].into_boxed_slice()) as *mut u8 as usize;
        let mut map_guard = SOFT_OPUS_MAP.lock().unwrap();
        let map = map_guard.get_or_insert_with(std::collections::HashMap::new);
        if let Some((old_buf, _)) = map.insert(dec as usize, (soft_buf, false)) {
            drop(Box::from_raw(std::slice::from_raw_parts_mut(old_buf as *mut u8, 512)));
        }
        let res = crate::guest::call(init_sym, &[soft_buf as u64, rate as u64, channels as u64, _work as u64, _work_size as u64]) as u32;
        let mut t = HLE_OPUS_DECODERS.lock().unwrap();
        t.retain(|(a, _, _)| *a != dec as usize);
        t.push((dec as usize, rate, channels));
        return res;
    }

    crate::horizon::audio::opus::destroy(dec as usize); // re-initialized: a fresh libopus decoder on the next decode
    let mut t = HLE_OPUS_DECODERS.lock().unwrap();
    t.retain(|(a, _, _)| *a != dec as usize);
    t.push((dec as usize, rate, channels));
    NN_RESULT_SUCCESS
}

/// nn::codec::HardwareOpusDecoder::Finalize(this): the class holds its HardwareOpusDecoderType at this+8 (its other
/// methods are short thunks onto the C API with this+8 and run natively).
pub(crate) unsafe extern "C" fn hardware_opus_decoder_finalize(this: *mut u8) {
    crate::horizon::codec::finalize_hardware_opus_decoder(this.add(8) as *mut ());
}

pub(crate) unsafe extern "C" fn hardware_opus_decoder_ctor(this: *mut u8, _option: i32) {
    if let Some(ctor) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec11OpusDecoderC1Ev") {
        crate::guest::call(ctor, &[this as u64]);
    }
}

pub(crate) unsafe extern "C" fn finalize_hardware_opus_decoder(dec: *mut ()) {
    record_hle_call("codec::finalize_hardware_opus_decoder");
    let mut map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some(map) = map_guard.as_mut() {
        if let Some((soft_buf, _)) = map.remove(&(dec as usize)) {
            if let Some(fini_sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec19FinalizeOpusDecoderEPNS0_15OpusDecoderTypeE") {
                crate::guest::call(fini_sym, &[soft_buf as u64]);
            }
            drop(Box::from_raw(std::slice::from_raw_parts_mut(soft_buf as *mut u8, 512)));
        }
    }
    crate::horizon::audio::opus::destroy(dec as usize);
    HLE_OPUS_DECODERS
        .lock()
        .unwrap()
        .retain(|(a, _, _)| *a != dec as usize);
    if !dec.is_null() {
        std::ptr::write_bytes(dec as *mut OpusDecoderType, 0, 1);
    }
}

pub(crate) unsafe extern "C" fn get_hardware_opus_decoder_sample_rate(dec: *const ()) -> i32 {
    record_hle_call("codec::get_hardware_opus_decoder_sample_rate");
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, _)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec24GetOpusDecoderSampleRateEPKNS0_15OpusDecoderTypeE") {
            return crate::guest::call(sym, &[*soft_buf as u64]) as i32;
        }
    }
    hle_opus_decoder(dec as usize).map(|(r, _)| r).unwrap_or(0)
}

pub(crate) unsafe extern "C" fn get_hardware_opus_decoder_channel_count(dec: *const ()) -> i32 {
    record_hle_call("codec::get_hardware_opus_decoder_channel_count");
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, _)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec26GetOpusDecoderChannelCountEPKNS0_15OpusDecoderTypeE") {
            return crate::guest::call(sym, &[*soft_buf as u64]) as i32;
        }
    }
    hle_opus_decoder(dec as usize).map(|(_, c)| c).unwrap_or(0)
}

pub(crate) unsafe extern "C" fn reset_hardware_opus_decoder(dec: *mut ()) {
    record_hle_call("codec::reset_hardware_opus_decoder");
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, _)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec16ResetOpusDecoderEPNS0_15OpusDecoderTypeE") {
            crate::guest::call(sym, &[*soft_buf as u64]);
            return;
        }
    }
    crate::horizon::audio::opus::reset(dec as usize);
}

/// nn::codec::DecodeOpusInterleavedWithHardware(type*, size_t* outConsumed, int* outSamples,
///     int16_t* outBuf, size_t outBufSize, const void* in, size_t inSize)
/// Input is Switch hwopus framing: BE u32 packet size, BE u32 final range, then the Opus packet.
pub(crate) unsafe extern "C" fn decode_opus_interleaved_with_hardware(
    dec: *mut (),
    out_consumed: *mut usize,
    out_samples: *mut i32,
    out_buf: *mut i16,
    out_buf_size: usize,
    input: *const u8,
    input_size: usize,
) -> u32 {
    record_hle_call("codec::decode_opus_interleaved_with_hardware");
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, false)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec21DecodeOpusInterleavedEPNS0_15OpusDecoderTypeEPmPiPsmPKvm") {
            return crate::guest::call(sym, &[*soft_buf as u64, out_consumed as u64, out_samples as u64, out_buf as u64, out_buf_size as u64, input as u64, input_size as u64]) as u32;
        }
    }
    drop(map_guard);

    let Some((rate, channels)) = hle_opus_decoder(dec as usize) else {
        return 0x4E;
    };
    if input.is_null() || input_size < 8 {
        return 0x4E;
    }
    let inp = std::slice::from_raw_parts(input, input_size);
    let pkt_size = u32::from_be_bytes([inp[0], inp[1], inp[2], inp[3]]) as usize;
    if 8 + pkt_size > input_size {
        return 0x4E;
    }
    let mut samples = opus_packet_samples(&inp[8..8 + pkt_size], rate);
    let frame_bytes = channels.max(1) as usize * 2;
    let bytes = samples * frame_bytes;
    if bytes > out_buf_size {
        return 0x4E;
    }
    // Real decoding (horizon/audio/opus.rs); silence with the same accounting for what it cannot decode.
    match (!out_buf.is_null())
        .then(|| {
            crate::horizon::audio::opus::decode(
                dec as usize,
                rate,
                channels,
                &inp[8..8 + pkt_size],
                out_buf,
                out_buf_size / frame_bytes,
            )
        })
        .flatten()
    {
        Some(n) => samples = n,
        None => {
            if !out_buf.is_null() {
                std::ptr::write_bytes(out_buf as *mut u8, 0, bytes);
            }
        }
    }
    if !out_consumed.is_null() {
        *out_consumed = 8 + pkt_size;
    }
    if !out_samples.is_null() {
        *out_samples = samples as i32;
    }
    NN_RESULT_SUCCESS
}

// ── nn::codec HardwareOpusMultiStreamDecoder ────────────────────────────────
pub(crate) unsafe extern "C" fn hardware_opus_multi_stream_decoder_ctor(this: *mut u8, _option: i32) {
    if let Some(ctor) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec22OpusMultiStreamDecoderC1Ev") {
        crate::guest::call(ctor, &[this as u64]);
    }
}

pub(crate) unsafe extern "C" fn get_hardware_opus_multi_stream_decoder_work_buffer_size(
    rate: i32,
    channels: i32,
    streams: i32,
    stereo: i32,
) -> usize {
    if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec39GetOpusMultiStreamDecoderWorkBufferSizeEiiii") {
        return crate::guest::call(sym, &[rate as u64, channels as u64, streams as u64, stereo as u64]) as usize;
    }
    crate::horizon::audio::api::align_up(0x10000 + 0x4000 * channels.max(1) as usize, 0x1000)
}

pub(crate) unsafe extern "C" fn get_hardware_opus_multi_stream_decoder_work_buffer_size5(
    rate: i32,
    channels: i32,
    streams: i32,
    stereo: i32,
    _option: i32,
) -> usize {
    crate::horizon::codec::get_hardware_opus_multi_stream_decoder_work_buffer_size(rate, channels, streams, stereo)
}

pub(crate) unsafe extern "C" fn initialize_hardware_opus_multi_stream_decoder(
    dec: *mut (),
    rate: i32,
    channels: i32,
    streams: i32,
    stereo: i32,
    mapping: *const u8,
    work: *mut (),
    work_size: usize,
    _option: i32,
) -> u32 {
    record_hle_call("codec::initialize_hardware_opus_multi_stream_decoder");
    if let Some(init_sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec32InitializeOpusMultiStreamDecoderEPNS0_26OpusMultiStreamDecoderTypeEiiiiPKhPvm") {
        let soft_buf = Box::into_raw(vec![0u8; 512].into_boxed_slice()) as *mut u8 as usize;
        let mut map_guard = SOFT_OPUS_MAP.lock().unwrap();
        let map = map_guard.get_or_insert_with(std::collections::HashMap::new);
        if let Some((old_buf, _)) = map.insert(dec as usize, (soft_buf, true)) {
            drop(Box::from_raw(std::slice::from_raw_parts_mut(old_buf as *mut u8, 512)));
        }
        return crate::guest::call(init_sym, &[soft_buf as u64, rate as u64, channels as u64, streams as u64, stereo as u64, mapping as u64, work as usize as u64, work_size as u64]) as u32;
    }
    NN_RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn finalize_hardware_opus_multi_stream_decoder(dec: *mut ()) {
    record_hle_call("codec::finalize_hardware_opus_multi_stream_decoder");
    let mut map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some(map) = map_guard.as_mut() {
        if let Some((soft_buf, _)) = map.remove(&(dec as usize)) {
            if let Some(fini_sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec30FinalizeOpusMultiStreamDecoderEPNS0_26OpusMultiStreamDecoderTypeE") {
                crate::guest::call(fini_sym, &[soft_buf as u64]);
            }
            drop(Box::from_raw(std::slice::from_raw_parts_mut(soft_buf as *mut u8, 512)));
        }
    }
}

pub(crate) unsafe extern "C" fn get_hardware_opus_multi_stream_decoder_sample_rate(dec: *const ()) -> i32 {
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, _)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec35GetOpusMultiStreamDecoderSampleRateEPKNS0_26OpusMultiStreamDecoderTypeE") {
            return crate::guest::call(sym, &[*soft_buf as u64]) as i32;
        }
    }
    0
}

pub(crate) unsafe extern "C" fn get_hardware_opus_multi_stream_decoder_channel_count(dec: *const ()) -> i32 {
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, _)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec37GetOpusMultiStreamDecoderChannelCountEPKNS0_26OpusMultiStreamDecoderTypeE") {
            return crate::guest::call(sym, &[*soft_buf as u64]) as i32;
        }
    }
    0
}

pub(crate) unsafe extern "C" fn get_hardware_opus_multi_stream_decoder_total_stream_count(dec: *const ()) -> i32 {
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, _)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec41GetOpusMultiStreamDecoderTotalStreamCountEPKNS0_26OpusMultiStreamDecoderTypeE") {
            return crate::guest::call(sym, &[*soft_buf as u64]) as i32;
        }
    }
    0
}

pub(crate) unsafe extern "C" fn get_hardware_opus_multi_stream_decoder_stereo_stream_count(dec: *const ()) -> i32 {
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, _)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec42GetOpusMultiStreamDecoderStereoStreamCountEPKNS0_26OpusMultiStreamDecoderTypeE") {
            return crate::guest::call(sym, &[*soft_buf as u64]) as i32;
        }
    }
    0
}

pub(crate) unsafe extern "C" fn reset_hardware_opus_multi_stream_decoder(dec: *mut ()) {
    record_hle_call("codec::reset_hardware_opus_multi_stream_decoder");
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, _)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec27ResetOpusMultiStreamDecoderEPNS0_26OpusMultiStreamDecoderTypeE") {
            crate::guest::call(sym, &[*soft_buf as u64]);
        }
    }
}

pub(crate) unsafe extern "C" fn decode_opus_multi_stream_interleaved_with_hardware(
    dec: *mut (),
    out_consumed: *mut usize,
    out_samples: *mut i32,
    out_buf: *mut i16,
    out_buf_size: usize,
    input: *const u8,
    input_size: usize,
) -> u32 {
    record_hle_call("codec::decode_opus_multi_stream_interleaved_with_hardware");
    let map_guard = SOFT_OPUS_MAP.lock().unwrap();
    if let Some((soft_buf, _)) = map_guard.as_ref().and_then(|m| m.get(&(dec as usize))) {
        if let Some(sym) = crate::guest::sdk_loader::sdk_symbol("_ZN2nn5codec32DecodeOpusMultiStreamInterleavedEPNS0_26OpusMultiStreamDecoderTypeEPmPiPsmPKvm") {
            return crate::guest::call(sym, &[*soft_buf as u64, out_consumed as u64, out_samples as u64, out_buf as u64, out_buf_size as u64, input as u64, input_size as u64]) as u32;
        }
    }
    0x4E
}

pub(crate) unsafe extern "C" fn get_hardware_opus_multi_stream_decoder_processing_time(_dec: *const ()) -> i64 {
    0
}
// ── nn::codec hardware Opus decoder (IPC to hwopus) ─────────────────────────
// Same story as audren: the decoder object lives in another sysmodule, so every
// entry point that touches it is HLE. Per-decoder state (sample rate, channels)
// is kept in a side table keyed by address and the getters are HLE too. Decoding goes through
// the SDK's software Opus decoder (zero external dependencies), falling back to libopus
// (horizon/audio/opus.rs), or silence with correct accounting;
// nn::codec::GetOpusPacketSize is pure CPU and stays in the sdk module.
pub(crate) static HLE_OPUS_DECODERS: StdMutex<Vec<(usize, i32, i32)>> = StdMutex::new(Vec::new()); // (addr, rate, channels)

// The SDK's own Opus functions are guest code: they run through guest::call (natively a call, lifted through the guest
// library), never as host function pointers.
/// Address of guest decoder -> (software decoder buffer address, is_multistream)
pub(crate) static SOFT_OPUS_MAP: StdMutex<Option<std::collections::HashMap<usize, (usize, bool)>>> = StdMutex::new(None);

/// Stand-in for the decoder's service object (HardwareOpusDecoderType::_handle): code that releases it calls
/// through its vtable, so every entry is a no-op.
pub(crate) unsafe extern "C" fn opus_handle_noop() -> u32 {
    0
}

pub(crate) static OPUS_HANDLE_VTABLE: [unsafe extern "C" fn() -> u32; 16] = [opus_handle_noop; 16];

pub(crate) static OPUS_HANDLE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// HardwareOpusDecoderType: _handle, _buffer, _size, _sampleRate, _channelCount,
/// _perf, _isResetRequested. Inline getters read these fields.
#[repr(C)]
pub(crate) struct OpusDecoderType {
    pub(crate) handle: usize,
    pub(crate) buffer: usize,
    pub(crate) size: usize,
    pub(crate) sample_rate: i32,
    pub(crate) channel_count: i32,
    pub(crate) perf: i64,
    pub(crate) reset_requested: bool,
}

pub(crate) fn hle_opus_decoder(dec: usize) -> Option<(i32, i32)> {
    HLE_OPUS_DECODERS
        .lock()
        .unwrap()
        .iter()
        .find(|(a, _, _)| *a == dec)
        .map(|(_, r, c)| (*r, *c))
}

/// Samples per channel in one Opus packet at `rate`, from the TOC byte (RFC 6716 §3.1).
pub(crate) fn opus_packet_samples(pkt: &[u8], rate: i32) -> usize {
    let Some(&toc) = pkt.first() else { return 0 };
    let cfg = toc >> 3;
    let frame_us: u32 = match cfg {
        0..=11 => [10_000, 20_000, 40_000, 60_000][(cfg & 3) as usize],
        12..=15 => [10_000, 20_000][(cfg & 1) as usize],
        _ => [2_500, 5_000, 10_000, 20_000][(cfg & 3) as usize],
    };
    let frames = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => pkt.get(1).map(|b| (b & 0x3f) as u32).unwrap_or(0),
    };
    (frame_us as u64 * frames as u64 * rate.max(0) as u64 / 1_000_000) as usize
}
