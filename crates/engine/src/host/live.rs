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
    fps_in_title: AtomicBool,
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
        fps_in_title: AtomicBool::new(s.display.fps_in_title),
    }
});

/// The settings as they are now: the resolved ones with what changed while the game ran.
pub fn current() -> crate::settings::Settings {
    let mut s = get().clone();
    refresh(&mut s);
    s
}

/// Writes the live values into `s` (its other fields stay as they are).
pub fn refresh(s: &mut crate::settings::Settings) {
    let l = &*LIVE;
    s.display.frame_limit = l.frame_limit.load(Relaxed);
    s.display.fps_in_title = l.fps_in_title.load(Relaxed);
    s.display.no_vsync = !crate::host::overlay::is_vsync();
    s.system.fps = crate::host::overlay::fps_mode();
    s.system.handheld = crate::horizon::hle::is_handheld();
    s.audio.volume = l.volume.load(Relaxed);
    s.audio.audio = !l.muted.load(Relaxed);
    s.input.no_rumble = !l.rumble.load(Relaxed);
    s.input.rumble_scale = l.rumble_scale.load(Relaxed) as f32 / 100.0;
    s.input.no_touch = !l.touch.load(Relaxed);
    s.input.swap_ab = l.swap_ab.load(Relaxed);
    s.input.swap_xy = l.swap_xy.load(Relaxed);
    s.graphics.async_pipelines = crate::host::overlay::is_async_shaders();
    s.totk.v1_1_2.dynamic_fps = l.real_time_speed.load(Relaxed);
}

/// The settings that change while the game runs, as (group heading, field): the settings window marks the others as
/// taking effect after a restart. A TotK option is named by field alone (its group's heading names the version).
pub const LIVE_FIELDS: &[(&str, &str)] = &[
    ("System", "fps"),
    ("System", "handheld"),
    ("Display", "no_vsync"),
    ("Display", "frame_limit"),
    ("Display", "fps_in_title"),
    ("Audio", "audio"),
    ("Audio", "volume"),
    ("Input", "no_rumble"),
    ("Input", "rumble_scale"),
    ("Input", "no_touch"),
    ("Input", "swap_ab"),
    ("Input", "swap_xy"),
    ("Graphics", "async_pipelines"),
    ("*", "dynamic_fps"),
];

pub fn is_live(heading: &str, field: &str) -> bool {
    LIVE_FIELDS.iter().any(|&(h, f)| f == field && (h == heading || h == "*"))
}

/// Applies the live fields of `s` (the settings window's edit), as the menu would; the window keeps the file.
pub fn apply(s: &crate::settings::Settings) {
    let l = &*LIVE;
    l.frame_limit.store(s.display.frame_limit.clamp(30, 144), Relaxed);
    l.fps_in_title.store(s.display.fps_in_title, Relaxed);
    l.volume.store(s.audio.volume.min(100), Relaxed);
    l.muted.store(!s.audio.audio, Relaxed);
    l.rumble.store(!s.input.no_rumble, Relaxed);
    l.rumble_scale.store((s.input.rumble_scale * 100.0).round().clamp(0.0, 400.0) as u32, Relaxed);
    l.touch.store(!s.input.no_touch, Relaxed);
    l.real_time_speed.store(s.totk.v1_1_2.dynamic_fps, Relaxed);
    if (l.swap_ab.load(Relaxed), l.swap_xy.load(Relaxed)) != (s.input.swap_ab, s.input.swap_xy) {
        l.swap_ab.store(s.input.swap_ab, Relaxed);
        l.swap_xy.store(s.input.swap_xy, Relaxed);
        crate::host::input::set_controller_swaps(s.input.swap_ab, s.input.swap_xy);
    }
    if crate::horizon::hle::is_handheld() != s.system.handheld {
        crate::horizon::hle::set_handheld(s.system.handheld);
    }
    crate::host::overlay::set_vsync(!s.display.no_vsync);
    crate::host::overlay::set_fps_mode(s.system.fps);
    crate::host::overlay::set_async_shaders(s.graphics.async_pipelines);
}

/// The display's refresh rate in Hz (0: unknown), which the window keeps current: with vsync the display paces at it.
static REFRESH: AtomicU32 = AtomicU32::new(0);

pub fn refresh_rate() -> u32 {
    REFRESH.load(Relaxed)
}

pub fn set_refresh_rate(hz: u32) {
    REFRESH.store(hz, Relaxed);
}

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

/// The window's title shows the frame rate.
pub fn fps_in_title() -> bool {
    LIVE.fps_in_title.load(Relaxed)
}

pub fn set_fps_in_title(on: bool) {
    LIVE.fps_in_title.store(on, Relaxed);
    crate::config::remember(|p| p.display.fps_in_title = Some(on));
}

/// TotK: game time from real time (profile/totk/frame_time.rs); off, the stock step from the present interval.
pub fn real_time_speed() -> bool {
    LIVE.real_time_speed.load(Relaxed)
}

pub fn set_real_time_speed(on: bool) {
    LIVE.real_time_speed.store(on, Relaxed);
    crate::config::remember(|p| p.totk.v1_1_2.dynamic_fps = Some(on));
}
