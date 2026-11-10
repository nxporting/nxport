// Screen shapes other than 16:9 in Tears of the Kingdom (1.0.0 and 1.1.2: the same code at other addresses, `Sites`).
//
// The game sizes its view from a 16:9 constant and from width / height pairs it computes. With an aspect set
// (`set_aspect`: the launch shape, then the window's as it is resized) the view takes that shape: wider than 16:9 widths
// grow by (aspect / 16:9), narrower heights by (16:9 / aspect), and narrower shapes keep 16:9's horizontal field of view
// and see more vertically (Vert+: a projection at the screen's aspect has tan(fovy / 2) scaled by 16:9 / aspect, and
// sin, cos and fovy derived from it, so the matrix, culling and shadows all see the taller view).
// - The constant (rodata) takes the launch shape when the setting is on at startup: cameras are created at it.
// - Two places set a projection's aspect to its view rectangle's width / height: the screen's aspect instead.
// - Cameras copy their aspect into their projection every update: a camera still at the launch aspect gets the current
//   one (a camera keeps the aspect it was created with), then Vert+.
// - sead::PerspectiveProjection::setFovy_ (projections set up from a field of view each frame, such as the inventory's
//   Link): Vert+.
// - Five places put a HUD pane at a point in 16:9 layout units: the point is scaled (x wider, y narrower).
// - Every layout calculation first adjusts the layout's panes (hud.rs).
// Projections the game stops updating (all of them while a menu is open) keep the aspect of their last update: the hooks
// record the projections they see, and set_aspect gives those still at the previous aspect the new one.
//
// The hooks rewrite instructions in place and call HLE functions (no code of our own in guest memory), so they apply to
// every runtime: natively, and to the image that is lifted. They are installed whenever the game is TotK and keep the
// game's own behaviour until an aspect is set, so the setting can be switched while the game runs. Where a call
// replaces code that keeps values in caller-saved registers, the rewritten code reloads them (native calls may clobber
// them; lifted ones keep them).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// Where one version's code is (offsets in main). Each site is checked against the code it expects before any is
/// written; the expected code is the same in both versions apart from pc-relative immediates.
pub struct Sites {
    /// The game's display aspect (rodata): 16 / 9, or what a 19.5:9 widescreen mod writes there.
    pub aspect_const: usize,
    /// `fdiv s0, s0, s1; strb w9, [x8, #8]; str s0, [x8, #0xb0]`: the projection in x8 takes its view rectangle's
    /// width / height.
    pub ratio: [usize; 2],
    /// A pane shown at a point the game projects to layout units: the check `tbnz w9, #0` of the pane's visibility,
    /// then where the code continues once the pane is placed.
    pub show: [(usize, usize); 2],
    /// A pane moved to a point its object keeps (`ldp s0, s1, [x20, #0x20]`, stored as the pane's translation).
    pub moved: usize,
    /// Points rounded down to whole layout units before a pane takes them: `stp w11, w12, [sp, #4]`, then 21
    /// instructions from `ldp s0, s1, [x8]`; and 23 from `ldp s0, s1, [x8]` with other registers.
    pub floor: [usize; 2],
    /// Cameras copying their aspect into their projection: x20's, then x23's (with an adrp before it).
    pub camera: [usize; 2],
    /// sead::PerspectiveProjection::setFovy_ storing tan(fovy / 2): `mov w8, #1` before it.
    pub fovy: usize,
    /// Layout calculations after their prologue (`ldr x8, [x0, #0x18]`): the library's, and the game's layout class's.
    pub layout: [usize; 2],
}

const ASPECT_GAME: u32 = 0x3fe3_8e3b;
const ASPECT_MOD: u32 = 0x400a_aaab;
const SIXTEEN_NINTHS: f32 = 16.0 / 9.0;
/// The narrowest shape (9:21).
const NARROWEST: f32 = 9.0 / 21.0;

// sead::PerspectiveProjection: the matrix dirty flag +8, fovy +0xa0, sin / cos / tan of fovy / 2 +0xa4 / +0xa8 / +0xac,
// aspect +0xb0.
const PROJ_DIRTY: usize = 8;
const PROJ_FOVY: usize = 0xa0;
const PROJ_ASPECT: usize = 0xb0;
// Pane fields (hud.rs): translation +0x30 (x, y, z), flags +0x58 (bit 0 visible, 0x10 matrix dirty).
const PANE_TRANSLATE: usize = 0x30;
const PANE_FLAGS: usize = 0x58;
// Layout: its root pane.
const LAYOUT_ROOT: usize = 0x18;

/// The aspect the game renders at (f32 bits); 0 until one is set: the hooks then keep the game's own behaviour.
static ASPECT: AtomicU32 = AtomicU32::new(0);
/// The aspect cameras are created at (the constant's value once installed, f32 bits).
static LAUNCH: AtomicU32 = AtomicU32::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Projections the hooks saw, and their vtables then (64 entries, by address bits 6..11): set_aspect updates those the
/// game stopped updating.
const PROJECTION_SLOTS: usize = 64;
static PROJECTIONS: [AtomicUsize; PROJECTION_SLOTS] = [const { AtomicUsize::new(0) }; PROJECTION_SLOTS];
static VTABLES: [AtomicUsize; PROJECTION_SLOTS] = [const { AtomicUsize::new(0) }; PROJECTION_SLOTS];

/// The aspect the game renders at, once one is set.
pub fn rendered_aspect() -> Option<f64> {
    let bits = ASPECT.load(Ordering::Relaxed);
    (bits != 0).then(|| f32::from_bits(bits) as f64)
}

/// Changes the aspect the game renders at (the hooks follow it from their next call). No-op if not installed.
pub fn set_aspect(aspect: f64) {
    if !INSTALLED.load(Ordering::Relaxed) {
        return;
    }
    let a = (aspect as f32).max(NARROWEST).to_bits();
    let old = match ASPECT.swap(a, Ordering::Relaxed) {
        0 => LAUNCH.load(Ordering::Relaxed),
        old => old,
    };
    if old == a {
        return;
    }
    for (p, vt) in PROJECTIONS.iter().zip(&VTABLES) {
        let p = p.load(Ordering::Relaxed);
        unsafe {
            if p != 0
                && readable(p, PROJ_ASPECT + 4)
                && *(p as *const usize) == vt.load(Ordering::Relaxed)
                && (*((p + PROJ_ASPECT) as *const AtomicU32))
                    .compare_exchange(old, a, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                *((p + PROJ_DIRTY) as *mut u8) = 1;
            }
        }
    }
}

fn readable(p: usize, len: usize) -> bool {
    #[cfg(nx_native)]
    return unsafe { crate::cpu::crash_handler::readable_len(p, len) } == len;
    #[cfg(windows)]
    return crate::os::readable_len(p, len) == len;
    #[cfg(not(any(nx_native, windows)))]
    {
        let _ = (p, len);
        false
    }
}

/// (width factor, height factor): max(aspect / 16:9, 1) and max(16:9 / aspect, 1); 1 for the game's own shape.
fn factors() -> (f32, f32) {
    match ASPECT.load(Ordering::Relaxed) {
        0 => (1.0, 1.0),
        a => {
            let a = f32::from_bits(a);
            ((a / SIXTEEN_NINTHS).max(1.0), (SIXTEEN_NINTHS / a).max(1.0))
        }
    }
}

fn record(p: usize) {
    let i = (p >> 6) % PROJECTION_SLOTS;
    // The vtable first: set_aspect pairs an entry with the vtable stored before it.
    VTABLES[i].store(unsafe { *(p as *const usize) }, Ordering::Relaxed);
    PROJECTIONS[i].store(p, Ordering::Relaxed);
}

/// Vert+ for projection `p` when its aspect is the screen's and narrower than 16:9: tan(fovy / 2) times 16:9 / aspect,
/// and sin, cos and fovy derived from it. Other projections keep the game's values.
unsafe fn vert_plus(p: usize) {
    let a = ASPECT.load(Ordering::Relaxed);
    if a == 0 || f32::from_bits(a) >= SIXTEEN_NINTHS || *((p + PROJ_ASPECT) as *const u32) != a {
        return;
    }
    let f = (p + PROJ_FOVY) as *mut f32;
    let tan = *f.add(3) * (SIXTEEN_NINTHS / f32::from_bits(a));
    let cos = 1.0 / (1.0 + tan * tan).sqrt();
    *f = 2.0 * tan.atan();
    *f.add(1) = tan * cos;
    *f.add(2) = cos;
    *f.add(3) = tan;
}

/// `fdiv s0, s0, s1` and the projection in x8 taking the result: the screen's aspect once set, else width / height.
/// Returns `this` (x0, kept for the caller).
pub unsafe extern "C" fn view_ratio(this: usize, proj: usize, w: f32, h: f32) -> usize {
    let r = match ASPECT.load(Ordering::Relaxed) {
        0 => w / h,
        a => f32::from_bits(a),
    };
    *((proj + PROJ_DIRTY) as *mut u8) = 1;
    *((proj + PROJ_ASPECT) as *mut f32) = r;
    record(proj);
    this
}

/// A camera copying its aspect (at `cam_aspect`) into its projection: one still at the launch aspect gets the screen's,
/// then Vert+.
pub unsafe extern "C" fn camera_aspect(proj: usize, cam_aspect: *const f32) {
    let mut v = *cam_aspect;
    let a = ASPECT.load(Ordering::Relaxed);
    if a != 0 && v.to_bits() == LAUNCH.load(Ordering::Relaxed) {
        v = f32::from_bits(a);
    }
    *((proj + PROJ_DIRTY) as *mut u8) = 1;
    *((proj + PROJ_ASPECT) as *mut f32) = v;
    record(proj);
    vert_plus(proj);
}

/// The end of setFovy_ (tan stored): the matrix marked dirty, then Vert+.
pub unsafe extern "C" fn fovy_set(proj: usize) {
    *((proj + PROJ_DIRTY) as *mut u8) = 1;
    vert_plus(proj);
}

/// Puts `pane` at the point at `src` (x, y in 16:9 layout units, scaled to the screen's shape) and marks its matrix
/// dirty; returns the y it took.
unsafe fn place(pane: usize, src: *const f32) -> f32 {
    let (fx, fy) = factors();
    let (x, y) = (*src * fx, *src.add(1) * fy);
    let t = (pane + PANE_TRANSLATE) as *mut f32;
    *t = x;
    *t.add(1) = y;
    *(t.add(2) as *mut u32) = 0;
    *((pane + PANE_FLAGS) as *mut u8) |= 0x10;
    y
}

/// A pane shown at a projected point: made visible, then placed. Returns the y (s0: one caller keeps it).
pub unsafe extern "C" fn pane_show_at(pane: usize, src: *const f32) -> f32 {
    *((pane + PANE_FLAGS) as *mut u8) |= 1;
    place(pane, src)
}

/// A pane moved to a point its object keeps.
pub unsafe extern "C" fn pane_move_to(pane: usize, src: *const f32) -> f32 {
    place(pane, src)
}

/// The point at `src`, scaled, each coordinate rounded down as the game's code does (fcvtzs, less one when negative and
/// not whole). Returned in s0, s1.
pub unsafe extern "C" fn floor_point(src: *const f32) -> crate::hle_abi::F32Pair {
    let floor = |v: f32| {
        let t = v as i32;
        t.wrapping_sub((v < 0.0 && t as f32 != v) as i32) as f32
    };
    let (fx, fy) = factors();
    crate::hle_abi::F32Pair(floor(*src * fx), floor(*src.add(1) * fy))
}

/// A layout calculation's start: hud.rs adjusts the layout's panes. Returns its root pane (the game's code checks it
/// next) and the calculation's second argument (x1, which the code passes on after the call).
pub unsafe extern "C" fn layout_calculate(layout: usize, arg: u64) -> crate::hle_abi::U64Pair {
    super::hud::calculate(layout);
    crate::hle_abi::U64Pair(*((layout + LAYOUT_ROOT) as *const usize) as u64, arg)
}

/// What a rewritten site holds: instructions, and one `bl` to the site's HLE trampoline.
#[cfg(nx_runtime)]
#[derive(Clone, Copy)]
enum Op {
    W(u32),
    Call,
}

const NOP: u32 = 0xd503_201f;
/// Any word (checked otherwise, or not at all).
const ANY: u32 = 0;

/// `w` with its pc-relative immediate cleared (adrp, b / bl, b.cond, cbz / cbnz, tbz / tbnz): what the versions share.
fn norm(w: u32) -> u32 {
    if w & 0x7c00_0000 == 0x1400_0000 {
        w & 0xfc00_0000
    } else if w & 0xff00_0010 == 0x5400_0000 || w & 0x7e00_0000 == 0x3400_0000 {
        w & 0xff00_001f
    } else if w & 0x7e00_0000 == 0x3600_0000 {
        w & 0xfff8_001f
    } else if w & 0x9f00_0000 == 0x9000_0000 {
        w & 0x9f00_001f
    } else {
        w
    }
}

/// `w` (at main offset `from`) moved to `to`: its pc-relative immediate (adrp page, cbz / cbnz) re-encoded.
fn relocate(w: u32, from: usize, to: usize) -> Option<u32> {
    let sext = |v: u32, bits: u32| ((v as i64) << (64 - bits)) >> (64 - bits);
    if w & 0x9f00_0000 == 0x9000_0000 {
        let pages = sext(((w >> 5) & 0x7ffff) << 2 | (w >> 29) & 3, 21);
        let page = (from as i64 & !0xfff) + (pages << 12);
        let d = (page - (to as i64 & !0xfff)) >> 12;
        (d.abs() < 1 << 20).then(|| w & 0x9f00_001f | ((d as u32 & 3) << 29) | (((d >> 2) as u32 & 0x7ffff) << 5))
    } else if w & 0x7e00_0000 == 0x3400_0000 {
        let target = from as i64 + sext((w >> 5) & 0x7ffff, 19) * 4;
        let d = (target - to as i64) / 4;
        (d.abs() < 1 << 18).then(|| w & 0xff00_001f | ((d as u32 & 0x7ffff) << 5))
    } else {
        Some(w)
    }
}

fn b(from: usize, to: usize) -> u32 {
    0x1400_0000 | (((to as i64 - from as i64) >> 2) as u32 & 0x03ff_ffff)
}

/// `add xD, xN, #imm` (`sub` for a negative one); xN 31 is sp.
fn add_imm(d: u32, n: u32, imm: i64) -> u32 {
    let op = if imm < 0 { 0xd100_0000 } else { 0x9100_0000 };
    op | ((imm.unsigned_abs() as u32 & 0xfff) << 10) | (n << 5) | d
}

fn mov_x(d: u32, m: u32) -> u32 {
    0xaa00_03e0 | (m << 16) | d
}

// The code each site holds (1.1.2's words; pc-relative immediates are not compared).
const RATIO_CODE: [u32; 3] = [
    0x1e21_1800, // fdiv s0, s0, s1
    0x3900_2109, // strb w9, [x8, #8]
    0xbd00_b100, // str s0, [x8, #0xb0]
];
const SHOW_CODE: [u32; 6] = [
    0x3700_00a9, // tbnz w9, #0, +0x14 (visible: straight to the ldp)
    0x3200_0129, // orr w9, w9, #1
    0x3901_6109, // strb w9, [x8, #0x58]
    ANY,         // ldr x8, [x19, #<the pane>]
    0x3941_6109, // ldrb w9, [x8, #0x58]
    ANY,         // ldp sX, sY, [xN, #off]: the point
];
const MOVED_CODE: [u32; 7] = [
    0x2d44_0680, // ldp s0, s1, [x20, #0x20]
    0xbd00_3501, // str s1, [x8, #0x34]
    0x3941_6109, // ldrb w9, [x8, #0x58]
    0xb900_391f, // str wzr, [x8, #0x38]
    0xbd00_3100, // str s0, [x8, #0x30]
    0x321c_0129, // orr w9, w9, #0x10
    0x3901_6109, // strb w9, [x8, #0x58]
];
/// floor(x), floor(y) of the point at x8 into s0, s1 (w8..w10 and s2 as scratch).
const FLOOR_STEPS: [u32; 21] = [
    0x2d40_0500, 0x5ea1_b802, 0x1e38_0008, 0x5e21_d842, 0x1e22_2000, 0x5ea1_b822, 0x1a9f_07e9, 0x1e20_2008,
    0x5e21_d842, 0x1a9f_a7ea, 0x0a09_0149, 0x4b09_0108, 0x1e22_2020, 0x1e22_0100, 0x1a9f_07e9, 0x1e38_0028,
    0x1e20_2028, 0x1a9f_a7ea, 0x0a09_0149, 0x4b09_0108, 0x1e22_0101,
];
const STP_W11_W12: u32 = 0x2900_b3eb; // stp w11, w12, [sp, #4]
const LDR_X8_X8_X22: u32 = 0xf876_6908; // ldr x8, [x8, x22]
/// The same with w10..w12 as scratch, and the pane's owner loaded between (words 19 and 21).
const FLOOR_CODE_B: [u32; 23] = [
    0x2d40_0500, 0x5ea1_b802, 0x1e38_0008, 0x1e38_002a, 0x5e21_d842, 0x1e22_2000, 0x5ea1_b822, 0x1a9f_07e9,
    0x1e20_2008, 0x5e21_d842, 0x1a9f_a7eb, 0x0a09_0169, 0x4b09_0108, 0x1e22_2020, 0x1e22_0100, 0x1a9f_07ec,
    0x1e20_2028, 0x1a9f_a7eb, 0x0a0c_016b, 0xf940_028c, 0x4b0b_0149, 0xf940_0d88, 0x1e22_0121,
];
const CAMERA_CODE_A: [u32; 5] = [
    0x9105_1268, // add x8, x19, #0x144
    0x3900_2295, // strb w21, [x20, #8]
    0xbd41_4260, // ldr s0, [x19, #0x140]: the camera's aspect
    0x3900_2295, // strb w21, [x20, #8]
    0xbd00_b280, // str s0, [x20, #0xb0]
];
const CAMERA_CODE_B: [u32; 5] = [
    0x9000_0009, // adrp x9, <page> (for the code after)
    0xbd00_aee0, // str s0, [x23, #0xac]
    0xbd41_4260, // ldr s0, [x19, #0x140]
    0x3900_22f8, // strb w24, [x23, #8]
    0xbd00_b2e0, // str s0, [x23, #0xb0]
];
const FOVY_CODE: [u32; 3] = [
    0x5280_0028, // mov w8, #1
    0xbd00_ae60, // str s0, [x19, #0xac]
    0x3900_2268, // strb w8, [x19, #8]
];
const LAYOUT_CODE_A: [u32; 9] = [
    0xf940_0c08, // ldr x8, [x0, #0x18]
    0xb400_0008, // cbz x8, <return>
    0xaa00_03f5, // mov x21, x0
    0x2a02_03f4, // mov w20, w2
    0x3900_a3ff, // strb wzr, [sp, #0x28]
    0x9100_03e0, // mov x0, sp
    0xaa15_03e2, // mov x2, x21
    0xf900_03ff, // str xzr, [sp]
    0xaa01_03f3, // mov x19, x1
];
const LAYOUT_CODE_B: [u32; 7] = [
    0xf940_0c08, // ldr x8, [x0, #0x18]
    0xb400_0008, // cbz x8, <return>
    0xaa00_03f4, // mov x20, x0
    0x2a02_03f5, // mov w21, w2
    0x9100_23e0, // add x0, sp, #8
    0xaa14_03e2, // mov x2, x20
    0xaa01_03f3, // mov x19, x1
];

/// The code at main offset `at` against `expected` (ANY: not compared).
#[cfg(nx_runtime)]
unsafe fn check(base: usize, at: usize, expected: &[u32]) -> Result<Vec<u32>, String> {
    let words: Vec<u32> = (0..expected.len()).map(|k| std::ptr::read((base + at + 4 * k) as *const u32)).collect();
    match words.iter().zip(expected).position(|(w, e)| *e != ANY && norm(*w) != norm(*e)) {
        Some(k) => Err(format!("unexpected instruction {:#010x} at main+{:#x}", words[k], at + 4 * k)),
        None => Ok(words),
    }
}

/// One rewritten site: its code, and the HLE function its call enters.
#[cfg(nx_runtime)]
struct Patch {
    at: usize,
    code: Vec<Op>,
    hle: *const (),
}

/// Every site's rewrite, each checked against the code it expects first.
#[cfg(nx_runtime)]
unsafe fn plan(base: usize, s: &Sites) -> Result<Vec<Patch>, String> {
    use Op::{Call, W};
    let mut patches = Vec::new();
    for at in s.ratio {
        check(base, at, &RATIO_CODE)?;
        // mov x1, x8 (the projection); bl view_ratio(x0, x1, s0, s1) -> x0.
        patches.push(Patch { at, code: vec![W(mov_x(1, 8)), Call, W(NOP)], hle: crate::hle!(view_ratio(_, _, _, _)) });
    }
    for (at, after) in s.show {
        let w = check(base, at, &SHOW_CODE)?;
        // ldr x8, [x19, #imm] and ldp sX, sY, [xN, #off] (signed offset).
        if w[3] & 0xffc0_03ff != 0xf940_0268 || w[5] & 0xffc0_0000 != 0x2d40_0000 {
            return Err(format!("unexpected code at main+{at:#x}"));
        }
        let (n, off) = ((w[5] >> 5) & 31, (((w[5] >> 15) & 0x7f) as i64) << 57 >> 55);
        // The pane, the point's address; bl pane_show_at; on to where the game's code continues.
        let code = vec![W(mov_x(0, 8)), W(add_imm(1, n, off)), Call, W(b(at + 12, after))];
        patches.push(Patch { at, code, hle: crate::hle!(pane_show_at(_, _)) });
    }
    {
        let at = s.moved;
        check(base, at, &MOVED_CODE)?;
        let mut code = vec![W(mov_x(0, 8)), W(add_imm(1, 20, 0x20)), Call];
        code.resize(MOVED_CODE.len(), W(NOP));
        patches.push(Patch { at, code, hle: crate::hle!(pane_move_to(_, _)) });
    }
    {
        let at = s.floor[0];
        let mut expected = vec![STP_W11_W12, LDR_X8_X8_X22];
        expected.extend(FLOOR_STEPS);
        check(base, at, &expected)?;
        // The point's rounding replaced by a call; w11 and w12, live after it, back from where the code just stored them
        // (ldp of the stp).
        let mut code = vec![W(STP_W11_W12), W(LDR_X8_X8_X22), W(mov_x(0, 8)), Call, W(STP_W11_W12 | 0x0040_0000)];
        code.resize(expected.len(), W(NOP));
        patches.push(Patch { at, code, hle: crate::hle!(floor_point(_)) });
    }
    {
        let at = s.floor[1];
        let w = check(base, at, &FLOOR_CODE_B)?;
        // Then the loads of the pane's owner (ldr x12, [x20]; ldr x8, [x12, #0x18]) the code interleaved.
        let mut code = vec![W(mov_x(0, 8)), Call, W(w[19]), W(w[21])];
        code.resize(FLOOR_CODE_B.len(), W(NOP));
        patches.push(Patch { at, code, hle: crate::hle!(floor_point(_)) });
    }
    {
        let at = s.camera[0];
        let w = check(base, at, &CAMERA_CODE_A)?;
        // camera_aspect(x20, &[x19 + 0x140]); then x8 = x19 + 0x144 for the code after.
        let code = vec![W(mov_x(0, 20)), W(add_imm(1, 19, 0x140)), Call, W(w[0]), W(NOP)];
        patches.push(Patch { at, code, hle: crate::hle!(camera_aspect(_, _)) });
    }
    {
        let at = s.camera[1];
        let w = check(base, at, &CAMERA_CODE_B)?;
        // tan stored first; camera_aspect(x23, &[x19 + 0x140]); the adrp after the call (x9 is caller-saved).
        let adrp = relocate(w[0], base + at, base + at + 16).ok_or_else(|| format!("main+{at:#x}: adrp out of range"))?;
        let code = vec![W(w[1]), W(mov_x(0, 23)), W(add_imm(1, 19, 0x140)), Call, W(adrp)];
        patches.push(Patch { at, code, hle: crate::hle!(camera_aspect(_, _)) });
    }
    {
        let at = s.fovy;
        let w = check(base, at, &FOVY_CODE)?;
        let code = vec![W(w[1]), W(mov_x(0, 19)), Call];
        patches.push(Patch { at, code, hle: crate::hle!(fovy_set(_)) });
    }
    // The arguments into the callee-saved registers the prologue freed, layout_calculate(x0, x1) -> (its root pane, x1),
    // then the game's check of the root (cbz x0 now) and the rest of the code it displaced.
    let cbz_x0 = |w: &[u32], at: usize, to: usize| {
        relocate(w[1] & !31, base + at + 4, base + at + 4 * to).ok_or_else(|| format!("main+{at:#x}: cbz out of range"))
    };
    {
        let at = s.layout[0];
        let w = check(base, at, &LAYOUT_CODE_A)?;
        let code = vec![W(w[2]), W(w[3]), W(w[8]), Call, W(cbz_x0(&w, at, 4)?), W(w[4]), W(w[5]), W(w[6]), W(w[7])];
        patches.push(Patch { at, code, hle: crate::hle!(layout_calculate(_, _)) });
    }
    {
        let at = s.layout[1];
        let w = check(base, at, &LAYOUT_CODE_B)?;
        let code = vec![W(w[2]), W(w[3]), W(w[6]), Call, W(cbz_x0(&w, at, 4)?), W(w[4]), W(w[5])];
        patches.push(Patch { at, code, hle: crate::hle!(layout_calculate(_, _)) });
    }
    Ok(patches)
}

/// Installs the hooks (all or none) and, with the dynamic aspect ratio setting on, starts at the launch aspect
/// (--launch-aspect, else 16:9). With it off the game keeps its own shape until set_aspect is called.
#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, s: &Sites) {
    let patches = match plan(base, s) {
        Ok(p) => p,
        Err(e) => return println!("[!] Aspect: {e}; the picture stays 16:9"),
    };
    let c = std::ptr::read((base + s.aspect_const) as *const u32);
    if c != ASPECT_GAME && c != ASPECT_MOD {
        return println!("[!] Aspect: unexpected aspect constant {c:#010x}; the picture stays 16:9");
    }
    for p in &patches {
        let site = base + p.at;
        // A trampoline per site, made whether or not its branch reaches (the same trampolines at lift and run time).
        let tramp = mem_space.make_hle_trampoline(p.hle) as usize;
        let k = p.code.iter().position(|o| matches!(o, Op::Call)).unwrap_or(0);
        let d = tramp as i64 - (site + 4 * k) as i64;
        if d.abs() >= 1 << 27 {
            return println!("[!] Aspect: the trampolines are out of branch range of main+{:#x}", p.at);
        }
        for (i, o) in p.code.iter().enumerate() {
            let w = match o {
                Op::W(w) => *w,
                Op::Call => 0x9400_0000 | ((d >> 2) as u32 & 0x03ff_ffff),
            };
            std::ptr::write((site + 4 * i) as *mut u32, w);
        }
        crate::os::flush_icache(site, 4 * p.code.len());
    }
    let settings = crate::settings::get();
    if settings.totk.enhancements.dynamic_aspect_ratio {
        let a = settings.display.launch_aspect;
        let start = (a.map_or(16.0 / 9.0, |a| a.ratio()) as f32).max(NARROWEST);
        std::ptr::write((base + s.aspect_const) as *mut u32, start.to_bits());
        LAUNCH.store(start.to_bits(), Ordering::Relaxed);
        ASPECT.store(start.to_bits(), Ordering::Relaxed);
        match a {
            Some(a) => println!("[+] Aspect: the game renders at the screen's shape, starting at {a}"),
            None => println!("[+] Aspect: the game renders at the screen's shape"),
        }
    } else {
        LAUNCH.store(c, Ordering::Relaxed);
        println!("[+] Aspect: hooks installed; the game keeps its 16:9 shape (the dynamic aspect ratio setting is off)");
    }
    INSTALLED.store(true, Ordering::Relaxed);
}
