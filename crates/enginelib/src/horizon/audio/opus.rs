// Real Opus decoding for the HLE'd hardware Opus decoder (horizon/hle.rs, nn::codec::DecodeOpusInterleavedWithHardware): TotK
// streams its music and ambience as Opus and mixes them on the CPU, so a decoder that only accounted for samples made
// every stream silent.
//
// One decoder per guest HardwareOpusDecoderType (keyed by its address), created on first use with the guest's sample rate
// and channel count, reset by ResetHardwareOpusDecoder and destroyed by FinalizeHardwareOpusDecoder.

use std::collections::HashMap;
use std::sync::Mutex;

use opus_decoder::OpusDecoder;

/// Guest decoder address -> decoder.
static DECODERS: Mutex<Option<HashMap<usize, OpusDecoder>>> = Mutex::new(None);

/// Decodes one packet into interleaved 16-bit samples; returns samples per channel, or None (caller writes silence).
///
/// # Safety
/// `out` holds `out_frames_max` frames of `channels` samples.
pub unsafe fn decode(
    guest: usize,
    rate: i32,
    channels: i32,
    packet: &[u8],
    out: *mut i16,
    out_frames_max: usize,
) -> Option<usize> {
    let mut map = DECODERS.lock().unwrap();
    let map = map.get_or_insert_with(HashMap::new);
    if !map.contains_key(&guest) {
        match OpusDecoder::new(rate as u32, channels as usize) {
            Ok(d) => {
                map.insert(guest, d);
            }
            Err(e) => {
                println!("[!] opus: a decoder for {rate} Hz, {channels} channels: {e:?}");
                return None;
            }
        }
    }
    let dec = map.get_mut(&guest)?;
    let out = std::slice::from_raw_parts_mut(out, out_frames_max * channels as usize);
    match dec.decode(packet, out, false) {
        Ok(n) => Some(n),
        Err(e) => {
            static REPORTED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if REPORTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 5 {
                println!("[!] opus: decoding a {}-byte packet: {e:?}", packet.len());
            }
            None
        }
    }
}

pub fn reset(guest: usize) {
    if let Some(d) = DECODERS.lock().unwrap().as_mut().and_then(|m| m.get_mut(&guest)) {
        d.reset();
    }
}

pub fn destroy(guest: usize) {
    if let Some(m) = DECODERS.lock().unwrap().as_mut() {
        m.remove(&guest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_decodes_resets_and_is_destroyed() {
        // A 20 ms stereo CELT frame of silence: code 0, no payload beyond the TOC byte's two filler bytes.
        let packet = [0xFC, 0xFF, 0xFE];
        let mut pcm = vec![1i16; 960 * 2];
        let n = unsafe { decode(0x1000, 48000, 2, &packet, pcm.as_mut_ptr(), 960) };
        assert_eq!(n, Some(960));
        assert!(pcm.iter().all(|&s| s.unsigned_abs() < 8), "not silence");
        reset(0x1000);
        destroy(0x1000);
        reset(0x1000);
        destroy(0x1000);
        // A rate the format does not have: no decoder, the caller writes silence.
        assert_eq!(unsafe { decode(0x2000, 44100, 2, &packet, pcm.as_mut_ptr(), 960) }, None);
    }
}
