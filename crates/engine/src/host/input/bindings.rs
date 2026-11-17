// Bindings: the Switch inputs, keyboard and controller bindings, the controls file.

use super::*;

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
    #[cfg(any(target_os = "macos", windows))]
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
    pub(crate) fn button_bit(self) -> Option<u32> {
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
    #[cfg(any(target_os = "macos", windows))]
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

pub(crate) static BINDINGS: Mutex<Option<Bindings>> = Mutex::new(None);

pub fn bindings() -> Bindings {
    let mut g = BINDINGS.lock().unwrap();
    *g.get_or_insert_with(load)
}

/// Replaces the bindings, saves them and refreshes the Npad state.
#[cfg(any(target_os = "macos", windows))]
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

/// The input whose binding the next key, mouse button or controller button sets (Input > Controls, Windows), as its
/// index in SWITCH_INPUTS; -1: none.
#[cfg(windows)]
pub(crate) static CAPTURE: AtomicI32 = AtomicI32::new(-1);

/// Starts setting the binding of SWITCH_INPUTS[`i`]: the window's next key, mouse button or controller button.
#[cfg(windows)]
pub fn begin_capture(i: usize) {
    clear_keys();
    CAPTURE.store(i as i32, Ordering::Relaxed);
    println!("[+] Input: press a key or controller button for {} (Esc cancels, Delete clears)", SWITCH_INPUTS[i].label());
}

/// The input being bound, if one is.
#[cfg(windows)]
pub fn capturing() -> Option<SwitchInput> {
    usize::try_from(CAPTURE.load(Ordering::Relaxed)).ok().and_then(|i| SWITCH_INPUTS.get(i).copied())
}

/// What a capture ends with: a key, a controller element, the binding cleared, or nothing changed.
#[cfg(windows)]
pub enum Captured {
    Key(u16),
    Pad(PadElem),
    Clear,
    Cancel,
}

/// Ends the capture: sets (and saves) the binding as `c` says.
#[cfg(windows)]
pub fn end_capture(c: Captured) {
    let Some(input) = capturing() else { return };
    CAPTURE.store(-1, Ordering::Relaxed);
    let mut b = bindings();
    let slot = &mut b[input.index()];
    match c {
        Captured::Key(k) => slot.key = Some(k),
        Captured::Pad(p) => slot.pad = Some(p),
        Captured::Clear => *slot = Binding { key: None, pad: None },
        Captured::Cancel => return println!("[+] Input: {} unchanged", input.label()),
    }
    println!("[+] Input: {} is now {}", input.label(), binding_label(*slot));
    set_bindings(b);
}

/// A binding as the Controls menu shows it: "Z / Bottom button (A / ✕)".
#[cfg(not(target_os = "ios"))]
#[cfg_attr(not(windows), allow(dead_code))] // the Windows menu lists the bindings; macOS has the Controls window
pub fn binding_label(b: Binding) -> String {
    let key = b.key.map_or("no key", key_name);
    match b.pad {
        Some(p) => format!("{key} / {}", p.label()),
        None => key.to_string(),
    }
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

pub(crate) fn load() -> Bindings {
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
                Some(("key", v)) => {
                    let mouse = MOUSE_KEYS.iter().find(|(_, n)| n.eq_ignore_ascii_case(v)).map(|&(c, _)| c);
                    slot.key = mouse.or_else(|| v.parse().ok()).or(slot.key);
                }
                Some(("pad", "none")) => slot.pad = None,
                Some(("pad", v)) => slot.pad = PadElem::from_id(v).or(slot.pad),
                _ => println!("[!] Input: {}: ignoring '{}'", path.display(), kv),
            }
        }
    }
    println!("[+] Input: controls loaded from {}", path.display());
    b
}

#[cfg(any(target_os = "macos", windows))]
pub(crate) fn save(b: &Bindings) -> std::io::Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let app_name = crate::profile::app_title();
    let editor = if cfg!(windows) { "Input > Controls in the menu bar" } else { "the Controls window (Cmd-,)" };
    let mut out = format!("# {app_name} (native) controls, edited by {editor}.\n");
    out.push_str("# <input> key=<macOS virtual key code | none> pad=<controller element | none>\n");
    for s in SWITCH_INPUTS {
        let bd = b[s.index()];
        let key = bd.key.map_or("none".to_string(), |k| {
            MOUSE_KEYS.iter().find(|&&(c, _)| c == k).map_or_else(|| k.to_string(), |&(_, n)| n.to_string())
        });
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
