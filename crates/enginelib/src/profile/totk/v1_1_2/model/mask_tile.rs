// The map mask tile pass, main+0xbdc1d0(this, w, mode, col): a colour target object over a copy of the mask texture's
// state (this+0x3668), its render buffer (size 1 x 1, region 500 x 400), a tile of the buffer (column `col` of 0.1,
// row `mode` of 0.125 from the top) cleared to (0, 0, 0, 1), then the tile's uniform block (main+0xbdf1c0 fills it) drawn
// with the full-screen triangle and the default texture. The stack objects are host objects.

use super::draw::{Port, R};
use super::ring::RING_HOLDER;
use super::scene_passes::{default_state, fullscreen};
use super::targets::{self, CONTEXT_TYPE_GUARD, RENDER_BUFFER_UNBIND};
use super::texture_slots::{copy_target, new_buffer, new_target, release, Host, BUFFER_BYTES, COLOR_VTABLE, TARGET_BYTES};
use super::vfx::{program_apply, program_block, program_texture};
use super::viewport;

/// main+0xbdf1c0(list, block, index, s0, s1, s2): the 0xcb0-byte uniform block of tile `index` (pure: reads the list at
/// this+8, writes the block).
const TILE_BLOCK: usize = 0xbdf1c0;
const BLOCK_BYTES: usize = 0xcb0;
/// The tile's width step (0.1).
const TILE_STEP: usize = 0x380efb8;
/// [[var]+0x18]: the default texture bound at slot 0.
const DEFAULT_TEXTURES: usize = 0x4631740;
const VIEWPORT_VTABLE: usize = 0x46323b0;
const ORIENTATION: usize = 0x46312d8;
/// Target object virtual +0x18 the bind's setup accepts (one level / every level).
const VIEW_SETUP: usize = 0x2a2f550;
const VIEW_SETUP_ALL_LEVELS: usize = 0x2a2f89c;
/// Uniform block virtuals +0x20 (alignment) and +0x48 (bind) the ring blocks have.
const BLOCK_ALIGNMENT: usize = 0x2a30390;
const BLOCK_BIND: usize = 0x2a303b4;

/// The program's setup finished (+0x10 / +0x14).
unsafe fn program_ready(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// The targets of render buffer `rb` can be bound: none locked by another thread, setup virtuals the port knows.
unsafe fn targets_ok(p: &Port, rb: usize) -> R<()> {
    for o in (0x20..=0x60).step_by(8) {
        let t = p.m.r64(rb + o);
        if t == 0 {
            continue;
        }
        let c8 = p.m.r32(t + 0xc8);
        if (c8 as i32) < 0 {
            return Err("render target locked");
        }
        if c8 & 0x7fff_ffff != 0 && !matches!(p.vfn(t, 0x18), VIEW_SETUP | VIEW_SETUP_ALL_LEVELS) {
            return Err("render target setup");
        }
    }
    Ok(())
}

/// A buffer of the context that the bind unbinds and the final unbind binds again.
unsafe fn buffer_ok(p: &Port, rb: usize) -> R<()> {
    if p.vfn(rb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer unbind");
    }
    targets_ok(p, rb)
}

/// Everything after the commit that could stop: the programs, the default texture, the ring's block virtuals, the
/// context's static guard and the buffers the bind and the final unbind handle.
unsafe fn checks(p: &Port, w: usize, prog: usize) -> R<()> {
    program_ready(p, prog)?;
    let tex = p.m.r64(p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES)) + 0x18);
    if p.m.r32(tex + 0xe4) != 0 {
        return Err("agl texture still finishing");
    }
    let ring = p.m.r64(p.m.r64(p.base + RING_HOLDER));
    let template = p.m.r64(ring + 0x50);
    if p.vfn(template, 0x20) != BLOCK_ALIGNMENT || p.vfn(template, 0x48) != BLOCK_BIND {
        return Err("uniform block virtual function");
    }
    if p.m.r8(p.m.r64(p.base + CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    let prev = p.m.r64(w + 0xf8);
    let saved = p.m.r64(w + 0x100);
    if saved != 0 {
        buffer_ok(p, saved)?;
    }
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 && prev != 0 {
        buffer_ok(p, prev)?;
    }
    Ok(())
}

/// main+0xbdc1d0(this, w, mode, col): see the file comment. The commit comes before the first ring block.
pub(super) unsafe fn mask_tile_pass(p: &mut Port, this: usize, w: usize, mode: i32, col: u32) -> R<()> {
    let mut hosts = Vec::new();
    let r = run(p, this, w, mode, col, &mut hosts);
    for h in hosts {
        h.release(p);
    }
    r
}

unsafe fn run(p: &mut Port, this: usize, w: usize, mode: i32, col: u32, hosts: &mut Vec<Host>) -> R<()> {
    let prog = p.m.r64(p.m.r64(p.m.r64(this + 0x3548) + 8) + 0x10);
    checks(p, w, prog)?;

    // The colour target object over a copy of the texture state (its slot reference counted up), the render buffer
    // holding it (size 1 x 1, region (0, 0) - (500, 400)).
    let t = Host::new(p, TARGET_BYTES);
    let t_addr = t.addr();
    hosts.push(t);
    new_target(p, t_addr, COLOR_VTABLE);
    copy_target(p, t_addr, p.m.r64(this + 0x3668))?;
    let rb = Host::new(p, BUFFER_BYTES);
    let rb_addr = rb.addr();
    hosts.push(rb);
    new_buffer(p, rb_addr);
    p.m.w::<f32>(rb_addr + 0x18, 500.0);
    p.m.w::<f32>(rb_addr + 0x1c, 400.0);
    p.m.w::<u64>(rb_addr + 0x20, t_addr as u64);
    targets_ok(p, rb_addr)?;

    // The render state: no depth / blending bits of the default, bit 0 of word 1 set, byte 0x65 clear.
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    *(g.as_mut_ptr() as *mut u8).add(0x65) = 0;
    g[1] |= 1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    targets::bind(p, rb_addr, w)?;

    // The viewport: the tile at column `col` (0.1 wide) and row `mode` (0.125 high, from the top), ordered corners.
    let step = p.m.f32(p.base + TILE_STEP);
    let s1 = (col as f32) * step;
    let s0 = 1.0 - ((mode as u32).wrapping_add(1) as f32 * 0.125);
    let s4 = s1 + step;
    let s2 = s0 + 0.125;
    let (x0, x1) = if s4 > s1 { (s1, s4) } else { (s4, s1) };
    let (y0, y1) = if s2 > s0 { (s0, s2) } else { (s2, s0) };
    let orientation = p.m.i32(p.m.r64(p.base + ORIENTATION));
    let vt = (p.m.r64(p.base + VIEWPORT_VTABLE) + 0x10) as u64;
    let mut vp = [0u32; 10];
    vp[0] = vt as u32;
    vp[1] = (vt >> 32) as u32;
    for (n, v) in [x0, y0, x1, y1].into_iter().enumerate() {
        vp[2 + n] = v.to_bits();
    }
    vp[6] = orientation as u32;
    vp[7] = 0;
    vp[8] = 1f32.to_bits();
    viewport::apply(p, vp.as_ptr() as usize, w, rb_addr);
    let black: [u32; 4] = [0, 0, 0, 1f32.to_bits()];
    targets::clear(p, rb_addr, w, 0, 1, black.as_ptr() as usize, 0, vp.as_ptr() as usize, 1.0)?;

    // The tile's uniform block: filled by the game's pure function, then two words set.
    let blk = Host::new(p, BLOCK_BYTES);
    let b = blk.addr();
    hosts.push(blk);
    let floats = [p.m.f32(this + 0x368c), p.m.f32(this + 0x3690), p.m.f32(this + 0x3694)];
    let index = col.wrapping_add((mode as u32).wrapping_mul(10));
    p.query_f(p.base + TILE_BLOCK, &[(this + 8) as u64, b as u64, index as u64], &floats);
    p.m.w::<f32>(b + 0xc94, floats[0]);
    p.m.w::<u32>(b + 0xc9c, 0);

    program_apply(p, prog, w)?;
    p.m.commit();
    let block = p.ring_block(b, BLOCK_BYTES);
    program_block(p, prog, w, 0, block, 0)?;
    let tex = p.m.r64(p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES)) + 0x18);
    program_texture(p, prog, w, 0, tex)?;
    fullscreen(p, w);
    targets::barrier_targets(p, rb_addr, w);
    targets::unbind(p, w)?;
    // The target's slot reference given back.
    release(p, t_addr + 8);
    Ok(())
}
