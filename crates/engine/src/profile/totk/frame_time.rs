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
// Steps the game takes without its frame ratio, which only the 30 fps it ships at hid (found by totk-pc-recomp): the
// rotation check of gears and water wheels (rotate_check), the map cursor's pan (map_pan) and the gyro's vertical aim
// (gyro_pitch) take the ratio the code around them already has.
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
    /// The screen behind the ability wheel and the quick menus (open_short_cut_bg): where the UI manager's pointer is, the
    /// function that finds a screen by its hash, the hash.
    pub short_cut_bg: Option<(usize, usize, u64)>,
    /// The equipment-switch cooldown's step test (equip_cooldown), where known: its `fmov s1, w8` (FLT_EPSILON), and the
    /// `ldr s0, [x19, #0x109c]` that loads the cooldown before it (1.0.0 has an add and the constant between them).
    pub equip_cooldown: Option<(usize, usize)>,
    /// The rotation check of gears and water wheels (rotate_check), where known: the update's `ldr x1, [x0, #0x40]` (the
    /// first of rotate_entry_code), the function that normalises a vector it calls three words on, and the threshold's
    /// `fdiv s12, s0, s4`.
    pub rotate_check: Option<(usize, usize, usize)>,
    /// The map cursor's pan step (map_pan), where known: its 16 words from `ldr s3, [x19, #0x2cc]` (map_pan_code).
    pub map_pan: Option<(usize, [u32; 16])>,
    /// The gyro's vertical aim (gyro_pitch), where known: the four loads after the call at the aim update's join, the
    /// version's `ldr s9, [x22, #<gyro pitch>]` two words before them, and where the update loads the frame ratio
    /// (`ldr s10, [x20, #8]`).
    pub gyro_pitch: Option<(usize, u32, usize)>,
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
    open_short_cut_bg();
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

/// main's base and Sites::short_cut_bg.
static SHORT_CUT_BG: std::sync::OnceLock<(usize, usize, usize, u64)> = std::sync::OnceLock::new();

/// The ability wheel captures the world for its background when its opening animation has reached frame 1, and the world
/// stops being drawn 4 rendered frames after it starts opening, however long a frame is. The capture lands 3 rendered
/// frames after the animation's frame reaches 1, so with steps under a whole game frame it came after the world stopped
/// and the background was black (at a held step: 2.0 and 1.0 right, 0.99, 0.5 and 0.25 black). So the opening animation
/// starts at frame 1 of its 6.
unsafe fn open_short_cut_bg() {
    let Some(&(base, manager, find, hash)) = SHORT_CUT_BG.get() else { return };
    let manager = *((base + manager) as *const usize);
    let manager = if manager != 0 { *(manager as *const usize) } else { 0 };
    if manager == 0 {
        return;
    }
    let screen = crate::guest::call(base + find, &[manager as u64, hash]) as usize;
    if screen == 0 || *((screen + 0x16e) as *const u8) != 1 {
        return;
    }
    let layout = *((screen + 0x28) as *const usize);
    if layout == 0 {
        return;
    }
    let animation = match *((layout + 0x60) as *const usize) {
        0 => *((layout + 0x68) as *const usize),
        a => a,
    };
    if animation != 0 {
        let frame = (animation + 0x20) as *mut f32;
        *frame = (*frame).max(1.0);
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
    if let Some((manager, find, hash)) = s.short_cut_bg {
        let _ = SHORT_CUT_BG.set((base, manager, find, hash));
    }
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
    if let Some((at, load)) = s.equip_cooldown {
        equip_cooldown(base, at, load);
    }
    if let Some((at, normalize, fdiv)) = s.rotate_check {
        install_rotate_check(base, mem_space, at, normalize, fdiv);
    }
    if let Some((at, code)) = s.map_pan {
        map_pan(base, at, &code);
    }
    if let Some((at, ldr_pitch, ratio_at)) = s.gyro_pitch {
        gyro_pitch(base, at, ldr_pitch, ratio_at);
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

/// The equipment-switch cooldown (player state +0x109c) is stepped only while it is above FLT_EPSILON, but the equipment
/// gate blocks weapon, shield and bow changes while it is above 0: a real-time step can leave a remainder in
/// (0, FLT_EPSILON] that is never stepped again, and equipment stays locked until a load. Compared with 0 instead, the
/// last step takes it below 0 like any other; whole 30 Hz steps reach exactly 0 either way, so this holds at any rate.
/// Found on 1.2.1 and 1.4.3 by totk-pc-recomp (issue 18) and MacNativeGames, both MIT; located here in 1.0.0 and 1.1.2
/// by the same code: the load, this compare, and a branch that hands main + 0x109c to the timer step.
unsafe fn equip_cooldown(base: usize, at: usize, load_at: usize) {
    const LDR_S0_COOLDOWN: u32 = 0xbd50_9e60; // ldr s0, [x19, #0x109c]
    const FMOV_S1_EPSILON: u32 = 0x1e27_0101; // fmov s1, w8 (w8 = FLT_EPSILON)
    const FMOV_S1_ZERO: u32 = 0x1e27_03e1; // fmov s1, wzr
    let load = std::ptr::read((base + load_at) as *const u32);
    let p = (base + at) as *mut u32;
    match (load, std::ptr::read(p)) {
        (LDR_S0_COOLDOWN, FMOV_S1_EPSILON) => {
            std::ptr::write(p, FMOV_S1_ZERO);
            crate::os::flush_icache(p as usize, 4);
            println!("[+] Equipment: its switch cooldown runs out at any frame rate");
        }
        (LDR_S0_COOLDOWN, FMOV_S1_ZERO) => {}
        (l, w) => println!("[!] Equipment: unexpected code {l:#010x} {w:#010x} at main+{load_at:#x} / main+{at:#x}; not patched"),
    }
}

/// The rotate module's update (the AI's check that a gear or water wheel turns, which shrine doors at Riogok, Turakamik
/// and the Wind Temple wait on) reports turning when the angle turned since its last update is above RotateStartDeg / 30:
/// the angle of one 30 Hz frame. It is called with the AI's update context, whose +8 is the frame delta in 30 Hz frames
/// (the frame-counter node adds it up each update), but ignores it: above 30 fps a gear turned less than that per update,
/// read as stopped, and the doors never opened. The threshold becomes the angle of the update's own delta, one 30 Hz
/// frame's times the delta (1.4.3 divides the angle by the delta instead: the same test).
///
/// The update's first words (rotate_entry_code) load the module's axis, keep the module and normalise the axis into a
/// local. They become: keep the module, call rotate_check (which normalises the axis with the same function and returns
/// 30 / delta), keep that in s12, which the prologue has saved and the update first writes at the threshold. The
/// threshold's `fdiv s12, s0, s4` (s4 = 30) then divides by s12 instead: 30 / 1 at 30 fps, the stock value bit for bit.
/// Found on 1.2.1 and 1.4.3 by totk-pc-recomp (MIT, built on MacNativeGames, MIT).
pub const fn rotate_entry_code(at: usize, normalize: usize) -> [u32; 4] {
    let d = (normalize as i64 - (at as i64 + 12)) >> 2;
    [
        0xf940_2001,                   // ldr x1, [x0, #0x40] (the axis)
        0xaa00_03f3,                   // mov x19, x0
        0xd100_e3a0,                   // sub x0, x29, #0x38
        BL | (d as u32 & 0x03ff_ffff), // bl <normalize>
    ]
}
const FDIV_S12_S0_S4: u32 = 0x1e24_180c;
const FDIV_S12_S0_S12: u32 = 0x1e2c_180c;

/// The function rotate_check normalises the axis with (guest address).
static NORMALIZE: AtomicU64 = AtomicU64::new(0);

/// Called at the start of the rotate module's update (x0 = the module, x1 = the update context, x2 = where the update
/// keeps the normalised axis): normalises the axis as the update did, and returns 30 / the context's frame delta, which
/// the threshold then divides by.
pub unsafe extern "C" fn rotate_check(module: usize, ctx: usize, out: usize) -> f32 {
    crate::guest::call(NORMALIZE.load(Ordering::Relaxed) as usize, &[out as u64, *((module + 0x40) as *const u64)]);
    if ctx == 0 {
        return 30.0;
    }
    30.0 / *((ctx + 8) as *const f32)
}

#[cfg(nx_runtime)]
unsafe fn install_rotate_check(
    base: usize,
    mem_space: &mut crate::guest::loader::GuestMemorySpace,
    at: usize,
    normalize: usize,
    fdiv: usize,
) {
    let w = std::ptr::read((base + fdiv) as *const u32);
    if w != FDIV_S12_S0_S4 {
        return println!("[!] Gears: unexpected instruction {w:#010x} at main+{fdiv:#x}; their rotation check stays at 30 fps");
    }
    NORMALIZE.store((base + normalize) as u64, Ordering::Relaxed);
    let code = vec![
        0xaa00_03f3, // mov x19, x0
        0xd100_e3a2, // sub x2, x29, #0x38
        BL,          // bl <rotate_check trampoline> (s0 = 30 / delta)
        0x1e20_400c, // fmov s12, s0
    ];
    let hle = crate::hle!(rotate_check(_, _, _));
    match replace_with_call(base, mem_space, at, &rotate_entry_code(at, normalize), code, 2, hle) {
        Ok(()) => {
            std::ptr::write((base + fdiv) as *mut u32, FDIV_S12_S0_S12);
            crate::os::flush_icache(base + fdiv, 4);
            println!("[+] Gears: their rotation check counts the update's frame delta (shrine doors open at any frame rate)");
        }
        Err(e) => println!("[!] Gears: main+{at:#x}: {e}; their rotation check stays at 30 fps"),
    }
}

/// Rewrites the words at main+`at` from `from` to `to` in place (the same count); true if they are `to` now. On other
/// code, says where it differs and writes nothing.
unsafe fn rewrite(base: usize, at: usize, from: &[u32], to: &[u32], what: &str) -> bool {
    let p = (base + at) as *mut u32;
    let now: Vec<u32> = (0..from.len()).map(|k| std::ptr::read(p.add(k))).collect();
    if now == to {
        return true;
    }
    if let Some(k) = (0..from.len()).find(|&k| now[k] != from[k]) {
        println!("[!] {what}: unexpected instruction {:#010x} at main+{:#x}; not patched", now[k], at + 4 * k);
        return false;
    }
    for (k, w) in to.iter().enumerate() {
        std::ptr::write(p.add(k), *w);
    }
    crate::os::flush_icache(p as usize, 4 * to.len());
    true
}

/// The map cursor's pan step, with the version's stick loads (`ldr s1` / `ldr s2` of x and y), the old position's
/// `fmov s0, wN`, the bound load (`ldr s4, [x8, #..]`) and the `adrp x8` that reloads the page x8 already holds.
pub const fn map_pan_code(stick_x: u32, stick_y: u32, fmov_old_x: u32, ldr_bound: u32, adrp: u32) -> [u32; 16] {
    [
        0xbd42_ce63, // ldr s3, [x19, #0x2cc] (the position's y)
        0x1e21_3922, // fsub s2, s9, s1
        0x1e22_0800, // fmul s0, s0, s2
        0xbd42_ca62, // ldr s2, [x19, #0x2c8] (its x)
        0x1e20_2820, // fadd s0, s1, s0 (the pan speed, eased towards its target)
        0xbd02_e260, // str s0, [x19, #0x2e0]
        stick_x,     // ldr s1, <stick x>
        0x1e20_0821, // fmul s1, s1, s0
        0x1e21_2841, // fadd s1, s2, s1
        0xbd02_ca61, // str s1, [x19, #0x2c8] (stored again, clamped, before anything reads it)
        stick_y,     // ldr s2, <stick y>
        0x1e22_0802, // fmul s2, s0, s2
        fmov_old_x,  // fmov s0, wN
        0x1e24_3800, // fsub s0, s0, s4
        ldr_bound,   // ldr s4, [x8, #..]
        adrp,        // adrp x8, <the page x8 holds>
    ]
}

/// The map screen's update eases the cursor's pan speed towards its target with the frame's 30 Hz ratio (0.75 to the
/// power of the ratio: the same speed after the same time at any frame rate) but steps the position by speed * stick
/// each update, so the cursor panned 2x as fast at 60 fps and 4x at 120. The step is speed * ratio * stick: the ratio
/// the easing reads ([[x23] + 8]), the same at 30 fps (1.0) bit for bit. Room for the two loads and the multiply comes
/// from the code itself: the position's x and y load as one pair through x21 (the update set it to x19 + 0x2c8 on the
/// one way here), the first store of the unclamped x goes (the clamped x is stored over it before anything reads it),
/// and so does an adrp of the page x8 already holds. x9 and s5 are free: the code after the window writes them before
/// any read (the easing's call clobbers them before it). Found on 1.2.1 by totk-pc-recomp (MIT, built on MacNativeGames,
/// MIT).
unsafe fn map_pan(base: usize, at: usize, code: &[u32; 16]) {
    let c = code;
    let to = [
        0xf940_02e9, // ldr x9, [x23] (the frame object)
        c[1],
        c[2],
        0x2d40_0ea2, // ldp s2, s3, [x21] (x, y)
        c[4],
        c[5],
        0xbd40_0925, // ldr s5, [x9, #8] (the 30 Hz ratio)
        0x1e25_0800, // fmul s0, s0, s5 (this update's step)
        c[6],
        c[7],
        c[8],
        c[10],
        c[11],
        c[12],
        c[13],
        c[14],
    ];
    if rewrite(base, at, code, &to, "Map cursor") {
        println!("[+] Map cursor: it pans at the same speed at any frame rate");
    }
}

/// The aim update for Ultrahand and Recall turns the gyro's vertical movement ([x22 + <gyro pitch>], loaded into s9 at
/// its join) into aim without the frame's 30 Hz ratio, which the update loaded into s10 ([x20 + 8]) and multiplies its
/// stick aim by: above 30 fps the vertical gyro aim moved 2x as fast at 60 fps and 4x at 120. s9 is read twice after: the
/// Recall aim's vertical step and the Ultrahand gyro aim's, whose step totk-pc-recomp scales after it is divided by the
/// aim's range (where nxport's camera_rate already scales the stick's: scaling there would count it twice). Scaling s9
/// by s10 at the join scales both by the ratio once. s10 holds [x20 + 8] there on every way in (nothing writes it before
/// later in the update). Room for the multiply: the four loads after the call become an add and two pairs (offsets past
/// what a pair can reach, from x8, which the call has clobbered and the code writes again before reading). Found on 1.2.1
/// by totk-pc-recomp (MIT, built on MacNativeGames, MIT).
unsafe fn gyro_pitch(base: usize, at: usize, ldr_pitch: u32, ratio_at: usize) {
    const LDR_S10_RATIO: u32 = 0xbd40_0a8a; // ldr s10, [x20, #8]
    let from = [
        0xbd41_ae68, // ldr s8, [x19, #0x1ac]
        0xbd41_b26b, // ldr s11, [x19, #0x1b0]
        0xbd41_be6d, // ldr s13, [x19, #0x1bc]
        0xbd41_c26e, // ldr s14, [x19, #0x1c0]
    ];
    let to = [
        0x9106_b268, // add x8, x19, #0x1ac
        0x2d40_2d08, // ldp s8, s11, [x8]
        0x2d42_390d, // ldp s13, s14, [x8, #0x10]
        0x1e2a_0929, // fmul s9, s9, s10
    ];
    let (pitch, ratio) = (std::ptr::read((base + at - 8) as *const u32), std::ptr::read((base + ratio_at) as *const u32));
    if pitch != ldr_pitch || ratio != LDR_S10_RATIO {
        return println!(
            "[!] Gyro aim: unexpected code {pitch:#010x} {ratio:#010x} at main+{:#x} / main+{ratio_at:#x}; not patched",
            at - 8
        );
    }
    if rewrite(base, at, &from, &to, "Gyro aim") {
        println!("[+] Gyro aim: vertical gyro aiming moves at the same speed at any frame rate");
    }
}
