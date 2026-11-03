// Frame pacing and game time in Tears of the Kingdom (1.0.0 and 1.1.2: the same code at other addresses, `Sites`).
//
// Frame pacing: each frame the pacer picks the present interval from the frame's measured duration (the pacing state at
// frame object + 0x248: lowest and highest interval, bias, frames before raising, cooldown, current interval, the
// interval it may drop to, a list of listeners that can veto a drop). Stock, the lowest interval is 2 and a listener
// vetoes drops to it from 3. Both are lifted here, always: how fast the game may run is decided while it runs, by the
// present interval the engine gives the game (nvn::api::present_interval; the 30 fps setting keeps it at 2 or more).
//
// Game time: the time manager's per-frame update, called with the interval just picked, sets the timer's step to
// interval / 2 (+8: 1.0 for a frame at 30 fps) and interval / 60 (+0xc: seconds). Stepping by the interval keeps the game
// at real speed only while it presents exactly at the interval; without vsync, or when a frame misses its vblank, it runs
// fast or slow. The step comes from the real time since the previous frame instead. The controllers' hold timers take
// their own step from the interval (through a pointer the code loads); it follows the same real-time step.
//
// With vsync pacing frames (vblank_period), frames reach the screen a whole number of refreshes apart, but the game's
// frames start unevenly between them (the swapchain lets a frame start once an image frees): real-time steps then moved
// things unevenly across evenly shown frames, a stutter. There the step is rounded to whole refreshes, the remainder
// carried into the next frames' steps, so game time keeps to real time.
//
// Camera rate: a getter the camera code reads a per-frame amount through (1.1.2: main+0x1d3675c, 1.0.0: main+0x1cd3678;
// UltraCam scales it the same way in 1.1.2: docs/ultracam-totk-112.md) is not stepped by the time manager, so the camera turned faster the faster
// the game ran (Ultrahand's vertical camera). It branches to camera_rate, which scales the amount by the frame's step
// in frames at 30 fps (1 at 30 fps).
//
// Ground carry: a slow-motion check on the velocity Link keeps from what he stands on compares a value that grows with
// the frame rate (30 / (step * time scale)) with 100; above 100 fps he slid off moving platforms. ground_carry takes out
// the step (the same fix as MacNativeGames' and SkyboundTOTK's for 1.0.0).
//
// Hair and cloth (1.0.0): the actor animation update builds a clock {seconds = factor / 30, ratio = factor} each rendered
// frame from a 1/30 s constant, so above 30 fps hair and cloth swung too fast. Its four places (three inline, one before a
// call) load two words time_step keeps at {step seconds, step frames} instead, and scale both by the factor at once
// (`ldr d`, `fmul v.2s, v.2s, v.s[0]`, `str d`): the same instruction count, no call. 1.1.2 leaves that clock unset.
//
// The hooks rewrite instructions and branch to HLE trampolines (no code of our own in guest memory), so they apply to
// every runtime: natively, and to the image that is lifted.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Where one version's pacing and game-time code is (offsets in main), and the code expected there.
pub struct Sites {
    /// The pacing limits {lowest 2, highest 3}: movz x8, #2 (movk x8, #3, lsl #32 follows).
    pub lowest: usize,
    /// After asking the listeners, a veto keeps the current interval: cbnz w8, <back into the pacer>.
    pub veto: usize,
    /// The step computed from the interval in w1 and stored at [x0 + 8]; word 4 is an adrp the code after it uses.
    pub step: usize,
    pub step_code: [u32; 9],
    /// The hold-timer step stored through a pointer; words 2 and 3 (adrp, ldr) load the pointer.
    pub input: usize,
    pub input_code: [u32; 8],
    /// The camera-rate getter and the field it reads (`ldr s0, [x0, #field]; ret`: camera_rate_code), where known.
    pub camera_rate: Option<(usize, usize)>,
    /// The slow-motion check on the velocity Link keeps from the ground he stands on (ground_carry_code) and the
    /// version's word 1 (`ldr x8, [xN]`: the register differs), where known.
    pub ground_carry: Option<(usize, u32)>,
    /// The hair and cloth clock: two words in main's mapping that time_step keeps at {step seconds, step frames}, and
    /// the instructions that read them in place of the 1/30 s constant (cloth_clock).
    pub cloth: Option<(usize, &'static [ClothSite])>,
}

/// An instruction of the hair and cloth clock: (offset, the code expected, its replacement; ADRP_TO_CLOTH: `adrp x8` to
/// the clock words' page).
pub type ClothSite = (usize, u32, u32);
pub const ADRP_TO_CLOTH: u32 = 0;

/// The camera-rate getter: ldr s0, [x0, #field]; ret.
pub const fn camera_rate_code(field: usize) -> [u32; 2] {
    [0xbd40_0000 | ((field as u32 / 4) << 10), 0xd65f_03c0]
}
const _: () = assert!(camera_rate_code(0xc54)[0] == 0xbd4c_5400);

/// The ground-carry check: a dummy load, the time object ([[xN] + 0xc8]), its +0x28 compared with 100 (b.gt follows).
pub const fn ground_carry_code(ldr_obj: u32) -> [u32; 7] {
    [
        0xf940_36df, // ldr xzr, [x22, #0x68]
        ldr_obj,     // ldr x8, [xN]
        0xf940_6508, // ldr x8, [x8, #0xc8]
        0xbd40_2900, // ldr s0, [x8, #0x28]
        0x52a8_5908, // mov w8, #0x42c80000 (100.0)
        0x1e27_0101, // fmov s1, w8
        0x1e21_2000, // fcmp s0, s1
    ]
}

pub const MOVZ_X8_2: u32 = 0xd280_0048;
pub const MOVZ_X8_1: u32 = 0xd280_0028;
pub const CBNZ_VETO: u32 = 0x35ff_f908;
pub const NOP: u32 = 0xd503_201f;
/// BL and B, offset 0.
const BL: u32 = 0x9400_0000;
const B: u32 = 0x1400_0000;

/// The step code (scvtf s0, w1 .. stp s0, s1, [x0, #8]) with the version's adrp as word 4.
pub const fn step_code(adrp: u32) -> [u32; 9] {
    [
        0x1e22_0020, // scvtf s0, w1
        0x52a8_4e08, // mov w8, #0x42700000 (60.0)
        0x1e27_0101, // fmov s1, w8
        0x1e2c_1002, // fmov s2, #0.5
        adrp,        // adrp x8, <page>
        0xaa00_03f3, // mov x19, x0
        0x1e21_1801, // fdiv s1, s0, s1
        0x1e22_0800, // fmul s0, s0, s2
        0x2d01_0400, // stp s0, s1, [x0, #8]
    ]
}

/// The hold-timer step code with the version's pointer load (adrp, ldr) as words 2 and 3.
pub const fn input_code(adrp: u32, ldr: u32) -> [u32; 8] {
    [
        0xbd42_5e60, // ldr s0, [x19, #0x25c]
        0x1e2c_1001, // fmov s1, #0.5
        adrp,        // adrp x8, <page>
        ldr,         // ldr x8, [x8, #<pointer>]
        0x910a_4260, // add x0, x19, #0x290
        0x5e21_d800, // scvtf s0, s0
        0x1e21_0800, // fmul s0, s0, s1
        0xbd00_0100, // str s0, [x8]
    ]
}

/// The shortest and longest frame a step accounts for: no slower than 1/240 s of game time a frame, and no more than two
/// frames at 30 fps (a stall, a loading screen, a debugger) in one step.
const MIN_STEP_S: f32 = 1.0 / 240.0;
const MAX_STEP_S: f32 = 2.0 / 30.0;
/// A second call within this time (the setter has two callers) reuses the step: the frame has not ended.
const SAME_FRAME_NS: u64 = 1_000_000;

static LAST_NS: AtomicU64 = AtomicU64::new(0);
static LAST_STEP: AtomicU32 = AtomicU32::new(0);
/// The most refreshes a burst of frames may run ahead of real time (swapchain images and frames in flight).
const MAX_AHEAD: f32 = 8.0;
/// Real time not yet stepped (f32 bits, seconds; may be negative): what rounding to whole refreshes left over.
static CARRY: AtomicU32 = AtomicU32::new(0);
/// The hair and cloth clock words (guest address; 0: none): {step seconds, step frames at 30 fps}.
static CLOTH: AtomicU64 = AtomicU64::new(0);

/// Whether the running title has the hooks (installed when it was loaded; a lifted game also had them when lifted).
static INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the game steps by real time now: then it may run at any frame rate without running fast or slow.
pub fn real_time() -> bool {
    INSTALLED.load(Ordering::Relaxed) && crate::host::live::real_time_speed()
}

/// The interval the game picked last (the stock step, with real-time game speed off).
static LAST_INTERVAL: AtomicU32 = AtomicU32::new(2);

/// Replaces the step the time manager computes from the interval: the real time since the previous frame, clamped.
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
        let dt = match vblank_period() {
            Some(period) => {
                let real = dt + f32::from_bits(CARRY.load(Ordering::Relaxed));
                let step = (real / period).round().max(1.0) * period;
                // Frames started in a burst each show a refresh after the last: the time they are ahead is carried
                // into the wait that follows (up to MAX_AHEAD refreshes). Time behind carries up to a refresh, so a
                // stall is not made up over the frames after it.
                CARRY.store((real - step).clamp(-MAX_AHEAD * period, period).to_bits(), Ordering::Relaxed);
                step
            }
            None => {
                CARRY.store(0, Ordering::Relaxed);
                dt
            }
        };
        LAST_STEP.store(dt.to_bits(), Ordering::Relaxed);
        dt
    };
    LAST_INTERVAL.store(interval.max(1) as u32, Ordering::Relaxed);
    // Real-time game speed off (the menu): the stock step, from the interval the game picked.
    let dt = if crate::host::live::real_time_speed() { dt } else { interval.max(1) as f32 / 60.0 };
    *((timer + 8) as *mut f32) = dt * 30.0;
    *((timer + 0xc) as *mut f32) = dt;
    let cloth = CLOTH.load(Ordering::Relaxed) as usize;
    if cloth != 0 {
        let frames = frames_at_30();
        *(cloth as *mut [f32; 2]) = [frames / 30.0, frames];
    }
    // Installing the hook says it is in the code; this says the game calls it (a lifted game calls what it was lifted with).
    static CALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !CALLED.swap(true, Ordering::Relaxed) {
        println!("[+] Game time: the game steps by real time ({:.1} ms this frame)", dt * 1000.0);
    }
}

/// With vsync, the display's refresh period when the swapchain paces frames: the frame-rate limit is at or above the
/// refresh rate (nvn::api's limiter then leaves pacing to the swapchain). Known on hosts that report the refresh rate.
fn vblank_period() -> Option<f32> {
    use crate::host::{live, overlay};
    let hz = live::refresh_hz();
    let paced = overlay::is_vsync() && overlay::fps_mode() != crate::settings::FpsMode::Stock30 && hz >= 30.0;
    (paced && live::frame_limit() + 1 >= live::refresh_rate()).then(|| 1.0 / hz)
}

/// The last frame's step in frames at 30 fps: what time_step gave the game.
fn frames_at_30() -> f32 {
    if !crate::host::live::real_time_speed() {
        return LAST_INTERVAL.load(Ordering::Relaxed) as f32 * 0.5;
    }
    let dt = f32::from_bits(LAST_STEP.load(Ordering::Relaxed));
    if dt > 0.0 { dt * 30.0 } else { 1.0 }
}

/// The field the camera-rate getter reads (the version's).
static CAMERA_FIELD: AtomicU64 = AtomicU64::new(0);

/// Replaces the camera-rate getter (x0 = the camera's object): its per-frame amount, scaled to the frame's step.
pub unsafe extern "C" fn camera_rate(obj: usize) -> f32 {
    *((obj + CAMERA_FIELD.load(Ordering::Relaxed) as usize) as *const f32) * frames_at_30()
}

/// The ground-carry check's value (x0 = the time object): Link keeps a tenth of the velocity of what he stands on when
/// the object's +0x28, 30 / (step * time scale), is over 100: meant for time slowed below 0.3, but above 100 fps the
/// step alone got it there and he slid off moving platforms and trampolines. Times the step (+0x38) it is 30 / time
/// scale: the same at 30 fps, and independent of the frame rate.
pub unsafe extern "C" fn ground_carry(obj: usize) -> f32 {
    *((obj + 0x28) as *const f32) * *((obj + 0x38) as *const f32)
}

/// Replaces the controllers' hold-timer step (frames at 30 fps): the last step time_step took.
pub unsafe extern "C" fn input_step(dest: usize) {
    if dest == 0 {
        return;
    }
    *(dest as *mut f32) = frames_at_30();
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

/// Replaces `expected` at main+`at` with `code`, whose word `call` (BL or B) gets the offset of a trampoline for `hle_fn`.
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
    code[call] |= (d >> 2) as u32 & 0x03ff_ffff;
    for (k, w) in code.iter().enumerate() {
        std::ptr::write(site.add(k), *w);
    }
    crate::os::flush_icache(site as usize, code.len() * 4);
    Ok(())
}

/// Branches the step computation to time_step (x0 = the timer, w1 = the interval, as it receives them), and the
/// controllers' hold-timer step to input_step (x0 = where it goes).
#[cfg(nx_runtime)]
unsafe fn install_time_step(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, s: &Sites) -> Result<(), String> {
    let step = vec![
        0xaa00_03f3, // mov x19, x0 (the timer, kept across the call)
        BL,          // bl <time_step trampoline>
        NOP,
        NOP,
        s.step_code[4], // adrp x8, <page> (in place)
        NOP,
        NOP,
        NOP,
        NOP,
    ];
    replace_with_call(base, mem_space, s.step, &s.step_code, step, 1, crate::hle!(time_step(_, _)))
        .map_err(|e| format!("main+{:#x}: {e}", s.step))?;
    let input = vec![
        NOP,
        NOP,
        s.input_code[2], // adrp x8, <page> (in place)
        s.input_code[3], // ldr x8, [x8, #<pointer>]
        0xaa08_03e0,     // mov x0, x8
        BL,              // bl <input_step trampoline>
        s.input_code[4], // add x0, x19, #0x290 (the next call's argument)
        NOP,
    ];
    replace_with_call(base, mem_space, s.input, &s.input_code, input, 5, crate::hle!(input_step(_)))
        .map_err(|e| format!("main+{:#x}: {e}", s.input))
}

/// Game time from real time, and the pacer free to run at any interval (the engine caps it while the game runs).
#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, s: &Sites) {
    match install_time_step(base, mem_space, s) {
        Ok(()) => {
            INSTALLED.store(true, Ordering::Relaxed);
            println!("[+] Game time: each frame steps by the real time since the last one (1/240 to 1/15 s)")
        }
        Err(e) => println!("[!] Game time: {e}; the step stays tied to the present interval"),
    }
    if let Some((at, field)) = s.camera_rate {
        CAMERA_FIELD.store(field as u64, Ordering::Relaxed);
        // b <camera_rate trampoline>: it returns to the getter's caller.
        match replace_with_call(base, mem_space, at, &camera_rate_code(field), vec![B, NOP], 0, crate::hle!(camera_rate(_))) {
            Ok(()) => println!("[+] Camera rate: scaled to the frame's step"),
            Err(e) => println!("[!] Camera rate: main+{at:#x}: {e}; not scaled"),
        }
    }
    if let Some((data, sites)) = s.cloth {
        // adrp x8, <the clock words' page> at `at` (both offsets in main: the same at lift and run time).
        let adrp = |at: usize| {
            let d = ((data & !0xfff) as i64 - (at & !0xfff) as i64) >> 12;
            0x9000_0008 | (d as u32 & 3) << 29 | ((d >> 2) as u32 & 0x7_ffff) << 5
        };
        match sites.iter().find(|(at, from, to)| {
            let w = std::ptr::read((base + at) as *const u32);
            w != *from && w != if *to == ADRP_TO_CLOTH { adrp(*at) } else { *to }
        }) {
            Some((at, from, _)) => println!(
                "[!] Hair and cloth: main+{at:#x} is {:#010x}, not {from:#010x}; their clock stays at 1/30 s a frame",
                std::ptr::read((base + at) as *const u32)
            ),
            None => {
                *((base + data) as *mut [f32; 2]) = [1.0 / 30.0, 1.0];
                CLOTH.store((base + data) as u64, Ordering::Relaxed);
                for (at, _, to) in sites {
                    std::ptr::write((base + at) as *mut u32, if *to == ADRP_TO_CLOTH { adrp(*at) } else { *to });
                    crate::os::flush_icache(base + at, 4);
                }
                println!("[+] Hair and cloth: their clock follows the frame's step");
            }
        }
    }
    if let Some((at, ldr_obj)) = s.ground_carry {
        let carry = vec![
            ldr_obj,     // ldr x8, [xN]
            0xf940_6500, // ldr x0, [x8, #0xc8]
            BL,          // bl <ground_carry trampoline> (s0)
            0x52a8_5908, // mov w8, #0x42c80000 (100.0)
            0x1e27_0101, // fmov s1, w8
            0x1e21_2000, // fcmp s0, s1
            NOP,
        ];
        match replace_with_call(base, mem_space, at, &ground_carry_code(ldr_obj), carry, 2, crate::hle!(ground_carry(_))) {
            Ok(()) => println!("[+] Ground carry: Link keeps moving platforms' velocity at any frame rate"),
            Err(e) => println!("[!] Ground carry: main+{at:#x}: {e}; not patched"),
        }
    }
    // Both or neither: the lowest interval alone stays vetoed.
    let p = (base + s.veto) as *const u32;
    if std::ptr::read(p) != CBNZ_VETO && std::ptr::read(p) != NOP {
        println!("[!] Frame pacer: unexpected instruction {:#010x} at main+{:#x}; not unlocked", std::ptr::read(p), s.veto);
        return;
    }
    if patch(base, s.lowest, MOVZ_X8_2, MOVZ_X8_1, "lowest interval") && patch(base, s.veto, CBNZ_VETO, NOP, "veto") {
        println!("[+] Frame pacer: lowest present interval 1, drops not vetoed (the frame-rate setting caps it)");
    }
}
