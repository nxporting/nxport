// Rumble: the game's HD Rumble values (nn::hid::SendVibrationValue[s], HLE'd in horizon/hle.rs) drive Core Haptics.
//
// Each Switch motor gets a VibrationValue (low / high band amplitude and frequency).
// The HLE stores per motor (handle 1 = left, 2 = right, as GetVibrationDeviceHandles hands them out) an intensity
// (the larger band amplitude) and a sharpness (the high band's share). The main thread's controller poll (host/input.rs,
// 250 Hz on macOS and iOS) feeds them to every connected controller that has haptics (GCController.haptics: one engine
// per handle where the controller has two, else one for both motors), or on iOS without such a controller to the
// phone's own Taptic Engine. Each engine plays one looping continuous haptic event whose intensity and sharpness follow
// the game through dynamic parameters. --no-rumble disables it; --rumble-scale=x scales intensity (default 1).

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

#[link(name = "CoreHaptics", kind = "framework")]
extern "C" {
    static CHHapticEventTypeHapticContinuous: *const AnyObject;
    static CHHapticEventParameterIDHapticIntensity: *const AnyObject;
    static CHHapticEventParameterIDHapticSharpness: *const AnyObject;
    static CHHapticDynamicParameterIDHapticIntensityControl: *const AnyObject;
    static CHHapticDynamicParameterIDHapticSharpnessControl: *const AnyObject;
}

#[link(name = "GameController", kind = "framework")]
extern "C" {
    static GCHapticsLocalityDefault: *const AnyObject;
    static GCHapticsLocalityLeftHandle: *const AnyObject;
    static GCHapticsLocalityRightHandle: *const AnyObject;
}

/// Per motor (0 = left, 1 = right): intensity and sharpness in 0..1 (f32 bits).
static LEVEL: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static SHARP: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

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
    // Mostly low band (the heavy motor feel): soft; mostly high band: crisp.
    let sharp = if lo + hi > 0.0 {
        0.2 + 0.6 * hi / (lo + hi)
    } else {
        0.5
    };
    LEVEL[motor].store(level.to_bits(), Ordering::Relaxed);
    SHARP[motor].store(sharp.to_bits(), Ordering::Relaxed);
}

fn motor(m: usize) -> (f32, f32) {
    (
        f32::from_bits(LEVEL[m].load(Ordering::Relaxed)),
        f32::from_bits(SHARP[m].load(Ordering::Relaxed)),
    )
}

/// One Core Haptics engine with its looping player and which motors it plays.
struct Engine {
    engine: *mut AnyObject,
    player: *mut AnyObject,
    motors: &'static [usize],
    sent: (f32, f32),
}

impl Engine {
    unsafe fn release(&self) {
        let _: () = msg_send![self.player, release];
        let _: () =
            msg_send![self.engine, stopWithCompletionHandler: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![self.engine, release];
    }
}

struct Device {
    /// The GCController (retained), or null for the phone's own engine.
    controller: *mut AnyObject,
    engines: Vec<Engine>,
    /// Gave up on it (no haptics, or engine errors): retried after `retry_at` polls.
    retry_at: u64,
}

struct State {
    devices: Vec<Device>,
    polls: u64,
}
// Only touched on the main thread (the controller poll).
unsafe impl Send for State {}

static STATE: Mutex<State> = Mutex::new(State {
    devices: Vec::new(),
    polls: 0,
});

unsafe fn nsarray(objs: &[*mut AnyObject]) -> *mut AnyObject {
    let Some(cls) = AnyClass::get(c"NSArray") else {
        return std::ptr::null_mut();
    };
    msg_send![cls, arrayWithObjects: objs.as_ptr(), count: objs.len()]
}

/// A started engine (from a GCDeviceHaptics locality, or the device's own when `haptics` is null) playing a looping
/// continuous event at intensity 0.
unsafe fn make_engine(
    haptics: *mut AnyObject,
    locality: *const AnyObject,
    motors: &'static [usize],
) -> Option<Engine> {
    let engine: *mut AnyObject = if haptics.is_null() {
        let cls = AnyClass::get(c"CHHapticEngine")?;
        let e: *mut AnyObject = msg_send![cls, alloc];
        let mut err: *mut AnyObject = std::ptr::null_mut();
        msg_send![e, initAndReturnError: &mut err]
    } else {
        let e: *mut AnyObject = msg_send![haptics, createEngineWithLocality: locality];
        if !e.is_null() {
            let _: *mut AnyObject = msg_send![e, retain];
        }
        e
    };
    if engine.is_null() {
        return None;
    }
    let _: () = msg_send![engine, setPlaysHapticsOnly: true];
    let _: () = msg_send![engine, setAutoShutdownEnabled: false];
    let mut err: *mut AnyObject = std::ptr::null_mut();
    let ok: bool = msg_send![engine, startAndReturnError: &mut err];
    let fail = |engine: *mut AnyObject| {
        let _: () = msg_send![engine, release];
        None
    };
    if !ok {
        return fail(engine);
    }
    let pcls = AnyClass::get(c"CHHapticEventParameter")?;
    let param = |id: *const AnyObject, v: f32| -> *mut AnyObject {
        let p: *mut AnyObject = msg_send![pcls, alloc];
        let p: *mut AnyObject = msg_send![p, initWithParameterID: id, value: v];
        let _: *mut AnyObject = msg_send![p, autorelease];
        p
    };
    let params = nsarray(&[
        param(CHHapticEventParameterIDHapticIntensity, 1.0),
        param(CHHapticEventParameterIDHapticSharpness, 0.5),
    ]);
    let ecls = AnyClass::get(c"CHHapticEvent")?;
    let ev: *mut AnyObject = msg_send![ecls, alloc];
    // Continuous events last at most 30 s: the player loops it.
    let ev: *mut AnyObject = msg_send![ev, initWithEventType: CHHapticEventTypeHapticContinuous, parameters: params, relativeTime: 0.0f64, duration: 30.0f64];
    let _: *mut AnyObject = msg_send![ev, autorelease];
    let pat_cls = AnyClass::get(c"CHHapticPattern")?;
    let pat: *mut AnyObject = msg_send![pat_cls, alloc];
    let pat: *mut AnyObject =
        msg_send![pat, initWithEvents: nsarray(&[ev]), parameters: nsarray(&[]), error: &mut err];
    if pat.is_null() {
        return fail(engine);
    }
    let _: *mut AnyObject = msg_send![pat, autorelease];
    let player: *mut AnyObject =
        msg_send![engine, createAdvancedPlayerWithPattern: pat, error: &mut err];
    if player.is_null() {
        return fail(engine);
    }
    let _: *mut AnyObject = msg_send![player, retain];
    let _: () = msg_send![player, setLoopEnabled: true];
    let e = Engine {
        engine,
        player,
        motors,
        sent: (-1.0, -1.0),
    };
    if !send(&e, 0.0, 0.5) {
        e.release();
        return None;
    }
    let ok: bool = msg_send![player, startAtTime: 0.0f64, error: &mut err];
    if !ok {
        e.release();
        return None;
    }
    Some(e)
}

unsafe fn send(e: &Engine, level: f32, sharp: f32) -> bool {
    let Some(cls) = AnyClass::get(c"CHHapticDynamicParameter") else {
        return false;
    };
    let dp = |id: *const AnyObject, v: f32| -> *mut AnyObject {
        let p: *mut AnyObject = msg_send![cls, alloc];
        let p: *mut AnyObject =
            msg_send![p, initWithParameterID: id, value: v, relativeTime: 0.0f64];
        let _: *mut AnyObject = msg_send![p, autorelease];
        p
    };
    // Sharpness control is an offset from the event's 0.5.
    let list = nsarray(&[
        dp(CHHapticDynamicParameterIDHapticIntensityControl, level),
        dp(
            CHHapticDynamicParameterIDHapticSharpnessControl,
            sharp - 0.5,
        ),
    ]);
    let mut err: *mut AnyObject = std::ptr::null_mut();
    msg_send![e.player, sendParameters: list, atTime: 0.0f64, error: &mut err]
}

/// Engines for a controller: one per handle when it has both, else one playing both motors.
unsafe fn controller_engines(c: *mut AnyObject) -> Vec<Engine> {
    let haptics: *mut AnyObject = msg_send![c, haptics];
    if haptics.is_null() {
        return Vec::new();
    }
    let locs: *mut AnyObject = msg_send![haptics, supportedLocalities];
    let has =
        |l: *const AnyObject| -> bool { !locs.is_null() && msg_send![locs, containsObject: l] };
    if has(GCHapticsLocalityLeftHandle) && has(GCHapticsLocalityRightHandle) {
        let l = make_engine(haptics, GCHapticsLocalityLeftHandle, &[0]);
        let r = make_engine(haptics, GCHapticsLocalityRightHandle, &[1]);
        if let (Some(l), Some(r)) = (l, r) {
            return vec![l, r];
        }
    }
    make_engine(haptics, GCHapticsLocalityDefault, &[0, 1])
        .into_iter()
        .collect()
}

/// Main thread, from the controller poll: keep one device entry per connected controller (plus the phone on iOS when
/// no controller rumbles) and send them the game's current levels.
pub unsafe fn update(controllers: &[*mut AnyObject]) {
    if !enabled() {
        return;
    }
    let mut st = STATE.lock().unwrap();
    st.polls += 1;
    let polls = st.polls;
    // Disconnected controllers go.
    st.devices.retain(|d| {
        let keep = d.controller.is_null() || controllers.contains(&d.controller);
        if !keep {
            for e in &d.engines {
                e.release();
            }
            let _: () = msg_send![d.controller, release];
        }
        keep
    });
    for &c in controllers {
        if !st.devices.iter().any(|d| d.controller == c) {
            let _: *mut AnyObject = msg_send![c, retain];
            st.devices.push(Device {
                controller: c,
                engines: Vec::new(),
                retry_at: 0,
            });
        }
    }
    // The phone itself (iOS): only while no controller can rumble.
    let pad_rumbles = st
        .devices
        .iter()
        .any(|d| !d.controller.is_null() && !d.engines.is_empty());
    let want_phone = cfg!(target_os = "ios") && !pad_rumbles;
    let has_phone = st.devices.iter().any(|d| d.controller.is_null());
    if want_phone && !has_phone {
        st.devices.push(Device {
            controller: std::ptr::null_mut(),
            engines: Vec::new(),
            retry_at: 0,
        });
    } else if !want_phone && has_phone {
        st.devices.retain(|d| {
            if d.controller.is_null() {
                for e in &d.engines {
                    e.release();
                }
            }
            !d.controller.is_null()
        });
    }
    let levels = [motor(0), motor(1)];
    for d in st.devices.iter_mut() {
        if d.engines.is_empty() {
            if polls < d.retry_at {
                continue;
            }
            d.engines = if d.controller.is_null() {
                make_engine(std::ptr::null_mut(), std::ptr::null(), &[0, 1])
                    .into_iter()
                    .collect()
            } else {
                controller_engines(d.controller)
            };
            if d.engines.is_empty() {
                d.retry_at = polls + 250 * 5; // no haptics here (or a failure): look again in ~5 s
                continue;
            }
            println!(
                "[+] Rumble: {} haptic engine(s) for {}",
                d.engines.len(),
                if d.controller.is_null() {
                    "the device"
                } else {
                    "a controller"
                }
            );
        }
        let mut failed = false;
        for e in d.engines.iter_mut() {
            let (mut level, mut sharp) = (0.0f32, 0.5f32);
            for &m in e.motors {
                if levels[m].0 >= level {
                    (level, sharp) = levels[m];
                }
            }
            if (level - e.sent.0).abs() < 0.01 && (sharp - e.sent.1).abs() < 0.02 {
                continue;
            }
            if send(e, level, sharp) {
                e.sent = (level, sharp);
            } else {
                failed = true; // the engine stopped (app in the background, controller reset): rebuild it
            }
        }
        if failed {
            for e in &d.engines {
                e.release();
            }
            d.engines.clear();
            d.retry_at = polls + 250;
        }
    }
}
