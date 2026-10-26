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
        // Timer (twice a second): show the presented frame rate in the window title.
        #[unsafe(method(updateTitle:))]
        fn update_title(&self, _timer: &objc2::runtime::AnyObject) {
            let mtm = MainThreadMarker::from(self);
            let fps = if crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Vulkan {
                crate::vulkan::display_fps()
            } else {
                crate::metal::display_fps()
            };
            let app_name = crate::profile::app_title();
            let title = if fps > 0.0 {
                format!("{app_name}  |  {:.0} fps", fps)
            } else {
                app_name.to_string()
            };
            for w in NSApplication::sharedApplication(mtm).windows().iter() {
                if w.title().to_string().starts_with(app_name) {
                    w.setTitle(&NSString::from_str(&title));
                }
            }
            static LOGGED: AtomicBool = AtomicBool::new(false);
            if fps > 0.0 && !LOGGED.swap(true, Ordering::Relaxed) {
                println!("[+] Display: window title now \"{}\"", title);
            }
        }

        // App menu > Controls… (Cmd-,).
        #[unsafe(method(openControls:))]
        fn open_controls(&self, _sender: &objc2::runtime::AnyObject) {
            crate::host::controls_ui::open(MainThreadMarker::from(self));
        }

        #[unsafe(method(toggleOverlay:))]
        fn toggle_overlay(&self, _sender: &objc2::runtime::AnyObject) {
            crate::host::overlay::toggle_overlay();
        }

        #[unsafe(method(toggleDynamicAspect:))]
        fn toggle_dynamic_aspect(&self, _sender: &objc2::runtime::AnyObject) {
            crate::host::overlay::toggle_dynamic_aspect();
        }

        #[unsafe(method(toggleUnlockedFps:))]
        fn toggle_unlocked_fps(&self, _sender: &objc2::runtime::AnyObject) {
            crate::host::overlay::toggle_fps_mode();
        }

        #[unsafe(method(toggleHandheldMode:))]
        fn toggle_handheld_mode(&self, _sender: &objc2::runtime::AnyObject) {
            let next = !crate::horizon::hle::is_handheld();
            crate::horizon::hle::set_handheld(next);
        }

        #[unsafe(method(toggleVsync:))]
        fn toggle_vsync(&self, _sender: &objc2::runtime::AnyObject) {
            crate::host::overlay::toggle_vsync();
        }

        #[unsafe(method(toggleAsyncShaders:))]
        fn toggle_async_shaders(&self, _sender: &objc2::runtime::AnyObject) {
            crate::host::overlay::toggle_async_shaders();
        }
    }

    unsafe impl NSMenuItemValidation for AppDelegate {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let action = item.action();
            if action == Some(sel!(toggleOverlay:)) {
                item.setState(if crate::host::overlay::is_overlay_enabled() {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
            } else if action == Some(sel!(toggleDynamicAspect:)) {
                item.setState(if crate::host::overlay::is_dynamic_aspect() {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
            } else if action == Some(sel!(toggleUnlockedFps:)) {
                item.setState(if crate::host::overlay::fps_mode() == crate::settings::FpsMode::Unlocked {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
            } else if action == Some(sel!(toggleHandheldMode:)) {
                item.setState(if crate::horizon::hle::is_handheld() {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
            } else if action == Some(sel!(toggleVsync:)) {
                item.setState(if crate::host::overlay::is_vsync() {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
            } else if action == Some(sel!(toggleAsyncShaders:)) {
                item.setState(if crate::host::overlay::is_async_shaders() {
                    NSControlStateValueOn
                } else {
                    NSControlStateValueOff
                });
            }
            true
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

/// The application menu with Controls… (Cmd-,) and Quit (Cmd-Q), plus Settings menu.
fn install_menu(app: &NSApplication, mtm: MainThreadMarker) {
    let bar = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    let controls = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str("Controls…"),
            Some(sel!(openControls:)),
            &NSString::from_str(","),
        )
    };
    app_menu.addItem(&controls);
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));

    let overlay_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str("Performance Overlay"),
            Some(sel!(toggleOverlay:)),
            &NSString::from_str("o"),
        )
    };
    app_menu.addItem(&overlay_item);
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));

    let dynamic_aspect = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str("Dynamic Aspect Ratio"),
            Some(sel!(toggleDynamicAspect:)),
            &NSString::from_str(""),
        )
    };
    app_menu.addItem(&dynamic_aspect);

    let unlocked_fps = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str("Unlocked Framerate"),
            Some(sel!(toggleUnlockedFps:)),
            &NSString::from_str(""),
        )
    };
    app_menu.addItem(&unlocked_fps);

    let handheld = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str("Handheld Mode (720p)"),
            Some(sel!(toggleHandheldMode:)),
            &NSString::from_str(""),
        )
    };
    app_menu.addItem(&handheld);
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));

    let vsync = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str("VSync"),
            Some(sel!(toggleVsync:)),
            &NSString::from_str(""),
        )
    };
    app_menu.addItem(&vsync);

    let async_shaders = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str("Asynchronous Shaders"),
            Some(sel!(toggleAsyncShaders:)),
            &NSString::from_str(""),
        )
    };
    app_menu.addItem(&async_shaders);
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));

    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str("Quit"),
            Some(sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    app_menu.addItem(&quit);
    app_item.setSubmenu(Some(&app_menu));
    bar.addItem(&app_item);

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
    if !crate::host::overlay::is_dynamic_aspect() {
        return false;
    }
    static LAST_REPORTED: AtomicU64 = AtomicU64::new(0);
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
