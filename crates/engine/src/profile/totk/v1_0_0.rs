// Profile and hooks for The Legend of Zelda: Tears of the Kingdom v1.0.0.
// NSO Build ID: 082ce09b06e33a123cb1e2770f5f9147
//
// Frame pacing: each frame, main+0x8f6bec picks the present interval from the frame's measured duration (the pacing
// state at frame object + 0x248: lowest and highest interval, bias, frames before raising, cooldown, current interval,
// the interval it may drop to, a list of listeners that can veto a drop). Stock, the lowest interval is 2 and a
// listener vetoes drops to it from 3.
//
// Game time: main+0x7a83e4 (timer, interval), called each frame with the interval just picked, sets the timer's step
// to interval / 2 (+8: 1.0 for a frame at 30 fps) and interval / 60 (+0xc: seconds). Stepping by the interval keeps the
// game at real speed only while it presents exactly at the interval; without vsync, or when a frame misses its vblank,
// it runs fast or slow. The step comes from the real time since the previous frame instead, in every fps mode. The controllers'
// hold timers take their own step from the interval (main+0x8f68b8, through the pointer at main+0x45666a0); it follows
// the same real-time step.
//
// The hooks rewrite instructions and branch to HLE trampolines, so they apply to every runtime: natively, and to the
// image that is lifted (a lifted game takes the fps setting it was lifted with).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::profile::VersionProfile;

pub static PROFILE: VersionProfile = VersionProfile {
    version_string: "1.0.0",
    build_id_prefix: "082ce09b06e33a123cb1e2770f5f9147",
    #[cfg(nx_runtime)]
    install_hooks: Some(install_v1_0_0_hooks),
    #[cfg(not(nx_runtime))]
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

/// main+0xe9a018 builds the pacing limits {lowest 2, highest 3} (movz x8, #2; movk x8, #3, lsl #32).
const SITE_LOWEST: usize = 0xe9a018;
const MOVZ_X8_2: u32 = 0xd280_0048;
const MOVZ_X8_1: u32 = 0xd280_0028;
/// main+0x8f6d74: after asking the listeners, a veto keeps the current interval (cbnz w8, main+0x8f6c94).
const SITE_VETO: usize = 0x8f6d74;
const CBNZ_VETO: u32 = 0x35ff_f908;
const NOP: u32 = 0xd503_201f;
/// main+0x7a83f4..0x7a8418: the step computed from the interval in w1 and stored at [x0 + 8] (the adrp at +0x10 stays:
/// the code after the store uses it).
const SITE_STEP: usize = 0x7a83f4;
const STEP_CODE: [u32; 9] = [
    0x1e22_0020, // scvtf s0, w1
    0x52a8_4e08, // mov w8, #0x42700000 (60.0)
    0x1e27_0101, // fmov s1, w8
    0x1e2c_1002, // fmov s2, #0.5
    0xb001_eda8, // adrp x8, main+0x455d000
    0xaa00_03f3, // mov x19, x0
    0x1e21_1801, // fdiv s1, s0, s1
    0x1e22_0800, // fmul s0, s0, s2
    0x2d01_0400, // stp s0, s1, [x0, #8]
];

/// main+0x8f68b8..0x8f68d8: the controllers' hold-timer step, (float)interval * 0.5 stored through the pointer at
/// main+0x45666a0 (the adrp / ldr at +8 and +0xc stay: their pages are relative to their own addresses).
const SITE_INPUT_STEP: usize = 0x8f68b8;
const INPUT_STEP_CODE: [u32; 8] = [
    0xbd42_5e60, // ldr s0, [x19, #0x25c]
    0x1e2c_1001, // fmov s1, #0.5
    0x9001_e388, // adrp x8, main+0x4566000
    0xf943_5108, // ldr x8, [x8, #0x6a0]
    0x910a_4260, // add x0, x19, #0x290
    0x5e21_d800, // scvtf s0, s0
    0x1e21_0800, // fmul s0, s0, s1
    0xbd00_0100, // str s0, [x8]
];

/// The shortest and longest frame a step accounts for: no slower than 1/240 s of game time a frame, and no more than two
/// frames at 30 fps (a stall, a loading screen, a debugger) in one step.
const MIN_STEP_S: f32 = 1.0 / 240.0;
const MAX_STEP_S: f32 = 2.0 / 30.0;
/// A second call within this time (the setter has two callers) reuses the step: the frame has not ended.
const SAME_FRAME_NS: u64 = 1_000_000;

static LAST_NS: AtomicU64 = AtomicU64::new(0);
static LAST_STEP: AtomicU32 = AtomicU32::new(0);

/// Replaces the step main+0x7a83e4 computes from the interval: the real time since the previous frame, clamped.
pub unsafe extern "C" fn time_step(timer: usize, interval: i32) {
    if timer == 0 {
        return;
    }
    let now = crate::clock::now_ns();
    let last = LAST_NS.load(Ordering::Relaxed);
    let reuse = f32::from_bits(LAST_STEP.load(Ordering::Relaxed));
    let dt = if last != 0 && now.saturating_sub(last) < SAME_FRAME_NS && reuse > 0.0 {
        reuse
    } else {
        LAST_NS.store(now, Ordering::Relaxed);
        let dt = if last == 0 {
            interval.max(1) as f32 / 60.0
        } else {
            ((now - last) as f64 / 1e9) as f32
        }
        .clamp(MIN_STEP_S, MAX_STEP_S);
        LAST_STEP.store(dt.to_bits(), Ordering::Relaxed);
        dt
    };
    *((timer + 8) as *mut f32) = dt * 30.0;
    *((timer + 0xc) as *mut f32) = dt;
}

/// Replaces the controllers' hold-timer step (frames at 30 fps): the last step time_step took.
pub unsafe extern "C" fn input_step(dest: usize) {
    if dest == 0 {
        return;
    }
    let dt = f32::from_bits(LAST_STEP.load(Ordering::Relaxed));
    *(dest as *mut f32) = if dt > 0.0 { dt * 30.0 } else { 0.5 };
}

#[cfg(nx_runtime)]
unsafe fn patch(base: usize, site: usize, from: u32, to: u32, what: &str) -> bool {
    let p = (base + site) as *mut u32;
    match std::ptr::read(p) {
        w if w == from => {
            std::ptr::write(p, to);
            crate::os::flush_icache(p as usize, 4);
            true
        }
        w if w == to => true,
        w => {
            println!("[!] Frame pacer: unexpected instruction {w:#010x} at main+{site:#x} ({what}); not patched");
            false
        }
    }
}

/// Replaces `expected` at main+`at` with `code`, whose word `call` becomes a bl to a trampoline for `hle_fn`.
#[cfg(nx_runtime)]
unsafe fn replace_with_call(
    base: usize,
    mem_space: &mut crate::guest::loader::GuestMemorySpace,
    at: usize,
    expected: &[u32],
    mut code: Vec<u32>,
    call: usize,
    hle_fn: *const (),
) -> Result<(), &'static str> {
    let site = (base + at) as *mut u32;
    if (0..expected.len()).any(|k| std::ptr::read(site.add(k)) != expected[k]) {
        return Err("unexpected code");
    }
    let tramp = mem_space.make_hle_trampoline(hle_fn) as usize;
    let d = tramp as i64 - (site as usize + call * 4) as i64;
    if d.abs() >= 1 << 27 {
        return Err("the trampoline is out of branch range");
    }
    code[call] = 0x9400_0000 | ((d >> 2) as u32 & 0x03ff_ffff);
    for (k, w) in code.iter().enumerate() {
        std::ptr::write(site.add(k), *w);
    }
    crate::os::flush_icache(site as usize, code.len() * 4);
    Ok(())
}

/// Branches main+0x7a83e4's step computation to time_step (x0 = the timer, w1 = the interval, as it receives them),
/// and the controllers' hold-timer step to input_step (x0 = where it goes).
#[cfg(nx_runtime)]
unsafe fn install_time_step(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) -> Result<(), String> {
    let step = vec![
        0xaa00_03f3, // mov x19, x0 (the timer, kept across the call)
        0,           // bl <time_step trampoline>
        NOP,
        NOP,
        STEP_CODE[4], // adrp x8, main+0x455d000 (in place)
        NOP,
        NOP,
        NOP,
        NOP,
    ];
    replace_with_call(base, mem_space, SITE_STEP, &STEP_CODE, step, 1, crate::hle!(time_step(_, _)))
        .map_err(|e| format!("main+{SITE_STEP:#x}: {e}"))?;
    let input = vec![
        NOP,
        NOP,
        INPUT_STEP_CODE[2], // adrp x8, main+0x4566000 (in place)
        INPUT_STEP_CODE[3], // ldr x8, [x8, #0x6a0]
        0xaa08_03e0,        // mov x0, x8
        0,                  // bl <input_step trampoline>
        INPUT_STEP_CODE[4], // add x0, x19, #0x290 (the next call's argument)
        NOP,
    ];
    replace_with_call(base, mem_space, SITE_INPUT_STEP, &INPUT_STEP_CODE, input, 5, crate::hle!(input_step(_)))
        .map_err(|e| format!("main+{SITE_INPUT_STEP:#x}: {e}"))
}

#[cfg(nx_runtime)]
pub unsafe fn install_v1_0_0_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    // In every mode: at 30 fps the real-time step is the stock one, and a lifted game calls the trampolines it was
    // lifted with whatever fps setting it runs with.
    match install_time_step(base, mem_space) {
        Ok(()) => println!("[+] Game time: each frame steps by the real time since the last one (1/240 to 1/15 s)"),
        Err(e) => println!("[!] Game time: {e}; the step stays tied to the present interval"),
    }
    if crate::settings::get().system.fps == crate::settings::FpsMode::Stock30 {
        println!("[+] Frame pacer: stock 30 fps cap active (lowest present interval 2)");
        return;
    }
    // Both or neither: the lowest interval alone stays vetoed.
    let p = (base + SITE_VETO) as *const u32;
    if std::ptr::read(p) != CBNZ_VETO && std::ptr::read(p) != NOP {
        println!("[!] Frame pacer: unexpected instruction {:#010x} at main+{SITE_VETO:#x}; fps unlock not applied", std::ptr::read(p));
        return;
    }
    if !(patch(base, SITE_LOWEST, MOVZ_X8_2, MOVZ_X8_1, "lowest interval") && patch(base, SITE_VETO, CBNZ_VETO, NOP, "veto")) {
        return;
    }
    println!("[+] Frame pacer: lowest present interval 1, drops not vetoed (up to 60 fps with vsync)");
}
