// Input self-test and debug input.

use super::*;

// ---- debug ----

/// Debug: --debug-input-script=2600-2900:13,3000-3300:34+37 holds the given key codes (here W, then I+L) while the
/// presented frame number is in each range. Applied from nn::hid reads, so it also works headless.
pub fn apply_script() {
    let script = &crate::settings::get().debug.debug_input_script;
    if script.is_empty() {
        return;
    }
    let frame = crate::gpu::backend().presented_frames();
    // The mouse moves once per presented frame (the game may read its controller more often).
    static MOVED_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);
    let new_frame = MOVED_AT.swap(frame as u64, Ordering::Relaxed) != frame as u64;
    let mut want = [false; 128];
    let mut mouse = (0, 0);
    for held in script {
        if (held.first..held.last).contains(&frame) {
            for &k in &held.keys {
                if (k as usize) < 128 {
                    want[k as usize] = true;
                }
            }
            if let Some((x, y)) = held.mouse {
                mouse = (mouse.0 + x, mouse.1 + y);
            }
        }
    }
    let mut changed = false;
    for (k, w) in KEYS.iter().zip(want) {
        changed |= k.swap(w, Ordering::Relaxed) != w;
    }
    if changed {
        println!(
            "[input-script] frame {}: keys {:?}",
            frame,
            (0..128).filter(|&i| want[i]).collect::<Vec<_>>()
        );
        update_npad();
    }
    // After the keys: a held tilt key takes the movement.
    if new_frame && mouse != (0, 0) {
        mouse_look_moved(mouse.0 as f32, mouse.1 as f32);
    }
}

/// Debug: --log-hid prints each distinct (function, npad id, count) the game reads, once.
pub fn log_hid(func: &str, id: u32, count: i32) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| crate::settings::get().log.log_hid) {
        return;
    }
    static SEEN: Mutex<Vec<(String, u32, i32)>> = Mutex::new(Vec::new());
    let mut s = SEEN.lock().unwrap();
    let k = (func.to_string(), id, count);
    if !s.contains(&k) {
        println!(
            "[hid] {} npad id {:#x} count {} (frame {})",
            func,
            id,
            count,
            crate::gpu::backend().presented_frames()
        );
        s.push(k);
    }
}
