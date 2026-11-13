// In-process presentation: a window with a CAMetalLayer, owned by the process main thread
// (AppKit requires window/view creation and the event loop there), and a render thread that
// hands the Metal device/layer to metal/mod.rs, which presents on the guest's `nvnQueuePresentTexture`.
//
// CAMetalLayer itself is thread-agnostic (nextDrawable / encode / present may run anywhere); only
// the NSApplication run loop, NSWindow and NSView need the main thread. The guest therefore runs
// on its own thread (see main.rs) and talks to this module through `notify_present`.
//
// Set --headless to skip the window entirely.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSControlStateValueOff,
    NSControlStateValueOn, NSMenuItemValidation, NSWindow, NSWindowStyleMask,
};
use objc2_app_kit::{NSApplicationDelegate, NSMenu, NSMenuItem};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_metal::{MTLCreateSystemDefaultDevice, MTLPixelFormat};
use objc2_quartz_core::CAMetalLayer;

static WINDOW_UP: AtomicBool = AtomicBool::new(false);

/// True once the window and layer exist (presents before that are dropped).
pub fn window_up() -> bool {
    WINDOW_UP.load(Ordering::Acquire)
}

pub fn headless() -> bool {
    crate::settings::get().display.headless
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and AppDelegate does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    impl AppDelegate {
        // Timer (twice a second): the window title (with the presented frame rate if asked for), the display's refresh
        // rate, and the layer's presentation (vsync, low latency) from the Display menu.
        #[unsafe(method(updateTitle:))]
        fn update_title(&self, _timer: &objc2::runtime::AnyObject) {
            let mtm = MainThreadMarker::from(self);
            let fps = if crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Vulkan {
                crate::vulkan::display_fps()
            } else {
                crate::metal::display_fps()
            };
            let app_name = crate::profile::app_title();
            let title = if fps > 0.0 && crate::host::live::fps_in_title() {
                format!("{app_name}  |  {:.0} fps", fps)
            } else {
                app_name.to_string()
            };
            for w in NSApplication::sharedApplication(mtm).windows().iter() {
                if w.title().to_string().starts_with(app_name) {
                    w.setTitle(&NSString::from_str(&title));
                    // The display the window is on (it may have moved): with vsync the display paces at its rate.
                    if let Some(screen) = w.screen() {
                        crate::host::live::set_refresh_rate(screen.maximumFramesPerSecond() as f32);
                    }
                }
            }
            update_layer();
            static LOGGED: AtomicBool = AtomicBool::new(false);
            if fps > 0.0 && !LOGGED.swap(true, Ordering::Relaxed) {
                println!("[+] Display: window title now \"{}\"", title);
            }
        }

        // Every menu item (host/menu.rs) by its tag.
        #[unsafe(method(menuCommand:))]
        fn menu_command(&self, sender: &NSMenuItem) {
            let mtm = MainThreadMarker::from(self);
            match crate::host::menu::command(sender.tag() as u16) {
                Ok(_) => {}
                Err(crate::host::menu::Action::FullScreen) => {
                    if let Some(w) = game_window(mtm) {
                        w.toggleFullScreen(None);
                    }
                }
                Err(crate::host::menu::Action::Size(h)) => set_window_height(mtm, h as f64),
                Err(crate::host::menu::Action::Controls) | Err(crate::host::menu::Action::Bind(_)) => {
                    crate::host::controls_ui::open(mtm)
                }
            }
        }
    }

    unsafe impl NSMenuItemValidation for AppDelegate {
        // Check marks, availability and the frame-rate limits' labels from the current values, as the menu opens.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            validate(item)
        }
    }

    unsafe impl NSApplicationDelegate for AppDelegate {
        // Closing the window (the X button) quits, like any single-window app.
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _sender: &NSApplication) -> bool {
            true
        }

        // Quit via the window, Cmd-Q or the Dock: the guest has no clean shutdown path, so leave immediately rather
        // than let AppKit wait on threads that will never finish.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: &NSNotification) {
            println!("[+] Display: quit requested; exiting");
            std::process::exit(0);
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> objc2::rc::Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

/// Check marks, availability and the frame-rate limits' labels of a menu item (host/menu.rs) from the current values.
fn validate(item: &NSMenuItem) -> bool {
    use crate::host::menu::{self, id};
    if item.action() != Some(sel!(menuCommand:)) {
        return true;
    }
    let cmd = item.tag() as u16;
    if let Some(on) = menu::checked(cmd) {
        item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
    }
    if (id::LIMIT..id::LIMIT + 200).contains(&cmd) {
        item.setTitle(&NSString::from_str(&menu::limit_item_label((cmd - id::LIMIT) as u32)));
    }
    menu::enabled(cmd)
}

/// The game's window (its title starts with the app's name; the Controls window is the other).
fn game_window(mtm: MainThreadMarker) -> Option<objc2::rc::Retained<NSWindow>> {
    let app_name = crate::profile::app_title();
    NSApplication::sharedApplication(mtm).windows().iter().find(|w| w.title().to_string().starts_with(app_name))
}

/// A window of `height` points at the window's current shape (16:9 at first), leaving full screen first.
fn set_window_height(mtm: MainThreadMarker, height: f64) {
    let Some(w) = game_window(mtm) else { return };
    if w.styleMask().contains(NSWindowStyleMask::FullScreen) {
        w.toggleFullScreen(None);
    }
    let size = w.contentView().map_or(NSSize::new(16.0, 9.0), |v| v.bounds().size);
    let width = if size.height > 0.0 { (height * size.width / size.height).round() } else { (height * 16.0 / 9.0).round() };
    w.setContentSize(NSSize::new(width, height));
}

/// The layer presents as the Display menu says: with vsync at the display's refresh, and with low latency at most one
/// finished frame waiting for the screen (two drawables instead of three).
fn update_layer() {
    let ptr = LAYER_PTR.load(Ordering::Acquire);
    // Vulkan: MoltenVK's swapchain sets them (vulkan/window.rs makes it again when the settings change).
    if ptr == 0 || crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Vulkan {
        return;
    }
    let layer = unsafe { &*(ptr as *const CAMetalLayer) };
    let vsync = crate::host::overlay::is_vsync();
    let drawables = if crate::host::overlay::is_low_latency() { 2 } else { 3 };
    if layer.displaySyncEnabled() != vsync {
        layer.setDisplaySyncEnabled(vsync);
    }
    if layer.maximumDrawableCount() != drawables {
        layer.setMaximumDrawableCount(drawables);
    }
}

/// The menu bar: the menus every desktop host shares (host/menu.rs bar), each item sending menuCommand: with its id as
/// the tag. What is the Mac's own: key equivalents, Controls as a window, and "Controls…" (⌘,) atop the app menu.
fn install_menu(app: &NSApplication, mtm: MainThreadMarker) {
    use crate::host::menu::{self, id, Entry};
    let new_item = |title: &str, cmd: u16, key: &str| {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str(title),
                Some(sel!(menuCommand:)),
                &NSString::from_str(key),
            )
        };
        item.setTag(cmd as isize);
        item
    };
    let separator = |m: &NSMenu| m.addItem(&NSMenuItem::separatorItem(mtm));
    let note = |m: &NSMenu, title: &str| {
        let i = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &NSString::from_str(title), None, &NSString::from_str("")) };
        i.setEnabled(false);
        m.addItem(&i);
    };
    // A command as the Mac shows it: its key equivalent, or the key that does it shown in the title.
    let command = |m: &NSMenu, label: &str, cmd: u16| {
        let i = match cmd {
            id::QUIT => new_item(label, cmd, "q"),
            id::PERF_OVERLAY => new_item(label, cmd, "o"),
            id::FULL_SCREEN => {
                let i = new_item(label, cmd, "f");
                i.setKeyEquivalentModifierMask(objc2_app_kit::NSEventModifierFlags::Command | objc2_app_kit::NSEventModifierFlags::Control);
                i
            }
            id::CAPTURE => new_item(&format!("{label} (F9)"), cmd, ""),
            _ => new_item(label, cmd, ""),
        };
        m.addItem(&i);
    };
    // `head` fills the top of the first menu (the app menu) before its entries.
    fn fill(m: &NSMenu, entries: &[Entry], mtm: MainThreadMarker, head: Option<&dyn Fn(&NSMenu)>, leaf: &dyn Fn(&NSMenu, &Entry)) {
        for (n, e) in entries.iter().enumerate() {
            match e {
                Entry::Sub(title, inner) => {
                    let sm = NSMenu::new(mtm);
                    sm.setTitle(&NSString::from_str(title));
                    if let Some(head) = head.filter(|_| n == 0) {
                        head(&sm);
                    }
                    fill(&sm, inner, mtm, None, leaf);
                    let i = NSMenuItem::new(mtm);
                    i.setTitle(&NSString::from_str(title));
                    i.setSubmenu(Some(&sm));
                    m.addItem(&i);
                }
                other => leaf(m, other),
            }
        }
    }
    let leaf = |m: &NSMenu, e: &Entry| match e {
        Entry::Item { label, id: cmd, .. } => command(m, label, *cmd),
        Entry::Note(text) => note(m, text),
        Entry::Separator => separator(m),
        Entry::Controls => {
            command(m, "Controls…", id::CONTROLS);
            command(m, "Reset Controls to Defaults", id::RESET_CONTROLS);
        }
        Entry::Sub(..) => {}
    };
    let bar = NSMenu::new(mtm);
    // The app menu opens with the Controls window, as a Mac app's settings (⌘,).
    let head = |m: &NSMenu| {
        m.addItem(&new_item("Controls…", id::CONTROLS, ","));
        separator(m);
    };
    fill(&bar, &menu::bar(), mtm, Some(&head), &leaf);
    app.setMainMenu(Some(&bar));
}

/// Create the window and run the Cocoa event loop. Must be called on the process main thread.
/// Never returns (the process exits when the window closes or the guest finishes).
pub fn run_main_thread() -> ! {
    let mtm =
        MainThreadMarker::new().expect("display::run_main_thread must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    install_menu(&app, mtm);

    // The window opens at the screen shape asked for when the game can render at it.
    let shape = crate::settings::get()
        .display
        .launch_aspect
        .map(|a| a.ratio())
        .filter(|_| crate::profile::renders_screen_shape());
    let (w, h) = (shape.map_or(1280.0, |a| (720.0 * a).round()), 720.0);
    let rect = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(w, h));
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    let app_name = crate::profile::app_title();
    window.setTitle(&NSString::from_str(&app_name));
    unsafe { window.setReleasedWhenClosed(false) };
    window.setBackgroundColor(Some(&objc2_app_kit::NSColor::blackColor()));

    let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
    let layer = CAMetalLayer::new();
    layer.setDevice(Some(&device));
    layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    layer.setFramebufferOnly(true);
    layer.setDrawableSize(NSSize::new(1920.0, 1080.0));
    // Keep the game's 16:9 when the window (or full screen) has another shape: the drawable is fitted into the layer
    // (kCAGravityResizeAspect) and the black window background fills the bars.
    unsafe {
        let _: () = msg_send![&layer, setContentsGravity: &*NSString::from_str("resizeAspect")];
    }

    // The content view takes keyboard input for the game (host/controls_ui.rs).
    let view = crate::host::controls_ui::GameView::new(mtm, rect);
    view.setWantsLayer(true);
    view.setLayer(Some(&layer));
    window.setContentView(Some(&view));
    window.makeFirstResponder(Some(&view));
    // Mouse look and the game's own mouse read movement without a button down.
    window.setAcceptsMouseMovedEvents(true);
    window.makeKeyAndOrderFront(None);
    app.activate();
    crate::host::controls_ui::install(mtm, Some(&window));

    // The Vulkan backend presents to the layer itself (vulkan/window.rs).
    if crate::settings::get().graphics.gpu != crate::settings::GpuBackend::Vulkan {
        crate::metal::init(device.clone(), Some(layer.clone()));
    }
    LAYER_PTR.store((&*layer) as *const CAMetalLayer as usize, Ordering::Release);
    if crate::horizon::hle::is_handheld() {
        update_handheld_drawable(true);
    }

    // Debug: --debug-close-after=N clicks the window's close button (performClose:) after N seconds, to test
    // quitting through the X button without UI scripting.
    if let Some(secs) = crate::settings::get().debug.debug_close_after {
        unsafe {
            let _: () = msg_send![&window, performSelector: sel!(performClose:), withObject: std::ptr::null::<objc2::runtime::AnyObject>(), afterDelay: secs];
        }
    }

    unsafe {
        let _: *mut objc2::runtime::AnyObject = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.5f64,
            target: &*delegate,
            selector: sel!(updateTitle:),
            userInfo: std::ptr::null::<objc2::runtime::AnyObject>(),
            repeats: true
        ];
    }

    WINDOW_UP.store(true, Ordering::Release);
    println!("[+] Display: window up ({w}x{h}); presenting guest frames");
    app.run();
    // Window closed / app terminated: take the whole process down (the guest has no clean shutdown path yet).
    std::process::exit(0);
}

/// The game presents a new size (metal::present): the drawable takes it; kCAGravityResizeAspect fits it into the window.
pub fn content_size_changed(layer: &CAMetalLayer, w: f64, h: f64) {
    // A game rendering at another screen shape (config::display_aspect) still draws a 16:9 image: present it widened.
    let w = crate::config::display_aspect().map_or(w, |a| (h * a).round());
    layer.setDrawableSize(NSSize::new(w, h));
}

static WINDOW_ASPECT: AtomicU64 = AtomicU64::new(0);

pub fn current_window_aspect() -> Option<f64> {
    let bits = WINDOW_ASPECT.load(Ordering::Relaxed);
    (bits != 0).then(|| f64::from_bits(bits))
}

/// The window's shape (width / height of the game layer), checked at every present: a game rendering at another
/// shape follows the window as it is resized. Returns whether the shape changed.
pub fn follow_window_shape(layer: &CAMetalLayer) -> bool {
    let b = layer.bounds().size;
    if b.width < 1.0 || b.height < 1.0 {
        return false;
    }
    let a = b.width / b.height;
    WINDOW_ASPECT.store(a.to_bits(), Ordering::Relaxed);
    static LAST_REPORTED: AtomicU64 = AtomicU64::new(0);
    // Off, the game is back at 16:9 (live::set_dynamic_aspect): switched on again, the shape is reported again.
    if !crate::host::live::dynamic_aspect() {
        LAST_REPORTED.store(0, Ordering::Relaxed);
        return false;
    }
    let prev = f64::from_bits(LAST_REPORTED.load(Ordering::Relaxed));
    if (a - prev).abs() < 0.002 * a {
        return false;
    }
    LAST_REPORTED.store(a.to_bits(), Ordering::Relaxed);
    crate::profile::screen_aspect_changed(a);
    true
}

static LAYER_PTR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The window's CAMetalLayer (0 before the window is up or without one).
pub fn layer_ptr() -> usize {
    LAYER_PTR.load(Ordering::Acquire)
}

pub fn update_handheld_drawable(handheld: bool) {
    let ptr = LAYER_PTR.load(Ordering::Acquire);
    if ptr != 0 {
        let layer = unsafe { &*(ptr as *const CAMetalLayer) };
        let aspect = current_window_aspect().unwrap_or(16.0 / 9.0);
        let (w, h) = if handheld {
            ((720.0 * aspect).round(), 720.0)
        } else {
            ((1080.0 * aspect).round(), 1080.0)
        };
        layer.setDrawableSize(NSSize::new(w, h));
        println!("[+] Display: drawable size updated to {w}x{h} ({})", if handheld { "720p handheld" } else { "1080p docked" });
    }
}
