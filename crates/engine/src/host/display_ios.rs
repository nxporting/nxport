// iOS presentation (host/display.rs is the macOS one; same interface): UIApplicationMain on the process main thread, one
// full-screen landscape window whose root view hosts a CAMetalLayer, handed to metal/mod.rs like the macOS window's layer.
// Game controllers are polled on the main thread at 250 Hz (host/input.rs, GameController.framework).

use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSObjectProtocol, NSString};
use objc2_metal::{MTLCreateSystemDefaultDevice, MTLPixelFormat};
use objc2_quartz_core::CAMetalLayer;
use objc2_ui_kit::{UIResponder, UIViewController};

static WINDOW_UP: AtomicBool = AtomicBool::new(false);

/// True once the window and layer exist (presents before that are dropped).
pub fn window_up() -> bool {
    WINDOW_UP.load(Ordering::Acquire)
}

pub fn headless() -> bool {
    crate::settings::get().display.headless
}

/// The size the game last presented (metal::present), 0 before the first frame.
static CONTENT_SIZE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The game presents a new size: the drawable takes it and the view lays the layer out again at its aspect ratio.
pub fn content_size_changed(layer: &CAMetalLayer, w: f64, h: f64) {
    CONTENT_SIZE.store((w as u64) << 32 | h as u64, Ordering::Relaxed);
    layer.setDrawableSize(CGSize::new(w, h));
    #[allow(static_mut_refs)]
    if let Some(view) = unsafe { ROOT_VIEW } {
        let nil: *mut AnyObject = std::ptr::null_mut();
        unsafe {
            let _: () = msg_send![view, performSelectorOnMainThread: sel!(setNeedsLayout), withObject: nil, waitUntilDone: false];
        }
    }
}

/// The game's output size; the layer is letterboxed to this aspect ratio.
fn out_size() -> (f64, f64) {
    let c = CONTENT_SIZE.load(Ordering::Relaxed);
    if c != 0 {
        return ((c >> 32) as f64, (c & 0xffff_ffff) as f64);
    }
    if crate::horizon::hle::is_handheld() {
        (1280.0, 720.0)
    } else {
        (1920.0, 1080.0)
    }
}

/// Aspect ratio (w, h) the layer is fitted to: the game's output, or the display aspect set for an aspect ratio mod
/// (config::display_aspect; the game's 16:9 image is stretched to it, as Ryujinx / yuzu do with "stretch to window").
fn target_aspect_ratio() -> (f64, f64) {
    match crate::config::display_aspect() {
        Some(r) => (r, 1.0),
        None => out_size(),
    }
}

/// The Metal layer (main thread only after launch).
static mut LAYER: Option<Retained<CAMetalLayer>> = None;

/// On-screen virtual controller (GCVirtualController, iOS 15+).
static CONTROLS_ENABLED: AtomicBool = AtomicBool::new(false);
static mut VIRTUAL_CONTROLLER: Option<Retained<AnyObject>> = None;
/// Touch buttons for what the virtual controller cannot show: Plus, Minus and the stick presses (TotK crouches with
/// L3). Each button's tag is the key code it presses (host/input.rs default bindings).
static mut OVERLAY_BUTTONS: Vec<Retained<AnyObject>> = Vec::new();
const OVERLAY: [(&str, u16); 4] = [("−", 27), ("+", 24), ("L3", 3), ("R3", 4)];

/// Frame of overlay button `i` in a view of `bw` x `bh`: Minus / Plus at the top corners, the stick presses below them
/// (the virtual controller's window covers the lower part of the screen).
fn overlay_frame(i: usize, bw: f64, _bh: f64) -> CGRect {
    // L3 / R3 a full touch target (44 pt) below - / +, so neither is hit for the other.
    let (w, h) = (40.0, 36.0);
    let (x, y) = match i {
        0 => (60.0, 16.0),
        1 => (bw - 60.0 - w, 16.0),
        2 => (60.0, 16.0 + h + 44.0),
        _ => (bw - 60.0 - w, 16.0 + h + 44.0),
    };
    CGRect::new(CGPoint::new(x, y), CGSize::new(w, h))
}

unsafe fn set_overlay_hidden(hidden: bool) {
    #[allow(static_mut_refs)]
    for b in OVERLAY_BUTTONS.iter() {
        let _: () = msg_send![&**b, setHidden: hidden];
    }
}
static mut ROOT_VIEW: Option<*mut AnyObject> = None;
static mut APP_DELEGATE: Option<*const AppDelegate> = None;

fn read_onscreen_controls_pref() -> bool {
    unsafe {
        let Some(cls_defaults) = objc2::runtime::AnyClass::get(c"NSUserDefaults") else {
            return true;
        };
        let defaults: *mut AnyObject = msg_send![cls_defaults, standardUserDefaults];
        if defaults.is_null() {
            return true;
        }
        let _: bool = msg_send![defaults, synchronize];
        let k_ctrl = NSString::from_str("onscreen_controls");
        msg_send![defaults, boolForKey: &*k_ctrl]
    }
}

fn set_onscreen_controls_pref(enabled: bool) {
    unsafe {
        let Some(cls_defaults) = objc2::runtime::AnyClass::get(c"NSUserDefaults") else {
            return;
        };
        let defaults: *mut AnyObject = msg_send![cls_defaults, standardUserDefaults];
        if defaults.is_null() {
            return;
        }
        let k_ctrl = NSString::from_str("onscreen_controls");
        let _: () = msg_send![defaults, setBool: enabled, forKey: &*k_ctrl];
        let _: bool = msg_send![defaults, synchronize];
    }
}

fn sync_ios_settings() {
    unsafe {
        let Some(cls_defaults) = objc2::runtime::AnyClass::get(c"NSUserDefaults") else {
            return;
        };
        let defaults: *mut AnyObject = msg_send![cls_defaults, standardUserDefaults];
        if defaults.is_null() {
            return;
        }
        let _: bool = msg_send![defaults, synchronize];

        let k_overlay = NSString::from_str("perf_overlay");
        let has_overlay: bool = msg_send![defaults, boolForKey: &*k_overlay];
        crate::host::overlay::set_overlay_enabled(has_overlay);

        let k_vsync = NSString::from_str("vsync");
        let k_val: *mut AnyObject = msg_send![defaults, objectForKey: &*k_vsync];
        if !k_val.is_null() {
            let vsync: bool = msg_send![defaults, boolForKey: &*k_vsync];
            crate::host::overlay::set_vsync(vsync);
        }

        let k_async = NSString::from_str("async_shaders");
        let k_async_val: *mut AnyObject = msg_send![defaults, objectForKey: &*k_async];
        if !k_async_val.is_null() {
            let async_sh: bool = msg_send![defaults, boolForKey: &*k_async];
            crate::host::overlay::set_async_shaders(async_sh);
        }
    }
}

/// The physical-controller swaps: options --swap-ab / --swap-xy, else the app's preferences (an older single
/// "swap_ab_xy" preference stands for both).
fn read_controller_swaps() -> (bool, bool) {
    let (mut ab, mut xy) = (
        crate::settings::get().input.swap_ab,
        crate::settings::get().input.swap_xy,
    );
    unsafe {
        let Some(cls_defaults) = objc2::runtime::AnyClass::get(c"NSUserDefaults") else {
            return (ab, xy);
        };
        let defaults: *mut AnyObject = msg_send![cls_defaults, standardUserDefaults];
        if defaults.is_null() {
            return (ab, xy);
        }
        let _: bool = msg_send![defaults, synchronize];
        let get = |k: &str| -> bool {
            let k = NSString::from_str(k);
            msg_send![defaults, boolForKey: &*k]
        };
        let old = get("swap_ab_xy");
        ab |= get("swap_ab") || old;
        xy |= get("swap_xy") || old;
    }
    (ab, xy)
}

/// The GCController the on-screen controller feeds (null while there is none). Main thread only.
pub fn virtual_gamepad() -> *mut AnyObject {
    #[allow(static_mut_refs)]
    unsafe {
        match VIRTUAL_CONTROLLER.as_ref() {
            Some(vc) => msg_send![&**vc, controller],
            None => std::ptr::null_mut(),
        }
    }
}

fn read_default_is_docked() -> bool {
    unsafe {
        let Some(cls_defaults) = objc2::runtime::AnyClass::get(c"NSUserDefaults") else {
            return false;
        };
        let defaults: *mut AnyObject = msg_send![cls_defaults, standardUserDefaults];
        if defaults.is_null() {
            return false;
        }
        let _: bool = msg_send![defaults, synchronize];
        let k_mode = NSString::from_str("display_mode");
        let val: *mut AnyObject = msg_send![defaults, stringForKey: &*k_mode];
        if val.is_null() {
            return false;
        }
        let utf8: *const libc::c_char = msg_send![val, UTF8String];
        if utf8.is_null() {
            return false;
        }
        let s = std::ffi::CStr::from_ptr(utf8).to_str().unwrap_or("");
        s.eq_ignore_ascii_case("docked")
    }
}

/// Updates the dynamic Home Screen Quick Actions so long-pressing the app icon
/// always offers the complementary mode (opposite of Settings default).
fn update_shortcut_items() {
    unsafe {
        let default_docked = read_default_is_docked();
        let Some(cls_icon) = objc2::runtime::AnyClass::get(c"UIApplicationShortcutIcon") else {
            return;
        };
        let icon_name = NSString::from_str(if default_docked {
            "gamecontroller"
        } else {
            "tv"
        });
        let icon: *mut AnyObject = msg_send![cls_icon, iconWithSystemImageName: &*icon_name];

        let Some(cls_item) = objc2::runtime::AnyClass::get(c"UIApplicationShortcutItem") else {
            return;
        };
        let item: *mut AnyObject = msg_send![cls_item, alloc];

        let type_str = NSString::from_str(if default_docked {
            "launch_handheld"
        } else {
            "launch_docked"
        });
        let title_str = NSString::from_str(if default_docked {
            "Launch in Handheld Mode (720p)"
        } else {
            "Launch in Docked Mode (1080p)"
        });
        let sub_str = NSString::from_str(if default_docked {
            "Switch to 720p for this session"
        } else {
            "Switch to 1080p for this session"
        });

        let item: *mut AnyObject = msg_send![
            item,
            initWithType: &*type_str,
            localizedTitle: &*title_str,
            localizedSubtitle: &*sub_str,
            icon: icon,
            userInfo: std::ptr::null::<AnyObject>()
        ];

        let Some(cls_arr) = objc2::runtime::AnyClass::get(c"NSArray") else {
            return;
        };
        let arr: *mut AnyObject = msg_send![cls_arr, arrayWithObject: item];
        let Some(cls_app) = objc2::runtime::AnyClass::get(c"UIApplication") else {
            return;
        };
        let app: *mut AnyObject = msg_send![cls_app, sharedApplication];
        let _: () = msg_send![app, setShortcutItems: arr];
        println!("[+] Display: Quick Action registered -> {}", title_str);
    }
}

fn handle_shortcut_item(item: *mut AnyObject) {
    if item.is_null() {
        return;
    }
    unsafe {
        let type_val: *mut AnyObject = msg_send![item, type];
        if type_val.is_null() {
            return;
        }
        let utf8: *const libc::c_char = msg_send![type_val, UTF8String];
        if utf8.is_null() {
            return;
        }
        let s = std::ffi::CStr::from_ptr(utf8).to_str().unwrap_or("");
        if s.contains("docked") {
            crate::horizon::hle::set_handheld(false);
            println!("[+] Display: launched via Quick Action: Docked Mode (1080p)");
        } else if s.contains("handheld") {
            crate::horizon::hle::set_handheld(true);
            println!("[+] Display: launched via Quick Action: Handheld Mode (720p)");
        }
    }
}

unsafe fn setup_virtual_controller() {
    #[allow(static_mut_refs)]
    if VIRTUAL_CONTROLLER.is_some() {
        return;
    }
    let Some(cls_config) = objc2::runtime::AnyClass::get(c"GCVirtualControllerConfiguration")
    else {
        return;
    };
    let config: *mut AnyObject = msg_send![cls_config, new];
    let Some(cls_set) = objc2::runtime::AnyClass::get(c"NSMutableSet") else {
        return;
    };
    let elements: *mut AnyObject = msg_send![cls_set, new];
    // Apple touch controller supports these 10 elements. Thumbstick and D-pad cannot coexist simultaneously,
    // and Button Menu / Button Options are not supported by the touch overlay.
    let names = [
        "Left Thumbstick",
        "Right Thumbstick",
        "Button A",
        "Button B",
        "Button X",
        "Button Y",
        "Left Shoulder",
        "Right Shoulder",
        "Left Trigger",
        "Right Trigger",
    ];
    for name in names {
        let s = NSString::from_str(name);
        let _: () = msg_send![elements, addObject: &*s];
    }
    let _: () = msg_send![config, setElements: elements];

    let Some(cls_vc) = objc2::runtime::AnyClass::get(c"GCVirtualController") else {
        return;
    };
    let vc: *mut AnyObject = msg_send![cls_vc, alloc];
    let vc: *mut AnyObject = msg_send![vc, initWithConfiguration: config];
    if vc.is_null() {
        eprintln!("[-] Display: failed to allocate GCVirtualController");
        return;
    }
    let _: () = msg_send![vc, connectWithReplyHandler: std::ptr::null::<AnyObject>()];
    VIRTUAL_CONTROLLER = Some(Retained::from_raw(vc).expect("GCVirtualController"));
    CONTROLS_ENABLED.store(true, Ordering::Release);
    println!("[+] Display: on-screen controls enabled (GCVirtualController connected)");

    #[allow(static_mut_refs)]
    if let (Some(view), Some(delegate)) = (ROOT_VIEW, APP_DELEGATE) {
        setup_menu_buttons(view, &*delegate);
    }
}

unsafe fn setup_menu_buttons(view: *mut AnyObject, delegate: &AppDelegate) {
    #[allow(static_mut_refs)]
    if !OVERLAY_BUTTONS.is_empty() {
        return;
    }
    let Some(cls_btn) = objc2::runtime::AnyClass::get(c"UIButton") else {
        return;
    };
    let Some(cls_color) = objc2::runtime::AnyClass::get(c"UIColor") else {
        return;
    };
    let bg_color: *mut AnyObject = msg_send![cls_color, colorWithWhite: 0.3f64, alpha: 0.5f64];
    let text_color: *mut AnyObject = msg_send![cls_color, whiteColor];
    let bounds: CGRect = msg_send![view, bounds];
    for (i, (title, key)) in OVERLAY.iter().enumerate() {
        let btn: *mut AnyObject = msg_send![cls_btn, buttonWithType: 0usize];
        let btn: *mut AnyObject = msg_send![btn, retain];
        let t = NSString::from_str(title);
        let _: () = msg_send![btn, setTitle: &*t, forState: 0usize];
        let _: () = msg_send![btn, setTitleColor: text_color, forState: 0usize];
        let _: () = msg_send![btn, setBackgroundColor: bg_color];
        let _: () = msg_send![btn, setTag: *key as isize];
        let layer: *mut AnyObject = msg_send![btn, layer];
        let _: () = msg_send![layer, setCornerRadius: 18.0f64];
        let _: () = msg_send![layer, setMasksToBounds: true];
        let _: () =
            msg_send![btn, setFrame: overlay_frame(i, bounds.size.width, bounds.size.height)];
        // Touch down; touch up inside / outside, cancel.
        let _: () = msg_send![btn, addTarget: delegate, action: sel!(overlayDown:), forControlEvents: 1usize << 0];
        let _: () = msg_send![btn, addTarget: delegate, action: sel!(overlayUp:), forControlEvents: (1usize << 6) | (1usize << 7) | (1usize << 8)];
        let _: () = msg_send![view, addSubview: btn];
        #[allow(static_mut_refs)]
        OVERLAY_BUTTONS.push(Retained::from_raw(btn).unwrap());
    }
}

unsafe fn enable_virtual_controller() {
    #[allow(static_mut_refs)]
    if VIRTUAL_CONTROLLER.is_none() {
        setup_virtual_controller();
    } else if let Some(vc) = VIRTUAL_CONTROLLER.as_ref() {
        let _: () = msg_send![&**vc, connectWithReplyHandler: std::ptr::null::<AnyObject>()];
        set_overlay_hidden(false);
        println!("[+] Display: on-screen controls enabled (GCVirtualController reconnected)");
    }
    CONTROLS_ENABLED.store(true, Ordering::Release);
}

unsafe fn disable_virtual_controller() {
    #[allow(static_mut_refs)]
    if let Some(vc) = VIRTUAL_CONTROLLER.as_ref() {
        let _: () = msg_send![&**vc, disconnect];
        println!("[+] Display: on-screen controls disabled (GCVirtualController disconnected)");
    }
    set_overlay_hidden(true);
    CONTROLS_ENABLED.store(false, Ordering::Release);
}

/// Centres the layer in `bounds` at the game's aspect ratio (out_size or target_aspect_ratio).
fn fit_layer(bounds: CGRect) {
    #[allow(static_mut_refs)]
    let Some(layer) = (unsafe { LAYER.as_ref() }) else {
        return;
    };
    let (bw, bh) = (bounds.size.width, bounds.size.height);
    let (tar_w, tar_h) = target_aspect_ratio();
    let stretch = crate::settings::get().display.stretch;
    let (w, h) = if stretch
        || crate::config::display_aspect().is_some() && (bw / bh - tar_w / tar_h).abs() < 0.05
    {
        // --stretch, or a set display aspect within 5% of the screen's: fill the screen edge to edge.
        (bw, bh)
    } else if bw / bh > tar_w / tar_h {
        (bh * tar_w / tar_h, bh)
    } else {
        (bw, bw * tar_h / tar_w)
    };
    layer.setFrame(CGRect::new(
        CGPoint::new((bw - w) / 2.0, (bh - h) / 2.0),
        CGSize::new(w, h),
    ));

    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        println!("[+] Display iOS: bounds {:.1}x{:.1}, target aspect {:.2}, layer frame {:.1}x{:.1} (display aspect set: {})",
            bw, bh, tar_w / tar_h, w, h, crate::config::display_aspect().is_some());
    }

    #[allow(static_mut_refs)]
    unsafe {
        for (i, b) in OVERLAY_BUTTONS.iter().enumerate() {
            let _: () = msg_send![&**b, setFrame: overlay_frame(i, bw, bh)];
        }
    }
}

/// Reports each UITouch in `touches` (an NSSet) to host/input.rs: its object address identifies the finger for the touch's
/// lifetime, its position is taken relative to the (letterboxed) game layer.
unsafe fn forward_touches(vc: &GameViewController, touches: &AnyObject, ended: bool) {
    #[allow(static_mut_refs)]
    let Some(layer) = LAYER.as_ref() else {
        return;
    };
    let frame = layer.frame();
    if frame.size.width <= 0.0 || frame.size.height <= 0.0 {
        return;
    }
    let view: *mut AnyObject = msg_send![vc, view];
    let all: *mut AnyObject = msg_send![touches, allObjects];
    let n: usize = msg_send![all, count];
    for i in 0..n {
        let t: *mut AnyObject = msg_send![all, objectAtIndex: i];
        let key = t as usize;
        if ended {
            crate::host::input::touch_ended(key);
        } else {
            let p: CGPoint = msg_send![t, locationInView: view];
            crate::host::input::touch_moved(
                key,
                (p.x - frame.origin.x) / frame.size.width,
                (p.y - frame.origin.y) / frame.size.height,
            );
        }
    }
}

define_class!(
    // Landscape only, no status bar, home indicator hidden after a moment: a game screen.
    #[unsafe(super = UIViewController)]
    #[thread_kind = MainThreadOnly]
    #[name = "NxportGameViewController"]
    struct GameViewController;

    unsafe impl NSObjectProtocol for GameViewController {}

    impl GameViewController {
        #[unsafe(method(prefersStatusBarHidden))]
        fn prefers_status_bar_hidden(&self) -> bool {
            true
        }

        #[unsafe(method(prefersHomeIndicatorAutoHidden))]
        fn prefers_home_indicator_auto_hidden(&self) -> bool {
            true
        }

        // Fingers on the picture are the Switch touch screen (touches the on-screen controller takes never get here).
        #[unsafe(method(touchesBegan:withEvent:))]
        fn touches_began(&self, touches: &AnyObject, _event: *mut AnyObject) {
            unsafe { forward_touches(self, touches, false) };
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &AnyObject, _event: *mut AnyObject) {
            unsafe { forward_touches(self, touches, false) };
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &AnyObject, _event: *mut AnyObject) {
            unsafe { forward_touches(self, touches, true) };
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, touches: &AnyObject, _event: *mut AnyObject) {
            unsafe { forward_touches(self, touches, true) };
        }

        // UIInterfaceOrientationMaskLandscape.
        #[unsafe(method(supportedInterfaceOrientations))]
        fn supported_interface_orientations(&self) -> usize {
            24
        }

        #[unsafe(method(viewDidLayoutSubviews))]
        fn view_did_layout_subviews(&self) {
            unsafe {
                let _: () = msg_send![super(self), viewDidLayoutSubviews];
                let view: *mut AnyObject = msg_send![self, view];
                let bounds: CGRect = msg_send![view, bounds];
                fit_layer(bounds);
            }
        }
    }
);

define_class!(
    #[unsafe(super = UIResponder)]
    #[thread_kind = MainThreadOnly]
    #[name = "NxportAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    impl AppDelegate {
        #[unsafe(method(application:didFinishLaunchingWithOptions:))]
        fn did_finish_launching(&self, _app: &AnyObject, options: *mut AnyObject) -> bool {
            if !options.is_null() {
                unsafe {
                    let k = NSString::from_str("UIApplicationLaunchOptionsShortcutItemKey");
                    let item: *mut AnyObject = msg_send![options, objectForKey: &*k];
                    if !item.is_null() {
                        handle_shortcut_item(item);
                    }
                }
            }
            let mtm = MainThreadMarker::from(self);
            unsafe { launch(mtm, self) };
            true
        }

        #[unsafe(method(application:performActionForShortcutItem:completionHandler:))]
        fn perform_action_for_shortcut_item(
            &self,
            _app: &AnyObject,
            item: *mut AnyObject,
            completion_handler: *mut AnyObject,
        ) {
            unsafe {
                handle_shortcut_item(item);
                if !completion_handler.is_null() {
                    type BlockFn = unsafe extern "C" fn(*mut AnyObject, bool);
                    #[repr(C)]
                    struct BlockLiteral {
                        _isa: *mut libc::c_void,
                        _flags: i32,
                        _reserved: i32,
                        invoke: BlockFn,
                    }
                    let block = completion_handler as *mut BlockLiteral;
                    ((*block).invoke)(completion_handler, true);
                }
            }
        }

        // Toggle on-screen controls via 3-finger tap.
        #[unsafe(method(toggleControls:))]
        fn toggle_controls(&self, _gesture: &AnyObject) {
            unsafe {
                if CONTROLS_ENABLED.load(Ordering::Acquire) {
                    disable_virtual_controller();
                    set_onscreen_controls_pref(false);
                } else {
                    enable_virtual_controller();
                    set_onscreen_controls_pref(true);
                }
            }
        }

        #[unsafe(method(overlayDown:))]
        fn overlay_down(&self, sender: &AnyObject) {
            let tag: isize = unsafe { msg_send![sender, tag] };
            crate::host::input::set_key(tag as u16, true);
        }

        #[unsafe(method(overlayUp:))]
        fn overlay_up(&self, sender: &AnyObject) {
            let tag: isize = unsafe { msg_send![sender, tag] };
            crate::host::input::set_key(tag as u16, false);
        }

        // 250 Hz controller poll (NSTimer).
        #[unsafe(method(poll:))]
        fn poll(&self, _timer: &AnyObject) {
            crate::host::input::poll_controllers(MainThreadMarker::from(self));
        }

        // No GPU work while inactive (metal::SUSPENDED): iOS fails command buffers from background apps. The executor
        // finishes what is in flight first so nothing is still running when the app goes to the background.
        #[unsafe(method(applicationWillResignActive:))]
        fn will_resign_active(&self, _app: &AnyObject) {
            crate::metal::SUSPENDED.store(true, Ordering::Release);
            crate::metal::wait_serial(crate::metal::submitted_serial(), 2_000_000_000);
            crate::host::input::clear_keys();
            println!("[+] Display: app inactive; GPU work suspended");
        }

        #[unsafe(method(applicationDidBecomeActive:))]
        fn did_become_active(&self, _app: &AnyObject) {
            if crate::metal::SUSPENDED.swap(false, Ordering::AcqRel) {
                println!("[+] Display: app active again; GPU work resumed");
            }
            unsafe {
                let enabled = read_onscreen_controls_pref();
                if enabled && !CONTROLS_ENABLED.load(Ordering::Acquire) {
                    enable_virtual_controller();
                } else if !enabled && CONTROLS_ENABLED.load(Ordering::Acquire) {
                    disable_virtual_controller();
                }
                update_shortcut_items();
                { let (ab, xy) = read_controller_swaps(); crate::host::input::set_controller_swaps(ab, xy); }
                sync_ios_settings();
            }
        }

        // The guest has no clean shutdown path; iOS may also kill a backgrounded app at any time.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _app: &AnyObject) {
            println!("[+] Display: app terminating; exiting");
            std::process::exit(0);
        }
    }
);

unsafe fn launch(mtm: MainThreadMarker, delegate: &AppDelegate) {
    update_shortcut_items();
    sync_ios_settings();

    let screen: *mut AnyObject = msg_send![objc2::class!(UIScreen), mainScreen];
    let bounds: CGRect = msg_send![screen, bounds];
    let window: *mut AnyObject = msg_send![objc2::class!(UIWindow), alloc];
    let window: *mut AnyObject = msg_send![window, initWithFrame: bounds];
    let vc: Retained<GameViewController> = msg_send![GameViewController::alloc(mtm), init];
    let view: *mut AnyObject = msg_send![&vc, view];
    let black: *mut AnyObject = msg_send![objc2::class!(UIColor), blackColor];
    let _: () = msg_send![view, setBackgroundColor: black];

    // The executor is already running (crate::run initialises it before the guest starts); the layer joins it here.
    let device = crate::metal::device()
        .unwrap_or_else(|| MTLCreateSystemDefaultDevice().expect("no Metal device"));
    let layer = CAMetalLayer::new();
    layer.setDevice(Some(&device));
    layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    layer.setFramebufferOnly(true);
    let (out_w, out_h) = out_size();
    layer.setDrawableSize(CGSize::new(out_w, out_h));
    let view_layer: *mut AnyObject = msg_send![view, layer];
    let _: () = msg_send![view_layer, addSublayer: &*layer];
    LAYER = Some(layer.clone());
    fit_layer(bounds);

    ROOT_VIEW = Some(view);
    APP_DELEGATE = Some(delegate as *const AppDelegate);

    // 3-finger tap to toggle on-screen controls in-game
    if let Some(cls_tap) = objc2::runtime::AnyClass::get(c"UITapGestureRecognizer") {
        let tap: *mut AnyObject = msg_send![cls_tap, alloc];
        let tap: *mut AnyObject =
            msg_send![tap, initWithTarget: delegate, action: sel!(toggleControls:)];
        let _: () = msg_send![tap, setNumberOfTouchesRequired: 3usize];
        let _: () = msg_send![view, addGestureRecognizer: tap];
    }

    if read_onscreen_controls_pref() {
        enable_virtual_controller();
    } else {
        println!("[+] Display: on-screen controls disabled by settings");
    }
    {
        let (ab, xy) = read_controller_swaps();
        crate::host::input::set_controller_swaps(ab, xy);
    }

    let _: () = msg_send![window, setRootViewController: &*vc];
    let _: () = msg_send![window, makeKeyAndVisible];
    // Kept for the life of the process.
    std::mem::forget(vc);

    crate::metal::attach_layer(layer);

    let _: *mut AnyObject = msg_send![
        objc2::class!(NSTimer),
        scheduledTimerWithTimeInterval: 1.0f64 / 120.0,
        target: delegate,
        selector: sel!(poll:),
        userInfo: std::ptr::null::<AnyObject>(),
        repeats: true
    ];
    WINDOW_UP.store(true, Ordering::Release);
    println!(
        "[+] Display: iOS window up ({}x{} points, drawable {}x{}); presenting guest frames",
        bounds.size.width, bounds.size.height, out_w, out_h
    );
}

extern "C" fn uncaught_exception_handler(exception: *mut AnyObject) {
    unsafe {
        let name: *mut AnyObject = msg_send![exception, name];
        let reason: *mut AnyObject = msg_send![exception, reason];
        let stack: *mut AnyObject = msg_send![exception, callStackSymbols];
        let name_str: *const libc::c_char = if !name.is_null() {
            msg_send![name, UTF8String]
        } else {
            std::ptr::null()
        };
        let reason_str: *const libc::c_char = if !reason.is_null() {
            msg_send![reason, UTF8String]
        } else {
            std::ptr::null()
        };
        let n = if !name_str.is_null() {
            std::ffi::CStr::from_ptr(name_str).to_str().unwrap_or("?")
        } else {
            "?"
        };
        let r = if !reason_str.is_null() {
            std::ffi::CStr::from_ptr(reason_str).to_str().unwrap_or("?")
        } else {
            "?"
        };
        eprintln!("[FATAL] Uncaught Objective-C exception {}: {}", n, r);
        if !stack.is_null() {
            let desc: *mut AnyObject = msg_send![stack, description];
            if !desc.is_null() {
                let s: *const libc::c_char = msg_send![desc, UTF8String];
                if !s.is_null() {
                    eprintln!(
                        "Call stack:\n{}",
                        std::ffi::CStr::from_ptr(s).to_str().unwrap_or("")
                    );
                }
            }
        }
    }
}

extern "C" {
    fn UIApplicationMain(
        argc: i32,
        argv: *mut *mut libc::c_char,
        principal: *const NSString,
        delegate: *const NSString,
    ) -> i32;
    fn NSSetUncaughtExceptionHandler(handler: extern "C" fn(*mut AnyObject));
}

/// Runs UIApplicationMain with the delegate above. Must be called on the process main thread; never returns.
pub fn run_main_thread() -> ! {
    let _mtm =
        MainThreadMarker::new().expect("display::run_main_thread must run on the main thread");
    unsafe {
        NSSetUncaughtExceptionHandler(uncaught_exception_handler);
    }
    // Registers the classes before UIKit looks them up by name.
    let _ = (AppDelegate::class(), GameViewController::class());
    let name = NSString::from_str("NxportAppDelegate");
    unsafe {
        UIApplicationMain(0, std::ptr::null_mut(), std::ptr::null(), &*name);
    }
    std::process::exit(0);
}
