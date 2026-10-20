// Screen shapes other than 16:9 for Tears of the Kingdom 1.1.2.
//
// The game sizes its view from a 16:9 constant and from width / height pairs it computes. This patch sets the
// constant to the requested aspect, makes two places that divide a 16:9 rectangle's width by its height load the
// aspect instead, and scales five width / height pairs right after they are loaded: wider than 16:9 the width by
// (aspect / 16:9), narrower the height by (16:9 / aspect). Narrower shapes keep 16:9's horizontal field of view and
// see more vertically: where cameras write their projection, tan(fovy / 2) is scaled by (16:9 / aspect).
// The added code goes into padding between functions (zero words after an unconditional branch or return, never
// executed) and loads the aspect and the factor from a data word the engine can change while the game runs
// (`set_aspect`: the window's shape). Every site is checked before it is written: it must hold the game's
// instruction, or the same site as a 19.5:9 widescreen mod patched it.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// The game's display aspect (rodata): 16 / 9, or what a 19.5:9 widescreen mod writes there.
const ASPECT_CONST: usize = 0x3813d0c;
const ASPECT_GAME: u32 = 0x3fe3_8e3b;
const ASPECT_MOD: u32 = 0x400a_aaab;
/// `fdiv s0, s0, s1`: width / height of the view rectangle.
const RATIO_SITES: [usize; 2] = [0x19b5480, 0x1ece314];
/// `ldp sW, sH, [Xn, #off]`: width / height pairs whose width is widened (site, the game's instruction).
const WIDTH_SITES: [(usize, u32); 5] = [
    (0x1ae0440, 0x2d43_07a0), // ldp s0, s1, [x29, #0x18]
    (0x1ae4c24, 0x2d44_0680), // ldp s0, s1, [x20, #0x20]
    (0x1adf838, 0x2d41_03e1), // ldp s1, s0, [sp, #8]
    (0x12c2418, 0x2d40_0500), // ldp s0, s1, [x8]
    (0x12c2828, 0x2d40_0500), // ldp s0, s1, [x8]
];
/// Where padding is looked for: main's text (functions end with up to 3 words of padding before the next 16-byte
/// boundary; branches reach all of it).
const CAVE_SEARCH: (usize, usize) = (0, 0x2b1_8000);

const FDIV_S0_S0_S1: u32 = 0x1e211800;
const SIXTEEN_NINTHS: f32 = 16.0 / 9.0;

/// The aspect the game renders at once patched (f64 bits; 0: its own 16:9).
static RENDERED: AtomicU64 = AtomicU64::new(0);
/// The data word the patched code reads: the aspect (f32, low half) and the width factor max(aspect / 16:9, 1) (high
/// half).
static SLOT: AtomicUsize = AtomicUsize::new(0);
/// The second data word: the aspect when narrower than 16:9, else 0 (low half), and the height factor
/// max(16:9 / aspect, 1) (high half).
static TALL: AtomicUsize = AtomicUsize::new(0);
/// The narrowest shape (9:21).
const NARROWEST: f32 = 9.0 / 21.0;

/// `ldr s0, [x19, #0x140]`: cameras copy their aspect into their projection (in x20 / x23) every frame they update
/// (with the field of view from +0x13c). A camera keeps the aspect it was created with, so a camera at the startup
/// aspect gets the current one here.
const CAMERA_SITES: [(usize, u32); 2] = [(0xccbcc0, 20), (0x125559c, 23)];
const LDR_S0_X19_140: u32 = 0xbd41_4260;
/// `str s0, [x19, #0xac]` in sead::PerspectiveProjection::setFovy_ (main+0x1a61b94): tan(fovy / 2).
const SET_FOVY_SITE: usize = 0x1a61bd8;
const STR_S0_X19_AC: u32 = 0xbd00_ae60;
/// Cameras the game stops updating (all of them while a menu is open; the inventory's Link) keep the aspect of their
/// last update: the camera and ratio sites also record their projection here (64 entries, by address bits 6..11), and
/// `set_aspect` gives the recorded projections that still hold the previous aspect the new one.
static PROJECTIONS: AtomicUsize = AtomicUsize::new(0);
const PROJECTION_SLOTS: usize = 64;
/// sead::PerspectiveProjection's vtable: aspect +0xb0, matrix dirty flag +8.
const PERSPECTIVE_VTABLE: usize = 0x45b5760;
static MAIN: AtomicUsize = AtomicUsize::new(0);

/// The aspect the game renders at, if this patch changed it.
pub fn rendered_aspect() -> Option<f64> {
    let bits = RENDERED.load(Ordering::Relaxed);
    (bits != 0).then(|| f64::from_bits(bits))
}

/// Changes the aspect the patched game renders at. No-op if not patched.
pub fn set_aspect(aspect: f64) {
    let slot = SLOT.load(Ordering::Relaxed);
    if slot == 0 {
        return;
    }
    let a = (aspect as f32).max(NARROWEST);
    let wide = (a / SIXTEEN_NINTHS).max(1.0);
    let tall = (SIXTEEN_NINTHS / a).max(1.0);
    let narrow = if a < SIXTEEN_NINTHS { a.to_bits() } else { 0 };
    let word = (a.to_bits() as u64) | (wide.to_bits() as u64) << 32;
    let tall_word = narrow as u64 | (tall.to_bits() as u64) << 32;
    // The tall word first: a projection at the new aspect must not get the old height factor.
    unsafe {
        (*(TALL.load(Ordering::Relaxed) as *const AtomicU64)).store(tall_word, Ordering::Relaxed)
    };
    let old = unsafe { (*(slot as *const AtomicU64)).swap(word, Ordering::Relaxed) } as u32;
    RENDERED.store((a as f64).to_bits(), Ordering::Relaxed);
    let table = PROJECTIONS.load(Ordering::Relaxed);
    let vtable = MAIN.load(Ordering::Relaxed) + PERSPECTIVE_VTABLE;
    for i in 0..PROJECTION_SLOTS {
        let p = unsafe { *((table + 8 * i) as *const usize) };
        unsafe {
            if p != 0
                && crate::cpu::crash_handler::readable_len(p, 0xb4) == 0xb4
                && *(p as *const usize) == vtable
                && (*((p + 0xb0) as *const AtomicU32))
                    .compare_exchange(old, a.to_bits(), Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                *((p + 8) as *mut u8) = 1;
            }
        }
    }
}

fn b(from: usize, to: usize) -> u32 {
    0x1400_0000 | (((to as i64 - from as i64) >> 2) as u32 & 0x03ff_ffff)
}

fn adrp(pc: usize, target: usize, rd: u32) -> u32 {
    let pages = ((target & !0xfff) as i64 - (pc & !0xfff) as i64) >> 12;
    0x9000_0000 | ((pages as u32 & 3) << 29) | (((pages >> 2) as u32 & 0x7ffff) << 5) | rd
}

/// `ldur sT, [Xn, #off]` (any offset in -256..256).
fn ldur_s(rt: u32, rn: u32, off: i64) -> u32 {
    0xbc40_0000 | ((off as u32 & 0x1ff) << 12) | (rn << 5) | rt
}

/// `ldr sT, [Xn, #off]` (off a multiple of 4 in 0..16384).
fn ldr_s(rt: u32, rn: u32, off: usize) -> u32 {
    0xbd40_0000 | (((off / 4) as u32 & 0xfff) << 10) | (rn << 5) | rt
}

/// `str sT, [Xn, #off]` (off a multiple of 4 in 0..16384).
fn str_s(rt: u32, rn: u32, off: usize) -> u32 {
    0xbd00_0000 | (((off / 4) as u32 & 0xfff) << 10) | (rn << 5) | rt
}

fn fmul_s(rd: u32, rn: u32, rm: u32) -> u32 {
    0x1e20_0800 | (rm << 16) | (rn << 5) | rd
}

fn fadd_s(rd: u32, rn: u32, rm: u32) -> u32 {
    0x1e20_2800 | (rm << 16) | (rn << 5) | rd
}

fn fdiv_s(rd: u32, rn: u32, rm: u32) -> u32 {
    0x1e20_1800 | (rm << 16) | (rn << 5) | rd
}

fn fsqrt_s(rd: u32, rn: u32) -> u32 {
    0x1e21_c000 | (rn << 5) | rd
}

/// `fmadd sD, sN, sM, sA`: a + n * m.
fn fmadd_s(rd: u32, rn: u32, rm: u32, ra: u32) -> u32 {
    0x1f00_0000 | (rm << 16) | (ra << 10) | (rn << 5) | rd
}

const SUB_SP_16: u32 = 0xd100_43ff;
const ADD_SP_16: u32 = 0x9100_43ff;
const STR_X0_SP: u32 = 0xf900_03e0;
const LDR_X0_SP: u32 = 0xf940_03e0;

/// `ldp sT, sT2, [Xn, #off]` (32-bit SIMD, signed offset): (Rt, Rt2, Rn, offset).
fn decode_ldp_s(w: u32) -> Option<(u32, u32, u32, i64)> {
    (w & 0xffc0_0000 == 0x2d40_0000).then(|| {
        let imm7 = ((w >> 15) & 0x7f) as i64;
        let imm7 = if imm7 & 0x40 != 0 { imm7 - 0x80 } else { imm7 };
        (w & 31, (w >> 10) & 31, (w >> 5) & 31, imm7 * 4)
    })
}

unsafe fn word(base: usize, off: usize) -> u32 {
    std::ptr::read((base + off) as *const u32)
}

unsafe fn put(base: usize, off: usize, w: u32) {
    std::ptr::write((base + off) as *mut u32, w);
}

/// Padding to put code in: 3-word runs of zeros right after an unconditional branch or a return.
unsafe fn find_gaps(base: usize) -> Vec<usize> {
    let mut gaps = Vec::new();
    let mut off = CAVE_SEARCH.0;
    while off + 16 <= CAVE_SEARCH.1 {
        let prev = word(base, off);
        let ends_flow =
            prev == 0xd65f_03c0 || prev >> 26 == 0x05 || prev & 0xffff_fc1f == 0xd61f_0000;
        if ends_flow && (1..=3).all(|i| word(base, off + 4 * i) == 0) {
            gaps.push(off + 4);
            off += 16;
        } else {
            off += 4;
        }
    }
    gaps
}

/// Writes `code` into chained gaps (main offsets; two instructions and a branch to the next gap each), ending with a
/// branch to main offset `back`; returns the first gap.
unsafe fn emit(
    base: usize,
    gaps: &mut std::vec::IntoIter<usize>,
    code: &[u32],
    back: usize,
) -> Option<usize> {
    let first = gaps.as_slice().first().copied()?;
    let mut chunks = code.chunks(2).peekable();
    while let Some(chunk) = chunks.next() {
        let at = gaps.next()?;
        for (i, w) in chunk.iter().enumerate() {
            put(base, at + 4 * i, *w);
        }
        let jump_at = at + 4 * chunk.len();
        let next = if chunks.peek().is_some() {
            *gaps.as_slice().first()?
        } else {
            back
        };
        put(base, jump_at, b(jump_at, next));
    }
    Some(first)
}

/// Vert+ for the projection in x<proj> whose aspect is in s0: when that aspect is the narrow screen aspect (the tall
/// word's low half), its tan(fovy / 2) is multiplied by the height factor and sin, cos and fovy derived from it; other
/// projections keep their values bitwise. Uses x0..x2 and s1..s7 (saved by the caller). `pc_of(i)`: the address
/// instruction i of the code will have.
fn vert_plus(
    code: &mut Vec<u32>,
    pc_of: &dyn Fn(usize) -> Option<usize>,
    proj: u32,
    tall: usize,
    coef: usize,
) -> Result<(), String> {
    let pc_tall = pc_of(code.len()).ok_or("out of padding")?;
    code.extend([
        adrp(pc_tall, tall, 0),
        0xf940_0001 | ((tall & 0xfff) as u32 / 8) << 10, // ldr x1, [x0, #tall]
        0x1e26_0002,                                     // fmov w2, s0
        0x4a01_0042,                                     // eor w2, w2, w1
        0x5ac0_1042,                                     // clz w2, w2
        0x5305_7c42,                                     // lsr w2, w2, #5
        0x4b02_03e2,                                     // neg w2, w2
        0xd360_fc21,                                     // lsr x1, x1, #32
        0x1e27_0022,                                     // fmov s2, w1 (the height factor)
        ldr_s(1, proj, 0xac),
        fmul_s(1, 1, 2), // tan
        fmul_s(2, 1, 1),
        0x1e2e_1003,     // fmov s3, #1.0
        0x1e23_2842,     // fadd s2, s2, s3
        0x1e21_c042,     // fsqrt s2, s2
        0x1e22_1863,     // fdiv s3, s3, s2: cos = 1 / sqrt(1 + tan^2)
        fmul_s(2, 1, 3), // sin = tan * cos
    ]);
    // fovy = 2 atan(tan) = 8 atan(v), v = tan(fovy / 8) by two half-angle steps (v < 0.42 below 170 degrees),
    // atan(v) by its series to v^13.
    let pc_coef = pc_of(code.len()).ok_or("out of padding")?;
    code.extend([
        adrp(pc_coef, coef, 0),
        0x9100_0000 | ((coef & 0xfff) as u32) << 10, // add x0, x0, #coef
        0x1e2e_1004,                                 // fmov s4, #1.0
        fadd_s(5, 3, 4),
        fdiv_s(5, 2, 5), // u = tan(fovy / 4) = sin / (1 + cos)
        fmul_s(6, 5, 5),
        fadd_s(6, 6, 4),
        fsqrt_s(6, 6),
        fadd_s(6, 6, 4),
        fdiv_s(5, 5, 6), // v = u / (1 + sqrt(1 + u^2))
        fmul_s(6, 5, 5), // v^2
        ldr_s(4, 0, 0),
    ]);
    for i in 1..7 {
        code.extend([ldr_s(7, 0, 4 * i), fmadd_s(4, 4, 6, 7)]);
    }
    code.extend([
        fmul_s(4, 4, 5), // atan(v)
        0x1e24_1007,     // fmov s7, #8.0
        fmul_s(4, 4, 7),
    ]);
    for (r, off) in [(1u32, 0xacu32), (3, 0xa8), (2, 0xa4), (4, 0xa0)] {
        code.extend([
            0xb940_0000 | (off / 4) << 10 | proj << 5, // ldr w0, [x<proj>, #off]
            0x1e26_0001 | r << 5,                      // fmov w1, s<r>
            0x4a00_0021,                               // eor w1, w1, w0
            0x0a02_0021,                               // and w1, w1, w2
            0x4a01_0000,                               // eor w0, w0, w1
            0xb900_0000 | (off / 4) << 10 | proj << 5, // str w0, [x<proj>, #off]
        ]);
    }
    Ok(())
}

/// Saves / restores x0..x2 and d1..d7 around `vert_plus` (0x60 bytes of stack).
const SAVE: [u32; 7] = [
    0xd101_83ff, // sub sp, sp, #0x60
    0xa900_07e0, // stp x0, x1, [sp]
    0xf900_0be2, // str x2, [sp, #0x10]
    0x6d02_0be1, // stp d1, d2, [sp, #0x20]
    0x6d03_13e3, // stp d3, d4, [sp, #0x30]
    0x6d04_1be5, // stp d5, d6, [sp, #0x40]
    0xfd00_2be7, // str d7, [sp, #0x50]
];
const RESTORE: [u32; 7] = [
    0x6d42_0be1, // ldp d1, d2, [sp, #0x20]
    0x6d43_13e3, // ldp d3, d4, [sp, #0x30]
    0x6d44_1be5, // ldp d5, d6, [sp, #0x40]
    0xfd40_2be7, // ldr d7, [sp, #0x50]
    0xf940_0be2, // ldr x2, [sp, #0x10]
    0xa940_07e0, // ldp x0, x1, [sp]
    0x9101_83ff, // add sp, sp, #0x60
];

/// Patches main (at `base`, still writable) to render at `aspect` (width / height).
pub unsafe fn apply(base: usize, aspect: f64) -> Result<(), String> {
    let a = (aspect as f32).max(NARROWEST);
    // Check every site before writing any.
    let c = word(base, ASPECT_CONST);
    if c != ASPECT_GAME && c != ASPECT_MOD {
        return Err(format!("unexpected aspect constant {c:#010x}"));
    }
    for s in RATIO_SITES {
        let w = word(base, s);
        // The game's division, or a widescreen mod's fixed `fmov s0, #imm`.
        if w != FDIV_S0_S0_S1 && w & 0xffe0_1fff != 0x1e20_1000 {
            return Err(format!("unexpected instruction {w:#010x} at main+{s:#x}"));
        }
    }
    let mut pairs = Vec::new();
    for (s, game) in WIDTH_SITES {
        let w = word(base, s);
        // The game's load, or a widescreen mod's branch to its own code.
        if w != game && w >> 26 != 0x05 {
            return Err(format!("unexpected instruction {w:#010x} at main+{s:#x}"));
        }
        match decode_ldp_s(game) {
            Some(p) if p.3 >= -256 && p.3 + 4 < 256 => pairs.push((s, p)),
            _ => return Err(format!("main+{s:#x}: {game:#010x} is not a supported ldp")),
        }
    }
    for (s, _) in CAMERA_SITES {
        let w = word(base, s);
        if w != LDR_S0_X19_140 {
            return Err(format!("unexpected instruction {w:#010x} at main+{s:#x}"));
        }
    }
    let w = word(base, SET_FOVY_SITE);
    if w != STR_S0_X19_AC {
        return Err(format!(
            "unexpected instruction {w:#010x} at main+{SET_FOVY_SITE:#x}"
        ));
    }
    let mut gaps = find_gaps(base).into_iter();
    // An 8-byte word in the engine's literal page (writable, at a fixed distance from main).
    let slot = crate::cpu::aot::literal(0);
    SLOT.store(slot, Ordering::Relaxed);
    let tall = crate::cpu::aot::literal(0);
    TALL.store(tall, Ordering::Relaxed);
    // atan series coefficients (f32 pairs): 1/13, -1/11, 1/9, -1/7, 1/5, -1/3, 1.
    let coef = crate::cpu::aot::literal(0);
    let c: [f32; 8] = [
        1.0 / 13.0,
        -1.0 / 11.0,
        1.0 / 9.0,
        -1.0 / 7.0,
        1.0 / 5.0,
        -1.0 / 3.0,
        1.0,
        0.0,
    ];
    for i in 0..4 {
        let at = if i == 0 {
            coef
        } else {
            crate::cpu::aot::literal(0)
        };
        assert_eq!(at, coef + 8 * i);
        *(at as *mut u64) = c[2 * i].to_bits() as u64 | (c[2 * i + 1].to_bits() as u64) << 32;
    }
    let table = crate::cpu::aot::literal(0);
    for i in 1..PROJECTION_SLOTS {
        assert_eq!(crate::cpu::aot::literal(0), table + 8 * i);
    }
    PROJECTIONS.store(table, Ordering::Relaxed);
    MAIN.store(base, Ordering::Relaxed);
    put(base, ASPECT_CONST, a.to_bits());
    // The view rectangle's ratio: the aspect, loaded with x0 saved on the stack. emit() puts instruction i
    // at gap i / 2, word i % 2: adrp's page delta depends on that address.
    let at = |gaps: &std::vec::IntoIter<usize>, i: usize| {
        gaps.as_slice().get(i / 2).map(|g| base + g + 4 * (i % 2))
    };
    for s in RATIO_SITES {
        let start = *gaps.as_slice().first().ok_or("out of padding")?;
        let pc = at(&gaps, 2).ok_or("out of padding")?;
        let pc_table = at(&gaps, 4).ok_or("out of padding")?;
        let code = [
            SUB_SP_16,
            0xa900_07e0, // stp x0, x1, [sp]
            adrp(pc, slot, 0),
            ldr_s(0, 0, slot & 0xfff),
            adrp(pc_table, table, 0),
            0x9100_0000 | ((table & 0xfff) as u32) << 10, // add x0, x0, #table
            0xd346_2d01, // ubfx x1, x8, #6, #6 (x8: the projection)
            0xf821_7808, // str x8, [x0, x1, lsl #3]
            0xa940_07e0, // ldp x0, x1, [sp]
            ADD_SP_16,
        ];
        emit(base, &mut gaps, &code, s + 4).ok_or("out of padding")?;
        put(base, s, b(s, start));
    }
    // Width / height pairs: load both, widen the width by the factor (in the height's register until the height is
    // loaded, x0 saved on the stack for the factor's address).
    for (s, (rw, rh, rn, off)) in pairs {
        if rn == 0 {
            return Err(format!(
                "main+{s:#x}: pairs loaded through x0 are not supported"
            ));
        }
        let start = *gaps.as_slice().first().ok_or("out of padding")?;
        let pc = at(&gaps, 3).ok_or("out of padding")?;
        let pc_tall = at(&gaps, 8).ok_or("out of padding")?;
        // The height is loaded below the 16 bytes this code puts on the stack.
        let h_off = off + 4 + if rn == 31 { 16 } else { 0 };
        let code = [
            ldur_s(rw, rn, off),
            SUB_SP_16,
            STR_X0_SP,
            adrp(pc, slot, 0),
            ldr_s(rh, 0, (slot & 0xfff) + 4),
            fmul_s(rw, rw, rh),
            str_s(rw, 31, 8),
            ldur_s(rw, rn, h_off),
            adrp(pc_tall, tall, 0),
            ldr_s(rh, 0, (tall & 0xfff) + 4),
            fmul_s(rh, rw, rh),
            ldr_s(rw, 31, 8),
            LDR_X0_SP,
            ADD_SP_16,
        ];
        emit(base, &mut gaps, &code, s + 4).ok_or("out of padding")?;
        put(base, s, b(s, start));
    }
    // Camera aspects: the loaded value, replaced by the current aspect when it equals the startup aspect (bitwise,
    // without flags or branches: mask = all ones iff equal). Then Vert+: when that aspect is the screen's and narrower
    // than 16:9, the projection's tan(fovy / 2) (just stored by the game) is multiplied by the height factor and its
    // sin, cos and fovy derived from it, so the matrix, culling and shadows all see the taller view (selected bitwise
    // as well: other projections keep the game's values exactly). x0..x2 and d1..d7 saved on the stack.
    for (s, proj) in CAMERA_SITES {
        let start = *gaps.as_slice().first().ok_or("out of padding")?;
        let pc = at(&gaps, 14).ok_or("out of padding")?;
        let pc_table = at(&gaps, 21).ok_or("out of padding")?;
        let bits = a.to_bits();
        let mut code = vec![LDR_S0_X19_140];
        code.extend(SAVE);
        code.extend([
            0x1e26_0001,                        // fmov w1, s0
            0x5280_0002 | (bits & 0xffff) << 5, // movz w2, #lo
            0x72a0_0002 | (bits >> 16) << 5,    // movk w2, #hi, lsl #16
            0x4a02_0022,                        // eor w2, w1, w2
            0x5ac0_1042,                        // clz w2, w2
            0x5305_7c42,                        // lsr w2, w2, #5
            adrp(pc, slot, 0),
            0xb940_0000 | ((slot & 0xfff) as u32 / 4) << 10, // ldr w0, [x0, #slot]
            0x4b02_03e2,                                     // neg w2, w2
            0x4a01_0000,                                     // eor w0, w0, w1
            0x0a02_0000,                                     // and w0, w0, w2
            0x4a00_0021,                                     // eor w1, w1, w0
            0x1e27_0020,                                     // fmov s0, w1
            adrp(pc_table, table, 0),
            0x9100_0000 | ((table & 0xfff) as u32) << 10, // add x0, x0, #table
            0xd346_2c01 | proj << 5,                      // ubfx x1, x<proj>, #6, #6
            0xf821_7800 | proj,                           // str x<proj>, [x0, x1, lsl #3]
        ]);
        vert_plus(&mut code, &|i| at(&gaps, i), proj, tall, coef)?;
        code.extend(RESTORE);
        emit(base, &mut gaps, &code, s + 4).ok_or("out of padding")?;
        put(base, s, b(s, start));
    }
    // sead::PerspectiveProjection::setFovy_ (projections set up from a field of view each frame, such as the
    // inventory's Link): Vert+ right after it stores tan, for a projection at the narrow screen aspect (w8 holds the
    // dirty flag value, kept).
    {
        let start = *gaps.as_slice().first().ok_or("out of padding")?;
        let mut code = vec![STR_S0_X19_AC];
        code.extend(SAVE);
        code.push(ldr_s(0, 19, 0xb0));
        vert_plus(&mut code, &|i| at(&gaps, i), 19, tall, coef)?;
        code.extend(RESTORE);
        emit(base, &mut gaps, &code, SET_FOVY_SITE + 4).ok_or("out of padding")?;
        put(base, SET_FOVY_SITE, b(SET_FOVY_SITE, start));
    }
    set_aspect(a as f64);
    Ok(())
}
