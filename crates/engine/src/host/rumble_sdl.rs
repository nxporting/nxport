// Rumble on Windows: the game's HD Rumble values (nn::hid::SendVibrationValue[s], HLE'd in horizon/hle.rs) drive the
// controllers' motors through SDL (host/display_sdl.rs polls controllers on the main thread).
//
// As on macOS (host/rumble.rs), each Switch motor (handle 1 = left, 2 = right) becomes one intensity: the larger band
// amplitude, scaled by --rumble-scale. SDL controllers have a heavy low-frequency motor on the left and a light
// high-frequency one on the right (Xbox, DualShock, DualSense; SDL emulates the pair on Switch controllers), so the Switch's
// left motor drives the low-frequency one and its right motor the other, as Core Haptics' left and right handle
// engines do. --no-rumble disables it.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Per motor (0 = left, 1 = right): intensity in 0..1 (f32 bits).
static LEVEL: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !crate::settings::get().input.no_rumble)
}

fn scale() -> f32 {
    static S: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *S.get_or_init(|| crate::settings::get().input.rumble_scale)
}

/// nn::hid::SendVibrationValue: `handle` as GetVibrationDeviceHandles returned it, `v` = amplitudeLow, frequencyLow,
/// amplitudeHigh, frequencyHigh.
pub fn set(handle: u32, v: [f32; 4]) {
    if !enabled() {
        return;
    }
    let motor = if handle == 2 { 1 } else { 0 };
    let (lo, hi) = (v[0].clamp(0.0, 1.0), v[2].clamp(0.0, 1.0));
    let level = (lo.max(hi) * scale()).clamp(0.0, 1.0);
    LEVEL[motor].store(level.to_bits(), Ordering::Relaxed);
}

/// An effect lasts this long unless renewed: rumble stops by itself if the process stops sending (a hang, a crash).
const EFFECT: Duration = Duration::from_millis(1000);
/// A steady level is sent again this often, well within EFFECT.
const RENEW: Duration = Duration::from_millis(400);

/// What one controller was last sent.
pub struct Rumbler {
    sent: (u16, u16),
    at: Option<Instant>,
    /// The controller refused rumble (it has no motors): not asked again.
    unsupported: bool,
}

impl Rumbler {
    pub fn new() -> Rumbler {
        Rumbler { sent: (0, 0), at: None, unsupported: false }
    }

    /// From the controller poll: sends the game's current levels to `pad` when they changed, and renews a level that
    /// is not zero before its effect runs out.
    pub fn update(&mut self, pad: &mut sdl3::gamepad::Gamepad) {
        if !enabled() || self.unsupported {
            return;
        }
        let motor = |m: usize| (f32::from_bits(LEVEL[m].load(Ordering::Relaxed)) * 65535.0).round() as u16;
        let want = (motor(0), motor(1));
        let due = self.at.is_none_or(|t| t.elapsed() >= RENEW);
        if want == self.sent && (want == (0, 0) || !due) {
            return;
        }
        if pad.set_rumble(want.0, want.1, EFFECT.as_millis() as u32).is_err() {
            if self.at.is_none() {
                println!("[+] Rumble: {} has no rumble", pad.name().unwrap_or_default());
            }
            self.unsupported = true;
            return;
        }
        if self.at.is_none() {
            println!("[+] Rumble: playing on {}", pad.name().unwrap_or_default());
        }
        (self.sent, self.at) = (want, Some(Instant::now()));
    }
}
