// Mouse movement into the camera's yaw and pitch (mouse look, host/input.rs).
//
// The camera keeps its position and the point it looks at; a function turns their difference into a distance, a pitch
// and a yaw in degrees (1.0.0: main+0xb8a1cc, called from main+0xb8a024 with x0 = where to store them and x1 = the
// vector), and the camera stores them (+0x404..0x40c of the object at camera+0x20). The next frame the turn function
// (main+0x9ab570) loads that yaw first and places the camera from it. The call is redirected to `camera_angles` below,
// which computes the same three values the same way (the game's two constants are read from its own code) and adds the
// mouse's sideways movement to the yaw: the game then turns the camera by exactly that, by its own rules (a mode that
// does not place the camera from the yaw ignores it).
//
// 1.1.2 has the same code with its fields 8 bytes further (+0x40c, the test's +0x149, the aim's +0x420) and the test on
// s11 instead of s14: the patterns mask field offsets and constants, and the test's register is read from the code.
//
// The pitch the camera is placed with is rebuilt each frame from the camera's +0x108. In its automatic mode (+0x180 = 3)
// that is eased toward a default; any right stick input switches it to manual (4), where +0x108 is the pitch the stick
// adds to and it stays where it is put. The game's test for that input (1.0.0 main+0x9abb28: fcmp s14, #0.0; b.gt) is
// rewritten to ask `camera_input`, which answers as the game would (s14, the stick's strength, above 0) or yes when the
// mouse has moved: the game then applies all its own conditions and picks the mode. The mouse's vertical degrees are
// added to +0x108 when the game stores its angles, if the camera is in manual mode; otherwise dropped, as a stick would
// turn nothing there. Nothing else of the game's code changes.
//
// The hooks need guest registers besides their arguments (the camera in x19 / x20, the stick's strength, s9). Lifted
// code reads them from the lifted CPU. Natively (and in images made for a native engine) each rewritten instruction
// calls a code block that saves every caller-saved register and the flags, passes those registers as arguments, calls the
// HLE trampoline and restores all but the result: the rewritten instructions sit in the middle of the game's functions,
// where any of them may be live.
//
// The bow's aim camera stores its angles through the same function but turns by its own steps (1.0.0 main+0x1cd2d14):
// it tests for stick input first, then adds the stick's turn in degrees to its aim yaw and pitch, which it clamps and
// which Link's body follows. Those three instructions are rewritten too (`aim_input`, `aim_yaw`, `aim_pitch`): the
// test answers as the game would or yes when mouse degrees wait, and the mouse's degrees are added where the stick's
// turn is. None of them keeps flags across the call (the lifter treats a call as leaving them undefined).
//
// The call is found by the instructions around it, not by address, so other builds compiled the same way get it too.
// Like aspect.rs, the rewritten call enters an HLE trampoline (no code of our own in guest memory), the same at lift and
// run time.

use std::sync::atomic::{AtomicU32, Ordering};

/// Around the call: stp s0, s2, [x29, #-0x10]; fsub s0, s1, s3; stur s0, [x29, #-8]; bl (the function);
/// ldr x9, [sp, #8]; add x8, x21, #0x404.
/// ldr x9, [sp, #8]; add x8, x21, #0x404 (1.1.2: #0x40c). The bl and the add are masked.
const AROUND: [(u32, u32); 6] = [
    (0x2d3e_0ba0, !0),
    (0x1e23_3820, !0),
    (0xbc1f_83a0, !0),
    (0x9400_0000, 0xfc00_0000),
    (0xf940_07e9, !0),
    (0x9100_02a8, IMM12),
];
const CALL: usize = 3;
/// A load, store or add's 12-bit immediate (field offsets move between versions).
const IMM12: u32 = 0xffc0_03ff;
/// The function's first words: sub sp, sp, #0x40; str d10, [sp, #0x10]; str x19, [sp, #0x38]; stp d9, d8, [sp, #0x18].
const CALLEE: [u32; 4] = [0xd101_03ff, 0xfd00_0bea, 0xf900_1ff3, 0x6d01_a3e9];
/// Where in the function its two constants are loaded: adrp x8 then ldr s1 (the pitch offset), adrp x8 then ldr s10
/// (degrees per radian).
const CONSTANTS_AT: usize = 0x88;

/// The camera-input test: ldrb w8, [x27, #0x141]; tbnz w8, #5, ...; fcmp s14, #0.0; b.gt (manual mode). 1.1.2:
/// [x27, #0x149] and s11.
const DECISION: [(u32, u32); 4] =
    [(0x3940_0368, IMM12), (0x3728_0008, 0xfff8_001f), (0x1e20_2008, 0xffff_fc1f), (0x5400_000c, 0xff00_001f)];
const TEST: usize = 2;
/// The bow's aim camera's turn function (1.0.0 main+0x1cd2d14) starts by testing for turn input: with the stick at
/// rest (+0x418 and +0x41c of the angles object both 0) it takes another path (+0x1e8 = 1) that does not turn the aim.
/// ldr s0, [x22, #0x418]; ldr s10, [x20, #8]; ldr x9, [x8]; ldr w8, [x8, #8]; fcmp s0, #0.0; stur x9, [x29, #-0x50];
/// stur w8, [x29, #-0x48]; b.ne (input). The fcmp is rewritten to ask `aim_input`, the b.ne to a cbnz w0.
const AIM_INPUT: [(u32, u32); 8] = [
    (0xbd40_02c0, IMM12),
    (0xbd40_0a8a, !0),
    (0xf940_0109, !0),
    (0xb940_0908, !0),
    (0x1e20_2008, !0),
    (0xf81b_03a9, !0),
    (0xb81b_83a8, !0),
    (0x5400_0001, 0xff00_001f),
];
const AIM_INPUT_AT: usize = 4;
const AIM_INPUT_BNE: usize = 7;
/// Then it adds the stick's sideways turn, in degrees, to its aim
/// yaw: ldr s1, [xN, #imm]; fmul s0, s0, s1; ldur s1, [x29, #-0x48]; fadd s0, s1, s0; bl (wrap); ldr x8, [x22, #0x158];
/// mov w9, #0x2808 (1.1.2: the ldr at +0x160, #0x5008). The fadd is rewritten to enter `aim_yaw`.
const AIM_YAW: [(u32, u32); 7] = [
    (0xbd40_0001, 0xffc0_001f),
    (0x1e21_0800, !0),
    (0xbc5b_83a1, !0),
    (0x1e20_2820, !0),
    (0x9400_0000, 0xfc00_0000),
    (0xf940_02c8, IMM12),
    (0x5280_0009, 0xffe0_001f),
];
const AIM_YAW_AT: usize = 3;
/// Its pitch: the stick's vertical turn in s0, its sign chosen by s9, goes to a call that adds it to the aim pitch
/// (+0x15c, written to +0x154): ldr s0, [sp, #0x60]; fcmp s9, #0.0; add x0, x19, #0x15c; fneg s1, s0;
/// fcsel s0, s0, s1, gt; bl. The fcsel is rewritten to enter `aim_pitch`, which makes the same choice from s9 itself (a call
/// in between would leave the flags undefined: the lifter does not keep them across calls).
const AIM_PITCH: [(u32, u32); 6] =
    [(0xbd40_63e0, !0), (0x1e20_2128, !0), (0x9105_7260, !0), (0x1e21_4001, !0), (0x1e21_cc00, !0), (0x9400_0000, 0xfc00_0000)];
const AIM_PITCH_AT: usize = 4;
/// The camera's pitch state and mode.
const PITCH: usize = 0x108;
const MODE: usize = 0x180;
const MANUAL: u32 = 4;

static OFFSET: AtomicU32 = AtomicU32::new(0);
/// The register the camera-input test compares (s14 in 1.0.0, s11 in 1.1.2).
static STICK_REG: AtomicU32 = AtomicU32::new(14);
static SCALE: AtomicU32 = AtomicU32::new(0);

fn word(text: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(text[at..at + 4].try_into().unwrap())
}

/// The call's offset in `text`, if found exactly once with the expected function behind it.
fn find(text: &[u8]) -> Option<usize> {
    let mut found = None;
    let n = text.len() / 4;
    for i in 0..(n + 1).saturating_sub(AROUND.len()) {
        let ok = AROUND.iter().enumerate().all(|(k, &(w, m))| word(text, 4 * (i + k)) & m == w);
        if ok {
            if found.is_some() {
                return None;
            }
            found = Some(4 * (i + CALL));
        }
    }
    let at = found?;
    let callee = callee_of(at, word(text, at))?;
    (callee + 16 <= text.len() && CALLEE.iter().enumerate().all(|(k, &w)| word(text, callee + 4 * k) == w)).then_some(at)
}

/// The offset in `text` of word `at` of `pattern` ((word, mask) pairs), if the pattern is found exactly once.
fn find_masked(text: &[u8], pattern: &[(u32, u32)], at: usize) -> Option<usize> {
    let mut found = None;
    for i in 0..(text.len() / 4 + 1).saturating_sub(pattern.len()) {
        if pattern.iter().enumerate().all(|(k, &(w, m))| word(text, 4 * (i + k)) & m == w) {
            if found.is_some() {
                return None;
            }
            found = Some(4 * (i + at));
        }
    }
    found
}

/// The offset in `text` of the camera-input test's fcmp, if found exactly once.
fn find_decision(text: &[u8]) -> Option<usize> {
    find_masked(text, &DECISION, TEST)
}

/// The target of the bl `w` at `at`.
fn callee_of(at: usize, w: u32) -> Option<usize> {
    let imm = ((w & 0x03ff_ffff) as i32) << 6 >> 4;
    usize::try_from(at as i64 + imm as i64).ok()
}

/// The f32 an `adrp xN` at `at` followed by `ldr sM, [xN, #imm]` loads, in `text` at `base` (data reached through
/// guest memory).
unsafe fn adrp_ldr_f32(base: usize, text: &[u8], at: usize) -> Option<f32> {
    let (adrp, ldr) = (word(text, at), word(text, at + 4));
    if adrp & 0x9f00_0000 != 0x9000_0000 || ldr & 0xffc0_0000 != 0xbd40_0000 {
        return None;
    }
    let lo = ((adrp >> 29) & 3) as i64;
    let hi = (((adrp >> 5) & 0x7ffff) as i64) << 45 >> 43;
    let page = ((base + at) as i64 & !0xfff) + ((hi | lo) << 12);
    let addr = page + (((ldr >> 10) & 0xfff) as i64) * 4;
    Some(std::ptr::read(addr as *const f32))
}

/// Rewrites the call (main's `text`, loaded at `base`) to enter `camera_angles`. Nothing happens unless the code and
/// its constants are as expected.
#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    let text = std::slice::from_raw_parts(base as *const u8, mem_space.text_size);
    let Some(at) = find(text) else {
        return println!("[!] TotK: the camera's angle code was not found; mouse aiming steers by speed only");
    };
    let callee = callee_of(at, word(text, at)).unwrap();
    let (Some(offset), Some(scale)) =
        (adrp_ldr_f32(base, text, callee + CONSTANTS_AT), adrp_ldr_f32(base, text, callee + CONSTANTS_AT + 8))
    else {
        return println!("[!] TotK: unexpected code in the camera's angle function; mouse aiming steers by speed only");
    };
    if (scale - 180.0 / std::f32::consts::PI).abs() > 1e-3 || !offset.is_finite() {
        return println!("[!] TotK: unexpected constants in the camera's angle function ({offset}, {scale})");
    }
    OFFSET.store(offset.to_bits(), Ordering::Relaxed);
    SCALE.store(scale.to_bits(), Ordering::Relaxed);
    // Natively (and in an image made for a native engine, which must hold exactly that engine's patches: cpu/aot.rs
    // checks them page by page) the camera goes as a third argument: the site is a call, so nothing caller-saved is live.
    let native = crate::cpu::aot::native_rewrites();
    let target = if native {
        let tramp = mem_space.make_hle_trampoline(crate::hle!(camera_angles_native(_, _, _))) as usize;
        let block = mem_space.make_code_block(2) as usize;
        let Some(b) = branch(block + 4, tramp, false) else {
            return println!("[!] TotK: the camera hook is out of branch range");
        };
        std::ptr::write(block as *mut u32, mov_x(2, 19));
        std::ptr::write((block + 4) as *mut u32, b);
        crate::os::flush_icache(block, 8);
        block
    } else {
        mem_space.make_hle_trampoline(crate::hle!(camera_angles(_, _))) as usize
    };
    if !branch_to(base, at, target) {
        return println!("[!] TotK: the camera hook is out of branch range");
    }
    let pitch = install_decision(base, text, mem_space, native);
    install_aim(base, text, mem_space, native);
    crate::host::input::set_angle_sink(pitch);
    let what = if pitch { "the camera's yaw and pitch" } else { "the camera's yaw" };
    println!("[+] TotK: mouse movement turns {what} directly (main+{at:#x}, profile/totk/camera_angles.rs)");
}

/// Rewrites the camera-input test to ask `camera_input`: bl to it, then cbnz w0 to the test's own target.
#[cfg(nx_runtime)]
unsafe fn install_decision(
    base: usize,
    text: &[u8],
    mem_space: &mut crate::guest::loader::GuestMemorySpace,
    native: bool,
) -> bool {
    let Some(at) = find_decision(text) else {
        println!("[!] TotK: the camera's input test was not found; mouse pitch steers by speed only");
        return false;
    };
    let reg = (word(text, at) >> 5) & 31;
    STICK_REG.store(reg, Ordering::Relaxed);
    let bgt = word(text, at + 4);
    let target = if native {
        // camera_input_native(the compared register, the camera in x20).
        let tramp = mem_space.make_hle_trampoline(crate::hle!(camera_input_native(_, _))) as usize;
        preserving_block(mem_space, tramp, &[fmov_s(0, reg), mov_x(0, 20)], Keep::X0)
    } else {
        Some(mem_space.make_hle_trampoline(crate::hle!(camera_input())) as usize)
    };
    if !target.is_some_and(|t| branch_to(base, at, t)) {
        println!("[!] TotK: the camera input hook is out of branch range");
        return false;
    }
    // b.gt and cbnz w0 keep their 19-bit offset in the same bits.
    std::ptr::write((base + at + 4) as *mut u32, 0x3500_0000 | (bgt & 0x00ff_ffe0));
    crate::os::flush_icache(base + at, 8);
    true
}

/// A b / bl from `from` to `to`, if in range.
fn branch(from: usize, to: usize, link: bool) -> Option<u32> {
    let d = to as i64 - from as i64;
    (d.abs() < 1 << 27).then(|| if link { 0x9400_0000 } else { 0x1400_0000 } | ((d >> 2) as u32 & 0x03ff_ffff))
}

/// Rewrites one instruction at `at` to bl `tramp`; false if out of range.
#[cfg(nx_runtime)]
unsafe fn branch_to(base: usize, at: usize, tramp: usize) -> bool {
    let Some(bl) = branch(base + at, tramp, true) else { return false };
    std::ptr::write((base + at) as *mut u32, bl);
    crate::os::flush_icache(base + at, 4);
    true
}

/// mov xd, xn
fn mov_x(d: u32, n: u32) -> u32 {
    0xaa00_03e0 | (n << 16) | d
}

/// fmov sd, sn
fn fmov_s(d: u32, n: u32) -> u32 {
    0x1e20_4000 | (n << 5) | d
}

/// The result register a preserving block leaves as the call returned it.
#[derive(Clone, Copy, PartialEq)]
enum Keep {
    X0,
    V0,
}

/// Bytes a preserving block's frame takes: x29 / x30, x0-x17, q0-q7, q16-q31, the flags.
const FRAME: u32 = 0x230;

/// The code of a block that calls `tramp` (at `block + 4 * index of the bl`) from the middle of a guest function:
/// saves every caller-saved register and the flags, runs `setup` (moves of guest registers into the arguments), calls,
/// and restores everything but `keep`. Returns to the instruction after the rewritten one (x30, set by its bl).
fn preserving_code(setup: &[u32], call: impl FnOnce(usize) -> Option<u32>, keep: Keep) -> Option<Vec<u32>> {
    let stp_x = |a: u32, b: u32, off: u32| 0xa900_0000 | ((off / 8) << 15) | (b << 10) | (31 << 5) | a;
    let ldp_x = |a: u32, b: u32, off: u32| 0xa940_0000 | ((off / 8) << 15) | (b << 10) | (31 << 5) | a;
    let stp_q = |a: u32, b: u32, off: u32| 0xad00_0000 | ((off / 16) << 15) | (b << 10) | (31 << 5) | a;
    let ldp_q = |a: u32, b: u32, off: u32| 0xad40_0000 | ((off / 16) << 15) | (b << 10) | (31 << 5) | a;
    let ldr_x = |a: u32, off: u32| 0xf940_0000 | ((off / 8) << 10) | (31 << 5) | a;
    let ldr_q = |a: u32, off: u32| 0x3dc0_0000 | ((off / 16) << 10) | (31 << 5) | a;
    let qs: Vec<(u32, u32)> = (0..4).map(|k| (2 * k, 0xa0 + 0x20 * k)).chain((0..8).map(|k| (16 + 2 * k, 0x120 + 0x20 * k))).collect();
    let mut c = vec![0xd100_03ff | (FRAME << 10), stp_x(29, 30, 0)]; // sub sp, sp, #FRAME; stp x29, x30, [sp]
    c.extend((0..9).map(|k| stp_x(2 * k, 2 * k + 1, 0x10 + 0x10 * k)));
    c.extend(qs.iter().map(|&(q, off)| stp_q(q, q + 1, off)));
    c.extend([0xd53b_4209, 0xf900_0000 | ((0x220 / 8) << 10) | (31 << 5) | 9]); // mrs x9, nzcv; str x9, [sp, #0x220]
    c.extend_from_slice(setup);
    c.push(call(c.len())?);
    c.extend([ldr_x(9, 0x220), 0xd51b_4209]); // msr nzcv, x9
    for &(q, off) in &qs {
        c.push(if q == 0 && keep == Keep::V0 { ldr_q(1, off + 0x10) } else { ldp_q(q, q + 1, off) });
    }
    for k in 0..9 {
        c.push(if k == 0 && keep == Keep::X0 { ldr_x(1, 0x18) } else { ldp_x(2 * k, 2 * k + 1, 0x10 + 0x10 * k) });
    }
    c.extend([ldp_x(29, 30, 0), 0x9100_03ff | (FRAME << 10), 0xd65f_03c0]); // add sp, sp, #FRAME; ret
    Some(c)
}

/// A preserving block (preserving_code) in the trampoline pool calling `tramp`; its address.
#[cfg(nx_runtime)]
unsafe fn preserving_block(
    mem_space: &mut crate::guest::loader::GuestMemorySpace,
    tramp: usize,
    setup: &[u32],
    keep: Keep,
) -> Option<usize> {
    let len = preserving_code(setup, |_| Some(0), keep)?.len();
    let block = mem_space.make_code_block(len) as usize;
    let code = preserving_code(setup, |i| branch(block + 4 * i, tramp, true), keep)?;
    for (k, w) in code.iter().enumerate() {
        std::ptr::write((block as *mut u32).add(k), *w);
    }
    crate::os::flush_icache(block, 4 * code.len());
    Some(block)
}

/// Rewrites the bow aim camera's yaw and pitch steps to add the mouse's degrees where the game adds the stick's turn
/// (`aim_yaw`, `aim_pitch`). Both or neither: the mouse then goes through the stick while aiming.
#[cfg(nx_runtime)]
unsafe fn install_aim(base: usize, text: &[u8], mem_space: &mut crate::guest::loader::GuestMemorySpace, native: bool) {
    let (Some(input), Some(yaw), Some(pitch)) = (
        find_masked(text, &AIM_INPUT, AIM_INPUT_AT),
        find_masked(text, &AIM_YAW, AIM_YAW_AT),
        find_masked(text, &AIM_PITCH, AIM_PITCH_AT),
    ) else {
        return println!("[!] TotK: the bow's aim steps were not found; mouse aiming with a bow steers by speed");
    };
    let bne_at = input + 4 * (AIM_INPUT_BNE - AIM_INPUT_AT);
    let bne = word(text, bne_at);
    let (ti, ty, tp) = if native {
        // aim_input_native(s0, the camera in x19); aim_yaw(s0, s1); aim_pitch_native(s0, s1, s9).
        let ti = mem_space.make_hle_trampoline(crate::hle!(aim_input_native(_, _))) as usize;
        let ty = mem_space.make_hle_trampoline(crate::hle!(aim_yaw(_, _))) as usize;
        let tp = mem_space.make_hle_trampoline(crate::hle!(aim_pitch_native(_, _, _))) as usize;
        (
            preserving_block(mem_space, ti, &[mov_x(0, 19)], Keep::X0),
            preserving_block(mem_space, ty, &[], Keep::V0),
            preserving_block(mem_space, tp, &[fmov_s(2, 9)], Keep::V0),
        )
    } else {
        (
            Some(mem_space.make_hle_trampoline(crate::hle!(aim_input())) as usize),
            Some(mem_space.make_hle_trampoline(crate::hle!(aim_yaw(_, _))) as usize),
            Some(mem_space.make_hle_trampoline(crate::hle!(aim_pitch(_, _))) as usize),
        )
    };
    let (Some(ti), Some(ty), Some(tp)) = (ti, ty, tp) else {
        return println!("[!] TotK: the bow aim hooks are out of branch range");
    };
    if !branch_to(base, input, ti) || !branch_to(base, yaw, ty) || !branch_to(base, pitch, tp) {
        return println!("[!] TotK: the bow aim hooks are out of branch range");
    }
    // b.ne and cbnz w0 keep their 19-bit offset in the same bits.
    std::ptr::write((base + bne_at) as *mut u32, 0x3500_0000 | (bne & 0x00ff_ffe0));
    crate::os::flush_icache(base + bne_at, 4);
    AIM_HOOKED.store(true, Ordering::Release);
    println!("[+] TotK: mouse movement turns the bow's aim directly (main+{yaw:#x}, main+{pitch:#x})");
}

/// The bow's aim steps enter `aim_yaw` / `aim_pitch`.
static AIM_HOOKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The aim camera those steps last ran for (x19 of its turn function).
static AIMED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The aim camera's input test: the game's own (the stick's sideways value, s0, not 0) or mouse degrees waiting. Runs
/// every frame the aim camera turns, so it also marks the camera running for the pause rule.
pub unsafe extern "C" fn aim_input() -> u64 {
    let cpu = crate::lifted::current();
    if cpu.is_null() {
        return 0;
    }
    aim_input_native(f32::from_bits((*cpu).v[0] as u32), (*cpu).x[19] as usize)
}

/// aim_input with the stick's value (s0) and the aim camera (x19) as arguments (native preserving block).
pub unsafe extern "C" fn aim_input_native(stick: f32, camera: usize) -> u64 {
    AIMED.store(camera, Ordering::Release);
    crate::host::input::camera_ran();
    (stick != 0.0 || crate::host::input::mouse_angle_pending()) as u64
}

/// The aim yaw step: what the game computes (`yaw` + `turn`, the stick's turn) plus the mouse's sideways degrees.
pub unsafe extern "C" fn aim_yaw(turn: f32, yaw: f32) -> f32 {
    yaw + turn + AIM_YAW_SIGN * crate::host::input::take_mouse_angle(0)
}

/// The aim pitch step, in place of `fcsel s0, s0, s1, gt` after `fcmp s9, #0.0`: the turn the game picks (`s0` when s9 is
/// above 0, else `s1`, as gt is false for NaN too) plus the mouse's vertical degrees.
pub unsafe extern "C" fn aim_pitch(s0: f32, s1: f32) -> f32 {
    let cpu = crate::lifted::current();
    let s9 = if cpu.is_null() { 0.0 } else { f32::from_bits((*cpu).v[9] as u32) };
    aim_pitch_native(s0, s1, s9)
}

/// aim_pitch with s9 as an argument (native preserving block).
pub unsafe extern "C" fn aim_pitch_native(s0: f32, s1: f32, s9: f32) -> f32 {
    let turn = if s9 > 0.0 { s0 } else { s1 };
    turn + AIM_PITCH_SIGN * crate::host::input::take_mouse_angle(1)
}

/// How the aim camera's angles turn for the normal camera's: measured against the stick (see the commit adding them).
const AIM_YAW_SIGN: f32 = 1.0;
const AIM_PITCH_SIGN: f32 = 1.0;

/// The camera-input test: the game's own (s14, the right stick's strength, above 0) or mouse movement waiting.
pub unsafe extern "C" fn camera_input() -> u64 {
    let cpu = crate::lifted::current();
    if cpu.is_null() {
        return 0;
    }
    let reg = STICK_REG.load(Ordering::Relaxed) as usize;
    camera_input_native(f32::from_bits((*cpu).v[reg] as u32), (*cpu).x[20] as usize)
}

/// camera_input with the compared register's value and the camera (x20 of the turn function) as arguments (native
/// preserving block).
pub unsafe extern "C" fn camera_input_native(stick: f32, camera: usize) -> u64 {
    let stick = stick > 0.0;
    // The camera this code runs for: the one the mouse's angles are for.
    STEERED.store(camera, Ordering::Release);
    let mouse = crate::host::input::take_mouse_input();
    if mouse {
        SEEN.store(true, Ordering::Release);
    }
    (stick || mouse) as u64
}

/// The camera object the turn function with the input test runs for (0: not seen yet). Other cameras (the bow's aim)
/// store their angles through the same function but are not placed from them: the mouse goes through the stick then.
static STEERED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The game has tested for camera input since mouse movement arrived (its answer is in the camera's mode).
static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// To [-180, 180), as the game's code wraps them.
fn wrap(a: f32) -> f32 {
    if (-180.0..180.0).contains(&a) {
        a
    } else {
        a - 360.0 * ((a + 180.0) / 360.0).floor()
    }
}

/// The distance, pitch and yaw of `v` (x, y, z: the camera's position minus the point it looks at) into `out`, as the
/// game's function computes them; yaw and pitch also go to mouse aiming.
pub unsafe extern "C" fn camera_angles(out: *mut f32, v: *const f32) -> usize {
    let cpu = crate::lifted::current();
    let camera = if cpu.is_null() { 0 } else { (*cpu).x[19] as usize };
    camera_angles_native(out, v, camera)
}

/// camera_angles with the camera storing them (x19 of the caller; 0: unknown) as an argument (native: a block moves it
/// into x2).
pub unsafe extern "C" fn camera_angles_native(out: *mut f32, v: *const f32, camera: usize) -> usize {
    let (x, y, z) = (*v, *v.add(1), *v.add(2));
    let flat = (x * x) as f64 + (z * z) as f64;
    let all = flat + (y * y) as f64;
    let h = if flat > 0.0 { flat.sqrt() as f32 } else { 0.0 };
    let dist = if all > 0.0 { all.sqrt() as f32 } else { 0.0 };
    let (offset, scale) = (f32::from_bits(OFFSET.load(Ordering::Relaxed)), f32::from_bits(SCALE.load(Ordering::Relaxed)));
    let pitch = wrap((offset - h.atan2(y)) * scale);
    // The game builds the next frame's camera from this yaw (the turn function main+0x9ab570 loads it first and places
    // the camera from it): sideways mouse movement is added here.
    // Which camera stores its angles: the steered one takes the mouse here; the bow's aim camera takes it in its own
    // steps (aim_yaw, aim_pitch) when they are hooked; any other turns by the stick.
    let mut here = true;
    if camera != 0 {
        let steered = STEERED.load(Ordering::Acquire);
        let aim = AIM_HOOKED.load(Ordering::Acquire) && camera == AIMED.load(Ordering::Acquire);
        here = steered == 0 || camera == steered;
        crate::host::input::set_angle_route(here || aim);
    }
    if !here {
        *out = dist;
        *out.add(1) = pitch;
        *out.add(2) = wrap(x.atan2(z) * scale);
        return out as usize;
    }
    let yaw = wrap(x.atan2(z) * scale + crate::host::input::take_mouse_angle(0));
    // The pitch state (camera +0x108), in manual mode only.
    // In manual mode added; else kept until the game has tested the input, and dropped if it then stayed out of manual
    // mode (as a stick would turn nothing there).
    let dp = crate::host::input::take_mouse_angle(1);
    if dp != 0.0 && camera != 0 {
        if std::ptr::read((camera + MODE) as *const u32) == MANUAL {
            let p = (camera + PITCH) as *mut f32;
            std::ptr::write(p, wrap(std::ptr::read(p) + dp));
            SEEN.store(false, Ordering::Release);
        } else if !SEEN.swap(false, Ordering::AcqRel) {
            crate::host::input::keep_mouse_angle(1, dp);
        }
    }
    *out = dist;
    *out.add(1) = pitch;
    *out.add(2) = yaw;
    out as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_with(at: usize, callee: usize) -> Vec<u8> {
        let mut t = vec![0u8; callee + 0x200];
        let put = |t: &mut Vec<u8>, a: usize, w: u32| t[a..a + 4].copy_from_slice(&w.to_le_bytes());
        for (k, &(w, _)) in AROUND.iter().enumerate() {
            let a = at - 4 * CALL + 4 * k;
            let w = if k == CALL { w | (((callee - at) / 4) as u32 & 0x03ff_ffff) } else { w };
            put(&mut t, a, w);
        }
        for (k, &w) in CALLEE.iter().enumerate() {
            put(&mut t, callee + 4 * k, w);
        }
        t
    }

    #[test]
    fn finds_the_call_by_its_code() {
        let t = text_with(0x40, 0x100);
        assert_eq!(find(&t), Some(0x40));
        assert_eq!(callee_of(0x40, word(&t, 0x40)), Some(0x100));
        // Another function behind the call: not this one.
        let mut other = t.clone();
        other[0x100] ^= 1;
        assert_eq!(find(&other), None);
        // Found twice: ambiguous.
        let mut twice = text_with(0x40, 0x180);
        let first = text_with(0x40, 0x180);
        twice[0x80 - 12..0x80 + 12].copy_from_slice(&first[0x40 - 12..0x40 + 12]);
        assert_eq!(find(&twice), None);
    }

    /// The words around each site in 1.1.2 (main+0xbbc4bc, 0xa339fc, 0x1d35e34, 0x1d36248): the patterns written from
    /// 1.0.0 find them.
    #[test]
    fn finds_the_1_1_2_sites() {
        let put = |words: &[u32]| words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>();
        let around = [0x2d3e_0ba0, 0x1e23_3820, 0xbc1f_83a0, 0x9400_006a, 0xf940_07e9, 0x9110_32a8];
        let t = put(&around);
        assert!(AROUND.iter().zip(around).all(|(&(w, m), v)| v & m == w));
        let decision = put(&[0x3945_2768, 0x3728_bca8, 0x1e20_2168, 0x5400_f38c]);
        assert_eq!(find_decision(&[&[0u8; 8][..], &decision].concat()), Some(16));
        assert_eq!((word(&decision, 8) >> 5) & 31, 11);
        let input = put(&[0xbd44_22c0, 0xbd40_0a8a, 0xf940_0109, 0xb940_0908, 0x1e20_2008, 0xf81b_03a9, 0xb81b_83a8, 0x5400_03a1]);
        assert_eq!(find_masked(&[&input[..], &[0u8; 8]].concat(), &AIM_INPUT, AIM_INPUT_AT), Some(16));
        let yaw = put(&[0xbd4e_66a1, 0x1e21_0800, 0xbc5b_83a1, 0x1e20_2820, 0x97ba_1a6c, 0xf940_b2c8, 0x528a_0109]);
        assert_eq!(find_masked(&[&yaw[..], &[0u8; 8]].concat(), &AIM_YAW, AIM_YAW_AT), Some(12));
        let _ = t;
    }

    #[test]
    fn preserving_block_saves_all_but_the_result() {
        let code = preserving_code(&[fmov_s(0, 11), mov_x(0, 20)], |i| Some(0x9400_0000 | i as u32), Keep::X0).unwrap();
        assert_eq!(code[0], 0xd108_c3ff); // sub sp, sp, #0x230
        assert_eq!(code[1], 0xa900_7bfd); // stp x29, x30, [sp]
        assert!(code.contains(&0xa909_47f0)); // stp x16, x17, [sp, #0x90]
        assert!(code.contains(&0xad10_7ffe)); // stp q30, q31, [sp, #0x200]
        assert!(code.contains(&0xd53b_4209) && code.contains(&0xd51b_4209)); // the flags
        let bl = code.iter().position(|&w| w & 0xfc00_0000 == 0x9400_0000).unwrap();
        assert_eq!(code[bl] & 0x03ff_ffff, bl as u32); // the call is told its own index
        assert_eq!(&code[bl - 2..bl], &[0x1e20_4160, 0xaa14_03e0]); // fmov s0, s11; mov x0, x20
        assert!(code.contains(&0xf940_0fe1) && !code.contains(&0xa941_07e0)); // ldr x1, not ldp x0, x1
        assert!(code.contains(&0xad45_07e0)); // q0 / q1 restored
        assert_eq!(&code[code.len() - 3..], &[0xa940_7bfd, 0x9108_c3ff, 0xd65f_03c0]);
        let v0 = preserving_code(&[], |_| Some(0x9400_0000), Keep::V0).unwrap();
        assert!(v0.contains(&0x3dc0_2fe1) && !v0.contains(&0xad45_07e0)); // ldr q1, not ldp q0, q1
        assert!(v0.contains(&0xa941_07e0)); // x0 / x1 restored
    }

    #[test]
    fn finds_the_camera_input_test() {
        // 1.0.0's words: ldrb w8, [x27, #0x141]; tbnz w8, #5, +0x1774; fcmp s14, #0.0; b.gt +0x1e48.
        let words = [0x3945_0768u32, 0x3728_bb88, 0x1e20_21c8, 0x5400_f22c];
        let mut text = vec![0u8; 0x40];
        for (k, w) in words.iter().enumerate() {
            text[0x10 + 4 * k..0x14 + 4 * k].copy_from_slice(&w.to_le_bytes());
        }
        assert_eq!(find_decision(&text), Some(0x18));
        // The cbnz written in place of b.gt branches to the same target: the same 19-bit offset.
        let cbnz = 0x3500_0000 | (words[3] & 0x00ff_ffe0);
        assert_eq!(cbnz, 0x3500_f220);
        assert_eq!((cbnz >> 5) & 0x7ffff, (words[3] >> 5) & 0x7ffff);
        text[0x18] ^= 1;
        assert_eq!(find_decision(&text), None);
    }

    #[test]
    fn finds_the_bow_aim_steps() {
        // 1.0.0's words at main+0x1cd3168 and main+0x1cd2efc.
        let yaw = [0xbd4f_66a1u32, 0x1e21_0800, 0xbc5b_83a1, 0x1e20_2820, 0x97ba_dd78, 0xf940_aec8, 0x5285_0109];
        let pitch = [0xbd40_63e0u32, 0x1e20_2128, 0x9105_7260, 0x1e21_4001, 0x1e21_cc00, 0x97fa_481f];
        let mut text = vec![0u8; 0x100];
        for (k, w) in yaw.iter().enumerate() {
            text[0x20 + 4 * k..0x24 + 4 * k].copy_from_slice(&w.to_le_bytes());
        }
        for (k, w) in pitch.iter().enumerate() {
            text[0x80 + 4 * k..0x84 + 4 * k].copy_from_slice(&w.to_le_bytes());
        }
        assert_eq!(find_masked(&text, &AIM_YAW, AIM_YAW_AT), Some(0x2c));
        assert_eq!(find_masked(&text, &AIM_INPUT, AIM_INPUT_AT), None);
        assert_eq!(find_masked(&text, &AIM_PITCH, AIM_PITCH_AT), Some(0x90));
        // Twice: ambiguous.
        let copy: Vec<u8> = text[0x80..0x98].to_vec();
        text[0xc0..0xd8].copy_from_slice(&copy);
        assert_eq!(find_masked(&text, &AIM_PITCH, AIM_PITCH_AT), None);
    }

    #[test]
    fn angles_wrap_like_the_game() {
        assert_eq!(wrap(179.5), 179.5);
        assert_eq!(wrap(180.0), -180.0);
        assert!((wrap(-190.0) - 170.0).abs() < 1e-4);
        assert!((wrap(540.0) - -180.0).abs() < 1e-4);
    }
}
