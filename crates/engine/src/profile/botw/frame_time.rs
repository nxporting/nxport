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
//
// The hooks rewrite instructions and branch to HLE trampolines, so they apply natively and to the image that is lifted.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
    let p = (base + s.frame_wait) as *mut u32;
    match std::ptr::read(p) {
        STP_FRAME_WAIT => {
            std::ptr::write(p, RET);
            crate::os::flush_icache(p as usize, 4);
            println!("[+] Frame loop: its vsync wait is left to the present pacer");
        }
        RET => {}
        w => println!("[!] Frame loop: unexpected instruction {w:#010x} at main+{:#x}; its vsync wait stays", s.frame_wait),
    }
}
