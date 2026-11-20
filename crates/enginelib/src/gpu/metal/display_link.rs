// --present-mode=display-link: frames are presented into the drawables a CAMetalDisplayLink hands out for the refreshes it
// schedules. The link asks the display for the frame-rate limit (a variable-refresh display then refreshes at the game's
// rate), and each drawable carries the time it will be shown. A frame takes its drawable when the game takes its time step
// (begin_frame), so the step is the time from the previous frame's refresh to this one's: what the frame will be on
// screen for. A step from the last two frames shown is a frame behind, and when the display alternates one and two
// refreshes each frame then moves by the other frame's time (back-and-forth judder).

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, ClassType};
use objc2_foundation::{NSDefaultRunLoopMode, NSObject, NSRunLoop};
use objc2_quartz_core::{
    CAFrameRateRange, CAMetalDisplayLink, CAMetalDisplayLinkDelegate, CAMetalDisplayLinkUpdate, CAMetalDrawable,
    CAMetalLayer,
};
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

type Drawable = Retained<ProtocolObject<dyn CAMetalDrawable>>;

/// A drawable handed from the link's thread to the presenting one (drawables are thread-safe objects).
struct Handed(Drawable);
unsafe impl Send for Handed {}

/// The newest drawable the link handed out and the time it will be shown (host seconds); an older one not taken yet is
/// dropped (that refresh shows the previous frame again).
static NEXT: Mutex<Option<(Handed, f64)>> = Mutex::new(None);
static NEXT_CV: Condvar = Condvar::new();
/// The display link (once started) and the frame rate it was asked for (f32 bits).
static LINK: AtomicPtr<CAMetalDisplayLink> = AtomicPtr::new(std::ptr::null_mut());
static RATE: AtomicU64 = AtomicU64::new(0);
/// Set while the link's thread is making it.
static STARTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Presentation times of the last two frames presented (f64 bits; 0: none yet).
static SHOWN: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
/// The drawable the frame being made took at its time step, and when it will be shown.
static RESERVED: Mutex<Option<(Handed, f64)>> = Mutex::new(None);
/// The refresh the latest frame was given (f64 bits, host seconds).
static LAST_TARGET: AtomicU64 = AtomicU64::new(0);

define_class!(
    // SAFETY: NSObject has no subclassing requirements and LinkDelegate does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "NxportDisplayLinkDelegate"]
    struct LinkDelegate;

    unsafe impl NSObjectProtocol for LinkDelegate {}

    unsafe impl CAMetalDisplayLinkDelegate for LinkDelegate {
        #[unsafe(method(metalDisplayLink:needsUpdate:))]
        fn needs_update(&self, _link: &CAMetalDisplayLink, update: &CAMetalDisplayLinkUpdate) {
            let d = update.drawable();
            let t = update.targetPresentationTimestamp();
            *NEXT.lock().unwrap() = Some((Handed(d), t));
            NEXT_CV.notify_all();
        }
    }
);

/// Starts the link on `layer` (once), or resumes it; asks for `fps` (the frame-rate limit, at most `max_hz`).
pub(super) fn run(layer: &CAMetalLayer, fps: f32, max_hz: f32) {
    let link = LINK.load(Ordering::Acquire);
    if link.is_null() {
        if STARTING.swap(true, Ordering::AcqRel) {
            return; // the thread is making it
        }
        let layer_ptr = layer as *const CAMetalLayer as usize;
        let _ = std::thread::Builder::new().name("display-link".into()).spawn(move || unsafe {
            let layer = &*(layer_ptr as *const CAMetalLayer);
            let link = CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), layer);
            let delegate: Retained<LinkDelegate> = msg_send![LinkDelegate::class(), new];
            link.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            std::mem::forget(delegate);
            link.addToRunLoop_forMode(&NSRunLoop::currentRunLoop(), NSDefaultRunLoopMode);
            LINK.store(Retained::into_raw(link), Ordering::Release);
            STARTING.store(false, Ordering::Release);
            println!("[+] Display link: started");
            NSRunLoop::currentRunLoop().run();
        });
        // The rate is set once the thread made the link.
        RATE.store(0, Ordering::Relaxed);
        return;
    }
    let link = unsafe { &*link };
    if link.isPaused() {
        link.setPaused(false);
    }
    let want = fps.min(max_hz).max(1.0);
    if RATE.swap(want.to_bits() as u64, Ordering::Relaxed) != want.to_bits() as u64 {
        link.setPreferredFrameRateRange(CAFrameRateRange { minimum: want.min(30.0), maximum: max_hz, preferred: want });
        println!("[+] Display link: frame rate {want} (display up to {max_hz})");
    }
}

/// Ends the link (another presentation mode took over): the layer hands out drawables itself only while no display link
/// is attached to it (nextDrawable raises CAMetalLayerInvalidOperation otherwise, paused or not).
pub(super) fn stop() {
    let link = LINK.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !link.is_null() {
        let link = unsafe { Retained::from_raw(link) }.unwrap();
        link.invalidate();
        RATE.store(0, Ordering::Relaxed);
        println!("[+] Display link: stopped");
    }
    NEXT.lock().unwrap().take();
    RESERVED.lock().unwrap().take();
    LAST_TARGET.store(0, Ordering::Relaxed);
}

/// Whether a display link is attached (or being started) now.
pub(super) fn active() -> bool {
    !LINK.load(Ordering::Acquire).is_null() || STARTING.load(Ordering::Acquire)
}

/// The next drawable the link hands out (waits for one, up to `timeout`), and the time it will be shown.
pub(super) fn next(timeout: std::time::Duration) -> Option<(Drawable, f64)> {
    let mut g = NEXT.lock().unwrap();
    let end = std::time::Instant::now() + timeout;
    while g.is_none() {
        let now = std::time::Instant::now();
        if now >= end {
            return None;
        }
        g = NEXT_CV.wait_timeout(g, end - now).unwrap().0;
    }
    g.take().map(|(d, t)| (d.0, t))
}

/// A frame for the refresh at `shown` (host seconds) was presented.
pub(super) fn presented(shown: f64) {
    SHOWN[0].store(SHOWN[1].load(Ordering::Relaxed), Ordering::Relaxed);
    SHOWN[1].store(shown.to_bits(), Ordering::Relaxed);
    if shown > f64::from_bits(LAST_TARGET.load(Ordering::Relaxed)) {
        LAST_TARGET.store(shown.to_bits(), Ordering::Relaxed);
    }
}

/// At the game's time step: takes the drawable for the next refresh the link scheduled and returns the time from the
/// previous frame's refresh to it (seconds). None while no link runs, while the previous frame's drawable has not been
/// presented yet, or before a first refresh to measure from.
pub fn begin_frame() -> Option<f64> {
    if LINK.load(Ordering::Acquire).is_null() || RESERVED.lock().unwrap().is_some() {
        return None;
    }
    let (d, t) = next(std::time::Duration::from_millis(100))?;
    *RESERVED.lock().unwrap() = Some((Handed(d), t));
    let last = f64::from_bits(LAST_TARGET.swap(t.to_bits(), Ordering::Relaxed));
    (last > 0.0 && t > last).then_some(t - last)
}

/// The drawable begin_frame took for this frame, if any.
pub(super) fn take_reserved() -> Option<(Drawable, f64)> {
    RESERVED.lock().unwrap().take().map(|(d, t)| (d.0, t))
}

/// The time between the last two frames presented through the link (seconds): the game's step.
pub fn step() -> Option<f64> {
    let (a, b) = (f64::from_bits(SHOWN[0].load(Ordering::Relaxed)), f64::from_bits(SHOWN[1].load(Ordering::Relaxed)));
    (a > 0.0 && b > a).then_some(b - a)
}
