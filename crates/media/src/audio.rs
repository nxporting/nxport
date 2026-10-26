// Audio: AAC and Vorbis through symphonia, Opus through opus-decoder. Output is interleaved 16-bit PCM at the stream's
// own rate and channel count (Opus: 48 kHz).

use std::collections::VecDeque;

use symphonia::core::audio::{Channels, SampleBuffer};
use symphonia::core::codecs::{CodecParameters, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_VORBIS};
use symphonia::core::errors::Error;
use symphonia::core::formats::Packet;

use crate::{Codec, Decode, Decoded, Track};

const AAC_RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

fn channel_mask(n: u32) -> Channels {
    let all = [
        Channels::FRONT_LEFT,
        Channels::FRONT_RIGHT,
        Channels::FRONT_CENTRE,
        Channels::LFE1,
        Channels::REAR_LEFT,
        Channels::REAR_RIGHT,
        Channels::SIDE_LEFT,
        Channels::SIDE_RIGHT,
    ];
    if n == 1 {
        return Channels::FRONT_CENTRE;
    }
    all.iter().take(n.clamp(1, 8) as usize).fold(Channels::empty(), |a, &c| a | c)
}

/// Matroska's Vorbis CodecPrivate is the three header packets in Xiph lacing; symphonia wants the identification header
/// followed by the setup header.
fn vorbis_headers(private: &[u8]) -> Result<Vec<u8>, String> {
    let bad = || "vorbis: bad CodecPrivate".to_string();
    if private.first() != Some(&2) {
        return Err(bad());
    }
    let mut o = 1;
    let mut sizes = [0usize; 2];
    for s in &mut sizes {
        loop {
            let b = *private.get(o).ok_or_else(bad)?;
            o += 1;
            *s += b as usize;
            if b < 255 {
                break;
            }
        }
    }
    let ident = private.get(o..o + sizes[0]).ok_or_else(bad)?;
    let setup = private.get(o + sizes[0] + sizes[1]..).ok_or_else(bad)?;
    Ok([ident, setup].concat())
}

pub(crate) struct Symphonia {
    dec: Box<dyn symphonia::core::codecs::Decoder>,
}

impl Symphonia {
    pub(crate) fn new(track: &Track) -> Result<Symphonia, String> {
        let mut params = CodecParameters::new();
        match track.codec {
            Codec::Vorbis => {
                params.for_codec(CODEC_TYPE_VORBIS).with_extra_data(vorbis_headers(&track.private)?.into_boxed_slice());
            }
            _ => {
                let asc = if track.private.len() >= 2 {
                    track.private.clone()
                } else {
                    // No AudioSpecificConfig: AAC-LC at the track's rate and channel count.
                    let f = AAC_RATES.iter().position(|&r| r == track.sample_rate).unwrap_or(4) as u16;
                    (2u16 << 11 | f << 7 | (track.channels as u16 & 15) << 3).to_be_bytes().to_vec()
                };
                params.for_codec(CODEC_TYPE_AAC).with_extra_data(asc.into_boxed_slice());
            }
        }
        if track.sample_rate > 0 {
            params.with_sample_rate(track.sample_rate);
        }
        if track.channels > 0 {
            params.with_channels(channel_mask(track.channels));
        }
        let dec = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| format!("{:?}: {e}", track.codec))?;
        Ok(Symphonia { dec })
    }
}

impl Decode for Symphonia {
    fn decode(&mut self, data: &[u8], pts_us: i64, out: &mut VecDeque<Decoded>) -> Result<(), String> {
        match self.dec.decode(&Packet::new_from_slice(0, 0, 0, data)) {
            Ok(buf) => {
                let spec = *buf.spec();
                let mut s = SampleBuffer::<i16>::new(buf.capacity() as u64, spec);
                s.copy_interleaved_ref(buf);
                if !s.samples().is_empty() {
                    out.push_back(Decoded::Pcm { pts_us, channels: spec.channels.count() as u32, samples: s.samples().to_vec() });
                }
                Ok(())
            }
            // A damaged packet is skipped, as ffmpeg's decoders do.
            Err(Error::DecodeError(_)) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn drain(&mut self, _out: &mut VecDeque<Decoded>) {}

    fn reset(&mut self) {
        self.dec.reset();
    }
}

pub(crate) struct Opus {
    dec: opus_decoder::OpusDecoder,
    channels: usize,
    /// OpusHead's pre-skip: samples (at 48 kHz) at the start that are not part of the stream.
    pre_skip: usize,
    skip: usize,
    pcm: Vec<i16>,
}

impl Opus {
    pub(crate) fn new(track: &Track) -> Result<Opus, String> {
        let head = &track.private;
        let channels = if head.len() >= 19 && &head[..8] == b"OpusHead" { head[9] as usize } else { track.channels as usize };
        if !(1..=2).contains(&channels) {
            return Err(format!("opus: {channels} channels"));
        }
        let pre_skip = if head.len() >= 12 { u16::from_le_bytes([head[10], head[11]]) as usize } else { 0 };
        let dec = opus_decoder::OpusDecoder::new(48_000, channels).map_err(|e| format!("opus: {e:?}"))?;
        Ok(Opus { dec, channels, pre_skip, skip: pre_skip, pcm: vec![0; 5760 * channels] })
    }
}

impl Decode for Opus {
    fn decode(&mut self, data: &[u8], pts_us: i64, out: &mut VecDeque<Decoded>) -> Result<(), String> {
        let n = match self.dec.decode(data, &mut self.pcm, false) {
            Ok(n) => n,
            Err(_) => return Ok(()),
        };
        let skip = self.skip.min(n);
        self.skip -= skip;
        if n > skip {
            let samples = self.pcm[skip * self.channels..n * self.channels].to_vec();
            out.push_back(Decoded::Pcm { pts_us, channels: self.channels as u32, samples });
        }
        Ok(())
    }

    fn drain(&mut self, _out: &mut VecDeque<Decoded>) {}

    fn reset(&mut self) {
        if let Ok(d) = opus_decoder::OpusDecoder::new(48_000, self.channels) {
            self.dec = d;
        }
        self.skip = self.pre_skip;
    }
}
