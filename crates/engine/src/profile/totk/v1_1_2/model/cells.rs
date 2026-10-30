// A dispatch delegate target drawing a 4 x 8 x 10 set of cells around the camera (main+0x886c90): the view's slot,
// three blocks and the slot's block, then per present cell within reach (closest point of its box, squared distance
// against the layer's radius) the cell's uniform block and the cell's draw through the drawer object at +0x390.

use super::draw::{load_resource, Port, R};
use super::grid::{bind_slot, bind_textures};

const MODE_HOLDER: usize = 0x462ade0;
const FADE_OFFSET: usize = 0x380f5e0;
const FADE_SCALE: usize = 0x380f3c0;

/// Virtual calls the port leaves to the game: (main+ function | slot << 40, count).
static CALLS: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

fn note(f: usize, slot: usize) {
    let mut g = CALLS.lock().unwrap();
    let k = f | slot << 40;
    match g.iter_mut().find(|x| x.0 == k) {
        Some(x) => x.1 += 1,
        None => g.push((k, 1)),
    }
}

pub(super) fn report() -> String {
    let mut g = std::mem::take(&mut *CALLS.lock().unwrap());
    g.sort_by(|a, b| b.1.cmp(&a.1));
    g.iter().map(|(f, n)| format!("+{:#x} main+{:#x} {n}", f >> 40, f & ((1 << 40) - 1))).collect::<Vec<_>>().join(", ")
}

/// AArch64 FMIN: NaN if either is NaN, -0 below +0.
fn fmin(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// |v| as the game takes it (FCMP, then FNEG when less: a NaN is negated).
fn abs(v: f32) -> f32 {
    if !(v >= 0.0) { -v } else { v }
}

/// main+0x886c90(this, arg).
pub(super) unsafe fn draw(p: &mut Port, this: usize, arg: usize) -> R<()> {
    if p.m.r8(this + 0x378) & 1 == 0 {
        return Ok(());
    }
    let ctx = p.m.r64(arg);
    let a388 = p.m.r64(this + 0x388);
    let view = p.m.r8(ctx + 0x30) as u32;
    let n = p.m.r32(a388 + 0x1d0);
    if !(n as i32 > view as i32) {
        return Ok(());
    }
    let ent = p.m.r64(a388 + 0x1d8) + if n > view { view } else { 0 } as usize * 8;
    if p.m.r8(ent + 4) == 0 {
        return Ok(());
    }
    let slot = p.m.i32(ent);
    if slot < 0 {
        return Ok(());
    }
    if p.m.r64(p.m.r64(arg + 8) + 0x68) != p.m.r64(p.m.r64(this + 0x390) + 0x58) {
        return Ok(());
    }
    let w = p.m.r64(ctx + 0x20);
    let ri = if p.m.r32(this + 8) > slot as u32 { slot as u32 } else { 0 } as usize;
    let rec = p.m.r64(this + 0x10) + ri * 32;
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    let blocks = p.m.r64(rec + 8);
    let levels = p.m.r32(rec);
    bind_slot(p, w, arg, 7, blocks, 0)?;
    bind_slot(p, w, arg, 9, blocks + (levels > 1) as usize * 0x80, 0)?;
    bind_slot(p, w, arg, 10, blocks + (levels > 2) as usize * 0x100, 0)?;
    let slots = p.m.r32(p.m.r64(p.m.r64(p.m.r64(arg + 0x10) + 8) + 8) + 0x14);
    p.bind_uniform_block(p.m.r64(rec + 0x18), w, slots, 0)?;
    let f68 = p.vfn(this, 0x68);
    match f68 {
        0x13ef344 => setup(p, this, arg, None)?,
        0x13efdcc => setup(p, this, arg, Some((0x462ce50, 0x462ce58)))?,
        0x13ef70c => setup(p, this, arg, Some((0x462ce18, 0x462ce20)))?,
        0x13ef9a8 => setup(p, this, arg, Some((0x462ce38, 0x462ce40)))?,
        _ => {
            note(f68, 0x68);
            p.helper("cell setup", f68, &[this as u64, arg as u64, slot as u32 as u64])?;
        }
    }
    let radius = p.m.f32(this + 0x368);
    let r_far = radius * radius;
    let drawer = p.m.r64(this + 0x390);
    let mut r_near = r_far;
    if p.m.r32(this + 0x380) == 0
        && (p.m.r8(p.m.r64(p.m.r64(p.m.r64(p.base + MODE_HOLDER)) + 0x128) + 0xc99) as u32).wrapping_sub(7) <= 1
    {
        let k = if p.m.r32(a388 + 0x1e0) > slot as u32 { slot as u32 } else { 0 } as usize;
        let e = p.m.r64(a388 + 0x1e8) + k * 0x58;
        let t = (p.m.f32(e + 0x1c) + p.m.f32(p.base + FADE_OFFSET)) * p.m.f32(p.base + FADE_SCALE);
        let fade = if t < 0.0 { 2.0 } else { 2.0 - fmin(t, 1.0) };
        let r = radius * fade;
        r_near = r * r;
    }
    let f30 = p.vfn(this, 0x30);
    for layer in 0..4usize {
        let reach = if layer == 0 { r_near } else { r_far };
        for row in 0..8usize {
            for col in 0..10usize {
                let cell = layer * 0x50 + row * 10 + col;
                let b = p.m.r64(this + 0x410 + cell * 8);
                if b == 0 {
                    continue;
                }
                let t = p.m.r64(this + 0x3a0);
                let k = if (cell as u64) < p.m.r32(t + 0x10) as u64 { cell } else { 0 };
                let id = p.m.r16(p.m.r64(t + 0x18) + k * 2);
                if id == 0 {
                    continue;
                }
                let a388 = p.m.r64(this + 0x388);
                let k = if p.m.r32(a388 + 0x1e0) > slot as u32 { slot as u32 } else { 0 } as i32 as i64;
                let e = (p.m.r64(a388 + 0x1e8) as i64 + k * 0x58) as usize;
                let above = match f30 {
                    0x26abb50 | 0x26abe78 => false,
                    0x26ac39c | 0x26ac00c => true,
                    _ => {
                        note(f30, 0x30);
                        p.helper("cell test", f30, &[this as u64])? & 1 != 0
                    }
                };
                let (cy, ey) = (p.m.f32(b + 4), p.m.f32(e + 0x2c));
                if above && cy > ey {
                    continue;
                }
                // The box corner nearest the eye on each axis (min at +0, max at +0xc), its squared distance.
                let pick = |p: &Port, eye: f32, k: usize| {
                    let (to_max, to_min) = (abs(eye - p.m.f32(b + 0xc + k * 4)), abs(eye - p.m.f32(b + k * 4)));
                    p.m.f32(if to_max < to_min { b + 0xc } else { b } + k * 4)
                };
                let (ex, ez) = (p.m.f32(e + 0x28), p.m.f32(e + 0x30));
                let dx = ex - pick(p, ex, 0);
                let dy = ey - pick(p, ey, 1);
                let dz = ez - pick(p, ez, 2);
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2 > reach {
                    continue;
                }
                let index = p.m.r16(this + 0x98 + cell * 2) as i16 as i32;
                bind_slot(p, w, arg, 8, this + 0x18, index)?;
                if id == 0xffff {
                    let f = p.vfn(drawer, 0x20);
                    match f {
                        // The cell's index range in the drawer's tables (+0x4d0 first, +0x9d0 count; 4 x 8 x 10),
                        // in units of 6 or 9 indices, drawn from its mesh at +0x428.
                        0x13f247c | 0x26add8c | 0x26ad688 | 0x26ad0b0 => {
                            let unit = if f == 0x13f247c || f == 0x26add8c { 6 } else { 9 };
                            let c = drawer + layer * 0x140 + row * 0x28 + col * 4;
                            let (first, count) = (p.m.r32(c + 0x4d0).wrapping_mul(unit), p.m.r32(c + 0x9d0).wrapping_mul(unit));
                            super::cave::mesh_elements(p, drawer + 0x428, w, first, count, 1, 0, 0);
                        }
                        _ => {
                            note(f, 0x20);
                            p.helper("cell draw", f, &[drawer as u64, w as u64, col as u64, layer as u64, row as u64])?;
                        }
                    }
                } else {
                    let f = p.vfn(drawer, 0x28);
                    if matches!(f, 0x879d20 | 0x13f263c | 0x26ad1cc | 0x26ad7a4) {
                        runs(p, drawer, w, col, layer, row, id as u32, if f == 0x879d20 || f == 0x13f263c { 6 } else { 9 });
                        continue;
                    }
                    note(f, 0x28);
                    p.helper("cell draw", f, &[drawer as u64, w as u64, col as u64, layer as u64, row as u64, id as u64])?;
                }
            }
        }
    }
    Ok(())
}

/// The cell setups (main+0x13ef344, and main+0x13efdcc / 0x13ef70c / 0x13ef9a8 through a cast of the drawer with
/// the static guard and type token `cast`): the drawer's three textures (+0x90 indices into its list, +0x80 table
/// slots) bound with every other entry of the drawable's pass program (main+0x777170).
unsafe fn setup(p: &mut Port, this: usize, arg: usize, cast: Option<(usize, usize)>) -> R<()> {
    let ctx = p.m.r64(arg);
    let drawer = p.m.r64(this + 0x390);
    let w = p.m.r64(ctx + 0x20);
    let obj = match cast {
        None => drawer,
        Some((guard, ty)) => {
            if p.m.r8(p.m.r64(p.base + guard)) & 1 == 0 {
                p.helper("cell setup (static init)", p.vfn(this, 0x68), &[this as u64, arg as u64])?;
                return Ok(());
            }
            p.query(p.base + p.vfn(drawer, 0), &[drawer as u64, p.m.r64(p.base + ty) as u64]) as usize
        }
    };
    let mut table = [0u64; 32];
    for k in 0..3usize {
        let list = p.m.r64(obj + 0x90);
        let i = p.m.i32(list + if p.m.r32(obj + 0x88) > k as u32 { k } else { 0 } * 4);
        let tex = if i < 0 {
            0
        } else {
            let t = p.m.r64(p.m.r64(p.m.r64(obj + 0x70) + 0x18) + i as u32 as usize * 8);
            if t == 0 { 0 } else { p.m.r64(t + 0x40) }
        };
        let slots = p.m.r64(obj + 0x80);
        let s = p.m.i32(slots + if p.m.r32(obj + 0x78) > k as u32 { k } else { 0 } * 4);
        if let Some(e) = table.get_mut(s as usize) {
            *e = tex as u64;
        }
    }
    // The pass program and defaults are the drawable's (arg + 8).
    let d = p.m.r64(arg + 8);
    let pb = p.m.r64(p.m.r64(d + 0x68) + p.m.r8(ctx + 0x31) as usize * 0x60 + 0x40);
    let defaults = p.m.r64(p.m.r64(d + 8) + 0x180) + p.m.r16(d + 0x18) as usize * 0x80;
    bind_textures(p, w, pb, defaults, &table, u32::MAX)
}

/// main+0x879d2c (unit 6, through main+0x879d20 / 0x13f263c) / main+0x26ad1cc / 0x26ad7a4 (unit 9): the cell's 16 parts in `mask`, lowest to
/// highest set bit; consecutive parts with indices form one run (first from +0xed0 of its first part, counts from
/// +0x5ed0 added up; a part without indices does not start a run), each run drawn from the mesh at +0x428 in
/// `unit`-index units.
#[allow(clippy::too_many_arguments)]
unsafe fn runs(p: &mut Port, obj: usize, w: usize, col: usize, layer: usize, row: usize, mask: u32, unit: u32) {
    let m = mask & 0xffff;
    let lo = if m == 0 { 16 } else { m.trailing_zeros() };
    let hi = 32 - m.leading_zeros();
    let c = obj + layer * 0x1400 + row * 0x280 + col * 0x40;
    let part = |k: u32| c + if k < 16 { k as usize } else { 0 } * 4;
    let mut draw = |p: &mut Port, start: u32, count: u32| {
        let first = p.m.r32(part(start) + 0xed0);
        super::cave::mesh_elements(p, obj + 0x428, w, first.wrapping_mul(unit), count.wrapping_mul(unit), 1, 0, 0);
    };
    let (mut start, mut count) = (u32::MAX, 0u32);
    let mut k = lo;
    while (k as i32) < (hi as i32) {
        let bit = (1u32 << (k & 31)) & mask & 0xffff;
        if start == u32::MAX {
            if bit != 0 {
                let n = p.m.r32(part(k) + 0x5ed0);
                if n != 0 {
                    start = k;
                    count = n;
                }
            }
        } else if bit != 0 {
            count = count.wrapping_add(p.m.r32(part(k) + 0x5ed0));
        } else {
            if count != 0 {
                draw(p, start, count);
            }
            start = u32::MAX;
            count = 0;
        }
        k += 1;
    }
    if start != u32::MAX && count != 0 {
        draw(p, start, count);
    }
}
