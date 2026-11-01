// Settings that change while the game runs: each starts from the resolved settings, the window's menu changes it, and
// the change is kept in the game's own settings file (config::remember), a layer above the app's, so the next run
// starts with it. The engine reads these here, not settings::get(), wherever it reads them per frame or per sample.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::LazyLock;

use crate::settings::{get, SettingsPatch};

struct Live {
    frame_limit: AtomicU32,
    volume: AtomicU32,
    muted: AtomicBool,
    rumble: AtomicBool,
    /// Rumble strength in percent.
    rumble_scale: AtomicU32,
    touch: AtomicBool,
    swap_ab: AtomicBool,
    swap_xy: AtomicBool,
    real_time_speed: AtomicBool,
}

static LIVE: LazyLock<Live> = LazyLock::new(|| {
    let s = get();
    Live {
        frame_limit: AtomicU32::new(s.display.frame_limit.clamp(30, 144)),
        volume: AtomicU32::new(s.audio.volume.min(100)),
        muted: AtomicBool::new(!s.audio.audio),
        rumble: AtomicBool::new(!s.input.no_rumble),
        rumble_scale: AtomicU32::new((s.input.rumble_scale * 100.0).round().clamp(0.0, 400.0) as u32),
        touch: AtomicBool::new(!s.input.no_touch),
        swap_ab: AtomicBool::new(s.input.swap_ab),
        swap_xy: AtomicBool::new(s.input.swap_xy),
        real_time_speed: AtomicBool::new(s.totk.v1_1_2.dynamic_fps),
    }
});

/// The frame-rate limit when the title may run faster than 30 fps (the 30 fps setting is overlay::fps_mode's).
pub fn frame_limit() -> u32 {
    LIVE.frame_limit.load(Relaxed)
}

pub fn set_frame_limit(fps: u32) {
    LIVE.frame_limit.store(fps, Relaxed);
    crate::config::remember(|p| p.display.frame_limit = Some(fps));
}

/// The output gain: 0 when muted.
pub fn gain() -> f32 {
    if LIVE.muted.load(Relaxed) {
        0.0
    } else {
        LIVE.volume.load(Relaxed) as f32 / 100.0
    }
}

pub fn volume() -> u32 {
    LIVE.volume.load(Relaxed)
}

pub fn set_volume(percent: u32) {
    LIVE.volume.store(percent, Relaxed);
    crate::config::remember(|p| p.audio.volume = Some(percent));
}

pub fn muted() -> bool {
    LIVE.muted.load(Relaxed)
}

pub fn set_muted(on: bool) {
    LIVE.muted.store(on, Relaxed);
    crate::config::remember(|p| p.audio.audio = Some(!on));
}

pub fn rumble() -> bool {
    LIVE.rumble.load(Relaxed)
}

pub fn set_rumble(on: bool) {
    LIVE.rumble.store(on, Relaxed);
    crate::config::remember(|p| p.input.no_rumble = Some(!on));
}

/// Rumble strength as a factor.
pub fn rumble_scale() -> f32 {
    LIVE.rumble_scale.load(Relaxed) as f32 / 100.0
}

pub fn rumble_percent() -> u32 {
    LIVE.rumble_scale.load(Relaxed)
}

pub fn set_rumble_percent(percent: u32) {
    LIVE.rumble_scale.store(percent, Relaxed);
    crate::config::remember(|p| p.input.rumble_scale = Some(percent as f32 / 100.0));
}

pub fn touch() -> bool {
    LIVE.touch.load(Relaxed)
}

pub fn set_touch(on: bool) {
    LIVE.touch.store(on, Relaxed);
    crate::config::remember(|p| p.input.no_touch = Some(!on));
}

/// Physical controllers' A/B and X/Y by position, as on a Switch controller (host/input.rs applies them).
pub fn swaps() -> (bool, bool) {
    (LIVE.swap_ab.load(Relaxed), LIVE.swap_xy.load(Relaxed))
}

pub fn set_swaps(ab: bool, xy: bool) {
    LIVE.swap_ab.store(ab, Relaxed);
    LIVE.swap_xy.store(xy, Relaxed);
    crate::host::input::set_controller_swaps(ab, xy);
    crate::config::remember(|p: &mut SettingsPatch| {
        p.input.swap_ab = Some(ab);
        p.input.swap_xy = Some(xy);
    });
}

/// TotK: game time from real time (profile/totk/frame_time.rs); off, the stock step from the present interval.
pub fn real_time_speed() -> bool {
    LIVE.real_time_speed.load(Relaxed)
}

pub fn set_real_time_speed(on: bool) {
    LIVE.real_time_speed.store(on, Relaxed);
    crate::config::remember(|p| p.totk.v1_1_2.dynamic_fps = Some(on));
}
