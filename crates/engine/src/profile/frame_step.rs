// The real-time step games' frame-rate hooks give their clocks (totk/frame_time.rs, botw/frame_time.rs): the time since
// the previous frame, so game time keeps to real time at any frame rate.
//
// With vsync pacing frames (vblank_period), frames reach the screen a whole number of refreshes apart, but the game's
// frames start unevenly between them (the swapchain lets a frame start once an image frees): real-time steps then moved
// things unevenly across evenly shown frames, a stutter. There the step is rounded to whole refreshes, the remainder
// carried into the next frames' steps, so game time keeps to real time.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// The shortest and longest frame a step accounts for: no slower than 1/240 s of game time a frame, and no more than two
/// frames at 30 fps (a stall, a loading screen, a debugger) in one step.
const MIN_STEP_S: f32 = 1.0 / 240.0;
const MAX_STEP_S: f32 = 2.0 / 30.0;
/// A second call within this time reuses the step: the frame has not ended (TotK's setter has two callers; BotW updates
/// its clock twice a frame while paused).
const SAME_FRAME_NS: u64 = 1_000_000;
/// The most refreshes a burst of frames may run ahead of real time (swapchain images and frames in flight).
const MAX_AHEAD: f32 = 8.0;

static LAST_NS: AtomicU64 = AtomicU64::new(0);
static LAST_STEP: AtomicU32 = AtomicU32::new(0);
/// Real time not yet stepped (f32 bits, seconds; may be negative): what rounding to whole refreshes left over.
static CARRY: AtomicU32 = AtomicU32::new(0);

/// This frame's step in seconds, from the real time since the previous frame (`interval`, the game's present interval,
/// sizes the first one). Called once per game frame, by the game's clock update.
pub fn step_seconds(interval: i32) -> f32 {
    let now = crate::platform::clock::now_ns();
    let last = LAST_NS.load(Ordering::Relaxed);
    let reuse = f32::from_bits(LAST_STEP.load(Ordering::Relaxed));
    if last != 0 && now.saturating_sub(last) < SAME_FRAME_NS && reuse > 0.0 {
        return reuse;
    }
    LAST_NS.store(now, Ordering::Relaxed);
    let dt = if last == 0 {
        interval.max(1) as f32 / 60.0
    } else {
        ((now - last) as f64 / 1e9) as f32
    }
    .clamp(MIN_STEP_S, MAX_STEP_S);
    let dt = match vblank_period() {
        Some(period) => {
            let real = dt + f32::from_bits(CARRY.load(Ordering::Relaxed));
            let step = (real / period).round().max(1.0) * period;
            // Frames started in a burst each show a refresh after the last: the time they are ahead is carried into the
            // wait that follows (up to MAX_AHEAD refreshes). Time behind carries up to a refresh, so a stall is not made
            // up over the frames after it.
            CARRY.store((real - step).clamp(-MAX_AHEAD * period, period).to_bits(), Ordering::Relaxed);
            step
        }
        None => {
            CARRY.store(0, Ordering::Relaxed);
            dt
        }
    };
    LAST_STEP.store(dt.to_bits(), Ordering::Relaxed);
    dt
}

/// The step step_seconds gave last (seconds; 0 before the first).
pub fn last_step_seconds() -> f32 {
    f32::from_bits(LAST_STEP.load(Ordering::Relaxed))
}

/// With vsync, the display's refresh period when the swapchain paces frames: the frame-rate limit is at or above the
/// refresh rate (nvn::api's limiter then leaves pacing to the swapchain). Known on hosts that report the refresh rate.
fn vblank_period() -> Option<f32> {
    use crate::host::{live, overlay};
    let hz = live::refresh_hz();
    let paced = overlay::is_vsync() && overlay::fps_mode() != crate::settings::FpsMode::Stock30 && hz >= 30.0;
    (paced && live::frame_limit() + 1 >= live::refresh_rate()).then(|| 1.0 / hz)
}
