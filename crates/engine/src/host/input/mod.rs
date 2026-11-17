// Player input: keyboard and game controllers mapped onto one Switch Npad (nn::hid::NpadFullKeyState).
//
// Every Switch input (buttons, +Control Pad, each stick direction) has one keyboard binding and one controller
// binding; both are active at once. Bindings are edited in the Controls window (host/controls_ui.rs, Cmd-,) and saved to
// ~/Library/Application Support/nxport/controls.cfg (--controls-file overrides the path). Windows reads the same file
// from %APPDATA%\nxport\controls.cfg: keys are macOS virtual key codes there too (host/display_sdl.rs translates SDL's
// scancodes into them), so a controls file means the same keys on both. Past them, codes 128 to 132 are the mouse's
// buttons (MOUSE_KEYS: key=MouseLeft and so on in the file), pressed while mouse look holds the mouse.
//
// Threading: AppKit and GameController objects are only touched on the main thread (keyboard events arrive there; a
// 250 Hz timer polls controllers), and guest threads must not call into Objective-C (they clobber thread-local state
// libobjc relies on). The main thread therefore folds keyboard + controller state through the bindings into a finished
// Npad state kept in atomics, which nn::hid::GetNpadStates (horizon/hle.rs) reads. On Windows SDL's event loop on the
// main thread does the same (host/display_sdl.rs).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

mod bindings;
mod state;
mod mouse;
mod debug;
mod controllers;
mod key_names;
pub use bindings::*;
pub use state::*;
pub use mouse::*;
pub use debug::*;
pub use controllers::*;
pub use key_names::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_turns_with_the_gyroscope_and_levels_with_gravity() {
        // A quarter turn about up (Z) in one second, in 200 readings: the controller's X axis points along the world's Y.
        let p = 7;
        motion_clear(p);
        let w = std::f64::consts::FRAC_PI_2;
        for i in 0..=200u64 {
            motion_gyro(p, [0.0, 0.0, w], i * 5_000_000);
        }
        let (_, gyro, angle, dir) = motion_state(p).unwrap();
        assert!((gyro[2] - 0.25).abs() < 1e-6, "{gyro:?}");
        assert!((angle[2] - 0.25).abs() < 1e-3, "{angle:?}");
        assert!(dir[0].abs() < 1e-3 && (dir[1] - 1.0).abs() < 1e-3, "{dir:?}");
        // Tilted 30 degrees about X while the accelerometer reads flat: the tilt goes, the yaw stays.
        motion_gyro(p, [0.5236, 0.0, 0.0], 1_005_000_000 + 1_000_000_000);
        for _ in 0..1000 {
            motion_accel(p, [0.0, 0.0, -1.0]);
        }
        let (_, _, _, dir) = motion_state(p).unwrap();
        // The controller's Z axis back to the world's up.
        assert!((dir[8] - 1.0).abs() < 1e-3, "{dir:?}");
        assert!((dir[1] - 1.0).abs() < 1e-2, "{dir:?}");
        motion_clear(p);
    }

    /// A touch's start and release each reach the game on exactly one read, however long apart the reads are.
    #[test]
    fn touch_start_and_end_are_settled_by_reads() {
        let _ = crate::settings::try_install(crate::settings::resolve([]));
        const KEY: usize = 0x7e57_7e57;
        let mine = |v: Vec<Touch>| v.into_iter().find(|c| c.key == KEY);
        touch_moved(KEY, 0.5, 0.5);
        touch_moved(KEY, 0.6, 0.5);
        let first = mine(touches()).expect("reported");
        assert!(first.fresh && !first.ended);
        assert_eq!((first.x, first.y), ((0.6f64 * 1279.0).round() as i32, (0.5f64 * 719.0).round() as i32));
        let second = mine(touches()).expect("still held");
        assert!(!second.fresh && !second.ended);
        touch_ended(KEY);
        let released = mine(touches()).expect("its release is reported");
        assert!(released.ended && released.finger_id == first.finger_id);
        assert!(mine(touches()).is_none(), "gone after its release was read");
    }

    #[test]
    fn mouse_buttons_are_keys() {
        for (i, &(code, name)) in MOUSE_KEYS.iter().enumerate() {
            assert!((code as usize) < KEYS.len(), "{name}");
            #[cfg(target_os = "macos")]
            assert_ne!(key_name(code), "?", "{name}");
            assert_eq!(code, 128 + i as u16);
        }
    }

    #[test]
    fn mouse_look_deflection_follows_speed() {
        // At rest: centred.
        assert_eq!(mouse_look_deflection(0.0, 0.0, 1.0, 1.0), (0.0, 0.0));
        // Slow movement still passes the games' deadzone; fast movement saturates; mouse down is stick down.
        let (x, y) = mouse_look_deflection(30.0, 0.0, 1.0, 1.0);
        assert!(x > MOUSE_LOOK_DEADZONE && y == 0.0, "{x}");
        let (x, y) = mouse_look_deflection(0.0, 10_000.0, 1.0, 1.0);
        assert!(x.abs() < 1e-6 && (y + 1.0).abs() < 1e-6, "{x} {y}");
        // Faster means further, and the speed setting scales it.
        assert!(mouse_look_deflection(600.0, 0.0, 1.0, 1.0).0 > mouse_look_deflection(300.0, 0.0, 1.0, 1.0).0);
        assert!(mouse_look_deflection(300.0, 0.0, 2.0, 1.0).0 > mouse_look_deflection(300.0, 0.0, 1.0, 1.0).0);
        // Diagonals stay inside the unit circle.
        let (x, y) = mouse_look_deflection(5000.0, -5000.0, 1.0, 1.0);
        assert!((x * x + y * y).sqrt() <= 1.0 + 1e-5);
        // A game that takes a longer stick: fast movement goes past full deflection, up to that length.
        let (x, _) = mouse_look_deflection(3000.0, 0.0, 1.0, 4.0);
        assert!(x > 1.5 && x <= 4.0, "{x}");
        assert!((mouse_look_deflection(100_000.0, 0.0, 1.0, 4.0).0 - 4.0).abs() < 1e-5);
    }

    #[test]
    fn mouse_state_layout() {
        let s = mouse_state(9, true, [640, 360], [-3, 4], 120, 0b101);
        let word = |o: usize| i32::from_le_bytes(s[o..o + 4].try_into().unwrap());
        assert_eq!(u64::from_le_bytes(s[0..8].try_into().unwrap()), 9);
        assert_eq!([word(8), word(12), word(16), word(20), word(24)], [640, 360, -3, 4, 120]);
        assert_eq!(word(28), 0, "reserved");
        assert_eq!(word(32), 0b101);
        assert_eq!(word(36), 2, "connected");
        let off = mouse_state(9, false, [640, 360], [-3, 4], 120, 1);
        assert!(off[8..].iter().all(|b| *b == 0));
    }

    #[test]
    fn keyboard_state_layout() {
        // A (usage 4), left Shift (225), right Alt (230).
        let keys = [1 << 4, 0, 0, 1 << 33 | 1 << 38];
        let s = keyboard_state(7, true, keys);
        assert_eq!(u64::from_le_bytes(s[0..8].try_into().unwrap()), 7);
        assert_eq!(u32::from_le_bytes(s[8..12].try_into().unwrap()), 0b0_1010);
        assert_eq!(u32::from_le_bytes(s[12..16].try_into().unwrap()), 1);
        assert_eq!(s[16], 1 << 4);
        assert_eq!(s[44], 1 << 1 | 1 << 6);
    }

    #[test]
    fn keyboard_state_off_shows_no_keys() {
        let s = keyboard_state(7, false, [u64::MAX; 4]);
        assert_eq!(u64::from_le_bytes(s[0..8].try_into().unwrap()), 7);
        assert!(s[8..].iter().all(|b| *b == 0));
    }

    #[test]
    fn keyboard_and_mouse_calls_stay_in_their_buffers() {
        use crate::horizon::hle::*;
        unsafe {
            let mut buf = [0xaau8; 0x40];
            crate::horizon::hid::get_keyboard_state(buf.as_mut_ptr());
            assert!(buf[0x30..].iter().all(|b| *b == 0xaa));

            // Asked for two, given one, and the second is left alone.
            let mut buf = [0xaau8; 0x70];
            let mut written = -1;
            assert_eq!(crate::horizon::hid::detail_get_keyboard_states(&mut written, buf.as_mut_ptr(), 2), 0);
            assert_eq!(written, 1);
            assert!(buf[0x30..].iter().all(|b| *b == 0xaa));

            let mut buf = [0xaau8; 0x60];
            let mut written = -1;
            assert_eq!(crate::horizon::hid::detail_get_mouse_states(&mut written, buf.as_mut_ptr(), 2), 0);
            assert_eq!(written, 1);
            assert!(buf[0x28..].iter().all(|b| *b == 0xaa));
        }
    }

    #[test]
    fn test_multiplayer_player_slots() {
        assert_eq!(MAX_PLAYERS, 8);
        assert!(is_player_connected(0));
        assert!(!is_player_connected(1));
        assert!(!is_player_connected(7));
        assert!(!is_player_connected(8));
        assert_eq!(connected_player_count(), 1);

        PLAYER_CONNECTED[1].store(true, Ordering::SeqCst);
        assert!(is_player_connected(1));
        assert_eq!(connected_player_count(), 2);
        PLAYER_CONNECTED[1].store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_player_npad_state_out_of_bounds() {
        let (buttons, sticks) = player_npad_state(MAX_PLAYERS);
        assert_eq!(buttons, 0);
        assert_eq!(sticks, [0; 4]);
    }
}
