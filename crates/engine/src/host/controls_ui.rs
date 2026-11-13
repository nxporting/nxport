// The game window's keyboard and mouse view and the Controls window (app menu > Controls…, Cmd-,), where each Switch
// input gets a keyboard key or mouse button and a controller element. Click a binding, then press a key, mouse button
// (in the window, outside the buttons) or controller button / stick direction; Esc cancels, Delete clears. Changes apply
// and save immediately (host/input.rs owns the bindings and the file).
//
// The mouse, as on Windows (host/display_sdl.rs): with mouse look (Input menu) the window holds it (hidden, its movement
// turns the camera, its buttons are keys controls.cfg can bind); F8 frees it, and a freed mouse is the touch screen (left
// button), so menus drawn for touch take its clicks; F8 holds it again. With direct mouse access the game reads the mouse
// itself. Keys also reach nn::hid's keyboard state (direct keyboard access) by their USB usage.
//
// Everything here runs on the main thread. A 250 Hz timer (`tick`) polls controllers, captures controller bindings, and
// drops held keys and frees the mouse when the game window loses focus (its key-up events would go elsewhere).

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSEvent, NSResponder, NSTextField, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::host::input::{self, Bindings, PAD_ELEMS, SWITCH_INPUTS};

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
    fn CGWarpMouseCursorPosition(p: CGPoint) -> i32;
}

/// F6, F8, F9, F10 and space as macOS virtual key codes.
const KEY_F6: u16 = 97;
const KEY_F8: u16 = 100;
const KEY_F9: u16 = 101;
const KEY_F10: u16 = 109;
const KEY_SPACE: u16 = 49;

/// The nn::hid::MouseButtonSet bit of an NSEvent buttonNumber (0 left, 1 right, 2 middle, 3 back, 4 forward).
fn mouse_bit(number: isize) -> Option<u32> {
    Some(match number {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 4,
        4 => 3,
        _ => return None,
    })
}

fn direct_mouse() -> bool {
    crate::settings::get().input.direct_mouse
}

define_class!(
    /// Content view of the game window: forwards key and mouse state to host/input.rs (and keeps AppKit from beeping at
    /// unhandled keys).
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    pub struct GameView;

    unsafe impl NSObjectProtocol for GameView {}

    impl GameView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        // The first click on an inactive window reaches the game too (the touch screen, a bound mouse button).
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let code = event.keyCode();
            let repeat = event.isARepeat();
            if let Some(u) = input::usage_of_mac_key(code) {
                input::set_usb_key(u, true);
            }
            // F9 or space (unless space is bound to a game input): capture the next frame (image + per-draw trace in the
            // log; with --frame-history also the last frames) to show a glitch from a live session.
            let space_free = || !input::bindings().iter().any(|b| b.key == Some(KEY_SPACE));
            match code {
                // F6: display pacing of unlocked frame rates on / off (to compare by feel).
                KEY_F6 => {
                    if !repeat {
                        crate::metal::toggle_display_pacing();
                    }
                }
                // F8: mouse look frees the mouse, or takes it again.
                KEY_F8 if input::mouse_look_on() => {
                    if !repeat {
                        let held = with_ui(|ui| ui.mouse_held).unwrap_or(false);
                        hold_mouse(!held);
                    }
                }
                // F10: the performance overlay.
                KEY_F10 => {
                    if !repeat {
                        crate::host::overlay::toggle_overlay();
                    }
                }
                _ if code == KEY_F9 || (code == KEY_SPACE && space_free()) => {
                    if !repeat {
                        request_capture();
                    }
                }
                _ => input::set_key(code, true),
            }
        }

        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            if let Some(u) = input::usage_of_mac_key(event.keyCode()) {
                input::set_usb_key(u, false);
            }
            input::set_key(event.keyCode(), false);
        }

        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            let code = event.keyCode();
            if let Some(down) = input::modifier_down(code, event.modifierFlags().0) {
                if let Some(u) = input::usage_of_mac_key(code) {
                    input::set_usb_key(u, down);
                }
                input::set_key(code, down);
            }
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.motion(event, false);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.motion(event, true);
        }

        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            self.motion(event, false);
        }

        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            self.motion(event, false);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.button(event, true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.button(event, false);
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            self.button(event, true);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            self.button(event, false);
        }

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            self.button(event, true);
        }

        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) {
            self.button(event, false);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            if direct_mouse() {
                // Wheel units of 120 a notch, as on Windows.
                input::mouse_wheel((event.deltaY() * 120.0).round() as i32);
            }
        }
    }
);

/// F9 / space: the frame capture of the backend in use.
fn request_capture() {
    if crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Vulkan {
        crate::gpu::history::request();
        println!("[+] Capture requested: the frame history is written at the next present");
    } else {
        crate::metal::request_capture();
    }
}

impl GameView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] }
    }

    /// The game's picture is fitted into the view at the shape it renders at (16:9 letterboxed, or the window's own with
    /// the dynamic aspect ratio), so a position in it is the cursor's fraction of that fitted rectangle, 0 to 1 across each
    /// side from the top left (the view is not flipped: y grows upwards).
    fn game_position(&self, event: &NSEvent) -> (f64, f64) {
        let p = self.convertPoint_fromView(event.locationInWindow(), None);
        let b = self.bounds();
        if b.size.width <= 0.0 || b.size.height <= 0.0 {
            return (0.0, 0.0);
        }
        let a = crate::profile::rendered_aspect().unwrap_or(16.0 / 9.0);
        let (w, h) = if b.size.width > b.size.height * a {
            (b.size.height * a, b.size.height)
        } else {
            (b.size.width, b.size.width / a)
        };
        let (x0, y0) = ((b.size.width - w) / 2.0, (b.size.height - h) / 2.0);
        ((p.x - x0) / w, 1.0 - (p.y - y0) / h)
    }

    fn held() -> bool {
        with_ui(|ui| ui.mouse_held).unwrap_or(false)
    }

    /// Movement: the game's own mouse (direct access), mouse look while held, else the touch screen while the left
    /// button drags.
    fn motion(&self, event: &NSEvent, left: bool) {
        if direct_mouse() {
            let (x, y) = self.game_position(event);
            input::mouse_moved((x * 1280.0) as i32, (y * 720.0) as i32, event.deltaX() as i32, event.deltaY() as i32);
        } else if Self::held() {
            input::mouse_look_moved(event.deltaX() as f32, event.deltaY() as f32);
        } else if left {
            let (x, y) = self.game_position(event);
            input::touch_moved(0, x, y);
        }
    }

    /// A button: the game's own mouse (direct access), a bindable key while held (input::MOUSE_KEYS), else the left
    /// button is the touch screen.
    fn button(&self, event: &NSEvent, down: bool) {
        let Some(bit) = mouse_bit(event.buttonNumber()) else { return };
        if direct_mouse() {
            input::mouse_button(bit, down);
        } else if Self::held() {
            if let Some(&(code, _)) = input::MOUSE_KEYS.get(bit as usize) {
                input::set_key(code, down);
            }
        } else if bit == 0 {
            if down {
                let (x, y) = self.game_position(event);
                input::touch_moved(0, x, y);
            } else {
                input::touch_ended(0);
            }
        }
    }
}

/// Mouse look: hold the mouse in the game window (hidden, its movement relative: the cursor stays put) or free it.
fn hold_mouse(hold: bool) {
    let Some(window) = with_ui(|ui| {
        ui.mouse_held = hold;
        ui.game_window.clone()
    })
    .flatten() else {
        return;
    };
    grab_cursor(&window, hold && window.isKeyWindow());
}

/// Detaches the cursor from the mouse and hides it over the window's centre (so a click lands in the window), or gives it
/// back. Balanced: NSCursor counts hides.
fn grab_cursor(window: &NSWindow, grab: bool) {
    let grabbed = with_ui(|ui| std::mem::replace(&mut ui.cursor_grabbed, grab)).unwrap_or(false);
    if grabbed == grab {
        return;
    }
    unsafe {
        if grab {
            let f = window.frame();
            let screen_h = window.screen().map_or(f.origin.y + f.size.height, |s| {
                // Global display coordinates start at the top of the primary screen.
                let primary: Option<Retained<objc2_app_kit::NSScreen>> =
                    objc2_app_kit::NSScreen::screens(MainThreadMarker::from(window)).firstObject();
                primary.map_or(s.frame().size.height, |p| p.frame().size.height)
            });
            CGWarpMouseCursorPosition(CGPoint { x: f.origin.x + f.size.width / 2.0, y: screen_h - (f.origin.y + f.size.height / 2.0) });
            CGAssociateMouseAndMouseCursorPosition(0);
            let _: () = msg_send![objc2::class!(NSCursor), hide];
        } else {
            CGAssociateMouseAndMouseCursorPosition(1);
            let _: () = msg_send![objc2::class!(NSCursor), unhide];
        }
    }
}

define_class!(
    /// Content view of the Controls window: receives the key press or mouse button that completes a keyboard binding.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct CaptureView;

    unsafe impl NSObjectProtocol for CaptureView {}

    impl CaptureView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true // lay rows out top to bottom
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            captured_key(event.keyCode(), None);
        }

        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            let code = event.keyCode();
            if input::modifier_down(code, event.modifierFlags().0) == Some(true) {
                captured_key(code, Some(()));
            }
        }

        // A mouse button outside the window's buttons binds it (input::MOUSE_KEYS).
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            captured_mouse(event);
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            captured_mouse(event);
        }

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            captured_mouse(event);
        }
    }
);

fn captured_mouse(event: &NSEvent) {
    if let Some(&(code, _)) = mouse_bit(event.buttonNumber()).and_then(|b| input::MOUSE_KEYS.get(b as usize)) {
        if with_ui(|ui| matches!(ui.capture, Some((_, 0)))).unwrap_or(false) {
            captured_key(code, None);
        }
    }
}

define_class!(
    /// Target of the Controls window's buttons and of the input timer.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct ControlsTarget;

    unsafe impl NSObjectProtocol for ControlsTarget {}

    impl ControlsTarget {
        #[unsafe(method(bindClicked:))]
        fn bind_clicked(&self, sender: &NSButton) {
            let tag = sender.tag() as usize;
            start_capture(tag / 2, tag % 2);
        }

        #[unsafe(method(resetDefaults:))]
        fn reset_defaults(&self, _sender: &AnyObject) {
            cancel_capture();
            input::set_bindings(input::default_bindings());
            refresh_titles();
        }

        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &AnyObject) {
            tick(MainThreadMarker::from(self));
        }

        #[unsafe(method(selfTest:))]
        fn self_test(&self, _timer: &AnyObject) {
            self_test(MainThreadMarker::from(self));
        }
    }
);

impl ControlsTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

struct Ui {
    target: Retained<ControlsTarget>,
    game_window: Option<Retained<NSWindow>>,
    window: Option<Retained<NSWindow>>,
    view: Option<Retained<CaptureView>>,
    /// Per Switch input: (keyboard button, controller button).
    buttons: Vec<(Retained<NSButton>, Retained<NSButton>)>,
    /// (input row, column 0 keyboard / 1 controller) waiting for a press.
    capture: Option<(usize, usize)>,
    /// Controller values when the capture started (lowered as elements are released): only a new press binds.
    baseline: [f32; 25],
    /// Mouse look holds the mouse (F8 toggles); while the window is not key the cursor is given back meanwhile.
    mouse_held: bool,
    /// The cursor is detached and hidden now.
    cursor_grabbed: bool,
    /// The game window was key at the last tick.
    was_key: bool,
    /// Mouse look was on at the last tick (the menu switches it).
    mouse_look: bool,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|u| u.borrow_mut().as_mut().map(f))
}

/// Starts input handling: the 250 Hz timer, and the game window whose focus gates keyboard input. Main thread.
pub fn install(mtm: MainThreadMarker, game_window: Option<&NSWindow>) {
    let target = ControlsTarget::new(mtm);
    unsafe {
        let _: *mut AnyObject = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.004f64,
            target: &*target,
            selector: sel!(tick:),
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: true
        ];
    }
    UI.with(|u| {
        *u.borrow_mut() = Some(Ui {
            target,
            game_window: game_window.map(|w| w.retain()),
            window: None,
            view: None,
            buttons: Vec::new(),
            capture: None,
            baseline: [0.0; 25],
            mouse_held: false,
            cursor_grabbed: false,
            was_key: false,
            mouse_look: false,
        })
    });
    input::update_npad();
    println!(
        "[+] Input: keyboard + controllers active; controls file {}",
        input::config_path().display()
    );
    if crate::settings::get().debug.debug_input_selftest {
        if let Some(t) = with_ui(|ui| ui.target.clone()) {
            unsafe {
                let _: *mut AnyObject = msg_send![
                    objc2::class!(NSTimer),
                    scheduledTimerWithTimeInterval: 3.0f64,
                    target: &*t,
                    selector: sel!(selfTest:),
                    userInfo: std::ptr::null::<AnyObject>(),
                    repeats: false
                ];
            }
        }
    }
}

/// Debug (--debug-input-selftest, with --controls-file pointing at a scratch file): drives key events through
/// AppKit into the game window and the Controls window and logs the resulting Npad state and bindings.
fn self_test(mtm: MainThreadMarker) {
    let key_event = |window: &NSWindow, down: bool, code: u16| unsafe {
        let ty: usize = if down { 10 } else { 11 }; // NSEventTypeKeyDown / KeyUp
        let ev: *mut AnyObject = msg_send![
            objc2::class!(NSEvent),
            keyEventWithType: ty,
            location: NSPoint::new(0.0, 0.0),
            modifierFlags: 0usize,
            timestamp: 0.0f64,
            windowNumber: window.windowNumber(),
            context: std::ptr::null::<AnyObject>(),
            characters: &*NSString::from_str("x"),
            charactersIgnoringModifiers: &*NSString::from_str("x"),
            isARepeat: false,
            keyCode: code
        ];
        let _: () = msg_send![window, sendEvent: ev];
    };
    let Some(game) = with_ui(|ui| ui.game_window.clone()).flatten() else {
        return;
    };
    game.makeKeyAndOrderFront(None);
    key_event(&game, true, 13); // W: left stick up
    key_event(&game, true, 6); // Z: A
    println!("[selftest] W+Z down: npad {:?}", input::npad_state());
    key_event(&game, false, 13);
    key_event(&game, false, 6);
    println!("[selftest] released: npad {:?}", input::npad_state());

    open(mtm);
    let (window, button) =
        with_ui(|ui| (ui.window.clone(), ui.buttons.first().map(|b| b.0.clone()))).unwrap();
    let (Some(window), Some(button)) = (window, button) else {
        return;
    };
    unsafe { button.performClick(None) }; // A, keyboard column
    println!(
        "[selftest] capture after click: {:?}",
        with_ui(|ui| ui.capture).flatten()
    );
    key_event(&window, true, 5); // G
    key_event(&window, false, 5);
    println!(
        "[selftest] A bound to key {:?} (want Some(5)); capture {:?}",
        input::bindings()[0].key,
        with_ui(|ui| ui.capture).flatten()
    );
    key_event(&game, true, 5);
    println!(
        "[selftest] G down in game window: A pressed = {}",
        input::npad_state().0 & 1 != 0
    );
    key_event(&game, false, 5);
    let saved = std::fs::read_to_string(input::config_path()).unwrap_or_default();
    println!(
        "[selftest] saved file has 'A key=5': {}",
        saved
            .lines()
            .any(|l| l.starts_with("A ") && l.contains("key=5 "))
    );
    input::set_bindings(input::default_bindings());
    refresh_titles();
    println!("[selftest] done");
}

fn tick(mtm: MainThreadMarker) {
    input::poll_controllers(mtm);
    follow_focus_and_mouse_look();
    let mut done: Option<(usize, input::PadElem)> = None;
    let mut closed = false;
    with_ui(|ui| {
        // Keyboard input only reaches the game while its window has focus; drop held keys when it loses it.
        if let Some(w) = &ui.game_window {
            if !w.isKeyWindow() {
                input::clear_keys();
            }
        }
        if let Some((row, 1)) = ui.capture {
            if ui.window.as_ref().is_some_and(|w| !w.isVisible()) {
                closed = true;
                return;
            }
            for e in PAD_ELEMS {
                let v = input::pad_value(e);
                let b = &mut ui.baseline[e.index()];
                if v > 0.6 && *b < 0.3 {
                    done = Some((row, e));
                    return;
                }
                *b = b.min(v);
            }
        } else if ui.capture.is_some() && ui.window.as_ref().is_some_and(|w| !w.isVisible()) {
            closed = true;
        }
    });
    if closed {
        cancel_capture();
    }
    if let Some((row, e)) = done {
        let mut b = input::bindings();
        b[row].pad = Some(e);
        finish_capture(b);
    }
}

/// Mouse look switched in the menu takes or frees the mouse; the cursor is given back while the game window is not key
/// (the Controls window, another app) and taken again when it is, as long as mouse look holds it.
fn follow_focus_and_mouse_look() {
    let Some((window, was_key, held, look)) =
        with_ui(|ui| ui.game_window.clone().map(|w| (w, ui.was_key, ui.mouse_held, ui.mouse_look))).flatten()
    else {
        return;
    };
    let on = input::mouse_look_on();
    if on != look {
        with_ui(|ui| ui.mouse_look = on);
        hold_mouse(on);
        println!("[+] Input: mouse look {}", if on { "on (F8 frees the mouse)" } else { "off" });
    }
    let key = window.isKeyWindow();
    if key != was_key {
        with_ui(|ui| ui.was_key = key);
        if !key {
            input::touch_ended(0);
        }
    }
    let held = with_ui(|ui| ui.mouse_held).unwrap_or(held);
    grab_cursor(&window, held && key);
}

fn start_capture(row: usize, col: usize) {
    cancel_capture();
    with_ui(|ui| {
        ui.capture = Some((row, col));
        ui.baseline = std::array::from_fn(|i| input::pad_value(PAD_ELEMS[i]));
        if let Some((k, p)) = ui.buttons.get(row) {
            let b = if col == 0 { k } else { p };
            b.setTitle(&NSString::from_str(if col == 0 {
                "Press a key…"
            } else {
                "Press a controller button…"
            }));
        }
        if let (Some(w), Some(v)) = (&ui.window, &ui.view) {
            w.makeFirstResponder(Some(v));
        }
    });
    input::set_suspended(true);
}

fn cancel_capture() {
    let had = with_ui(|ui| ui.capture.take().is_some()).unwrap_or(false);
    if had {
        input::set_suspended(false);
        refresh_titles();
    }
}

fn finish_capture(b: Bindings) {
    with_ui(|ui| ui.capture = None);
    input::set_suspended(false);
    input::set_bindings(b);
    refresh_titles();
}

/// A key press in the Controls window. Esc cancels, Delete clears the binding being captured (either column).
fn captured_key(code: u16, _modifier: Option<()>) {
    let Some(Some((row, col))) = with_ui(|ui| ui.capture) else {
        return;
    };
    let mut b = input::bindings();
    match code {
        53 => return cancel_capture(),
        51 | 117 => {
            if col == 0 {
                b[row].key = None;
            } else {
                b[row].pad = None;
            }
        }
        _ if col == 0 => b[row].key = Some(code),
        _ => return, // waiting for a controller press
    }
    finish_capture(b);
}

fn refresh_titles() {
    let b = input::bindings();
    with_ui(|ui| {
        for (i, (k, p)) in ui.buttons.iter().enumerate() {
            let bd = b[i];
            k.setTitle(&NSString::from_str(bd.key.map_or("—", input::key_name)));
            p.setTitle(&NSString::from_str(bd.pad.map_or("—", |e| e.label())));
        }
    });
}

/// Shows the Controls window (creating it on first use).
pub fn open(mtm: MainThreadMarker) {
    let existing = with_ui(|ui| ui.window.clone()).flatten();
    if let Some(w) = existing {
        refresh_titles();
        w.makeKeyAndOrderFront(None);
        NSApplication::sharedApplication(mtm).activate();
        return;
    }
    let Some(target) = with_ui(|ui| ui.target.clone()) else {
        return;
    };

    const ROW: f64 = 26.0;
    const TOP: f64 = 56.0;
    let (w, h) = (640.0, TOP + ROW * SWITCH_INPUTS.len() as f64 + 76.0);
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, h));
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str("Controls"));
    unsafe { window.setReleasedWhenClosed(false) };
    let view: Retained<CaptureView> =
        unsafe { msg_send![CaptureView::alloc(mtm), initWithFrame: rect] };

    let label = |text: &str, frame: NSRect, bold: bool| {
        let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        l.setFrame(frame);
        if bold {
            unsafe {
                let font: *mut AnyObject =
                    msg_send![objc2::class!(NSFont), boldSystemFontOfSize: 13.0f64];
                let _: () = msg_send![&l, setFont: font];
            }
        }
        view.addSubview(&l);
    };
    label(
        "Switch",
        NSRect::new(NSPoint::new(20.0, 20.0), NSSize::new(170.0, 20.0)),
        true,
    );
    label(
        "Keyboard",
        NSRect::new(NSPoint::new(200.0, 20.0), NSSize::new(150.0, 20.0)),
        true,
    );
    label(
        "Controller",
        NSRect::new(NSPoint::new(370.0, 20.0), NSSize::new(250.0, 20.0)),
        true,
    );

    let action: Sel = sel!(bindClicked:);
    let mut buttons = Vec::new();
    for (i, s) in SWITCH_INPUTS.iter().enumerate() {
        let y = TOP + ROW * i as f64;
        label(
            s.label(),
            NSRect::new(NSPoint::new(20.0, y + 3.0), NSSize::new(170.0, 20.0)),
            false,
        );
        let make = |x: f64, width: f64, col: usize| {
            let b = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str("—"),
                    Some(&target),
                    Some(action),
                    mtm,
                )
            };
            b.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, 24.0)));
            b.setTag((i * 2 + col) as isize);
            view.addSubview(&b);
            b
        };
        buttons.push((make(196.0, 160.0, 0), make(366.0, 256.0, 1)));
    }
    let foot = TOP + ROW * SWITCH_INPUTS.len() as f64 + 12.0;
    label(
        "Click a binding, then press a key, mouse button or controller button / stick direction. Esc cancels; Delete clears.",
        NSRect::new(NSPoint::new(20.0, foot), NSSize::new(600.0, 18.0)),
        false,
    );
    let reset = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("Reset to Defaults"),
            Some(&target),
            Some(sel!(resetDefaults:)),
            mtm,
        )
    };
    reset.setFrame(NSRect::new(
        NSPoint::new(20.0, foot + 26.0),
        NSSize::new(160.0, 26.0),
    ));
    view.addSubview(&reset);

    window.setContentView(Some(&view));
    window.center();
    with_ui(|ui| {
        ui.window = Some(window.clone());
        ui.view = Some(view.clone());
        ui.buttons = buttons;
    });
    refresh_titles();
    window.makeKeyAndOrderFront(None);
    NSApplication::sharedApplication(mtm).activate();
}
