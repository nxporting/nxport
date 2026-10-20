// HLE of nn::audio's AudioOut API, timing only like horizon/audio/renderer.rs: the SDK
// implementation talks to the audout service over IPC. Appended buffers are "played" back to back in real time at the
// output's sample rate and released when their samples would have finished; a queue that runs dry stops the clock, as
// on hardware. Kirby's movie player feeds its audio track through this and paces video against it. 48 kHz 16-bit
// buffers are also played (host::audio::push_audio_out, mixed with the audio renderer's output).
//
// The AudioOut object is the game's; only its leading fields are written,
// and the queue lives here keyed by the object's address.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SAMPLE_FORMAT_PCM_INT16: i32 = 2;
const STATE_STARTED: i32 = 0;
const STATE_STOPPED: i32 = 1;

/// The leading fields of nn::audio::AudioOut.
#[repr(C)]
struct AudioOutHead {
    sample_rate: i32,
    channel_count: i32,
    sample_format: i32,
    state: i32,
    name: [u8; 256],
}

/// nn::audio::AudioOutBuffer.
#[repr(C)]
struct AudioOutBuffer {
    next: u64,
    buffer: u64,
    buffer_size: u64,
    size: u64,
    offset: u64,
}

struct Out {
    rate: u32,
    channels: u32,
    started: bool,
    /// Appended buffers with the time their last sample plays (None until started).
    queue: VecDeque<(usize, Option<Instant>, Duration)>,
    released: VecDeque<usize>,
    /// When the last scheduled buffer finishes.
    tail: Option<Instant>,
}

static OUTS: Mutex<Option<HashMap<usize, Out>>> = Mutex::new(None);

fn with<R>(o: *const AudioOutHead, f: impl FnOnce(&mut Out) -> R) -> Option<R> {
    let mut g = OUTS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(HashMap::new)
        .get_mut(&(o as usize))
        .map(f)
}

impl Out {
    fn schedule(&mut self, now: Instant) {
        if !self.started {
            return;
        }
        for e in self.queue.iter_mut() {
            if e.1.is_none() {
                let start = self.tail.map_or(now, |t| t.max(now));
                let end = start + e.2;
                e.1 = Some(end);
                self.tail = Some(end);
            }
        }
    }
    fn retire(&mut self, now: Instant) {
        while let Some(&(b, Some(end), _)) = self.queue.front() {
            if end > now {
                break;
            }
            self.queue.pop_front();
            self.released.push_back(b);
        }
    }
}

unsafe extern "C" fn initialize_parameter(p: *mut i32) {
    *p = 48000;
    *(p.add(1) as *mut u16) = 2;
    *(p.add(1) as *mut u16).add(1) = 0;
}

unsafe extern "C" fn open_default(o: *mut AudioOutHead, p: *const i32) -> u32 {
    // audout runs at 48 kHz; 0 selects the defaults (48 kHz, stereo).
    let rate = if *p == 0 { 48000 } else { *p as u32 };
    let ch = match *(p.add(1) as *const u16) {
        0 => 2,
        n => n as u32,
    };
    (*o).sample_rate = rate as i32;
    (*o).channel_count = ch as i32;
    (*o).sample_format = SAMPLE_FORMAT_PCM_INT16;
    (*o).state = STATE_STOPPED;
    let mut name = [0u8; 256];
    name[..10].copy_from_slice(b"DeviceOut\0");
    (*o).name = name;
    println!("[audout] OpenDefaultAudioOut({:p}): {} Hz x{}", o, rate, ch);
    let mut g = OUTS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(HashMap::new).insert(
        o as usize,
        Out {
            rate,
            channels: ch,
            started: false,
            queue: VecDeque::new(),
            released: VecDeque::new(),
            tail: None,
        },
    );
    0
}
unsafe extern "C" fn close(o: *mut AudioOutHead) {
    let mut g = OUTS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(HashMap::new).remove(&(o as usize));
}
unsafe extern "C" fn start(o: *mut AudioOutHead) -> u32 {
    (*o).state = STATE_STARTED;
    with(o, |s| {
        s.started = true;
        s.schedule(Instant::now());
    });
    0
}
unsafe extern "C" fn stop(o: *mut AudioOutHead) {
    (*o).state = STATE_STOPPED;
    with(o, |s| {
        // Stopping releases nothing by itself; unplayed buffers are rescheduled from the next start.
        let now = Instant::now();
        s.retire(now);
        s.started = false;
        s.tail = None;
        for e in s.queue.iter_mut() {
            e.1 = None;
        }
    });
}
unsafe extern "C" fn sample_rate(o: *const AudioOutHead) -> i32 {
    (*o).sample_rate
}
unsafe extern "C" fn channel_count(o: *const AudioOutHead) -> i32 {
    (*o).channel_count
}
unsafe extern "C" fn sample_format(o: *const AudioOutHead) -> i32 {
    (*o).sample_format
}
unsafe extern "C" fn state(o: *const AudioOutHead) -> i32 {
    (*o).state
}
unsafe extern "C" fn set_buffer_info(
    b: *mut AudioOutBuffer,
    buffer: u64,
    buffer_size: u64,
    data_size: u64,
) {
    (*b).next = 0;
    (*b).buffer = buffer;
    (*b).buffer_size = buffer_size;
    (*b).size = data_size;
    (*b).offset = 0;
}
unsafe extern "C" fn buffer_data(b: *const AudioOutBuffer) -> u64 {
    (*b).buffer
}
unsafe extern "C" fn buffer_buffer_size(b: *const AudioOutBuffer) -> u64 {
    (*b).buffer_size
}
unsafe extern "C" fn buffer_data_size(b: *const AudioOutBuffer) -> u64 {
    (*b).size
}
unsafe extern "C" fn append(o: *mut AudioOutHead, b: *mut AudioOutBuffer) -> bool {
    let size = (*b).size;
    // Sound: the buffer's PCM goes to the device mix (host_audio) now; the queue below keeps the release timing.
    if (*b).buffer >= 0x10000
        && (*o).sample_format == SAMPLE_FORMAT_PCM_INT16
        && (*o).sample_rate == 48000
    {
        let pcm = std::slice::from_raw_parts(
            ((*b).buffer + (*b).offset) as *const i16,
            (size / 2) as usize,
        );
        crate::host::audio::push_audio_out(pcm, (*o).channel_count.max(1) as usize);
    }
    with(o, |s| {
        let frames = size / (2 * s.channels.max(1) as u64);
        let d = Duration::from_nanos(frames * 1_000_000_000 / s.rate.max(1) as u64);
        s.queue.push_back((b as usize, None, d));
        s.schedule(Instant::now());
        true
    })
    .unwrap_or(false)
}
unsafe extern "C" fn get_released(o: *mut AudioOutHead) -> usize {
    with(o, |s| {
        s.retire(Instant::now());
        s.released.pop_front().unwrap_or(0)
    })
    .unwrap_or(0)
}
unsafe extern "C" fn contains(o: *const AudioOutHead, b: *const AudioOutBuffer) -> bool {
    with(o, |s| {
        s.queue.iter().any(|e| e.0 == b as usize) || s.released.contains(&(b as usize))
    })
    .unwrap_or(false)
}
unsafe extern "C" fn buffer_count(o: *const AudioOutHead) -> i32 {
    with(o, |s| {
        s.retire(Instant::now());
        s.queue.len() as i32
    })
    .unwrap_or(0)
}
unsafe extern "C" fn flush(o: *mut AudioOutHead) -> bool {
    with(o, |s| {
        while let Some((b, ..)) = s.queue.pop_front() {
            s.released.push_back(b);
        }
        s.tail = None;
        true
    })
    .unwrap_or(false)
}
unsafe extern "C" fn set_volume(_o: *mut AudioOutHead, _v: f32) {}
unsafe extern "C" fn get_volume(_o: *const AudioOutHead) -> f32 {
    1.0
}

/// Mangled name -> HLE function (chained by horizon::sdk::lookup).
pub fn lookup(name: &str) -> Option<usize> {
    macro_rules! a {
        ($x:expr) => {
            Some($x as *const () as usize)
        };
    }
    match name {
        "_ZN2nn5audio27InitializeAudioOutParameterEPNS0_17AudioOutParameterE" => {
            a!(initialize_parameter)
        }
        "_ZN2nn5audio19OpenDefaultAudioOutEPNS0_8AudioOutERKNS0_17AudioOutParameterE" => {
            a!(open_default)
        }
        "_ZN2nn5audio13CloseAudioOutEPNS0_8AudioOutE" => a!(close),
        "_ZN2nn5audio13StartAudioOutEPNS0_8AudioOutE" => a!(start),
        "_ZN2nn5audio12StopAudioOutEPNS0_8AudioOutE" => a!(stop),
        "_ZN2nn5audio21GetAudioOutSampleRateEPKNS0_8AudioOutE" => a!(sample_rate),
        "_ZN2nn5audio23GetAudioOutChannelCountEPKNS0_8AudioOutE" => a!(channel_count),
        "_ZN2nn5audio23GetAudioOutSampleFormatEPKNS0_8AudioOutE" => a!(sample_format),
        "_ZN2nn5audio16GetAudioOutStateEPKNS0_8AudioOutE" => a!(state),
        "_ZN2nn5audio21SetAudioOutBufferInfoEPNS0_14AudioOutBufferEPvmm" => a!(set_buffer_info),
        "_ZN2nn5audio28GetAudioOutBufferDataPointerEPKNS0_14AudioOutBufferE" => a!(buffer_data),
        "_ZN2nn5audio27GetAudioOutBufferBufferSizeEPKNS0_14AudioOutBufferE" => {
            a!(buffer_buffer_size)
        }
        "_ZN2nn5audio25GetAudioOutBufferDataSizeEPKNS0_14AudioOutBufferE" => a!(buffer_data_size),
        "_ZN2nn5audio20AppendAudioOutBufferEPNS0_8AudioOutEPNS0_14AudioOutBufferE" => a!(append),
        "_ZN2nn5audio25GetReleasedAudioOutBufferEPNS0_8AudioOutE" => a!(get_released),
        "_ZN2nn5audio22ContainsAudioOutBufferEPKNS0_8AudioOutEPKNS0_14AudioOutBufferE" => {
            a!(contains)
        }
        "_ZN2nn5audio22GetAudioOutBufferCountEPKNS0_8AudioOutE" => a!(buffer_count),
        "_ZN2nn5audio21FlushAudioOutBuffersEPNS0_8AudioOutE" => a!(flush),
        "_ZN2nn5audio17SetAudioOutVolumeEPNS0_8AudioOutEf" => a!(set_volume),
        "_ZN2nn5audio17GetAudioOutVolumeEPKNS0_8AudioOutE" => a!(get_volume),
        _ => None,
    }
}
