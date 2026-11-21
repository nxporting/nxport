// Mouse look, and the exact mouse look.

use super::*;

// ---- mouse look ----

/// Mouse look (settings input.mouse_look): mouse movement becomes the right stick. A stick reports a deflection, not a
/// movement, so the stick follows the mouse's speed: pixels per second, smoothed over ~10 ms, full deflection at
/// MOUSE_LOOK_FULL px/s times the speed setting. Small speeds are lifted past the deadzone games keep around the
/// stick's centre, so slow aiming still moves the camera. The mouse at rest centres the stick within ~50 ms.
pub(crate) struct MouseLook {
    pending: (f32, f32),
    smooth: (f32, f32),
    last: Option<std::time::Instant>,
}
pub(crate) static MOUSE_LOOK: std::sync::Mutex<MouseLook> =
    std::sync::Mutex::new(MouseLook { pending: (0.0, 0.0), smooth: (0.0, 0.0), last: None });
/// The right stick from the mouse, as two f32 bit patterns (x right, y up).
pub(crate) static MOUSE_STICK: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
pub(crate) const MOUSE_LOOK_FULL: f32 = 1500.0;
pub(crate) const MOUSE_LOOK_DEADZONE: f32 = 0.15;

pub(crate) fn mouse_look_on() -> bool {
    crate::host::live::mouse_look() && crate::settings::try_get().is_some_and(|s| !s.input.direct_mouse)
}

/// Relative mouse movement while mouse look holds the mouse (window pixels; y down). The Windows and macOS windows
/// hold the mouse.
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub fn mouse_look_moved(dx: f32, dy: f32) {
    if mouse_tilt_held() {
        let mut t = MOUSE_TILT.lock().unwrap();
        t.pending.0 += dx as f64;
        t.pending.1 += dy as f64;
        return;
    }
    // Into the game's camera angles where a profile takes them, as a stick would turn them: mouse right as the stick
    // right (the yaw falls), mouse down as the stick down (the pitch rises).
    // Only while the camera storing them is the one the profile takes them for (ANGLE_ROUTE): another camera (TotK's
    // bow aim) gets the movement through the stick.
    // While the game is paused its camera stores no angles: movement then is dropped, as a stick moved then would turn
    // nothing. Running means the camera stored its angles this frame or the last.
    let deg = || MOUSE_DEG_PER_PX * crate::settings::try_get().map_or(1.0, |s| s.input.mouse_look_speed);
    // The exact route is experimental and switched in the game's menu; off, the mouse moves the stick.
    let route = ANGLE_ROUTE.load(Ordering::Acquire) && crate::host::live::exact_mouse_aim();
    let running = crate::gpu::backend().presented_frames() as u64 <= CAMERA_FRAME.load(Ordering::Acquire) + 1;
    let dx = if route && ANGLE_SINK[0].load(Ordering::Relaxed) {
        if running {
            add_mouse_angle(0, -dx * deg());
        }
        0.0
    } else {
        dx
    };
    let dy = if route && ANGLE_SINK[1].load(Ordering::Relaxed) {
        if running {
            add_mouse_angle(1, dy * deg());
        }
        0.0
    } else {
        dy
    };
    let mut m = MOUSE_LOOK.lock().unwrap();
    m.pending.0 += dx;
    m.pending.1 += dy;
}

// ---- Exact mouse look -------------------------------------------------------------------------------------------------
//
// Where a game profile owns the camera's angles (profile/totk/camera_angles.rs), mouse movement is added to them
// directly: MOUSE_DEG_PER_PX degrees per pixel times mouse-look-speed, nothing else, and the mouse does not move the
// stick on that axis. The game's own rules decide what the angles do (a mode that does not take them ignores them, as it
// would the stick). Every pixel is added once, the next time the game takes the angles; movement made while the game's
// camera code does not run (paused) is dropped, as is movement the game would not have turned by.

/// Degrees per pixel of mouse movement at mouse-look-speed 1.
pub(crate) const MOUSE_DEG_PER_PX: f32 = 0.05;
/// Whether a game profile takes the yaw (0) and the pitch (1).
pub(crate) static ANGLE_SINK: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// Degrees of yaw and pitch the game has not taken yet (f32 bits).
pub(crate) static ANGLE_PENDING: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

/// A game profile takes the camera's yaw (`pitch`: and pitch) from take_mouse_angle.
pub fn set_angle_sink(pitch: bool) {
    ANGLE_SINK[0].store(true, Ordering::Relaxed);
    ANGLE_SINK[1].store(pitch, Ordering::Relaxed);
}

/// Called by the game profile as the game takes its camera's angle `i` (0 yaw, 1 pitch): the degrees to add (the
/// pending amount cleared).
pub fn take_mouse_angle(i: usize) -> f32 {
    if i == 0 {
        CAMERA_FRAME.store(crate::gpu::backend().presented_frames() as u64, Ordering::Release);
    }
    f32::from_bits(ANGLE_PENDING[i].swap(0, Ordering::AcqRel))
}

/// The presented frame in which the game last stored its camera's angles (take_mouse_angle(0) is called then).
pub(crate) static CAMERA_FRAME: AtomicU64 = AtomicU64::new(0);

/// Whether mouse movement goes into the angles (the profile's camera is the one in use) or through the stick.
pub(crate) static ANGLE_ROUTE: AtomicBool = AtomicBool::new(true);

/// The profile says whether its camera is the one in use. Leaving it, the degrees not taken yet go back to the stick as
/// the pixels they came from, so no movement is lost or applied later.
pub fn set_angle_route(on: bool) {
    if ANGLE_ROUTE.swap(on, Ordering::AcqRel) && !on {
        let deg = MOUSE_DEG_PER_PX * crate::settings::try_get().map_or(1.0, |s| s.input.mouse_look_speed);
        let (yaw, pitch) = (take_mouse_angle(0), take_mouse_angle(1));
        let mut m = MOUSE_LOOK.lock().unwrap();
        m.pending.0 += -yaw / deg;
        m.pending.1 += pitch / deg;
    }
}

/// The mouse moved since the game last tested for camera input (the game's test and its storing of the angles come in
/// either order in a frame, so the pending degrees cannot stand for it). Taking it clears it.
pub(crate) static MOUSE_INPUT: AtomicBool = AtomicBool::new(false);

/// For the game's camera-input test: whether the mouse moved since the last test.
pub fn take_mouse_input() -> bool {
    MOUSE_INPUT.swap(false, Ordering::AcqRel)
}

/// The profile's camera code ran this frame without taking angles yet (it takes them later in the frame, or only when
/// it sees input): movement arriving now is not made while paused.
pub fn camera_ran() {
    CAMERA_FRAME.store(crate::gpu::backend().presented_frames() as u64, Ordering::Release);
}

/// Whether mouse degrees wait to be taken.
pub fn mouse_angle_pending() -> bool {
    ANGLE_PENDING.iter().any(|a| a.load(Ordering::Acquire) != 0)
}

/// Pending degrees of angle `i` put back (the game has not taken them yet: the camera waits for its mode).
pub fn keep_mouse_angle(i: usize, deg: f32) {
    if deg != 0.0 {
        add_mouse_angle(i, deg);
    }
}

/// Adds `deg` to pending angle `i` (the window's input thread adds; the game's camera thread takes).
pub(crate) fn add_mouse_angle(i: usize, deg: f32) {
    MOUSE_INPUT.store(true, Ordering::Release);
    let mut cur = ANGLE_PENDING[i].load(Ordering::Acquire);
    loop {
        let next = (f32::from_bits(cur) + deg).to_bits();
        match ANGLE_PENDING[i].compare_exchange_weak(cur, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return,
            Err(c) => cur = c,
        }
    }
}
/// The stick for a mouse speed (px/s), deadzone lifted, no longer than `max` (1: inside the unit circle).
pub(crate) fn mouse_look_deflection(vx: f32, vy: f32, speed: f32, max: f32) -> (f32, f32) {
    let (x, y) = (vx * speed / MOUSE_LOOK_FULL, -vy * speed / MOUSE_LOOK_FULL);
    let m = (x * x + y * y).sqrt();
    if m < 1e-3 {
        return (0.0, 0.0);
    }
    let lifted = (MOUSE_LOOK_DEADZONE + (1.0 - MOUSE_LOOK_DEADZONE) * m).min(max);
    (x / m * lifted, y / m * lifted)
}

/// Turns the mouse movement since the last read into the right stick. Called as the game reads its controller.
pub(crate) fn update_mouse_look() {
    if !mouse_look_on() {
        return;
    }
    let speed = crate::settings::try_get().map_or(1.0, |s| s.input.mouse_look_speed);
    let now = std::time::Instant::now();
    let (x, y) = {
        let mut m = MOUSE_LOOK.lock().unwrap();
        let dt = m.last.map_or(0.016, |l| now.duration_since(l).as_secs_f32()).clamp(0.001, 0.1);
        m.last = Some(now);
        let moved = m.pending;
        m.pending = (0.0, 0.0);
        let v = (moved.0 / dt, moved.1 / dt);
        // 10 ms: enough to even out reads at uneven times, little lag (30 ms was felt as camera latency).
        let a = 1.0 - (-dt / 0.01).exp();
        m.smooth.0 += (v.0 - m.smooth.0) * a;
        m.smooth.1 += (v.1 - m.smooth.1) * a;
        // Under a pixel per frame at 60 Hz: at rest.
        if m.smooth.0.abs() < 60.0 && m.smooth.1.abs() < 60.0 && v == (0.0, 0.0) {
            m.smooth = (0.0, 0.0);
        }
        mouse_look_deflection(m.smooth.0, m.smooth.1, speed, 1.0)
    };
    let (ox, oy) = (f32::from_bits(MOUSE_STICK[0].load(Ordering::Relaxed)), f32::from_bits(MOUSE_STICK[1].load(Ordering::Relaxed)));
    if (x, y) != (ox, oy) {
        MOUSE_STICK[0].store(x.to_bits(), Ordering::Relaxed);
        MOUSE_STICK[1].store(y.to_bits(), Ordering::Relaxed);
        update_npad();
    }
}

// ---- mouse tilt ----

/// Mouse tilt (Savage Wind's "hold T"): while T is held, mouse movement turns a virtual controller instead of the
/// camera, for motion aiming and tilt puzzles: right turns it right (yaw), up raises its far end (pitch, at most 85
/// degrees). Letting go keeps its orientation, so the mouse can be moved back; tapping T without moving the mouse levels
/// it. Player 1's motion then comes from it, as from a controller's sensors.
pub(crate) struct MouseTilt {
    yaw: f64,
    pitch: f64,
    pending: (f64, f64),
    moved: bool,
    held: bool,
}
pub(crate) static MOUSE_TILT: Mutex<MouseTilt> =
    Mutex::new(MouseTilt { yaw: 0.0, pitch: 0.0, pending: (0.0, 0.0), moved: false, held: false });
/// T's key code (the macOS one, as KEYS holds them).
const TILT_KEY: usize = 17;
/// Radians the virtual controller turns per pixel of mouse movement (a tuning number, Savage Wind's 0.2 degrees).
const TILT_RAD_PER_PX: f64 = 0.2 * std::f64::consts::PI / 180.0;
const TILT_PITCH_MAX: f64 = 85.0 * std::f64::consts::PI / 180.0;

pub(crate) fn mouse_tilt_held() -> bool {
    mouse_look_on() && KEYS[TILT_KEY].load(Ordering::Relaxed)
}

/// Turns the mouse movement since the last read into the virtual controller's orientation, while T is held and at
/// the read after it is let go (that sample's gyroscope reads no turn, so the game's aim stops).
pub(crate) fn update_mouse_tilt() {
    // Without a lock while the tilt is idle (the game reads its controller many times a frame).
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    let held = mouse_tilt_held();
    if !held && !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    ACTIVE.store(held, Ordering::Relaxed);
    let q = {
        let mut t = MOUSE_TILT.lock().unwrap();
        if !held && !t.held {
            return;
        }
        if held && !t.held {
            t.moved = false;
        }
        let (dx, dy) = std::mem::take(&mut t.pending);
        t.moved |= dx != 0.0 || dy != 0.0;
        if !held && !t.moved {
            t.yaw = 0.0;
            t.pitch = 0.0;
        }
        t.held = held;
        t.yaw -= dx * TILT_RAD_PER_PX;
        t.pitch = (t.pitch - dy * TILT_RAD_PER_PX).clamp(-TILT_PITCH_MAX, TILT_PITCH_MAX);
        qmul(qrot([0.0, 0.0, t.yaw]), qrot([t.pitch, 0.0, 0.0]))
    };
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let ns = START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64;
    motion_set(0, q, ns);
}

/// Before the game reads player 1's controller: the debug input script, mouse look and the mouse tilt.
pub fn before_npad_read() {
    apply_script();
    update_mouse_tilt();
    update_mouse_look();
}

/// Folds keyboard and controller state through the bindings into each player's Npad state.
pub fn update_npad() {
    if SUSPENDED.load(Ordering::Relaxed) {
        for p in 0..MAX_PLAYERS {
            NPAD_BUTTONS[p].store(0, Ordering::Release);
            for s in &NPAD_STICKS[p] {
                s.store(0, Ordering::Release);
            }
        }
        return;
    }
    let b = bindings();
    for player in 0..MAX_PLAYERS {
        let value = |s: SwitchInput| -> f32 {
            let bd = b[s.index()];
            let k = if player == 0 {
                bd.key.is_some_and(|k| {
                    KEYS.get(k as usize)
                        .is_some_and(|a| a.load(Ordering::Relaxed))
                })
            } else {
                false
            };
            let p = bd.pad.map_or(0.0, |elem| player_pad_value(player, elem));
            if k {
                1.0
            } else {
                p
            }
        };
        let mut buttons = 0u64;
        for s in SWITCH_INPUTS {
            if let Some(bit) = s.button_bit() {
                if value(s) > 0.5 {
                    buttons |= 1 << bit;
                }
            }
        }
        use SwitchInput::*;
        let stick = |up, down, left, right| {
            let (mut x, mut y) = (value(right) - value(left), value(up) - value(down));
            let m = (x * x + y * y).sqrt();
            if m > 1.0 {
                // Keyboard diagonals: keep the stick inside its circle.
                x /= m;
                y /= m;
            }
            (x, y)
        };
        let (lx, ly) = stick(LUp, LDown, LLeft, LRight);
        let (mut rx, mut ry) = stick(RUp, RDown, RLeft, RRight);
        if player == 0 && mouse_look_on() {
            rx += f32::from_bits(MOUSE_STICK[0].load(Ordering::Relaxed));
            ry += f32::from_bits(MOUSE_STICK[1].load(Ordering::Relaxed));
        }
        // The HID service also reports each stick as four direction "buttons" (StickLLeft = bit 16 ... StickRDown = 23);
        // menus navigate with them.
        for (i, (x, y)) in [(lx, ly), (rx, ry)].into_iter().enumerate() {
            let base = 16 + 4 * i;
            if x < -0.5 {
                buttons |= 1 << base;
            }
            if y > 0.5 {
                buttons |= 1 << (base + 1);
            }
            if x > 0.5 {
                buttons |= 1 << (base + 2);
            }
            if y < -0.5 {
                buttons |= 1 << (base + 3);
            }
        }
        let unit = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0).round() as i32;
        for (slot, v) in NPAD_STICKS[player].iter().zip([lx, ly, rx, ry]) {
            slot.store(unit(v), Ordering::Release);
        }
        NPAD_BUTTONS[player].store(buttons, Ordering::Release);
    }
}
