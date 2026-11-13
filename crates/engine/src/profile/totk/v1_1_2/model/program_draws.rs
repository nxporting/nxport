// Full-screen draws of one agl program (the shape at streams +0x488) from the game's post passes (main+0xc4f7c8 and
// main+0xc555c4).

use super::draw::{Port, R};
use super::lights::{stream_draw, STREAMS};
use super::texture_slots::{alloc_target, assign, barrier_if_slot};
use super::vfx::{program_apply, program_texture};

/// sead::GraphicsContext's constructor (the default render state, 0x74 bytes).
const GRAPHICS_CONTEXT_INIT: usize = 0x688a38;

/// main+0xc57014(w, prog, tex): `tex` at the program's texture slot 0, then the full-screen shape.
pub(super) unsafe fn textured(p: &mut Port, w: usize, prog: usize, tex: usize) -> R<()> {
    program_texture(p, prog, w, 0, tex)?;
    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x488, w);
    Ok(())
}

/// main+0xc55378(this, w, tex, color, blend): the shader at +0x858 applied, its program's (+0x860) uniform 5 set to
/// `color` premultiplied by its alpha, a default render state with blend functions 0x61610202 (constant colour
/// `blend`, premultiplied) applied, then main+0xc57014.
pub(super) unsafe fn tinted(p: &mut Port, this: usize, w: usize, tex: usize, color: usize, blend: usize) -> R<()> {
    program_apply(p, p.m.r64(this + 0x858 + 8), w)?;
    let prog = p.m.r64(this + 0x860);
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let premultiplied = |p: &Port, c: usize| {
        let a = p.m.f32(c + 12);
        [p.m.f32(c) * a, a * p.m.f32(c + 4), a * p.m.f32(c + 8), a]
    };
    let mut bytes = [0u8; 16];
    for (k, v) in premultiplied(p, color).iter().enumerate() {
        bytes[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    super::cubemap::set_uniform(p, prog, w, 5, &bytes);
    let mut g = [0u32; 0x20];
    let gp = g.as_mut_ptr() as usize;
    p.query(p.base + GRAPHICS_CONTEXT_INIT, &[gp as u64]);
    g[0] &= 0xffff_0000;
    for (k, v) in premultiplied(p, blend).iter().enumerate() {
        g[2 + k] = v.to_bits();
    }
    g[10] = 0x6161_0202;
    p.apply_graphics_context(gp, w);
    textured(p, w, prog, tex)
}

const OFFSCREEN_SETUP: usize = 0xff07a0;
const SSAO_ALLOC: usize = 0xf817bc;
const GATHER: usize = 0x7f0354;
/// [[holder]]: the temporary target pool (the gather's object).
const TARGET_POOL: usize = 0x4634e70;
/// "blur", the temporary target's name.
const TARGET_NAME: usize = 0x37828af;

/// A one-float uniform draw: uniform 6 = `v`, the state's texture.
unsafe fn scaled_draw(p: &mut Port, w: usize, prog: usize, tex: usize, v: [f32; 4]) -> R<()> {
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let mut bytes = [0u8; 16];
    for (k, x) in v.iter().enumerate() {
        bytes[k * 4..k * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    super::cubemap::set_uniform(p, prog, w, 6, &bytes);
    textured(p, w, prog, tex)
}

/// main+0xc555c4(this, w, index, n): the frame record `index` (0x860 bytes from [this+0x5c8], 0 past the count +0x5c0)
/// at level n+1 of its size (+0x580 / +0x582, class +0x58a) as the pass size; its texture states A (+0x458) and B
/// (+0x550) and a temporary target T ("blur", format this +0x7d4):
///  1. A drawn through [this+0x818] into the record's buffer (+0x3c0), unbound, barrier;
///  2. T allocated, A = B, B = T, [this+0x830] drawn (uniform 6 = 1 / width) with A, unbound, barrier;
///  3. A = T, [this+0x848] drawn (uniform 6 = 1 / height) with A, unbound, barrier; A = B; the depth-of-field gather.
/// With this +0x7d0 bit 6 the levels are kept in the record's words (+0x520 / +0x524 / +0x528, flags +0x53c / +0x610)
/// instead of the game's offscreen setup (main+0xff07a0) and SSAO texture calls (main+0xf817bc, the record's +0x548
/// virtual +0x10). Applying only: everything past the program checks is past the commit point.
pub(super) unsafe fn post_pass(p: &mut Port, this: usize, w: usize, index: u32, n: u32) -> R<()> {
    let index = if p.m.r32(this + 0x5c0) <= index { 0 } else { index } as i32 as isize as usize;
    let f = p.m.r64(this + 0x5c8).wrapping_add(index.wrapping_mul(0x860));
    let (a, b) = (f + 0x458, f + 0x550);
    let (rb, ssao) = (f + 0x3c0, f + 0x548);
    let n1 = n.wrapping_add(1);
    let deferred = (p.m.r8(this + 0x7d0) >> 6) & 1 != 0;
    for o in [0x818, 0x830, 0x848] {
        let prog = p.m.r64(this + o);
        if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
            return Err("agl program setup pending");
        }
    }
    p.m.commit();
    // commit
    let mut g = [0u32; 0x20];
    p.query(p.base + GRAPHICS_CONTEXT_INIT, &[g.as_mut_ptr() as u64]);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);

    let level: u32 = if !deferred {
        let src = p.m.r64(f + 0x430 + n as usize * 8);
        p.helper("offscreen buffer setup", OFFSCREEN_SETUP, &[a as u64, src as u64])?;
        let src = p.m.r64(f + 0x430 + n1 as usize * 8);
        p.helper("ssao texture allocation", SSAO_ALLOC, &[ssao as u64, src as u64])?;
        p.m.r8(f + 0x616) as u32
    } else {
        p.m.w::<u32>(f + 0x528, 0);
        p.m.w::<f32>(f + 0x520, n as i32 as f32);
        p.m.w::<f32>(f + 0x524, n as i32 as f32);
        p.m.fetch_or32(f + 0x53c, 8);
        let cur = p.m.r8(f + 0x616) as u32;
        if n1 != cur {
            p.m.fetch_or32(f + 0x610, 1);
            p.m.w::<u8>(f + 0x616, n1 as u8);
            n1
        } else {
            cur
        }
    };

    // The pass size at this level.
    let sh = level & 0x1f;
    let h16 = p.m.r16(f + 0x582);
    let uw = ((p.m.r16(f + 0x580) as u32 >> sh) as u16).max(1);
    let uh = if p.m.r16(f + 0x58a) != 3 { 1 } else { h16 }.max((h16 as u32 >> sh) as u16);
    let (fw, fh) = (uw as f32, uh as f32);
    p.m.w::<f32>(f + 0x3c8, fw);
    p.m.w::<f32>(f + 0x3cc, fh);
    p.m.w::<u32>(f + 0x3d0, 0);
    p.m.w::<f32>(f + 0x3d4, if uh == 0 { fh } else { 0.0 });
    p.m.w::<f32>(f + 0x3d8, fw);
    p.m.w::<f32>(f + 0x3dc, if uh == 0 { 0.0 } else { fh });
    let vp = super::viewport::full(p, fw, fh);

    // 1.
    super::targets::bind(p, rb, w)?;
    super::viewport::apply(p, vp.as_ptr() as usize, w, rb);
    program_apply(p, p.m.r64(this + 0x818), w)?;
    textured(p, w, p.m.r64(this + 0x818), a)?;
    super::targets::unbind(p, w)?;
    barrier_if_slot(p, f + 0x5a8, w);

    // 2.
    let t = alloc_target(p, w, TARGET_NAME, p.m.r32(this + 0x7d4), fw as u32, fh as u32)?;
    assign(p, a, b)?;
    p.m.fetch_or32(f + 0x53c, 1);
    let keep: u8 = if !deferred {
        p.m.r8(f + 0x616)
    } else {
        p.m.w::<u32>(f + 0x528, 0);
        p.m.w::<f32>(f + 0x520, n1 as i32 as f32);
        p.m.w::<f32>(f + 0x524, n1 as i32 as f32);
        p.m.fetch_or32(f + 0x53c, 8);
        if p.m.r8(f + 0x616) != 0 {
            p.m.fetch_or32(f + 0x610, 1);
            p.m.w::<u8>(f + 0x616, 0);
        }
        0
    };
    let size14 = p.m.r16(f + 0x614);
    assign(p, b, t)?;
    p.m.fetch_or32(f + 0x610, 4);
    if p.m.r16(f + 0x614) != size14 {
        p.m.fetch_or32(f + 0x610, 2);
        p.m.w::<u16>(f + 0x614, size14);
    }
    if p.m.r8(f + 0x616) != keep {
        p.m.fetch_or32(f + 0x610, 1);
        p.m.w::<u8>(f + 0x616, keep);
    }
    p.helper("ssao step", p.vfn(ssao, 0x10), &[ssao as u64])?;
    super::targets::bind(p, rb, w)?;
    program_apply(p, p.m.r64(this + 0x830), w)?;
    scaled_draw(p, w, p.m.r64(this + 0x830), a, [1.0 / fw, 0.0, 0.0, 0.0])?;
    super::targets::unbind(p, w)?;
    barrier_if_slot(p, f + 0x5a8, w);
    if !deferred {
        p.helper("ssao texture allocation", SSAO_ALLOC, &[ssao as u64, a as u64])?;
    } else {
        assign(p, b, a)?;
        p.m.fetch_or32(f + 0x610, 4);
        if p.m.r16(f + 0x614) != 0 {
            p.m.fetch_or32(f + 0x610, 2);
            p.m.w::<u16>(f + 0x614, 0);
        }
        if n1 != p.m.r8(f + 0x616) as u32 {
            p.m.fetch_or32(f + 0x610, 1);
            p.m.w::<u8>(f + 0x616, n1 as u8);
        }
        p.helper("ssao step", p.vfn(ssao, 0x10), &[ssao as u64])?;
        p.m.w::<u64>(f + 0x520, 0);
        p.m.w::<u32>(f + 0x528, 0);
        p.m.fetch_or32(f + 0x53c, 8);
    }

    // 3.
    assign(p, a, t)?;
    p.m.fetch_or32(f + 0x53c, 1);
    super::targets::bind(p, rb, w)?;
    super::viewport::apply(p, vp.as_ptr() as usize, w, rb);
    program_apply(p, p.m.r64(this + 0x848), w)?;
    scaled_draw(p, w, p.m.r64(this + 0x848), a, [0.0, 1.0 / fh, 0.0, 0.0])?;
    super::targets::unbind(p, w)?;
    barrier_if_slot(p, f + 0x5a8, w);
    assign(p, a, b)?;
    p.m.fetch_or32(f + 0x53c, 1);
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    p.helper("depth of field gather", GATHER, &[pool as u64, t as u64])?;
    Ok(())
}
