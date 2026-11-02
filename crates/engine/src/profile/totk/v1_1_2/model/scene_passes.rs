// Small full-screen passes of the game's scene renderer: each a program with its textures and blocks, a render state
// built from sead::GraphicsContext's defaults, and the full-screen triangle (shapes +0x10e8, streams +0x488).

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, SHAPES, STREAMS};
use super::vfx::{program_apply, program_block, program_storage_block, program_texture};

/// sead::GraphicsContext's constructor (0x74 bytes of default state).
const GRAPHICS_CONTEXT_INIT: usize = 0x688a38;
/// [[holder]+0x40]: the default texture of the pass below without its own.
const DEFAULT_TEXTURES: usize = 0x4631740;

/// A default render state as words, for the caller's changes.
unsafe fn default_state(p: &Port) -> [u32; 0x20] {
    let mut g = [0u32; 0x20];
    p.query(p.base + GRAPHICS_CONTEXT_INIT, &[g.as_mut_ptr() as u64]);
    g
}

unsafe fn fullscreen(p: &mut Port, w: usize) {
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);
    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x488, w);
}

/// main+0x955ff4(this, w, view, ctx), with +0xa bit 3: the program at [[+0xa8]+8]+0x10 with texture 5 (the view's
/// record +0x5d0 in +0x18 when ctx +0x18 is set, else the default), the scene block (ctx[0] +0x18, +0x390) and the
/// storage block ctx[2] +0x1c0 / +0x248; blending into targets 0 (and 2 with ctx +0x18) only (+0x1c), no depth.
pub(super) unsafe fn view_composite(p: &mut Port, this: usize, w: usize, view: u32, ctx: usize) -> R<()> {
    let v = if p.m.r32(this + 0x10) > view { view } else { 0 } as i32 as isize as usize;
    if (p.m.r8(this + 10) >> 3) & 1 == 0 {
        return Ok(());
    }
    let records = p.m.r64(this + 0x18);
    let c0 = p.m.r64(ctx);
    let scene = p.m.r64(c0 + 0x18);
    p.m.w::<u8>(scene + 0x421, p.m.r8(scene + 0x421) | 1);
    let scene = p.m.r64(c0 + 0x18) + 0x390;
    let flag = p.m.r8(ctx + 0x18) != 0;
    let storage = p.m.r64(ctx + 0x10) + if flag { 0x1c0 } else { 0x248 };
    let prog = p.m.r64(p.m.r64(p.m.r64(this + 0xa8) + 8) + 0x10);
    program_apply(p, prog, w)?;
    let tex = if flag {
        records.wrapping_add(v.wrapping_mul(0xb90)) + 0x5d0
    } else {
        p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES) + 0x40)
    };
    program_texture(p, prog, w, 5, tex)?;
    program_block(p, prog, w, 2, scene, 0)?;
    program_storage_block(p, prog, w, 1, storage, 0)?;
    let mut g = default_state(p);
    g[0] = (g[0] & 0xffff_0000) | 1;
    g[1] = 0;
    g[7] = if flag { 0xf0000f } else { 0xf };
    g[0x19] = (g[0x19] & 0xffff_ff00) | 3;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    fullscreen(p, w);
    Ok(())
}

/// main+0x962f54(this, w, view, prog, scene): the scene blocks shared by several passes: texture 3 (scene +0xd0), the
/// pass block (+0x2d0), the view's record block (+0x360, 0xe8d8 each, +0xe5e0), the scene block (+0x18 +0x390), the
/// scene's per-frame block (+0x5a0; the first user each frame fills it with main+0x765148) and the storage block
/// +0x1c0.
unsafe fn scene_blocks(p: &mut Port, this: usize, w: usize, view: u32, prog: usize, scene: usize) -> R<()> {
    let v = if p.m.r32(this + 0x358) > view { view } else { 0 } as i32 as isize as usize;
    program_texture(p, prog, w, 3, p.m.r64(scene + 0xd0))?;
    program_block(p, prog, w, 1, this + 0x2d0, 0)?;
    program_block(p, prog, w, 0, p.m.r64(this + 0x360).wrapping_add(v.wrapping_mul(0xe8d8)) + 0xe5e0, 0)?;
    let s = p.m.r64(scene + 0x18);
    p.m.w::<u8>(s + 0x421, p.m.r8(s + 0x421) | 1);
    program_block(p, prog, w, 2, p.m.r64(scene + 0x18) + 0x390, 0)?;
    let frame = p.m.r64(scene + 0x5a0);
    if p.m.r32(frame + 0xc8) == 0 {
        if p.m.predict() {
            return Err("scene frame block filled by the call");
        }
        p.m.fetch_or32(frame + 0xc8, 1);
        p.call(p.base + FILL_FRAME_BLOCK, &[scene as u64, 0]);
    } else {
        p.m.fetch_or32(frame + 0xc8, 1);
    }
    program_block(p, prog, w, 3, frame, 0)?;
    program_storage_block(p, prog, w, 0, this + 0x1c0, 0)?;
    Ok(())
}

const FILL_FRAME_BLOCK: usize = 0x765148;

/// main+0x957448(this, w, view, ctx), with +0x352 bit 0: the second program of [+0x368]+8 with the scene blocks, the
/// view's textures (ctx[1] record +0x2118 -> +0x18 / +0x208, the record +0x7f0), storage blocks (+0x1c0 / +0x248 by
/// ctx +0x18, ctx[2] +0x7328 per view), a no-depth render state blending into target 0, then the quad shape instanced
/// over the record's grid (+0x21a8 * +0x21ac).
pub(super) unsafe fn view_grid(p: &mut Port, this: usize, w: usize, view: u32, ctx: usize) -> R<()> {
    if p.m.r8(this + 0x352) & 1 == 0 {
        return Ok(());
    }
    let a1 = p.m.r64(ctx + 8);
    let v1 = if p.m.r32(a1 + 8) > view { view } else { 0 } as i32 as isize as usize;
    let rec = p.m.r64(a1 + 0x10).wrapping_add(v1.wrapping_mul(0x21b0));
    let t = p.m.r64(rec + 0x2118);
    let storage = if p.m.r8(ctx + 0x18) == 0 { this + 0x248 } else { this + 0x1c0 };
    let progs = p.m.r64(p.m.r64(this + 0x368) + 8);
    let prog = p.m.r64(progs + 0x20) + if p.m.r32(progs + 0x18) > 1 { 0x158 } else { 0 };
    program_apply(p, prog, w)?;
    scene_blocks(p, this, w, view, prog, p.m.r64(ctx))?;
    program_texture(p, prog, w, 0, t + 0x18)?;
    program_texture(p, prog, w, 1, t + 0x208)?;
    program_texture(p, prog, w, 2, rec + 0x7f0)?;
    program_storage_block(p, prog, w, 0, storage, 0)?;
    let a2 = p.m.r64(ctx + 0x10);
    let v2 = if p.m.r32(a2 + 0x10) > view { view } else { 0 } as i32 as isize as usize;
    program_storage_block(p, prog, w, 1, p.m.r64(a2 + 0x18).wrapping_add(v2.wrapping_mul(0x7400)) + 0x7328, 0)?;
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 1;
    g[7] = 0xf;
    g[10] = 0x0101_0909;
    g[0x19] = (g[0x19] & 0xffff_ff00) | 6;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0xd28, w);
    let n = p.m.i32(rec + 0x21ac).wrapping_mul(p.m.i32(rec + 0x21a8)) as u32;
    super::lights::stream_draw_n(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x1e8, w, n);
    Ok(())
}

/// main+0x9737f8(this, w, view, scene): the first program of [+0x368]+8 with the scene blocks, the view record's
/// viewport (+0x360 record +0xe660), a default render state without blending targets (+0x1c = 0), the full-screen
/// triangle.
pub(super) unsafe fn view_fullscreen(p: &mut Port, this: usize, w: usize, view: u32, scene: usize) -> R<()> {
    let v = if p.m.r32(this + 0x358) > view { view } else { 0 } as i32 as isize as usize;
    let rec = p.m.r64(this + 0x360).wrapping_add(v.wrapping_mul(0xe8d8));
    let prog = p.m.r64(p.m.r64(p.m.r64(this + 0x368) + 8) + 0x10);
    program_apply(p, prog, w)?;
    scene_blocks(p, this, w, view, prog, scene)?;
    let vp = super::viewport::full(p, p.m.f32(rec + 0xe668), p.m.f32(rec + 0xe66c));
    super::viewport::apply(p, vp.as_ptr() as usize, w, rec + 0xe660);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[7] = 0;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    fullscreen(p, w);
    Ok(())
}

/// The program set holder ([[[[[[holder]]+0x10]+0x5f8]+8]+0x10]+8) of main+0x95f810.
const PROGRAM_HOLDER: usize = 0x4634e40;

/// main+0x95f810(this, w, view, _, k, flags): layer `k` (0x1298 bytes from this) of an effect drawn when enabled
/// (+0x210, the layer's +0x1518, intensity +0x1938 > 0): a render state from the layer's switches, the program variant
/// they select, its ring block (+0x3e8 per view 0x720, the layer's 0xd0 bytes at +0x180) and blocks, two or four
/// textures by the fade (+0x1578, +0x1598), the optional textures +0x3c0 / +0x3c8, then the layer's shape (flags bit 0:
/// the shape at +0x5298 and its stream +0x478).
pub(super) unsafe fn effect_layer(p: &mut Port, this: usize, w: usize, view: u32, k: i32, flags: u8) -> R<()> {
    let v = if p.m.r32(this + 0x3e0) > view { view } else { 0 } as i32 as isize as usize;
    if p.m.r8(this + 0x210) == 0 {
        return Ok(());
    }
    let kk = k as isize as usize;
    let c = this.wrapping_add(kk.wrapping_mul(0x1298));
    if p.m.r8(c + 0x1518) == 0 || p.m.f32(c + 0x1938) <= 0.0 {
        return Ok(());
    }
    let fading = p.m.r8(c + 0x1578) != 0;
    let fade = p.m.f32(c + 0x1598);
    let partial = fading && 0.0 < fade && fade < 1.0;
    let mut u = (p.m.r8(c + 0x1dd8) != 0) as u32;
    let second = p.m.r8(c + 0x2058) != 0;
    if second {
        u |= 2;
    }
    if p.m.r8(c + 0x2038) != 0 || p.m.r32(this + 0x5780) == 0 {
        u |= 4;
    }
    if p.m.r8(c + 0x2078) != 0 || flags & 1 != 0 {
        u |= 8;
    }
    if partial {
        u |= 0x10;
    }
    let mut g = default_state(p);
    let b = g.as_mut_ptr() as *mut u8;
    *b = ((p.m.r8(this + 0x270) == 0) & (flags & 1 == 0)) as u8;
    *b.add(1) = 0;
    g[7] |= 0xf;
    *b.add(0x28) = 5;
    *b.add(0x29) = if p.m.r8(this + 0x230) != 0 || p.m.r8(this + 0x3d0) != 0 { 1 } else { 5 };
    *b.add(0x2a) = if flags & 1 != 0 { 2 } else { 6 };
    *b.add(0x2b) = 6;
    g[0x16] = p.m.r32(this + 0x57ac);
    g[0x17] = p.m.r32(this + 0x57b0);
    g[0x18] = 0;
    *b.add(0x65) = 0;
    *b.add(0x6d) |= 1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let h = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    let z = p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.m.r64(h + 0x10) + 0x5f8) + 8) + 0x10) + 8);
    let prog = if u == 0 {
        p.m.r64(z + 0x10)
    } else {
        let i = if p.m.r32(z + 0x18) > u - 1 { u - 1 } else { 0 };
        p.m.r64(z + 0x20) + i as usize * 0x158
    };
    program_apply(p, prog, w)?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x12c8, w);
    let rec = p.m.r64(this + 0x3e8).wrapping_add(v.wrapping_mul(0x720));
    let block = p.ring_block_direct(rec.wrapping_add(kk.wrapping_mul(0xd0)) + 0x180, 0xd0);
    program_block(p, prog, w, 0, block, 0)?;
    program_block(p, prog, w, 1, rec.wrapping_add(kk.wrapping_mul(0x80)), 0)?;
    if !fading || !(fade >= 1.0) {
        program_texture(p, prog, w, 0, c + 0x20c0)?;
        program_texture(p, prog, w, 1, c + 0x21b0)?;
        if fading {
            program_texture(p, prog, w, 2, c + 0x22a0)?;
            program_texture(p, prog, w, 3, c + 0x2390)?;
        }
    } else {
        program_texture(p, prog, w, 0, c + 0x22a0)?;
        program_texture(p, prog, w, 1, c + 0x2390)?;
    }
    if second && p.m.r64(this + 0x3c0) != 0 {
        program_texture(p, prog, w, 4, p.m.r64(this + 0x3c0))?;
    }
    if p.m.r64(this + 0x3c8) != 0 {
        program_texture(p, prog, w, 5, p.m.r64(this + 0x3c8))?;
    }
    let (shape, stream) = if flags & 1 != 0 {
        (this + 0x5298, this + 0x5478)
    } else if p.m.r8(c + 0x20b8) == 0 {
        (this + 0x4ad8, this + 0x4e98)
    } else {
        (this + 0x4cb8, this + 0x4ef8)
    };
    shape_state(p, shape, w);
    stream_draw(p, stream, w);
    Ok(())
}
