// Player input: keyboard and game controllers mapped onto one Switch Npad (nn::hid::NpadFullKeyState).
//
// Every Switch input (buttons, +Control Pad, each stick direction) has one keyboard binding and one controller
// binding; both are active at once. Bindings are edited in the Controls window (host/controls_ui.rs, Cmd-,) and saved to
// ~/Library/Application Support/nxport/controls.cfg (--controls-file overrides the path). Windows reads the same file
// from %APPDATA%\nxport\controls.cfg: keys are macOS virtual key codes there too (host/display_sdl.rs translates SDL's
// scancodes into them), so a controls file means the same keys on both.
//
// Threading: AppKit and GameController objects are only touched on the main thread (keyboard events arrive there; a
// 250 Hz timer polls controllers), and guest threads must not call into Objective-C (they clobber thread-local state
// libobjc relies on). The main thread therefore folds keyboard + controller state through the bindings into a finished
// Npad state kept in atomics, which nn::hid::GetNpadStates (horizon/hle.rs) reads. On Windows SDL's event loop on the
// main thread does the same (host/display_sdl.rs).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

/// The Switch inputs a binding can drive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchInput {
    A,
    B,
    X,
    Y,
    L,
    R,
    ZL,
    ZR,
    Plus,
    Minus,
    StickL,
    StickR,
    DUp,
    DDown,
    DLeft,
    DRight,
    LUp,
    LDown,
    LLeft,
    LRight,
    RUp,
    RDown,
    RLeft,
    RRight,
}

pub const SWITCH_INPUTS: [SwitchInput; 24] = {
    use SwitchInput::*;
    [
        A, B, X, Y, L, R, ZL, ZR, Plus, Minus, StickL, StickR, DUp, DDown, DLeft, DRight, LUp,
        LDown, LLeft, LRight, RUp, RDown, RLeft, RRight,
    ]
};

impl SwitchInput {
    pub fn index(self) -> usize {
        SWITCH_INPUTS.iter().position(|&s| s == self).unwrap()
    }
    /// Name in the config file.
    pub fn id(self) -> &'static str {
        use SwitchInput::*;
        match self {
            A => "A",
            B => "B",
            X => "X",
            Y => "Y",
            L => "L",
            R => "R",
            ZL => "ZL",
            ZR => "ZR",
            Plus => "Plus",
            Minus => "Minus",
            StickL => "StickL",
            StickR => "StickR",
            DUp => "DpadUp",
            DDown => "DpadDown",
            DLeft => "DpadLeft",
            DRight => "DpadRight",
            LUp => "LStickUp",
            LDown => "LStickDown",
            LLeft => "LStickLeft",
            LRight => "LStickRight",
            RUp => "RStickUp",
            RDown => "RStickDown",
            RLeft => "RStickLeft",
            RRight => "RStickRight",
        }
    }
    /// Name shown in the Controls window.
    #[cfg(target_os = "macos")]
    pub fn label(self) -> &'static str {
        use SwitchInput::*;
        match self {
            A => "A",
            B => "B",
            X => "X",
            Y => "Y",
            L => "L",
            R => "R",
            ZL => "ZL",
            ZR => "ZR",
            Plus => "+ (Plus)",
            Minus => "− (Minus)",
            StickL => "Left stick press",
            StickR => "Right stick press",
            DUp => "D-pad up",
            DDown => "D-pad down",
            DLeft => "D-pad left",
            DRight => "D-pad right",
            LUp => "Left stick up",
            LDown => "Left stick down",
            LLeft => "Left stick left",
            LRight => "Left stick right",
            RUp => "Right stick up",
            RDown => "Right stick down",
            RLeft => "Right stick left",
            RRight => "Right stick right",
        }
    }
    /// NpadButton bit for inputs that are buttons; sticks are analog.
    fn button_bit(self) -> Option<u32> {
        use SwitchInput::*;
        Some(match self {
            A => 0,
            B => 1,
            X => 2,
            Y => 3,
            StickL => 4,
            StickR => 5,
            L => 6,
            R => 7,
            ZL => 8,
            ZR => 9,
            Plus => 10,
            Minus => 11,
            DLeft => 12,
            DUp => 13,
            DRight => 14,
            DDown => 15,
            _ => return None,
        })
    }
}

/// Controller elements (GCExtendedGamepad), each read as a value in 0..=1 (a stick direction is its deflection that way).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PadElem {
    South,
    East,
    West,
    North,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    Menu,
    Options,
    Home,
    LeftStickButton,
    RightStickButton,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LeftStickUp,
    LeftStickDown,
    LeftStickLeft,
    LeftStickRight,
    RightStickUp,
    RightStickDown,
    RightStickLeft,
    RightStickRight,
}

pub const PAD_ELEMS: [PadElem; 25] = {
    use PadElem::*;
    [
        South,
        East,
        West,
        North,
        LeftShoulder,
        RightShoulder,
        LeftTrigger,
        RightTrigger,
        Menu,
        Options,
        Home,
        LeftStickButton,
        RightStickButton,
        DpadUp,
        DpadDown,
        DpadLeft,
        DpadRight,
        LeftStickUp,
        LeftStickDown,
        LeftStickLeft,
        LeftStickRight,
        RightStickUp,
        RightStickDown,
        RightStickLeft,
        RightStickRight,
    ]
};

impl PadElem {
    pub fn index(self) -> usize {
        PAD_ELEMS.iter().position(|&e| e == self).unwrap()
    }
    pub fn id(self) -> &'static str {
        use PadElem::*;
        match self {
            South => "South",
            East => "East",
            West => "West",
            North => "North",
            LeftShoulder => "LeftShoulder",
            RightShoulder => "RightShoulder",
            LeftTrigger => "LeftTrigger",
            RightTrigger => "RightTrigger",
            Menu => "Menu",
            Options => "Options",
            Home => "Home",
            LeftStickButton => "LeftStickButton",
            RightStickButton => "RightStickButton",
            DpadUp => "DpadUp",
            DpadDown => "DpadDown",
            DpadLeft => "DpadLeft",
            DpadRight => "DpadRight",
            LeftStickUp => "LeftStickUp",
            LeftStickDown => "LeftStickDown",
            LeftStickLeft => "LeftStickLeft",
            LeftStickRight => "LeftStickRight",
            RightStickUp => "RightStickUp",
            RightStickDown => "RightStickDown",
            RightStickLeft => "RightStickLeft",
            RightStickRight => "RightStickRight",
        }
    }
    #[cfg(target_os = "macos")]
    pub fn label(self) -> &'static str {
        use PadElem::*;
        match self {
            South => "Bottom button (A / ✕)",
            East => "Right button (B / ○)",
            West => "Left button (X / □)",
            North => "Top button (Y / △)",
            LeftShoulder => "Left bumper (LB / L1)",
            RightShoulder => "Right bumper (RB / R1)",
            LeftTrigger => "Left trigger (LT / L2)",
            RightTrigger => "Right trigger (RT / R2)",
            Menu => "Menu / Start",
            Options => "View / Select",
            Home => "Home / Guide",
            LeftStickButton => "Left stick press",
            RightStickButton => "Right stick press",
            DpadUp => "D-pad up",
            DpadDown => "D-pad down",
            DpadLeft => "D-pad left",
            DpadRight => "D-pad right",
            LeftStickUp => "Left stick up",
            LeftStickDown => "Left stick down",
            LeftStickLeft => "Left stick left",
            LeftStickRight => "Left stick right",
            RightStickUp => "Right stick up",
            RightStickDown => "Right stick down",
            RightStickLeft => "Right stick left",
            RightStickRight => "Right stick right",
        }
    }
    fn from_id(s: &str) -> Option<PadElem> {
        PAD_ELEMS
            .iter()
            .copied()
            .find(|e| e.id().eq_ignore_ascii_case(s))
    }
}

/// One Switch input's bindings: a macOS virtual key code and a controller element (either may be unset).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    pub key: Option<u16>,
    pub pad: Option<PadElem>,
}

pub type Bindings = [Binding; 24];

/// Defaults: the keyboard layout follows Ryujinx's (sticks on WASD / IJKL, face buttons on Z X C V); controller face
/// buttons are positional (Switch A is the right button, B the bottom one), as on a Switch Pro Controller.
pub fn default_bindings() -> Bindings {
    use PadElem as P;
    use SwitchInput::*;
    let mut b = [Binding {
        key: None,
        pad: None,
    }; 24];
    let mut set = |s: SwitchInput, key: u16, pad: PadElem| {
        b[s.index()] = Binding {
            key: Some(key),
            pad: Some(pad),
        }
    };
    set(A, 6, P::East); // Z
    set(B, 7, P::South); // X
    set(X, 8, P::North); // C
    set(Y, 9, P::West); // V
    set(L, 14, P::LeftShoulder); // E
    set(R, 32, P::RightShoulder); // U
    set(ZL, 12, P::LeftTrigger); // Q
    set(ZR, 31, P::RightTrigger); // O
    set(Plus, 24, P::Menu); // =
    set(Minus, 27, P::Options); // -
    set(StickL, 3, P::LeftStickButton); // F
    set(StickR, 4, P::RightStickButton); // H
    set(DUp, 126, P::DpadUp);
    set(DDown, 125, P::DpadDown);
    set(DLeft, 123, P::DpadLeft);
    set(DRight, 124, P::DpadRight);
    set(LUp, 13, P::LeftStickUp); // W
    set(LDown, 1, P::LeftStickDown); // S
    set(LLeft, 0, P::LeftStickLeft); // A
    set(LRight, 2, P::LeftStickRight); // D
    set(RUp, 34, P::RightStickUp); // I
    set(RDown, 40, P::RightStickDown); // K
    set(RLeft, 38, P::RightStickLeft); // J
    set(RRight, 37, P::RightStickRight); // L
    b
}

static BINDINGS: Mutex<Option<Bindings>> = Mutex::new(None);

pub fn bindings() -> Bindings {
    let mut g = BINDINGS.lock().unwrap();
    *g.get_or_insert_with(load)
}

/// Replaces the bindings, saves them and refreshes the Npad state.
#[cfg(target_os = "macos")]
pub fn set_bindings(b: Bindings) {
    *BINDINGS.lock().unwrap() = Some(b);
    if let Err(e) = save(&b) {
        println!(
            "[!] Input: could not save controls to {}: {}",
            config_path().display(),
            e
        );
    }
    update_npad();
}

pub fn config_path() -> std::path::PathBuf {
    if let Some(p) = &crate::settings::get().input.controls_file {
        return p.clone();
    }
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return std::path::Path::new(&appdata).join("nxport").join("controls.cfg");
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    std::path::Path::new(&home).join("Library/Application Support/nxport/controls.cfg")
}

fn load() -> Bindings {
    let mut b = default_bindings();
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        println!(
            "[+] Input: no {} yet; using default controls",
            path.display()
        );
        return b;
    };
    for line in text.lines() {
        // Comments run to the end of the line (the Controls window writes each key's label as one).
        let line = line.split_once('#').map_or(line, |(l, _)| l).trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else { continue };
        let Some(input) = SWITCH_INPUTS
            .iter()
            .copied()
            .find(|s| s.id().eq_ignore_ascii_case(name))
        else {
            println!("[!] Input: {}: unknown input '{}'", path.display(), name);
            continue;
        };
        let slot = &mut b[input.index()];
        for kv in parts {
            match kv.split_once('=') {
                Some(("key", "none")) => slot.key = None,
                Some(("key", v)) => slot.key = v.parse().ok().or(slot.key),
                Some(("pad", "none")) => slot.pad = None,
                Some(("pad", v)) => slot.pad = PadElem::from_id(v).or(slot.pad),
                _ => println!("[!] Input: {}: ignoring '{}'", path.display(), kv),
            }
        }
    }
    println!("[+] Input: controls loaded from {}", path.display());
    b
}

#[cfg(target_os = "macos")]
fn save(b: &Bindings) -> std::io::Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let app_name = crate::profile::app_title();
    let mut out =
        format!("# {app_name} (native) controls, edited by the Controls window (Cmd-,).\n");
    out.push_str("# <input> key=<macOS virtual key code | none> pad=<controller element | none>\n");
    for s in SWITCH_INPUTS {
        let bd = b[s.index()];
        let key = bd.key.map_or("none".to_string(), |k| k.to_string());
        let pad = bd.pad.map_or("none", |p| p.id());
        out.push_str(&format!(
            "{:<12} key={:<4} pad={}   # {}\n",
            s.id(),
            key,
            pad,
            bd.key.map_or("", key_name)
        ));
    }
    std::fs::write(&path, out)
}

// ---- live state (written on the main thread, read by nn::hid) ----

pub const MAX_PLAYERS: usize = 8;

static KEYS: [AtomicBool; 128] = [const { AtomicBool::new(false) }; 128];
static PLAYER_PADS: [[AtomicU32; 25]; MAX_PLAYERS] =
    [const { [const { AtomicU32::new(0) }; 25] }; MAX_PLAYERS];
static PLAYER_CONNECTED: [AtomicBool; MAX_PLAYERS] = {
    let mut arr = [const { AtomicBool::new(false) }; MAX_PLAYERS];
    arr[0] = AtomicBool::new(true);
    arr
};
/// While the Controls window captures a binding, the game sees no input.
static SUSPENDED: AtomicBool = AtomicBool::new(false);
/// Physical controllers only: A/B and X/Y by position (as on a Switch controller: the bottom button is B, the left one Y)
/// instead of by label. The on-screen controller always goes by its labels.
static SWAP_AB: AtomicBool = AtomicBool::new(false);
static SWAP_XY: AtomicBool = AtomicBool::new(false);

#[cfg_attr(target_os = "macos", allow(dead_code))] // the Windows menu bar and the iOS settings change them
pub fn set_controller_swaps(ab: bool, xy: bool) {
    SWAP_AB.store(ab, Ordering::Relaxed);
    SWAP_XY.store(xy, Ordering::Relaxed);
}

static NPAD_BUTTONS: [AtomicU64; MAX_PLAYERS] = [const { AtomicU64::new(0) }; MAX_PLAYERS];
static NPAD_STICKS: [[AtomicI32; 4]; MAX_PLAYERS] =
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
    pub began: std::time::Instant,
    /// When the contact was lifted: it is still reported (with the End attribute) for a short while so a game polling
    /// at its frame rate sees the release.
    pub ended: Option<std::time::Instant>,
}

/// A lifted contact stays visible this long (a 30 fps game polls every ~33 ms).
pub const TOUCH_END_HOLD: std::time::Duration = std::time::Duration::from_millis(50);

static TOUCHES: std::sync::Mutex<Vec<Touch>> = std::sync::Mutex::new(Vec::new());

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
    let now = std::time::Instant::now();
    t.retain(|c| {
        c.ended
            .is_none_or(|e| now.duration_since(e) < TOUCH_END_HOLD)
    });
    if let Some(c) = t.iter_mut().find(|c| c.key == key && c.ended.is_none()) {
        (c.x, c.y) = (x, y);
    } else if t.len() < 16 {
        let finger_id = (0..)
            .find(|i| !t.iter().any(|c| c.finger_id == *i))
            .unwrap_or(0);
        t.push(Touch {
            key,
            finger_id,
            x,
            y,
            began: now,
            ended: None,
        });
    }
}

/// Ends contact `key`.
pub fn touch_ended(key: usize) {
    if let Some(c) = TOUCHES
        .lock()
        .unwrap()
        .iter_mut()
        .find(|c| c.key == key && c.ended.is_none())
    {
        c.ended = Some(std::time::Instant::now());
    }
}

/// The contacts to report now, oldest first (the order nn::hid uses).
pub fn touches() -> Vec<Touch> {
    let now = std::time::Instant::now();
    let mut t = TOUCHES.lock().unwrap();
    t.retain(|c| {
        c.ended
            .is_none_or(|e| now.duration_since(e) < TOUCH_END_HOLD)
    });
    t.clone()
}

static MOUSE_XY: [AtomicI32; 2] = [const { AtomicI32::new(0) }; 2];
static MOUSE_DELTA: [AtomicI32; 2] = [const { AtomicI32::new(0) }; 2];
static MOUSE_WHEEL: AtomicI32 = AtomicI32::new(0);
static MOUSE_BUTTONS: AtomicU32 = AtomicU32::new(0);
static MOUSE_SAMPLE: AtomicU64 = AtomicU64::new(0);

/// The cursor moved, in the game's 1280x720 screen. Only the SDL hosts have this so far.
#[cfg(not(target_vendor = "apple"))]
pub fn mouse_moved(x: i32, y: i32, dx: i32, dy: i32) {
    MOUSE_XY[0].store(x, Ordering::Relaxed);
    MOUSE_XY[1].store(y, Ordering::Relaxed);
    MOUSE_DELTA[0].fetch_add(dx, Ordering::Relaxed);
    MOUSE_DELTA[1].fetch_add(dy, Ordering::Relaxed);
}

/// A mouse button went down or up: its bit of nn::hid::MouseButtonSet (left 0, right 1, middle 2, forward 3, back 4).
#[cfg(not(target_vendor = "apple"))]
pub fn mouse_button(bit: u32, down: bool) {
    if down {
        MOUSE_BUTTONS.fetch_or(1 << bit, Ordering::Relaxed);
    } else {
        MOUSE_BUTTONS.fetch_and(!(1 << bit), Ordering::Relaxed);
    }
}

#[cfg(not(target_vendor = "apple"))]
pub fn mouse_wheel(delta: i32) {
    MOUSE_WHEEL.fetch_add(delta, Ordering::Relaxed);
}

/// nn::hid::MouseState: sampling number, x, y, the deltas and wheel since the last state, 4 reserved bytes, buttons,
/// attributes (bit 1: connected).
fn mouse_state(sample: u64, connected: bool, xy: [i32; 2], delta: [i32; 2], wheel: i32, buttons: u32) -> [u8; 0x28] {
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
static USB_KEYS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static KEYBOARD_SAMPLE: AtomicU64 = AtomicU64::new(0);

/// A key of the host keyboard went down or up, by USB HID usage (what SDL scancodes are). Only the SDL hosts have this
/// so far: on Apple devices a game still sees a keyboard with nothing pressed.
#[cfg(not(target_vendor = "apple"))]
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
fn keyboard_state(sample: u64, connected: bool, keys: [u64; 4]) -> [u8; 0x30] {
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

/// Folds keyboard and controller state through the bindings into each player's Npad state.
// ---- mouse look ----

/// Mouse look (settings input.mouse_look): mouse movement becomes the right stick. A stick reports a deflection, not a
/// movement, so the stick follows the mouse's speed: pixels per second, smoothed over ~30 ms, full deflection at
/// MOUSE_LOOK_FULL px/s times the speed setting. Small speeds are lifted past the deadzone games keep around the
/// stick's centre, so slow aiming still moves the camera. The mouse at rest centres the stick within ~50 ms.
struct MouseLook {
    pending: (f32, f32),
    smooth: (f32, f32),
    last: Option<std::time::Instant>,
}
static MOUSE_LOOK: std::sync::Mutex<MouseLook> =
    std::sync::Mutex::new(MouseLook { pending: (0.0, 0.0), smooth: (0.0, 0.0), last: None });
/// The right stick from the mouse, as two f32 bit patterns (x right, y up).
static MOUSE_STICK: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
const MOUSE_LOOK_FULL: f32 = 1500.0;
const MOUSE_LOOK_DEADZONE: f32 = 0.15;

pub(crate) fn mouse_look_on() -> bool {
    crate::settings::try_get().is_some_and(|s| s.input.mouse_look && !s.input.direct_mouse)
}

/// Relative mouse movement while mouse look holds the mouse (window pixels; y down). The SDL host (Windows) holds
/// the mouse; the macOS host does not have mouse look yet.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn mouse_look_moved(dx: f32, dy: f32) {
    let mut m = MOUSE_LOOK.lock().unwrap();
    m.pending.0 += dx;
    m.pending.1 += dy;
}

/// The stick for a mouse speed (px/s), deadzone lifted, inside the unit circle.
fn mouse_look_deflection(vx: f32, vy: f32, speed: f32) -> (f32, f32) {
    let (x, y) = (vx * speed / MOUSE_LOOK_FULL, -vy * speed / MOUSE_LOOK_FULL);
    let m = (x * x + y * y).sqrt();
    if m < 1e-3 {
        return (0.0, 0.0);
    }
    let lifted = (MOUSE_LOOK_DEADZONE + (1.0 - MOUSE_LOOK_DEADZONE) * m).min(1.0);
    (x / m * lifted, y / m * lifted)
}

/// Turns the mouse movement since the last read into the right stick. Called as the game reads its controller.
fn update_mouse_look() {
    if !mouse_look_on() {
        return;
    }
    let speed = crate::settings::try_get().map_or(1.0, |s| s.input.mouse_look_speed);
    let now = std::time::Instant::now();
    let (x, y) = {
        let mut m = MOUSE_LOOK.lock().unwrap();
        let dt = m.last.map_or(0.016, |l| now.duration_since(l).as_secs_f32()).clamp(0.001, 0.1);
        m.last = Some(now);
        let v = (m.pending.0 / dt, m.pending.1 / dt);
        m.pending = (0.0, 0.0);
        let a = 1.0 - (-dt / 0.03).exp();
        m.smooth.0 += (v.0 - m.smooth.0) * a;
        m.smooth.1 += (v.1 - m.smooth.1) * a;
        // Under a pixel per frame at 60 Hz: at rest.
        if m.smooth.0.abs() < 60.0 && m.smooth.1.abs() < 60.0 && v == (0.0, 0.0) {
            m.smooth = (0.0, 0.0);
        }
        mouse_look_deflection(m.smooth.0, m.smooth.1, speed)
    };
    let (ox, oy) = (f32::from_bits(MOUSE_STICK[0].load(Ordering::Relaxed)), f32::from_bits(MOUSE_STICK[1].load(Ordering::Relaxed)));
    if (x, y) != (ox, oy) {
        MOUSE_STICK[0].store(x.to_bits(), Ordering::Relaxed);
        MOUSE_STICK[1].store(y.to_bits(), Ordering::Relaxed);
        update_npad();
    }
}

/// Before the game reads player 1's controller: the debug input script and mouse look.
pub fn before_npad_read() {
    apply_script();
    update_mouse_look();
}

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

// ---- debug ----

/// Debug: --debug-input-script=2600-2900:13,3000-3300:34+37 holds the given key codes (here W, then I+L) while the
/// presented frame number is in each range. Applied from nn::hid reads, so it also works headless.
pub fn apply_script() {
    let script = &crate::settings::get().debug.debug_input_script;
    if script.is_empty() {
        return;
    }
    let frame = crate::gpu::backend().presented_frames();
    let mut want = [false; 128];
    for held in script {
        if (held.first..held.last).contains(&frame) {
            for &k in &held.keys {
                if (k as usize) < 128 {
                    want[k as usize] = true;
                }
            }
        }
    }
    let mut changed = false;
    for (k, w) in KEYS.iter().zip(want) {
        changed |= k.swap(w, Ordering::Relaxed) != w;
    }
    if changed {
        println!(
            "[input-script] frame {}: keys {:?}",
            frame,
            (0..128).filter(|&i| want[i]).collect::<Vec<_>>()
        );
        update_npad();
    }
}

/// Debug: --log-hid prints each distinct (function, npad id, count) the game reads, once.
pub fn log_hid(func: &str, id: u32, count: i32) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| crate::settings::get().log.log_hid) {
        return;
    }
    static SEEN: Mutex<Vec<(String, u32, i32)>> = Mutex::new(Vec::new());
    let mut s = SEEN.lock().unwrap();
    let k = (func.to_string(), id, count);
    if !s.contains(&k) {
        println!(
            "[hid] {} npad id {:#x} count {} (frame {})",
            func,
            id,
            count,
            crate::gpu::backend().presented_frames()
        );
        s.push(k);
    }
}

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
unsafe fn read_controller_elements(c: *mut objc2::runtime::AnyObject, physical: bool) -> [f32; 25] {
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

// ---- key names ----

/// Display name of a macOS virtual key code (Carbon kVK_* values, US layout).
#[cfg(target_os = "macos")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_look_deflection_follows_speed() {
        // At rest: centred.
        assert_eq!(mouse_look_deflection(0.0, 0.0, 1.0), (0.0, 0.0));
        // Slow movement still passes the games' deadzone; fast movement saturates; mouse down is stick down.
        let (x, y) = mouse_look_deflection(30.0, 0.0, 1.0);
        assert!(x > MOUSE_LOOK_DEADZONE && y == 0.0, "{x}");
        let (x, y) = mouse_look_deflection(0.0, 10_000.0, 1.0);
        assert!(x.abs() < 1e-6 && (y + 1.0).abs() < 1e-6, "{x} {y}");
        // Faster means further, and the speed setting scales it.
        assert!(mouse_look_deflection(600.0, 0.0, 1.0).0 > mouse_look_deflection(300.0, 0.0, 1.0).0);
        assert!(mouse_look_deflection(300.0, 0.0, 2.0).0 > mouse_look_deflection(300.0, 0.0, 1.0).0);
        // Diagonals stay inside the unit circle.
        let (x, y) = mouse_look_deflection(5000.0, -5000.0, 1.0);
        assert!((x * x + y * y).sqrt() <= 1.0 + 1e-5);
    }

    #[test]
    fn mouse_state_layout() {
        let s = mouse_state(9, true, [640, 360], [-3, 4], 120, 0b101);
        let word = |o: usize| i32::from_le_bytes(s[o..o + 4].try_into().unwrap());
        assert_eq!(u64::from_le_bytes(s[0..8].try_into().unwrap()), 9);
        assert_eq!([word(8), word(12), word(16), word(20), word(24)], [640, 360, -3, 4, 120]);
        assert_eq!(word(28), 0, "reserved");
        assert_eq!(word(32), 0b101);
        assert_eq!(word(36), 2, "connected");
        let off = mouse_state(9, false, [640, 360], [-3, 4], 120, 1);
        assert!(off[8..].iter().all(|b| *b == 0));
    }

    #[test]
    fn keyboard_state_layout() {
        // A (usage 4), left Shift (225), right Alt (230).
        let keys = [1 << 4, 0, 0, 1 << 33 | 1 << 38];
        let s = keyboard_state(7, true, keys);
        assert_eq!(u64::from_le_bytes(s[0..8].try_into().unwrap()), 7);
        assert_eq!(u32::from_le_bytes(s[8..12].try_into().unwrap()), 0b0_1010);
        assert_eq!(u32::from_le_bytes(s[12..16].try_into().unwrap()), 1);
        assert_eq!(s[16], 1 << 4);
        assert_eq!(s[44], 1 << 1 | 1 << 6);
    }

    #[test]
    fn keyboard_state_off_shows_no_keys() {
        let s = keyboard_state(7, false, [u64::MAX; 4]);
        assert_eq!(u64::from_le_bytes(s[0..8].try_into().unwrap()), 7);
        assert!(s[8..].iter().all(|b| *b == 0));
    }

    #[test]
    fn keyboard_and_mouse_calls_stay_in_their_buffers() {
        use crate::horizon::hle::*;
        unsafe {
            let mut buf = [0xaau8; 0x40];
            hle_nn_hid_GetKeyboardState(buf.as_mut_ptr());
            assert!(buf[0x30..].iter().all(|b| *b == 0xaa));

            // Asked for two, given one, and the second is left alone.
            let mut buf = [0xaau8; 0x70];
            let mut written = -1;
            assert_eq!(hle_nn_hid_detail_GetKeyboardStates(&mut written, buf.as_mut_ptr(), 2), 0);
            assert_eq!(written, 1);
            assert!(buf[0x30..].iter().all(|b| *b == 0xaa));

            let mut buf = [0xaau8; 0x60];
            let mut written = -1;
            assert_eq!(hle_nn_hid_detail_GetMouseStates(&mut written, buf.as_mut_ptr(), 2), 0);
            assert_eq!(written, 1);
            assert!(buf[0x28..].iter().all(|b| *b == 0xaa));
        }
    }

    #[test]
    fn test_multiplayer_player_slots() {
        assert_eq!(MAX_PLAYERS, 8);
        assert!(is_player_connected(0));
        assert!(!is_player_connected(1));
        assert!(!is_player_connected(7));
        assert!(!is_player_connected(8));
        assert_eq!(connected_player_count(), 1);

        PLAYER_CONNECTED[1].store(true, Ordering::SeqCst);
        assert!(is_player_connected(1));
        assert_eq!(connected_player_count(), 2);
        PLAYER_CONNECTED[1].store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_player_npad_state_out_of_bounds() {
        let (buttons, sticks) = player_npad_state(MAX_PLAYERS);
        assert_eq!(buttons, 0);
        assert_eq!(sticks, [0; 4]);
    }
}
