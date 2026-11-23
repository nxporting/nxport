// The map mask pass main+0xbdc94c(this, w, index): a compute dispatch with the record's blocks, then (with this +0x28 bit
// 1 and both mesh lists non-empty) two temporary targets of the record's size (+0x90 x +0x94: colour format 0x1d, depth
// format 0x5a) bound as a render buffer (size = the colour target's level size) and cleared (colour and depth, depth
// 1.0). The first mesh list (this +0x58 / +0x60, program of this +0x80) is drawn into it; after the buffer's unbind and
// target barrier the colour target is bound again and the second list (this +0x68 / +0x70, texture slot 0 = a stack
// texture made from the colour state) is drawn; then the unbind, the target objects' slot references and the targets'
// return to the pool. Both lists draw mesh objects (main+0xbddc28, `mesh`): shape states of the global at [0x46323a0],
// the object's uniform block and the index streams of the global at [0x46323a8], once, or twice with another render
// state when the object's +8 is 0 and the flag is set. The stack objects are host objects.
//
// Order of the port: the compute part is emitted first (undoable), then everything that can stop the rest (programs, the
// context's buffers, the stack objects' virtual functions) is checked, then the commit, then the allocations.

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw_n};
use super::scene_passes::default_state;
use super::targets::{self, CONTEXT_TYPE_GUARD, RENDER_BUFFER_UNBIND};
use super::texture_slots::{alloc_target, copy_target, level_size, new_buffer, new_target, release, release_sampler, set_extent, texture_assign, Host, BUFFER_BYTES, COLOR_VTABLE, DEPTH_VTABLE, TARGET_BYTES};
use super::vfx::{program_apply, program_block, program_storage_block, program_texture};
use super::viewport;

/// [[var]] x2: the global holding the shape states (+0x5a8, +0x968, +0x3c8 by mesh kind) and the one holding the index
/// streams.
const SHAPES_VAR: usize = 0x46323a0;
const STREAMS_VAR: usize = 0x46323a8;
/// [[var]]: the temporary target pool; main+0x7f0354(pool, target) gives a target back.
const TARGET_POOL: usize = 0x4634e70;
const GATHER: usize = 0x7f0354;
/// The temporary targets' names (colour, depth).
const NAME_COLOR: usize = 0x36e2576;
const NAME_DEPTH: usize = 0x377ee69;
/// "agl::TextureData", "agl::TextureSampler(from texture)".
const TEXTURE_NAME: usize = 0x37ccf1f;
const SAMPLER_NAME: usize = 0x37a908e;
/// Target object virtual +0x10 (colour / depth setup) and +0x18 (the bind's view setup).
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;
const VIEW_SETUP: usize = 0x2a2f550;
const VIEW_SETUP_ALL_LEVELS: usize = 0x2a2f89c;

/// The program still being set up (+0x10 / +0x14).
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

/// The context's static guard and the buffers its binds and unbinds handle.
unsafe fn context_ok(p: &Port, w: usize) -> R<()> {
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

/// The record of `index` (the list at this +0x38, 0x98 bytes each, the count at +0x30; an index past it reads record 0).
unsafe fn record(p: &Port, this: usize, index: u32) -> usize {
    let i = if p.m.r32(this + 0x30) > index { index } else { 0 };
    p.m.r64(this + 0x38).wrapping_add((i as i32 as isize as usize).wrapping_mul(0x98))
}

/// The program set [[this +0x80] +8].
unsafe fn program_set(p: &Port, this: usize) -> usize {
    p.m.r64(p.m.r64(this + 0x80) + 8)
}

/// Program `flag` of the set's array (+0x20, 0x158 bytes each, count +0x18; index 0 when past the count).
unsafe fn set_program(p: &Port, set: usize, flag: u32) -> usize {
    let i = if p.m.r32(set + 0x18) > flag { flag } else { 0 } as usize;
    p.m.r64(set + 0x20) + i * 0x158
}

/// main+0xbddc28(obj, w, slots, flag): one mesh object. Kind (+0xc) 0, 1 or 2 (others draw nothing): the shape state of
/// the shapes global, the object's block (+0x10) at `slots`, then the kind's index stream of the streams global: without
/// the flag once; with it once under render state bit 0 of word 1 and halfword 0x101 when the object's +8 is set, else
/// under halfword 1, then again with that bit cleared on the stream after it.
unsafe fn mesh(p: &mut Port, obj: usize, w: usize, slots: u32, flag: u32) -> R<()> {
    let ty = p.m.r32(obj + 0xc);
    let (shape, first, second) = match ty {
        0 => (0x5a8, 0x2328, 0x2448),
        1 => (0x968, 0x1768, 0x17c8),
        2 => (0x3c8, 0x2d68, 0x2e88),
        _ => return Ok(()),
    };
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES_VAR));
    shape_state(p, shapes + shape, w);
    p.bind_uniform_block(obj + 0x10, w, slots, 0)?;
    let streams = p.m.r64(p.m.r64(p.base + STREAMS_VAR));
    if flag & 1 == 0 {
        stream_draw_n(p, streams + first, w, 1);
        return Ok(());
    }
    let two = p.m.r32(obj + 8) == 0;
    let mut g = default_state(p);
    g[0] = (g[0] & 0xffff_0000) | if two { 1 } else { 0x101 };
    g[1] |= 1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    stream_draw_n(p, streams + first, w, 1);
    if two {
        g[1] &= !1;
        p.apply_graphics_context(g.as_ptr() as usize, w);
        stream_draw_n(p, streams + second, w, 1);
    }
    Ok(())
}

/// The mesh list of `this` at +`count` / +`list` drawn with the program's slots of index 1 (its first entry when the
/// program has only one).
unsafe fn meshes(p: &mut Port, this: usize, w: usize, prog: usize, count: usize, list: usize, flag: u32) -> R<()> {
    let first = p.m.r64(this + list);
    let slots = p.m.r32(p.m.r64(prog + 0x50) + if p.m.r32(prog + 0x48) > 1 { 0x10 } else { 0 });
    for i in 0..p.m.i32(this + count).max(0) as usize {
        mesh(p, p.m.r64(first + i * 8), w, slots, flag)?;
    }
    Ok(())
}

/// main+0xbdd86c(this, w, index, flag): the first mesh list: the program (the one at the set +0x10, or with the flag's
/// bit 0 the set's program 1) applied with the record's block and the storage block this +0x90, then the meshes.
unsafe fn draw_first(p: &mut Port, this: usize, w: usize, index: u32, flag: u32) -> R<()> {
    let rec = record(p, this, index);
    let set = program_set(p, this);
    let prog = if flag & 1 != 0 { set_program(p, set, 1) } else { p.m.r64(set + 0x10) };
    program_apply(p, prog, w)?;
    program_block(p, prog, w, 0, rec + 0x10, 0)?;
    program_storage_block(p, prog, w, 0, this + 0x90, 0)?;
    meshes(p, this, w, prog, 0x58, 0x60, flag & 1)
}

/// main+0xbdda28(this, w, index, tex, flag): the second mesh list: the set's program `flag & 1` applied with the block,
/// the storage block and the texture (when there is one) in slot 0, then the meshes of this +0x68 / +0x70.
unsafe fn draw_second(p: &mut Port, this: usize, w: usize, index: u32, tex: usize, flag: u32) -> R<()> {
    let rec = record(p, this, index);
    let set = program_set(p, this);
    let prog = set_program(p, set, flag & 1);
    program_apply(p, prog, w)?;
    program_block(p, prog, w, 0, rec + 0x10, 0)?;
    program_storage_block(p, prog, w, 0, this + 0x90, 0)?;
    if tex != 0 {
        program_texture(p, prog, w, 0, tex)?;
    }
    meshes(p, this, w, prog, 0x68, 0x70, flag & 1)
}

/// A stack texture with a sampler made from a texture state: the constructor's constants and the state assigned.
struct Sampled(Host);

impl Sampled {
    unsafe fn new(p: &mut Port, src: usize) -> R<Sampled> {
        let h = Host::new(p, 0x100);
        let x = h.addr();
        p.m.w::<i32>(x + 0x58, -1);
        p.m.w::<u8>(x + 0xa1, 1);
        p.m.w::<u64>(x + 0xa8, (p.base + TEXTURE_NAME) as u64);
        p.m.w::<u64>(x + 0xb0, (p.base + SAMPLER_NAME) as u64);
        p.m.w::<u64>(x + 0xb8, 0);
        p.m.w::<u64>(x + 0xc0, 0x3f80_0000_0000_0000);
        p.m.w::<u64>(x + 0xc8, 0x4170_0000_0000_0000);
        p.m.w::<u32>(x + 0xd0, 0);
        p.m.w::<u64>(x + 0xd4, 0x0101_0707_0702_0101);
        p.m.w::<u8>(x + 0xdc, 0);
        p.m.w::<u16>(x + 0xde, 0);
        p.m.w::<u64>(x + 0xe0, 0xff_0504_0302);
        p.m.w::<u8>(x + 0xe8, 0x20);
        p.m.w::<u16>(x + 0xea, 0xffff);
        let s = Sampled(h);
        texture_assign(p, x, src)?;
        Ok(s)
    }

    fn addr(&self) -> usize {
        self.0.addr()
    }

    /// The sampler id and the slot reference given back, the buffer freed.
    unsafe fn release(self, p: &mut Port) {
        release_sampler(p, self.addr() + 0xea);
        release(p, self.addr());
        self.0.release(p);
    }
}

/// main+0x7f0354(pool, target): a temporary target given back.
unsafe fn gather(p: &mut Port, target: usize) -> R<()> {
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    p.helper("mask pass target gather by the call", GATHER, &[pool as u64, target as u64])?;
    Ok(())
}

/// main+0xbdc94c(this, w, index): see the file comment.
pub(super) unsafe fn mask_pass(p: &mut Port, this: usize, w: usize, index: u32) -> R<()> {
    let mut hosts = Vec::new();
    let r = run(p, this, w, index, &mut hosts);
    for h in hosts {
        h.release(p);
    }
    r
}

unsafe fn run(p: &mut Port, this: usize, w: usize, index: u32, hosts: &mut Vec<Host>) -> R<()> {
    // The compute part: the program of this +0x88, the record's block and the storage block, one workgroup.
    let rec = record(p, this, index);
    let prog = p.m.r64(p.m.r64(p.m.r64(this + 0x88) + 8) + 0x10);
    program_ready(p, prog)?;
    program_apply(p, prog, w)?;
    program_block(p, prog, w, 0, rec + 0x10, 0)?;
    program_storage_block(p, prog, w, 0, this + 0x90, 0)?;
    let cmd = p.m.r64(w + 0xb8);
    p.dispatch(cmd, 1, 1, 1);
    if (p.m.r8(this + 0x28) >> 1) & 1 == 0 || p.m.r32(this + 0x58) == 0 || p.m.r32(this + 0x68) == 0 {
        p.m.commit();
        return Ok(());
    }

    // What the rest could stop at: the two mesh programs, the context's buffers, the stack objects' virtuals.
    let width = p.m.r32(rec + 0x90);
    let height = p.m.r32(rec + 0x94);
    let set = program_set(p, this);
    program_ready(p, p.m.r64(set + 0x10))?;
    program_ready(p, set_program(p, set, 0))?;
    context_ok(p, w)?;
    let c = Host::new(p, TARGET_BYTES);
    let c_addr = c.addr();
    hosts.push(c);
    let d = Host::new(p, TARGET_BYTES);
    let d_addr = d.addr();
    hosts.push(d);
    let rb = Host::new(p, BUFFER_BYTES);
    let rb_addr = rb.addr();
    hosts.push(rb);
    new_target(p, c_addr, COLOR_VTABLE);
    new_target(p, d_addr, DEPTH_VTABLE);
    new_buffer(p, rb_addr);
    p.m.w::<u64>(rb_addr + 0x20, c_addr as u64);
    p.m.w::<u64>(rb_addr + 0x60, d_addr as u64);
    buffer_ok(p, rb_addr)?;
    if p.vfn(c_addr, 0x10) != SETUP_COLOR || p.vfn(d_addr, 0x10) != SETUP_DEPTH {
        return Err("target object setup");
    }
    if p.m.predict() {
        return Err("mask pass temporary targets allocated by the call");
    }
    p.m.commit();
    // commit

    let st_c = alloc_target(p, w, NAME_COLOR, 0x1d, width, height)?;
    let st_d = alloc_target(p, w, NAME_DEPTH, 0x5a, width, height)?;
    copy_target(p, c_addr, st_c)?;
    copy_target(p, d_addr, st_d)?;
    // The buffer's extent: the colour target's size at its level; the full viewport over it.
    let (ew, eh) = level_size(p, c_addr, false);
    let (fw, fh) = set_extent(p, rb_addr, ew, eh);
    let vp = viewport::full(p, fw, fh);
    let mut g = default_state(p);
    g[1] &= !1;
    viewport::apply(p, vp.as_ptr() as usize, w, rb_addr);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    // Bound and cleared (colour 0 and depth, all values 1.0).
    let mut ones = [0u32; 0x22];
    for v in ones.iter_mut().take(0x21) {
        *v = 1f32.to_bits();
    }
    targets::bind_clear(p, rb_addr, w, vp.as_ptr() as usize, 0x101, ones.as_ptr() as usize)?;
    draw_first(p, this, w, index, 0)?;
    targets::unbind(p, w)?;
    targets::barrier_targets(p, rb_addr, w);
    // The colour target bound again and sampled by the second list.
    g[0] = (g[0] & 0xffff_0000) | 1;
    g[7] = 0xe;
    *(g.as_mut_ptr() as *mut u8).add(0x65) = 0;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    targets::bind(p, rb_addr, w)?;
    let tex = Sampled::new(p, st_c)?;
    draw_second(p, this, w, index, tex.addr(), 0)?;
    tex.release(p);
    targets::unbind(p, w)?;
    // The target objects' slot references (depth first), the targets back to the pool (colour first).
    release(p, d_addr + 8);
    release(p, c_addr + 8);
    gather(p, st_c)?;
    gather(p, st_d)?;
    Ok(())
}
