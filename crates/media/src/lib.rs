// Movie files for the engine's movie:: HLE (crates/engine/src/horizon/movie.rs): a container (WebM / Matroska, MP4) split
// into samples, and decoders that turn samples into pictures (NV12 or RGBA) and 16-bit PCM. Everything is built into
// the program: VP9, Opus, AAC and Vorbis in Rust, H.264 through OpenH264 (C++ compiled with this crate). Nothing is
// installed or loaded at run time. VP8 has no decoder yet: its tracks are listed, and opening a decoder for one fails.

mod audio;
mod mkv;
mod mp4file;
mod video;

use std::collections::VecDeque;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Video,
    Audio,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec {
    Avc,
    Vp8,
    Vp9,
    Aac,
    Vorbis,
    Opus,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub kind: Kind,
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub sample_rate: u32,
    pub channels: u32,
    pub duration_us: i64,
    /// What the decoder needs besides the samples: Matroska's CodecPrivate; from MP4, the avcC record (AVC) or the
    /// AudioSpecificConfig (AAC).
    pub private: Vec<u8>,
}

/// One compressed sample of a track, as stored: a frame (or VP9 superframe), or a packet of audio.
pub struct Sample {
    pub track: usize,
    pub pts_us: i64,
    pub key: bool,
    pub data: Vec<u8>,
}

trait Demux: Send {
    fn next(&mut self) -> Option<Sample>;
    /// Positions the next sample at the last keyframe at or before `us`.
    fn seek(&mut self, us: i64) -> bool;
}

/// A movie file: its tracks and its samples in file order, read one ahead.
pub struct Source {
    tracks: Vec<Track>,
    demux: Box<dyn Demux>,
    current: Option<Sample>,
    eof: bool,
}

impl Source {
    pub fn open(path: &Path) -> Result<Source, String> {
        let mut magic = [0u8; 8];
        {
            use std::io::Read;
            let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            f.read_exact(&mut magic).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let (tracks, demux): (Vec<Track>, Box<dyn Demux>) = if magic[..4] == [0x1a, 0x45, 0xdf, 0xa3] {
            mkv::open(path)?
        } else if &magic[4..8] == b"ftyp" {
            mp4file::open(path)?
        } else {
            return Err(format!("{}: not a WebM, Matroska or MP4 file", path.display()));
        };
        Ok(Source { tracks, demux, current: None, eof: false })
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// The current sample (the next one in the file once the last was advanced past), or None at the end.
    pub fn peek(&mut self) -> Option<&Sample> {
        if self.current.is_none() && !self.eof {
            self.current = self.demux.next();
            self.eof = self.current.is_none();
        }
        self.current.as_ref()
    }

    pub fn advance(&mut self) {
        self.current = None;
    }

    pub fn seek(&mut self, us: i64) -> bool {
        self.current = None;
        self.eof = false;
        self.demux.seek(us)
    }
}

/// What a decoder hands out (movie::DecoderOutputFormat): 16-bit PCM, RGBA bytes, or NV12.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Pcm16,
    Rgba,
    Nv12,
}

/// The result of Decoder::receive.
#[derive(Debug)]
pub enum Output {
    /// A picture was written: `stride` bytes per row (RGBA) or per luma row (NV12).
    Picture { pts_us: i64, width: u32, height: u32, stride: u32, size: usize },
    /// Interleaved 16-bit samples were written.
    Audio { pts_us: i64, frames: u32, channels: u32, size: usize },
    /// Nothing decoded yet: send more samples.
    NeedInput,
    /// Everything was handed out after the end of the stream was sent.
    End,
    /// The next output needs this many bytes.
    TooSmall(usize),
}

enum Decoded {
    /// 8-bit 4:2:0 planes, chroma planes ((width + 1) / 2) x ((height + 1) / 2).
    Picture { pts_us: i64, width: u32, height: u32, y: Vec<u8>, u: Vec<u8>, v: Vec<u8> },
    Pcm { pts_us: i64, channels: u32, samples: Vec<i16> },
}

trait Decode: Send {
    fn decode(&mut self, data: &[u8], pts_us: i64, out: &mut VecDeque<Decoded>) -> Result<(), String>;
    /// End of stream: hands out what is still held back.
    fn drain(&mut self, out: &mut VecDeque<Decoded>);
    /// After a seek: drops references and anything held back.
    fn reset(&mut self);
}

pub struct Decoder {
    codec: Box<dyn Decode>,
    format: Format,
    ready: VecDeque<Decoded>,
    ended: bool,
}

impl Decoder {
    pub fn new(track: &Track, format: Format) -> Result<Decoder, String> {
        let codec: Box<dyn Decode> = match track.codec {
            Codec::Vp9 => Box::new(video::Vp9::new()),
            Codec::Avc => Box::new(video::Avc::new(&track.private)?),
            Codec::Aac | Codec::Vorbis => Box::new(audio::Symphonia::new(track)?),
            Codec::Opus => Box::new(audio::Opus::new(track)?),
            c => return Err(format!("no decoder for {c:?}")),
        };
        Ok(Decoder { codec, format, ready: VecDeque::new(), ended: false })
    }

    /// Decodes one sample (if `data` is not empty); `eos` marks the end of the stream, after which receive hands out
    /// what the decoder held back and then End.
    pub fn send(&mut self, data: &[u8], pts_us: i64, eos: bool) -> Result<(), String> {
        if !data.is_empty() {
            self.codec.decode(data, pts_us, &mut self.ready)?;
        }
        if eos && !self.ended {
            self.codec.drain(&mut self.ready);
            self.ended = true;
        }
        Ok(())
    }

    pub fn receive(&mut self, out: &mut [u8]) -> Output {
        let Some(d) = self.ready.front() else {
            return if self.ended { Output::End } else { Output::NeedInput };
        };
        let r = match d {
            Decoded::Picture { pts_us, width, height, y, u, v } => {
                let (w, h) = (*width as usize, *height as usize);
                let (cw, ch) = ((w + 1) / 2, (h + 1) / 2);
                match self.format {
                    Format::Rgba => {
                        let size = w * h * 4;
                        if out.len() < size {
                            return Output::TooSmall(size);
                        }
                        video::yuv420_to_rgba(&mut out[..size], w, h, y, u, v);
                        Output::Picture { pts_us: *pts_us, width: *width, height: *height, stride: (w * 4) as u32, size }
                    }
                    _ => {
                        let size = w * h + 2 * cw * ch;
                        if out.len() < size {
                            return Output::TooSmall(size);
                        }
                        out[..w * h].copy_from_slice(y);
                        for (i, uv) in out[w * h..size].chunks_exact_mut(2).enumerate() {
                            uv[0] = u[i];
                            uv[1] = v[i];
                        }
                        Output::Picture { pts_us: *pts_us, width: *width, height: *height, stride: w as u32, size }
                    }
                }
            }
            Decoded::Pcm { pts_us, channels, samples } => {
                let size = samples.len() * 2;
                if out.len() < size {
                    return Output::TooSmall(size);
                }
                for (o, s) in out.chunks_exact_mut(2).zip(samples) {
                    o.copy_from_slice(&s.to_le_bytes());
                }
                let frames = (samples.len() / (*channels).max(1) as usize) as u32;
                Output::Audio { pts_us: *pts_us, frames, channels: *channels, size }
            }
        };
        self.ready.pop_front();
        r
    }

    pub fn flush(&mut self) {
        self.codec.reset();
        self.ready.clear();
        self.ended = false;
    }
}
