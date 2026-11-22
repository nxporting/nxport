// nn::vi as the game calls it: displays and layers.

use std::ffi::CStr;
use std::sync::Mutex as StdMutex;
use crate::horizon::os::objects::*;

pub(crate) unsafe extern "C" fn initialize() -> i32 {
    println!("[+] HLE: nn::vi::Initialize() called");
    0
}

pub(crate) unsafe extern "C" fn open_default_display(out_display: *mut *mut usize) -> i32 {
    println!("[+] HLE: nn::vi::OpenDefaultDisplay called");
    if !out_display.is_null() {
        *out_display = &raw mut DUMMY_DISPLAY;
    }
    0
}

pub(crate) unsafe extern "C" fn create_layer(
    out_layer: *mut *mut usize,
    _display: *mut usize,
) -> i32 {
    println!("[+] HLE: nn::vi::CreateLayer called");
    if !out_layer.is_null() {
        *out_layer = &raw mut DUMMY_LAYER;
    }
    0
}

/// The layers are the engine's (CreateLayer): the presenter scales the game's image to the window whatever the mode.
pub(crate) unsafe extern "C" fn set_layer_scaling_mode(_layer: *mut usize, mode: i32) -> i32 {
    println!("[+] HLE: nn::vi::SetLayerScalingMode({mode})");
    0
}

pub(crate) unsafe extern "C" fn get_display_vsync_event(
    out_event: *mut u8,
    _display: *mut usize,
) -> i32 {
    println!("[+] HLE: nn::vi::GetDisplayVsyncEvent called");
    if !out_event.is_null() {
        // The vsync event is an auto-clear readable event signaled once per refresh (60Hz).
        // Register it in the SystemEvent table so Wait/TryWait work, and drive it from a pacer.
        let ev = hle_system_event_register(out_event as usize);
        nn_vi_start_vsync_pacer(ev);
    }
    0
}

pub(crate) unsafe extern "C" fn get_native_window(
    out_window: *mut *mut usize,
    _layer: *mut usize,
) -> i32 {
    println!("[+] HLE: nn::vi::GetNativeWindow called");
    if !out_window.is_null() {
        *out_window = &raw mut DUMMY_WINDOW;
    }
    0
}
pub(crate) static mut DUMMY_DISPLAY: usize = 0x1000;

pub(crate) static mut DUMMY_LAYER: usize = 0x2000;

pub(crate) static mut DUMMY_WINDOW: usize = 0x3000;

/// The display's refresh period the vsync event follows: the console's 60 Hz, unless a setting raises the game's frame
/// rate above it (games that pace themselves by the vsync event, such as Unity's player, cannot go faster than it).
static VSYNC_PERIOD_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(16_666_667);

/// Sets the vsync event's rate in Hz (60: the console's).
pub(crate) fn set_vsync_rate(hz: u32) {
    let ns = 1_000_000_000 / hz.max(1) as u64;
    if VSYNC_PERIOD_NS.swap(ns, std::sync::atomic::Ordering::Relaxed) != ns {
        println!("[+] Display: the vsync event at {hz} Hz");
    }
}

/// `wait` moved to the display's nearest refresh when the window shows frames on it: the console's vsync is its
/// screen's refresh, and games that pace by the vsync event (Unity's player) would otherwise drift against the refreshes
/// our presents land on (a 60 Hz game on a 120 Hz screen then misses every so many refreshes and shows 40 fps).
fn on_refresh(wait: std::time::Duration) -> std::time::Duration {
    #[cfg(nx_native)]
    if crate::gpu::metal::display_pacing() && crate::host::overlay::paces_to_refresh() {
        if let (Some(to_grid), Some(period)) = (crate::gpu::metal::display_pacing_wait(), crate::gpu::metal::display_period()) {
            if wait.as_secs_f64() >= 0.75 * period {
                let j = ((wait.as_secs_f64() - to_grid.as_secs_f64()) / period).round().max(0.0);
                return to_grid + std::time::Duration::from_secs_f64(j * period);
            }
        }
    }
    wait
}

pub(crate) fn nn_vi_start_vsync_pacer(ev: std::sync::Arc<HleSystemEvent>) {
    static EVENTS: StdMutex<Vec<std::sync::Arc<HleSystemEvent>>> = StdMutex::new(Vec::new());
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    EVENTS.lock().unwrap().push(ev);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("hle-vsync".into())
        .spawn(|| {
            let period = || std::time::Duration::from_nanos(VSYNC_PERIOD_NS.load(std::sync::atomic::Ordering::Relaxed));
            let mut next = std::time::Instant::now() + period();
            loop {
                let now = std::time::Instant::now();
                if next > now {
                    std::thread::sleep(on_refresh(next - now));
                }
                // A late wake-up (or a change of rate) starts the grid again rather than signalling to catch up.
                next = (next + period()).max(std::time::Instant::now());
                for e in EVENTS.lock().unwrap().iter() {
                    hle_system_event_signal(e);
                }
            }
        })
        .expect("spawn vsync pacer");
}

pub(crate) static PRESENT_PACED: StdMutex<Vec<&'static str>> = StdMutex::new(Vec::new());

/// Light message queue receives on threads with this name return only after the next present (at most 20 ms later).
pub(crate) fn pace_to_present(thread_name: &'static str) {
    PRESENT_PACED.lock().unwrap().push(thread_name);
}

pub(crate) fn present_paced_wait() {
    thread_local! {
        static PACED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    }
    let paced = PACED.with(|p| {
        if let Some(v) = p.get() {
            return v;
        }
        let v = (|| {
            let names = PRESENT_PACED.lock().unwrap();
            if names.is_empty() {
                return false;
            }
            // The name the game gave this thread (nn::os::SetThreadName), the same on every host; the host thread's own
            // name is set differently per platform.
            let p = unsafe { crate::horizon::os::get_thread_name_pointer(std::ptr::null()) };
            let name = if p.is_null() { &[][..] } else { unsafe { CStr::from_ptr(p) }.to_bytes() };
            names.iter().any(|n| n.as_bytes() == name)
        })();
        p.set(Some(v));
        v
    });
    if !paced {
        return;
    }
    let Some(g) = crate::gpu::try_backend() else { return };
    let f0 = g.presented_frames();
    let until = std::time::Instant::now() + std::time::Duration::from_millis(20);
    while g.presented_frames() == f0 && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
}
