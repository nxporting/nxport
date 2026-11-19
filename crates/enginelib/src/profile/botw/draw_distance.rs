// Draw distance in Breath of the Wild: how far from the camera placed actors are created (trees and bushes ~150 m,
// rocks and buildings 300-3000 m, enemies ~150 m).
//
// Each placed actor's distance is its ActorInfo traverseDist (100 when unset) times a scale: the float at +0x310 (1.0.0: +0x308) of the
// actor debug manager, or 1 when that object doesn't exist (the released game never makes it: its "ActorDebug" heap
// is missing). The function (1.9.0 main+0x10a4640) ends `fcsel s0, s0, s2, gt` (the distance, or 100), `fmul s0, s0,
// s1` (times the scale) and returns. 1.0.0 inlines the same into its callers instead, nine copies (Site). Each multiply
// becomes a call that also applies the setting, so every placed actor's distance follows it, and the game's own value
// at 100%. At most 150%: further out the game silently leaves actors
// out (around 1.75x) or fails while it loads the world (2.25x-2.5x and up), and the budget it runs out of is unknown
// (Savage Wind's play tests); 150% kept every actor there. Found by Savage Wind (MIT; its traverse_scale).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The setting in percent (0: not read yet).
static PERCENT: AtomicU32 = AtomicU32::new(0);
/// Some site took the hook.
static INSTALLED: AtomicBool = AtomicBool::new(false);

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

/// A placed actor's traverse distance where the game multiplies straight into another register: its own choice of the
/// distance (when set, above 0) or `unset` (100), then traverse_distance.
pub unsafe extern "C" fn traverse_distance_or(distance: f32, scale: f32, unset: f32) -> f32 {
    traverse_distance(if distance > 0.0 { distance } else { unset }, scale)
}

/// Where a version computes the scaled distance (the game inlines it: 1.0.0 has nine copies). Each is the game's
/// `fcmp s0, #0.0; fcsel s0, s0, s2, gt` (the distance or 100, in s2) and `fmul <d>, s0, s1` (times the scale in s1).
#[derive(Clone, Copy)]
pub enum Site {
    /// main+at: the fcsel, then `fmul s0, s0, s1` in a function whose return address is on the stack: the multiply
    /// becomes `bl <traverse_distance>`.
    Call(usize),
    /// main+at: the fcsel, then `fmul s0, s0, s1; ret` in a leaf (the return address still in x30): `b
    /// <traverse_distance>`, which returns to the caller.
    Tail(usize),
    /// main+at: the fcmp, then the fcsel and `fmul s8, s0, s1`: `bl <traverse_distance_or>; nop; fmov s8, s0`.
    IntoS8(usize),
}

const FCMP: u32 = 0x1e20_2008; // fcmp s0, #0.0
const FCSEL: u32 = 0x1e22_cc00; // fcsel s0, s0, s2, gt
const FMUL: u32 = 0x1e21_0800; // fmul s0, s0, s1
const FMUL_S8: u32 = 0x1e21_0808; // fmul s8, s0, s1
const RET: u32 = 0xd65f_03c0;
const BL: u32 = 0x9400_0000;
const B: u32 = 0x1400_0000;
const NOP: u32 = 0xd503_201f;
const FMOV_S8: u32 = 0x1e20_4008; // fmov s8, s0

/// 1.9.0: one function (main+0x10a4640), its frame restored after the multiply.
pub const V1_9_0: &[Site] = &[Site::Call(0x10a4684)];
/// 1.0.0: the copies inlined into its callers (each reads the actor debug manager's scale at +0x308 through the GOT
/// entry main+0x226f038; found by that load and checked one by one: the calls' functions keep x30 on the stack and
/// no caller-saved register but s0 is live across them).
pub const V1_0_0: &[Site] = &[
    Site::Tail(0xbd8e4c),
    Site::Tail(0xbd8ea8),
    Site::Tail(0xbd8f3c),
    Site::Tail(0xbd90d8),
    Site::Call(0xbd96dc),
    Site::Call(0xbda4c8),
    Site::Call(0xbda728),
    Site::Call(0xbdaae4),
    Site::IntoS8(0xbdc2cc),
];

/// Makes every site's distance follow the setting.
#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, sites: &[Site]) {
    let mut failed = 0;
    for &site in sites {
        let (at, r) = match site {
            Site::Call(at) => (at, crate::profile::totk::frame_time::replace_with_call(base, mem_space, at, &[FCSEL, FMUL], vec![FCSEL, BL], 1, crate::hle!(traverse_distance(_, _)))),
            Site::Tail(at) => (at, crate::profile::totk::frame_time::replace_with_call(base, mem_space, at, &[FCSEL, FMUL, RET], vec![FCSEL, B, RET], 1, crate::hle!(traverse_distance(_, _)))),
            Site::IntoS8(at) => (at, crate::profile::totk::frame_time::replace_with_call(base, mem_space, at, &[FCMP, FCSEL, FMUL_S8], vec![BL, NOP, FMOV_S8], 0, crate::hle!(traverse_distance_or(_, _, _)))),
        };
        if let Err(e) = r {
            println!("[!] Draw distance: main+{at:#x}: {e}; placed actors there keep the game's distance");
            failed += 1;
        }
    }
    if failed < sites.len() {
        INSTALLED.store(true, Ordering::Relaxed);
        println!("[+] Draw distance: placed actors appear at {}% of the game's distance", percent());
    }
}

/// Whether this version's draw distance can be changed.
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}
