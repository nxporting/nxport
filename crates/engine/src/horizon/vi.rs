// nn::vi as the game calls it: displays and layers.

use super::hle::*;

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
