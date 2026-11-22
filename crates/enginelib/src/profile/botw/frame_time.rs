// Game time in Breath of the Wild (1.9.0: the sites were found by Savage Wind, MIT, tools/game_sites.py and
// runtime/hle/framepace.c, and checked here against the code; 1.5.0: the same code found by its shape, names from the
// decomp of that version).
//
// The game's clock is ksys::VFR: each frame VFR::updateInterval sets the interval ratio (+0x3c: interval / 2, 1.0 at
// 30 fps, 1.5 at 20) from the present interval the frame loop picked (+0x34), then hands it to each core's clock (the
// list at +0x80, +0x78 entries), which multiplies it by the slowest time multiplier (slow motion) into the delta frame
// and delta time everything else reads: the time of day, the physics step, animation, AI. Stepping by the interval keeps
// the game at real speed only while it presents exactly at that interval. vfr_update gives the ratio from the real time
// since the previous frame instead (frame_step.rs), before the cores take it; the game then runs at any frame rate. While
// the game overrides the interval (+0x29: single steps while a stage loads) its own ratio stays.
//
// Other steps that count frames:
// - The menus' speed: once a frame the game stores interval x 0.5 as the GUI speed (the float at +0xb20 of a UI
//   object): 1 at 30 fps, so its cursor and animations would run faster above. gui_speed gives the frame's step in frames at
//   30 fps instead (as Cemu's FPS++ does on the Wii U).
// - The controllers' hold counters (sead::ControllerBase::updateDerivativeParams_) go up by a float the game keeps at 1
//   (read through a GOT slot): it is set to the frame's step in frames at 30 fps, so holds last as long as on the
//   console. 1.5.0 counts whole frames in integers: not changed there.
// - The frame loop's vsync wait (it waits for vblanks until interval / 60 s have passed; only the first frames after
//   start-up reach it) returns at once: the engine's present pacer paces.
// - Arrows: once in their fall, an arrow adds its gravity (0, ArrowGravity, 0) to its velocity once per update, with no
//   step (the straight part before it scales by the delta frame): above 30 fps they would fall 2 or 4 times as fast and
//   land short. The gravity is scaled by the frame's step in frames at 30 fps (slow motion left out, as on the console).
//   The code calls a function that always returns 0 just before (the branch it guards is never taken); that call and
//   its dead branch make room for the call that scales it.
// - The sound system's frame: the game hands it the delta frame, at least 0.5 (a 60 fps frame; 1.5.0: 1, a 30 fps
//   frame), and every sound timer and fade steps by it: above that rate the sound's clock would run ahead of the game's
//   (the field battle music would start each segment over the last). The floor is the game's times the frame's step in
//   frames at 30 fps: the console's at 30 fps.
// - Keyframed bodies (lifts, gates, a Divine Beast tilting) get each physics step the angular velocity that takes them to
//   their target, from the angle of the turn left: 2 acos(|w|) of its quaternion (hkQuaternion::getAngle), which rounds
//   to 0 below about a milliradian. The smaller turns of shorter steps read as none, and a tilting floor would stand
//   still for some steps and catch up at once (Link hops on it). kf_angle gives 2 atan2(|xyz|, |w|), the same angle
//   without the rounding.
//
// - Havok's deactivation checks (phys_world_tick, phys_body_tick) count console frames, not physics steps.
//
// All found by Savage Wind (#144, #145, #209, #398).
//
// The hooks rewrite instructions and branch to HLE trampolines, so they apply natively and to the image that is lifted.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Words replaced at main+`at`: `expected` there, `code` instead, whose word `call` is a BL to the hook.
pub struct Patch {
    pub at: usize,
    pub expected: &'static [u32],
    pub code: &'static [u32],
    pub call: usize,
}

/// Where one version's clock code is (offsets in main), and the code expected there.
pub struct Sites {
    /// The frame loop's vsync wait (it waits for a vblank, then for vblanks until interval / 60 s have passed): words
    /// (offset, the game's, ours) that skip it. 1.9.0 has it in one function, 1.5.0 inlines it twice.
    pub frame_wait: &'static [(usize, u32, u32)],
    /// In VFR::updateInterval, from where the ratio (+0x3c) is set to where the cores take it: the same, then a call of
    /// vfr_update with x0 = the VFR, and the ratio in s1 again for 1.5.0's loop over the cores.
    pub vfr_ratio: Patch,
    /// The GUI speed: the interval in w0 for a call of gui_speed, its result in s0 where the game stores
    /// interval x 0.5 at +0xb20.
    pub gui_speed: Patch,
    /// The controllers' step: code that loads it (adrp x8; mov x21, xzr; ldr x8, [x8, #<slot>]; mov w22, #1;
    /// ldr s8, [x8]), and the float's offset in main the GOT slot holds.
    pub controller_step: Option<(usize, usize)>,
    /// Arrows: the dead call `bl <returns 0>; tbz w0, #0, +0x1c` (and the four words after it) at arrow.0, the function it
    /// calls at arrow.1, and the gravity's load `ldr s1, [x20, #0x108]` at arrow.2.
    pub arrow: Option<(usize, usize, usize)>,
    /// The sound frame's floor (s8 = max(the delta frame, snd_floor()) after it) and the game's floor in frames at
    /// 30 fps.
    pub snd_floor: Option<(Patch, f32)>,
    /// The keyframed body's `bl hkQuaternion::getAngle` (x0 = the quaternion), and the function it calls.
    pub kf_angle: Option<(usize, usize)>,
    /// Havok's deactivation: the world step's counter update (PHYS_WORLD_TICK_CODE, x8 = the hkpWorld), in the step and
    /// in the multithreaded step, and each body's check counter (PHYS_BODY_TICK_CODE with the body's register).
    pub phys_world_tick: [usize; 2],
    pub phys_body_tick: [(usize, u32); 2],
    /// A player demo move's arrival (the move that walks Link along a rail or to a point in a cutscene, shared by
    /// PlayerRailMove and PlayerDestinationMove): `fcmp s8, s9` (distance left, one console frame of walking), its b.mi, and
    /// the nine words after it through the other end test's `fcmp s8, s0` (2 x speed x delta frame), DEMO_MOVE_FIXED.
    pub demo_move: Option<usize>,
}

pub const STP_FRAME_WAIT: u32 = 0xa9bd_7bfd;
/// 1.9.0's VFR::updateInterval after the ratio is worked out (x19 = VFR).
pub const VFR_RATIO_CODE: [u32; 5] = [
    0x1e20_2020, // fcmp s1, s0
    0x3900_a268, // strb w8, [x19, #0x28]
    0x5400_0040, // b.eq +8
    0xbd00_3e61, // str s1, [x19, #0x3c]
    0xb940_7a68, // ldr w8, [x19, #0x78]
];
/// strb; str (unconditional: the same value); mov x0, x19; bl <vfr_update>; ldr.
pub const VFR_RATIO_CALL: [u32; 5] = [VFR_RATIO_CODE[1], VFR_RATIO_CODE[3], 0xaa13_03e0, BL, VFR_RATIO_CODE[4]];

/// 1.5.0's VFR::updateInterval from the interval check to the ratio's store (x0 = VFR, a leaf function: no frame, the
/// ratio stays in s1 for the first core).
pub const VFR_RATIO_CODE_V1_5: [u32; 24] = [
    0x1e27_0120, 0x3400_0201, 0x6b01_011f, 0x5400_01c0, 0xbd41_5002, 0x7e21_d842, 0x1e2e_1003, 0x1e22_1862, 0x3200_03e8,
    0x1e23_0021, 0x1e22_0821, 0xb900_3401, 0x3900_a008, 0xbd00_3001, 0x1e20_2020, 0x5400_00c1, 0x1400_0007, 0xbd40_3001,
    0x3900_a01f, 0x1e20_2020, 0x5400_0060, 0xbd00_3c01, 0x1400_0002, 0x4ea0_1c01,
];
/// The same choice of the ratio in fewer words (without the old ratio in s0: the store is unconditional), then
/// stp x0, x30, [sp, #-0x10]!; bl <vfr_update>; ldp x0, x30, [sp], #0x10; ldr s1, [x0, #0x3c].
pub const VFR_RATIO_CALL_V1_5: [u32; 24] = [
    0x3400_01c1, // cbz w1, <same>
    0x6b01_011f, // cmp w8, w1
    0x5400_0180, // b.eq <same>
    0xbd41_5002, // ldr s2, [x0, #0x150]
    0x7e21_d842, // ucvtf s2, s2
    0x1e2e_1003, // fmov s3, #1.0
    0x1e22_1862, // fdiv s2, s3, s2
    0x3200_03e8, // mov w8, #1
    0x1e23_0021, // ucvtf s1, w1
    0x1e22_0821, // fmul s1, s1, s2
    0xb900_3401, // str w1, [x0, #0x34]
    0x3900_a008, // strb w8, [x0, #0x28]
    0xbd00_3001, // str s1, [x0, #0x30]
    0x1400_0003, // b <store>
    0xbd40_3001, // same: ldr s1, [x0, #0x30]
    0x3900_a01f, // strb wzr, [x0, #0x28]
    0xbd00_3c01, // store: str s1, [x0, #0x3c]
    0xa9bf_7be0,
    BL,
    0xa8c1_7be0,
    0xbd40_3c01,
    NOP,
    NOP,
    NOP,
];

/// The GUI speed code with the version's adrp and add x19 as words 1 and 2.
pub const fn gui_speed_code(adrp: u32, add: u32) -> [u32; 8] {
    [
        0x1e23_0000, // ucvtf s0, w0
        adrp,        // adrp x19, <page>
        add,         // add x19, x19, #<object pointer>
        0x1e2c_1001, // fmov s1, #0.5
        0x2a1f_03e0, // mov w0, wzr
        0xf940_0268, // ldr x8, [x19]
        0x1e21_0800, // fmul s0, s0, s1
        0xbd0b_2100, // str s0, [x8, #0xb20]
    ]
}

/// The same with ucvtf s0, w0 -> bl <gui_speed> (s0), fmov s1 and fmul -> nop.
pub const fn gui_speed_call(adrp: u32, add: u32) -> [u32; 8] {
    let mut c = gui_speed_code(adrp, add);
    c[0] = BL;
    c[3] = NOP;
    c[6] = NOP;
    c
}

/// 1.5.0's GUI speed (w8 = the interval; 0.5 when it is 0), from fmov s1, #0.5 to the fcsel before the store.
pub const GUI_SPEED_CODE_V1_5: [u32; 8] = [
    0x1e2c_1001, // fmov s1, #0.5
    0x1e23_0100, // ucvtf s0, w8
    0xf947_4273, // ldr x19, [x19, #0xe80]
    0xf940_0269, // ldr x9, [x19]
    0x7100_011f, // cmp w8, #0
    0x2a1f_03e0, // mov w0, wzr
    0x1e21_0800, // fmul s0, s0, s1
    0x1e20_0c20, // fcsel s0, s1, s0, eq
];
/// mov w0, w8; bl <gui_speed>; the two loads and mov w0, wzr in place.
pub const GUI_SPEED_CALL_V1_5: [u32; 8] = [0x2a08_03e0, BL, 0xf947_4273, 0xf940_0269, NOP, 0x2a1f_03e0, NOP, NOP];

/// 1.9.0's sound floor: fmov s1, #0.5; fcmp s0, s1; fcsel s8, s1, s0, mi (s0 = the delta frame), and
/// fmov s8, s0; bl <snd_floor>; fmax s8, s8, s0 (max(delta frame, floor), NaN as fcsel gave it).
pub const SND_FLOOR_CODE: [u32; 3] = [0x1e2c_1001, 0x1e21_2000, 0x1e20_4c28];
pub const SND_FLOOR_CALL: [u32; 3] = [0x1e20_4008, BL, 0x1e20_4908];
/// 1.5.0's: ldr s0, [x8] (the delta frame); fmov s1, #1.0; fmax s8, s0, s1, and ldr s8, [x8]; bl <snd_floor>;
/// fmax s8, s8, s0.
pub const SND_FLOOR_CODE_V1_5: [u32; 3] = [0xbd40_0100, 0x1e2e_1001, 0x1e21_4808];
pub const SND_FLOOR_CALL_V1_5: [u32; 3] = [0xbd40_0108, BL, 0x1e20_4908];

pub const RET: u32 = 0xd65f_03c0;
pub const NOP: u32 = 0xd503_201f;
const BL: u32 = 0x9400_0000;

/// Whether the running title has the clock hook (installed when it was loaded; a lifted game also had it when lifted).
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// The controllers' step float (guest address; 0: not found).
static CONTROLLER_STEP: AtomicU64 = AtomicU64::new(0);
/// The interval VFR held at its last update (the stock step, with real-time game speed off).
static LAST_INTERVAL: AtomicU32 = AtomicU32::new(2);
/// The game's floor of the sound frame, in frames at 30 fps (f32 bits).
static SND_FLOOR: AtomicU32 = AtomicU32::new(0x3f00_0000);

/// Whether the game steps by real time now: then it may run at any frame rate without running fast or slow.
pub fn real_time() -> bool {
    INSTALLED.load(Ordering::Relaxed) && crate::host::live::real_time_speed()
}

/// The last frame's step in frames at 30 fps (`interval / 2` with real-time game speed off).
fn frames_at_30(interval: u32) -> f32 {
    if !crate::host::live::real_time_speed() {
        return interval.max(1) as f32 * 0.5;
    }
    let dt = crate::profile::frame_step::last_step_seconds();
    if dt > 0.0 { dt * 30.0 } else { 1.0 }
}

/// VFR::updateInterval, after it set the ratio (+0x3c) and before the cores take it: the ratio from real time.
pub unsafe extern "C" fn vfr_update(vfr: usize) {
    if vfr == 0 {
        return;
    }
    const OVERRIDE: usize = 0x29;
    const INTERVAL: usize = 0x34;
    const RATIO: usize = 0x3c;
    let interval = *((vfr + INTERVAL) as *const u32);
    LAST_INTERVAL.store(interval, Ordering::Relaxed);
    let dt = crate::profile::frame_step::step_seconds(interval as i32);
    if crate::host::live::real_time_speed() && *((vfr + OVERRIDE) as *const u8) == 0 {
        *((vfr + RATIO) as *mut f32) = dt * 30.0;
    }
    let step = CONTROLLER_STEP.load(Ordering::Relaxed) as usize;
    if step != 0 {
        *(step as *mut f32) = frames_at_30(interval);
    }
    static CALLED: AtomicBool = AtomicBool::new(false);
    if !CALLED.swap(true, Ordering::Relaxed) {
        println!("[+] Game time: the game steps by real time ({:.1} ms this frame)", dt * 1000.0);
    }
}

/// The GUI speed (w0 = the frame loop's interval): the frame's step in frames at 30 fps.
pub unsafe extern "C" fn gui_speed(interval: u32) -> f32 {
    frames_at_30(interval)
}

/// A falling arrow's gravity (x0 = the arrow): per frame at 30 fps, scaled to the frame's step.
pub unsafe extern "C" fn arrow_gravity(arrow: usize) -> f32 {
    *((arrow + 0x108) as *const f32) * frames_at_30(LAST_INTERVAL.load(Ordering::Relaxed))
}

/// The floor of the sound system's frame: the game's, times the frame's step.
pub unsafe extern "C" fn snd_floor() -> f32 {
    f32::from_bits(SND_FLOOR.load(Ordering::Relaxed)) * frames_at_30(LAST_INTERVAL.load(Ordering::Relaxed))
}

/// The world step's deactivation counter update (hkpWorld +0x3ef, flags at +0x3ed / +0x3ee), 21 words from the counter's
/// load to its reset; branches inside it only.
pub const PHYS_WORLD_TICK_CODE: [u32; 21] = [
    0x394f_bd0a, 0x1100_0549, 0x1100_154a, 0x390f_bd09, 0x1200_094a, 0x3500_008a, 0x394f_b50a, 0x5200_014a, 0x390f_b50a,
    0x1200_092a, 0x3500_008a, 0x394f_b50a, 0x521f_014a, 0x390f_b50a, 0x1200_0d29, 0x3500_00c9, 0x394f_b909, 0x3200_03ea,
    0x4b09_0149, 0x390f_b909, 0x390f_bd1f,
];

/// A body's deactivation check counter (hkpMotion +0xd): ldrb w8, [xM, #0xd]; add w8, w8, #1; strb w8, [xM, #0xd];
/// and w9, w8, #3 (cbnz w9, <no check this step> follows).
pub const fn phys_body_tick_code(motion: u32) -> [u32; 4] {
    [0x3940_3408 | motion << 5, 0x1100_0508, 0x3900_3408 | motion << 5, 0x1200_0509]
}

/// Whether the latest world step completes a console frame (phys_world_tick): only those count a deactivation check.
static PHYS_TICKED: AtomicBool = AtomicBool::new(true);

/// Per world: console frames stepped and not yet counted, the deactivation classes' values as the game set them and as
/// given for the step length last used.
struct PhysWorld {
    frames: f32,
    game: [[f32; 2]; 6],
    put: [[f32; 2]; 6],
    put_r: f32,
}
static PHYS_WORLDS: std::sync::Mutex<Vec<(usize, PhysWorld)>> = std::sync::Mutex::new(Vec::new());
const PHYS_CLASSES: usize = 0x314;

/// Havok's deactivation at the console's pace (Savage Wind #398). Havok judges whether a body came to rest once per
/// physics step, and the game takes one step a frame: a body still for 24 steps (0.8 s at 30 fps) puts its island to
/// sleep, 0.07 s at 360 fps, and Ree Dahee Shrine's ball falls asleep on its ramp for good above ~120 fps. Only the steps
/// that complete a console frame (1/30 s of the frame's step, slow motion left out as on the console) run the world's
/// and the bodies' check counters; every step at 30 fps and slower. In every step the solver slows a body under its
/// deactivation class's speed by a multiplier and stops it under the class's sleep speed (hkpWorld +0x314, 0x20 a
/// class: +8, +0xc): a step adds speed in proportion to its length r (console frames, at most 1), so the multiplier
/// becomes multiplier^r and the sleep speed sleep speed x r: as much slowing a second as on the console.
/// Replaces the counter update (x0 = the hkpWorld).
pub unsafe extern "C" fn phys_world_tick(world: usize) {
    if world == 0 {
        return;
    }
    let r = frames_at_30(LAST_INTERVAL.load(Ordering::Relaxed)).min(1.0);
    let r = if r > 0.0 { r } else { 1.0 };
    let mut worlds = PHYS_WORLDS.lock().unwrap();
    let i = match worlds.iter().position(|(w, _)| *w == world) {
        Some(i) => i,
        None => {
            if worlds.len() >= 16 {
                worlds.remove(0);
            }
            worlds.push((world, PhysWorld { frames: 0.0, game: [[0.0; 2]; 6], put: [[f32::NAN; 2]; 6], put_r: 0.0 }));
            worlds.len() - 1
        }
    };
    let p = &mut worlds[i].1;
    // Steps of 1/n console frame add up to 1 within float rounding.
    p.frames += r;
    let tick = p.frames >= 1.0 - 1e-4;
    if tick {
        p.frames = (p.frames - 1.0).max(0.0);
    }
    PHYS_TICKED.store(tick, Ordering::Relaxed);
    for c in 0..6 {
        let m = (world + PHYS_CLASSES + 0x20 * c + 8) as *mut [f32; 2];
        let now = *m;
        if now != p.put[c] {
            p.game[c] = now;
            p.put_r = 0.0;
        }
    }
    if r != p.put_r {
        for c in 0..6 {
            let m = (world + PHYS_CLASSES + 0x20 * c + 8) as *mut [f32; 2];
            let g = p.game[c];
            let v = if r == 1.0 { g } else { [g[0].powf(r), g[1] * r] };
            *m = v;
            p.put[c] = v;
        }
        p.put_r = r;
    }
    if !tick {
        return;
    }
    // The game's own update.
    let b = |o: usize| (world + o) as *mut u8;
    let v = *b(0x3ef) as u32;
    let (w9, w10) = (v + 1, v + 5);
    *b(0x3ef) = w9 as u8;
    if w10 & 7 == 0 {
        *b(0x3ed) ^= 1;
    }
    if w9 & 7 == 0 {
        *b(0x3ed) ^= 2;
    }
    if w9 & 0xf == 0 {
        *b(0x3ee) = 1u8.wrapping_sub(*b(0x3ee));
        *b(0x3ef) = 0;
    }
}

/// A body's check counter (x0 = its hkpMotion): incremented and returned when the world's step completes a console frame;
/// otherwise 1, which skips the check (the counter is left as it was).
pub unsafe extern "C" fn phys_body_tick(motion: usize) -> u32 {
    if motion == 0 || !PHYS_TICKED.load(Ordering::Relaxed) {
        return 1;
    }
    let c = *((motion + 0xd) as *const u8) as u32 + 1;
    *((motion + 0xd) as *mut u8) = c as u8;
    c
}

/// The angle of a turn (x0 = its quaternion x, y, z, w): 2 atan2(|xyz|, |w|).
pub unsafe extern "C" fn kf_angle(q: usize) -> f32 {
    let v = std::slice::from_raw_parts(q as *const f32, 4);
    2.0 * (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().atan2(v[3].abs())
}

#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, s: &Sites) {
    use crate::profile::totk::frame_time::replace_with_call;
    let apply = |mem_space: &mut crate::guest::loader::GuestMemorySpace, p: &Patch, hle: *const ()| {
        replace_with_call(base, mem_space, p.at, p.expected, p.code.to_vec(), p.call, hle)
    };
    match apply(mem_space, &s.vfr_ratio, crate::hle!(vfr_update(_))) {
        Ok(()) => {
            INSTALLED.store(true, Ordering::Relaxed);
            println!("[+] Game time: each frame steps by the real time since the last one (1/240 to 1/15 s)");
        }
        Err(e) => {
            println!("[!] Game time: main+{:#x}: {e}; the step stays tied to the present interval", s.vfr_ratio.at);
            return;
        }
    }
    if let Some((code, float)) = s.controller_step {
        install_controller_step(base, code, float);
    }
    match apply(mem_space, &s.gui_speed, crate::hle!(gui_speed(_))) {
        Ok(()) => println!("[+] Menus: their speed follows the frame's step"),
        Err(e) => println!("[!] Menus: main+{:#x}: {e}; their speed stays tied to the present interval", s.gui_speed.at),
    }
    if let Some((dead, zero, load)) = s.arrow {
        install_arrow(base, mem_space, dead, zero, load);
    }
    if let Some(at) = s.demo_move {
        install_demo_move(base, mem_space, at);
    }
    if let Some((p, floor)) = &s.snd_floor {
        SND_FLOOR.store(floor.to_bits(), Ordering::Relaxed);
        match apply(mem_space, p, crate::hle!(snd_floor())) {
            Ok(()) => println!("[+] Sound: its frame follows the frame's step"),
            Err(e) => println!("[!] Sound: main+{:#x}: {e}; its frame stays at least the game's floor", p.at),
        }
    }
    install_phys(base, mem_space, s);
    if let Some((at, get_angle)) = s.kf_angle {
        // bl <getAngle> -> bl <kf_angle> (same argument and result).
        let expected = BL | ((get_angle as i64 - at as i64) >> 2) as u32 & 0x03ff_ffff;
        match replace_with_call(base, mem_space, at, &[expected], vec![BL], 0, crate::hle!(kf_angle(_))) {
            Ok(()) => println!("[+] Keyframed bodies: small turns are kept at any frame rate"),
            Err(e) => println!("[!] Keyframed bodies: main+{at:#x}: {e}; not patched"),
        }
    }
    let ours = |&(at, _, to): &(usize, u32, u32)| std::ptr::read((base + at) as *const u32) == to;
    let game = |&(at, from, _): &(usize, u32, u32)| std::ptr::read((base + at) as *const u32) == from;
    if s.frame_wait.iter().all(game) {
        for &(at, _, to) in s.frame_wait {
            let p = (base + at) as *mut u32;
            std::ptr::write(p, to);
            crate::platform::os::flush_icache(p as usize, 4);
        }
        println!("[+] Frame loop: its vsync wait is left to the present pacer");
    } else if !s.frame_wait.iter().all(ours) {
        println!("[!] Frame loop: unexpected code at main+{:#x}; its vsync wait stays", s.frame_wait[0].0);
    }
}

/// The controllers' step: the GOT slot the code loads must hold the float's address.
#[cfg(nx_runtime)]
unsafe fn install_controller_step(base: usize, code: usize, float: usize) {
    let w = |k: usize| std::ptr::read((base + code + 4 * k) as *const u32);
    let (adrp, ldr) = (w(0), w(2));
    if adrp & 0x9f00_001f == 0x9000_0008 && w(1) == 0xaa1f_03f5 && ldr & 0xffc0_03ff == 0xf940_0108 && w(4) == 0xbd40_0108 {
        let imm = ((adrp >> 29) & 3 | ((adrp >> 5) & 0x7_ffff) << 2) as i64;
        let imm = (imm << 43) >> 43;
        let slot = (((base + code) & !0xfff) as i64 + (imm << 12)) as usize + ((ldr >> 10) & 0xfff) as usize * 8;
        let target = *(slot as *const usize);
        if target == base + float {
            CONTROLLER_STEP.store(target as u64, Ordering::Relaxed);
            println!("[+] Controllers: hold times follow the frame's step");
        } else {
            println!("[!] Controllers: the step's slot holds {target:#x}, not main+{float:#x}; holds count frames");
        }
    } else {
        println!("[!] Controllers: unexpected code at main+{code:#x}; holds count frames");
    }
}

#[cfg(nx_runtime)]
unsafe fn install_phys(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, s: &Sites) {
    use crate::profile::totk::frame_time::replace_with_call;
    // Havok's deactivation: mov x0, x8; bl <phys_world_tick>; b <past the update> (the rest stays, unreached), and in each
    // body: mov x0, xM; bl <phys_body_tick>; mov w8, w0; and w9, w8, #3 (its cbnz follows).
    let mut phys = 0;
    for at in s.phys_world_tick {
        let mut code = PHYS_WORLD_TICK_CODE.to_vec();
        code[..3].copy_from_slice(&[0xaa08_03e0, BL, 0x1400_0000 | (0x54 - 8) / 4]);
        match replace_with_call(base, mem_space, at, &PHYS_WORLD_TICK_CODE, code, 1, crate::hle!(phys_world_tick(_))) {
            Ok(()) => phys += 1,
            Err(e) => println!("[!] Physics: main+{at:#x}: {e}; the world's deactivation counts every step"),
        }
    }
    for (at, motion) in s.phys_body_tick {
        let code = vec![0xaa00_03e0 | motion << 16, BL, 0x2a00_03e8, 0x1200_0509];
        match replace_with_call(base, mem_space, at, &phys_body_tick_code(motion), code, 1, crate::hle!(phys_body_tick(_))) {
            Ok(()) => phys += 1,
            Err(e) => println!("[!] Physics: main+{at:#x}: {e}; bodies' deactivation counts every step"),
        }
    }
    if phys == 4 {
        println!("[+] Physics: deactivation checks at the console's pace");
    }
}

/// Arrows' gravity: the dead call (`bl <mov w0, wzr; ret>`, `tbz w0, #0, +0x1c` and the four words of its branch)
/// becomes `mov x0, x20; bl <arrow_gravity>; fmov s4, s0; b +0x14`, and the gravity's load `ldr s1, [x20, #0x108]`
/// becomes `fmov s1, s4` (s4 is not touched between them; 1.5.0 calls sqrtf there only for a NaN velocity).
#[cfg(nx_runtime)]
unsafe fn install_arrow(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, dead: usize, zero: usize, load: usize) {
    use crate::profile::totk::frame_time::replace_with_call;
    const LDR_GRAVITY: u32 = 0xbd41_0a81; // ldr s1, [x20, #0x108]
    const FMOV_S1_S4: u32 = 0x1e20_4081;
    let returns_zero = std::slice::from_raw_parts((base + zero) as *const u32, 2) == [0x2a1f_03e0, RET];
    let bl_zero = BL | ((zero as i64 - dead as i64) >> 2) as u32 & 0x03ff_ffff;
    let expected = [bl_zero, 0x3600_00e0, 0xf940_4288, 0x2d40_0500]; // bl; tbz w0, #0, +0x1c; ldr x8, [x20, #0x80]; ldp s0, s1, [x8]
    let p = (base + load) as *mut u32;
    if !returns_zero || std::ptr::read(p) != LDR_GRAVITY {
        println!("[!] Arrows: unexpected code at main+{zero:#x} / main+{load:#x}; their gravity stays per frame");
        return;
    }
    let code = vec![0xaa14_03e0, BL, 0x1e20_4004, 0x1400_0005];
    match replace_with_call(base, mem_space, dead, &expected, code, 1, crate::hle!(arrow_gravity(_))) {
        Ok(()) => {
            std::ptr::write(p, FMOV_S1_S4);
            crate::platform::os::flush_icache(p as usize, 4);
            println!("[+] Arrows: their fall follows the frame's step");
        }
        Err(e) => println!("[!] Arrows: main+{dead:#x}: {e}; their gravity stays per frame"),
    }
}

/// The demo move's arrival code (index, word): fcmp s8, s9; b.mi <arrive>; adrp x8 (any, the constant's page);
/// ldr s1, [x19, #0x40]; ldr s2, [x8, #lo]; fadd s1, s1, s2; fcmp s1, s8; b.pl <store>; fadd s1, s9, s9;
/// fmul s0, s1, s0; fcmp s8, s0; b.mi <arrive>.
const DEMO_MOVE_FIXED: [(usize, u32); 10] = [
    (0, 0x1e29_2100),
    (1, 0x5400_01a4),
    (3, 0xbd40_4261),
    (5, 0x1e22_2821),
    (6, 0x1e28_2020),
    (7, 0x5400_00a5),
    (8, 0x1e29_2921),
    (9, 0x1e20_0820),
    (10, 0x1e20_2100),
    (11, 0x5400_0064),
];

/// The game's constant the second test adds to the distance kept from the last frame (main+..., found at install).
static DEMO_MOVE_SLACK: AtomicU64 = AtomicU64::new(0);

/// Whether a player demo move has arrived (1: go to its end, 0: keep the distance left and go on), as the game decides
/// it with one change. The move (PlayerRailMove / PlayerDestinationMove's shared step, x19) ends when the distance left
/// is under one console frame of walking (speed), or, when it no longer shrinks by more than the game's slack, under
/// 2 x speed x the delta frame. Above ~100 fps a frame's step is short enough for Link to come within one console
/// frame of an intermediate point of a rail, and the walk ends there for good (leaving the Shrine of Resurrection).
/// So the first test takes speed x the delta frame when the frame is shorter than a console frame (0 < delta < 1);
/// a zero delta (paused, loading) or a longer frame keeps the game's test. Savage Wind's finding (MIT, #364).
pub unsafe extern "C" fn demo_move_arrive(mv: usize, delta: f32, speed: f32, left: f32) -> u32 {
    let step = if delta > 0.0 && delta < 1.0 { delta } else { 1.0 };
    if left < speed * step {
        return 1;
    }
    let slack = f32::from_bits(DEMO_MOVE_SLACK.load(Ordering::Relaxed) as u32);
    let kept = *((mv + 0x40) as *const f32);
    if !(kept + slack < left) {
        return 0;
    }
    (left < 2.0 * speed * delta) as u32
}

/// mov x0, x19; fmov s1, s9; fmov s2, s8 (s0 is the delta frame already); bl <demo_move_arrive>; cbnz w0, <arrive>;
/// b <store>. The game's words after them are no longer reached.
#[cfg(nx_runtime)]
unsafe fn install_demo_move(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, at: usize) {
    let site = (base + at) as *const u32;
    let old: Vec<u32> = (0..12).map(|k| std::ptr::read(site.add(k))).collect();
    let ldr = old[4];
    if DEMO_MOVE_FIXED.iter().any(|&(k, w)| old[k] != w) || old[2] & 0x9f00_001f != 0x9000_0008 || ldr & 0xffc0_03ff != 0xbd40_0102 {
        println!("[!] Demo moves: unexpected code at main+{at:#x}; their arrival stays one 30 fps frame of walking");
        return;
    }
    let adrp = old[2];
    let imm = ((adrp >> 29) & 3 | ((adrp >> 5) & 0x7_ffff) << 2) as i64;
    let page = (((base + at + 8) & !0xfff) as i64 + (((imm << 43) >> 43) << 12)) as usize;
    let slack = page + ((ldr >> 10) & 0xfff) as usize * 4;
    DEMO_MOVE_SLACK.store(std::ptr::read(slack as *const u32) as u64, Ordering::Relaxed);
    // cbnz at word 4 to word 14 (the arrival, +0x28); b at word 5 to word 12 (the store, +0x1c).
    let code = vec![0xaa13_03e0, 0x1e20_4121, 0x1e20_4102, BL, 0x3500_0140, 0x1400_0007];
    match crate::profile::totk::frame_time::replace_with_call(base, mem_space, at, &old[..6], code, 3, crate::hle!(demo_move_arrive(_, _, _, _))) {
        Ok(()) => println!("[+] Demo moves: they arrive within the frame's step of walking"),
        Err(e) => println!("[!] Demo moves: main+{at:#x}: {e}; their arrival stays one 30 fps frame of walking"),
    }
}
