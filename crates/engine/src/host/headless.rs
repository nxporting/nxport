// The host platform where guest code is lifted, until SDL3 gives it a window and input (docs/windows-port.md, step 6):
// no window, no input. Audio is the native hosts' module, whose output device is SDL3's here. The functions the runtime
// calls, with the native host modules' signatures.

#[path = "audio.rs"]
pub mod audio;

pub mod display {
    pub fn window_up() -> bool {
        false
    }

    pub fn headless() -> bool {
        true
    }

    pub fn update_handheld_drawable(_handheld: bool) {}
}

pub mod input {
    /// A contact on the touch screen (host::input::Touch).
    pub struct Touch {
        pub key: usize,
        pub finger_id: i32,
        pub x: i32,
        pub y: i32,
        pub began: std::time::Instant,
        pub ended: Option<std::time::Instant>,
    }

    pub fn touches() -> Vec<Touch> {
        Vec::new()
    }

    pub const MAX_PLAYERS: usize = 8;

    /// (buttons, sticks): nothing pressed.
    pub fn npad_state() -> (u64, [i32; 4]) {
        (0, [0; 4])
    }

    pub fn player_npad_state(_player: usize) -> (u64, [i32; 4]) {
        (0, [0; 4])
    }

    pub fn is_player_connected(player: usize) -> bool {
        player == 0
    }

    pub fn connected_player_count() -> usize {
        1
    }

    pub fn apply_script() {}

    pub fn log_hid(_func: &str, _id: u32, _count: i32) {}
}

pub mod rumble {
    pub fn set(_handle: u32, _v: [f32; 4]) {}
}

pub mod overlay {
    use crate::settings::FpsMode;

    /// The settings' values: there is no overlay to change them.
    pub fn fps_mode() -> FpsMode {
        crate::settings::get().system.fps
    }

    pub fn is_vsync() -> bool {
        !crate::settings::get().display.no_vsync
    }
}
