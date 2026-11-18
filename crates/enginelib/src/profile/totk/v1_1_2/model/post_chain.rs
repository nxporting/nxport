// The post chain, main+0xc4f7c8(this, w, view, args, n): for the view's frame record (0x860 bytes at this+0x5c8) the
// scene colour (state at +0x1e0) is reduced through the detect pyramids ("detect0" / "detect1", five levels), the
// bloom levels and the blur / bloom_ex pair, drawn into the record's render buffer (+0x3c0) with the mrt programs of the
// program set 0x41, and the result is put back into the record's own state; the four post passes (main+0xc555c4) run
// in between. `args` is the caller's [render buffer, viewport, mrt holder, colour state, state, flag, state].
// The temporary targets come from the dynamic allocator, which keeps pool state: the port is applied only (everything
// past the checks is after the commit point); predicting stops at the first call. Everything that can stop (programs
// still being set up, the render buffers' unbind) is checked before.

use super::cubemap::set_uniform;
use super::draw::{Port, R};
use super::lights::{shape_state, SHAPES};
use super::program_draws::{post_pass, textured, tinted};
use super::scene_passes::default_state;
use super::targets::{self, RENDER_BUFFER_UNBIND};
use super::texture_slots::{assign, barrier_if_slot, texture_assign};
use super::viewport;
use super::vfx::{program_apply, program_texture, shadow_mask_setup};

/// [[holder]]: the program sets ([+0x10] the array; +8 the count; entry 0x41 at +0x208 is the mrt program table).
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[holder]]: the temporary target pool.
const TARGET_POOL: usize = 0x4634e70;
/// main+0x7f0ad4(pool, w, &name, format, width, height, levels, 0, 0, 1, 0, 0): a temporary target (its state).
const ALLOC_TARGET: usize = 0x7f0ad4;
/// main+0x7f0354(pool, target): gives a temporary target back.
const GATHER: usize = 0x7f0354;
/// agl::sdw::SSAO::Context::allocTexture(context, state).
const SSAO_ALLOC: usize = 0xf817bc;
/// main+0xc58c90(ssao context, record, level, 0): the context's level setup.
const SSAO_LEVEL: usize = 0xc58c90;
/// gsys::RenderBufferContext::releaseAABuffer(state).
const RELEASE_AA: usize = 0xc57dd4;
/// main+0xc57e74(this, w, view, k, render buffer): the bloom composite k.
const BLOOM_PASS: usize = 0xc57e74;
/// main+0xc57d98(this, view): the chain's finish.
const FINISH: usize = 0xc57d98;
/// [[holder]]: [[+0x18]] is the default texture state.
const DEFAULT_TEXTURES: usize = 0x4631740;

/// "reduce", "detect0", "detect1", "bloom_ex" and the blur levels' temporary name.
const NAME_REDUCE: usize = 0x370fd0f;
const NAME_DETECT0: usize = 0x36e65ed;
const NAME_DETECT1: usize = 0x375a5df;
const NAME_BLOOM_EX: usize = 0x36ff593;
const NAME_LEVEL: usize = 0x370fd16;

/// The chain's flags (this +0x7d0): bit 0 no mrt resolve, 1 no own state, 2 state, 3 no post passes, 4 no bloom levels,
/// 5 no tint of the first level, 6 deferred detect targets, 7 reduce.
const FLAGS: usize = 0x7d0;

struct C {
    this: usize,
    w: usize,
    view: u32,
    /// The view record (this +0x5d8 array).
    v: usize,
    /// The frame record (this +0x5c8 array, 0x860 each).
    f: usize,
    /// The caller's argument array and its render buffer.
    a: usize,
    rbuf: usize,
    /// The record's render buffer (+0x3c0).
    rb: usize,
}

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

unsafe fn flag(p: &Port, this: usize, bit: u32) -> bool {
    (p.m.r8(this + FLAGS) >> bit) & 1 != 0
}

unsafe fn tex_w(p: &Port, t: usize) -> u32 {
    (p.m.r16(t + 0x30) as u32).max(1)
}

unsafe fn tex_h(p: &Port, t: usize) -> u32 {
    let h = p.m.r16(t + 0x32) as u32;
    (if p.m.r16(t + 0x3a) == 3 { h } else { 1 }).max(h)
}

fn f4(v: [f32; 4]) -> [u8; 16] {
    let mut b = [0u8; 16];
    for (k, x) in v.iter().enumerate() {
        b[k * 4..k * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    b
}

fn words4(v: [u32; 4]) -> [u8; 16] {
    let mut b = [0u8; 16];
    for (k, x) in v.iter().enumerate() {
        b[k * 4..k * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    b
}

unsafe fn ready(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

unsafe fn pool(p: &Port) -> usize {
    p.m.r64(p.m.r64(p.base + TARGET_POOL))
}

/// A temporary target with `levels` mip levels (main+0x7f0ad4 with 12 arguments, four on the stack).
unsafe fn alloc(p: &mut Port, c: &C, name: usize, format: u32, width: u32, height: u32, levels: u32) -> R<usize> {
    let name: [u64; 2] = [(p.base + name) as u64, 0];
    let args = [pool(p) as u64, c.w as u64, name.as_ptr() as u64, format as u64, width as u64, height as u64, levels as u64, 0, 0, 1, 0, 0];
    Ok(p.call_stack("post chain target allocated by the call", ALLOC_TARGET, &args)? as usize)
}

unsafe fn gather(p: &mut Port, target: usize) -> R<()> {
    let pl = pool(p);
    p.helper("depth of field gather by the call", GATHER, &[pl as u64, target as u64])?;
    Ok(())
}

unsafe fn ssao_alloc(p: &mut Port, context: usize, state: usize) -> R<()> {
    p.helper("ssao texture allocation by the call", SSAO_ALLOC, &[context as u64, state as u64])?;
    Ok(())
}

fn span(v: f32) -> (f32, f32) {
    if v <= 0.0 {
        (v, 0.0)
    } else {
        (0.0, v)
    }
}

/// The record's render buffer extent: size (+0x3c8, +0x3cc) and region (+0x3d0 x, +0x3d4 y, +0x3d8 x2, +0x3dc y2).
unsafe fn set_region(p: &mut Port, f: usize, fw: f32, fh: f32) {
    p.m.w::<f32>(f + 0x3c8, fw);
    p.m.w::<f32>(f + 0x3cc, fh);
    let ((x, x2), (y, y2)) = (span(fw), span(fh));
    p.m.w::<f32>(f + 0x3d0, x);
    p.m.w::<f32>(f + 0x3d4, y);
    p.m.w::<f32>(f + 0x3d8, x2);
    p.m.w::<f32>(f + 0x3dc, y2);
}

/// The viewport of the record's render buffer size.
unsafe fn rb_viewport(p: &Port, f: usize) -> [u32; 10] {
    viewport::full(p, p.m.f32(f + 0x3c8), p.m.f32(f + 0x3cc))
}

/// The record's buffer bound and its viewport applied.
unsafe fn bind_view(p: &mut Port, c: &C, vp: &[u32; 10]) -> R<()> {
    targets::bind(p, c.rb, c.w)?;
    viewport::apply(p, vp.as_ptr() as usize, c.w, c.rb);
    Ok(())
}

/// The unbind of the record's render buffer (virtual +0x28) and the barrier when state `id_at` has a slot.
unsafe fn finish_pass(p: &mut Port, c: &C, slot: usize) -> R<()> {
    targets::unbind(p, c.w)?;
    barrier_if_slot(p, c.f + slot, c.w);
    Ok(())
}

/// The flags of the SSAO target object at q+0x548 after it was given a state: 4 (layer / level flags when they changed
/// since `layer` / `level`).
unsafe fn target_flags(p: &mut Port, q: usize, layer: u16, level: u8) {
    p.m.fetch_or32(q + 0x610, 4);
    if p.m.r16(q + 0x614) != layer {
        p.m.fetch_or32(q + 0x610, 2);
        p.m.w::<u16>(q + 0x614, layer);
    }
    if p.m.r8(q + 0x616) != level {
        p.m.fetch_or32(q + 0x610, 1);
        p.m.w::<u8>(q + 0x616, level);
    }
}

/// The SSAO target object at q+0x548 (state +0x550, flags +0x610, layer +0x614, level +0x616) given the state `src`,
/// then its virtual +0x10 (the game's step, which this does not port).
unsafe fn ssao_target(p: &mut Port, q: usize, src: usize) -> R<()> {
    let (layer, level) = (p.m.r16(q + 0x614), p.m.r8(q + 0x616));
    assign(p, q + 0x550, src)?;
    target_flags(p, q, layer, level);
    p.helper("ssao step by the call", p.vfn(q + 0x548, 0x10), &[(q + 0x548) as u64])?;
    Ok(())
}

/// The level of an SSAO target object reset (flags bit 0) when it is not 0.
unsafe fn reset_level(p: &mut Port, q: usize) {
    if p.m.r8(q + 0x616) != 0 {
        p.m.fetch_or32(q + 0x610, 1);
        p.m.w::<u8>(q + 0x616, 0);
    }
}

/// The default texture state put into the frame record (the small views): the state assigned and marked as needing its
/// finish.
unsafe fn default_texture(p: &mut Port, f: usize) -> R<()> {
    let d = p.m.r64(p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES)) + 0x18);
    texture_assign(p, f, d)
}

/// main+0xc4f7c8(this, w, view, args, n): see the file head.
pub(super) unsafe fn post_chain(p: &mut Port, this: usize, w: usize, view: u32, a: usize, flag_n: u32) -> R<()> {
    let vi = clamp_index(p.m.r32(this + 0x18) & view, p.m.r32(this + 0x5d0));
    if p.m.r8(this + 0x7c8) == 0 {
        return Ok(());
    }
    let v = p.m.r64(p.m.r64(this + 0x5d8).wrapping_add(vi.wrapping_mul(8)));
    let on = if view == 0xffff_ffff { p.m.r32(this + 8) } else { (p.m.r32(this + 8) >> (view & 0x1f)) & 1 };
    if on == 0 {
        return Ok(());
    }
    let f = p.m.r64(this + 0x5c8).wrapping_add(clamp_index(view, p.m.r32(this + 0x5c0)).wrapping_mul(0x860));
    let (rbuf, vp) = (p.m.r64(a), p.m.r64(a + 8));
    // The viewport's extent in the render buffer's units (swapped for orientations 1 and 2).
    let rf = |o: usize| p.m.f32(o);
    let (ext_w, ext_h) = (rf(vp + 0x10) - rf(vp + 8), rf(vp + 0x14) - rf(vp + 0xc));
    let (ex, ey) = if p.m.r32(vp + 0x18).wrapping_sub(1) < 2 { (ext_h, ext_w) } else { (ext_w, ext_h) };
    let fx = (ex / rf(rbuf + 8)) * (rf(rbuf + 0x18) - rf(rbuf + 0x10));
    let fy = (ey / rf(rbuf + 0xc)) * (rf(rbuf + 0x1c) - rf(rbuf + 0x14));
    if fx < 64.0 || fy < 64.0 {
        return default_texture(p, f);
    }
    let c = C { this, w, view, v, f, a, rbuf, rb: f + 0x3c0 };

    // The state a[3] (else the buffer's first target's) and the optional states a[4], a[6]; has5: the argument flag a[5].
    let a4 = p.m.r64(a + 0x20);
    let a5 = p.m.r8(a + 0x28) != 0;
    let own = a4 == 0 || flag(p, this, 1);
    let has5 = if own { a5 && p.m.r64(rbuf + 0x60) == 0 } else { a5 };
    let reduce = (p.m.r8(this + FLAGS) as i8) < 0 && p.m.r64(a + 0x18) == 0 && p.m.r32(v + 0x510) == 0;
    let prog = mrt_program(p, &c, has5)?;
    check(p, &c, prog, reduce, flag_n)?;
    p.m.commit();
    // commit

    barrier_if_slot(p, p.m.r64(rbuf + 0x20) + 0x60, w);
    let scale = p.m.f32(f + 0x848) * p.m.f32(f + 0x84c);
    let src = match p.m.r64(a + 0x18) {
        0 => p.m.r64(rbuf + 0x20) + 8,
        s => s,
    };
    texture_assign(p, f + 0x1e0, src)?;
    if own {
        let off = p.m.r64(rbuf + 0x60);
        if off != 0 {
            shadow_mask_setup(p, f + 0xf0, off + 8)?;
        }
    } else {
        texture_assign(p, f + 0xf0, a4)?;
    }
    let a6 = p.m.r64(a + 0x30);
    if a6 != 0 {
        texture_assign(p, f + 0x2d0, a6)?;
    }
    let (ww, hh) = ((fx * scale) as u32, (fy * scale) as u32);
    let (w3, h3) = (ww.wrapping_add(3), hh.wrapping_add(3));
    if (w3 & !3) < 0x10 || (h3 & !3) < 0x10 {
        return default_texture(p, f);
    }
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);

    let reduced = if reduce { Some(reduce_scene(p, &c, ww, hh)?) } else { None };
    detect(p, &c, w3, h3)?;
    scene_pass(p, &c, prog)?;
    if let Some(t) = reduced {
        gather(p, t)?;
    }
    if !flag(p, this, 3) {
        for n in 0..4 {
            post_pass(p, this, w, view, n)?;
        }
    }
    bloom_levels(p, &c)?;
    if !flag(p, this, 6) {
        shadow_mask_setup(p, f + 0x458, p.m.r64(f + 0x430))?;
        ssao_alloc(p, f + 0x548, p.m.r64(f + 0x430))?;
    } else {
        p.m.w::<u64>(f + 0x520, 0);
        p.m.w::<u32>(f + 0x528, 0);
        p.m.fetch_or32(f + 0x53c, 8);
        reset_level(p, f);
    }
    if !flag(p, this, 6) {
        shadow_mask_setup(p, f + 0x660, p.m.r64(f + 0x638))?;
        ssao_alloc(p, f + 0x750, p.m.r64(f + 0x638))?;
    } else {
        p.m.w::<u64>(f + 0x728, 0);
        p.m.w::<u32>(f + 0x730, 0);
        p.m.fetch_or32(f + 0x744, 8);
        if p.m.r8(f + 0x81e) != 0 {
            p.m.fetch_or32(f + 0x818, 1);
            p.m.w::<u8>(f + 0x81e, 0);
        }
    }
    blur_levels(p, &c)?;
    restore_scene(p, &c)?;
    resolve(p, &c, prog, flag_n)
}

/// The mrt program of the scene pass: the set 0x41's table (+8: count +0x28, entries +0x30 of 0x78 bytes; the
/// selector at +0x20 of each) summed over the enabled features picks the entry of the mrt array (this +0x890, 0x18 each).
unsafe fn mrt_program(p: &Port, c: &C, has5: bool) -> R<usize> {
    let (this, v) = (c.this, c.v);
    let h = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    let set = if p.m.r32(h + 8) < 0x42 { 0 } else { p.m.r64(p.m.r64(h + 0x10) + 0x208) };
    if set == 0 {
        return Err("post chain program set missing");
    }
    let t = p.m.r64(set + 8);
    let count = p.m.r32(t + 0x28);
    let arr = p.m.r64(t + 0x30);
    let e = |off: usize| p.m.r16(arr + off + 0x20) as u32;
    let at = |k: u32, off: usize| if count < k { 0 } else { off };
    let t0 = if p.m.r8(v + 0x490) != 0 { e(0) } else { 0 };
    let t1 = if has5 { e(at(4, 0x78)) } else { 0 };
    let t2 = if p.m.r32(v + 0x450) != 0 { e(at(5, 0xa0)) } else { 0 };
    let t3 = if p.m.r8(v + 0x2a8) != 0 { e(at(6, 0xc8)) } else { 0 };
    let t4 = if p.m.r8(this + 0x4e0) != 0 { e(at(7, 0xf0)) } else { 0 };
    let t5 = if p.m.r8(this + 0x500) != 0 { e(at(8, 0x118)) } else { 0 };
    let t6 = if p.m.r8(v + 0x168) != 0 {
        if p.m.f32(v + 0x1c8) <= 0.0 {
            0
        } else {
            e(if count > 1 { 0x28 } else { 0 })
        }
    } else {
        0
    };
    let t7 = if p.m.r8(v + 0x208) == 0 || p.m.f32(v + 0x268) <= 0.0 { 0 } else { e(at(3, 0x50)) };
    let sum = t1.wrapping_add(t0).wrapping_add(t2).wrapping_add(t3).wrapping_add(t4).wrapping_add(t5).wrapping_add(t6).wrapping_add(t7);
    let i = (if p.m.r32(this + 0x888) <= sum { 0 } else { sum }) as usize;
    Ok(p.m.r64(p.m.r64(this + 0x890) + i * 0x18 + 8))
}

/// Everything that can stop, before the first call: programs still being set up, the render buffers' unbind.
unsafe fn check(p: &Port, c: &C, prog: usize, reduce: bool, flag_n: u32) -> R<()> {
    let (this, v) = (c.this, c.v);
    if p.vfn(c.rb, 0x28) != RENDER_BUFFER_UNBIND || p.vfn(c.rbuf, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    ready(p, prog)?;
    if reduce {
        ready(p, p.m.r64(this + 0x800))?;
    }
    if !flag(p, this, 3) || p.m.r32(v + 0x450) != 0 {
        for o in [0x818, 0x830, 0x848] {
            ready(p, p.m.r64(this + o))?;
        }
    }
    if !flag(p, this, 4) || p.m.r32(v + 0x450) != 0 {
        ready(p, p.m.r64(this + 0x860))?;
    }
    if p.m.r64(c.f + 0x840) != 0 {
        ready(p, p.m.r64(this + 0x878))?;
    }
    if flag_n & 1 == 0 && !flag(p, this, 0) {
        ready(p, p.m.r64(p.m.r64(c.a + 0x10) + 8))?;
    }
    Ok(())
}

/// The reduce pass: the scene at twice the pass size drawn into a temporary "reduce" target through the mrt this+0x7f8,
/// which then is the scene state (+0x1e0) and the record's first target. Returns the target (given back after the
/// scene pass).
unsafe fn reduce_scene(p: &mut Port, c: &C, ww: u32, hh: u32) -> R<usize> {
    let (this, w, f) = (c.this, c.w, c.f);
    let (fw, fh) = (ww as f32, hh as f32);
    let (fw, fh) = (fw + fw, fh + fh);
    let t = alloc(p, c, NAME_REDUCE, p.m.r32(this + 0x7d4), fw as u32, fh as u32, 1)?;
    set_region(p, f, fw, fh);
    reset_level(p, f);
    ssao_target(p, f, t)?;
    p.m.w::<u64>(f + 0x3e0, (f + 0x548) as u64);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let vp = rb_viewport(p, f);
    bind_view(p, c, &vp)?;
    let prog = p.m.r64(this + 0x800);
    program_apply(p, prog, w)?;
    textured(p, w, prog, f + 0x1e0)?;
    finish_pass(p, c, 0x5a8)?;
    texture_assign(p, f + 0x1e0, t)?;
    Ok(t)
}

/// The detect pyramids k = 0, 1 (at the record and 0x208 on): five levels of the pass size (one target of five mip levels
/// in the deferred mode), each given to the SSAO target object (q+0x548), copied to the level state (q+0x458) and
/// bound as the record buffer's target k.
unsafe fn detect(p: &mut Port, c: &C, w3: u32, h3: u32) -> R<()> {
    let (this, f) = (c.this, c.f);
    let fmt = p.m.r32(this + 0x7d4);
    let deferred = flag(p, this, 6);
    for k in 0..2usize {
        let q = f + k * 0x208;
        let name = if k == 0 { NAME_DETECT0 } else { NAME_DETECT1 };
        if !deferred {
            let m = (w3 & !3, h3 & !3);
            let dims = [m, (m.0 >> 1, m.1 >> 1), (w3 >> 2, h3 >> 2), (w3 >> 3, h3 >> 3), (w3 >> 4, h3 >> 4)];
            for (i, (dw, dh)) in dims.into_iter().enumerate() {
                let t = alloc(p, c, name, fmt, dw, dh, 1)?;
                p.m.w::<u64>(q + 0x430 + i * 8, t as u64);
            }
        } else {
            let t = alloc(p, c, name, fmt, w3 & !3, h3 & !3, if k == 0 { 5 } else { 1 })?;
            p.m.w::<u64>(q + 0x430, t as u64);
            reset_level(p, q);
            p.m.w::<u64>(q + 0x520, 0);
            p.m.w::<u32>(q + 0x528, 0);
            p.m.fetch_or32(q + 0x53c, 8);
        }
        let src = p.m.r64(q + 0x430);
        ssao_target(p, q, src)?;
        texture_assign(p, q + 0x458, q + 0x550)?;
        p.m.w::<u64>(f + 0x3e0 + k * 8, (q + 0x548) as u64);
    }
    Ok(())
}

/// The scene pass: the default state with the write mask, the mrt program `prog` with its ~10 uniforms (the depth
/// range, the view matrix rows, the pass size) and the textures 2 (+0x2d0) and 1 (+0xf0), the scene state (+0x1e0)
/// drawn into the record's two detect targets at the state's (+0x550) size; then the detect targets' barriers.
unsafe fn scene_pass(p: &mut Port, c: &C, prog: usize) -> R<()> {
    let (this, w, v, f) = (c.this, c.w, c.v, c.f);
    let (w1, h1) = (tex_w(p, f + 0x1e0), tex_h(p, f + 0x1e0));
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = if p.m.r32(v + 0x450) != 0 { 0xff } else { 0xf };
    p.apply_graphics_context(g.as_ptr() as usize, w);
    program_apply(p, prog, w)?;

    let rf = |o: usize| p.m.f32(o);
    let ru = |o: usize| p.m.r32(o);
    let f54 = rf(f + 0x854);
    let sa = f54 * rf(v + 0x38);
    let inv_a = if sa > 0.0 { 1.0 / sa } else { 0.0 };
    let u1 = [inv_a * rf(this + 0x5a0), 0.0, rf(v + 0x58), 10000.0];
    let u2 = [inv_a * rf(this + 0x5b0), inv_a * rf(this + 0x5b4), inv_a * rf(this + 0x5b8), (f54 * -rf(v + 0x18)) * inv_a];
    let sb = f54 * rf(v + 0xe0);
    let inv_b = if sb > 0.0 { 1.0 / sb } else { 0.0 };
    let u3 = [inv_b, 0.0, rf(v + 0x100), 10000.0];
    let u4 = [rf(this + 0x560) + rf(this + 0x560), 0.0, 0.0, 0.0];
    let (f85c, f858) = (rf(f + 0x85c), rf(f + 0x858));
    let diff = f85c - f858;
    let inv_d = 1.0 / diff;
    let d_a = diff / (rf(v + 0x1a8) - rf(v + 0x188));
    let d_b = diff / (rf(v + 0x248) - rf(v + 0x228));
    let u5 = [d_a, inv_d * (d_a * -rf(v + 0x188)), d_b, inv_d * (d_b * -rf(v + 0x228))];
    let u6 = [ru(v + 0x1e8), ru(v + 0x1c8), ru(v + 0x268), 0];
    let u7 = [1.0 - f858 / f85c, f858 * inv_d, 1.0 / w1 as f32, 1.0 / h1 as f32];
    let n9 = diff / (rf(v + 0x2e8) - rf(v + 0x2c8));
    let u9 = [n9.to_bits(), (inv_d * (n9 * -rf(v + 0x2c8))).to_bits(), ru(v + 0x328), ru(v + 0x308)];

    set_uniform(p, prog, w, 1, &f4(u1));
    set_uniform(p, prog, w, 0, &f4(u2));
    set_uniform(p, prog, w, 8, &f4(u3));
    program_texture(p, prog, w, 2, f + 0x2d0)?;
    set_uniform(p, prog, w, 7, &f4(u4));
    program_texture(p, prog, w, 1, f + 0xf0)?;
    set_uniform(p, prog, w, 2, &f4(u5));
    set_uniform(p, prog, w, 3, &words4(u6));
    set_uniform(p, prog, w, 4, &f4(u7));
    set_uniform(p, prog, w, 9, &words4(u9));

    let (sw, sh) = (tex_w(p, f + 0x550), tex_h(p, f + 0x550));
    set_region(p, f, sw as f32, sh as f32);
    let vp = rb_viewport(p, f);
    bind_view(p, c, &vp)?;
    textured(p, w, prog, f + 0x1e0)?;
    finish_pass(p, c, 0x5a8)?;
    barrier_if_slot(p, f + 0x7b0, w);
    p.m.w::<u64>(f + 0x3e8, 0);
    Ok(())
}

/// The tint weights of the bloom levels: four rows of four floats (all (0, 0, 0, 1) unless this +0x418 gives them).
unsafe fn level_weights(p: &Port, c: &C) -> [f32; 16] {
    let (this, v) = (c.this, c.v);
    let mut e = [0f32; 16];
    for k in 0..4 {
        e[k * 4 + 3] = 1.0;
    }
    match p.m.r32(this + 0x418) {
        0 => {
            for (k, o) in [0x348usize, 0x350, 0x370, 0x378, 0x398, 0x3a0, 0x3c0, 0x3c8].into_iter().enumerate() {
                e[k * 2] = f32::from_bits(p.m.r32(v + o));
                e[k * 2 + 1] = f32::from_bits(p.m.r32(v + o + 4));
            }
        }
        1 => {
            let x = p.m.f32(v + 0xa0);
            let three = x * 3.0;
            let m1 = three + -1.0;
            let a = if x < 0.33333334 { three } else { 1.0 };
            let b = if x < 0.33333334 { 0.0 } else { m1 };
            let inv = 1.0 / (((a + 1.0) + b) + 0.0);
            for k in 0..4 {
                e[k * 4] = 1.0;
                e[k * 4 + 1] = 1.0;
                e[k * 4 + 2] = 1.0;
            }
            e[3] = inv;
            e[7] = a * inv;
            e[11] = b * inv;
            e[15] = inv * 0.0;
        }
        _ => {}
    }
    e
}

/// The bloom levels m = 3, 2, 1 (smaller each): the level's SSAO target and offscreen state set (or the deferred
/// words), the record buffer sized to the level, bound, and the level state (+0x458) tinted into it with the weights.
unsafe fn bloom_levels(p: &mut Port, c: &C) -> R<()> {
    let (this, w, v, f) = (c.this, c.w, c.v, c.f);
    if flag(p, this, 4) {
        return Ok(());
    }
    let e = level_weights(p, c);
    let deferred = flag(p, this, 6);
    for m in (1..=3u32).rev() {
        let t = p.m.r64(f + 0x430);
        let (mut lw, mut lh) = (tex_w(p, t), tex_h(p, t));
        if !deferred {
            ssao_alloc(p, f + 0x548, p.m.r64(f + m as usize * 8 + 0x430))?;
            shadow_mask_setup(p, f + 0x458, p.m.r64(f + m as usize * 8 + 0x438))?;
        } else {
            if m != p.m.r8(f + 0x616) as u32 {
                p.m.fetch_or32(f + 0x610, 1);
                p.m.w::<u8>(f + 0x616, m as u8);
            }
            p.m.w::<f32>(f + 0x520, m as i32 as f32 + 1.0);
            p.m.w::<f32>(f + 0x524, m as i32 as f32 + 1.0);
            p.m.w::<u32>(f + 0x528, 0);
            p.m.fetch_or32(f + 0x53c, 8);
        }
        lw >>= m & 0x1f;
        lh >>= m & 0x1f;
        set_region(p, f, lw as f32, lh as f32);
        let vp = rb_viewport(p, f);
        targets::bind(p, c.rb, w)?;
        viewport::apply(p, vp.as_ptr() as usize, w, c.rb);
        let k = m as usize - 1;
        let blend: [f32; 4] = std::array::from_fn(|j| e[k * 4 + j]);
        if k == 0 {
            if !flag(p, this, 5) {
                let tint: [f32; 4] = [
                    p.m.f32(v + 0x78) * e[0],
                    p.m.f32(v + 0x7c) * e[1],
                    p.m.f32(v + 0x80) * e[2],
                    p.m.f32(v + 0x84) * e[3],
                ];
                tinted(p, this, w, f + 0x458, v + 0x78, tint.as_ptr() as usize)?;
            }
        } else if p.m.r8(this + 0x520) == 0 {
            if m == 2 {
                let ones = [1.0f32; 4];
                tinted(p, this, w, f + 0x458, ones.as_ptr() as usize, blend.as_ptr() as usize)?;
            } else {
                let color: [f32; 4] = std::array::from_fn(|j| e[12 + j]);
                tinted(p, this, w, f + 0x458, color.as_ptr() as usize, blend.as_ptr() as usize)?;
            }
        } else {
            let color: [f32; 4] = std::array::from_fn(|j| e[m as usize * 4 + j]);
            tinted(p, this, w, f + 0x458, color.as_ptr() as usize, blend.as_ptr() as usize)?;
        }
        finish_pass(p, c, 0x5a8)?;
    }
    Ok(())
}

/// The blur chain (this +0x818 / 0x828 / 0x840), with the view's blur mode (v +0x450: 1 horizontal halving, 2 vertical),
/// `v +0x470` times: a "level" target (state +0x458, the SSAO context at +0x548), drawn through this+0x818 into the
/// record's buffer, a "bloom_ex" target (state +0x660, the context at +0x750) drawn from it through the mode's blur
/// program (uniform 6 = the step), and the targets given back; then the last level state (+0x30 / +0x32 at the clamped
/// level +0xc8) tinted to the view's colour (v +0x120).
unsafe fn blur_levels(p: &mut Port, c: &C) -> R<()> {
    let (this, w, v, f) = (c.this, c.w, c.v, c.f);
    if p.m.r32(v + 0x450) == 0 {
        return Ok(());
    }
    let (uw, uh) = (p.m.r16(f + 0x788) as u32, p.m.r16(f + 0x78a) as u32);
    let hh = if p.m.r16(f + 0x792) != 3 { 1 } else { uh };
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let fmt = p.m.r32(this + 0x7d4);
    let mut fh = hh.max(uh) as f32;
    let mut fw = uw.max(1) as f32;
    let mut prev: usize = 0;
    let count = p.m.i32(v + 0x470);
    for _ in 0..count.max(0) {
        let mode = p.m.r32(v + 0x450);
        let (mut step_x, mut step_y) = (0.0f32, 0.0f32);
        if mode == 2 {
            fh *= 0.5;
            step_y = 1.0 / fh;
        } else if mode == 1 {
            fw *= 0.5;
            step_x = 1.0 / fw;
        }
        if 1.0 <= fw && 1.0 <= fh {
            let level = alloc(p, c, NAME_LEVEL, fmt, fw as u32, fh as u32, 1)?;
            texture_assign(p, f + 0x458, level)?;
            ssao_target(p, f, level)?;
            set_region(p, f, fw, fh);
            let vp = rb_viewport(p, f);
            viewport::apply(p, vp.as_ptr() as usize, w, c.rb);
            let first = p.m.r64(this + 0x818);
            program_apply(p, first, w)?;
            set_uniform(p, first, w, 5, &[0u8; 16]);
            p.m.w::<u64>(f + 0x3e0, (f + 0x548) as u64);
            targets::bind(p, c.rb, w)?;
            textured(p, w, first, f + 0x660)?;
            finish_pass(p, c, 0x5a8)?;
            if prev != 0 {
                gather(p, prev)?;
            }
            prev = alloc(p, c, NAME_BLOOM_EX, fmt, fw as u32, fh as u32, 1)?;
            ssao_target(p, f + 0x208, prev)?;
            texture_assign(p, f + 0x660, prev)?;
            let mrt = if p.m.r32(v + 0x450) == 1 { this + 0x828 } else { this + 0x840 };
            let blur = p.m.r64(mrt + 8);
            program_apply(p, blur, w)?;
            set_uniform(p, blur, w, 6, &f4([step_x, step_y, 0.0, 0.0]));
            p.m.w::<u64>(f + 0x3e0, (f + 0x750) as u64);
            targets::bind(p, c.rb, w)?;
            textured(p, w, blur, f + 0x458)?;
            targets::unbind(p, w)?;
            barrier_if_slot(p, f + 0x7b0, w);
            gather(p, level)?;
        }
    }
    // The last level of the chain tinted into the record's buffer.
    let top = f;
    let max_level = (p.m.r8(top + 0x39) as u32).wrapping_sub(1);
    let want = p.m.f32(top + 0xc8) as u32;
    let level = if max_level <= want { max_level } else { want };
    let lw = (p.m.r16(top + 0x30) as u32 >> (level & 31)).max(1);
    let h = p.m.r16(top + 0x32) as u32;
    let lh = (if p.m.r16(top + 0x3a) != 3 { 1 } else { h }).max(h >> (level & 31));
    set_region(p, f, lw as f32, lh as f32);
    p.helper("ssao level by the call", SSAO_LEVEL, &[(f + 0x548) as u64, f as u64, level as u64, 0])?;
    p.m.w::<u64>(f + 0x3e0, (f + 0x548) as u64);
    let vp = rb_viewport(p, f);
    targets::bind(p, c.rb, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, c.rb);
    let ones = [1.0f32; 4];
    tinted(p, this, w, f + 0x660, v + 0x120, ones.as_ptr() as usize)?;
    targets::unbind(p, w)?;
    if prev != 0 {
        gather(p, prev)?;
    }
    Ok(())
}

/// The AA buffer released and the frame record's state restored: the first level (offscreen buffer setup of level 1, or
/// the deferred copy of the first target with the unit exposure words +0xc8 / +0xd0), then the optional second state
/// (+0x840) drawn over it through the mrt this+0x870.
unsafe fn restore_scene(p: &mut Port, c: &C) -> R<()> {
    let (this, w, f) = (c.this, c.w, c.f);
    p.helper("aa buffer release by the call", RELEASE_AA, &[(f + 0x638) as u64])?;
    if !flag(p, this, 6) {
        shadow_mask_setup(p, f, p.m.r64(f + 0x438))?;
    } else {
        let first = p.m.r64(f + 0x430);
        texture_assign(p, f, first)?;
        p.m.w::<u64>(f + 0xc8, 0x3f80_0000_3f80_0000);
        p.m.w::<u32>(f + 0xd0, 0);
        p.m.fetch_or32(f + 0xe4, 8);
    }
    let second = p.m.r64(f + 0x840);
    if second != 0 {
        let (sw, sh) = (tex_w(p, second), tex_h(p, second));
        ssao_alloc(p, f + 0x548, second)?;
        p.m.w::<u64>(f + 0x3e0, (f + 0x548) as u64);
        set_region(p, f, sw as f32, sh as f32);
        let vp = rb_viewport(p, f);
        targets::bind(p, c.rb, w)?;
        viewport::apply(p, vp.as_ptr() as usize, w, c.rb);
        let mut g = default_state(p);
        g[10] = 0x0202_0202;
        g[0] &= 0xffff_0000;
        g[1] |= 1;
        p.apply_graphics_context(g.as_ptr() as usize, w);
        let prog = p.m.r64(this + 0x878);
        program_apply(p, prog, w)?;
        textured(p, w, prog, f)?;
        shadow_mask_setup(p, f, second)?;
        p.m.w::<u64>(f + 0xc8, 0);
        p.m.w::<u32>(f + 0xd0, 0);
        p.m.fetch_or32(f + 0xe4, 8);
        targets::unbind(p, w)?;
    }
    Ok(())
}

/// The caller's buffer bound again with its viewport; unless `flag_n` bit 0 / this+0x7d0 bit 0 the caller's mrt (args[2])
/// resolves the record's state (f) into it with the state of the view (v +0x408: 1 / 2 add the blend bits); the bloom
/// composites 0..2 by this+0x5ac; the buffer's unbind and the finish.
unsafe fn resolve(p: &mut Port, c: &C, _prog: usize, flag_n: u32) -> R<()> {
    let (this, w, v, f, a) = (c.this, c.w, c.v, c.f, c.a);
    let (rbuf, vp) = (p.m.r64(a), p.m.r64(a + 8));
    targets::bind(p, rbuf, w)?;
    viewport::apply(p, vp, w, rbuf);
    if flag_n & 1 == 0 && !flag(p, this, 0) {
        let mrt = p.m.r64(a + 0x10);
        let prog = p.m.r64(mrt + 8);
        program_apply(p, prog, w)?;
        let mut g = default_state(p);
        g[0] &= 0xffff_0000;
        g[1] &= !1;
        g[7] = (g[7] & 0xffff_fff0) | 7;
        if !flag(p, this, 2) {
            match p.m.r32(v + 0x408) {
                2 => {
                    g[1] |= 1;
                    g[10] = 0x0202_0a0a;
                }
                1 => {
                    g[1] |= 1;
                    g[10] = 0x0202_0202;
                }
                _ => {}
            }
        }
        p.apply_graphics_context(g.as_ptr() as usize, w);
        textured(p, w, prog, f)?;
    }
    for k in 0..3u32 {
        if (p.m.r16(this + 0x5ac) >> (4 + k)) & 1 != 0 {
            super::mask_callees::bloom_pass(p, this, w, c.view, k as u32, rbuf as usize)?;
        }
    }
    targets::unbind(p, w)?;
    // The per-level debug names ("bloom:%d") are only formatted into a stack string.
    if flag_n & 1 == 0 {
        super::mask_callees::post_finish(p, this, c.view)?;
    }
    Ok(())
}
