// Full-screen draws of one agl program (the shape at streams +0x488) from the game's post passes (main+0xc4f7c8 and
// main+0xc555c4).

use super::draw::{Port, R};
use super::lights::{stream_draw, STREAMS};
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
