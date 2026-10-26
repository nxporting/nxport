// WebM / Matroska through matroska-demuxer. Timestamps are in the segment's timestamp scale (nanoseconds per unit).

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use matroska_demuxer::{Frame, MatroskaFile, TrackType};

use crate::{Codec, Demux, Kind, Sample, Track};

struct Mkv {
    path: PathBuf,
    file: MatroskaFile<BufReader<File>>,
    /// Matroska track number -> index in the track list.
    index: Vec<(u64, usize)>,
    scale_ns: u64,
    /// The track whose keyframes seeks land on (the first video track), and its keyframe times, read on the first seek.
    video: Option<u64>,
    keyframes: Option<Vec<u64>>,
    frame: Frame,
}

fn open_file(path: &Path) -> Result<MatroskaFile<BufReader<File>>, String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    MatroskaFile::open(BufReader::new(f)).map_err(|e| format!("{}: {e:?}", path.display()))
}

pub(crate) fn open(path: &Path) -> Result<(Vec<Track>, Box<dyn Demux>), String> {
    let file = open_file(path)?;
    let scale_ns = file.info().timestamp_scale().get();
    let duration_us = file.info().duration().map_or(0, |d| (d * scale_ns as f64 / 1000.0) as i64);
    let mut tracks = Vec::new();
    let mut index = Vec::new();
    let mut video = None;
    for t in file.tracks() {
        let id = t.codec_id();
        let codec = match id {
            "V_VP9" => Codec::Vp9,
            "V_VP8" => Codec::Vp8,
            "V_MPEG4/ISO/AVC" => Codec::Avc,
            "A_VORBIS" => Codec::Vorbis,
            "A_OPUS" => Codec::Opus,
            _ if id.starts_with("A_AAC") => Codec::Aac,
            _ => Codec::Unknown,
        };
        let kind = match t.track_type() {
            TrackType::Video => Kind::Video,
            TrackType::Audio => Kind::Audio,
            _ => Kind::Other,
        };
        let mut track = Track {
            kind,
            codec,
            width: 0,
            height: 0,
            fps: 0.0,
            sample_rate: 0,
            channels: 0,
            duration_us,
            private: t.codec_private().map(<[u8]>::to_vec).unwrap_or_default(),
        };
        if let Some(v) = t.video() {
            track.width = v.pixel_width().get() as u32;
            track.height = v.pixel_height().get() as u32;
            track.fps = t.default_duration().map_or(0.0, |d| 1e9 / d.get() as f64);
            if video.is_none() {
                video = Some(t.track_number().get());
            }
        }
        if let Some(a) = t.audio() {
            track.sample_rate = a.sampling_frequency() as u32;
            track.channels = a.channels().get() as u32;
        }
        index.push((t.track_number().get(), tracks.len()));
        tracks.push(track);
    }
    let demux = Mkv { path: path.to_path_buf(), file, index, scale_ns, video, keyframes: None, frame: Frame::default() };
    Ok((tracks, Box::new(demux)))
}

impl Mkv {
    fn keyframes(&mut self) -> &[u64] {
        if self.keyframes.is_none() {
            let mut k = Vec::new();
            if let (Some(v), Ok(mut f)) = (self.video, open_file(&self.path)) {
                let mut frame = Frame::default();
                while let Ok(true) = f.next_frame(&mut frame) {
                    if frame.track == v && frame.is_keyframe != Some(false) {
                        k.push(frame.timestamp);
                    }
                }
            }
            self.keyframes = Some(k);
        }
        self.keyframes.as_deref().unwrap()
    }
}

impl Demux for Mkv {
    fn next(&mut self) -> Option<Sample> {
        loop {
            if !matches!(self.file.next_frame(&mut self.frame), Ok(true)) {
                return None;
            }
            let Some(&(_, track)) = self.index.iter().find(|(n, _)| *n == self.frame.track) else {
                continue;
            };
            return Some(Sample {
                track,
                pts_us: (self.frame.timestamp * self.scale_ns / 1000) as i64,
                key: self.frame.is_keyframe.unwrap_or(true),
                data: std::mem::take(&mut self.frame.data),
            });
        }
    }

    fn seek(&mut self, us: i64) -> bool {
        let scale = self.scale_ns;
        let target = (us.max(0) as u64 * 1000) / scale;
        // The demuxer seeks to the first frame at or after a time: a keyframe's own time lands on that keyframe.
        let t = if self.video.is_some() {
            self.keyframes().iter().copied().filter(|&k| k <= target).last().unwrap_or(0)
        } else {
            target
        };
        self.file.seek(t).is_ok()
    }
}
