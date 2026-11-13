// Two small passes of the game's depth-of-field / edge-adjust chain, both per view with a render buffer of their own:
// main+0xa3e79c (the composite of the cached depth-of-field buffer into the view buffer, one instance per grid cell) and
// main+0xa434a8 (gsys::RenderBufferContext edge adjust: updateNLD, then a full-screen pass into the edge buffer and an
// optional second one). Applying only: the game's own updateNLD, the texture finish and the buffer allocations are side-effecting
// game calls, past the commit point.

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, stream_draw_n, SHAPES, STREAMS};
use super::scene_passes::default_state;
use super::texture_slots::{acquire, alloc_target, copy_target, release, release_sampler, slot_count, texture_assign, Host};
use super::targets;
use super::vfx::{program_apply, program_block, program_storage_block, program_texture, shadow_mask_setup};
use super::viewport;

/// The render buffer context table (*[+0x40] +0x408 / +0x810 entries, count at +0x38).
const BUFFER_TABLE: usize = 0x4639f00;
/// Render buffer virtual +0x28 (the wrapper's unbind, targets::unbind).
const RENDER_BUFFER_UNBIND: usize = 0xbe19ac;
/// "agl::TextureData" and "agl::TextureSampler(from texture)".
const TEXTURE_NAME: usize = 0x37ccf1f;
const SAMPLER_NAME: usize = 0x37a908e;
/// "gsys::RenderBufferContext::EdgeAdjust" / "...::BlurAdjust".
const EDGE_NAME: usize = 0x378b2da;
const BLUR_NAME: usize = 0x379bf44;
/// The default textures of the scene ([[+0x...]] objects at +0x98 and +0x18).
const DEFAULT_TEXTURES: usize = 0x4631740;
/// main+0x7f0354: gives a temporary target back. Table of the composite targets' names (offsets from the table, by k).
const GATHER: usize = 0x7f0354;
const COMPOSITE_NAMES: usize = 0x386dd80;
/// The dynamic texture allocator's owner, its allocation (12 arguments) and the ssao context's texture allocation.
const ALLOCATOR_OWNER: usize = 0x4634e70;
const ALLOCATE_TEXTURE: usize = 0x7f0ad4;
const SSAO_ALLOC_TEXTURE: usize = 0xf817bc;
/// gsys::RenderBufferContext::updateNLD(this, w, view, arg, 0).
const UPDATE_NLD: usize = 0xa441ec;
/// The graphics settings holder (+0x50c bit 1: invalidate shader).
const HOLDER: usize = 0x462bbf0;

fn byte(g: &mut [u32; 0x20], o: usize, v: u8) {
    unsafe { *(g.as_mut_ptr() as *mut u8).add(o) = v }
}

/// A texture's width / height as the game derives them from a buffer record (+0x30 width, +0x32 height, +0x3a kind):
/// the width at least 1, the height as is for kind 3 else at least 1.
unsafe fn record_size(p: &Port, rec: usize) -> (u32, u32) {
    let w = (p.m.r16(rec + 0x30) as u32).max(1);
    let h = p.m.r16(rec + 0x32) as u32;
    let h = if p.m.r16(rec + 0x3a) as i16 == 3 { h } else { h.max(1) };
    (w, h)
}

/// Render-target locks the bind would stop at (a target another thread is setting up).
unsafe fn bind_ok(p: &Port, rb: usize, w: usize) -> R<()> {
    let prev = p.m.r64(w + 0xf8);
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 && prev != 0 && p.vfn(prev, 0x28) != targets::RENDER_BUFFER_UNBIND {
        return Err("render buffer unbind");
    }
    for o in (0x20..=0x60).step_by(8) {
        let t = p.m.r64(rb + o);
        if t != 0 && (p.m.r32(t + 0xc8) as i32) < 0 {
            return Err("render target locked");
        }
    }
    Ok(())
}

/// Handle reference counts over a sequence of releases / takes, to find a take of an unreferenced handle (the game
/// registers it again) before anything is changed.
struct Counts(Vec<(i32, i64)>);

impl Counts {
    unsafe fn get(&mut self, p: &Port, h: i32) -> usize {
        if let Some(i) = self.0.iter().position(|e| e.0 == h) {
            return i;
        }
        self.0.push((h, slot_count(p, h) as i64));
        self.0.len() - 1
    }

    unsafe fn release(&mut self, p: &Port, h: i32) {
        if h != -1 {
            let i = self.get(p, h);
            self.0[i].1 -= 1;
        }
    }

    unsafe fn take(&mut self, p: &Port, h: i32) -> R<()> {
        if h != -1 {
            let i = self.get(p, h);
            if self.0[i].1 == 0 {
                return Err("texture handle without references");
            }
            self.0[i].1 += 1;
        }
        Ok(())
    }
}

/// The context's unbind is possible (the type guard initialised, the buffer's virtual the known one).
unsafe fn unbind_ok(p: &Port) -> R<()> {
    if p.m.r8(p.m.r64(p.base + targets::CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    Ok(())
}

/// main+0xa3e79c(this, w, view, k): the depth-of-field buffer cached at this+0x13b8 (allocated again when it is missing
/// or the view's size changed; its first pass with the storage block is left to the game) drawn into the view's render buffer (this + view * 0x578 + 0x4f0): the
/// buffer context program picked by the view / k entries of the render buffer table, its blocks (+0x1060, the optional
/// storage block +0x10e0), a copy of the texture at +0x208 (with its sampler) in slot 2 and the full-screen quad shape
/// instanced over the grid (+0x1440 * +0x1444) when the storage block is in use.
pub(super) unsafe fn dof_composite(p: &mut Port, this: usize, w: usize, view: i32, k: u32) -> R<()> {
    let vbase = this.wrapping_add((view as isize as usize).wrapping_mul(0x578));
    let fb = vbase + 0x4f0;
    let vp = vbase + 0x560;
    let width = (p.m.f32(vbase + 0x508) - p.m.f32(vbase + 0x500)) as u32;
    let height = (p.m.f32(vbase + 0x50c) - p.m.f32(vbase + 0x504)) as u32;
    // The cached buffer must fit, else it is given back and a new one allocated.
    let rec = p.m.r64(this + 0x13b8);
    let fits = rec != 0 && record_size(p, rec) == (width, height);
    if !fits && p.m.r64(this + 0x10e0) != 0 && p.m.r8(this + 8) & 6 == 2 {
        return Err("depth of field buffer first pass");
    }
    // The program of the table entry.
    let t = p.m.r64(p.m.r64(p.base + BUFFER_TABLE));
    if p.m.r32(t + 0x38) < 4 {
        return Err("render buffer table");
    }
    let table = p.m.r64(p.m.r64(t + 0x40) + 0x408 + 8);
    let n = p.m.r32(table + 0x28);
    let arr = p.m.r64(table + 0x30);
    let kk = this.wrapping_add((k as usize).wrapping_mul(0x14));
    let base_slot = p.m.r16(arr + 0x20) as u32;
    let mut extra = 0u32;
    if p.m.i8(kk + 0x13d0) != -1 {
        extra = p.m.r16(arr + (n > 1) as usize * 0x28 + 0x20) as u32;
    }
    let fixed = p.m.r8(kk + 0x13d2);
    let tint = p.m.r8(kk + 0x13d1);
    let a3 = p.m.r16(arr + if n >= 3 { 0x50 } else { 0 } + 0x20) as u32;
    let a4 = p.m.r16(arr + if n >= 4 { 0x78 } else { 0 } + 0x20) as u32;
    let storage = p.m.r64(this + 0x10e0);
    let has2 = storage != 0 && p.m.r8(this + 8) & 2 != 0;
    let instances = if has2 { (p.m.i32(this + 0x1444).wrapping_mul(p.m.i32(this + 0x1440))) as u32 } else { 1 };
    let sum = extra
        .wrapping_add(base_slot.wrapping_mul(k))
        .wrapping_add(0u32.wrapping_sub((fixed & 1) as u32) & a3)
        .wrapping_add(if has2 { a4 } else { 0 }) as i32;
    let prog = if sum < 1 {
        p.m.r64(table + 0x10)
    } else {
        let i = (sum - 1) as u32;
        let i = if p.m.r32(table + 0x18) <= i { 0 } else { i };
        p.m.r64(table + 0x20) + i as usize * 0x158
    };
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    // The texture copy takes a reference to the texture's handle.
    let handle = p.m.i32(this + 0x260);
    let mut counts = Counts(Vec::new());
    counts.take(p, handle)?;
    if p.vfn(fb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    unbind_ok(p)?;
    bind_ok(p, fb, w)?;
    if p.m.predict() {
        return Err("composite texture finish");
    }
    p.m.commit();
    // commit

    if !fits {
        let pool = p.m.r64(p.m.r64(p.base + ALLOCATOR_OWNER));
        if rec != 0 {
            p.helper("depth of field gather", GATHER, &[pool as u64, rec as u64])?;
            p.m.w::<u64>(this + 0x13b8, 0);
        }
        let name = (COMPOSITE_NAMES as isize + p.m.i32(p.base + COMPOSITE_NAMES + k as usize * 4) as isize) as usize;
        let t = alloc_target(p, w, name, 0x59, width, height)?;
        p.m.w::<u64>(this + 0x13b8, t as u64);
        copy_target(p, this + 0x12c8, t)?;
    }
    p.m.w::<u64>(vbase + 0x550, (this + 0x12c8) as u64);
    viewport::apply(p, vp, w, fb);
    if has2 {
        // The storage block object's virtual +0x50 (obj, w, {0, 1}).
        let args = [0u32, 1u32];
        p.helper("storage block virtual +0x50", p.vfn(storage, 0x50), &[storage as u64, w as u64, args.as_ptr() as u64])?;
    }
    // A new buffer's depth is cleared (to 0.5 with this +8 bit 2, else 1); colours (0, 0, 0, 1) are not.
    let mut values = [0u32; 0x22];
    for c in 0..8 {
        values[c * 4 + 3] = 1f32.to_bits();
    }
    values[0x20] = if p.m.r8(this + 8) & 4 != 0 { 0.5f32 } else { 1f32 }.to_bits();
    targets::bind_clear(p, fb, w, vp, if fits { 0 } else { 0x100 }, values.as_ptr() as usize)?;

    let mut g = default_state(p);
    g[0] = (g[0] & 0xffff_0000) | 0x101;
    g[1] = 0;
    byte(&mut g, 0x64, 8);
    g[7] = 0;
    p.apply_graphics_context(g.as_ptr() as usize, w);

    program_apply(p, prog, w)?;
    program_block(p, prog, w, 0, this + 0x1060, 0)?;
    if has2 {
        program_storage_block(p, prog, w, 0, storage, 0)?;
    }

    // The stack texture: a copy of the texture object at +0x208 (+0xa0 bytes) with the sampler of +0x2a8.
    let tex = Host::new(p, 0x100);
    let x = tex.addr();
    p.m.w::<u16>(x + 0xa0, 0x100);
    p.m.w::<u64>(x + 0xa8, (p.base + TEXTURE_NAME) as u64);
    p.m.w::<u64>(x + 0xb0, (p.base + SAMPLER_NAME) as u64);
    p.m.w::<u64>(x + 0xb8, 0);
    p.m.w::<u64>(x + 0xc0, 0x3f80_0000_0000_0000);
    p.m.w::<u64>(x + 0xc8, 0x4170_0000_0000_0000);
    p.m.w::<u32>(x + 0xd0, 0);
    p.m.w::<u64>(x + 0xd4, 0x0101_0707_0702_0101);
    p.m.w::<u8>(x + 0xdc, 0);
    p.m.w::<u16>(x + 0xde, 0);
    p.m.w::<u8>(x + 0xe8, 0x20);
    p.m.w::<u16>(x + 0xea, 0xffff);
    for o in (0..0xa0).step_by(4) {
        p.m.w::<u32>(x + o, p.m.r32(this + 0x208 + o));
    }
    acquire(p, x)?;
    for o in (0..0x10).step_by(8) {
        p.m.w::<u64>(x + 0xa0 + o, p.m.r64(this + 0x2a8 + o) as u64);
    }
    // The setup word (+0xe4 = 0xff, the first-use flags) stays; the finish in program_texture clears it.
    p.m.w::<u16>(x + 0xd4, 0);
    p.m.w::<u8>(x + 0xd6, 0);
    p.m.w::<u32>(x + 0xe4, 0xff);
    for o in 0..3 {
        p.m.w::<u8>(x + 0xe0 + o, tint);
    }
    p.m.w::<u8>(x + 0xe3, 1);
    p.m.w::<u8>(x + 0xe8, p.m.r8(x + 0xe8) | 2);
    program_texture(p, prog, w, 2, x)?;
    release_sampler(p, x + 0xea);
    release(p, x);
    tex.release(p);

    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0xd28, w);
    stream_draw_n(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x1e8, w, instances);
    targets::unbind(p, w)
}

/// main+0xa434a8(this, w, view, arg, flags): gsys::RenderBufferContext's edge adjust for the view's record
/// (this+0x10 + view * 0x21b0). A render state (flags bit 1: first context byte, bit 0: write mask 0xff), the game's
/// updateNLD, then the record's edge render buffer (+0x1c8) bound with its target cleared to 1.0 when flags bit 1, the
/// program of this+0xe8 (table index from the render buffer table), textures +0x610 / +0xe40, block +0x2120 and the
/// full-screen triangle (not with this+0x1c bit 0), unbind, a barrier, an optional second pass (record bits +5 / +9)
/// into the buffer +0x908 sampling +0x1d78, and a final barrier (not with this+0x1c bit 6). Without flags bit 0 the
/// record's two textures (+0x1d78, +0x1f60) are copies of the default textures; with it the EdgeAdjust / BlurAdjust
/// buffers are allocated (by the game's allocator, 12 arguments) and set up.
pub(super) unsafe fn edge_adjust(p: &mut Port, this: usize, w: usize, view: u32, arg: u32, flags: u32) -> R<()> {
    let recs = p.m.r64(this + 0x10);
    let idx = if p.m.r32(this + 8) <= view { 0 } else { view } as i32 as isize as usize;
    let rec = recs.wrapping_add(idx.wrapping_mul(0x21b0));
    let (alloc, clear) = (flags & 1 != 0, (flags >> 1) & 1);

    let t = p.m.r64(p.m.r64(p.base + BUFFER_TABLE));
    if p.m.r32(t + 0x38) < 7 {
        return Err("render buffer table");
    }
    let table = p.m.r64(p.m.r64(t + 0x40) + 0x810 + 8);
    let n = p.m.r32(table + 0x28);
    let arr = p.m.r64(table + 0x30);
    let slot = |o: usize| p.m.r16(arr + o + 0x20) as u32;
    let mask6 = if (p.m.r32(rec) >> 6) & 1 != 0 { u32::MAX } else { 0 };
    let mut index = (slot(if n >= 3 { 0x50 } else { 0 }) & mask6).wrapping_add(slot(0)).wrapping_add(slot(if n >= 6 { 200 } else { 0 }));
    if alloc {
        index = index.wrapping_add(slot(if n > 4 { 0xa0 } else { 0 }));
    }
    let index = if p.m.r32(this + 0xe0) <= index { 0 } else { index } as usize;
    let shader = p.m.r64(this + 0xe8) + index * 0x18;
    let prog = p.m.r64(shader + 8);
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let second = (p.m.r8(rec + 5) >> 4) & 1 != 0 && (p.m.r8(rec + 9) >> 3) & 1 != 0;
    let prog2 = p.m.r64(this + 0xf8);
    if second && (p.m.r32(prog2 + 0x10) != 0 || p.m.r32(prog2 + 0x14) != 0) {
        return Err("agl program setup pending");
    }
    let defaults = p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES));
    let (src1, src2) = (p.m.r64(defaults + 0x98), p.m.r64(defaults + 0x18));
    if !alloc {
        // Each copy releases the target's old handle, then takes the source's.
        let mut counts = Counts(Vec::new());
        counts.release(p, p.m.i32(rec + 0x1dd0));
        counts.take(p, p.m.i32(src1 + 0x58))?;
        counts.release(p, p.m.i32(rec + 0x1fb8));
        counts.take(p, p.m.i32(src2 + 0x58))?;
    }
    unbind_ok(p)?;
    bind_ok(p, rec + 0x1c8, w)?;
    if second {
        bind_ok(p, rec + 0x908, w)?;
    }
    if p.m.predict() {
        return Err("render buffer context update");
    }
    p.m.commit();
    // commit

    let mut g = default_state(p);
    byte(&mut g, 0, clear as u8);
    byte(&mut g, 1, 0);
    byte(&mut g, 0x64, 5);
    byte(&mut g, 0x1c, if alloc { 0xff } else { 0xf });
    g[1] = 0;
    p.apply_graphics_context(g.as_ptr() as usize, w);

    p.helper("render buffer context update", UPDATE_NLD, &[this as u64, w as u64, view as u64, arg as u64, 0])?;
    let (saved_a, saved_b) = (p.m.r64(rec + 0x1e8), p.m.r64(rec + 0x1f0));
    p.m.w::<u64>(rec + 0x1e8, (rec + 0x328) as u64);
    if !alloc {
        texture_assign(p, rec + 0x1d78, src1)?;
        texture_assign(p, rec + 0x1f60, src2)?;
    } else {
        let owner = p.m.r64(p.m.r64(p.base + ALLOCATOR_OWNER)) as u64;
        let buf = p.m.r64(rec + 0x518);
        let (bw, bh) = record_size(p, buf);
        let name = [(p.base + EDGE_NAME) as u64];
        let edge = p.call_stack("edge adjust buffer", ALLOCATE_TEXTURE, &[owner, w as u64, name.as_ptr() as u64, 10, bw as u64, bh as u64, 1, 0, 0, 1, 0, 0])?;
        p.m.w::<u64>(rec + 0x1e68, edge);
        let buf = p.m.r64(rec + 0x518);
        let (_, bh) = record_size(p, buf);
        let name = [(p.base + BLUR_NAME) as u64];
        let blur = p.call_stack(
            "blur adjust buffer",
            ALLOCATE_TEXTURE,
            &[owner, w as u64, name.as_ptr() as u64, 0x1d, (p.m.r16(buf + 0x30) >> 1) as u64, (bh >> 1) as u64, 1, 0, 0, 1, 0, 0],
        )?;
        p.m.w::<u64>(rec + 0x2050, blur);
        p.helper("ssao context texture", SSAO_ALLOC_TEXTURE, &[(rec + 0x1c88) as u64, edge])?;
        shadow_mask_setup(p, rec + 0x1d78, edge as usize)?;
        p.helper("ssao context texture", SSAO_ALLOC_TEXTURE, &[(rec + 0x1e70) as u64, blur])?;
        shadow_mask_setup(p, rec + 0x1f60, blur as usize)?;
        p.m.w::<u64>(rec + 0x1f0, (rec + 0x1c88) as u64);
        p.m.w::<u32>(rec + 8, p.m.r32(rec + 8) | 0x800);
    }

    // All eight colour targets 1.0, depth 0, stencil 0 (only target 0 is cleared, with flags bit 1).
    let mut values = [0u32; 0x22];
    for v in values.iter_mut().take(0x20) {
        *v = 1.0f32.to_bits();
    }
    viewport::apply(p, rec + 0x8e0, w, rec + 0x1c8);
    targets::bind_clear(p, rec + 0x1c8, w, rec + 0x8e0, clear, values.as_ptr() as usize)?;
    program_apply(p, prog, w)?;
    program_texture(p, prog, w, 0, rec + 0x610)?;
    program_texture(p, prog, w, 2, rec + 0xe40)?;
    program_block(p, prog, w, 0, rec + 0x2120, 0)?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    shape_state(p, shapes + 0x10e8, w);
    if p.m.r8(this + 0x1c) & 1 == 0 {
        stream_draw(p, streams + 0x488, w);
    }
    targets::unbind(p, w)?;
    let cmd = p.m.r64(w + 0xb8);
    if p.m.i32(rec + 0x388) != -1 {
        let bits = targets::barrier_bits(p);
        p.barrier(cmd, bits);
    }
    if second {
        let old = p.m.r64(rec + 0x928);
        p.m.w::<u64>(rec + 0x928, (rec + 0x1e70) as u64);
        let buffer = rec + 0x908;
        let full = viewport::full(p, p.m.f32(buffer + 8), p.m.f32(buffer + 0xc));
        viewport::apply(p, full.as_ptr() as usize, w, buffer);
        targets::bind(p, buffer, w)?;
        let mut g = default_state(p);
        g[0] &= 0xffff_0000;
        p.apply_graphics_context(g.as_ptr() as usize, w);
        program_apply(p, prog2, w)?;
        program_texture(p, prog2, w, 1, rec + 0x1d78)?;
        program_block(p, prog2, w, 0, rec + 0x2120, 0)?;
        stream_draw(p, streams + 0x488, w);
        targets::unbind(p, w)?;
        p.m.w::<u32>(rec + 8, p.m.r32(rec + 8) | 0x1000);
        p.m.w::<u64>(rec + 0x928, old as u64);
    }
    p.m.w::<u64>(rec + 0x1e8, saved_a as u64);
    p.m.w::<u64>(rec + 0x1f0, saved_b as u64);
    if (p.m.r8(this + 0x1c) >> 6) & 1 == 0 {
        let s = p.m.r64(p.m.r64(p.base + HOLDER));
        let invalidate = if (p.m.r8(s + 0x50c) >> 1) & 1 == 0 { 0x10 } else { 0x110 };
        p.barrier(cmd, invalidate | 1);
    }
    Ok(())
}
