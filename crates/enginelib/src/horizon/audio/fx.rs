// Audio renderer effects that need their own DSP state (horizon/audio/renderer.rs runs them per mix): I3DL2 reverb (Reverb3d). A port
// of Ryujinx.Audio's Reverb3dCommand / Reverb3dState / DelayLine3d / DecayDelay (same constants, same order of
// operations), which follow Nintendo's DSP implementation.

/// Reverb3dParameter field offsets (in the effect's 0xA0 parameter bytes).
mod p3d {
    pub const CHANNEL_COUNT: usize = 0xE;
    pub const SAMPLE_RATE: usize = 0x14;
    pub const ROOM_HF: usize = 0x18;
    pub const HF_REFERENCE: usize = 0x1C;
    pub const DECAY_TIME: usize = 0x20;
    pub const HF_DECAY_RATIO: usize = 0x24;
    pub const ROOM_GAIN: usize = 0x28;
    pub const REFLECTIONS_GAIN: usize = 0x2C;
    pub const REVERB_GAIN: usize = 0x30;
    pub const DIFFUSION: usize = 0x34;
    pub const REFLECTION_DELAY: usize = 0x38;
    pub const REVERB_DELAY_TIME: usize = 0x3C;
    pub const DENSITY: usize = 0x40;
    pub const DRY_GAIN: usize = 0x44;
    /// Bytes that define the effect (everything before ParameterStatus).
    pub const DEFINING: usize = 0x48;
}

fn f32_at(p: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(p[o..o + 4].try_into().unwrap())
}

/// FloatingPointHelper.Pow10 (Nintendo's table supports x in (-5.3, 0)).
fn pow10(x: f32) -> f32 {
    if x >= 0.0 {
        1.0
    } else if x <= -5.3 {
        0.0
    } else {
        10f32.powf(x)
    }
}

fn cos_deg(x: f32) -> f32 {
    x.to_radians().cos()
}

fn sin_deg(x: f32) -> f32 {
    x.to_radians().sin()
}

/// IDelayLine.GetSampleCount: sample rate in kHz times a delay in ms.
fn sample_count(rate_khz: u32, ms: f32) -> usize {
    (rate_khz as f32 * ms).round().max(0.0) as usize
}

/// DelayLine3d.
#[derive(Clone)]
struct DelayLine3d {
    buf: Vec<f32>,
    max: usize,
    cur: usize,
    last: usize,
    count: usize,
}

impl DelayLine3d {
    fn new(rate_khz: u32, max_ms: f32) -> Self {
        let max = sample_count(rate_khz, max_ms);
        let mut d = DelayLine3d {
            buf: vec![0.0; max + 1],
            max,
            cur: 0,
            last: 0,
            count: 0,
        };
        d.set_delay(rate_khz, max_ms);
        d
    }

    fn set_delay(&mut self, rate_khz: u32, ms: f32) {
        let target = sample_count(rate_khz, ms);
        if self.max >= target {
            self.count = target;
            self.last = (self.cur + target) % (self.max + 1);
        }
    }

    fn read(&self) -> f32 {
        self.buf[self.cur]
    }

    fn update(&mut self, v: f32) -> f32 {
        self.buf[self.last] = v;
        self.last += 1;
        let out = self.read();
        self.cur += 1;
        if self.cur >= self.max {
            self.cur = 0;
        }
        if self.last >= self.max {
            self.last = 0;
        }
        out
    }

    /// TapUnsafe(sampleIndex, offset): `index + offset` samples behind the write position.
    fn tap(&self, index: usize, offset: i64) -> f32 {
        let mut t = self.last as i64 - (index as i64 + offset);
        if t < 0 {
            t += self.max as i64 + 1;
        }
        self.buf.get(t as usize).copied().unwrap_or(0.0)
    }
}

/// DecayDelay: an all-pass section around a delay line.
#[derive(Clone)]
struct DecayDelay {
    line: DelayLine3d,
    rate: f32,
}

impl DecayDelay {
    fn update(&mut self, v: f32) -> f32 {
        let d = self.line.read();
        let p = v - self.rate * d;
        self.line.update(p) + p * self.rate
    }
}

const FDN_MIN: [f32; 4] = [5.0, 6.0, 13.0, 14.0];
const FDN_MAX: [f32; 4] = [45.704, 82.782, 149.94, 271.58];
const DECAY_MAX_1: [f32; 4] = [17.0, 13.0, 9.0, 7.0];
const DECAY_MAX_2: [f32; 4] = [19.0, 11.0, 10.0, 6.0];
const EARLY_DELAY_TIMES: [f32; 20] = [
    0.017136, 0.059154, 0.16173, 0.39019, 0.42526, 0.45541, 0.68974, 0.74591, 0.83384, 0.8595, 0.0,
    0.075024, 0.16879, 0.2999, 0.33744, 0.3719, 0.59901, 0.71674, 0.81786, 0.85166,
];
const EARLY_GAIN: [f32; 20] = [
    0.67096, 0.61027, 1.0, 0.35680, 0.68361, 0.65978, 0.51939, 0.24712, 0.45945, 0.45021, 0.64196,
    0.54879, 0.92925, 0.38270, 0.72867, 0.69794, 0.5464, 0.24563, 0.45214, 0.44042,
];

/// Reverb3dState plus the parameter bytes it was built from.
#[derive(Clone)]
pub struct Reverb3d {
    defining: Vec<u8>,
    rate_khz: u32,
    fdn: [DelayLine3d; 4],
    decay1: [DecayDelay; 4],
    decay2: [DecayDelay; 4],
    pre_delay: DelayLine3d,
    front_center: DelayLine3d,
    dry_gain: f32,
    early_delay: [usize; 20],
    prev_pre_delay_value: f32,
    prev_pre_delay_gain: f32,
    target_pre_delay_gain: f32,
    early_gain: f32,
    late_gain: f32,
    reflection_delay: usize,
    direct_fdn_gain: [f32; 4],
    current_fdn_gain: [f32; 4],
    current_output_gain: [f32; 4],
    prev_feedback: [f32; 4],
}

impl Reverb3d {
    pub fn new(params: &[u8]) -> Self {
        let rate_khz = u32::from_le_bytes(
            params[p3d::SAMPLE_RATE..p3d::SAMPLE_RATE + 4]
                .try_into()
                .unwrap(),
        ) / 1000;
        let line = |ms: f32| DelayLine3d::new(rate_khz, ms);
        let mut r = Reverb3d {
            defining: Vec::new(),
            rate_khz,
            fdn: std::array::from_fn(|i| line(FDN_MAX[i])),
            decay1: std::array::from_fn(|i| DecayDelay {
                line: line(DECAY_MAX_1[i]),
                rate: 0.0,
            }),
            decay2: std::array::from_fn(|i| DecayDelay {
                line: line(DECAY_MAX_2[i]),
                rate: 0.0,
            }),
            pre_delay: line(400.0),
            front_center: line(5.0),
            dry_gain: 0.0,
            early_delay: [0; 20],
            prev_pre_delay_value: 0.0,
            prev_pre_delay_gain: 0.0,
            target_pre_delay_gain: 1.0,
            early_gain: 0.0,
            late_gain: 0.0,
            reflection_delay: 0,
            direct_fdn_gain: [0.0; 4],
            current_fdn_gain: [0.0; 4],
            current_output_gain: [0.0; 4],
            prev_feedback: [0.0; 4],
        };
        r.update_parameter(params);
        r
    }

    /// Whether `params` describe the effect this state was built for (else: update_parameter, or a new state when the
    /// sample rate changed).
    pub fn matches(&self, params: &[u8]) -> bool {
        self.defining == params[..p3d::DEFINING]
    }

    pub fn sample_rate_matches(&self, params: &[u8]) -> bool {
        u32::from_le_bytes(
            params[p3d::SAMPLE_RATE..p3d::SAMPLE_RATE + 4]
                .try_into()
                .unwrap(),
        ) / 1000
            == self.rate_khz
    }

    /// Reverb3dState.UpdateParameter.
    pub fn update_parameter(&mut self, p: &[u8]) {
        self.defining = p[..p3d::DEFINING].to_vec();
        let f = |o: usize| f32_at(p, o);
        let sample_rate = (self.rate_khz * 1000) as f32;
        self.dry_gain = f(p3d::DRY_GAIN);
        self.prev_feedback = [0.0; 4];
        self.prev_pre_delay_value = 0.0;
        self.early_gain =
            pow10((f(p3d::ROOM_GAIN) + f(p3d::REFLECTIONS_GAIN)).min(5000.0) / 2000.0);
        self.late_gain = pow10((f(p3d::ROOM_GAIN) + f(p3d::REVERB_GAIN)).min(5000.0) / 2000.0);
        let hf_room_gain = pow10(f(p3d::ROOM_HF) / 2000.0);
        if hf_room_gain < 1.0 {
            let a = 1.0 - hf_room_gain;
            let b =
                2.0 - (2.0 * hf_room_gain) * cos_deg(256.0 * f(p3d::HF_REFERENCE) / sample_rate);
            let c = (b * b - 4.0 * (1.0 - hf_room_gain) * (1.0 - hf_room_gain)).sqrt();
            self.prev_pre_delay_gain = (b - c) / (2.0 * a);
            self.target_pre_delay_gain = 1.0 - self.prev_pre_delay_gain;
        } else {
            self.prev_pre_delay_gain = 0.0;
            self.target_pre_delay_gain = 1.0;
        }
        self.reflection_delay = sample_count(
            self.rate_khz,
            1000.0 * (f(p3d::REFLECTION_DELAY) + f(p3d::REVERB_DELAY_TIME)),
        );
        let echo_decay = 0.6 * f(p3d::DIFFUSION) * 0.01;
        for i in 0..4 {
            let ms = FDN_MIN[i] + f(p3d::DENSITY) / 100.0 * (FDN_MAX[i] - FDN_MIN[i]);
            self.fdn[i].set_delay(self.rate_khz, ms);
            let total =
                (self.fdn[i].count + self.decay1[i].line.count + self.decay2[i].line.count) as f32;
            let a = (-60.0 * total) / (f(p3d::DECAY_TIME) * sample_rate);
            let b = a / f(p3d::HF_DECAY_RATIO);
            let c = cos_deg(128.0 * 0.5 * f(p3d::HF_REFERENCE) / sample_rate)
                / sin_deg(128.0 * 0.5 * f(p3d::HF_REFERENCE) / sample_rate);
            let d = pow10((b - a) / 40.0);
            let e = pow10((b + a) / 40.0) * 0.7071;
            self.direct_fdn_gain[i] = e * ((d * c) + 1.0) / (c + d);
            self.current_fdn_gain[i] = e * (1.0 - (d * c)) / (c + d);
            self.current_output_gain[i] = (c - d) / (c + d);
            self.decay1[i].rate = echo_decay;
            self.decay2[i].rate = echo_decay * -0.9;
        }
        for i in 0..20 {
            let ms = f(p3d::REFLECTION_DELAY) * 1000.0
                + EARLY_DELAY_TIMES[i] * 1000.0 * (f(p3d::REVERB_DELAY_TIME) * 0.9998 + 0.02);
            self.early_delay[i] = sample_count(self.rate_khz, ms).min(self.pre_delay.max);
        }
    }

    /// Reverb3dCommand.ProcessReverb3dGeneric over `n` samples: `inputs` / `outputs` are channel buffers (legacy channel
    /// order for 6 channels: FL FR BL BR FC LFE).
    pub fn process(&mut self, inputs: &[Vec<f32>], outputs: &mut [Vec<f32>], n: usize) {
        let chans = inputs.len();
        const MONO: ([usize; 20], &[usize], &[i32]) = (
            [0; 20],
            &[
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            ],
            &[0],
        );
        const STEREO: ([usize; 20], &[usize], &[i32]) = (
            [0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1],
            &[
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            ],
            &[0, 1],
        );
        const QUAD: ([usize; 20], &[usize], &[i32]) = (
            [0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 0, 0, 0, 0, 3, 3, 3],
            &[
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            ],
            &[0, 1, 2, 3],
        );
        // Surround: the early table has 40 entries indexed by early delay; the target table walks them pairwise.
        const SURROUND_EARLY: [usize; 40] = [
            4, 5, 0, 5, 0, 5, 1, 5, 1, 5, 1, 5, 1, 5, 2, 5, 2, 5, 2, 5, 1, 5, 1, 5, 1, 5, 0, 5, 0,
            5, 0, 5, 0, 5, 3, 5, 3, 5, 3, 5,
        ];
        const SURROUND_TARGET: [usize; 40] = [
            0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
            13, 14, 14, 15, 15, 16, 16, 17, 17, 18, 18, 19, 19,
        ];
        const SURROUND_FEEDBACK: [i32; 6] = [0, 1, 2, 3, -1, 3];
        let (early_out, early_target, feedback_target): (&[usize], &[usize], &[i32]) = match chans {
            1 => (&MONO.0, MONO.1, MONO.2),
            2 => (&STEREO.0, STEREO.1, STEREO.2),
            4 => (&QUAD.0, QUAD.1, QUAD.2),
            6 => (&SURROUND_EARLY, &SURROUND_TARGET, &SURROUND_FEEDBACK),
            _ => return,
        };
        let mut channel_input = [0.0f32; 6];
        for s in 0..n {
            let mut outv = [0.0f32; 6];
            let tap_out = self.pre_delay.tap(self.reflection_delay, 1);
            for &early in early_target {
                // Ryujinx: outputIndex = outputEarlyIndicesTable[earlyDelayIndex] (the early-delay index, not the
                // position in the target table).
                let out_index = early_out[early];
                outv[out_index] +=
                    self.pre_delay.tap(self.early_delay[early], 1) * EARLY_GAIN[early];
            }
            let mut target_pre_delay = 0.0;
            for c in 0..chans {
                channel_input[c] = inputs[c][s];
                target_pre_delay += channel_input[c];
            }
            for v in outv.iter_mut().take(chans) {
                *v *= self.early_gain;
            }
            self.prev_pre_delay_value = target_pre_delay * self.target_pre_delay_gain
                + self.prev_pre_delay_value * self.prev_pre_delay_gain;
            self.pre_delay.update(self.prev_pre_delay_value);
            let mut fb_out = [0.0f32; 4];
            for i in 0..4 {
                let fdn = self.fdn[i].read();
                let o = fdn * self.direct_fdn_gain[i] + self.prev_feedback[i];
                self.prev_feedback[i] =
                    fdn * self.current_fdn_gain[i] + o * self.current_output_gain[i];
                fb_out[i] = o;
            }
            let fb = [
                fb_out[2] + fb_out[1],
                -fb_out[0] - fb_out[3],
                fb_out[0] - fb_out[3],
                fb_out[1] - fb_out[2],
            ];
            let mut values = [0.0f32; 4];
            for i in 0..4 {
                let t = self.decay1[i].update(tap_out * self.late_gain + fb[i]);
                values[i] = self.decay2[i].update(t);
                self.fdn[i].update(values[i]);
            }
            for (c, &t) in feedback_target.iter().enumerate() {
                if t >= 0 && c < chans {
                    outputs[c][s] = outv[c] + values[t as usize] + channel_input[c] * self.dry_gain;
                }
            }
            if chans == 1 {
                outputs[0][s] += values[1];
            }
            if chans == 6 {
                outputs[4][s] += outv[4]
                    + self.front_center.update((values[2] - values[3]) * 0.5)
                    + channel_input[4] * self.dry_gain;
            }
        }
    }

    pub fn channel_count(params: &[u8]) -> usize {
        u16::from_le_bytes([params[p3d::CHANNEL_COUNT], params[p3d::CHANNEL_COUNT + 1]]) as usize
    }
}

/// DelayParameter offsets: Input[6] 0, Output[6] 6, ChannelCount 0xE, DelayTimeMax 0x10, DelayTime 0x14, SampleRate
/// 0x18 (Q14), InGain 0x1C, FeedbackGain 0x20, OutGain 0x24, DryGain 0x28, ChannelSpread 0x2C, LowPassAmount 0x30 (all
/// Q14), Status 0x34.
mod pdl {
    pub const CHANNEL_COUNT: usize = 0xE;
    pub const DELAY_TIME_MAX: usize = 0x10;
    pub const DELAY_TIME: usize = 0x14;
    pub const SAMPLE_RATE: usize = 0x18;
    pub const IN_GAIN: usize = 0x1C;
    pub const FEEDBACK_GAIN: usize = 0x20;
    pub const OUT_GAIN: usize = 0x24;
    pub const DRY_GAIN: usize = 0x28;
    pub const CHANNEL_SPREAD: usize = 0x2C;
    pub const LOW_PASS_AMOUNT: usize = 0x30;
    pub const DEFINING: usize = 0x34;
}

fn u32_at(p: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(p[o..o + 4].try_into().unwrap())
}

fn q14(p: &[u8], o: usize) -> f32 {
    u32_at(p, o) as i32 as f32 / 16384.0
}

/// Ryujinx DelayLine (the delay effect's; not DelayLine3d).
#[derive(Clone)]
struct DelayLine {
    buf: Vec<f32>,
    cur: usize,
    last: usize,
}

impl DelayLine {
    fn new(rate_khz: u32, max_ms: f32, ms: f32) -> Self {
        let max = sample_count(rate_khz, max_ms);
        let mut d = DelayLine {
            buf: vec![0.0; max + 1],
            cur: 0,
            last: 0,
        };
        let count = sample_count(rate_khz, ms).min(max);
        d.cur = 0;
        d.last = count.saturating_sub(1);
        d
    }

    fn read(&self) -> f32 {
        self.buf[self.cur]
    }

    fn update(&mut self, v: f32) -> f32 {
        let out = self.read();
        self.buf[self.cur] = v;
        self.cur += 1;
        if self.cur >= self.last {
            self.cur = 0;
        }
        out
    }
}

/// DelayState + DelayCommand.
#[derive(Clone)]
pub struct Delay {
    defining: Vec<u8>,
    shape: (u32, u32, u32, usize),
    lines: Vec<DelayLine>,
    low_pass_z: Vec<f32>,
    feedback_base: f32,
    feedback_cross: f32,
    low_pass_feedback: f32,
    low_pass_base: f32,
}

impl Delay {
    pub fn channel_count(p: &[u8]) -> usize {
        u16::from_le_bytes([p[pdl::CHANNEL_COUNT], p[pdl::CHANNEL_COUNT + 1]]) as usize
    }

    fn shape(p: &[u8]) -> (u32, u32, u32, usize) {
        (
            u32_at(p, pdl::SAMPLE_RATE),
            u32_at(p, pdl::DELAY_TIME_MAX),
            u32_at(p, pdl::DELAY_TIME),
            Self::channel_count(p),
        )
    }

    pub fn new(p: &[u8]) -> Self {
        let chans = Self::channel_count(p).min(6);
        let rate_khz = (u32_at(p, pdl::SAMPLE_RATE) >> 14) / 1000;
        let (max_ms, ms) = (
            u32_at(p, pdl::DELAY_TIME_MAX) as f32,
            u32_at(p, pdl::DELAY_TIME) as f32,
        );
        let mut d = Delay {
            defining: Vec::new(),
            shape: Self::shape(p),
            lines: (0..chans)
                .map(|_| DelayLine::new(rate_khz, max_ms, ms))
                .collect(),
            low_pass_z: vec![0.0; chans],
            feedback_base: 0.0,
            feedback_cross: 0.0,
            low_pass_feedback: 0.0,
            low_pass_base: 1.0,
        };
        d.update_parameter(p);
        d
    }

    /// Whether `p` still fits this state (sample rate, delay times and channels): else a new one is needed.
    pub fn fits(&self, p: &[u8]) -> bool {
        self.shape == Self::shape(p)
    }

    pub fn matches(&self, p: &[u8]) -> bool {
        self.defining == p[..pdl::DEFINING]
    }

    /// DelayState.UpdateParameter.
    pub fn update_parameter(&mut self, p: &[u8]) {
        self.defining = p[..pdl::DEFINING].to_vec();
        let feedback = q14(p, pdl::FEEDBACK_GAIN) * 0.98;
        let spread = q14(p, pdl::CHANNEL_SPREAD);
        self.feedback_base = (1.0 - spread) * feedback;
        let chans = Self::channel_count(p);
        self.feedback_cross = if chans == 4 || chans == 6 {
            spread * 0.5 * feedback
        } else {
            spread * feedback
        };
        self.low_pass_feedback = 0.95 * q14(p, pdl::LOW_PASS_AMOUNT);
        self.low_pass_base = 1.0 - self.low_pass_feedback;
    }

    /// DelayCommand.ProcessDelay{Mono,Stereo,Quadraphonic,Surround} over `n` samples (channel-resource order).
    pub fn process(&mut self, p: &[u8], inputs: &[Vec<f32>], outputs: &mut [Vec<f32>], n: usize) {
        let chans = inputs.len().min(self.lines.len());
        let (b, c) = (self.feedback_base, self.feedback_cross);
        let raw_feedback = q14(p, pdl::FEEDBACK_GAIN);
        let m: Vec<[f32; 6]> = match chans {
            1 => vec![[raw_feedback, 0.0, 0.0, 0.0, 0.0, 0.0]],
            2 => vec![[b, c, 0.0, 0.0, 0.0, 0.0], [c, b, 0.0, 0.0, 0.0, 0.0]],
            4 => vec![
                [b, c, c, 0.0, 0.0, 0.0],
                [c, b, 0.0, c, 0.0, 0.0],
                [c, 0.0, b, c, 0.0, 0.0],
                [0.0, c, c, b, 0.0, 0.0],
            ],
            6 => vec![
                [b, 0.0, c, 0.0, c, 0.0],
                [0.0, b, c, 0.0, 0.0, c],
                [c, c, b, 0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, raw_feedback, 0.0, 0.0],
                [c, 0.0, 0.0, 0.0, b, c],
                [0.0, c, 0.0, 0.0, c, b],
            ],
            _ => return,
        };
        let (in_gain, dry, out_gain) = (
            q14(p, pdl::IN_GAIN),
            q14(p, pdl::DRY_GAIN),
            q14(p, pdl::OUT_GAIN),
        );
        for s in 0..n {
            let mut input = [0.0f32; 6];
            let mut line = [0.0f32; 6];
            for ch in 0..chans {
                input[ch] = inputs[ch][s] * 64.0;
                line[ch] = self.lines[ch].read();
            }
            for ch in 0..chans {
                let mut t = input[ch] * in_gain;
                for k in 0..chans {
                    t += m[ch][k] * line[k];
                }
                // UpdateLowPassFilter
                let lp = self.low_pass_feedback * self.low_pass_z[ch] + t * self.low_pass_base;
                self.low_pass_z[ch] = lp;
                self.lines[ch].update(lp);
                outputs[ch][s] = (input[ch] * dry + line[ch] * out_gain) / 64.0;
            }
        }
    }
}
