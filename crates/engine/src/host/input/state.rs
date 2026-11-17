// Live input state, written on the main thread and read by nn::hid.

use super::*;

// ---- live state (written on the main thread, read by nn::hid) ----

pub const MAX_PLAYERS: usize = 8;

pub(crate) static KEYS: [AtomicBool; 133] = [const { AtomicBool::new(false) }; 133];
/// The mouse buttons as key codes past the keyboard's: (code, name in controls.cfg), in nn::hid::MouseButtonSet's bit
/// order (left, right, middle, forward, back).
pub const MOUSE_KEYS: [(u16, &str); 5] =
    [(128, "MouseLeft"), (129, "MouseRight"), (130, "MouseMiddle"), (131, "MouseForward"), (132, "MouseBack")];
pub(crate) static PLAYER_PADS: [[AtomicU32; 25]; MAX_PLAYERS] =
    [const { [const { AtomicU32::new(0) }; 25] }; MAX_PLAYERS];
pub(crate) static PLAYER_CONNECTED: [AtomicBool; MAX_PLAYERS] = {
    let mut arr = [const { AtomicBool::new(false) }; MAX_PLAYERS];
    arr[0] = AtomicBool::new(true);
    arr
};
/// While the Controls window captures a binding, the game sees no input.
pub(crate) static SUSPENDED: AtomicBool = AtomicBool::new(false);
/// Physical controllers only: A/B and X/Y by position (as on a Switch controller: the bottom button is B, the left one Y)
/// instead of by label. The on-screen controller always goes by its labels.
pub(crate) static SWAP_AB: AtomicBool = AtomicBool::new(false);
pub(crate) static SWAP_XY: AtomicBool = AtomicBool::new(false);

pub fn set_controller_swaps(ab: bool, xy: bool) {
    SWAP_AB.store(ab, Ordering::Relaxed);
    SWAP_XY.store(xy, Ordering::Relaxed);
}

pub(crate) static NPAD_BUTTONS: [AtomicU64; MAX_PLAYERS] = [const { AtomicU64::new(0) }; MAX_PLAYERS];
pub(crate) static NPAD_STICKS: [[AtomicI32; 4]; MAX_PLAYERS] =
    [const { [const { AtomicI32::new(0) }; 4] }; MAX_PLAYERS];

/// The current Npad state for player 0: (NpadButtonSet bits, [left x, left y, right x, right y]) in hid units (±0x7fff).
#[cfg_attr(target_os = "ios", allow(dead_code))] // the macOS controls window
pub fn npad_state() -> (u64, [i32; 4]) {
    player_npad_state(0)
}

/// The current Npad state for `player` (0..7): (NpadButtonSet bits, [left x, left y, right x, right y]).
pub fn player_npad_state(player: usize) -> (u64, [i32; 4]) {
    if player < MAX_PLAYERS {
        (
            NPAD_BUTTONS[player].load(Ordering::Acquire),
            std::array::from_fn(|i| NPAD_STICKS[player][i].load(Ordering::Acquire)),
        )
    } else {
        (0, [0; 4])
    }
}

pub fn is_player_connected(player: usize) -> bool {
    if player < MAX_PLAYERS {
        PLAYER_CONNECTED[player].load(Ordering::Acquire)
    } else {
        false
    }
}

pub fn connected_player_count() -> usize {
    PLAYER_CONNECTED
        .iter()
        .filter(|c| c.load(Ordering::Acquire))
        .count()
        .max(1)
}

/// Touch screen (nn::hid TouchScreen): contacts from the game window (mouse on macOS, fingers on iOS) in the Switch
/// panel's 1280x720 space. Written on the main thread, read by guest threads through GetTouchScreenState.
#[derive(Clone, Copy, Debug)]
pub struct Touch {
    /// The platform's identity for the contact (UITouch address; 0 for the mouse).
    pub key: usize,
    /// The id the game sees: the lowest one not held by another contact, as on the console.
    pub finger_id: i32,
    pub x: i32,
    pub y: i32,
    /// Not yet reported to the game: the next read carries the Start attribute.
    pub fresh: bool,
    /// Lifted: the next read carries the End attribute, and the contact is gone after it. Reads, not time, settle both,
    /// so a game sees each start and release once whatever its frame rate.
    pub ended: bool,
}

pub(crate) static TOUCHES: std::sync::Mutex<Vec<Touch>> = std::sync::Mutex::new(Vec::new());

/// Begins or moves contact `key` (any id the platform gives a finger) at normalized position (0..1, 0..1, origin top
/// left). --no-touch ignores all touch input.
pub fn touch_moved(key: usize, nx: f64, ny: f64) {
    if !crate::host::live::touch() {
        return;
    }
    let (x, y) = (
        (nx.clamp(0.0, 1.0) * 1279.0).round() as i32,
        (ny.clamp(0.0, 1.0) * 719.0).round() as i32,
    );
    let mut t = TOUCHES.lock().unwrap();
    if let Some(c) = t.iter_mut().find(|c| c.key == key && !c.ended) {
        (c.x, c.y) = (x, y);
    } else if t.len() < 16 {
        // A lifted contact keeps its finger id until the game has read its release.
        let finger_id = (0..)
            .find(|i| !t.iter().any(|c| c.finger_id == *i))
            .unwrap_or(0);
        t.push(Touch { key, finger_id, x, y, fresh: true, ended: false });
    }
}

/// Ends contact `key`.
pub fn touch_ended(key: usize) {
    if let Some(c) = TOUCHES.lock().unwrap().iter_mut().find(|c| c.key == key && !c.ended) {
        c.ended = true;
    }
}

/// The contacts to report on this read, oldest first (the order nn::hid uses), with their Start (`fresh`) and End
/// (`ended`) as of this read. Reading settles them: a reported start is not fresh again, a reported release is gone.
pub fn touches() -> Vec<Touch> {
    let mut t = TOUCHES.lock().unwrap();
    let now = t.clone();
    t.retain(|c| !c.ended);
    for c in t.iter_mut() {
        c.fresh = false;
    }
    now
}
pub(crate) static MOUSE_XY: [AtomicI32; 2] = [const { AtomicI32::new(0) }; 2];
pub(crate) static MOUSE_DELTA: [AtomicI32; 2] = [const { AtomicI32::new(0) }; 2];
pub(crate) static MOUSE_WHEEL: AtomicI32 = AtomicI32::new(0);
pub(crate) static MOUSE_BUTTONS: AtomicU32 = AtomicU32::new(0);
pub(crate) static MOUSE_SAMPLE: AtomicU64 = AtomicU64::new(0);

/// The cursor moved, in the game's 1280x720 screen.
#[cfg(not(target_os = "ios"))]
pub fn mouse_moved(x: i32, y: i32, dx: i32, dy: i32) {
    MOUSE_XY[0].store(x, Ordering::Relaxed);
    MOUSE_XY[1].store(y, Ordering::Relaxed);
    MOUSE_DELTA[0].fetch_add(dx, Ordering::Relaxed);
    MOUSE_DELTA[1].fetch_add(dy, Ordering::Relaxed);
}

/// A mouse button went down or up: its bit of nn::hid::MouseButtonSet (left 0, right 1, middle 2, forward 3, back 4).
#[cfg(not(target_os = "ios"))]
pub fn mouse_button(bit: u32, down: bool) {
    if down {
        MOUSE_BUTTONS.fetch_or(1 << bit, Ordering::Relaxed);
    } else {
        MOUSE_BUTTONS.fetch_and(!(1 << bit), Ordering::Relaxed);
    }
}

#[cfg(not(target_os = "ios"))]
pub fn mouse_wheel(delta: i32) {
    MOUSE_WHEEL.fetch_add(delta, Ordering::Relaxed);
}

/// nn::hid::MouseState: sampling number, x, y, the deltas and wheel since the last state, 4 reserved bytes, buttons,
/// attributes (bit 1: connected).
pub(crate) fn mouse_state(sample: u64, connected: bool, xy: [i32; 2], delta: [i32; 2], wheel: i32, buttons: u32) -> [u8; 0x28] {
    let mut state = [0u8; 0x28];
    state[0..8].copy_from_slice(&sample.to_le_bytes());
    if connected {
        for (i, v) in [xy[0], xy[1], delta[0], delta[1], wheel].iter().enumerate() {
            state[8 + i * 4..12 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        state[32..36].copy_from_slice(&buttons.to_le_bytes());
        state[36..40].copy_from_slice(&2u32.to_le_bytes());
    }
    state
}

/// What a game reading the mouse sees now: nothing connected unless direct mouse access is on. Deltas are consumed.
pub fn next_mouse_state() -> [u8; 0x28] {
    let on = crate::settings::try_get().is_some_and(|s| s.input.direct_mouse);
    let sample = MOUSE_SAMPLE.fetch_add(1, Ordering::Relaxed) + 1;
    if !on {
        return mouse_state(sample, false, [0; 2], [0; 2], 0, 0);
    }
    mouse_state(
        sample,
        true,
        std::array::from_fn(|i| MOUSE_XY[i].load(Ordering::Relaxed)),
        std::array::from_fn(|i| MOUSE_DELTA[i].swap(0, Ordering::Relaxed)),
        MOUSE_WHEEL.swap(0, Ordering::Relaxed),
        MOUSE_BUTTONS.load(Ordering::Relaxed),
    )
}

/// Keys held for nn::hid::KeyboardState, by USB HID usage. Read when the setting allows, so a key held while it is
/// turned on still counts.
pub(crate) static USB_KEYS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
pub(crate) static KEYBOARD_SAMPLE: AtomicU64 = AtomicU64::new(0);

/// A key of the host keyboard went down or up, by USB HID usage (what SDL scancodes are; macOS: usage_of_mac_key). On
/// iOS a game still sees a keyboard with nothing pressed.
#[cfg(not(target_os = "ios"))]
pub fn set_usb_key(usage: usize, down: bool) {
    if let Some(word) = USB_KEYS.get(usage / 64) {
        let bit = 1u64 << (usage % 64);
        if down {
            word.fetch_or(bit, Ordering::Relaxed);
        } else {
            word.fetch_and(!bit, Ordering::Relaxed);
        }
    }
}

/// nn::hid::KeyboardState: sampling number, modifier bits, attributes (bit 0: connected), then a bit per USB HID usage.
/// Caps, Scroll and Num Lock (modifier bits 8..10) are not reported.
pub(crate) fn keyboard_state(sample: u64, connected: bool, keys: [u64; 4]) -> [u8; 0x30] {
    let mut state = [0u8; 0x30];
    state[0..8].copy_from_slice(&sample.to_le_bytes());
    if connected {
        // Usages 224..231 are left Control, Shift, Alt, GUI, then right Control, Shift, Alt, GUI.
        let held = |usage: u32| (keys[3] >> (usage - 192)) as u32 & 1;
        let modifiers = (held(224) | held(228))
            | (held(225) | held(229)) << 1
            | held(226) << 2
            | held(230) << 3
            | (held(227) | held(231)) << 4;
        state[8..12].copy_from_slice(&modifiers.to_le_bytes());
        state[12..16].copy_from_slice(&1u32.to_le_bytes());
        for (i, word) in keys.iter().enumerate() {
            state[16 + i * 8..24 + i * 8].copy_from_slice(&word.to_le_bytes());
        }
    }
    state
}

/// What a game reading the keyboard sees now: nothing connected unless direct keyboard access is on.
pub fn next_keyboard_state() -> [u8; 0x30] {
    let on = crate::settings::try_get().is_some_and(|s| s.input.direct_keyboard);
    let keys = std::array::from_fn(|i| USB_KEYS[i].load(Ordering::Relaxed));
    keyboard_state(KEYBOARD_SAMPLE.fetch_add(1, Ordering::Relaxed) + 1, on, keys)
}

pub fn set_key(code: u16, down: bool) {
    if let Some(k) = KEYS.get(code as usize) {
        k.store(down, Ordering::Relaxed);
        update_npad();
    }
}

pub fn clear_keys() {
    MOUSE_BUTTONS.store(0, Ordering::Relaxed);
    for word in &USB_KEYS {
        word.store(0, Ordering::Relaxed);
    }
    if KEYS.iter().any(|k| k.swap(false, Ordering::Relaxed)) {
        update_npad();
    }
}

#[cfg(target_os = "macos")]
pub fn set_suspended(s: bool) {
    SUSPENDED.store(s, Ordering::Relaxed);
    update_npad();
}

#[cfg_attr(target_os = "ios", allow(dead_code))] // the macOS controls window
pub fn pad_value(e: PadElem) -> f32 {
    player_pad_value(0, e)
}

pub fn player_pad_value(player: usize, e: PadElem) -> f32 {
    if player < MAX_PLAYERS {
        f32::from_bits(PLAYER_PADS[player][e.index()].load(Ordering::Relaxed))
    } else {
        0.0
    }
}
