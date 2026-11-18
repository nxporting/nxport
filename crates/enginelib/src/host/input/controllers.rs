// Game controllers (GameController.framework, main thread only).

use super::*;

// ---- game controllers (GameController.framework, main thread only) ----

/// Takes each connected controller's element values (`PAD_ELEMS` order), controller i for player i (player 0 also has the
/// keyboard and stays connected without one), and refreshes the Npad state if anything changed.
#[cfg(windows)]
pub fn set_players(pads: &[[f32; 25]]) {
    let mut changed = false;
    for p in 0..MAX_PLAYERS {
        let v = pads.get(p).copied().unwrap_or([0.0; 25]);
        for (slot, val) in PLAYER_PADS[p].iter().zip(v) {
            changed |= slot.swap(val.to_bits(), Ordering::Relaxed) != val.to_bits();
        }
        let connected = p == 0 || p < pads.len();
        changed |= PLAYER_CONNECTED[p].swap(connected, Ordering::Relaxed) != connected;
    }
    if changed {
        update_npad();
    }
}

/// The elements a controller's buttons labeled A, B, X and Y feed (by label: the button labeled A feeds the element
/// bound to the Switch's A, East by default), or by position on `physical` controllers with the swaps on.
pub fn face_elems(physical: bool) -> [PadElem; 4] {
    use PadElem as P;
    let (a, b) = if physical && SWAP_AB.load(Ordering::Relaxed) {
        (P::South, P::East)
    } else {
        (P::East, P::South)
    };
    let (x, y) = if physical && SWAP_XY.load(Ordering::Relaxed) {
        (P::West, P::North)
    } else {
        (P::North, P::West)
    };
    [a, b, x, y]
}

#[cfg(target_vendor = "apple")]
#[link(name = "GameController", kind = "framework")]
extern "C" {}

#[cfg(target_vendor = "apple")]
/// Reads all elements from one controller (either physical or virtual).
pub(crate) unsafe fn read_controller_elements(c: *mut objc2::runtime::AnyObject, physical: bool) -> [f32; 25] {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2::sel;
    use PadElem as P;

    let mut v = [0f32; 25];
    let gp: *mut AnyObject = msg_send![c, extendedGamepad];
    if gp.is_null() {
        return v;
    }
    let button = |sel: objc2::runtime::Sel| -> f32 {
        let b: *mut AnyObject = msg_send![gp, performSelector: sel];
        if b.is_null() {
            0.0
        } else {
            msg_send![b, value]
        }
    };
    let dir = |pad: *mut AnyObject| -> [f32; 4] {
        // up, down, left, right
        if pad.is_null() {
            return [0.0; 4];
        }
        let x: *mut AnyObject = msg_send![pad, xAxis];
        let y: *mut AnyObject = msg_send![pad, yAxis];
        let (x, y): (f32, f32) = (msg_send![x, value], msg_send![y, value]);
        [y.max(0.0), (-y).max(0.0), (-x).max(0.0), x.max(0.0)]
    };
    let mut put = |e: PadElem, val: f32| v[e.index()] = v[e.index()].max(val);
    let [a_elem, b_elem, x_elem, y_elem] = face_elems(physical);
    put(a_elem, button(sel!(buttonA)));
    put(b_elem, button(sel!(buttonB)));
    put(x_elem, button(sel!(buttonX)));
    put(y_elem, button(sel!(buttonY)));
    put(P::LeftShoulder, button(sel!(leftShoulder)));
    put(P::RightShoulder, button(sel!(rightShoulder)));
    put(P::LeftTrigger, button(sel!(leftTrigger)));
    put(P::RightTrigger, button(sel!(rightTrigger)));
    put(P::Menu, button(sel!(buttonMenu)));
    put(P::Options, button(sel!(buttonOptions)));
    put(P::Home, button(sel!(buttonHome)));
    put(P::LeftStickButton, button(sel!(leftThumbstickButton)));
    put(P::RightStickButton, button(sel!(rightThumbstickButton)));
    let dpad: *mut AnyObject = msg_send![gp, dpad];
    let ls: *mut AnyObject = msg_send![gp, leftThumbstick];
    let rs: *mut AnyObject = msg_send![gp, rightThumbstick];
    for (elems, vals) in [
        (
            [P::DpadUp, P::DpadDown, P::DpadLeft, P::DpadRight],
            dir(dpad),
        ),
        (
            [
                P::LeftStickUp,
                P::LeftStickDown,
                P::LeftStickLeft,
                P::LeftStickRight,
            ],
            dir(ls),
        ),
        (
            [
                P::RightStickUp,
                P::RightStickDown,
                P::RightStickLeft,
                P::RightStickRight,
            ],
            dir(rs),
        ),
    ] {
        for (e, val) in elems.into_iter().zip(vals) {
            put(e, val);
        }
    }
    v
}

/// Polls connected gamepads, assigning distinct controllers to players 0..7 (player 0 combines keyboard + controller 0),
/// and refreshes the Npad state. Main thread only.
#[cfg(target_vendor = "apple")]
pub fn poll_controllers(_mtm: objc2::MainThreadMarker) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    unsafe {
        let Some(cls) = AnyClass::get(c"GCController") else {
            return;
        };
        static MONITOR: std::sync::Once = std::sync::Once::new();
        MONITOR.call_once(|| {
            // Keep reading controllers while the Controls window (or another app) has focus.
            let _: () = msg_send![cls, setShouldMonitorBackgroundEvents: true];
        });
        let list: *mut AnyObject = msg_send![cls, controllers];
        if list.is_null() {
            return;
        }
        let n: usize = msg_send![list, count];
        let all: Vec<*mut AnyObject> = (0..n).map(|i| msg_send![list, objectAtIndex: i]).collect();
        // The game's rumble to controllers with haptics (or the phone itself).
        crate::host::rumble::update(&all);

        #[cfg(target_os = "ios")]
        let virtual_pad = crate::host::display::virtual_gamepad();
        #[cfg(not(target_os = "ios"))]
        let virtual_pad: *mut AnyObject = std::ptr::null_mut();

        // Separate virtual on-screen controller from physical controllers
        let mut physical = Vec::new();
        for &c in &all {
            if c != virtual_pad {
                physical.push(c);
            }
        }

        let mut changed = false;

        // Player 0: virtual gamepad (if present) combined with first physical controller (if present)
        let mut p0 = [0f32; 25];
        if !virtual_pad.is_null() {
            let v = read_controller_elements(virtual_pad, false);
            for i in 0..25 {
                p0[i] = p0[i].max(v[i]);
            }
        }
        if let Some(&c0) = physical.first() {
            let v = read_controller_elements(c0, true);
            for i in 0..25 {
                p0[i] = p0[i].max(v[i]);
            }
        }
        for (slot, &val) in PLAYER_PADS[0].iter().zip(&p0) {
            changed |= slot.swap(val.to_bits(), Ordering::Relaxed) != val.to_bits();
        }
        // Player 0 is always connected
        changed |= !PLAYER_CONNECTED[0].swap(true, Ordering::Relaxed);

        // Players 1..7: physical controllers 1..7
        for p in 1..MAX_PLAYERS {
            let mut pv = [0f32; 25];
            let is_connected = p < physical.len();
            if is_connected {
                pv = read_controller_elements(physical[p], true);
            }
            for (slot, &val) in PLAYER_PADS[p].iter().zip(&pv) {
                changed |= slot.swap(val.to_bits(), Ordering::Relaxed) != val.to_bits();
            }
            changed |= PLAYER_CONNECTED[p].swap(is_connected, Ordering::Relaxed) != is_connected;
        }

        if changed {
            update_npad();
        }
    }
}
