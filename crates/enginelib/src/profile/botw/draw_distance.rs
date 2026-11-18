// Draw distance in Breath of the Wild: how far from the camera placed actors are created (trees and bushes ~150 m,
// rocks and buildings 300-3000 m, enemies ~150 m).
//
// Each placed actor's distance is its ActorInfo traverseDist (100 when unset) times a scale: the float at +0x310 of the
// actor debug manager, or 1 when that object doesn't exist (the released game never makes it: its "ActorDebug" heap
// is missing). The function (1.9.0 main+0x10a4640) ends `fcsel s0, s0, s2, gt` (the distance, or 100), `fmul s0, s0,
// s1` (times the scale) and returns. The multiply becomes a call that also applies the setting, so every placed
// actor's distance follows it, and the game's own value at 100%. At most 150%: further out the game silently leaves actors
// out (around 1.75x) or fails while it loads the world (2.25x-2.5x and up), and the budget it runs out of is unknown
// (Savage Wind's play tests); 150% kept every actor there. Found by Savage Wind (MIT; its traverse_scale).

use std::sync::atomic::{AtomicU32, Ordering};

/// The setting in percent (0: not read yet).
static PERCENT: AtomicU32 = AtomicU32::new(0);

/// The draw distance setting in percent of the game's own (100).
pub fn percent() -> u32 {
    match PERCENT.load(Ordering::Relaxed) {
        0 => crate::settings::get().graphics.draw_distance.clamp(50, 150),
        p => p,
    }
}

/// Sets the draw distance in percent; actors placed from then on use it.
pub fn set_percent(p: u32) {
    PERCENT.store(p.clamp(50, 150), Ordering::Relaxed);
}

/// A placed actor's traverse distance: the game's `distance x scale`, times the setting.
pub unsafe extern "C" fn traverse_distance(distance: f32, scale: f32) -> f32 {
    let d = distance * scale;
    match percent() {
        100 => d,
        p => d * (p as f32 / 100.0),
    }
}

/// The function's last words, from its fcsel: fcsel s0, s0, s2, gt; fmul s0, s0, s1; ldp x29, x30, [sp], #0x10.
const TAIL: [u32; 3] = [0x1e22_cc00, 0x1e21_0800, 0xa8c1_7bfd];

/// Replaces the multiply at main+`at` (the fcsel before it) with `bl <traverse_distance>`.
#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, at: usize) {
    let code = vec![TAIL[0], 0x9400_0000, TAIL[2]]; // bl, its offset filled in
    match crate::profile::totk::frame_time::replace_with_call(base, mem_space, at, &TAIL, code, 1, crate::hle!(traverse_distance(_, _))) {
        Ok(()) => println!("[+] Draw distance: placed actors appear at {}% of the game's distance", percent()),
        Err(e) => println!("[!] Draw distance: main+{at:#x}: {e}; placed actors keep the game's distance"),
    }
}
