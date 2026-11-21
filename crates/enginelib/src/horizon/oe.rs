// nn::oe as the game calls it: application state (focus, operation mode, performance mode, display) and launch parameters.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::collections::VecDeque;

// 7. Horizon Memory & Heap Management
pub(crate) unsafe extern "C" fn set_cpu_boost_mode(mode: u32) -> i32 {
    println!("[+] HLE: nn::oe::SetCpuBoostMode(mode={})", mode);
    0
}

// 8. Operation Environment (nn::oe) and Account
pub(crate) unsafe extern "C" fn initialize() {
    println!("[+] HLE: nn::oe::Initialize() called");
}

pub(crate) unsafe extern "C" fn set_focus_handling_mode(mode: i32) {
    println!("[+] HLE: nn::oe::SetFocusHandlingMode(mode={})", mode);
}

pub(crate) unsafe extern "C" fn set_performance_configuration(mode: i32, config: i32) {
    println!(
        "[+] HLE: nn::oe::SetPerformanceConfiguration(mode={}, config={})",
        mode, config
    );
}

pub(crate) unsafe extern "C" fn set_resume_notification_enabled(enabled: bool) {
    println!("[+] HLE: nn::oe::SetResumeNotificationEnabled({})", enabled);
}

pub(crate) unsafe extern "C" fn set_operation_mode_changed_notification_enabled(enabled: bool) {
    println!(
        "[+] HLE: nn::oe::SetOperationModeChangedNotificationEnabled({})",
        enabled
    );
}

pub(crate) unsafe extern "C" fn set_performance_mode_changed_notification_enabled(enabled: bool) {
    println!(
        "[+] HLE: nn::oe::SetPerformanceModeChangedNotificationEnabled({})",
        enabled
    );
}

pub(crate) unsafe extern "C" fn get_operation_mode() -> i32 {
    if is_handheld() {
        0 // Handheld mode
    } else {
        1 // Docked mode
    }
}

pub(crate) unsafe extern "C" fn get_performance_mode() -> i32 {
    if is_handheld() {
        0 // Normal / Handheld 720p
    } else {
        1 // Boosted / Docked 1080p
    }
}

pub(crate) unsafe extern "C" fn get_performance_configuration(mode: i32) -> u32 {
    let config = if mode == 1 {
        0x00010001
    } else {
        0x00020003
    };
    println!(
        "[+] HLE: nn::oe::GetPerformanceConfiguration(mode={}) -> 0x{:08x}",
        mode, config
    );
    config
}

pub(crate) unsafe extern "C" fn get_current_focus_state() -> i32 {
    1 // InFocus
}

pub(crate) unsafe extern "C" fn set_user_inactivity_detection_time_extended(_extended: bool) {}

pub(crate) unsafe extern "C" fn get_desired_language() -> u64 {
    u64::from_le_bytes(*b"en-US\0\0\0")
}

// ── nn::oe / nn::pl / nn::account / nn::err ──────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn get_expected_volume_balance(app: *mut f32, sdk: *mut f32) {
    if !app.is_null() {
        *app = 1.0;
    }
    if !sdk.is_null() {
        *sdk = 1.0;
    }
}

pub(crate) unsafe extern "C" fn set_expected_volume_balance(_app: f32, _sdk: f32) {}

pub(crate) unsafe extern "C" fn finish_startup_logo() {}

/// Asks the system to deliver MessageExitRequest instead of terminating the program; no exit
/// requests are generated here (quitting the window ends the process), so entering / leaving the section changes nothing.
pub(crate) unsafe extern "C" fn enter_exit_request_handling_section() {}

pub(crate) unsafe extern "C" fn leave_exit_request_handling_section() {}

pub(crate) fn push_notification_message(msg: u32) {
    let mut g = OE_MESSAGES.lock().unwrap();
    let q = g.get_or_insert_with(|| VecDeque::from([15u32]));
    q.push_back(msg);
    drop(g);
    message_event_update();
}

pub(crate) fn pop() -> Option<u32> {
    let m = OE_MESSAGES
        .lock()
        .unwrap()
        .get_or_insert_with(|| VecDeque::from([15u32]))
        .pop_front();
    message_event_update();
    m
}

/// The SystemEventType GetNotificationMessageEvent hands out: signaled while messages are queued.
static MESSAGE_EVENT: Mutex<[u64; 8]> = Mutex::new([0; 8]);

fn message_event_addr() -> usize {
    MESSAGE_EVENT.lock().unwrap().as_ptr() as usize
}

fn message_event_update() {
    let addr = message_event_addr();
    let Some(ev) = crate::horizon::os::objects::hle_system_event_lookup(addr) else {
        return;
    };
    let pending = OE_MESSAGES.lock().unwrap().as_ref().map_or(true, |q| !q.is_empty());
    *ev.signaled.lock().unwrap() = pending;
    if pending {
        ev.cv.notify_all();
    }
}

/// nn::oe::GetNotificationMessageEvent(): the event games wait on before TryPopNotificationMessage; manual-clear and
/// kept in step with the message queue.
pub(crate) unsafe extern "C" fn get_notification_message_event() -> usize {
    let addr = message_event_addr();
    if crate::horizon::os::objects::hle_system_event_lookup(addr).is_none() {
        let ev = crate::horizon::os::objects::hle_system_event_register(addr);
        ev.manual.store(true, std::sync::atomic::Ordering::Relaxed);
        message_event_update();
    }
    addr
}

/// nn::oe::GetDefaultDisplayResolution(int* width, int* height): the resolution of the current operation mode.
pub(crate) unsafe extern "C" fn get_default_display_resolution(w: *mut i32, h: *mut i32) {
    let (rw, rh) = if is_handheld() { (1280, 720) } else { (1920, 1080) };
    if !w.is_null() {
        *w = rw;
    }
    if !h.is_null() {
        *h = rh;
    }
}

/// nn::oe::SetPerformanceConfiguration(int config): the form without a mode (both modes).
pub(crate) unsafe extern "C" fn set_performance_configuration_all(config: i32) {
    println!("[+] HLE: nn::oe::SetPerformanceConfiguration(config={})", config);
}

/// The launch parameter channel: a game started normally has none (the native call reached the applet manager's
/// storage objects, which do not exist here, and read through null).
pub(crate) unsafe extern "C" fn try_pop_launch_parameter(out_size: *mut usize, _buf: *mut u8, _cap: usize) -> bool {
    if !out_size.is_null() {
        *out_size = 0;
    }
    false
}

pub(crate) unsafe extern "C" fn try_pop_notification_message(out: *mut u32) -> bool {
    match crate::horizon::oe::pop() {
        Some(m) => {
            if !out.is_null() {
                *out = m;
            }
            true
        }
        None => false,
    }
}

pub(crate) unsafe extern "C" fn pop_notification_message() -> u32 {
    crate::horizon::oe::pop().unwrap_or(0)
}

pub(crate) unsafe extern "C" fn get_program_total_active_time() -> i64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos() as i64
}

/// nn::oe::GetPseudoDeviceId(nn::util::Uuid*): a fixed id (the SDK's own asks a service).
pub(crate) unsafe extern "C" fn get_pseudo_device_id(out: *mut u8) {
    if !out.is_null() {
        let id: [u8; 16] = *b"nxport-device-id";
        std::ptr::copy_nonoverlapping(id.as_ptr(), out, 16);
    }
}
pub(crate) static HANDHELD_OVERRIDE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

pub(crate) fn set_handheld(handheld: bool) {
    HANDHELD_OVERRIDE.store(
        if handheld { 1 } else { 0 },
        std::sync::atomic::Ordering::Release,
    );
    crate::horizon::oe::push_notification_message(0x1e); // MessageOperationModeChanged
    crate::horizon::oe::push_notification_message(0x1f); // MessagePerformanceModeChanged
    #[cfg(target_os = "macos")]
    crate::host::display::update_handheld_drawable(handheld);
    if handheld {
        println!("[+] HLE: Handheld mode active (720p target)");
    } else {
        println!("[+] HLE: Docked mode active (1080p target)");
    }
}

pub(crate) fn is_handheld() -> bool {
    match HANDHELD_OVERRIDE.load(std::sync::atomic::Ordering::Acquire) {
        0 => false,
        1 => true,
        _ => {
            static HANDHELD_ENV: OnceLock<bool> = OnceLock::new();
            *HANDHELD_ENV.get_or_init(|| {
                let h = crate::settings::get().system.handheld;
                if h {
                    println!(
                        "[+] HLE: Handheld mode active (720p target; --docked for docked 1080p)"
                    );
                } else {
                    println!(
                        "[+] HLE: Docked mode active (1080p target; --handheld for handheld 720p)"
                    );
                }
                h
            })
        }
    }
}
/// Notification messages for the application. After launch the system reports the focus
/// state once (MessageFocusStateChanged = 15; GetCurrentFocusState then says InFocus), as Ryujinx's applet state
/// manager does. Kirby and the Forgotten Land (FocusHandlingMode_SuspendAndNotify) ignored all input without it.
pub(crate) static OE_MESSAGES: Mutex<Option<VecDeque<u32>>> = Mutex::new(None);
