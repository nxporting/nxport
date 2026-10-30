// A dispatch delegate target drawing a tiled grid with LOD distance windows (main+0x18ccb80): per visible cell of the
// level-2 grid, up to five textures through a texture table, the cell's uniform block and one indexed draw.
// Pure game helpers (type casts, texture selection) are called as they are; what records commands is ported.

use super::draw::{load_resource, Port, R};

const PFN_DEVICE_GET_TEXTURE_HANDLE: usize = 0x466d428;
const PFN_BIND_TEXTURE: usize = 0x466e008;
const PFN_BUFFER_GET_ADDRESS: usize = 0x466d818;
const PFN_DRAW_ELEMENTS: usize = 0x466e088;
const DEVICE_HOLDER: usize = 0x462bbe8;
const ENV_HOLDER: usize = 0x46353f8;
/// Static guard and type token of the shader object cast.
const CAST_GUARD: usize = 0x46354b0;
const CAST_TYPE: usize = 0x46354b8;
const TEXTURE_PRESENT: usize = 0x745630;
const TEXTURE_OF: usize = 0x745674;
const ENV_TEXTURE: usize = 0x745e10;
const GRID_LEVEL: usize = 0x18cd474;
const GRID_BLOCKS: usize = 0x18cd498;
/// Distance windows (far start / end, near start / end) and the constants the window math uses.
const FAR_A: usize = 0x46b3af8;
const FAR_B: usize = 0x46b3af0;
const NEAR_A: usize = 0x46b3ae8;
const NEAR_B: usize = 0x46b3ae0;
const MIN_WIDTH: usize = 0x380e988;
const MIN_SCALE: usize = 0x380eeb0;

/// AArch64 FMAX: NaN if either is NaN, +0 above -0.
fn fmax(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// main+0x7456fc(w, arg, k, block, index): uniform block `block` bound to the program's block slots k.
unsafe fn bind_slot(p: &mut Port, w: usize, arg: usize, k: usize, block: usize, index: i32) -> R<()> {
    let slots = p.m.r32(p.m.r64(p.m.r64(p.m.r64(arg + 0x10) + 8)) + 0x10 + k * 4);
    p.bind_uniform_block(block, w, slots, index)?;
    Ok(())
}

/// main+0x745a30(w, arg, table, mask): the textures of the program's table entries in `mask` (a table entry 0: the
/// drawable's default), bound to their slots.
unsafe fn bind_texture_table(p: &mut Port, w: usize, arg: usize, table: &[u64; 32], mask: u32) -> R<()> {
    if mask == 0 {
        return Ok(());
    }
    let (ctx, d) = (p.m.r64(arg), p.m.r64(arg + 8));
    let pb = p.m.r64(p.m.r64(d + 0x68) + p.m.r8(ctx + 0x31) as usize * 0x60 + 0x40);
    let defaults = p.m.r64(p.m.r64(d + 8) + 0x180) + p.m.r16(d + 0x18) as usize * 0x80;
    // All bits: main+0x777170, the same loop over every entry.
    let all = mask == u32::MAX;
    let mut k = 0usize;
    while (k as i32) < p.m.i32(pb + 0x308) {
        let e = pb + k * 0x18;
        k += 1;
        let idx = p.m.r8(e + 0x18) as usize;
        if !all && (mask >> (idx & 0x1f)) & 1 == 0 {
            continue;
        }
        let mut t = table.get(idx).copied().unwrap_or(0) as usize;
        if t == 0 {
            t = p.m.r64(p.m.r64(defaults + 0x50) + idx * 8);
            if t == 0 {
                continue;
            }
            t = p.m.r64(t + 0x40);
            if t == 0 {
                continue;
            }
        }
        let sampler = p.m.r64(p.m.r64(p.m.r64(pb) + 0x90) + idx * 8);
        let tid = p.m.i32(t + 0x90);
        if tid == -1 || sampler & 0xffff == 0xffff {
            continue;
        }
        let slots = p.m.r32(e + 8);
        let dev = p.m.r64(p.m.r64(p.m.r64(p.base + DEVICE_HOLDER)) + 0x30);
        let handle = p.query(p.pfn(PFN_DEVICE_GET_TEXTURE_HANDLE), &[dev as u64, tid as u32 as u64, sampler as i16 as i32 as u32 as u64]);
        let cmd = p.m.r64(w + 0xb8) as u64;
        for (stage, sh) in [(0u64, 0), (1, 8), (2, 16), (5, 24)] {
            if (slots >> sh) & 0xff != 0xff {
                p.call(p.pfn(PFN_BIND_TEXTURE), &[cmd, stage, (slots >> sh) as i8 as i64 as u64, handle]);
            }
        }
    }
    Ok(())
}

/// main+0x18cd4c0(scale, shader, w, 0): the shader's index range `scale` times, drawn when the shader is enabled.
unsafe fn draw_range(p: &mut Port, s: usize, w: usize) -> u32 {
    let n = (p.m.r32(p.m.r64(s + 0x4d8)) as f32 * 1.0) as i32;
    if n == 0 || p.m.r8(s + 8) & 1 == 0 {
        return 0;
    }
    let prim = p.m.r32(s + 0x42c) as u64;
    let cmd = p.m.r64(w + 0xb8);
    let first = p.m.r32(p.m.r64(s + 0x4c8));
    let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[(s + 0x450) as u64]).wrapping_add(p.m.r32(s + 0x44c).wrapping_mul(first) as u64);
    let itype = p.m.r32(s + 0x428) as u64;
    p.m.w::<u8>(cmd + 0x10, 1);
    p.call(p.pfn(PFN_DRAW_ELEMENTS), &[cmd as u64, prim, itype, n as u32 as u64, addr]);
    n as u32
}

/// main+0x18ccb80(this, arg, k); `k` is what the delegate invoke leaves in w2 (the target's address).
pub(super) unsafe fn draw(p: &mut Port, this: usize, arg: usize, k: u32) -> R<()> {
    if p.m.r8(p.m.r64(p.base + CAST_GUARD)) & 1 == 0 {
        p.helper("grid draw (static init)", 0x18ccb80, &[this as u64, arg as u64, k as u64])?;
        return Ok(());
    }
    let g = p.m.r64(this + 0x10);
    let recs = p.m.r64(g + 0x208);
    let ri = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
    let ti = if p.m.r32(this + 0x90) > k { k } else { 0 } as i32 as i64;
    let t98 = p.m.r64(this + 0x98);
    let ctx = p.m.r64(arg);
    let w = p.m.r64(ctx + 0x20);
    let rec = (recs as i64 + ri * 0xa8) as usize;
    let scale0 = p.m.f32(rec + 0x4c);
    let s0 = p.m.r64(this + 0x18);
    let shader = if s0 == 0 { 0 } else { p.query(p.base + p.vfn(s0, 0), &[s0 as u64, p.m.r64(p.base + CAST_TYPE) as u64]) as usize };
    let tiles = p.m.r64(rec + 0x28);
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    bind_slot(p, w, arg, 7, p.m.r64(rec + 0x98), 0)?;
    // Everything is bound for the first cell.
    let mut mask = u32::MAX;
    let mut table = [0u64; 32];
    let role = |p: &Port, n: u32, off: usize| p.m.i32(p.m.r64(shader + 0x78) + if p.m.r32(shader + 0x70) > n { off } else { 0 }) as usize;
    let set = |idx: usize, v: u64, mask: &mut u32, table: &mut [u64; 32]| {
        if let Some(e) = table.get_mut(idx) {
            if *e != v {
                *e = v;
                *mask |= 1u32.wrapping_shl(idx as u32);
            }
        }
    };
    set(role(p, 6, 0x18), (p.m.r64(p.m.r64(rec + 0x30) + 0x10) + 0xa18) as u64, &mut mask, &mut table);
    let flags = p.m.r32(g + 8);
    if flags & 4 == 0 {
        let v = p.query(p.base + ENV_TEXTURE, &[p.m.r64(p.m.r64(p.base + ENV_HOLDER)) as u64]);
        set(role(p, 4, 0x10), v, &mut mask, &mut table);
    }
    let mut drawn = 0u32;
    'body: {
        if p.m.r8(p.m.r64(p.m.r64(tiles + 0x58) + 0x10) + 8) & 1 == 0 {
            break 'body;
        }
        let t3 = if p.m.r32(tiles + 0x50) < 3 { 0 } else { p.m.r64(p.m.r64(tiles + 0x58) + 0x10) };
        set(role(p, 5, 0x14), (t3 + 0x3230) as u64, &mut mask, &mut table);
        let min_width = p.m.f32(p.base + MIN_WIDTH);
        let window = |a: f32, b: f32| {
            let lo = fmax(a, 0.0);
            let mut hi = if lo > b { lo } else { b };
            let mut width = hi - lo;
            if width <= 0.0 {
                width = min_width;
                hi = lo + width;
            }
            (hi, width)
        };
        let (far_hi, far_w) = window(p.m.f32(p.base + FAR_A), p.m.f32(p.base + FAR_B));
        let ratio = far_hi / far_w;
        let (near_hi, near_w) = window(p.m.f32(p.base + NEAR_A), p.m.f32(p.base + NEAR_B));
        if ratio.is_nan() {
            break 'body;
        }
        let min_scale = p.m.f32(p.base + MIN_SCALE);
        let s = fmax(scale0, min_scale);
        if (1.0 / (s * far_w)).is_nan() || (near_hi / near_w).is_nan() || (1.0 / (s * near_w)).is_nan() {
            break 'body;
        }
        let level = p.query(p.base + GRID_LEVEL, &[tiles as u64, 2]) as usize;
        let blocks = p.query(p.base + GRID_BLOCKS, &[tiles as u64, 2]) as usize;
        let g = p.m.r64(this + 0x10);
        let ri = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
        let dist = p.m.f32(g + 0x210);
        let scale = p.m.f32((p.m.r64(g + 0x208) as i64 + ri * 0xa8) as usize + 0x4c);
        let far = p.m.f32(p.base + FAR_A);
        let reach = |a: f32, far_first: bool| {
            if a <= 0.0 {
                f32::MAX
            } else if a == f32::MAX {
                0.0
            } else if far_first {
                dist / (a * fmax(scale, min_scale))
            } else {
                dist / (fmax(scale, min_scale) * a)
            }
        };
        let r_far = reach(far, true);
        let r_near = reach(p.m.f32(p.base + NEAR_B), false);
        let cells_i = p.m.i32(p.m.r64(level) + 0x20);
        let cell_size = p.m.f32(tiles + 8);
        bind_slot(p, w, arg, 9, p.m.r64((t98 as i64 + ti * 0x10) as usize + 8), 0)?;
        let n = p.m.r32(level + 8) as usize;
        if n == 0 {
            break 'body;
        }
        let half = cell_size * cells_i as f32;
        let (end, start) = (half + r_far, r_near - half);
        for e in 0..n {
            let l = p.m.r64(level);
            let h = p.m.r16(level + 0xc + e * 4) as u32;
            let (x, y) = (h & 0xff, h >> 8);
            let ci = (p.m.r32(l + 8).wrapping_mul(y).wrapping_add(x)) as i32 as i64;
            let c = (p.m.r64(l + 0x10) as i64 + ci * 0x14) as usize;
            let b = p.m.r8(c + 0xc);
            if b & 1 != 0 || p.m.r8(c + 0xe) < 5 {
                continue;
            }
            let at = p.m.f32(c + 8);
            if end < at {
                break 'body;
            }
            if start > at || b & 0x10 == 0 {
                continue;
            }
            let step = p.m.i32(l + 0x24);
            let sh = p.m.r32(l + 0x1c) & 0x1f;
            let tsh = p.m.r32(tiles + 0x34) & 0x1f;
            let gx = ((x as i32).wrapping_sub(p.m.r16(l + 0x30) as i32).wrapping_add(if (x as i32) < p.m.r16(l + 0x28) as i16 as i32 { step } else { 0 })
                << sh) as i16 as i32
                >> tsh;
            let ext = p.m.i32(tiles + 0x3c);
            let (x0, y0) = (p.m.r16(tiles + 0x44) as i16 as i32, p.m.r16(tiles + 0x46) as i16 as i32);
            if gx < x0 || gx >= ext.wrapping_add(x0) {
                continue;
            }
            let gy = ((y as i32).wrapping_sub(p.m.r16(l + 0x32) as i32).wrapping_add(if (y as i32) < p.m.r16(l + 0x2a) as i16 as i32 { step } else { 0 })
                << sh) as i16 as i32
                >> tsh;
            if gy < y0 || gy >= ext.wrapping_add(y0) {
                continue;
            }
            let arr = p.m.r64(tiles + 0x28);
            if arr == 0 {
                continue;
            }
            let a = (p.m.r16(tiles + 0x48) as i32).wrapping_add(gx) as i16 as i32;
            let bb = (p.m.r16(tiles + 0x4a) as i32).wrapping_add(gy) as i16 as i32;
            let rem = |v: i32| v.wrapping_sub(if ext == 0 { 0 } else { v.wrapping_div(ext) }.wrapping_mul(ext));
            let cell = p.m.r32(tiles + 0x20).wrapping_mul(rem(bb) as u32).wrapping_add(rem(a) as u32) as i32 as i64;
            let t = (arr as i64 + cell * 0x110) as usize;
            if p.m.r8(t) & 2 != 0 || p.m.i8(t + 1) < 0 {
                continue;
            }
            let present = |p: &Port, o: usize| p.query(p.base + TEXTURE_PRESENT, &[(t + o) as u64]) & 1 != 0;
            if !(present(p, 0x10) || present(p, 0x90)) || p.m.i8(t + 2) < 0 {
                continue;
            }
            if !(present(p, 0x30) || present(p, 0xb0)) || p.m.i8(t + 3) < 0 {
                continue;
            }
            if !(present(p, 0x50) || present(p, 0xd0)) {
                continue;
            }
            for (o, alt, n, off) in [(0x10, 0x90, 1, 4), (0x30, 0xb0, 2, 8), (0x50, 0xd0, 3, 0xc)] {
                let src = if present(p, o) { t + o } else { t + alt };
                let v = p.query(p.base + TEXTURE_OF, &[src as u64]);
                set(role(p, n, off), v, &mut mask, &mut table);
            }
            let row = (step as u32).wrapping_mul(y);
            if flags & 4 != 0 {
                let src = if present(p, 0x70) { t + 0x70 } else { t + 0xf0 };
                let v = p.query(p.base + TEXTURE_OF, &[src as u64]);
                set(role(p, 4, 0x10), v, &mut mask, &mut table);
            }
            if mask != 0 {
                let m = mask;
                mask = 0;
                bind_texture_table(p, w, arg, &table, m)?;
            }
            bind_slot(p, w, arg, 8, blocks, row.wrapping_add(x) as i32)?;
            drawn = drawn.wrapping_add(draw_range(p, shader, w));
        }
    }
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(drawn / 3));
    Ok(())
}

const SCENE_HOLDER: usize = 0x462bc08;
const INPUT_HOLDER: usize = 0x462c118;

/// Draws the gated target left to the game: (main+ target, count).
static OTHER: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

pub(super) fn report() -> String {
    let mut g = std::mem::take(&mut *OTHER.lock().unwrap());
    g.sort_by(|a, b| b.1.cmp(&a.1));
    g.iter().map(|(f, n)| format!("main+{f:#x} {n}")).collect::<Vec<_>>().join(", ")
}

/// `a > b` as AArch64 HI after FCMP: greater or unordered.
fn hi(a: f32, b: f32) -> bool {
    !(a <= b)
}

/// The dispatch delegate target main+0x27da4cc(this, arg): the drawable's slot for the context's view (table at
/// this+0x10 +0x1f8, 12-byte entries {_, index, enabled}), drawn through its virtual +0x48 (this, arg, index) while
/// the environment and scene allow it, the slot's record has a grid, is in the area window of the scene mode (+0x28:
/// 4 and 2 have fixed windows), its block is not in use (+0xc negative) and its model is the drawable's.
pub(super) unsafe fn gate(p: &mut Port, this: usize, arg: usize) -> R<()> {
    if p.m.r8(this + 8) & 1 == 0 {
        return Ok(());
    }
    let env = p.m.r64(p.m.r64(p.base + ENV_HOLDER));
    if env == 0 || p.m.r32(env + 8) & 0x81 != 1 {
        return Ok(());
    }
    if p.m.r16(p.m.r64(p.m.r64(p.base + INPUT_HOLDER)) + 0x2b0) & 0xc40 != 0 {
        return Ok(());
    }
    let s = p.m.r64(p.m.r64(p.base + SCENE_HOLDER));
    if s != 0 {
        if !p.m.r32(s + 0x948) & 6 != 0 || p.m.r32(s + 0x1e8) != 0 || p.m.r8(s + 0x98) != 0 {
            return Ok(());
        }
        if p.m.r8(p.m.r64(s + 0x70) + 0x2c4) & 1 == 0 {
            return Ok(());
        }
        let x = p.m.r64(s + 0x8f8);
        if x != 0 && (p.m.r8(x + 0x10) >> 2) & 1 != 0 {
            return Ok(());
        }
    }
    let g = p.m.r64(this + 0x10);
    let view = p.m.r8(p.m.r64(arg) + 0x30) as u32;
    let n = p.m.r32(g + 0x1f0);
    if !(n as i32 > view as i32) {
        return Ok(());
    }
    let slot = p.m.r64(g + 0x1f8) + if n > view { view } else { 0 } as usize * 12;
    if p.m.r8(slot + 8) == 0 {
        return Ok(());
    }
    let index = p.m.i32(slot + 4);
    if index < 0 {
        return Ok(());
    }
    let ri = if p.m.r32(g + 0x200) > index as u32 { index as u32 } else { 0 } as usize;
    let rec = p.m.r64(g + 0x208) + ri * 0xa8;
    if p.m.r64(rec + 0x28) == 0 {
        return Ok(());
    }
    let obj = p.m.r64(rec + 0x10);
    if p.query(p.base + p.vfn(obj, 0x118), &[obj as u64]) & 1 == 0 {
        return Ok(());
    }
    let g = p.m.r64(this + 0x10);
    if p.m.i32(g + 0x214) > 0 {
        return Ok(());
    }
    let (x, y, z) = (p.m.f32(rec + 0x40), p.m.f32(rec + 0x44), p.m.f32(rec + 0x48));
    let f = |o: usize| p.m.f32(g + o);
    let z_max = match p.m.i32(g + 0x28) {
        2 => {
            if !(x >= -555.0) || hi(x, 155.0) || !(y >= 300.0) || hi(y, 900.0) || !(z >= -1241.0) {
                return Ok(());
            }
            -641.0
        }
        4 => {
            if !(f(0x10) <= x) || hi(x, f(0x1c)) || !(y >= 490.0) || !(y <= f(0x20)) || hi(f(0x18), z) {
                return Ok(());
            }
            f(0x24)
        }
        _ => {
            if hi(f(0x10), x) || hi(x, f(0x1c)) || hi(f(0x14), y) || hi(y, f(0x20)) || hi(f(0x18), z) {
                return Ok(());
            }
            f(0x24)
        }
    };
    if hi(z, z_max) {
        return Ok(());
    }
    if p.m.i32(p.m.r64(rec + 0x18) + 0xc) >= 0 {
        return Ok(());
    }
    if p.m.r64(p.m.r64(arg + 8) + 0x68) != p.m.r64(p.m.r64(this + 0x18) + 0x50) {
        return Ok(());
    }
    let target = p.vfn(this, 0x48);
    if target == 0x18ccb80 {
        return draw(p, this, arg, index as u32);
    }
    {
        let mut o = OTHER.lock().unwrap();
        match o.iter_mut().find(|e| e.0 == target) {
            Some(e) => e.1 += 1,
            None => o.push((target, 1)),
        }
    }
    p.helper("grid draw", target, &[this as u64, arg as u64, index as u32 as u64])?;
    Ok(())
}
