// The window on Windows, through SDL3: the process main thread creates it and runs SDL's event loop, the Vulkan backend
// presents into its HWND from the thread that submits the game's frames (vulkan/window.rs), and the guest runs on its
// own threads (lib.rs), as on macOS (host/display.rs).
//
// The event loop also does what AppKit and the 250 Hz controller timer do there: keys go to host/input.rs as macOS
// virtual key codes (the bindings in controls.cfg are those codes on every host), the left mouse button is the touch
// screen, game controllers are read every 4 ms and given the game's rumble (host/rumble_sdl.rs), and the title shows
// the presented frame rate. F11 or Alt+Enter toggles full screen (borderless, at the desktop's resolution), F9 asks for a
// frame capture. Guest threads never call SDL.
//
// Set --headless to skip the window entirely.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use sdl3::event::{Event, WindowEvent};
use sdl3::gamepad::{Axis, Button, ButtonLabel, Gamepad};
use sdl3::keyboard::{Mod, Scancode};
use sdl3::mouse::MouseButton;
use sdl3::video::FullscreenType;

use crate::host::input::{self, PadElem};
use crate::host::rumble::Rumbler;

static WINDOW_UP: AtomicBool = AtomicBool::new(false);
static HWND: AtomicUsize = AtomicUsize::new(0);

/// True once the window exists (presents before that are dropped).
pub fn window_up() -> bool {
    WINDOW_UP.load(Ordering::Acquire)
}

pub fn headless() -> bool {
    crate::settings::get().display.headless
}

/// The window's HWND (0 before the window is up or without one).
pub fn hwnd() -> usize {
    HWND.load(Ordering::Acquire)
}

/// Docked and handheld mode only change what the game renders: the swapchain follows the window's size, not the
/// game's output resolution as the macOS drawable does.
pub fn update_handheld_drawable(_handheld: bool) {}

/// The main thread's state: the window and the open controllers.
struct Host {
    window: sdl3::video::Window,
    gamepads: sdl3::GamepadSubsystem,
    pads: Vec<(Gamepad, Rumbler)>,
}

/// Create the window and run SDL's event loop. Must be called on the process main thread.
/// Never returns (the process exits when the window closes or the guest finishes).
pub fn run_main_thread() -> ! {
    let fail = |what: &str, e: &dyn std::fmt::Display| -> ! {
        eprintln!("[!!] Display: {what}: {e} (--headless runs without a window)");
        std::process::exit(1)
    };
    // Controllers keep working while another window has focus, as macOS's controller monitoring does.
    sdl3::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
    let sdl = sdl3::init().unwrap_or_else(|e| fail("SDL", &e));
    let video = sdl.video().unwrap_or_else(|e| fail("SDL video", &e));
    let gamepads = sdl.gamepad().unwrap_or_else(|e| fail("SDL game controllers", &e));
    let s = crate::settings::get();
    input::set_controller_swaps(s.input.swap_ab, s.input.swap_xy);

    // The window opens at the screen shape asked for when the game can render at it.
    let shape = s.display.launch_aspect.map(|a| a.ratio()).filter(|_| crate::profile::renders_screen_shape());
    let (w, h) = (shape.map_or(1280.0, |a| (720.0 * a).round()), 720.0);
    let base_title = format!("{} - native", crate::profile::app_title());
    let window = video
        .window(&base_title, w as u32, h as u32)
        .position_centered()
        .resizable()
        .high_pixel_density()
        .build()
        .unwrap_or_else(|e| fail("window", &e));
    let hwnd = unsafe {
        use sdl3::sys::{properties::SDL_GetPointerProperty, video};
        let props = video::SDL_GetWindowProperties(window.raw());
        SDL_GetPointerProperty(props, video::SDL_PROP_WINDOW_WIN32_HWND_POINTER, std::ptr::null_mut()) as usize
    };
    if hwnd == 0 {
        fail("window", &"no HWND");
    }
    HWND.store(hwnd, Ordering::Release);
    let mut pump = sdl.event_pump().unwrap_or_else(|e| fail("SDL events", &e));
    let mut host = Host { window, gamepads, pads: Vec::new() };

    WINDOW_UP.store(true, Ordering::Release);
    println!("[+] Display: window up ({w}x{h}); presenting guest frames");
    let start = Instant::now();
    let mut title_at = start;
    loop {
        // Events as they come, and at least every 4 ms the controllers (the 250 Hz poll macOS does).
        if let Some(e) = pump.wait_event_timeout(Duration::from_millis(4)) {
            host.event(e);
        }
        for e in pump.poll_iter() {
            host.event(e);
        }
        host.poll_controllers();
        if title_at.elapsed() >= Duration::from_millis(500) {
            title_at = Instant::now();
            host.update_title(&base_title);
        }
        // Debug: --debug-close-after=N closes the window after N seconds, as its close button does.
        if s.debug.debug_close_after.is_some_and(|secs| start.elapsed().as_secs_f64() >= secs) {
            quit();
        }
    }
}

/// Closing the window quits: the guest has no clean shutdown path, so leave immediately.
fn quit() -> ! {
    println!("[+] Display: quit requested; exiting");
    std::process::exit(0)
}

impl Host {
    fn event(&mut self, e: Event) {
        match e {
            Event::Quit { .. } => quit(),
            Event::Window { win_event, .. } => match win_event {
                WindowEvent::CloseRequested => quit(),
                // Key-up events go elsewhere while another window has focus: drop held keys and the mouse's touch.
                WindowEvent::FocusLost => {
                    input::clear_keys();
                    input::touch_ended(0);
                }
                _ => {}
            },
            Event::KeyDown { scancode: Some(sc), keymod, repeat, .. } => {
                let alt = keymod.intersects(Mod::LALTMOD | Mod::RALTMOD);
                if sc == Scancode::F11 || (sc == Scancode::Return && alt) {
                    if !repeat {
                        self.toggle_fullscreen();
                    }
                    return;
                }
                // F9 or space (unless space is bound to a game input): capture the frame history at the next present,
                // to show a glitch from a live session.
                let space_free = || !input::bindings().iter().any(|b| b.key == Some(49));
                if sc == Scancode::F9 || (sc == Scancode::Space && space_free()) {
                    if !repeat {
                        crate::gpu::history::request();
                        println!("[+] Capture requested: the frame history is written at the next present");
                    }
                    return;
                }
                if let Some(code) = mac_key(sc) {
                    input::set_key(code, true);
                }
            }
            Event::KeyUp { scancode: Some(sc), .. } => {
                if let Some(code) = mac_key(sc) {
                    input::set_key(code, false);
                }
            }
            // The mouse (left button) is the touch screen: one finger at the cursor while the button is down.
            Event::MouseButtonDown { mouse_btn: MouseButton::Left, x, y, .. } => self.mouse_touch(x, y),
            Event::MouseMotion { mousestate, x, y, .. } if mousestate.left() => self.mouse_touch(x, y),
            Event::MouseButtonUp { mouse_btn: MouseButton::Left, .. } => input::touch_ended(0),
            Event::GamepadAdded { which, .. } => match self.gamepads.open(which) {
                Ok(pad) => {
                    println!("[+] Input: controller connected: {}", pad.name().unwrap_or_default());
                    self.pads.push((pad, Rumbler::new()));
                }
                Err(e) => println!("[!] Input: cannot open a controller: {e}"),
            },
            Event::GamepadRemoved { which, .. } => {
                self.pads.retain(|(p, _)| p.id().ok() != Some(which));
                println!("[+] Input: controller disconnected");
            }
            _ => {}
        }
    }

    fn toggle_fullscreen(&mut self) {
        let full = self.window.fullscreen_state() != FullscreenType::Off;
        if let Err(e) = self.window.set_fullscreen(!full) {
            println!("[!] Display: full screen: {e}");
        }
    }

    /// The game's 16:9 picture is fitted into the window (letterboxed, vulkan/window.rs), so the panel position is the
    /// cursor's fraction of that fitted rectangle (window coordinates, y grows downwards).
    fn mouse_touch(&self, x: f32, y: f32) {
        let (bw, bh) = self.window.size();
        let (bw, bh) = (bw as f64, bh as f64);
        if bw > 0.0 && bh > 0.0 {
            let (w, h) = if bw * 9.0 > bh * 16.0 { (bh * 16.0 / 9.0, bh) } else { (bw, bw * 9.0 / 16.0) };
            let (x0, y0) = ((bw - w) / 2.0, (bh - h) / 2.0);
            input::touch_moved(0, (x as f64 - x0) / w, (y as f64 - y0) / h);
        }
    }

    /// Reads every open controller into host/input.rs's controller elements, in the order they were connected: the first
    /// is player 1 (with the keyboard), the next player 2 and so on. Sends each the game's rumble.
    fn poll_controllers(&mut self) {
        use PadElem as P;
        let mut players: Vec<[f32; 25]> = Vec::with_capacity(self.pads.len());
        for (pad, rumble) in self.pads.iter_mut().take(input::MAX_PLAYERS) {
            let mut v = [0f32; 25];
            let mut put = |e: PadElem, val: f32| v[e.index()] = v[e.index()].max(val);
            let button = |b: Button| if pad.button(b) { 1.0 } else { 0.0 };
            // SDL names the face buttons by position; macOS reads them by label (input::face_elems). Each position's
            // label (Xbox letters where SDL does not know the controller) picks its element.
            let faces = input::face_elems(true);
            for (b, xbox) in [(Button::South, 0), (Button::East, 1), (Button::West, 2), (Button::North, 3)] {
                let label = match pad.button_label_for_gamepad_type(b) {
                    ButtonLabel::A | ButtonLabel::Cross => 0,
                    ButtonLabel::B | ButtonLabel::Circle => 1,
                    ButtonLabel::X | ButtonLabel::Square => 2,
                    ButtonLabel::Y | ButtonLabel::Triangle => 3,
                    ButtonLabel::Unknown => xbox,
                };
                put(faces[label], button(b));
            }
            put(P::LeftShoulder, button(Button::LeftShoulder));
            put(P::RightShoulder, button(Button::RightShoulder));
            put(P::Menu, button(Button::Start));
            put(P::Options, button(Button::Back));
            put(P::Home, button(Button::Guide));
            put(P::LeftStickButton, button(Button::LeftStick));
            put(P::RightStickButton, button(Button::RightStick));
            put(P::DpadUp, button(Button::DPadUp));
            put(P::DpadDown, button(Button::DPadDown));
            put(P::DpadLeft, button(Button::DPadLeft));
            put(P::DpadRight, button(Button::DPadRight));
            let axis = |a: Axis| (pad.axis(a) as f32 / 32767.0).clamp(-1.0, 1.0);
            put(P::LeftTrigger, axis(Axis::TriggerLeft).max(0.0));
            put(P::RightTrigger, axis(Axis::TriggerRight).max(0.0));
            // SDL's y axes grow downwards.
            for (elems, x, y) in [
                ([P::LeftStickUp, P::LeftStickDown, P::LeftStickLeft, P::LeftStickRight], Axis::LeftX, Axis::LeftY),
                ([P::RightStickUp, P::RightStickDown, P::RightStickLeft, P::RightStickRight], Axis::RightX, Axis::RightY),
            ] {
                let (x, y) = dead_zone(axis(x), -axis(y));
                for (e, val) in elems.into_iter().zip([y.max(0.0), (-y).max(0.0), (-x).max(0.0), x.max(0.0)]) {
                    put(e, val);
                }
            }
            rumble.update(pad);
            players.push(v);
        }
        input::set_players(&players);
    }

    fn update_title(&mut self, base_title: &str) {
        let fps = crate::vulkan::display_fps();
        let title = if fps > 0.0 { format!("{base_title}  |  {:.0} fps", fps) } else { base_title.to_string() };
        if self.window.title() != title {
            let _ = self.window.set_title(&title);
        }
        static LOGGED: AtomicBool = AtomicBool::new(false);
        if fps > 0.0 && !LOGGED.swap(true, Ordering::Relaxed) {
            println!("[+] Display: window title now \"{}\"", title);
        }
    }
}

/// SDL reports the sticks' raw positions, which rest a few percent off centre; GameController removes that before
/// macOS sees the values. A radial dead zone does the same here, rescaled so the stick still reaches 1 at its edge.
fn dead_zone(x: f32, y: f32) -> (f32, f32) {
    const DEAD: f32 = 0.12;
    let m = (x * x + y * y).sqrt();
    if m <= DEAD {
        return (0.0, 0.0);
    }
    let k = ((m - DEAD) / (1.0 - DEAD)).min(1.0) / m;
    (x * k, y * k)
}

/// The macOS virtual key code (Carbon kVK_*, what controls.cfg stores) of the key at SDL scancode `sc` (USB HID usage):
/// the key in the same place on a US keyboard. PC keys without a Mac counterpart take the Mac key in their place (Alt is
/// Option, the Windows key Command, Insert is Help, Num Lock the keypad's Clear).
fn mac_key(sc: Scancode) -> Option<u16> {
    let sc = sc.to_i32();
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
