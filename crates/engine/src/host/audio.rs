// The host side of sound output: 48 kHz stereo float frames from the audio renderer (horizon/audio/renderer.rs) go into a ring buffer
// that an AudioQueue (AudioToolbox, macOS and iOS) plays. Playback starts with the first pushed frame, only in windowed
// runs (never with --headless) and not with --audio=0. --audio-wav=path also writes everything pushed to a
// WAV file (works headless: how agent runs check the output without making sound).
//
// The renderer produces frames on the guest's schedule (wall-clock 5 ms frames), the queue consumes them on the
// device's clock: the ring absorbs the difference and is trimmed back to TARGET_MS when it grows past MAX_MS, so latency
// stays bounded; when it runs dry the queue plays silence.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

const RATE: f64 = 48000.0;
/// Frames per AudioQueue buffer (10 ms) and buffers in flight.
const QUEUE_FRAMES: usize = 480;
const QUEUE_BUFFERS: usize = 3;
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
        unsafe { start_queue() };
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
        unsafe { start_queue() };
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
    h.extend_from_slice(&48000u32.to_le_bytes());
    h.extend_from_slice(&(48000u32 * 4).to_le_bytes());
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

// ---- AudioToolbox ----------------------------------------------------------------------------------------------------

#[repr(C)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct AudioQueueBuffer {
    capacity: u32,
    data: *mut c_void,
    byte_size: u32,
    user_data: *mut c_void,
    packet_description_capacity: u32,
    packet_descriptions: *mut c_void,
    packet_description_count: u32,
}

type AudioQueueRef = *mut c_void;
type Callback = unsafe extern "C" fn(*mut c_void, AudioQueueRef, *mut AudioQueueBuffer);

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioQueueNewOutput(
        format: *const AudioStreamBasicDescription,
        callback: Callback,
        user: *mut c_void,
        run_loop: *const c_void,
        run_loop_mode: *const c_void,
        flags: u32,
        out: *mut AudioQueueRef,
    ) -> i32;
    fn AudioQueueAllocateBuffer(
        aq: AudioQueueRef,
        size: u32,
        out: *mut *mut AudioQueueBuffer,
    ) -> i32;
    fn AudioQueueEnqueueBuffer(
        aq: AudioQueueRef,
        buf: *mut AudioQueueBuffer,
        n: u32,
        descs: *const c_void,
    ) -> i32;
    fn AudioQueueStart(aq: AudioQueueRef, at: *const c_void) -> i32;
}

/// Fills one queue buffer from the ring (silence for what is missing) and hands it back to the queue.
unsafe extern "C" fn refill(_user: *mut c_void, aq: AudioQueueRef, buf: *mut AudioQueueBuffer) {
    let out = std::slice::from_raw_parts_mut((*buf).data as *mut f32, QUEUE_FRAMES * 2);
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
    (*buf).byte_size = (QUEUE_FRAMES * 8) as u32;
    AudioQueueEnqueueBuffer(aq, buf, 0, std::ptr::null());
}

/// iOS: playback apps must say so, or the default (ambient) audio session category is muted by the ring / silent switch
/// and by other apps. Playback category, then activate the session, before the queue starts.
#[cfg(target_os = "ios")]
unsafe fn activate_session() {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    #[link(name = "AVFAudio", kind = "framework")]
    extern "C" {
        static AVAudioSessionCategoryPlayback: *const AnyObject;
    }
    let session: *mut AnyObject = msg_send![class!(AVAudioSession), sharedInstance];
    if session.is_null() {
        return;
    }
    let mut err: *mut AnyObject = std::ptr::null_mut();
    let ok: bool = msg_send![session, setCategory: AVAudioSessionCategoryPlayback, error: &mut err];
    let active: bool = msg_send![session, setActive: true, error: &mut err];
    println!(
        "[+] host_audio: audio session category Playback {}, active {}",
        ok, active
    );
}

unsafe fn start_queue() {
    #[cfg(target_os = "ios")]
    activate_session();
    let format = AudioStreamBasicDescription {
        sample_rate: RATE,
        format_id: u32::from_be_bytes(*b"lpcm"),
        format_flags: 1 | 8, // float, packed
        bytes_per_packet: 8,
        frames_per_packet: 1,
        bytes_per_frame: 8,
        channels_per_frame: 2,
        bits_per_channel: 32,
        reserved: 0,
    };
    let mut aq: AudioQueueRef = std::ptr::null_mut();
    // No run loop: the queue calls back on its own thread.
    let err = AudioQueueNewOutput(
        &format,
        refill,
        std::ptr::null_mut(),
        std::ptr::null(),
        std::ptr::null(),
        0,
        &mut aq,
    );
    if err != 0 {
        println!(
            "[!] host_audio: AudioQueueNewOutput failed ({}): no sound",
            err
        );
        return;
    }
    for _ in 0..QUEUE_BUFFERS {
        let mut buf: *mut AudioQueueBuffer = std::ptr::null_mut();
        if AudioQueueAllocateBuffer(aq, (QUEUE_FRAMES * 8) as u32, &mut buf) == 0 {
            refill(std::ptr::null_mut(), aq, buf);
        }
    }
    let err = AudioQueueStart(aq, std::ptr::null());
    println!(
        "[+] host_audio: AudioQueue output started (48 kHz stereo){}",
        if err != 0 {
            format!(", start failed ({})", err)
        } else {
            String::new()
        }
    );
}

/// (trims of an overfull ring, underruns) since start, for logs.
pub fn stats() -> (u64, u64) {
    let r = RING.lock().unwrap();
    (r.trimmed, r.underruns)
}
