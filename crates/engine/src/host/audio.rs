// The host side of sound output: 48 kHz stereo float frames from the audio renderer (horizon/audio/renderer.rs) go into a ring buffer
// that the platform's output device plays: an AudioQueue (AudioToolbox, audio_queue.rs) on macOS and iOS, an SDL3 audio
// stream (audio_sdl.rs) where guest code is lifted. Playback starts with the first pushed frame, only in windowed
// runs (never with --headless) and not with --audio=0. --audio-wav=path also writes everything pushed to a
// WAV file (works headless: how agent runs check the output without making sound).
//
// The renderer produces frames on the guest's schedule (wall-clock 5 ms frames), the device consumes them on its own
// clock: the ring absorbs the difference and is trimmed back to TARGET_MS when it grows past MAX_MS, so latency
// stays bounded; when it runs dry the device plays silence.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

#[cfg(target_vendor = "apple")]
#[path = "audio_queue.rs"]
mod device;
#[cfg(not(target_vendor = "apple"))]
#[path = "audio_sdl.rs"]
mod device;

const RATE: u32 = 48000;
const TARGET_MS: usize = 60;
const MAX_MS: usize = 150;

struct Ring {
    samples: VecDeque<f32>, // interleaved L, R
    started: bool,
    trimmed: u64,
    underruns: u64,
}

static RING: Mutex<Ring> = Mutex::new(Ring {
    samples: VecDeque::new(),
    started: false,
    trimmed: 0,
    underruns: 0,
});
/// A second source mixed in at the device: nn::audio::AudioOut streams (horizon/audio/out.rs; Kirby's movie soundtracks).
static RING2: Mutex<VecDeque<f32>> = Mutex::new(VecDeque::new());

/// Prepares the output device on the process main thread, before any guest thread runs (SDL wants its subsystems
/// initialised there); the AudioQueue needs nothing ahead of the first frame.
pub fn init() {
    if playback_enabled() {
        device::init();
    }
}

/// Appends 16-bit PCM from an AudioOut buffer (interleaved, `channels` per frame, 48 kHz) to the second ring: mono is
/// duplicated, more than two channels keep the first two.
pub fn push_audio_out(pcm: &[i16], channels: usize) {
    if !playback_enabled() || channels == 0 {
        return;
    }
    let start = {
        let mut r = RING2.lock().unwrap();
        for f in pcm.chunks_exact(channels) {
            let l = f[0] as f32 / 32768.0;
            let rr = if channels > 1 {
                f[1] as f32 / 32768.0
            } else {
                l
            };
            r.push_back(l);
            r.push_back(rr);
        }
        let max = MAX_MS * 48 * 2;
        if r.len() > max {
            let drop = r.len() - TARGET_MS * 48 * 2;
            r.drain(..drop);
        }
        !std::mem::replace(&mut RING.lock().unwrap().started, true)
    };
    if start {
        device::start();
    }
}

struct Wav {
    file: std::fs::File,
    frames: u32,
}

static WAV: OnceLock<Option<Mutex<Wav>>> = OnceLock::new();

fn playback_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        !crate::settings::get().display.headless && crate::settings::get().audio.audio
    })
}

/// Appends interleaved stereo frames (float, ±1.0 full scale).
pub fn push(stereo: &[f32]) {
    if let Some(w) = WAV.get_or_init(open_wav) {
        let mut w = w.lock().unwrap();
        let bytes: Vec<u8> = stereo
            .iter()
            .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
            .collect();
        let _ = w.file.write_all(&bytes);
        w.frames += (stereo.len() / 2) as u32;
        if w.frames % 48000 < (stereo.len() / 2) as u32 {
            finish_wav(&mut w); // keep the header valid every second (the process is usually killed, not closed)
        }
    }
    if !playback_enabled() {
        return;
    }
    let start = {
        let mut r = RING.lock().unwrap();
        r.samples.extend(stereo.iter().copied());
        let max = MAX_MS * 48 * 2;
        if r.samples.len() > max {
            let drop = r.samples.len() - TARGET_MS * 48 * 2;
            r.samples.drain(..drop);
            r.trimmed += 1;
        }
        !std::mem::replace(&mut r.started, true)
    };
    if start {
        device::start();
    }
}

/// Fills `out` (interleaved stereo) for the device: the renderer's ring (silence for what is missing), the AudioOut
/// ring mixed over it.
fn fill(out: &mut [f32]) {
    {
        let mut r = RING.lock().unwrap();
        let n = r.samples.len().min(out.len());
        for (o, s) in out.iter_mut().zip(r.samples.drain(..n)) {
            *o = s;
        }
        if n < out.len() {
            out[n..].fill(0.0);
            r.underruns += 1;
        }
    }
    {
        let mut r2 = RING2.lock().unwrap();
        let n = r2.len().min(out.len());
        for (o, s) in out.iter_mut().zip(r2.drain(..n)) {
            *o = (*o + s).clamp(-1.0, 1.0);
        }
    }
}

fn open_wav() -> Option<Mutex<Wav>> {
    let path = crate::settings::get().audio.audio_wav.as_ref()?;
    let mut file = std::fs::File::create(path).ok()?;
    let _ = file.write_all(&[0u8; 44]);
    println!(
        "[+] host_audio: writing renderer output to {}",
        path.display()
    );
    Some(Mutex::new(Wav { file, frames: 0 }))
}

fn finish_wav(w: &mut Wav) {
    use std::io::{Seek, SeekFrom};
    let data = w.frames * 4;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes()); // PCM
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&RATE.to_le_bytes());
    h.extend_from_slice(&(RATE * 4).to_le_bytes());
    h.extend_from_slice(&4u16.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    let _ = w
        .file
        .seek(SeekFrom::Start(0))
        .and_then(|_| w.file.write_all(&h))
        .and_then(|_| w.file.seek(SeekFrom::End(0)));
}

/// (trims of an overfull ring, underruns) since start, for logs.
pub fn stats() -> (u64, u64) {
    let r = RING.lock().unwrap();
    (r.trimmed, r.underruns)
}
