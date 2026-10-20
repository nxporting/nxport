// A window that runs a Dear ImGui interface on the main thread: an AppKit view feeding imgui's input, a CAMetalLayer
// drawn by host::ui::renderer at the display's refresh rate.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEvent,
    NSEventModifierFlags, NSModalResponseOK, NSOpenPanel, NSPasteboard, NSPasteboardTypeString,
    NSResponder, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLPixelFormat,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};

use dear_imgui_rs as imgui;

use super::renderer::Renderer;

/// The interface's text size, in points.
const FONT_POINTS: f32 = 15.0;

/// Draws one frame of the interface; returning false closes the window.
pub type Frame = Box<dyn FnMut(&imgui::Ui) -> bool>;

struct State {
    ctx: imgui::Context,
    renderer: Renderer,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    layer: Retained<CAMetalLayer>,
    view: Retained<UiView>,
    window: Retained<NSWindow>,
    frame: Frame,
    last: Instant,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_io(f: impl FnOnce(&mut imgui::Io)) {
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            f(s.ctx.io_mut());
        }
    });
}

define_class!(
    // SAFETY: NSView has no subclassing requirements and UiView does not implement Drop.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct UiView;

    unsafe impl NSObjectProtocol for UiView {}

    impl UiView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        // Top-left origin, as imgui's coordinates.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, e: &NSEvent) {
            self.mouse(e, Some((imgui::MouseButton::Left, true)));
        }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, e: &NSEvent) {
            self.mouse(e, Some((imgui::MouseButton::Left, false)));
        }
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, e: &NSEvent) {
            self.mouse(e, Some((imgui::MouseButton::Right, true)));
        }
        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, e: &NSEvent) {
            self.mouse(e, Some((imgui::MouseButton::Right, false)));
        }
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, e: &NSEvent) {
            self.mouse(e, None);
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, e: &NSEvent) {
            self.mouse(e, None);
        }
        #[unsafe(method(rightMouseDragged:))]
        fn right_mouse_dragged(&self, e: &NSEvent) {
            self.mouse(e, None);
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, e: &NSEvent) {
            // The end of a two-finger tap can carry a large delta (a jump); AppKit marks it cancelled.
            if e.phase() == objc2_app_kit::NSEventPhase::Cancelled {
                return;
            }
            // Trackpad deltas are in points: moved 1:1 (Dear ImGui scrolls 5 lines of text per wheel unit). Mouse
            // wheel deltas are in lines already.
            let k = if e.hasPreciseScrollingDeltas() { 1.0 / (5.0 * FONT_POINTS) } else { 1.0 };
            let (dx, dy) = (e.scrollingDeltaX() as f32 * k, e.scrollingDeltaY() as f32 * k);
            with_io(|io| io.add_mouse_wheel_event([dx, dy]));
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, e: &NSEvent) {
            self.key(e, true);
        }
        #[unsafe(method(keyUp:))]
        fn key_up(&self, e: &NSEvent) {
            self.key(e, false);
        }
        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, e: &NSEvent) {
            modifiers(e.modifierFlags());
        }
    }
);

impl UiView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] }
    }

    fn mouse(&self, e: &NSEvent, button: Option<(imgui::MouseButton, bool)>) {
        let p = self.convertPoint_fromView(e.locationInWindow(), None);
        with_io(|io| {
            io.add_mouse_pos_event([p.x as f32, p.y as f32]);
            if let Some((b, down)) = button {
                io.add_mouse_button_event(b, down);
            }
        });
    }

    fn key(&self, e: &NSEvent, down: bool) {
        modifiers(e.modifierFlags());
        if let Some(k) = key_for(e.keyCode()) {
            with_io(|io| io.add_key_event(k, down));
        }
        if down && !e.modifierFlags().contains(NSEventModifierFlags::Command) {
            if let Some(chars) = e.characters() {
                let text = chars.to_string();
                with_io(|io| {
                    text.chars()
                        .filter(|c| !c.is_control() && !('\u{f700}'..='\u{f8ff}').contains(c))
                        .for_each(|c| io.add_input_character(c))
                });
            }
        }
    }
}

fn modifiers(f: NSEventModifierFlags) {
    with_io(|io| {
        io.add_key_event(
            imgui::Key::ModShift,
            f.contains(NSEventModifierFlags::Shift),
        );
        io.add_key_event(
            imgui::Key::ModCtrl,
            f.contains(NSEventModifierFlags::Control),
        );
        io.add_key_event(imgui::Key::ModAlt, f.contains(NSEventModifierFlags::Option));
        io.add_key_event(
            imgui::Key::ModSuper,
            f.contains(NSEventModifierFlags::Command),
        );
    });
}

/// macOS virtual key codes for the keys imgui's widgets use.
fn key_for(code: u16) -> Option<imgui::Key> {
    use imgui::Key as K;
    Some(match code {
        48 => K::Tab,
        123 => K::LeftArrow,
        124 => K::RightArrow,
        125 => K::DownArrow,
        126 => K::UpArrow,
        116 => K::PageUp,
        121 => K::PageDown,
        115 => K::Home,
        119 => K::End,
        117 => K::Delete,
        51 => K::Backspace,
        49 => K::Space,
        36 => K::Enter,
        76 => K::KeypadEnter,
        53 => K::Escape,
        0 => K::A,
        8 => K::C,
        9 => K::V,
        7 => K::X,
        16 => K::Y,
        6 => K::Z,
        _ => return None,
    })
}

struct Pasteboard;

impl imgui::ClipboardBackend for Pasteboard {
    fn get(&mut self) -> Option<String> {
        let pb = NSPasteboard::generalPasteboard();
        unsafe { pb.stringForType(NSPasteboardTypeString) }.map(|s| s.to_string())
    }
    fn set(&mut self, value: &str) {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        unsafe { pb.setString_forType(&NSString::from_str(value), NSPasteboardTypeString) };
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Ticker does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    struct Ticker;

    unsafe impl NSObjectProtocol for Ticker {}

    impl Ticker {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &AnyObject) {
            let keep = draw();
            // A panel asked for during the frame runs now, outside it (its modal loop keeps ticking frames).
            if let Some((key, kind)) = PICK.with(|p| p.borrow_mut().0.take()) {
                let mtm = MainThreadMarker::from(self);
                if let Some(path) = choose_path(mtm, kind) {
                    PICK.with(|p| p.borrow_mut().1.insert(key, path));
                }
            }
            if !keep {
                let mtm = MainThreadMarker::from(self);
                if let Some(s) = STATE.with(|s| s.borrow_mut().take()) {
                    s.window.close();
                }
                let app = NSApplication::sharedApplication(mtm);
                app.stop(None);
                // stop: takes effect after the next event; post one.
                unsafe {
                    let ev: Option<Retained<NSEvent>> = msg_send![objc2::class!(NSEvent),
                        otherEventWithType: 15usize /* NSEventTypeApplicationDefined */,
                        location: NSPoint::new(0.0, 0.0), modifierFlags: 0usize, timestamp: 0.0f64,
                        windowNumber: 0isize, context: std::ptr::null::<AnyObject>(), subtype: 0i16, data1: 0isize, data2: 0isize];
                    if let Some(ev) = ev {
                        app.postEvent_atStart(&ev, true);
                    }
                }
            }
        }
    }
);

/// What a file panel lets the user choose.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    File,
    Directory,
    Either,
}

thread_local! {
    /// A file panel requested during a frame (key, what it chooses), and the paths chosen by key.
    static PICK: RefCell<(Option<(String, PathKind)>, HashMap<String, PathBuf>)> = RefCell::new((None, HashMap::new()));
}

/// Opens a file panel after this frame; its choice is collected with `take_path(key)`.
pub fn request_path(key: &str, kind: PathKind) {
    PICK.with(|p| p.borrow_mut().0 = Some((key.to_string(), kind)));
}

/// The path chosen for `key` since the last call, if any.
pub fn take_path(key: &str) -> Option<PathBuf> {
    PICK.with(|p| p.borrow_mut().1.remove(key))
}

fn choose_path(mtm: MainThreadMarker, kind: PathKind) -> Option<PathBuf> {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(kind != PathKind::File);
    panel.setCanChooseFiles(kind != PathKind::Directory);
    panel.setAllowsMultipleSelection(false);
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    panel
        .URL()
        .and_then(|u| u.path())
        .map(|p| PathBuf::from(p.to_string()))
}

/// One frame: input already queued by the view, then the interface, then the draw. False when it asked to close.
fn draw() -> bool {
    STATE.with(|s| {
        let mut guard = s.borrow_mut();
        let Some(s) = guard.as_mut() else {
            return false;
        };
        let scale = s.window.backingScaleFactor();
        let size = s.view.bounds().size;
        s.layer
            .setDrawableSize(NSSize::new(size.width * scale, size.height * scale));
        let now = Instant::now();
        let io = s.ctx.io_mut();
        io.set_display_size([size.width as f32, size.height as f32]);
        io.set_display_framebuffer_scale([scale as f32, scale as f32]);
        io.set_delta_time((now - s.last).as_secs_f32().max(1e-4));
        s.last = now;
        let frame = s.ctx.begin_frame();
        let keep = (s.frame)(frame.ui());
        let pending = frame.render(s.renderer.consumer());
        let Some(drawable) = s.layer.nextDrawable() else {
            return keep;
        };
        let Some(cb) = s.queue.commandBuffer() else {
            return keep;
        };
        if let Err(e) = s.renderer.render(
            pending,
            &cb,
            &drawable.texture(),
            Some([0.09, 0.09, 0.10, 1.0]),
        ) {
            eprintln!("[!] UI: {e}");
        }
        cb.presentDrawable(ProtocolObject::from_ref(&*drawable));
        cb.commit();
        keep
    })
}

/// Opens a window running `frame` every display refresh until it returns false or the window is closed, then returns.
/// Must be called on the main thread.
pub fn run(title: &str, width: f64, height: f64, frame: Frame) {
    let mtm = MainThreadMarker::new().expect("host::ui::window::run must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    let rect = NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(width, height));
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
    window.setTitle(&NSString::from_str(title));
    unsafe { window.setReleasedWhenClosed(false) };
    window.setAcceptsMouseMovedEvents(true);
    window.center();

    let device = MTLCreateSystemDefaultDevice().expect("no Metal device");
    let layer = CAMetalLayer::new();
    layer.setDevice(Some(&device));
    layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    let view = UiView::new(mtm, rect);
    view.setWantsLayer(true);
    view.setLayer(Some(&layer));
    window.setContentView(Some(&view));
    window.makeFirstResponder(Some(&view));

    let mut ctx = new_context();
    ctx.set_clipboard_backend(Pasteboard);
    let renderer =
        Renderer::new(&device, &mut ctx, MTLPixelFormat::BGRA8Unorm).expect("imgui renderer");
    let queue = device.newCommandQueue().expect("command queue");

    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            ctx,
            renderer,
            queue,
            layer,
            view,
            window: window.clone(),
            frame,
            last: Instant::now(),
        })
    });
    window.makeKeyAndOrderFront(None);
    app.activate();

    let ticker: Retained<Ticker> = unsafe { msg_send![Ticker::alloc(mtm), init] };
    let timer: Retained<AnyObject> = unsafe {
        msg_send![objc2::class!(NSTimer), scheduledTimerWithTimeInterval: 1.0f64 / 60.0, target: &*ticker,
            selector: sel!(tick:), userInfo: std::ptr::null::<AnyObject>(), repeats: true]
    };
    // Closing the window with its button ends the interface too.
    let close_watch: Retained<CloseWatch> = unsafe { msg_send![CloseWatch::alloc(mtm), init] };
    window.setDelegate(Some(ProtocolObject::from_ref(&*close_watch)));
    app.run();
    unsafe {
        let _: () = msg_send![&timer, invalidate];
    }
    window.setDelegate(None);
    STATE.with(|s| s.borrow_mut().take());
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and CloseWatch does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    struct CloseWatch;

    unsafe impl NSObjectProtocol for CloseWatch {}

    unsafe impl NSWindowDelegate for CloseWatch {
        #[unsafe(method(windowWillClose:))]
        fn will_close(&self, _n: &objc2_foundation::NSNotification) {
            // The next tick sees no state and stops the run loop.
            STATE.with(|s| {
                if let Ok(mut s) = s.try_borrow_mut() {
                    if let Some(st) = s.as_mut() {
                        st.frame = Box::new(|_| false);
                    }
                }
            });
        }
    }
);

fn new_context() -> imgui::Context {
    let mut ctx = imgui::Context::create();
    let _ = ctx.set_ini_filename(None::<String>);
    ctx.io_mut().set_config_macosx_behaviors(true);
    load_font(&mut ctx, FONT_POINTS);
    ctx
}

/// Renders `frames` frames of the interface at `width` x `height` points and `scale` pixels per point into an
/// offscreen texture, without a window: the last frame's pixels (width, height, BGRA rows).
pub fn snapshot(
    width: f64,
    height: f64,
    scale: f64,
    frames: usize,
    mut frame: Frame,
) -> Result<(usize, usize, Vec<u8>), String> {
    use objc2_metal::{
        MTLOrigin, MTLRegion, MTLSize, MTLStorageMode, MTLTexture, MTLTextureDescriptor,
        MTLTextureUsage,
    };
    let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
    let mut ctx = new_context();
    let mut renderer = Renderer::new(&device, &mut ctx, MTLPixelFormat::BGRA8Unorm)?;
    let queue = device.newCommandQueue().ok_or("command queue")?;
    let (pw, ph) = ((width * scale) as usize, (height * scale) as usize);
    let td = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::BGRA8Unorm,
            pw,
            ph,
            false,
        )
    };
    td.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    td.setStorageMode(MTLStorageMode::Shared);
    let target = device
        .newTextureWithDescriptor(&td)
        .ok_or("snapshot texture")?;
    for _ in 0..frames.max(1) {
        let io = ctx.io_mut();
        io.set_display_size([width as f32, height as f32]);
        io.set_display_framebuffer_scale([scale as f32, scale as f32]);
        io.set_delta_time(1.0 / 60.0);
        let f = ctx.begin_frame();
        frame(f.ui());
        let pending = f.render(renderer.consumer());
        let cb = queue.commandBuffer().ok_or("command buffer")?;
        renderer.render(pending, &cb, &target, Some([0.09, 0.09, 0.10, 1.0]))?;
        cb.commit();
        cb.waitUntilCompleted();
    }
    let mut px = vec![0u8; pw * ph * 4];
    unsafe {
        target.getBytes_bytesPerRow_fromRegion_mipmapLevel(
            std::ptr::NonNull::new(px.as_mut_ptr() as *mut libc::c_void).unwrap(),
            pw * 4,
            MTLRegion {
                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                size: MTLSize {
                    width: pw,
                    height: ph,
                    depth: 1,
                },
            },
            0,
        );
    }
    Ok((pw, ph, px))
}

/// The system UI font at `points` (Dear ImGui rasterizes it at the display's backing scale).
fn load_font(ctx: &mut imgui::Context, points: f32) {
    let paths = [
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
    ];
    let source = paths
        .iter()
        .find_map(|p| imgui::StbTrueTypeFontData::from_file(p).ok())
        .map(imgui::FontSource::stb_truetype)
        .unwrap_or_else(imgui::FontSource::default_font);
    ctx.font_atlas().add_font(&[source]);
    ctx.style_mut().set_font_size_base(points);
}
