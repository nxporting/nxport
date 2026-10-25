// The game window's keyboard view and the Controls window (app menu > Controls…, Cmd-,), where each Switch input gets a
// keyboard key and a controller element. Click a binding, then press a key (or a controller button / stick direction);
// Esc cancels, Delete clears. Changes apply and save immediately (host/input.rs owns the bindings and the file).
//
// Everything here runs on the main thread. A 250 Hz timer (`tick`) polls controllers, captures controller bindings, and
// drops held keys when the game window loses focus (its key-up events would go elsewhere).

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

define_class!(
    /// Content view of the game window: forwards key state to host/input.rs (and keeps AppKit from beeping at unhandled keys).
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    pub struct GameView;

    unsafe impl NSObjectProtocol for GameView {}

    impl GameView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            // F9 or space (unless space is bound to a game input): capture the next frame (image + per-draw trace in the
            // log; with --frame-history also the last frames) to show a glitch from a live session.
            let space_free = || !input::bindings().iter().any(|b| b.key == Some(49));
            if event.keyCode() == 101 || (event.keyCode() == 49 && space_free()) {
                if !event.isARepeat() {
                    if crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Vulkan {
                        crate::gpu::history::request();
                        println!("[+] Capture requested: the frame history is written at the next present");
                    } else {
                        crate::metal::request_capture();
                    }
                }
                return;
            }
            input::set_key(event.keyCode(), true);
        }

        // The mouse (left button) is the touch screen: one finger at the cursor while the button is down.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.mouse_touch(event, false);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.mouse_touch(event, false);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.mouse_touch(event, true);
        }

        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            input::set_key(event.keyCode(), false);
        }

        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            let code = event.keyCode();
            if let Some(down) = input::modifier_down(code, event.modifierFlags().0) {
                input::set_key(code, down);
            }
        }
    }
);

impl GameView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] }
    }

    /// The 16:9 drawable is fitted into the view (letterboxed, host/display.rs), so the panel position is the cursor's fraction
    /// of that fitted rectangle (the view is not flipped: y grows upwards).
    fn mouse_touch(&self, event: &NSEvent, up: bool) {
        if up {
            input::touch_ended(0);
            return;
        }
        let p = self.convertPoint_fromView(event.locationInWindow(), None);
        let b = self.bounds();
        if b.size.width > 0.0 && b.size.height > 0.0 {
            let (w, h) = if b.size.width * 9.0 > b.size.height * 16.0 {
                (b.size.height * 16.0 / 9.0, b.size.height)
            } else {
                (b.size.width, b.size.width * 9.0 / 16.0)
            };
            let (x0, y0) = ((b.size.width - w) / 2.0, (b.size.height - h) / 2.0);
            input::touch_moved(0, (p.x - x0) / w, 1.0 - (p.y - y0) / h);
        }
    }
}

define_class!(
    /// Content view of the Controls window: receives the key press that completes a keyboard binding.
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
    }
);

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
        "Click a binding, then press a key or a controller button / stick direction. Esc cancels; Delete clears.",
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
