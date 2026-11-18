// Key names for the controls file and the Controls window.

use super::*;

// ---- key names ----

/// The macOS virtual key code (Carbon kVK_*, what controls.cfg stores) of the key at USB HID usage `usage` (what SDL scancodes are):
/// the key in the same place on a US keyboard. PC keys without a Mac counterpart take the Mac key in their place (Alt is
/// Option, the Windows key Command, Insert is Help, Num Lock the keypad's Clear).
pub fn mac_key_of_usage(sc: i32) -> Option<u16> {
    Some(match sc {
        // Letters A..Z (4..29).
        4..=29 => [0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6][(sc - 4) as usize],
        // Digits 1..9, 0 (30..39).
        30..=39 => [18, 19, 20, 21, 23, 22, 26, 28, 25, 29][(sc - 30) as usize],
        40 => 36, // Return
        41 => 53, // Escape
        42 => 51, // Backspace (Delete)
        43 => 48, // Tab
        44 => 49, // Space
        45 => 27, // -
        46 => 24, // =
        47 => 33, // [
        48 => 30, // ]
        49 | 50 => 42, // \ (and the ISO key beside Return)
        51 => 41,  // ;
        52 => 39,  // '
        53 => 50,  // `
        54 => 43,  // ,
        55 => 47,  // .
        56 => 44,  // /
        57 => 57,  // Caps Lock
        // F1..F12 (58..69).
        58..=69 => [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111][(sc - 58) as usize],
        73 => 114, // Insert (Help)
        74 => 115, // Home
        75 => 116, // Page Up
        76 => 117, // Delete (Forward Delete)
        77 => 119, // End
        78 => 121, // Page Down
        79 => 124, // →
        80 => 123, // ←
        81 => 125, // ↓
        82 => 126, // ↑
        83 => 71,  // Num Lock (Keypad Clear)
        84 => 75,  // Keypad /
        85 => 67,  // Keypad *
        86 => 78,  // Keypad -
        87 => 69,  // Keypad +
        88 => 76,  // Keypad Enter
        // Keypad 1..9, 0 (89..98).
        89..=98 => [83, 84, 85, 86, 87, 88, 89, 91, 92, 82][(sc - 89) as usize],
        99 => 65,  // Keypad .
        100 => 10, // the ISO key beside left Shift (kVK_ISO_Section)
        103 => 81, // Keypad =
        // F13..F20 (104..111).
        104..=111 => [105, 107, 113, 106, 64, 79, 80, 90][(sc - 104) as usize],
        224 => 59, // left Control
        225 => 56, // left Shift
        226 => 58, // left Alt (Option)
        227 => 55, // left Windows (Command)
        228 => 62, // right Control
        229 => 60, // right Shift
        230 => 61, // right Alt (Option)
        231 => 54, // right Windows (Command)
        _ => return None,
    })
}

/// The USB HID usage of macOS virtual key code `code` (the inverse of mac_key_of_usage), for nn::hid's keyboard state.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn usage_of_mac_key(code: u16) -> Option<usize> {
    static TABLE: std::sync::LazyLock<[u8; 128]> = std::sync::LazyLock::new(|| {
        let mut t = [0u8; 128];
        for usage in (4..=231).rev() {
            if let Some(c) = mac_key_of_usage(usage).filter(|&c| (c as usize) < t.len()) {
                t[c as usize] = usage as u8;
            }
        }
        t
    });
    TABLE.get(code as usize).copied().filter(|&u| u != 0).map(usize::from)
}

/// Display name of a macOS virtual key code (Carbon kVK_* values, US layout).
#[cfg(any(target_os = "macos", windows))]
pub fn key_name(code: u16) -> &'static str {
    match code {
        0 => "A",
        1 => "S",
        2 => "D",
        3 => "F",
        4 => "H",
        5 => "G",
        6 => "Z",
        7 => "X",
        8 => "C",
        9 => "V",
        11 => "B",
        12 => "Q",
        13 => "W",
        14 => "E",
        15 => "R",
        16 => "Y",
        17 => "T",
        18 => "1",
        19 => "2",
        20 => "3",
        21 => "4",
        22 => "6",
        23 => "5",
        24 => "=",
        25 => "9",
        26 => "7",
        27 => "-",
        28 => "8",
        29 => "0",
        30 => "]",
        31 => "O",
        32 => "U",
        33 => "[",
        34 => "I",
        35 => "P",
        36 => "Return",
        37 => "L",
        38 => "J",
        39 => "'",
        40 => "K",
        41 => ";",
        42 => "\\",
        43 => ",",
        44 => "/",
        45 => "N",
        46 => "M",
        47 => ".",
        48 => "Tab",
        49 => "Space",
        50 => "`",
        51 => "Delete",
        53 => "Esc",
        54 => "Right ⌘",
        55 => "⌘",
        56 => "Shift",
        57 => "Caps Lock",
        58 => "Option",
        59 => "Control",
        60 => "Right Shift",
        61 => "Right Option",
        62 => "Right Control",
        63 => "fn",
        65 => "Keypad .",
        67 => "Keypad *",
        69 => "Keypad +",
        71 => "Keypad Clear",
        75 => "Keypad /",
        76 => "Keypad Enter",
        78 => "Keypad -",
        81 => "Keypad =",
        82 => "Keypad 0",
        83 => "Keypad 1",
        84 => "Keypad 2",
        85 => "Keypad 3",
        86 => "Keypad 4",
        87 => "Keypad 5",
        88 => "Keypad 6",
        89 => "Keypad 7",
        91 => "Keypad 8",
        92 => "Keypad 9",
        96 => "F5",
        97 => "F6",
        98 => "F7",
        99 => "F3",
        100 => "F8",
        101 => "F9",
        103 => "F11",
        105 => "F13",
        106 => "F16",
        107 => "F14",
        109 => "F10",
        111 => "F12",
        113 => "F15",
        114 => "Help",
        115 => "Home",
        116 => "Page Up",
        117 => "Forward Delete",
        118 => "F4",
        119 => "End",
        120 => "F2",
        121 => "Page Down",
        122 => "F1",
        123 => "←",
        124 => "→",
        125 => "↓",
        126 => "↑",
        128 => "Mouse Left",
        129 => "Mouse Right",
        130 => "Mouse Middle",
        131 => "Mouse Forward",
        132 => "Mouse Back",
        _ => "?",
    }
}

/// For a modifier key's flagsChanged event: whether that key is now down (device-dependent flag bits, so left and
/// right modifiers are told apart). None for other keys.
#[cfg(target_os = "macos")]
pub fn modifier_down(code: u16, flags: usize) -> Option<bool> {
    let bit = match code {
        56 => 0x2,     // left shift
        60 => 0x4,     // right shift
        59 => 0x1,     // left control
        62 => 0x2000,  // right control
        58 => 0x20,    // left option
        61 => 0x40,    // right option
        55 => 0x8,     // left command
        54 => 0x10,    // right command
        57 => 1 << 16, // caps lock (toggle state)
        63 => 1 << 23, // fn
        _ => return None,
    };
    Some(flags & bit != 0)
}

// Motion (nn::hid six-axis sensor states): a controller's gyroscope and accelerometer, fused into an orientation. Hosts
// feed readings in the Switch's axes (the controller flat in front of the player: X right, Y forward, Z up out of its
// face): acceleration in G (flat: 0, 0, -1), the gyroscope in radians per second. The gyroscope turns the orientation,
// and the accelerometer, when it reads about 1 G (the controller not being shaken), pulls its tilt toward gravity a
// little each reading, so the tilt does not drift. Yaw has nothing to correct it, as on the Switch.

/// How much of the tilt error one accelerometer reading corrects (a tuning number; the gyroscope leads).
#[cfg_attr(target_vendor = "apple", allow(dead_code))]
pub(crate) const TILT_CORRECTION: f64 = 0.02;

#[derive(Clone, Copy)]
#[cfg_attr(target_vendor = "apple", allow(dead_code))]
pub(crate) struct Motion {
    /// The controller's orientation (w, x, y, z): its axes into the world's (Z up).
    q: [f64; 4],
    /// The newest gyroscope reading (rad/s) and accelerometer reading (G).
    gyro: [f64; 3],
    acc: [f64; 3],
    /// The gyroscope's turn summed, in turns.
    angle: [f64; 3],
    /// The newest gyroscope reading's time, ns.
    gyro_ns: u64,
}

pub(crate) static MOTION: [Mutex<Option<Motion>>; MAX_PLAYERS] = [const { Mutex::new(None) }; MAX_PLAYERS];

#[cfg_attr(target_vendor = "apple", allow(dead_code))]
pub(crate) fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

#[cfg_attr(target_vendor = "apple", allow(dead_code))]
pub(crate) fn qnorm(q: [f64; 4]) -> [f64; 4] {
    let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    if n > 0.0 { q.map(|v| v / n) } else { [1.0, 0.0, 0.0, 0.0] }
}

/// The rotation by rotation vector `v` (axis times angle, radians).
#[cfg_attr(target_vendor = "apple", allow(dead_code))]
pub(crate) fn qrot(v: [f64; 3]) -> [f64; 4] {
    let a = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if a < 1e-12 {
        return [1.0, 0.0, 0.0, 0.0];
    }
    let s = (a / 2.0).sin() / a;
    [(a / 2.0).cos(), v[0] * s, v[1] * s, v[2] * s]
}

/// The rotation matrix of `q`: from the controller's axes into the world's (its columns are the controller's X, Y and
/// Z axes in the world).
pub(crate) fn qmat(q: [f64; 4]) -> [[f64; 3]; 3] {
    let [w, x, y, z] = q;
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// A gyroscope reading for `player` at `ns`: rad/s in the Switch's axes.
#[cfg_attr(target_vendor = "apple", allow(dead_code))]
pub fn motion_gyro(player: usize, w: [f64; 3], ns: u64) {
    let Some(slot) = MOTION.get(player) else { return };
    let mut g = slot.lock().unwrap();
    let m = g.get_or_insert(Motion { q: [1.0, 0.0, 0.0, 0.0], gyro: [0.0; 3], acc: [0.0, 0.0, -1.0], angle: [0.0; 3], gyro_ns: ns });
    let dt = if ns > m.gyro_ns { ((ns - m.gyro_ns) as f64 / 1e9).min(0.1) } else { 0.0 };
    m.gyro_ns = ns;
    m.gyro = w;
    m.q = qnorm(qmul(m.q, qrot(w.map(|v| v * dt))));
    for i in 0..3 {
        m.angle[i] += w[i] * dt / std::f64::consts::TAU;
    }
}

/// An accelerometer reading for `player`: G in the Switch's axes.
#[cfg_attr(target_vendor = "apple", allow(dead_code))]
pub fn motion_accel(player: usize, a: [f64; 3]) {
    let Some(slot) = MOTION.get(player) else { return };
    let mut g = slot.lock().unwrap();
    let Some(m) = g.as_mut() else { return };
    m.acc = a;
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if !(0.9..1.1).contains(&n) {
        return;
    }
    // Gravity where the orientation says it is, in the controller's axes, against where the accelerometer reads it:
    // turn by a share of the angle between them.
    let r = qmat(m.q);
    let down = [-r[2][0], -r[2][1], -r[2][2]];
    let meas = a.map(|v| v / n);
    let axis = [meas[1] * down[2] - meas[2] * down[1], meas[2] * down[0] - meas[0] * down[2], meas[0] * down[1] - meas[1] * down[0]];
    m.q = qnorm(qmul(m.q, qrot(axis.map(|v| v * TILT_CORRECTION))));
}

/// A controller with motion went away.
#[cfg_attr(target_vendor = "apple", allow(dead_code))]
pub fn motion_clear(player: usize) {
    if let Some(slot) = MOTION.get(player) {
        *slot.lock().unwrap() = None;
    }
}

/// `player`'s motion as nn::hid's SixAxisSensorState has it: acceleration (G), angular velocity (turns per second),
/// angle (turns), direction (the controller's X, Y and Z axes in the world, Z up). None without a motion source.
pub fn motion_state(player: usize) -> Option<([f32; 3], [f32; 3], [f32; 3], [f32; 9])> {
    let m = (*MOTION.get(player)?.lock().unwrap())?;
    let r = qmat(m.q);
    let dir = [r[0][0], r[1][0], r[2][0], r[0][1], r[1][1], r[2][1], r[0][2], r[1][2], r[2][2]].map(|v| v as f32);
    Some((m.acc.map(|v| v as f32), m.gyro.map(|v| (v / std::f64::consts::TAU) as f32), m.angle.map(|v| v as f32), dir))
}
