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
