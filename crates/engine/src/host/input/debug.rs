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
    let mut want = [false; 128];
    for held in script {
        if (held.first..held.last).contains(&frame) {
            for &k in &held.keys {
                if (k as usize) < 128 {
                    want[k as usize] = true;
                }
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
