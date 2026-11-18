// Game time in Breath of the Wild (1.9.0; the sites were found by Savage Wind, MIT, tools/game_sites.py and
// runtime/hle/framepace.c, and checked here against the code).
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
//   object): 1 at 30 fps, and its cursor and animations ran faster above. gui_speed gives the frame's step in frames at
//   30 fps instead (as Cemu's FPS++ does on the Wii U).
// - The controllers' hold counters (sead::ControllerBase::updateDerivativeParams_) go up by a float the game keeps at 1
//   (read through a GOT slot): it is set to the frame's step in frames at 30 fps, so holds last as long as on the
//   console.
// - The frame loop's vsync wait (it waits for vblanks until interval / 60 s have passed; only the first frames after
//   start-up reach it) returns at once: the engine's present pacer paces.
// - Arrows: once in their fall, an arrow adds its gravity (0, ArrowGravity, 0) to its velocity once per update, with no
//   step (the straight part before it scales by the delta frame): above 30 fps they fell 2 or 4 times as fast and landed
//   short. The gravity is scaled by the frame's step in frames at 30 fps (slow motion left out, as on the console).
//   The code calls a function that always returns 0 just before (the branch it guards is never taken); that call and
//   its dead branch make room for the call that scales it.
// - The sound system's frame: the game hands it the delta frame, at least 0.5 (a 60 fps frame), and every sound timer
//   and fade steps by it: above 60 fps the sound's clock ran ahead of the game's (the field battle music started each
//   segment over the last). The floor is 0.5 x the frame's step in frames at 30 fps: the console's at 30 fps.
// - Keyframed bodies (lifts, gates, a Divine Beast tilting) get each physics step the angular velocity that takes them to
//   their target, from the angle of the turn left: 2 acos(|w|) of its quaternion (hkQuaternion::getAngle), which rounds
//   to 0 below about a milliradian. The smaller turns of shorter steps read as none, and a tilting floor stood still
//   for some steps and caught up at once (Link hopped on it). kf_angle gives 2 atan2(|xyz|, |w|), the same angle
//   without the rounding.
//
// - Havok's deactivation checks (phys_world_tick, phys_body_tick) count console frames, not physics steps.
//
// All found by Savage Wind (#144, #145, #209, #398).
//
// The hooks rewrite instructions and branch to HLE trampolines, so they apply natively and to the image that is lifted.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// Where one version's clock code is (offsets in main), and the code expected there.
pub struct Sites {
    /// The frame loop's vsync wait: its first word, stp x29, x30, [sp, #-0x30]!.
    pub frame_wait: usize,
    /// In VFR::updateInterval, after the ratio is worked out (x19 = VFR): fcmp s1, s0; strb w8, [x19, #0x28];
    /// b.eq +8; str s1, [x19, #0x3c]; ldr w8, [x19, #0x78].
    pub vfr_ratio: usize,
    /// The GUI speed: ucvtf s0, w0 (w0 = the interval) .. str s0, [x8, #0xb20]; words 1 and 2 (adrp, add x19) stay.
    pub gui_speed: usize,
    pub gui_speed_code: [u32; 8],
    /// The controllers' step: code that loads it (adrp x8; mov x21, xzr; ldr x8, [x8, #<slot>]; mov w22, #1;
    /// ldr s8, [x8]), and the float's offset in main the GOT slot holds.
    pub controller_step: (usize, usize),
    /// Arrows: the dead call `bl <returns 0>; tbz w0, #0, +0x1c` (and the four words after it) at arrow.0, the function it
    /// calls at arrow.1, and the gravity's load `ldr s1, [x20, #0x108]` at arrow.2.
    pub arrow: Option<(usize, usize, usize)>,
    /// The sound frame's floor: fmov s1, #0.5; fcmp s0, s1; fcsel s8, s1, s0, mi (s0 = the delta frame).
    pub snd_floor: Option<usize>,
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
pub const VFR_RATIO_CODE: [u32; 5] = [
    0x1e20_2020, // fcmp s1, s0
    0x3900_a268, // strb w8, [x19, #0x28]
    0x5400_0040, // b.eq +8
    0xbd00_3e61, // str s1, [x19, #0x3c]
    0xb940_7a68, // ldr w8, [x19, #0x78]
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

const RET: u32 = 0xd65f_03c0;
const NOP: u32 = 0xd503_201f;
const BL: u32 = 0x9400_0000;

/// Whether the running title has the clock hook (installed when it was loaded; a lifted game also had it when lifted).
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// The controllers' step float (guest address; 0: not found).
static CONTROLLER_STEP: AtomicU64 = AtomicU64::new(0);
/// The interval VFR held at its last update (the stock step, with real-time game speed off).
static LAST_INTERVAL: AtomicU32 = AtomicU32::new(2);

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

/// The floor of the sound system's frame: half a frame at 30 fps, times the frame's step.
pub unsafe extern "C" fn snd_floor() -> f32 {
    0.5 * frames_at_30(LAST_INTERVAL.load(Ordering::Relaxed))
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
/// sleep, 0.07 s at 360 fps, and Ree Dahee Shrine's ball fell asleep on its ramp for good above ~120 fps. Only the steps
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
    // fcmp; strb; b.eq; str -> strb; str (unconditional: the same value); mov x0, x19; bl <vfr_update>.
    let vfr = vec![VFR_RATIO_CODE[1], VFR_RATIO_CODE[3], 0xaa13_03e0, BL, VFR_RATIO_CODE[4]];
    match replace_with_call(base, mem_space, s.vfr_ratio, &VFR_RATIO_CODE, vfr, 3, crate::hle!(vfr_update(_))) {
        Ok(()) => {
            INSTALLED.store(true, Ordering::Relaxed);
            println!("[+] Game time: each frame steps by the real time since the last one (1/240 to 1/15 s)");
        }
        Err(e) => {
            println!("[!] Game time: main+{:#x}: {e}; the step stays tied to the present interval", s.vfr_ratio);
            return;
        }
    }
    // The controllers' step: the GOT slot the code loads must hold the float's address.
    let (code, float) = s.controller_step;
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
    // ucvtf s0, w0 -> bl <gui_speed> (s0); fmov s1 and fmul -> nop.
    let mut gui = s.gui_speed_code.to_vec();
    gui[0] = BL;
    gui[3] = NOP;
    gui[6] = NOP;
    match replace_with_call(base, mem_space, s.gui_speed, &s.gui_speed_code, gui, 0, crate::hle!(gui_speed(_))) {
        Ok(()) => println!("[+] Menus: their speed follows the frame's step"),
        Err(e) => println!("[!] Menus: main+{:#x}: {e}; their speed stays tied to the present interval", s.gui_speed),
    }
    if let Some((dead, zero, load)) = s.arrow {
        install_arrow(base, mem_space, dead, zero, load);
    }
    if let Some(at) = s.demo_move {
        install_demo_move(base, mem_space, at);
    }
    if let Some(at) = s.snd_floor {
        // fmov s1, #0.5; fcmp s0, s1; fcsel s8, s1, s0, mi -> fmov s8, s0; bl <snd_floor>; fmax s8, s8, s0
        // (max(delta frame, floor), NaN as fcsel gave it).
        const CODE: [u32; 3] = [0x1e2c_1001, 0x1e21_2000, 0x1e20_4c28];
        match replace_with_call(base, mem_space, at, &CODE, vec![0x1e20_4008, BL, 0x1e20_4908], 1, crate::hle!(snd_floor())) {
            Ok(()) => println!("[+] Sound: its frame follows the frame's step"),
            Err(e) => println!("[!] Sound: main+{at:#x}: {e}; its frame stays at least half a 30 fps frame"),
        }
    }
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
    if let Some((at, get_angle)) = s.kf_angle {
        // bl <getAngle> -> bl <kf_angle> (same argument and result).
        let expected = BL | ((get_angle as i64 - at as i64) >> 2) as u32 & 0x03ff_ffff;
        match replace_with_call(base, mem_space, at, &[expected], vec![BL], 0, crate::hle!(kf_angle(_))) {
            Ok(()) => println!("[+] Keyframed bodies: small turns are kept at any frame rate"),
            Err(e) => println!("[!] Keyframed bodies: main+{at:#x}: {e}; not patched"),
        }
    }
    let p = (base + s.frame_wait) as *mut u32;
    match std::ptr::read(p) {
        STP_FRAME_WAIT => {
            std::ptr::write(p, RET);
            crate::platform::os::flush_icache(p as usize, 4);
            println!("[+] Frame loop: its vsync wait is left to the present pacer");
        }
        RET => {}
        w => println!("[!] Frame loop: unexpected instruction {w:#010x} at main+{:#x}; its vsync wait stays", s.frame_wait),
    }
}

/// Arrows' gravity: the dead call (`bl <mov w0, wzr; ret>`, `tbz w0, #0, +0x1c` and the four words of its branch)
/// becomes `mov x0, x20; bl <arrow_gravity>; fmov s4, s0; b +0x14`, and the gravity's load `ldr s1, [x20, #0x108]`
/// becomes `fmov s1, s4` (s4 is not touched between them).
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
/// frame of an intermediate point of a rail, and the walk ended there for good (leaving the Shrine of Resurrection).
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
