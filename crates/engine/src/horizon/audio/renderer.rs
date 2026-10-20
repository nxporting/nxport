// Server side of the audio renderer (the "audren" sysmodule), timing only.
//
// The game's nn::audio code runs natively: it serializes its AudioRendererConfig into an update buffer,
// calls IAudioRenderer::RequestUpdate (vtable slot 8 of the service object behind the AudioRendererHandle),
// and parses the returned statuses back into the config. Those statuses are the only way voice playback,
// wave-buffer release, memory-pool attachment and the renderer's elapsed frame count ever advance, so a
// stubbed RequestUpdate freezes every sound in its initial state (and anything the game times against
// sound waits forever).
//
// This module implements that service call. Wire format and state machines follow Ryujinx
// (Ryujinx.Audio/Renderer/Server: StateUpdater, VoiceInfo, PoolMapper, Dsp/DataSourceHelper). No samples
// voice samples are decoded yet: each 5 ms renderer frame advances every playing voice through its wave buffers at its
// sample rate and pitch, exactly as the DSP would, which is what the game observes.
//
// Sound: TotK mixes on the CPU and hands its output to the renderer through auxiliary-buffer (Aux) effects: the DSP
// writes a mix's buffers into the effect's send ring and reads the game's samples from its return ring (Ryujinx
// AuxiliaryBufferCommand). Each frame the mix graph is processed as Ryujinx's CommandGenerator orders it (submixes
// farthest from the final mix first: effects by processing order, then the mix into its destination through the volume
// matrix; then the final mix's effects and volume) and the device sink's channels go to host_audio (5.1 downmixed to
// stereo as DownMixSurroundToStereoCommand). Not implemented yet: voice sample decoding, splitters, and every effect but
// Aux (Reverb3d, Limiter, ... pass their input through).

use std::sync::Mutex;

const WAVE_BUFFER_COUNT: usize = 4;
const HEADER_SIZE: usize = 0x40;
const RESULT_INVALID_UPDATE_INFO: u32 = 0x5299; // audio module 153, description 41 (InvalidUpdateInfo)

fn rev_number(rev: i32) -> i32 {
    (rev >> 24) - b'0' as i32
}

#[derive(Clone, Copy)]
struct WaveBuffer {
    /// Guest address of the samples, their size, and of the ADPCM loop context (AdpcmLoopContext) if any.
    address: u64,
    size: u64,
    context: u64,
    start: u32,
    end: u32,
    loop_start: u32,
    loop_end: u32,
    loop_count: i32,
    looping: bool,
    end_of_stream: bool,
    sent_to_dsp: bool,
}

impl Default for WaveBuffer {
    /// An empty slot counts as already handed to the DSP (Ryujinx VoiceInfo.InitializeWaveBuffers), so it
    /// is never played or reported as consumed until the client fills it.
    fn default() -> Self {
        WaveBuffer {
            address: 0,
            size: 0,
            context: 0,
            start: 0,
            end: 0,
            loop_start: 0,
            loop_end: 0,
            loop_count: 0,
            looping: false,
            end_of_stream: false,
            sent_to_dsp: true,
        }
    }
}

/// DSP-side state of one voice channel (Ryujinx VoiceState), keyed by channel resource id.
#[derive(Clone, Copy, Default)]
struct ChannelState {
    played_sample_count: u64,
    offset: i32,
    wave_buffer_index: usize,
    wave_buffer_consumed: u32,
    valid: [bool; WAVE_BUFFER_COUNT],
    loop_count: i32,
    fraction: f32,
    /// ADPCM decoder history (the two previous output samples) and the last source sample of the previous frame (the
    /// resampler interpolates from it).
    hist: [i32; 2],
    /// The last source samples of the previous frame (the resampler's taps reach back over them).
    history: [f32; 8],
    /// Transposed direct form II state of the voice's two biquad filters.
    biquad: [[f32; 2]; 2],
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum PlayState {
    Started,
    #[default]
    Stopped,
    Stopping,
    Paused,
}

#[derive(Clone, Copy, Default)]
struct Voice {
    in_use: bool,
    is_new: bool,
    play_state: PlayState,
    sample_rate: u32,
    pitch: f32,
    channel_count: usize,
    channel_ids: [usize; 6],
    wave_buffers_index: u32,
    flush_count: u32,
    reset_played_on_loop: bool,
    skip_src: bool,
    wave_buffers: [WaveBuffer; WAVE_BUFFER_COUNT],
    format: u8,
    /// SampleRateConversionQuality: 0 default (4 taps), 1 high (8 taps), 2 low (nearest).
    src_quality: u8,
    /// The voice's two biquad filters (BiquadFilterParameter at +0x24): enabled, then a0 a1 a2 b1 b2.
    biquads: [(bool, [f32; 5]); 2],
    volume: f32,
    prev_volume: f32,
    /// AdpcmParameter (16 coefficients, then the initial context) of an ADPCM voice.
    adpcm_param: u64,
    mix_id: i32,
    splitter_id: i32,
    /// Wave buffers taken from the client since the voice became new (debug accounting).
    appended: u32,
}

/// A mix (MixParameter, 0x930 bytes): routing state kept across updates (sent only when dirty from revision 7).
#[derive(Clone)]
struct Mix {
    used: bool,
    volume: f32,
    buffer_count: usize,
    dest_mix: i32,
    dest_splitter: i32,
    /// [source buffer][destination buffer], 24 x 24.
    matrix: Vec<f32>,
}

impl Default for Mix {
    fn default() -> Self {
        Mix {
            used: false,
            volume: 0.0,
            buffer_count: 0,
            dest_mix: UNUSED_MIX_ID,
            dest_splitter: -1,
            matrix: vec![0.0; 24 * 24],
        }
    }
}

const UNUSED_MIX_ID: i32 = 0x7fff_ffff;

/// An effect (EffectInParameter, 0xC0 bytes): type, enable, owning mix, processing order and its 0xA0 parameter bytes.
#[derive(Clone)]
struct Effect {
    kind: u8,
    enabled: bool,
    mix_id: i32,
    order: u32,
    params: [u8; 0xA0],
}

const EFFECT_AUX: u8 = 2;
const EFFECT_LIMITER: u8 = 7;
const EFFECT_REVERB3D: u8 = 5;
const EFFECT_DELAY: u8 = 3;
const EFFECT_BIQUAD: u8 = 6;

/// LimiterCommandVersion1 state per effect slot: detector and gain moving averages and the look-ahead delay line.
#[derive(Clone, Default)]
struct LimiterState {
    detector: Vec<f32>,
    gain: Vec<f32>,
    delay: Vec<f32>,
    position: Vec<usize>,
}

/// A voice channel resource (VoiceChannelResourceInParameter, 0x70): volumes into each buffer of the voice's mix.
#[derive(Clone, Copy)]
struct ChannelResource {
    mix: [f32; 24],
    prev: [f32; 24],
}

/// A splitter destination (SplitterDestinationInParameterVersion1, 0x70): a mix and volumes into its buffers.
#[derive(Clone, Copy)]
struct SplitterDestination {
    used: bool,
    mix_id: i32,
    volume: [f32; 24],
    prev: [f32; 24],
}

const SAMPLE_FORMAT_PCM16: u8 = 2;
const SAMPLE_FORMAT_FLOAT: u8 = 5;
const SAMPLE_FORMAT_ADPCM: u8 = 6;

struct Renderer {
    sample_rate: u32,
    sample_count: u32,
    voices: Vec<Voice>,
    channels: Vec<ChannelState>,
    mixes: Vec<Mix>,
    effects: Vec<Effect>,
    limiters: std::collections::HashMap<usize, LimiterState>,
    reverbs3d: std::collections::HashMap<usize, crate::horizon::audio::fx::Reverb3d>,
    /// BiquadFilter effect state per effect slot: transposed direct form II, per channel.
    biquad_fx: std::collections::HashMap<usize, Vec<[f32; 2]>>,
    delays: std::collections::HashMap<usize, crate::horizon::audio::fx::Delay>,
    /// The client's revision number (from its update header).
    revision: i32,
    resources: Vec<ChannelResource>,
    /// Splitter id -> its destination ids in order; destination id -> destination.
    splitters: std::collections::HashMap<i32, Vec<i32>>,
    destinations: Vec<SplitterDestination>,
    /// Buffer offset of each mix in mix_buffers for the frame being rendered.
    mix_offsets: Vec<usize>,
    /// The device sink's input buffers (relative to the final mix) and custom downmix coefficients.
    sink_inputs: Vec<usize>,
    sink_downmix: Option<[f32; 4]>,
    mix_buffers: Vec<f32>,
    elapsed_frames: u64,
    last_frame_at: Option<std::time::Instant>,
    updates: u64,
}

static RENDERER: Mutex<Option<Renderer>> = Mutex::new(None);

/// The service object the game's AudioRendererHandle points at: a vtable pointer followed by the vtable
/// itself. Slot 8 (+0x40) is IAudioRenderer::RequestUpdate; every other slot logs and fails.
#[repr(C)]
struct ServiceObject {
    vtable: *const usize,
    slots: [usize; 24],
}

static mut SERVICE: ServiceObject = ServiceObject {
    vtable: std::ptr::null(),
    slots: [0; 24],
};

/// Guest-callable trampolines (x18 / tpidr_el0 restored on return) for the vtable entries.
static TRAMPOLINES: Mutex<(usize, usize)> = Mutex::new((0, 0));

pub fn request_update_fn() -> *const () {
    request_update as *const ()
}
pub fn unexpected_call_fn() -> *const () {
    unexpected_service_call as *const ()
}
pub fn set_trampolines(request_update: usize, unexpected: usize) {
    *TRAMPOLINES.lock().unwrap() = (request_update, unexpected);
}

unsafe extern "C" fn unexpected_service_call() -> u32 {
    let lr: usize;
    std::arch::asm!("mov {}, x30", out(reg) lr);
    eprintln!(
        "[!] audren: unexpected IAudioRenderer call (return address {:#x})",
        lr
    );
    RESULT_INVALID_UPDATE_INFO
}

#[repr(C)]
struct SfBuffer {
    ptr: *mut u8,
    size: u64,
}

/// Creates the renderer for nn::audio::OpenAudioRenderer and returns the service-object pointer to store
/// in the AudioRendererHandle. `param` is the game's nn::audio::AudioRendererParameter.
pub unsafe fn open(param: *const u32) -> usize {
    let (sample_rate, sample_count) = if param.is_null() {
        (48000, 240)
    } else {
        (*param, *param.add(1))
    };
    println!(
        "[+] audren: renderer opened ({} Hz, {} samples per frame)",
        sample_rate, sample_count
    );
    *RENDERER.lock().unwrap() = Some(Renderer {
        sample_rate: sample_rate.max(1),
        sample_count: sample_count.max(1),
        voices: Vec::new(),
        channels: Vec::new(),
        mixes: Vec::new(),
        effects: Vec::new(),
        limiters: std::collections::HashMap::new(),
        reverbs3d: std::collections::HashMap::new(),
        biquad_fx: std::collections::HashMap::new(),
        delays: std::collections::HashMap::new(),
        revision: 0,
        resources: Vec::new(),
        splitters: std::collections::HashMap::new(),
        destinations: Vec::new(),
        mix_offsets: Vec::new(),
        sink_inputs: Vec::new(),
        sink_downmix: None,
        mix_buffers: Vec::new(),
        elapsed_frames: 0,
        last_frame_at: None,
        updates: 0,
    });
    let svc = &mut *std::ptr::addr_of_mut!(SERVICE);
    let (tramp_update, tramp_unexpected) = *TRAMPOLINES.lock().unwrap();
    let pick = |t: usize, f: *const ()| if t != 0 { t } else { f as usize };
    for s in svc.slots.iter_mut() {
        *s = pick(tramp_unexpected, unexpected_service_call as *const ());
    }
    svc.slots[8] = pick(tramp_update, request_update as *const ());
    svc.vtable = svc.slots.as_ptr();
    svc as *mut ServiceObject as usize
}

/// Starting the renderer restarts its frame clock (no frames elapse while it is stopped).
pub fn set_running(running: bool) {
    if let Some(r) = RENDERER.lock().unwrap().as_mut() {
        r.last_frame_at = running.then(std::time::Instant::now);
    }
}

unsafe extern "C" fn request_update(
    _this: usize,
    out: *const SfBuffer,
    perf: *const SfBuffer,
    input: *const SfBuffer,
) -> u32 {
    crate::horizon::hle::record_hle_call("audren::RequestUpdate");
    let (out, input) = (&*out, &*input);
    let _ = perf; // performance histories are not produced (HistorySize 0)
    let mut guard = RENDERER.lock().unwrap();
    let Some(r) = guard.as_mut() else {
        return RESULT_INVALID_UPDATE_INFO;
    };
    let inp = std::slice::from_raw_parts(input.ptr, input.size as usize);
    let outb = std::slice::from_raw_parts_mut(out.ptr, out.size as usize);
    match r.update(inp, outb) {
        Ok(()) => 0,
        Err(e) => {
            static REPORTED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if REPORTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 5 {
                eprintln!("[!] audren: RequestUpdate rejected: {}", e);
            }
            RESULT_INVALID_UPDATE_INFO
        }
    }
}

/// Channel frames that ran out of queued wave buffers mid-sound (--audio-debug prints them).
static STARVED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Aux frames processed and frames that read return data the game had not written yet (--audio-debug prints them).
static AUX_STATS: [std::sync::atomic::AtomicU64; 2] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

fn anomaly(msg: String) {
    static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 60 {
        eprintln!("[!] audren anomaly: {}", msg);
    }
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn rd64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn wr32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn wr64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

impl Renderer {
    fn update(&mut self, inp: &[u8], out: &mut [u8]) -> Result<(), String> {
        if inp.len() < HEADER_SIZE + 0x10 {
            return Err(format!("input too small ({} bytes)", inp.len()));
        }
        // UpdateDataHeader: revision, then section sizes in this order.
        let revision = rd32(inp, 0) as i32;
        let [behaviour_sz, pools_sz, voices_sz, resources_sz, effects_sz, mixes_sz, sinks_sz, perf_sz, splitter_sz, _render_sz] =
            std::array::from_fn(|i| rd32(inp, 4 + i * 4) as usize);
        let total_in = rd32(inp, 0x3C) as usize;
        if total_in > inp.len() {
            return Err(format!(
                "header total {:#x} > buffer {:#x}",
                total_in,
                inp.len()
            ));
        }
        let rev = rev_number(revision);
        self.revision = rev;

        // Frames the DSP rendered since the previous update, with the state the previous updates set up.
        self.render_elapsed_frames();

        out.fill(0);
        let mut o = HEADER_SIZE;
        let mut i = HEADER_SIZE + behaviour_sz; // BehaviourParameter carries flags only

        // Memory pools (0x20 in, 0x10 out): RequestAttach -> Attached, RequestDetach -> Detached.
        let pool_count = pools_sz / 0x20;
        for p in 0..pool_count {
            let base = i + p * 0x20;
            let state = rd32(inp, base + 0x10);
            let new_state = match state {
                4 => 5, // RequestAttach -> Attached
                2 => 3, // RequestDetach -> Detached
                _ => 0, // unchanged: the SDK only reads the status for requests it made
            };
            if new_state != 0 {
                wr32(out, o + p * 0x10, new_state);
            }
        }
        i += pools_sz;
        o += pool_count * 0x10;
        let out_pools_sz = pool_count * 0x10;

        // Voice channel resources (0x70 each): only their count matters here.
        let channel_count = resources_sz / 0x70;
        if self.channels.len() < channel_count {
            self.channels.resize(channel_count, ChannelState::default());
        }
        if self.resources.len() < channel_count {
            self.resources.resize(
                channel_count,
                ChannelResource {
                    mix: [0.0; 24],
                    prev: [0.0; 24],
                },
            );
        }
        for k in 0..channel_count {
            let b = i + k * 0x70;
            let id = rd32(inp, b) as usize;
            if id < self.resources.len() && inp[b + 0x64] != 0 {
                let r = &mut self.resources[id];
                for j in 0..24 {
                    r.mix[j] = f32::from_bits(rd32(inp, b + 4 + j * 4));
                }
            }
        }
        i += resources_sz;

        // Voices: VoiceInParameter is 0x188 bytes from revision 15 (float biquads), 0x170 before.
        let (vsz, shift) = if rev >= 15 { (0x188, 0x18) } else { (0x170, 0) };
        let voice_count = if voices_sz % vsz == 0 {
            voices_sz / vsz
        } else {
            return Err(format!(
                "voices size {:#x} not a multiple of {:#x}",
                voices_sz, vsz
            ));
        };
        if self.voices.len() < voice_count {
            self.voices.resize(voice_count, Voice::default());
        }
        for v in 0..voice_count {
            let base = i + v * vsz;
            self.update_voice(v, &inp[base..base + vsz], shift, rev);
            self.write_voice_status(v, &mut out[o + v * 0x10..o + v * 0x10 + 0x10]);
        }
        i += voices_sz;
        o += voice_count * 0x10;
        let out_voices_sz = voice_count * 0x10;

        // Effects (0xC0 in; out 0x10, or 0x90 with result state from revision 9).
        let effect_count = effects_sz / 0xC0;
        let effect_out = if rev >= 9 { 0x90 } else { 0x10 };
        if self.effects.len() < effect_count {
            self.effects.resize(
                effect_count,
                Effect {
                    kind: 0,
                    enabled: false,
                    mix_id: UNUSED_MIX_ID,
                    order: u32::MAX,
                    params: [0; 0xA0],
                },
            );
        }
        for e in 0..effect_count {
            let b = i + e * 0xC0;
            let enabled = inp[b + 2] != 0;
            out[o + e * effect_out] = if enabled { 3 } else { 4 }; // EffectState Enabled / Disabled
            let fx = &mut self.effects[e];
            fx.kind = inp[b];
            fx.enabled = enabled;
            fx.mix_id = rd32(inp, b + 4) as i32;
            fx.order = rd32(inp, b + 0x18);
            fx.params.copy_from_slice(&inp[b + 0x20..b + 0xC0]);
        }
        i += effects_sz;
        o += effect_count * effect_out;
        let out_effects_sz = effect_count * effect_out;

        // Splitters and mixes only configure routing; nothing to report.
        // Splitters: header {magic 'SNDH', splitter count, destination count}, then the changed splitters (SNDI: id,
        // sample rate, destination count, destination ids, then 12 unused bytes: SplitterContext.UpdateState) and
        // destinations (SNDD, version 1 before revision 12). Without the 12-byte skip only the first splitter of each
        // update was read, and the destination loop started inside the next SNDI record (TotK sounds routed through
        // the dropped splitters were silent, e.g. taking out the paraglider).
        if splitter_sz >= 0x20 && rd32(inp, i) == 0x4844_4E53 {
            let (nsplit, ndest) = (rd32(inp, i + 4) as usize, rd32(inp, i + 8) as usize);
            let end = i + splitter_sz;
            let mut q = i + 0x20;
            let (mut got_split, mut got_dest) = (0, 0);
            for _ in 0..nsplit {
                if q + 0x10 > end || rd32(inp, q) != 0x4944_4E53 {
                    break;
                }
                got_split += 1;
                let id = rd32(inp, q + 4) as i32;
                let n = (rd32(inp, q + 0xC) as usize).min((end - q - 0x10) / 4);
                let ids = (0..n).map(|k| rd32(inp, q + 0x10 + k * 4) as i32).collect();
                self.splitters.insert(id, ids);
                q += 0x10 + n * 4 + 0xC;
            }
            for _ in 0..ndest {
                if q + 0x70 > end || rd32(inp, q) != 0x4444_4E53 {
                    break;
                }
                got_dest += 1;
                let id = rd32(inp, q + 4) as usize;
                if id < 4096 {
                    if self.destinations.len() <= id {
                        self.destinations.resize(
                            id + 1,
                            SplitterDestination {
                                used: false,
                                mix_id: UNUSED_MIX_ID,
                                volume: [0.0; 24],
                                prev: [0.0; 24],
                            },
                        );
                    }
                    let d = &mut self.destinations[id];
                    for j in 0..24 {
                        d.volume[j] = f32::from_bits(rd32(inp, q + 8 + j * 4));
                    }
                    d.mix_id = rd32(inp, q + 0x68) as i32;
                    let used = inp[q + 0x6C] != 0;
                    // SplitterDestination.Update: from revision 13 the ResetPrevVolume flag decides, before it a destination
                    // that becomes used starts from its current volumes.
                    let reset = if rev >= 13 {
                        inp[q + 0x6D] != 0
                    } else {
                        !d.used && used
                    };
                    if reset {
                        d.prev = d.volume;
                    }
                    d.used = used;
                }
                q += 0x70;
            }
            if got_split < nsplit || got_dest < ndest {
                anomaly(format!("splitter update: header names {} splitters / {} destinations, read {} / {} (layout mismatch)",
                    nsplit, ndest, got_split, got_dest));
            }
        }
        // Mixes: from revision 7 a 0x20 header {magic, count} precedes only the dirty mixes, each naming its id.
        {
            let at = i + splitter_sz;
            let dirty_only = mixes_sz % 0x930 == 0x20;
            let (first, count) = if dirty_only {
                (at + 0x20, rd32(inp, at + 4) as usize)
            } else {
                (at, mixes_sz / 0x930)
            };
            for k in 0..count.min((mixes_sz - if dirty_only { 0x20 } else { 0 }) / 0x930) {
                let b = first + k * 0x930;
                let id = if dirty_only {
                    rd32(inp, b + 0x10) as usize
                } else {
                    k
                };
                if id >= 1024 {
                    continue;
                }
                if self.mixes.len() <= id {
                    self.mixes.resize(id + 1, Mix::default());
                }
                let m = &mut self.mixes[id];
                m.used = inp[b + 0xC] != 0;
                m.volume = f32::from_bits(rd32(inp, b));
                m.buffer_count = (rd32(inp, b + 8) as usize).min(24);
                m.dest_mix = rd32(inp, b + 0x924) as i32;
                m.dest_splitter = rd32(inp, b + 0x928) as i32;
                for (j, v) in m.matrix.iter_mut().enumerate() {
                    *v = f32::from_bits(rd32(inp, b + 0x24 + j * 4));
                }
            }
        }
        i += splitter_sz + mixes_sz;

        // Sinks (0x140 in, 0x20 out).
        let sink_count = sinks_sz / 0x140;
        for k in 0..sink_count {
            let b = i + k * 0x140;
            if inp[b] == 1 && inp[b + 1] != 0 {
                // DeviceParameter at +0x20: InputCount +0x100, Input[6] +0x104, DownMixParameterEnabled +0x10B,
                // DownMixParameter[4] +0x10C.
                let d = b + 0x20;
                let n = (rd32(inp, d + 0x100) as usize).min(6);
                self.sink_inputs = inp[d + 0x104..d + 0x104 + n]
                    .iter()
                    .map(|&x| x as usize)
                    .collect();
                self.sink_downmix = (inp[d + 0x10B] != 0)
                    .then(|| std::array::from_fn(|j| f32::from_bits(rd32(inp, d + 0x10C + j * 4))));
            }
        }
        i += sinks_sz;
        o += sink_count * 0x20;
        let out_sinks_sz = sink_count * 0x20;

        // Performance (0x10 in, 0x10 out: HistorySize 0).
        i += perf_sz;
        o += 0x10;

        // Behaviour error info (no errors).
        o += 0xB0;

        // Renderer info (revision 5+): elapsed frame count.
        let render_out = if rev >= 5 {
            wr64(out, o, self.elapsed_frames);
            o += 0x10;
            0x10
        } else {
            0
        };

        let _ = i;
        wr32(out, 0, revision as u32);
        wr32(out, 0x04, 0xB0);
        wr32(out, 0x08, out_pools_sz as u32);
        wr32(out, 0x0C, out_voices_sz as u32);
        wr32(out, 0x14, out_effects_sz as u32);
        wr32(out, 0x1C, out_sinks_sz as u32);
        wr32(out, 0x20, 0x10);
        wr32(out, 0x28, render_out);
        wr32(out, 0x3C, o as u32);
        if o != out.len() {
            return Err(format!(
                "output total {:#x} != SDK buffer {:#x} (rev {}, voices {}, effects {}, sinks {})",
                o,
                out.len(),
                rev,
                voice_count,
                effect_count,
                sink_count
            ));
        }
        self.updates += 1;
        if self.updates == 1 {
            println!(
                "[+] audren: first update: rev {} pools {} voices {} channels {} effects {} sinks {}",
                rev, pool_count, voice_count, channel_count, effect_count, sink_count
            );
        }
        Ok(())
    }

    fn update_voice(&mut self, v: usize, p: &[u8], s: usize, rev: i32) {
        let is_new = p[8] != 0;
        let in_use = p[9] != 0;
        let voice = &mut self.voices[v];
        voice.in_use = in_use;
        if !in_use {
            return;
        }
        if is_new {
            *voice = Voice {
                in_use: true,
                is_new: true,
                ..Voice::default()
            };
        }
        // UpdatePlayState
        voice.play_state = match p[0xA] {
            0 => PlayState::Started,
            1 if voice.play_state == PlayState::Stopped => PlayState::Stopped,
            1 => PlayState::Stopping,
            _ => PlayState::Paused,
        };
        voice.sample_rate = rd32(p, 0x0C);
        voice.channel_count = (rd32(p, 0x18) as usize).min(6);
        voice.pitch = f32::from_bits(rd32(p, 0x1C));
        voice.format = p[0xB];
        voice.src_quality = if rev >= 5 { p[0x15E + s] } else { 0 };
        // BiquadFilterParameter (0xC): Enable, reserved, Numerator[3], Denominator[2] in Q14 (revision 15 has float ones).
        if rev < 15 {
            for f in 0..2 {
                let b = 0x24 + f * 0xC;
                let q = |o: usize| i16::from_le_bytes([p[b + o], p[b + o + 1]]) as f32 / 16384.0;
                let enable = p[b] != 0;
                if enable && !voice.biquads[f].0 {
                    for c in 0..voice.channel_count {
                        if let Some(ch) = self.channels.get_mut(voice.channel_ids[c]) {
                            ch.biquad[f] = [0.0; 2]; // BiquadFilterNeedInitialization
                        }
                    }
                }
                voice.biquads[f] = (enable, [q(2), q(4), q(6), q(8), q(10)]);
            }
        }
        voice.volume = f32::from_bits(rd32(p, 0x20));
        if is_new {
            // VoiceState initializes PreviousVolume to 0: a new voice ramps in over its first frame.
            voice.prev_volume = 0.0;
        }
        voice.adpcm_param = rd64(p, 0x48 + s);
        voice.mix_id = rd32(p, 0x58 + s) as i32;
        voice.splitter_id = rd32(p, 0x5C + s) as i32;
        voice.wave_buffers_index = rd32(p, 0x40 + s);
        for c in 0..6 {
            voice.channel_ids[c] = rd32(p, 0x140 + s + c * 4) as usize;
        }
        if rev >= 5 {
            voice.flush_count += p[0x159 + s] as u32;
            let behaviour = u16::from_le_bytes([p[0x15C + s], p[0x15D + s]]);
            voice.reset_played_on_loop = behaviour & 1 != 0;
            voice.skip_src = behaviour & 2 != 0;
        }
        if is_new {
            for c in 0..voice.channel_count {
                if let Some(ch) = self.channels.get_mut(voice.channel_ids[c]) {
                    ch.valid = [false; WAVE_BUFFER_COUNT];
                }
                // VoiceState.InitializeResources: the channel resource's previous volumes become its current ones (no ramp
                // from whatever an earlier voice left there).
                if let Some(r) = self.resources.get_mut(voice.channel_ids[c]) {
                    r.prev = r.mix;
                }
            }
        }
        // UpdateWaveBuffers: take every buffer the client has not marked as already sent.
        for w in 0..WAVE_BUFFER_COUNT {
            let b = 0x60 + s + w * 0x38;
            let sent_to_server = p[b + 0x1A] != 0;
            if !sent_to_server {
                let ch0 = voice.channel_ids[0];
                if let Some(ch) = self.channels.get(ch0) {
                    if ch.valid[w] && !is_new {
                        anomaly(format!("voice {} slot {} replaced while still queued (consumed {}, appended {})", v, w, ch.wave_buffer_consumed, voice.appended));
                    }
                }
                voice.appended += 1;
                voice.wave_buffers[w] = WaveBuffer {
                    address: rd64(p, b),
                    size: rd64(p, b + 8),
                    context: rd64(p, b + 0x20),
                    start: rd32(p, b + 0x10),
                    end: rd32(p, b + 0x14),
                    looping: p[b + 0x18] != 0,
                    end_of_stream: p[b + 0x19] != 0,
                    loop_count: rd32(p, b + 0x1C) as i32,
                    loop_start: rd32(p, b + 0x30),
                    loop_end: rd32(p, b + 0x34),
                    sent_to_dsp: false,
                };
            }
        }
    }

    fn write_voice_status(&self, v: usize, out: &mut [u8]) {
        let voice = &self.voices[v];
        if !voice.in_use || voice.is_new {
            return; // zeros: VoiceDropFlag false, nothing played yet
        }
        let Some(ch) = self.channels.get(voice.channel_ids[0]) else {
            return;
        };
        if ch.wave_buffer_consumed > voice.appended {
            anomaly(format!(
                "voice {} reports {} buffers played but only {} were queued",
                v, ch.wave_buffer_consumed, voice.appended
            ));
        }
        wr64(out, 0, ch.played_sample_count);
        wr32(out, 8, ch.wave_buffer_consumed);
    }

    fn render_elapsed_frames(&mut self) {
        let Some(last) = self.last_frame_at else {
            return;
        };
        let frame =
            std::time::Duration::from_secs_f64(self.sample_count as f64 / self.sample_rate as f64);
        let now = std::time::Instant::now();
        let mut frames = (now.duration_since(last).as_secs_f64() / frame.as_secs_f64()) as u32;
        // After a long host stall, catch up a little rather than fast-forwarding every stream.
        if frames > 8 {
            self.last_frame_at = Some(now - frame * 8);
            frames = 8;
        }
        for _ in 0..frames {
            self.render_frame();
        }
        if let Some(l) = self.last_frame_at.as_mut() {
            *l += frame * frames;
        }
    }

    /// One DSP frame: command generation (VoiceInfo.UpdateForCommandGeneration) then the wave-buffer walk of
    /// DataSourceHelper.ProcessWaveBuffers for every channel of every playing voice.
    fn render_frame(&mut self) {
        self.elapsed_frames += 1;
        let (out_rate, sample_count) = (self.sample_rate, self.sample_count as i32);
        self.prepare_mix_buffers();
        // Decoded, resampled output of every playing channel: (voice, channel index within the voice, samples).
        let mut outputs: Vec<(usize, usize, Vec<f32>)> = Vec::new();
        for (vi, voice) in self.voices.iter_mut().enumerate() {
            if !voice.in_use {
                continue;
            }
            let chans: Vec<usize> = voice.channel_ids[..voice.channel_count]
                .iter()
                .copied()
                .filter(|&c| c < self.channels.len())
                .collect();
            if voice.is_new {
                for &c in &chans {
                    self.channels[c] = ChannelState::default();
                }
                voice.is_new = false;
            }
            if voice.flush_count != 0 {
                let mut idx = voice.wave_buffers_index as usize % WAVE_BUFFER_COUNT;
                for _ in 0..voice.flush_count {
                    voice.wave_buffers[idx].sent_to_dsp = true;
                    for &c in &chans {
                        let ch = &mut self.channels[c];
                        if ch.valid[idx] {
                            ch.wave_buffer_index = (ch.wave_buffer_index + 1) % WAVE_BUFFER_COUNT;
                            ch.wave_buffer_consumed += 1;
                            ch.valid[idx] = false;
                        }
                    }
                    idx = (idx + 1) % WAVE_BUFFER_COUNT;
                }
                voice.flush_count = 0;
            }
            match voice.play_state {
                PlayState::Started => {
                    for w in 0..WAVE_BUFFER_COUNT {
                        if !voice.wave_buffers[w].sent_to_dsp {
                            for &c in &chans {
                                self.channels[c].valid[w] = true;
                            }
                            voice.wave_buffers[w].sent_to_dsp = true;
                        }
                    }
                    let ratio = if voice.skip_src {
                        1.0
                    } else {
                        voice.sample_rate as f32 / out_rate as f32 * voice.pitch
                    };
                    for ci in 0..voice.channel_count {
                        let c = voice.channel_ids[ci];
                        if c >= self.channels.len() {
                            continue;
                        }
                        let src = Source {
                            format: voice.format,
                            channels: voice.channel_count,
                            index: ci,
                            adpcm_param: voice.adpcm_param,
                            quality: voice.src_quality,
                            skip_src: voice.skip_src,
                        };
                        let mut out = vec![0.0f32; sample_count.max(0) as usize];
                        process_wave_buffers(
                            &mut self.channels[c],
                            &voice.wave_buffers,
                            ratio,
                            sample_count,
                            voice.reset_played_on_loop,
                            &src,
                            &mut out,
                        );
                        // The voice's biquad filters (BiquadFilterHelper.ProcessBiquadFilter), before the volume ramp.
                        for (f, &(enabled, [a0, a1, a2, b1, b2])) in
                            voice.biquads.iter().enumerate()
                        {
                            if !enabled {
                                continue;
                            }
                            let st = &mut self.channels[c].biquad[f];
                            for x in out.iter_mut() {
                                let input = *x;
                                let y = input * a0 + st[0];
                                st[0] = input * a1 + y * b1 + st[1];
                                st[1] = input * a2 + y * b2;
                                *x = y;
                            }
                        }
                        outputs.push((vi, ci, out));
                    }
                }
                PlayState::Stopping => {
                    for w in 0..WAVE_BUFFER_COUNT {
                        voice.wave_buffers[w].sent_to_dsp = true;
                        for &c in &chans {
                            let ch = &mut self.channels[c];
                            if ch.valid[w] {
                                ch.wave_buffer_index =
                                    (ch.wave_buffer_index + 1) % WAVE_BUFFER_COUNT;
                                ch.wave_buffer_consumed += 1;
                            }
                            ch.valid[w] = false;
                        }
                    }
                    for &c in &chans {
                        let ch = &mut self.channels[c];
                        ch.offset = 0;
                        ch.played_sample_count = 0;
                        ch.fraction = 0.0;
                    }
                    // CommandGenerator: a voice that stops resets PreviousVolume, so a restart ramps in from silence.
                    voice.prev_volume = 0.0;
                    voice.play_state = PlayState::Stopped;
                }
                PlayState::Stopped | PlayState::Paused => {}
            }
        }
        self.mix_voices(outputs);
        self.mix_frame();
    }

    /// Lays out this frame's mix buffers by mix id (MixContext.UpdateMixBufferOffset) and clears them.
    fn prepare_mix_buffers(&mut self) {
        let n = self.sample_count as usize;
        self.mix_offsets.clear();
        let mut total = 0;
        for m in &self.mixes {
            self.mix_offsets.push(total);
            total += m.buffer_count;
        }
        self.mix_buffers.clear();
        self.mix_buffers.resize(total * n, 0.0);
    }

    /// Adds one buffer of samples into `count` buffers of mix `mix`, each volume ramping from `prev[b]` to `cur[b]`
    /// across the frame (MixRampGroupedCommand).
    fn add_to_mix(&mut self, samples: &[f32], mix: i32, prev: &[f32; 24], cur: &[f32; 24]) {
        if mix < 0 || mix as usize >= self.mixes.len() || !self.mixes[mix as usize].used {
            return;
        }
        let n = samples.len();
        let (off, count) = (
            self.mix_offsets[mix as usize],
            self.mixes[mix as usize].buffer_count,
        );
        for b in 0..count {
            let (p, c) = (prev[b], cur[b]);
            if p == 0.0 && c == 0.0 {
                continue;
            }
            let delta = (c - p) / n as f32;
            let dst = &mut self.mix_buffers[(off + b) * n..(off + b + 1) * n];
            for k in 0..n {
                dst[k] += samples[k] * (p + delta * k as f32);
            }
        }
    }

    /// Voice output into the mixes: the voice volume ramp, then its mix through the channel resource's volumes, or its
    /// splitter's destinations (channel c uses destinations c, c + channel count, ...), as CommandGenerator.GenerateVoice.
    fn mix_voices(&mut self, outputs: Vec<(usize, usize, Vec<f32>)>) {
        // Previous volumes advance only for what was mixed this frame (CommandGenerator: PreviousVolume after the volume
        // ramp, VoiceChannelResource.UpdateState after the voice's mix, destination internal state when used).
        let mut mixed_voices = Vec::new();
        let mut mixed_resources = Vec::new();
        let mut mixed_destinations = Vec::new();
        for (vi, ci, mut out) in outputs {
            let v = self.voices[vi];
            let n = out.len();
            if n == 0 {
                continue;
            }
            // Burst detector (a user heard two loud right-ear bursts): a jump or peak no game sound should make.
            let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            let jump = out
                .windows(2)
                .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
            if peak > 32768.0 || !peak.is_finite() {
                let ch = self
                    .channels
                    .get(v.channel_ids[ci])
                    .copied()
                    .unwrap_or_default();
                let wb = v.wave_buffers[ch.wave_buffer_index];
                anomaly(format!(
                    "burst f{} voice {} ch {} fmt {} {} Hz pitch {:.2} vol {:.2} mix {} splitter {} biquads {:?} peak {:.0} jump {:.0} | wb {} addr {:#x} size {:#x} samples {}..{} loop {}..{} looping {} | offset {} loops {} hist {:?} adpcm param {:#x}",
                    self.elapsed_frames, vi, ci, v.format, v.sample_rate, v.pitch, v.volume, v.mix_id, v.splitter_id, v.biquads, peak, jump,
                    ch.wave_buffer_index, wb.address, wb.size, wb.start, wb.end, wb.loop_start, wb.loop_end, wb.looping, ch.offset, ch.loop_count, ch.hist, v.adpcm_param));
            }
            let (p, c) = (v.prev_volume, v.volume);
            for (k, s) in out.iter_mut().enumerate() {
                *s *= p + (c - p) * k as f32 / n as f32;
            }
            let bad = |a: &[f32; 24]| a.iter().any(|x| !x.is_finite() || x.abs() > 8.0);
            mixed_voices.push(vi);
            if v.mix_id != UNUSED_MIX_ID {
                mixed_resources.push(v.channel_ids[ci]);
                if let Some(r) = self.resources.get(v.channel_ids[ci]).copied() {
                    if bad(&r.mix) || bad(&r.prev) {
                        anomaly(format!(
                            "voice {} channel resource {} volumes out of range: {:?}",
                            vi, v.channel_ids[ci], r.mix
                        ));
                    }
                    self.add_to_mix(&out, v.mix_id, &r.prev, &r.mix);
                }
            } else if v.splitter_id >= 0 {
                let Some(list) = self.splitters.get(&v.splitter_id).cloned() else {
                    continue;
                };
                let mut k = ci;
                while k < list.len() {
                    mixed_destinations.push(list[k] as usize);
                    if let Some(d) = self.destinations.get(list[k] as usize).copied() {
                        if d.used && (bad(&d.volume) || bad(&d.prev)) {
                            anomaly(format!("voice {} splitter {} destination {} volumes out of range: mix {} {:?}", vi, v.splitter_id, list[k], d.mix_id, d.volume));
                        }
                        if d.used {
                            self.add_to_mix(&out, d.mix_id, &d.prev, &d.volume);
                        }
                    }
                    k += v.channel_count.max(1);
                }
            }
        }
        for vi in mixed_voices {
            self.voices[vi].prev_volume = self.voices[vi].volume;
        }
        for r in mixed_resources {
            if let Some(r) = self.resources.get_mut(r) {
                r.prev = r.mix;
            }
        }
        for d in mixed_destinations {
            if let Some(d) = self.destinations.get_mut(d) {
                d.prev = d.volume;
            }
        }
    }

    /// Runs the mix graph for one frame and sends the device sink's output to host_audio.
    fn mix_frame(&mut self) {
        if self.elapsed_frames % 2000 == 0 {
            let (trims, underruns) = crate::host::audio::stats();
            if trims + underruns > 0 {
                println!(
                    "[audio] output ring: {} trims, {} underruns so far",
                    trims, underruns
                );
            }
        }
        let n = self.sample_count as usize;
        if self.mixes.is_empty() || self.sink_inputs.is_empty() || n == 0 {
            return;
        }
        // Voices were mixed into the buffers prepare_mix_buffers laid out.
        let offsets = self.mix_offsets.clone();
        if offsets.len() != self.mixes.len() {
            return;
        }
        // Where a mix sends: its destination mix, else the mix of its splitter's first configured destination.
        let next_mix = |id: usize| -> i32 {
            let m = &self.mixes[id];
            if m.dest_mix >= 0 && (m.dest_mix as usize) < self.mixes.len() {
                return m.dest_mix;
            }
            if m.dest_splitter >= 0 {
                if let Some(list) = self.splitters.get(&m.dest_splitter) {
                    for &d in list {
                        if let Some(dst) = self.destinations.get(d as usize) {
                            if dst.used
                                && dst.mix_id >= 0
                                && (dst.mix_id as usize) < self.mixes.len()
                            {
                                return dst.mix_id;
                            }
                        }
                    }
                }
            }
            -1
        };
        let distance = |mut id: usize| {
            let mut d = 0;
            while id != 0 && d < 64 {
                let next = next_mix(id);
                if next < 0 {
                    return -1; // routed nowhere
                }
                id = next as usize;
                d += 1;
            }
            d
        };
        let mut order: Vec<(i32, usize)> = (1..self.mixes.len())
            .filter(|&m| self.mixes[m].used)
            .map(|m| (distance(m), m))
            .collect();
        order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for &(_, m) in &order {
            self.run_effects(m, offsets[m], n);
            let mix = &self.mixes[m];
            let dest = mix.dest_mix;
            if dest < 0 || dest as usize >= self.mixes.len() {
                // Through a splitter (Ryujinx CommandGenerator.GenerateSubMix): destination k takes the mix's buffer
                // k % buffer count, scaled by the mix volume and the destination's volume per destination buffer.
                // Super Mario Bros. Wonder routes most mixes this way; they were dropped (no dry sound, no music).
                if mix.dest_splitter < 0 || mix.buffer_count == 0 {
                    continue;
                }
                let Some(list) = self.splitters.get(&mix.dest_splitter).cloned() else {
                    continue;
                };
                let (src_off, count, volume) = (offsets[m], mix.buffer_count, mix.volume);
                for (k, &d) in list.iter().enumerate() {
                    let Some(dst) = self.destinations.get(d as usize).copied() else {
                        continue;
                    };
                    if !dst.used || dst.mix_id < 0 || dst.mix_id as usize >= self.mixes.len() {
                        continue;
                    }
                    let (dst_off, dst_count) = (
                        offsets[dst.mix_id as usize],
                        self.mixes[dst.mix_id as usize].buffer_count,
                    );
                    let sb = k % count;
                    for db in 0..dst_count.min(24) {
                        let vol = volume * dst.volume[db];
                        if vol == 0.0 {
                            continue;
                        }
                        for i in 0..n {
                            let v = self.mix_buffers[(src_off + sb) * n + i] * vol;
                            self.mix_buffers[(dst_off + db) * n + i] += v;
                        }
                    }
                }
                continue;
            }
            let (src_off, dst_off, dst_count) = (
                offsets[m],
                offsets[dest as usize],
                self.mixes[dest as usize].buffer_count,
            );
            for sb in 0..mix.buffer_count {
                for db in 0..dst_count {
                    let vol = mix.volume * mix.matrix[sb * 24 + db];
                    if vol == 0.0 {
                        continue;
                    }
                    for k in 0..n {
                        let v = self.mix_buffers[(src_off + sb) * n + k] * vol;
                        self.mix_buffers[(dst_off + db) * n + k] += v;
                    }
                }
            }
        }
        if self.mixes[0].used {
            self.run_effects(0, 0, n);
            let vol = self.mixes[0].volume;
            if vol != 1.0 {
                for v in &mut self.mix_buffers[..self.mixes[0].buffer_count * n] {
                    *v *= vol;
                }
            }
        }
        // Device sink: its inputs index the final mix's buffers. Samples are in 16-bit scale.
        let ch = |i: usize| -> &[f32] {
            let b = self
                .sink_inputs
                .get(i)
                .copied()
                .unwrap_or(0)
                .min(self.mixes[0].buffer_count.saturating_sub(1));
            &self.mix_buffers[b * n..(b + 1) * n]
        };
        let mut stereo = vec![0.0f32; n * 2];
        match self.sink_inputs.len() {
            6 => {
                let c = self.sink_downmix.unwrap_or([1.0, 0.707, 0.251, 0.707]);
                let (fl, fr, fc, lfe, bl, br) = (ch(0), ch(1), ch(2), ch(3), ch(4), ch(5));
                for k in 0..n {
                    stereo[2 * k] = c[3] * bl[k] + c[2] * lfe[k] + c[1] * fc[k] + c[0] * fl[k];
                    stereo[2 * k + 1] = c[3] * br[k] + c[2] * lfe[k] + c[1] * fc[k] + c[0] * fr[k];
                }
            }
            1 => {
                for (k, &v) in ch(0).iter().enumerate() {
                    stereo[2 * k] = v;
                    stereo[2 * k + 1] = v;
                }
            }
            _ => {
                let (l, r) = (ch(0), ch(1));
                for k in 0..n {
                    stereo[2 * k] = l[k];
                    stereo[2 * k + 1] = r[k];
                }
            }
        }
        // Output check: a valid mix never leaves 16-bit range. A user heard right-ear blasts that stayed loud at the
        // lowest system volume (only samples far beyond full scale do that): find where they come from.
        let worst = stereo.iter().fold(0.0f32, |m, v| {
            if v.is_finite() {
                m.max(v.abs())
            } else {
                f32::INFINITY
            }
        });
        if worst > 32768.0 {
            let mut peaks = Vec::new();
            for (m, mix) in self.mixes.iter().enumerate() {
                for b in 0..mix.buffer_count {
                    let buf = &self.mix_buffers[(offsets[m] + b) * n..(offsets[m] + b + 1) * n];
                    let pk = buf.iter().fold(0.0f32, |a, v| {
                        if v.is_finite() {
                            a.max(v.abs())
                        } else {
                            f32::INFINITY
                        }
                    });
                    if pk > 32768.0 {
                        peaks.push(format!("mix{}[{}]={:.0}", m, b, pk));
                    }
                }
            }
            let bad_vol: Vec<String> = self
                .mixes
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    m.used
                        && (!m.volume.is_finite()
                            || m.volume.abs() > 8.0
                            || m.matrix.iter().any(|v| !v.is_finite() || v.abs() > 8.0))
                })
                .map(|(i, m)| {
                    format!(
                        "mix{} vol {} matrix max {:?}",
                        i,
                        m.volume,
                        m.matrix.iter().fold(0.0f32, |a, v| a.max(v.abs()))
                    )
                })
                .collect();
            anomaly(format!("output beyond full scale: f{} peak {:.0} (sink L/R) | buffers beyond 16-bit: [{}] | suspicious mix volumes: [{}]",
                self.elapsed_frames, worst, peaks.join(" "), bad_vol.join("; ")));
        }
        // Safety: never hand the device anything beyond full scale (non-finite samples become silence).
        for v in &mut stereo {
            *v = if v.is_finite() {
                (*v / 32768.0).clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        crate::host::audio::push(&stereo);
    }

    /// The effects attached to mix `m`, in processing order.
    fn run_effects(&mut self, m: usize, off: usize, n: usize) {
        let mut fx: Vec<usize> = (0..self.effects.len())
            .filter(|&e| self.effects[e].mix_id == m as i32 && self.effects[e].kind != 0)
            .collect();
        fx.sort_by_key(|&e| self.effects[e].order);
        for e in fx {
            let effect = self.effects[e].clone();
            let count = self.mixes[m].buffer_count;
            if effect.kind == EFFECT_AUX {
                unsafe { self.aux(&effect, off, count, n) };
            } else if effect.kind == EFFECT_LIMITER && effect.enabled {
                self.limiter(e, &effect, off, count, n);
            } else if effect.kind == EFFECT_REVERB3D && effect.enabled {
                self.reverb3d(e, &effect, off, count, n);
            } else if effect.kind == EFFECT_DELAY && effect.enabled {
                self.delay(e, &effect, off, count, n);
            } else if effect.kind == EFFECT_BIQUAD {
                self.biquad_effect(e, &effect, off, count, n);
            } else {
                // Not implemented (Reverb3d, Limiter, ...): pass the input through. Input[6] at +0, Output[6] at +6,
                // channel count at +0xE for every effect with six-channel mappings.
                let chans =
                    (u16::from_le_bytes([effect.params[0xE], effect.params[0xF]]) as usize).min(6);
                for c in 0..chans {
                    let (i, o) = (effect.params[c] as usize, effect.params[6 + c] as usize);
                    if i != o && i < count && o < count {
                        self.mix_buffers
                            .copy_within((off + i) * n..(off + i + 1) * n, (off + o) * n);
                    }
                }
            }
        }
    }

    /// BiquadFilter effect (Ryujinx CommandGenerator.GenerateBiquadFilterEffect). BiquadFilterEffectParameter:
    /// Input[6] +0, Output[6] +6, Numerator[3] +0xC and Denominator[2] +0x12 in Q14, ChannelCount +0x16, Status +0x17
    /// (UsageState: 0 invalid, 1 new). Enabled: each channel is filtered from its input to its output buffer; disabled:
    /// copied. Super Mario Bros. Wonder routes most of its mixes through these: read with the other effects' channel count
    /// offset (+0xE, a coefficient here) they dropped the dry signal and the music, leaving only the reverb.
    fn biquad_effect(&mut self, slot: usize, e: &Effect, off: usize, count: usize, n: usize) {
        let p = &e.params;
        let chans = (p[0x16] as usize).min(6);
        if !e.enabled {
            for c in 0..chans {
                let (i, o) = (p[c] as usize, p[6 + c] as usize);
                if i != o && i < count && o < count {
                    self.mix_buffers
                        .copy_within((off + i) * n..(off + i + 1) * n, (off + o) * n);
                }
            }
            return;
        }
        let q = |o: usize| i16::from_le_bytes([p[o], p[o + 1]]) as f32 / 16384.0;
        let (a0, a1, a2, b1, b2) = (q(0xC), q(0xE), q(0x10), q(0x12), q(0x14));
        let state = self.biquad_fx.entry(slot).or_default();
        if state.len() != 6 || p[0x17] == 0 {
            *state = vec![[0.0; 2]; 6]; // needInitialization
        }
        let mut tmp = vec![0.0f32; n];
        for c in 0..chans {
            let (i, o) = (p[c] as usize, p[6 + c] as usize);
            if i >= count || o >= count {
                continue;
            }
            let st = &mut state[c];
            for (k, t) in tmp.iter_mut().enumerate() {
                let input = self.mix_buffers[(off + i) * n + k];
                let y = input * a0 + st[0];
                st[0] = input * a1 + y * b1 + st[1];
                st[1] = input * a2 + y * b2;
                *t = y;
            }
            self.mix_buffers[(off + o) * n..(off + o + 1) * n].copy_from_slice(&tmp);
        }
    }

    /// Reverb3dCommand (horizon/audio/fx.rs). From revision 11 effects take channel-resource order (FL FR FC LFE BL BR) while the
    /// reverb processes the legacy one (FL FR BL BR FC LFE): six-channel indices are remapped as Ryujinx does
    /// (DataSourceHelper.RemapChannelResourceMappingToLegacy).
    fn reverb3d(&mut self, slot: usize, e: &Effect, off: usize, count: usize, n: usize) {
        let p = &e.params;
        let chans = crate::horizon::audio::fx::Reverb3d::channel_count(p);
        if !matches!(chans, 1 | 2 | 4 | 6) {
            return;
        }
        let mut ins: Vec<usize> = (0..chans).map(|c| p[c] as usize).collect();
        let mut outs: Vec<usize> = (0..chans).map(|c| p[6 + c] as usize).collect();
        if self.revision >= 11 && chans == 6 {
            for v in [&mut ins, &mut outs] {
                let (fc, lfe, bl, br) = (v[2], v[3], v[4], v[5]);
                v[2] = bl;
                v[3] = br;
                v[4] = fc;
                v[5] = lfe;
            }
        }
        if ins.iter().chain(outs.iter()).any(|&i| i >= count) {
            return;
        }
        let st = match self.reverbs3d.get_mut(&slot) {
            Some(st) if st.sample_rate_matches(p) => {
                if !st.matches(p) {
                    st.update_parameter(p);
                }
                st
            }
            _ => self
                .reverbs3d
                .entry(slot)
                .insert_entry(crate::horizon::audio::fx::Reverb3d::new(p))
                .into_mut(),
        };
        let buf = |b: usize| self.mix_buffers[(off + b) * n..(off + b + 1) * n].to_vec();
        let inputs: Vec<Vec<f32>> = ins.iter().map(|&b| buf(b)).collect();
        let mut outputs: Vec<Vec<f32>> = outs.iter().map(|&b| buf(b)).collect();
        st.process(&inputs, &mut outputs, n);
        for (k, &b) in outs.iter().enumerate() {
            self.mix_buffers[(off + b) * n..(off + b + 1) * n].copy_from_slice(&outputs[k]);
        }
    }

    /// DelayCommand (horizon/audio/fx.rs). The delay runs in channel-resource order: before revision 11 six-channel indices are
    /// legacy (FL FR BL BR FC LFE) and get remapped (RemapLegacyChannelEffectMappingToChannelResourceMapping).
    fn delay(&mut self, slot: usize, e: &Effect, off: usize, count: usize, n: usize) {
        let p = &e.params;
        let chans = crate::horizon::audio::fx::Delay::channel_count(p);
        if !matches!(chans, 1 | 2 | 4 | 6) {
            return;
        }
        let mut ins: Vec<usize> = (0..chans).map(|c| p[c] as usize).collect();
        let mut outs: Vec<usize> = (0..chans).map(|c| p[6 + c] as usize).collect();
        if self.revision < 11 && chans == 6 {
            for v in [&mut ins, &mut outs] {
                let (bl, br, fc, lfe) = (v[2], v[3], v[4], v[5]);
                v[2] = fc;
                v[3] = lfe;
                v[4] = bl;
                v[5] = br;
            }
        }
        if ins.iter().chain(outs.iter()).any(|&i| i >= count) {
            return;
        }
        let st = match self.delays.get_mut(&slot) {
            Some(st) if st.fits(p) => {
                if !st.matches(p) {
                    st.update_parameter(p);
                }
                st
            }
            _ => self
                .delays
                .entry(slot)
                .insert_entry(crate::horizon::audio::fx::Delay::new(p))
                .into_mut(),
        };
        let buf = |b: usize| self.mix_buffers[(off + b) * n..(off + b + 1) * n].to_vec();
        let inputs: Vec<Vec<f32>> = ins.iter().map(|&b| buf(b)).collect();
        let mut outputs: Vec<Vec<f32>> = outs.iter().map(|&b| buf(b)).collect();
        st.process(p, &inputs, &mut outputs, n);
        for (k, &b) in outs.iter().enumerate() {
            self.mix_buffers[(off + b) * n..(off + b + 1) * n].copy_from_slice(&outputs[k]);
        }
    }

    /// LimiterCommandVersion1 (LimiterParameter: Input[6] +0, Output[6] +6, ChannelCount +0xE, AttackCoefficient +0x24,
    /// ReleaseCoefficient +0x28, Threshold +0x2C, InputGain +0x30, OutputGain +0x34, DelayBufferSampleCountMin +0x38,
    /// DelayBufferSampleCountMax +0x3C): a look-ahead peak limiter with exponential attack / release.
    fn limiter(&mut self, slot: usize, e: &Effect, off: usize, count: usize, n: usize) {
        let p = &e.params;
        let f = |o: usize| f32::from_bits(rd32(p, o));
        let chans = (u16::from_le_bytes([p[0xE], p[0xF]]) as usize).min(6);
        let (attack, release, threshold, in_gain, out_gain) =
            (f(0x24), f(0x28), f(0x2C), f(0x30), f(0x34));
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| {
            println!("[audio] limiter: channels {} attack {} release {} threshold {} input gain {} output gain {} delay {}..{} status {}",
                chans, attack, release, threshold, in_gain, out_gain, rd32(p, 0x38), rd32(p, 0x3C), p[0x40]);
        });
        let (dmin, dmax) = (rd32(p, 0x38) as usize, rd32(p, 0x3C) as usize);
        if chans == 0 || dmin == 0 || dmin > dmax || dmax > 48000 {
            return;
        }
        let st = self.limiters.entry(slot).or_default();
        if st.detector.len() != chans || st.delay.len() != chans * dmax {
            *st = LimiterState {
                detector: vec![0.0; chans],
                gain: vec![1.0; chans],
                delay: vec![0.0; chans * dmax],
                position: vec![0; chans],
            };
        }
        for c in 0..chans {
            let (i, o) = (p[c] as usize, p[6 + c] as usize);
            if i >= count || o >= count {
                continue;
            }
            for k in 0..n {
                let input = self.mix_buffers[(off + i) * n + k] / 32767.0 * in_gain;
                let level = input.abs();
                let a = if level > st.detector[c] {
                    attack
                } else {
                    release
                };
                st.detector[c] += a * (level - st.detector[c]);
                let attenuation = if st.detector[c] > threshold {
                    threshold / st.detector[c]
                } else {
                    1.0
                };
                let b = if st.gain[c] > attenuation {
                    attack
                } else {
                    release
                };
                st.gain[c] += b * (attenuation - st.gain[c]);
                let slot = &mut st.delay[c * dmax + st.position[c]];
                let out = *slot * st.gain[c] * out_gain;
                *slot = input;
                self.mix_buffers[(off + o) * n + k] = out * 32767.0;
                st.position[c] += 1;
                while st.position[c] >= dmin {
                    st.position[c] -= dmin;
                }
            }
        }
    }

    /// AuxiliaryBufferCommand for every channel of an Aux effect (AuxiliaryBufferParameter: Input[24] +0, Output[24]
    /// +0x18, ChannelCount +0x30, BufferStorageSize +0x38, SendBufferInfoAddress +0x40, ReturnBufferInfoAddress
    /// +0x50). Each info address heads a 0x80-byte AuxiliaryBufferHeader whose DSP half (+0x40) holds {read offset,
    /// write offset}; the int32 ring follows at +0x80. Channel c of a frame lives at c * sample_count past the offset.
    unsafe fn aux(&mut self, e: &Effect, off: usize, count: usize, n: usize) {
        let p = &e.params;
        let chans = (rd32(p, 0x30) as usize).min(24);
        let count_max = rd32(p, 0x38) as usize;
        let (send, ret) = (rd64(p, 0x40) as usize, rd64(p, 0x50) as usize);
        if send < 0x10000 || ret < 0x10000 {
            return;
        }
        let (send_info, ret_info) = ((send + 0x40) as *mut u32, (ret + 0x40) as *mut u32);
        let (send_ring, ret_ring) = ((send + 0x80) as *mut i32, (ret + 0x80) as *mut i32);
        if !e.enabled {
            // Reset both DSP read / write offsets; the input passes through.
            *(send_info as *mut u64) = 0;
            *(ret_info as *mut u64) = 0;
            for c in 0..chans {
                let (i, o) = (p[c] as usize, p[0x18 + c] as usize);
                if i != o && i < count && o < count {
                    self.mix_buffers
                        .copy_within((off + i) * n..(off + i + 1) * n, (off + o) * n);
                }
            }
            return;
        }
        if count_max == 0 {
            return;
        }
        let write_base = std::ptr::read_volatile(send_info.add(1)) as usize;
        let read_base = std::ptr::read_volatile(ret_info) as usize;
        // Return-ring health: samples the game has written ahead of our read position (its write offset is in the CPU half
        // of the header). Reading a frame it has not written yet plays stale samples.
        {
            let cpu_write = std::ptr::read_volatile((ret as *const u32).add(1)) as usize;
            let ahead = (cpu_write + count_max - read_base) % count_max;
            AUX_STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if ahead < chans * n {
                AUX_STATS[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        for c in 0..chans {
            let (i, o) = (p[c] as usize, p[0x18 + c] as usize);
            if i >= count || o >= count {
                continue;
            }
            let w = write_base + c * n;
            if w <= count_max {
                for k in 0..n {
                    let v = self.mix_buffers[(off + i) * n + k] as i32;
                    std::ptr::write_volatile(send_ring.add((w + k) % count_max), v);
                }
            }
            let r = read_base + c * n;
            let out = &mut self.mix_buffers[(off + o) * n..(off + o + 1) * n];
            if r <= count_max {
                let mut peak = 0i32;
                for (k, v) in out.iter_mut().enumerate() {
                    let x = std::ptr::read_volatile(ret_ring.add((r + k) % count_max));
                    peak = peak.max(x.saturating_abs());
                    *v = x as f32;
                }
                if peak > 32767 {
                    anomaly(format!("aux return out of 16-bit range: f{} mix {} channel {} peak {} (read offset {} of {})", self.elapsed_frames, e.mix_id, c, peak, r, count_max));
                }
            } else {
                out.fill(0.0);
            }
        }
        let advance = chans * n;
        std::ptr::write_volatile(
            send_info.add(1),
            ((write_base + advance) % count_max) as u32,
        );
        std::ptr::write_volatile(ret_info, ((read_base + advance) % count_max) as u32);
    }
}

/// Sample accounting of DataSourceHelper.ProcessWaveBuffers without decoding: consumes
/// `fraction + ratio * sample_count` source samples across the channel's valid wave buffers.
/// Where a channel's samples come from: the voice's sample format and channel layout, and for ADPCM its coefficients.
struct Source {
    format: u8,
    channels: usize,
    index: usize,
    adpcm_param: u64,
    quality: u8,
    skip_src: bool,
}

/// Decodes `count` samples starting at sample `first` of wave buffer `wb` (DataSourceHelper.Decode*), appending them
/// to `out` in 16-bit scale. PCM samples interleave the voice's channels; ADPCM (mono) is DSP-ADPCM: 8-byte frames of
/// a predictor / scale byte and 14 4-bit samples, predicted from the two previous outputs with the voice's coefficients.
unsafe fn decode_segment(
    ch: &mut ChannelState,
    wb: &WaveBuffer,
    src: &Source,
    first: i32,
    count: i32,
    out: &mut Vec<f32>,
) {
    let (addr, size) = (wb.address as usize, wb.size as usize);
    let valid = addr >= 0x10000 && size > 0;
    match src.format {
        SAMPLE_FORMAT_PCM16 | SAMPLE_FORMAT_FLOAT => {
            let width = if src.format == SAMPLE_FORMAT_PCM16 {
                2
            } else {
                4
            };
            for k in 0..count {
                let at = ((first + k) as usize * src.channels.max(1) + src.index) * width;
                let v = if !valid || at + width > size {
                    0.0
                } else if width == 2 {
                    std::ptr::read_unaligned((addr + at) as *const i16) as f32
                } else {
                    std::ptr::read_unaligned((addr + at) as *const f32) * 32768.0
                };
                out.push(v);
            }
        }
        SAMPLE_FORMAT_ADPCM => {
            let coef: [i32; 16] = if src.adpcm_param >= 0x10000 {
                std::array::from_fn(|i| {
                    std::ptr::read_unaligned((src.adpcm_param as usize + i * 2) as *const i16)
                        as i32
                })
            } else {
                [0; 16]
            };
            for k in 0..count {
                let s = (first + k) as usize;
                let (frame, idx) = (s / 14, s % 14);
                if !valid || frame * 8 + 8 > size {
                    out.push(0.0);
                    continue;
                }
                let header = *((addr + frame * 8) as *const u8);
                let (scale, pred) = (1i32 << (header & 0xF), ((header >> 4) & 7) as usize);
                let byte = *((addr + frame * 8 + 1 + idx / 2) as *const u8);
                let mut nib = if idx % 2 == 0 {
                    (byte >> 4) as i32
                } else {
                    (byte & 0xF) as i32
                };
                if nib >= 8 {
                    nib -= 16;
                }
                let v = (((nib * scale) << 11)
                    + 1024
                    + coef[pred * 2] * ch.hist[0]
                    + coef[pred * 2 + 1] * ch.hist[1])
                    >> 11;
                let v = v.clamp(-32768, 32767);
                ch.hist = [v, ch.hist[0]];
                out.push(v as f32);
            }
        }
        _ => out.extend(std::iter::repeat_n(0.0, count.max(0) as usize)),
    }
}

/// DataSourceHelper.ProcessWaveBuffers: consumes `fraction + ratio * sample_count` source samples across the channel's
/// valid wave buffers (the accounting the game observes), decodes them, and resamples them into `out` (sample_count
/// samples) with the voice's sample-rate conversion quality.
fn process_wave_buffers(
    ch: &mut ChannelState,
    bufs: &[WaveBuffer; WAVE_BUFFER_COUNT],
    ratio: f32,
    sample_count: i32,
    reset_on_loop: bool,
    src: &Source,
    out: &mut [f32],
) {
    let frac0 = ch.fraction;
    let needed_f = ch.fraction + ratio * sample_count as f32;
    let needed = needed_f.trunc() as i32;
    ch.fraction = needed_f - needed as f32;
    // The resampler's input: its taps of history (DataSourceHelper: Pitch state, 4 or 8 samples), then this frame's
    // decoded samples; without sample-rate conversion only the decoded samples.
    let taps = if src.skip_src {
        0
    } else if src.quality == 1 {
        8
    } else {
        4
    };
    let mut decoded_samples: Vec<f32> = Vec::with_capacity(needed.max(0) as usize + taps);
    decoded_samples.extend_from_slice(&ch.history[8 - taps..]);
    let mut y = 0;
    while y < needed {
        if !ch.valid[ch.wave_buffer_index] {
            // Starving: plays silence until the game appends more data. Counted when it happens after the voice played
            // something (a voice that ran to its end is not starving).
            if ch.played_sample_count > 0 {
                STARVED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            break;
        }
        let idx = ch.wave_buffer_index;
        let wb = bufs[idx];
        let (start, end) = if ch.loop_count > 0
            && wb.loop_start != 0
            && wb.loop_end != 0
            && wb.loop_start <= wb.loop_end
        {
            (wb.loop_start as i32, wb.loop_end as i32)
        } else {
            (wb.start as i32, wb.end as i32)
        };
        let total = (end - start).max(0);
        let decoded = (total - ch.offset).clamp(0, needed - y);
        // Every wave buffer with an ADPCM context starts from it (DataSourceHelper: offset 0 and a context address).
        if ch.offset == 0 && wb.context >= 0x10000 && src.format == SAMPLE_FORMAT_ADPCM {
            unsafe {
                let c = wb.context as usize;
                ch.hist = [
                    std::ptr::read_unaligned((c + 2) as *const i16) as i32,
                    std::ptr::read_unaligned((c + 4) as *const i16) as i32,
                ];
            }
        }
        unsafe {
            decode_segment(
                ch,
                &wb,
                src,
                start + ch.offset,
                decoded,
                &mut decoded_samples,
            )
        };
        y += decoded;
        ch.offset += decoded;
        ch.played_sample_count += decoded as u64;
        if ch.offset >= total || decoded == 0 {
            ch.offset = 0;
            if wb.looping {
                ch.loop_count += 1;
                if wb.loop_count >= 0 && (decoded == 0 || ch.loop_count > wb.loop_count) {
                    mark_end_of_buffer(ch, &wb);
                }
                if decoded == 0 {
                    break;
                }
                if reset_on_loop {
                    ch.played_sample_count = 0;
                }
            } else {
                mark_end_of_buffer(ch, &wb);
            }
        }
    }
    // Starving (no data queued): silence for the rest.
    decoded_samples.resize(needed.max(0) as usize + taps, 0.0);
    if taps == 0 {
        for (k, o) in out.iter_mut().enumerate() {
            *o = decoded_samples.get(k).copied().unwrap_or(0.0);
        }
        return;
    }
    ch.fraction = resample(out, &decoded_samples, ratio, frac0, src.quality);
    let tail = &decoded_samples[decoded_samples.len() - taps..];
    ch.history[8 - taps..].copy_from_slice(tail);
}

/// ResamplerHelper.Resample: polyphase filters (default 4 taps / high 8 taps, phase = fraction * 128, tables chosen by
/// ratio) or nearest sample (low). `input` starts with the taps of history. Returns the new fraction.
fn resample(out: &mut [f32], input: &[f32], ratio: f32, mut fraction: f32, quality: u8) -> f32 {
    use crate::horizon::audio::src_lut as lut;
    let mut idx = 0usize;
    let at = |i: usize| input.get(i).copied().unwrap_or(0.0);
    match quality {
        2 => {
            for o in out.iter_mut() {
                *o = if fraction > 1.0 { at(idx + 1) } else { at(idx) };
                fraction += ratio;
                idx += fraction.trunc() as usize;
                fraction -= fraction.trunc();
            }
        }
        1 => {
            let table: &[i16] = if ratio <= 1.0 {
                &lut::HIGHCURVELUT1
            } else if ratio > 1.333313 {
                &lut::HIGHCURVELUT0
            } else {
                &lut::HIGHCURVELUT2
            };
            for o in out.iter_mut() {
                let base = (fraction * 128.0) as usize * 8;
                let mut acc = 0.0f32;
                for k in 0..8 {
                    acc +=
                        at(idx + k) * (table.get(base + k).copied().unwrap_or(0) as f32 / 32768.0);
                }
                *o = acc.round();
                fraction += ratio;
                idx += fraction.trunc() as usize;
                fraction -= fraction.trunc();
            }
        }
        _ => {
            let table: &[i16] = if ratio <= 1.0 {
                &lut::NORMALCURVELUT1
            } else if ratio > 1.333313 {
                &lut::NORMALCURVELUT0
            } else {
                &lut::NORMALCURVELUT2
            };
            for o in out.iter_mut() {
                let base = (fraction * 128.0) as usize * 4;
                let mut acc = 0.0f32;
                for k in 0..4 {
                    acc +=
                        at(idx + k) * (table.get(base + k).copied().unwrap_or(0) as f32 / 32768.0);
                }
                *o = acc.round();
                fraction += ratio;
                idx += fraction.trunc() as usize;
                fraction -= fraction.trunc();
            }
        }
    }
    fraction
}

fn mark_end_of_buffer(ch: &mut ChannelState, wb: &WaveBuffer) {
    ch.valid[ch.wave_buffer_index] = false;
    ch.loop_count = 0;
    ch.wave_buffer_consumed += 1;
    ch.wave_buffer_index = (ch.wave_buffer_index + 1) % WAVE_BUFFER_COUNT;
    if wb.end_of_stream {
        ch.played_sample_count = 0;
    }
}
