// agl::env::CubeMapMgr::UpdateAgent's step, main+0xcc8160(this, w): the agent's user callback, the render buffer's
// resolve (main+0xbda640: the temporary texture at +0x1d8 drawn into the buffer's colour target) and finish
// (main+0xbda574), then the step counter and, from the sixth step on, the agent's backend (main+0xcc913c, left to the
// game: the mip filter, irradiance and HDR encode, once per update).

use super::cubemap::transformed_quad;
use super::draw::{Port, R};
use super::mip_copy::buffer_setup;
use super::scene_passes::default_state;
use super::targets;
use super::texture_slots::{barrier_if_slot, release, release_sampler, texture_assign, Host};
use super::viewport;
use super::vfx::program_apply;

/// [[var]]: the program holder (+8 a version, +0x10 the programs; the resolve's program at +0x28 from version 6).
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[var]]: the temporary target pool; main+0x7f0354(pool, target) gives a target back.
const TARGET_POOL: usize = 0x4634e70;
const GATHER: usize = 0x7f0354;
/// agl::TextureData's constructor (0x100 bytes on the stack).
const TEXTURE_CTOR: usize = 0xbf2180;
/// The two 3x4 matrices (0x30 bytes each, picked by bit 0 of the resolve's third argument) and the 4-vector table.
const MATRICES: usize = 0x3841550;
const QUAD_ROWS: usize = 0x462bc10;
/// The agent's backend.
const BACKEND: usize = 0xcc913c;

/// main+0xbda640(this, w, kind): with +0x5b8 bit 5 on the render buffer `this`, the buffer unbound; with the temporary
/// texture (+0x1d0, its state at +0x1d8) set, a barrier, the texture copied to a stack texture and drawn as a
/// transformed quad (the matrix of `kind` & 1) into the colour target (+0xf0) at the remembered level (+0x360) and
/// face (+0x35c + 6 * +0x358) with the default state minus blending, then unbound again and the stack texture's
/// references given back; the remembered face / level reset. Not predictable with the temporary texture set (the stack
/// texture is finished by the game's call). The commit comes before that finish.
pub(super) unsafe fn resolve(p: &mut Port, this: usize, w: usize, kind: u32) -> R<()> {
    if p.m.r8(this + 0x5b8) & 0x20 == 0 {
        return Ok(());
    }
    targets::unbind(p, w)?;
    if p.m.r64(this + 0x1d0) == 0 {
        p.m.w::<u64>(this + 0x358, u64::MAX);
        p.m.w::<u32>(this + 0x360, u32::MAX);
        return Ok(());
    }
    if p.m.predict() {
        return Err("resolve texture finished by the call");
    }
    // The program the quad is drawn with (none before holder version 6: the game would not get far).
    let set = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    if p.m.r32(set + 8) < 6 {
        return Err("resolve program missing");
    }
    let prog = p.m.r64(p.m.r64(set + 0x10) + 0x28);
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    barrier_if_slot(p, this + 0x438, w);
    let tex = Host::new(p, 0x100);
    p.query(p.base + TEXTURE_CTOR, &[tex.addr() as u64]);
    let r = resolve_draw(p, this, w, kind, prog, tex.addr());
    release_sampler(p, tex.addr() + 0xea);
    release(p, tex.addr());
    tex.release(p);
    r?;
    p.m.w::<u64>(this + 0x358, u64::MAX);
    p.m.w::<u32>(this + 0x360, u32::MAX);
    Ok(())
}

unsafe fn resolve_draw(p: &mut Port, this: usize, w: usize, kind: u32, prog: usize, tex: usize) -> R<()> {
    // The temporary texture's state copied to the stack texture, marked as needing its finish.
    texture_assign(p, tex, this + 0x1d8)?;
    let level = p.m.r32(this + 0x360);
    let layer = p.m.r32(this + 0x35c).wrapping_add(p.m.r32(this + 0x358).wrapping_mul(6));
    let extra = [0u64; 3];
    buffer_setup(p, this, w, this + 0xf0, level, layer, 0, extra.as_ptr() as usize, 1, 0)?;
    // The default state without blending, colour mask and the byte at +0x65.
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[0x19] &= !0xff00;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let vp = viewport::full(p, p.m.f32(this + 0x370), p.m.f32(this + 0x374));
    viewport::apply(p, vp.as_ptr() as usize, w, this + 0x368);
    program_apply(p, prog, w)?;
    let ones = [1.0f32; 4];
    let m = p.base + MATRICES + (kind as usize & 1) * 0x30;
    let rows = p.pfn(QUAD_ROWS);
    // The quad's texture finish is the game's call: everything that can stop is behind us.
    p.m.commit();
    transformed_quad(p, w, prog, tex, m, rows, ones.as_ptr() as usize)?;
    targets::unbind(p, w)?;
    barrier_if_slot(p, this + 0x438, w);
    Ok(())
}

/// main+0xbda574(this, w): with +0x5b8 bit 3, a barrier after the colour target (when it has a slot), the temporary
/// targets at +0x1d0 / +0x288 given back to the pool (the game's call, past the commit) and the setup state cleared.
unsafe fn finish_buffers(p: &mut Port, this: usize, w: usize) -> R<()> {
    if p.m.r8(this + 0x5b8) & 8 == 0 {
        return Ok(());
    }
    barrier_if_slot(p, this + 0x438, w);
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    for o in [0x1d0, 0x288] {
        let t = p.m.r64(this + o);
        if t != 0 {
            p.m.commit();
            p.helper("temporary target given back by the call", GATHER, &[pool as u64, t as u64])?;
        }
    }
    p.m.w::<u64>(this + 0x1d0, 0);
    p.m.w::<u64>(this + 0x288, 0);
    p.m.w::<u8>(this + 0x5b8, p.m.r8(this + 0x5b8) & 0x87);
    p.m.w::<u32>(this + 0x290, 0);
    p.m.w::<u64>(this + 0x298, 0);
    p.m.w::<u64>(this + 0x2a0, 0);
    Ok(())
}

/// main+0xcc8160(this, w): the agent's step on the render buffer [[this+0x150]+0x10]: the callback at [[this+0x1a0]+0x428]
/// (if any: called with the buffer's +0x368 and the step count) first, the resolve (kind 1) and the finish; with no
/// pending work in [this+0x160] (+0x160 / +0x168 both null) the agent is marked done (+0x199), else the step count
/// (+0x18c) goes up and from its sixth value with +0x198 set the backend runs (the game's, past the commit).
pub(super) unsafe fn agent_step(p: &mut Port, this: usize, w: usize) -> R<()> {
    let buf = p.m.r64(p.m.r64(this + 0x150) + 0x10);
    let cb = p.m.r64(p.m.r64(this + 0x1a0) + 0x428);
    if cb != 0 {
        // The callback runs first; it may record commands, so it is the game's call with nothing of ours before it.
        if p.m.stops() {
            return Err("cubemap update callback by the call");
        }
        let arg: [u64; 2] = [(buf + 0x368) as u64, p.m.r32(this + 0x18c) as u64];
        let f = p.base + p.vfn(cb, 0);
        p.call(f, &[cb as u64, w as u64, arg.as_ptr() as u64]);
    }
    resolve(p, buf, w, 1)?;
    finish_buffers(p, buf, w)?;
    let agent = p.m.r64(this + 0x160);
    if p.m.r64(agent + 0x168) == 0 && p.m.r64(agent + 0x160) == 0 {
        p.m.w::<u8>(this + 0x199, 1);
        return Ok(());
    }
    let n = p.m.r32(this + 0x18c).wrapping_add(1);
    p.m.w::<u32>(this + 0x18c, n);
    if n > 5 && p.m.r8(this + 0x198) != 0 {
        p.m.commit();
        p.helper("cubemap update backend by the call", BACKEND, &[this as u64, w as u64])?;
    }
    Ok(())
}
