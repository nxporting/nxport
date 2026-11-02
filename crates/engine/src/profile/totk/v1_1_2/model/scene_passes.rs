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
