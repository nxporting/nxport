// MP4 through the mp4 crate: tracks' samples interleaved by decode time, seeks to the last sync sample before a time.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use mp4::{MediaType, Mp4Reader, TrackType};

use crate::{Codec, Demux, Kind, Sample, Track};

struct TrackState {
    id: u32,
    index: usize,
    timescale: u64,
    /// Decode time of each sample (timescale units), sample i at [i - 1].
    dts: Vec<u64>,
    /// Sync samples (1-based ids); None: every sample is one.
    sync: Option<Vec<u32>>,
    next: u32,
}

impl TrackState {
    fn us(&self, t: u64) -> i64 {
        (t as u128 * 1_000_000 / self.timescale.max(1) as u128) as i64
    }
}

struct Mp4 {
    reader: Mp4Reader<BufReader<File>>,
    tracks: Vec<TrackState>,
}

pub(crate) fn open(path: &Path) -> Result<(Vec<Track>, Box<dyn Demux>), String> {
    let err = |e: mp4::Error| format!("{}: {e}", path.display());
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let size = f.metadata().map_err(|e| format!("{}: {e}", path.display()))?.len();
    let reader = Mp4Reader::read_header(BufReader::new(f), size).map_err(err)?;
    let mut ids: Vec<u32> = reader.tracks().keys().copied().collect();
    ids.sort_unstable();
    let mut tracks = Vec::new();
    let mut states = Vec::new();
    for id in ids {
        let t = &reader.tracks()[&id];
        let kind = match t.track_type() {
            Ok(TrackType::Video) => Kind::Video,
            Ok(TrackType::Audio) => Kind::Audio,
            _ => Kind::Other,
        };
        let codec = match t.media_type() {
            Ok(MediaType::H264) => Codec::Avc,
            Ok(MediaType::VP9) => Codec::Vp9,
            Ok(MediaType::AAC) => Codec::Aac,
            _ => Codec::Unknown,
        };
        let stbl = &t.trak.mdia.minf.stbl;
        let mut track = Track {
            kind,
            codec,
            width: 0,
            height: 0,
            fps: 0.0,
            sample_rate: 0,
            channels: 0,
            duration_us: t.duration().as_micros() as i64,
            private: Vec::new(),
        };
        if kind == Kind::Video {
            track.width = t.width() as u32;
            track.height = t.height() as u32;
            track.fps = t.frame_rate();
        }
        if let Some(avc1) = &stbl.stsd.avc1 {
            let c = &avc1.avcc;
            let mut p = vec![1, c.avc_profile_indication, c.profile_compatibility, c.avc_level_indication];
            p.push(0xfc | c.length_size_minus_one);
            p.push(0xe0 | c.sequence_parameter_sets.len() as u8);
            for s in &c.sequence_parameter_sets {
                p.extend_from_slice(&(s.bytes.len() as u16).to_be_bytes());
                p.extend_from_slice(&s.bytes);
            }
            p.push(c.picture_parameter_sets.len() as u8);
            for s in &c.picture_parameter_sets {
                p.extend_from_slice(&(s.bytes.len() as u16).to_be_bytes());
                p.extend_from_slice(&s.bytes);
            }
            track.private = p;
        }
        if codec == Codec::Aac {
            let profile = t.audio_profile().map_or(2, |p| p as u8);
            let (freq, chans) = (t.sample_freq_index().ok(), t.channel_config().ok());
            track.sample_rate = freq.map_or(0, |f| f.freq());
            track.channels = chans.map_or(0, |c| c as u32);
            // AudioSpecificConfig: object type (5 bits), frequency index (4), channel configuration (4).
            let asc = (profile as u16) << 11 | (freq.map_or(4, |f| f as u16) & 15) << 7 | (track.channels as u16 & 15) << 3;
            track.private = asc.to_be_bytes().to_vec();
        }
        let mut dts = Vec::with_capacity(t.sample_count() as usize);
        let mut time = 0u64;
        for e in &stbl.stts.entries {
            for _ in 0..e.sample_count {
                dts.push(time);
                time += e.sample_delta as u64;
            }
        }
        states.push(TrackState {
            id,
            index: tracks.len(),
            timescale: t.timescale() as u64,
            dts,
            sync: stbl.stss.as_ref().map(|s| s.entries.clone()),
            next: 1,
        });
        tracks.push(track);
    }
    Ok((tracks, Box::new(Mp4 { reader, tracks: states })))
}

impl Demux for Mp4 {
    fn next(&mut self) -> Option<Sample> {
        loop {
            let t = self
                .tracks
                .iter()
                .filter(|t| (t.next as usize) <= t.dts.len())
                .min_by_key(|t| t.us(t.dts[t.next as usize - 1]))?;
            let (id, index, n) = (t.id, t.index, t.next);
            let i = self.tracks.iter().position(|t| t.id == id).unwrap();
            self.tracks[i].next += 1;
            let Ok(Some(s)) = self.reader.read_sample(id, n) else {
                continue;
            };
            let pts = (s.start_time as i64 + s.rendering_offset as i64).max(0) as u64;
            return Some(Sample { track: index, pts_us: self.tracks[i].us(pts), key: s.is_sync, data: s.bytes.to_vec() });
        }
    }

    fn seek(&mut self, us: i64) -> bool {
        for t in &mut self.tracks {
            // The last sample starting at or before the time, then back to the sync sample before it.
            let mut n = t.dts.iter().take_while(|&&d| t.us(d) <= us).count().max(1) as u32;
            if let Some(sync) = &t.sync {
                n = sync.iter().copied().filter(|&s| s <= n).last().unwrap_or(1);
            }
            t.next = n;
        }
        true
    }
}
