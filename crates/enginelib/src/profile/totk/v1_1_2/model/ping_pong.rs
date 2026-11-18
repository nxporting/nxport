// main+0xc24c30(this, w): a pass between two render buffers (+0xb8 and +0xc0, each a render buffer at +0x1d8 with its
// viewport at +0x338 and texture at +0x48), with programs from the input holder's list (+0x2e8: count +0x10, programs
// +0x18). On a reset (+8 bit 0) the first buffer is cleared to (0.5, 0.5, 0, 0). The first program copies the first
// buffer into the second, the second and third programs then draw the uniform blocks at +0x5b40 / +0x5bd8 (counts
// +0x5bc8 / +0x5c60) into it when the count at [+0x2f0] is positive, and the fourth copies the second back.

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw};
use super::vfx::{program_apply, program_block, program_texture};

const ENV_HOLDER: usize = 0x46353f8;
const INPUT_HOLDER: usize = 0x462c118;
/// The render buffer's virtual bind (vtable +0x48): agl::RenderBuffer's, or the subclass's main+0x8974e8.
const RENDER_BUFFER_BIND: usize = 0x7ab980;
const RENDER_BUFFER_BIND_PLAIN: usize = 0x8974e8;

unsafe fn bind(p: &mut Port, rt: usize, w: usize) -> R<()> {
    let rb = rt + 0x1d8;
    match p.vfn(rb, 0x48) {
        RENDER_BUFFER_BIND => super::targets::bind(p, rb, w)?,
        RENDER_BUFFER_BIND_PLAIN => super::targets::bind_plain(p, rb, w)?,
        _ => return Err("render buffer bind (virtual)"),
    }
    super::viewport::apply(p, rt + 0x338, w, rb);
    Ok(())
}

pub(super) unsafe fn draw(p: &mut Port, this: usize, w: usize) -> R<()> {
    let env = p.m.r64(p.m.r64(p.base + ENV_HOLDER));
    if env != 0 && (p.m.r8(env + 8) >> 6) & 1 != 0 {
        return Ok(());
    }
    let flags = p.m.r16(this + 8);
    p.m.w::<u16>(this + 8, flags & 0xfffe);
    if flags & 1 != 0 {
        let rt = p.m.r64(this + 0xb8);
        bind(p, rt, w)?;
        let rt = p.m.r64(this + 0xb8);
        let color = [0.5f32, 0.5, 0.0, 0.0];
        let vp = super::viewport::full(p, p.m.f32(rt + 0x1e0), p.m.f32(rt + 0x1e4));
        super::targets::clear(p, rt + 0x1d8, w, 0, 1, color.as_ptr() as usize, 0, vp.as_ptr() as usize, 1.0)?;
    }
    let list = p.m.r64(p.m.r64(p.m.r64(p.base + INPUT_HOLDER)) + 0x2e8);
    let prog = |p: &Port, k: usize| {
        let n = p.m.r32(list + 0x10) as usize;
        p.m.r64(p.m.r64(list + 0x18) + if n > k { k * 8 } else { 0 })
    };
    p.apply_graphics_context(this + 0xc8, w);
    bind(p, p.m.r64(this + 0xc0), w)?;
    let prog0 = p.m.r64(p.m.r64(list + 0x18));
    program_apply(p, prog0, w)?;
    program_texture(p, prog0, w, 0, p.m.r64(this + 0xb8) + 0x48)?;
    program_block(p, prog0, w, 0, this + 0x248, 0)?;
    shape_state(p, p.m.r64(this + 0x228), w);
    stream_draw(p, p.m.r64(this + 0x238), w);
    if p.m.i32(p.m.r64(this + 0x2f0)) > 0 {
        p.apply_graphics_context(this + 0x13c, w);
        bind(p, p.m.r64(this + 0xc0), w)?;
        for (k, block, count) in [(1usize, 0x5b40usize, 0x5bc8usize), (2, 0x5bd8, 0x5c60)] {
            let pr = prog(p, k);
            program_apply(p, pr, w)?;
            shape_state(p, p.m.r64(this + 0x230), w);
            let stream = p.m.r64(this + 0x240);
            for i in 0..p.m.i32(this + count).max(0) {
                program_block(p, pr, w, 0, this + block, i)?;
                stream_draw(p, stream, w);
            }
        }
        super::targets::barrier_targets(p, p.m.r64(this + 0xc0) + 0x1d8, w);
    }
    p.apply_graphics_context(this + 0x1b0, w);
    bind(p, p.m.r64(this + 0xb8), w)?;
    let n = p.m.r32(list + 0x10);
    let prog3 = p.m.r64(p.m.r64(list + 0x18) + if n < 4 { 0 } else { 0x18 });
    program_apply(p, prog3, w)?;
    program_texture(p, prog3, w, 0, p.m.r64(this + 0xc0) + 0x48)?;
    shape_state(p, p.m.r64(this + 0x228), w);
    stream_draw(p, p.m.r64(this + 0x238), w);
    if p.m.i32(p.m.r64(p.m.r64(this + 0xb8) + 0x1f8) + 0x60) != -1 {
        let bits = super::targets::barrier_bits(p);
        p.barrier(p.m.r64(w + 0xb8), bits);
    }
    Ok(())
}
