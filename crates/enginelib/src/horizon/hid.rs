// nn::hid as the game calls it: controllers (Npad styles, states, handles), six-axis sensors, vibration and touch.

use crate::horizon::call_log::*;
use std::sync::Mutex as StdMutex;


pub(crate) unsafe extern "C" fn initialize_npad() {
    println!("[+] HLE: nn::hid::InitializeNpad() called");
}

pub(crate) unsafe extern "C" fn set_supported_npad_id_type(_npad_ids: *const u32, count: usize) {
    println!(
        "[+] HLE: nn::hid::SetSupportedNpadIdType(count={}) called",
        count
    );
}

pub(crate) unsafe extern "C" fn set_supported_npad_style_set(style_set: u32) {
    println!(
        "[+] HLE: nn::hid::SetSupportedNpadStyleSet(0x{:X}) called",
        style_set
    );
    HID_SUPPORTED_STYLES.store(style_set, std::sync::atomic::Ordering::Relaxed);
}

/// BindNpadStyleSetUpdateEvent(const NpadIdType&, SystemEventType*, EventClearMode): the event a game waits on to
/// re-read an Npad's styles. Our controllers are connected from the start, so it begins signaled once (the update
/// a connection gives on the console). The SDK's own implementation opens hid over IPC (Super Mario Party Jamboree
/// aborted on it).
pub(crate) unsafe extern "C" fn bind_npad_style_set_update_event(_id: *const u32, event: *mut u8, _clear_mode: i32) {
    if !event.is_null() {
        crate::horizon::os::objects::hle_system_event_register_signaled(event as usize);
    }
}

pub(crate) unsafe extern "C" fn get_supported_npad_style_set() -> u32 {
    HID_SUPPORTED_STYLES.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) unsafe extern "C" fn get_npad_joy_hold_type() -> i32 {
    HID_HOLD_TYPE.load(std::sync::atomic::Ordering::Relaxed)
}

/// NpadJoyAssignmentMode: Dual = 0 (one Npad, both Joy-Con or a full controller), which is what the input layer reports.
pub(crate) unsafe extern "C" fn get_npad_joy_assignment(_id: *const u32) -> i32 {
    0
}

/// Assignment-mode and LR-assignment changes, Joy-Con swaps and the strap guide applet: one fixed controller layout.
pub(crate) unsafe extern "C" fn assignment_noop() {}

/// GetTouchScreenState<16>: {i64 samplingNumber, i32 count, reserved[4], TouchState touches[N]},
/// TouchState (0x28): {i64 deltaTime, u32 attributes (bit 0 Start, bit 1 End), i32 fingerId, i32 x, i32 y,
/// i32 diameterX, i32 diameterY, i32 rotationAngle, reserved[4]}, coordinates in the 1280x720 panel. The capacity N is
/// part of the imported symbol: only the <16> instantiation is bound to this function.
pub(crate) unsafe extern "C" fn get_touch_screen_state(out: *mut u8) {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| println!("[+] HLE: nn::hid::GetTouchScreenState<16> polled (window mouse / touches are the touch screen)"));
    write_touch_screen_state(out, 16);
}

pub(crate) unsafe extern "C" fn get_touch_screen_state1(out: *mut u8) {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| println!("[+] HLE: nn::hid::GetTouchScreenState<1> polled (window mouse / touches are the touch screen)"));
    write_touch_screen_state(out, 1);
}

pub(crate) unsafe extern "C" fn get_touch_screen_state5(out: *mut u8) {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| println!("[+] HLE: nn::hid::GetTouchScreenState<5> polled (window mouse / touches are the touch screen)"));
    write_touch_screen_state(out, 5);
}

pub(crate) unsafe extern "C" fn get_six_axis_sensor_state(out: *mut u8, _handle: *const u32) {
    if out.is_null() {
        return;
    }
    crate::host::input::log_hid("GetSixAxisSensorState", if _handle.is_null() { u32::MAX } else { *_handle }, 1);
    let samp = (crate::horizon::os::get_system_tick() / 96_000) as i64; // ~5ms ticks at 19.2MHz
    let mut state = SixAxisSensorState::default();
    state.sampling_number = samp;
    with_motion(&mut state);
    std::ptr::copy_nonoverlapping(&state, out as *mut SixAxisSensorState, 1);
}

pub(crate) unsafe extern "C" fn get_six_axis_sensor_states(
    out: *mut u8,
    count: i32,
    _handle: *const u32,
) -> i32 {
    if out.is_null() || count <= 0 {
        return 0;
    }
    // (the handle's first word: its type and npad id, as the game made it)
    crate::host::input::log_hid("GetSixAxisSensorStates", if _handle.is_null() { u32::MAX } else { *_handle }, count);
    let n = count.min(16) as usize;
    let samp = (crate::horizon::os::get_system_tick() / 96_000) as i64;
    let states_ptr = out as *mut SixAxisSensorState;
    for i in 0..n {
        let mut state = SixAxisSensorState::default();
        state.sampling_number = samp - i as i64;
        with_motion(&mut state);
        std::ptr::copy_nonoverlapping(&state, states_ptr.add(i), 1);
    }
    n as i32
}

pub(crate) unsafe extern "C" fn is_six_axis_sensor_at_rest(_handle: *const u32) -> bool {
    true
}

pub(crate) unsafe extern "C" fn get_npad_style_set(_npad_id: *const u32) -> u32 {
    let id = if _npad_id.is_null() {
        u32::MAX
    } else {
        *_npad_id
    };
    crate::host::input::log_hid("GetNpadStyleSet", id, 0);
    // NpadId: No1..No8 (0..7), Handheld (0x20).
    // In docked/standard setup, players 0..7 report FullKey (bit 0 = 1) if their controller is connected.
    // Handheld (0x20) reports 0 to preserve docked Pro Controller behavior for games like TotK / MK8D.
    if id < 8 && crate::host::input::is_player_connected(id as usize) {
        1
    } else {
        0
    }
}

pub(crate) unsafe extern "C" fn get_npad_states(
    states: *mut NpadFullKeyState,
    count: i32,
    _npad_id: *const u32,
) -> i32 {
    crate::host::input::log_hid(
        "GetNpadStates(FullKey)",
        if _npad_id.is_null() {
            u32::MAX
        } else {
            *_npad_id
        },
        count,
    );
    npad_states(states, count, _npad_id, 0)
}

/// GetTouchScreenStates<16>(states, count): the latest state only.
pub(crate) unsafe extern "C" fn get_touch_screen_states(out: *mut u8, count: i32) -> i32 {
    if count < 1 {
        return 0;
    }
    write_touch_screen_state(out, 16);
    1
}

pub(crate) unsafe extern "C" fn get_npad_joy_dual_states(
    states: *mut NpadFullKeyState,
    count: i32,
    npad_id: *const u32,
) -> i32 {
    crate::host::input::log_hid(
        "GetNpadStates(JoyDual)",
        if npad_id.is_null() {
            u32::MAX
        } else {
            *npad_id
        },
        count,
    );
    npad_states(states, count, npad_id, (1 << 2) | (1 << 4)) // IsLeftConnected | IsRightConnected
}

pub(crate) unsafe extern "C" fn get_npad_handheld_states(
    states: *mut NpadFullKeyState,
    count: i32,
    npad_id: *const u32,
) -> i32 {
    crate::host::input::log_hid(
        "GetNpadStates(Handheld)",
        if npad_id.is_null() {
            u32::MAX
        } else {
            *npad_id
        },
        count,
    );
    npad_states(states, count, npad_id, (1 << 2) | (1 << 4))
}

pub(crate) unsafe extern "C" fn get_six_axis_sensor_handles(
    handles: *mut u32,
    count: i32,
    _npad_id: *const u32,
    style: u32,
) -> i32 {
    // Returns the number of handles obtained, which depends on the style (count is only the
    // buffer's capacity). Per style as in Ryujinx's Npad: FullKey 1, Handheld 2 (both Joy-Con), JoyDual 2, JoyLeft /
    // JoyRight 1. Returning `count` (8) made Kirby loop over eight sensors and overwrite the controller state it had
    // just read on its stack: every button press was lost.
    let sensors = if style & 0b0110 != 0 { 2 } else { 1 }; // Handheld (bit 1) / JoyDual (bit 2)
    let n = sensors.min(count.max(0));
    if !handles.is_null() {
        for i in 0..n {
            *handles.add(i as usize) = (i + 1) as u32;
        }
    }
    n
}

pub(crate) unsafe extern "C" fn get_vibration_device_handles(
    handles: *mut u32,
    count: i32,
    _npad_id: *const u32,
    style: u32,
) -> i32 {
    // Like GetSixAxisSensorHandles: the number of vibration devices the style has (one per motor: Pro Controller,
    // handheld and Joy-Con pair 2; a single Joy-Con 1), not the buffer's capacity.
    let devices = if style & 0b11000 != 0 && style & 0b00111 == 0 {
        1
    } else {
        2
    }; // JoyLeft (bit 3) / JoyRight (bit 4)
    let n = devices.min(count.max(0));
    if !handles.is_null() {
        for i in 0..n {
            *handles.add(i as usize) = (i + 1) as u32;
        }
    }
    n
}

pub(crate) unsafe extern "C" fn start_six_axis_sensor(_handle: *const u32) -> i32 {
    println!("[+] HLE: nn::hid::StartSixAxisSensor called -> returning 0");
    0
}

pub(crate) unsafe extern "C" fn stop_six_axis_sensor(_handle: *const u32) -> i32 {
    0
}

pub(crate) unsafe extern "C" fn initialize_vibration_device(_handle: *const u32) -> i32 {
    println!("[+] HLE: nn::hid::InitializeVibrationDevice called -> returning 0");
    0
}

pub(crate) unsafe extern "C" fn set_npad_joy_hold_type(_hold_type: i32) {
    HID_HOLD_TYPE.store(_hold_type, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) unsafe extern "C" fn send_vibration_value(
    handle: *const u32,
    val: *const [f32; 4],
) -> i32 {
    // nn::hid::VibrationValue: amplitudeLow, frequencyLow, amplitudeHigh, frequencyHigh (host/rumble.rs plays it).
    if !handle.is_null() && !val.is_null() {
        send_vibration(*handle, std::ptr::read_unaligned(val));
    }
    0
}

/// The value a motor is vibrating with: the last one sent to it, or the neutral value (no amplitude at the 160 / 320 Hz
/// resonance frequencies) before any.
pub(crate) unsafe extern "C" fn current_vibration(out: *mut [f32; 4], handle: *const u32) -> i32 {
    if !out.is_null() {
        let sent = if handle.is_null() {
            None
        } else {
            VIBRATION_SENT.lock().unwrap().as_ref().and_then(|m| m.get(&*handle).copied())
        };
        std::ptr::write_unaligned(out, sent.unwrap_or([0.0, 160.0, 0.0, 320.0]));
    }
    0
}

pub(crate) unsafe extern "C" fn send_vibration_values(
    handles: *const u32,
    vals: *const [f32; 4],
    count: i32,
) -> i32 {
    if !handles.is_null() && !vals.is_null() {
        for i in 0..count.max(0) as usize {
            send_vibration(*handles.add(i), std::ptr::read_unaligned(vals.add(i)));
        }
    }
    0
}

pub(crate) unsafe extern "C" fn is_vibration_permitted(out_permitted: *mut bool) -> i32 {
    if !out_permitted.is_null() {
        *out_permitted = true;
    }
    0
}

pub(crate) unsafe extern "C" fn get_vibration_device_info(
    info_out: *mut u32,
    _handle: *const u32,
) -> i32 {
    if !info_out.is_null() {
        *info_out = 1; // LinearResonantActuator (HD Rumble)
        *info_out.add(1) = 1; // Left
    }
    0
}

pub(crate) unsafe extern "C" fn initialize_mouse() -> i32 {
    println!("[+] HLE: nn::hid::InitializeMouse() called -> returning 0");
    0
}

pub(crate) unsafe extern "C" fn initialize_keyboard() -> i32 {
    println!("[+] HLE: nn::hid::InitializeKeyboard() called -> returning 0");
    0
}

pub(crate) unsafe extern "C" fn initialize_touch_screen() -> i32 {
    println!("[+] HLE: nn::hid::InitializeTouchScreen() called -> returning 0");
    0
}

pub(crate) unsafe extern "C" fn get_mouse_state(state: *mut u8) {
    if !state.is_null() {
        let bytes = crate::host::input::next_mouse_state();
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), state, bytes.len());
    }
}

/// The newest state only, as for the keyboard.
pub(crate) unsafe extern "C" fn get_mouse_states(states: *mut u8, count: i32) -> i32 {
    if states.is_null() || count <= 0 {
        return 0;
    }
    crate::horizon::hid::get_mouse_state(states);
    1
}

/// nn::hid::detail::GetMouseStates(int* written, MouseState*, int): takes the count's address and returns a Result.
pub(crate) unsafe extern "C" fn detail_get_mouse_states(written: *mut i32, states: *mut u8, count: i32) -> u32 {
    let n = crate::horizon::hid::get_mouse_states(states, count);
    if !written.is_null() {
        *written = n;
    }
    0
}

pub(crate) unsafe extern "C" fn get_keyboard_state(state: *mut u8) {
    if !state.is_null() {
        let bytes = crate::host::input::next_keyboard_state();
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), state, bytes.len());
    }
}

/// The newest state only, as there is no history to give: a caller gets one state however many it asks for.
pub(crate) unsafe extern "C" fn get_keyboard_states(states: *mut u8, count: i32) -> i32 {
    if states.is_null() || count <= 0 {
        return 0;
    }
    crate::horizon::hid::get_keyboard_state(states);
    1
}

/// nn::hid::detail::GetKeyboardStates(int* written, KeyboardState*, int): takes the count's address and returns a Result.
pub(crate) unsafe extern "C" fn detail_get_keyboard_states(written: *mut i32, states: *mut u8, count: i32) -> u32 {
    let n = crate::horizon::hid::get_keyboard_states(states, count);
    if !written.is_null() {
        *written = n;
    }
    0
}

pub(crate) unsafe extern "C" fn initialize_debug_pad() {
    println!("[+] HLE: nn::hid::InitializeDebugPad called");
}

pub(crate) unsafe extern "C" fn get_debug_pad_state(state: *mut u8) {
    if !state.is_null() {
        std::ptr::write_bytes(state, 0, 64);
    }
}

pub(crate) unsafe extern "C" fn controller_support_arg_set_default(arg: *mut u8) {
    if !arg.is_null() {
        std::ptr::write_bytes(arg, 0, 0x100);
    }
}

pub(crate) unsafe extern "C" fn show_controller_support(
    result_info: *mut u8,
    arg: *const u8,
) -> i32 {
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if n < 3 || n % 10000 == 0 {
        let fp = crate::guest_reg!(x29);
        let bytes = if arg.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(arg, 0x20).to_vec()
        };
        println!(
            "[+] HLE: nn::hid::ShowControllerSupport() call #{} arg={:02x?} callers: {}",
            n,
            bytes,
            guest_chain(fp)
        );
    }
    // ControllerSupportResultInfo { int8 playerCount; u8 pad[3]; NpadIdType selectedId; ... }.
    // playerCount == 0 means the user cancelled, and the game then shows the applet again, forever.
    // A real session ends with the requested player count and Npad No1 selected.
    if !result_info.is_null() {
        std::ptr::write_bytes(result_info, 0, 0x40);
        let min_players = if arg.is_null() { 1 } else { (*arg).max(1) as i8 };
        let max_players = if arg.is_null() { 8 } else { (*arg.add(1)).max(min_players as u8) as i8 };
        let connected = crate::host::input::connected_player_count() as i8;
        let player_count = connected.clamp(min_players, max_players);
        *(result_info as *mut i8) = player_count;
        *(result_info.add(4) as *mut u32) = 0; // NpadId::No1
    }
    0
}

pub(crate) unsafe extern "C" fn initialize_seven_six_axis_sensor(_buf: *mut u8, _size: usize) -> i32 {
    println!("[+] HLE: nn::hid::InitializeSevenSixAxisSensor called");
    0
}

pub(crate) unsafe extern "C" fn finalize_seven_six_axis_sensor() {
    println!("[+] HLE: nn::hid::FinalizeSevenSixAxisSensor called");
}

pub(crate) unsafe extern "C" fn start_seven_six_axis_sensor() -> i32 {
    println!("[+] HLE: nn::hid::StartSevenSixAxisSensor called");
    0
}

pub(crate) unsafe extern "C" fn stop_seven_six_axis_sensor() -> i32 {
    println!("[+] HLE: nn::hid::StopSevenSixAxisSensor called");
    0
}

pub(crate) unsafe extern "C" fn is_seven_six_axis_sensor_at_rest() -> bool {
    true
}

pub(crate) unsafe extern "C" fn get_seven_six_axis_sensor_states(out: *mut u8, count: i32) -> i32 {
    if !out.is_null() && count > 0 {
        std::ptr::write_bytes(out, 0, (count as usize) * 0x60);
    }
    0
}

pub(crate) unsafe extern "C" fn reset_seven_six_axis_sensor_timestamp() {
    println!("[+] HLE: nn::hid::ResetSevenSixAxisSensorTimestamp called");
}

pub(crate) unsafe extern "C" fn get_states_zero(_out: *mut u8, _count: i32, _id: *const u8) -> i32 {
    crate::host::input::log_hid(
        "GetNpadStates(JoyLeft/JoyRight/SixAxis)",
        if _id.is_null() {
            u32::MAX
        } else {
            *(_id as *const u32)
        },
        _count,
    );
    0 // no controller input source yet: report zero new states
}

pub(crate) unsafe extern "C" fn set_handheld_activation_mode(mode: i64) {
    HANDHELD_ACTIVATION_MODE.store(mode, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) unsafe extern "C" fn get_handheld_activation_mode() -> i64 {
    HANDHELD_ACTIVATION_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) unsafe extern "C" fn disconnect_npad(_id: *const u8) {}
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub(crate) struct NpadFullKeyState {
    pub sampling_number: i64,
    pub buttons: u64,
    pub analog_stick_l_x: i32,
    pub analog_stick_l_y: i32,
    pub analog_stick_r_x: i32,
    pub analog_stick_r_y: i32,
    pub attributes: u32,
    pub _reserved: u32,
}

pub(crate) static SAMPLING_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

pub(crate) static HID_SUPPORTED_STYLES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// NpadJoyHoldType: Vertical = 0 (the default), Horizontal = 1.
pub(crate) static HID_HOLD_TYPE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Fills a TouchScreenState<n> from the window's contacts (host::input::touches).
pub(crate) unsafe fn write_touch_screen_state(out: *mut u8, n: usize) {
    if out.is_null() {
        return;
    }
    std::ptr::write_bytes(out, 0, 16 + n * 0x28);
    static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let now = std::time::Instant::now();
    // The touch panel is sampled at ~200 Hz on the console: the sampling number advances with time.
    let sampling = now.duration_since(*T0.get_or_init(|| now)).as_millis() as i64 / 5;
    *(out as *mut i64) = sampling;
    let touches = crate::host::input::touches();
    let count = touches.len().min(n);
    *(out.add(8) as *mut i32) = count as i32;
    for (i, t) in touches.iter().take(count).enumerate() {
        let p = out.add(16 + i * 0x28);
        let mut attributes = 0u32;
        if t.fresh {
            attributes |= 1; // Start
        }
        if t.ended {
            attributes |= 2; // End
        }
        *(p as *mut i64) = 0;
        *(p.add(8) as *mut u32) = attributes;
        *(p.add(12) as *mut i32) = t.finger_id;
        *(p.add(16) as *mut i32) = t.x;
        *(p.add(20) as *mut i32) = t.y;
        *(p.add(24) as *mut i32) = 15; // diameterX / Y: a fingertip
        *(p.add(28) as *mut i32) = 15;
    }
}

/// SixAxisSensorState (96 bytes): 200 Hz accelerometer + gyroscope samples.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct SixAxisSensorState {
    pub delta_ns: i64,
    pub sampling_number: i64,
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub angle: [f32; 3],
    pub direction: [f32; 9],
    pub attributes: u32,
    pub _pad: u32,
}

impl Default for SixAxisSensorState {
    fn default() -> Self {
        Self {
            delta_ns: 5_000_000, // 5 ms (200 Hz)
            sampling_number: 0,
            acceleration: [0.0, 0.0, -1.0], // Gravity vector: 1G downwards
            angular_velocity: [0.0, 0.0, 0.0],
            angle: [0.0, 0.0, 0.0],
            direction: [
                1.0, 0.0, 0.0,
                0.0, 1.0, 0.0,
                0.0, 0.0, 1.0,
            ],
            attributes: 1, // SixAxisSensorAttribute_IsConnected
            _pad: 0,
        }
    }
}

/// The first player's controller motion (host/input.rs), when it has a gyroscope; otherwise the default: flat and still.
pub(crate) fn with_motion(state: &mut SixAxisSensorState) {
    if let Some((acc, w, angle, dir)) = crate::host::input::motion_state(0) {
        state.acceleration = acc;
        state.angular_velocity = w;
        state.angle = angle;
        state.direction = dir;
    }
}

/// The shared body of the GetNpadStates overloads (all state structs have the NpadFullKeyState layout).
/// Supports up to 8 players (NpadId No1..No8, plus Handheld mapped to player 0);
/// `extra_attributes` adds style-specific attribute bits (JoyDual: IsLeftConnected | IsRightConnected).
pub(crate) unsafe fn npad_states(
    states: *mut NpadFullKeyState,
    count: i32,
    _npad_id: *const u32,
    extra_attributes: u32,
) -> i32 {
    let id = if _npad_id.is_null() {
        u32::MAX
    } else {
        *_npad_id
    };
    let player = match id {
        0..=7 => id as usize,
        0x20 => 0, // Handheld maps to player 0
        _ => usize::MAX,
    };
    let connected = player < crate::host::input::MAX_PLAYERS
        && crate::host::input::is_player_connected(player);
    if states.is_null() || count <= 0 {
        return 0;
    }
    if player == 0 {
        crate::host::input::before_npad_read();
    }
    // Keyboard / controller state mapped by the user's bindings (host/input.rs).
    let (buttons, sticks) = if connected {
        crate::host::input::player_npad_state(player)
    } else {
        (0, [0; 4])
    };
    // --log-hid-buttons: every read that returns pressed buttons (which id / style path the game polls).
    static LOG_BUTTONS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if buttons != 0 && *LOG_BUTTONS.get_or_init(|| crate::settings::get().log.log_hid_buttons) {
        let fp = crate::guest_reg!(x29);
        let lr = if fp > 0x10000 {
            *((fp + 8) as *const usize)
        } else {
            0
        };
        let _ = lr;
        println!(
            "[hid-buttons] frame {} id {:#x} player {} buttons {:#x} attrs {:#x} callers: {}",
            crate::gpu::backend().presented_frames(),
            id,
            player,
            buttons,
            1 | extra_attributes,
            guest_chain(fp)
        );
    }
    let n = count.min(1) as usize;
    for i in 0..n {
        let cur = states.add(i);
        std::ptr::write(
            cur,
            NpadFullKeyState {
                sampling_number: SAMPLING_COUNTER
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                buttons: buttons as _,
                analog_stick_l_x: sticks[0],
                analog_stick_l_y: sticks[1],
                analog_stick_r_x: sticks[2],
                analog_stick_r_y: sticks[3],
                attributes: if connected { 1 | extra_attributes } else { 0 }, // IsConnected
                _reserved: 0,
            },
        );
    }
    n as i32
}

/// The last vibration value sent to each motor handle.
pub(crate) static VIBRATION_SENT: StdMutex<Option<rustc_hash::FxHashMap<u32, [f32; 4]>>> = StdMutex::new(None);

pub(crate) fn send_vibration(handle: u32, v: [f32; 4]) {
    VIBRATION_SENT.lock().unwrap().get_or_insert_with(Default::default).insert(handle, v);
    crate::host::rumble::set(handle, v);
}
/// Which handheld Joy-Cons count as an active controller (both / either / none); the engine's controller is the
/// keyboard / gamepad, so the mode is only kept for the game to read back.
pub(crate) static HANDHELD_ACTIVATION_MODE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
