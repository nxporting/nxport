// Draw sites reached through virtual calls at Hateno: the GPU footprint pass (the virtual +0 of the object at
// GPUFootPrintMgr+0x88, main+0x18986f8) and the multi-filter units drawn from the UI layer's draw (virtual +0x58 of the
// layer's units: BlurFilter main+0x7ef8e0, ReduceFilter main+0xd5d44c). Filters allocate a temporary target from the
// dynamic allocator (a game call): applying only, the allocation is past the commit point.

use super::draw::{Port, R};
use super::cubemap::{set_uniform, texture_quad};
use super::lights::{shape_state, stream_draw, SHAPES, STREAMS};
use super::scene_passes::{default_state, fullscreen};
use super::texture_slots::{alloc_target, barrier_if_slot, copy_target, release, release_sampler, set_extent, texture_assign, Host};
use super::vfx::{program_apply, program_block, program_texture};
use super::{targets, viewport};

/// [[var]] +0x38 / +0x40: the filter programs (+0x10 then +0x338 for the reduce set, +0x60 for the blur set).
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[var]]: the temporary target pool (+0x280 bytes per thread entry, flag +0x288 bit 0), main+0x7f0354 gives a target back.
const TARGET_POOL: usize = 0x4634e70;
const GATHER: usize = 0x7f0354;
/// [var] a key and main+0x2b15be0(key) / main+0x2b15c10(): the index of the thread's pool entry.
const THREAD_KEY: usize = 0x462af70;
const THREAD_VALUE: usize = 0x2b15be0;
const THREAD_FALLBACK: usize = 0x2b15c10;
/// "MultiFilterBlur" / "MultiFilterReduce".
const BLUR_NAME: usize = 0x379bcdf;
const REDUCE_NAME: usize = 0x36de527;
/// The quad's scale and offset variables.
const QUAD_SCALE: usize = 0x4634f50;
const QUAD_OFFSET: usize = 0x462cee0;
/// [[var]] +0x18: the default texture state.
const DEFAULT_TEXTURES: usize = 0x4631740;
/// "agl::TextureData" and "agl::TextureSampler" (the texture constructor main+0xbf2180).
const TEXTURE_NAME: usize = 0x37ccf1f;
const SAMPLER_NAME: usize = 0x378aff3;
/// Target object virtual +0x10 (the copy accepts these two) and +0x18 (the bind's view setup).
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;
const VIEW_SETUP: usize = 0x2a2f550;
const VIEW_SETUP_ALL_LEVELS: usize = 0x2a2f89c;
/// The blur's draw helpers the port leaves to the game: the types 1 and 2 without a filter state, and the types with one.
const BLUR_TYPE1: usize = 0x7f2d48;
const BLUR_TYPE2: usize = 0xbe1a74;
const GAUSSIAN: usize = 0x21f6568;
const GAUSSIAN_OTHER: usize = 0x21f65f4;

/// A render buffer's bind would stop at: a target another thread is setting up, a view setup that is not the known
/// ones; the context's own unbind is possible (type guard initialised).
unsafe fn bind_ok(p: &Port, rb: usize, w: usize) -> R<()> {
    let prev = p.m.r64(w + 0xf8);
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 && prev != 0 && p.vfn(prev, 0x28) != targets::RENDER_BUFFER_UNBIND {
        return Err("render buffer unbind");
    }
    for o in (0x20..=0x60).step_by(8) {
        let t = p.m.r64(rb + o);
        if t == 0 {
            continue;
        }
        if (p.m.r32(t + 0xc8) as i32) < 0 {
            return Err("render target locked");
        }
        let v = p.vfn(t, 0x18);
        if v != VIEW_SETUP && v != VIEW_SETUP_ALL_LEVELS {
            return Err("render target setup");
        }
    }
    if p.m.r8(p.m.r64(p.base + targets::CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    Ok(())
}

fn pending(p: &Port, prog: usize) -> R<()> {
    unsafe {
        if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
            return Err("agl program setup pending");
        }
    }
    Ok(())
}

// ---- the GPU footprint pass ----

/// A stack texture as the game's constructor main+0xbf2180 leaves it: the constants and, with the default texture
/// holder, its state assigned (the slot reference taken).
unsafe fn texture_default(p: &mut Port, s: usize) -> R<()> {
    p.m.w::<u32>(s + 0x58, 0xffff_ffff);
    p.m.w::<u16>(s + 0xa1, 1);
    p.m.w::<u64>(s + 0xa8, (p.base + TEXTURE_NAME) as u64);
    p.m.w::<u64>(s + 0xb0, (p.base + SAMPLER_NAME) as u64);
    p.m.w::<u64>(s + 0xb8, 0);
    p.m.w::<u64>(s + 0xc0, 0x3f80_0000_0000_0000);
    p.m.w::<u64>(s + 0xc8, 0x4170_0000_0000_0000);
    p.m.w::<u32>(s + 0xd0, 0);
    p.m.w::<u64>(s + 0xd4, 0x0101_0707_0702_0101);
    p.m.w::<u8>(s + 0xdc, 0);
    p.m.w::<u16>(s + 0xde, 0);
    p.m.w::<u64>(s + 0xe0, 0xff_0504_0302);
    p.m.w::<u8>(s + 0xe8, 0x20);
    p.m.w::<u16>(s + 0xea, 0xffff);
    let holder = p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES));
    if holder != 0 {
        texture_assign(p, s, p.m.r64(holder + 0x18))?;
    }
    Ok(())
}

/// main+0x18986f8(this, w, ctx): the footprint pass (the virtual +0 of the object at GPUFootPrintMgr+0x88). ctx +0x28 the
/// render buffer (bound, its viewport ctx +0x30), +8 the texture state sampled in slot 0 (copied to a stack texture), +0x38
/// the texture of slot 1 (the default with this +0x11 bit 1 clear), +0x40 the one of slot 2 (with this +0x10 bit 0; it
/// also selects the second program of [[this+0x18]+8]): the program applied, a default render state without blending
/// bit and colour mask, a 16-byte ring block (the viewport span: this +0xc, +8 - +0xc, 1, 0) and the full-screen
/// triangle; then the buffer is unbound and barriered. The stack viewport the game builds for its bind is unused there.
pub(super) unsafe fn footprint_draw(p: &mut Port, this: usize, w: usize, ctx: usize) -> R<()> {
    let rb = p.m.r64(ctx + 0x28);
    let vp = p.m.r64(ctx + 0x30);
    let src = p.m.r64(ctx + 8);
    let third = if p.m.r8(this + 0x10) & 1 != 0 { p.m.r64(ctx + 0x40) } else { 0 };
    let prog = p.m.r64(p.m.r64(p.m.r64(this + 0x18) + 8) + if third != 0 { 0x20 } else { 0x10 });
    pending(p, prog)?;
    if p.vfn(rb, 0x28) != targets::RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    bind_ok(p, rb, w)?;
    if p.m.predict() {
        return Err("stack texture finished by the call");
    }
    p.m.commit();
    // commit

    targets::bind(p, rb, w)?;
    viewport::apply(p, vp, w, rb);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    program_apply(p, prog, w)?;
    let tex = Host::new(p, 0x100);
    let r = footprint_textures(p, this, w, ctx, prog, src, third, tex.addr());
    release_sampler(p, tex.addr() + 0xea);
    release(p, tex.addr());
    tex.release(p);
    r?;
    let data = [p.m.f32(this + 0xc), p.m.f32(this + 8) - p.m.f32(this + 0xc), 1.0, 0.0];
    let block = p.ring_block(data.as_ptr() as usize, 0x10);
    program_block(p, prog, w, 0, block, 0)?;
    fullscreen(p, w);
    targets::unbind(p, w)?;
    targets::barrier_targets(p, rb, w);
    Ok(())
}

/// The three textures of the footprint pass: the stack copy of `src`, the optional texture of ctx +0x38 and `third`.
unsafe fn footprint_textures(p: &mut Port, this: usize, w: usize, ctx: usize, prog: usize, src: usize, third: usize, tex: usize) -> R<()> {
    texture_default(p, tex)?;
    texture_assign(p, tex, src)?;
    program_texture(p, prog, w, 0, tex)?;
    let mut t1 = p.m.r64(ctx + 0x38);
    if t1 == 0 || (p.m.r8(this + 0x11) >> 1) & 1 == 0 {
        t1 = p.m.r64(p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES)) + 0x18);
    }
    program_texture(p, prog, w, 1, t1)?;
    if third != 0 {
        program_texture(p, prog, w, 2, third)?;
    }
    Ok(())
}

// ---- the multi-filter units ----

/// The source state of a filter run (the temporary of an earlier run in ctx +8, else ctx[0]) and its size: the width (at
/// least 1) and the height (as is for texture kind 3, else 1; at least the stored one).
unsafe fn filter_source(p: &Port, ctx: usize) -> (usize, u32, u32) {
    let old = p.m.r64(ctx + 8);
    let src = if old != 0 { old } else { p.m.r64(ctx) };
    let w = (p.m.r16(src + 0x30) as u32).max(1);
    let h = p.m.r16(src + 0x32) as u32;
    let hs = if p.m.r16(src + 0x3a) == 3 { h } else { 1 };
    (src, w, hs.max(h))
}

/// The pool entry of this thread is marked: the filter's draw is left out.
unsafe fn thread_skip(p: &Port) -> bool {
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    let key = p.m.r32(p.m.r64(p.base + THREAD_KEY)) as u64;
    let tls = p.query(p.base + THREAD_VALUE, &[key]);
    let slot = if tls == 0 { p.query(p.base + THREAD_FALLBACK, &[]) as u32 } else { (tls as u32).wrapping_sub(1) };
    p.m.r8(pool.wrapping_add((slot as i32 as isize as usize).wrapping_mul(0x280)) + 0x288) & 1 != 0
}

/// A filter run's target: the temporary of `width` x `height` from the allocator (ctx +0x10 the format) kept in ctx +8,
/// its state copied to the target object ctx +0x178 (state +0x180; the buffer ctx +0x108 holds it), the buffer's extent,
/// and the source state copied to the texture ctx +0x18. Returns the previous temporary.
unsafe fn filter_target(p: &mut Port, w: usize, ctx: usize, name: usize, src: usize, width: u32, height: u32) -> R<usize> {
    let old = p.m.r64(ctx + 8);
    let t = alloc_target(p, w, name, p.m.r32(ctx + 0x10), width, height)?;
    p.m.w::<u64>(ctx + 8, t as u64);
    copy_target(p, ctx + 0x178, t)?;
    set_extent(p, ctx + 0x108, width as u16, height as u16);
    texture_assign(p, ctx + 0x18, src)?;
    Ok(old)
}

/// The end of a filter run: the previous temporary given back, and the texture's pending flags reset when ctx +0x26c is
/// set (a 0x302 pair, +0xfa = 4, +0xfb the byte of +0x268, +0x100 bit 1, setup flag bit 0).
unsafe fn filter_end(p: &mut Port, ctx: usize, old: usize) -> R<()> {
    if old != 0 {
        let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
        p.helper("filter target given back", GATHER, &[pool as u64, old as u64])?;
    }
    if p.m.r8(ctx + 0x26c) != 0 {
        p.m.w::<u16>(ctx + 0xf8, 0x302);
        p.m.w::<u8>(ctx + 0xfa, 4);
        p.m.w::<u8>(ctx + 0xfb, p.m.r32(ctx + 0x268) as u8);
        p.m.w::<u8>(ctx + 0x100, p.m.r8(ctx + 0x100) | 2);
        p.m.fetch_or32(ctx + 0xfc, 1);
        p.m.w::<u8>(ctx + 0x26c, 0);
    }
    Ok(())
}

/// The unbind and barrier after a filter's draw: the wrapper's buffer unbound, the target texture (ctx +0x1d8) barriered
/// when it has a slot.
unsafe fn filter_unbind(p: &mut Port, ctx: usize, w: usize) -> R<()> {
    targets::unbind(p, w)?;
    barrier_if_slot(p, ctx + 0x1d8, w);
    Ok(())
}

/// The filter program set (+0x10 the first program, +0x18 count, +0x20 programs of 0x158 bytes) at +`off` of the holder
/// chain: [[[[var]]+0x10]+off]+8.
unsafe fn program_set(p: &Port, off: usize) -> usize {
    let g = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    p.m.r64(p.m.r64(p.m.r64(g + 0x10) + off) + 8)
}

/// Program `i` of a set (index 0 when past the count).
unsafe fn set_program(p: &Port, set: usize, i: u32) -> usize {
    let i = if p.m.r32(set + 0x18) <= i { 0 } else { i };
    p.m.r64(set + 0x20) + i as usize * 0x158
}

/// main+0xd5d44c(this, w, ctx, scale): agl::utl::ReduceFilter::drawReduce_. The source (ctx[1] else ctx[0]) is reduced to
/// the temporary "MultiFilterReduce" (size >> scale, at least 1): the buffer ctx +0x108 bound with the full viewport, the
/// shader of this +0x150 (+ scale * 8 below scale 3) applied when set (else the reduce program: the set's first for scale
/// 0, else entry scale - 1), the program's uniforms 2 (this +0x128), 1 (1 / source size) and 0 (0, 0, 1, 1), the source
/// texture (ctx +0x18) in slot 0 and the quad shape; then unbind, barrier and the end of the run.
pub(super) unsafe fn reduce_draw(p: &mut Port, this: usize, w: usize, ctx: usize, scale: u32) -> R<()> {
    let (src, sw, sh) = filter_source(p, ctx);
    let k = scale & 31;
    let (rw, rh) = ((sw >> k).max(1), (sh >> k).max(1));
    let skip = thread_skip(p);
    let shader = if scale < 3 { p.m.r64(this + 0x150 + scale as usize * 8) } else { p.m.r64(this + 0x150) };
    let set = program_set(p, 0x338);
    let prog = if (scale as i32) < 1 { p.m.r64(set + 0x10) } else { set_program(p, set, scale - 1) };
    if !skip {
        pending(p, prog)?;
        if shader != 0 {
            pending(p, p.m.r64(shader + 8))?;
        }
        bind_ok(p, ctx + 0x108, w)?;
        let f = p.vfn(ctx + 0x178, 0x10);
        if f != SETUP_COLOR && f != SETUP_DEPTH {
            return Err("target object setup");
        }
    }
    if p.m.predict() {
        return Err("filter target allocated by the call");
    }
    p.m.commit();
    // commit

    let old = filter_target(p, w, ctx, REDUCE_NAME, src, rw, rh)?;
    if !skip {
        let rb = ctx + 0x108;
        targets::bind(p, rb, w)?;
        let vp = viewport::full(p, p.m.f32(ctx + 0x110), p.m.f32(ctx + 0x114));
        viewport::apply(p, vp.as_ptr() as usize, w, rb);
        if shader != 0 {
            program_apply(p, p.m.r64(shader + 8), w)?;
        }
        if shader == 0 {
            program_apply(p, prog, w)?;
        }
        set_uniform(p, prog, w, 2, &p.m.r32(this + 0x128).to_le_bytes());
        let mut pair = [0u8; 8];
        pair[..4].copy_from_slice(&(1.0 / sw as f32).to_le_bytes());
        pair[4..].copy_from_slice(&(1.0 / sh as f32).to_le_bytes());
        set_uniform(p, prog, w, 1, &pair);
        let mut quad = [0u8; 16];
        quad[8..12].copy_from_slice(&1f32.to_le_bytes());
        quad[12..].copy_from_slice(&1f32.to_le_bytes());
        set_uniform(p, prog, w, 0, &quad);
        program_texture(p, prog, w, 0, ctx + 0x18)?;
        let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
        shape_state(p, shapes + 0xd28, w);
        stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x1e8, w);
        filter_unbind(p, ctx, w)?;
    }
    filter_end(p, ctx, old)
}

/// The filter state object of a blur type (this +0x170 for type 1, +0x178 + kernel * 8 for 2 and 4, +0x1a8 + kernel * 8
/// for 3; none past type 4).
unsafe fn blur_object(p: &Port, this: usize, ty: u32, kernel: u32) -> usize {
    let k = kernel as usize * 8;
    match ty {
        1 => p.m.r64(this + 0x170),
        2 | 4 => p.m.r64(this + 0x178 + k),
        3 => p.m.r64(this + 0x1a8 + k),
        _ => 0,
    }
}

/// The blur program of type 3 / 4: the set's first when (kernel << 2 | 1 / 3) is not positive, else entry kernel << 2 (| 2
/// for type 4).
unsafe fn blur_program(p: &Port, ty: u32, kernel: u32) -> usize {
    let set = program_set(p, 0x60);
    let (tag, bit) = if ty == 3 { (1u32, 0u32) } else { (3, 2) };
    if ((kernel << 2) | tag) as i32 <= 0 {
        p.m.r64(set + 0x10)
    } else {
        set_program(p, set, (kernel << 2) | bit)
    }
}

/// main+0x7ef8e0(this, w, ctx, type, kernel): agl::utl::BlurFilter::drawBlur_ (the unit's draw calls it this +0x148 times).
/// The source (ctx[1] else ctx[0]) blurred into the temporary "MultiFilterBlur" (its size): the buffer ctx +0x108 bound
/// with the full viewport and, by `type`, 1 / 2: the game's draw helpers (main+0x7f2d48 / main+0xbe1a74), 3 / 4: a quad of
/// the set program (main+0xbe1b2c); with a filter state object (this +0x170 ...) the game's gaussian helpers after a
/// default render state. Type 4 then runs type 3 once more. A filter state with a callback is left to the game.
pub(super) unsafe fn blur_draw(p: &mut Port, this: usize, w: usize, ctx: usize, ty: u32, kernel: u32) -> R<()> {
    if ty == 0 {
        return Ok(());
    }
    let skip = thread_skip(p);
    let passes: &[u32] = if ty == 4 { &[4, 3] } else { &[ty] };
    if !skip {
        bind_ok(p, ctx + 0x108, w)?;
        let f = p.vfn(ctx + 0x178, 0x10);
        if f != SETUP_COLOR && f != SETUP_DEPTH {
            return Err("target object setup");
        }
        for &t in passes {
            let obj = blur_object(p, this, t, kernel);
            if obj != 0 {
                if p.m.r64(obj) != 0 {
                    return Err("blur filter state callback");
                }
            } else if t == 3 || t == 4 {
                pending(p, blur_program(p, t, kernel))?;
            }
        }
    }
    if p.m.predict() {
        return Err("filter target allocated by the call");
    }
    p.m.commit();
    // commit

    for &t in passes {
        blur_pass(p, this, w, ctx, t, kernel, skip)?;
    }
    Ok(())
}

unsafe fn blur_pass(p: &mut Port, this: usize, w: usize, ctx: usize, ty: u32, kernel: u32, skip: bool) -> R<()> {
    let (src, width, height) = filter_source(p, ctx);
    let old = filter_target(p, w, ctx, BLUR_NAME, src, width, height)?;
    if !skip {
        let rb = ctx + 0x108;
        targets::bind(p, rb, w)?;
        let vp = viewport::full(p, p.m.f32(ctx + 0x110), p.m.f32(ctx + 0x114));
        let vpa = vp.as_ptr() as usize;
        viewport::apply(p, vpa, w, rb);
        let (scale, offset) = (p.pfn(QUAD_SCALE), p.pfn(QUAD_OFFSET));
        let obj = blur_object(p, this, ty, kernel);
        if obj != 0 {
            // A state object without a callback: the default render state, then the game's helpers.
            let g = default_state(p);
            p.apply_graphics_context(g.as_ptr() as usize, w);
            if ty == 1 {
                let args = [w as u64, obj as u64, (ctx + 0x18) as u64, vpa as u64, offset as u64, scale as u64, offset as u64];
                p.helper("blur gaussian", GAUSSIAN, &args)?;
            } else {
                let args = [w as u64, obj as u64, (ctx + 0x18) as u64, vpa as u64, offset as u64];
                p.helper("blur filter", GAUSSIAN_OTHER, &args)?;
            }
        } else {
            match ty {
                1 => {
                    let args = [w as u64, (ctx + 0x18) as u64, vpa as u64, 1, offset as u64, scale as u64, offset as u64];
                    p.helper("blur type 1", BLUR_TYPE1, &args)?;
                }
                2 => {
                    let args = [w as u64, (ctx + 0x18) as u64, vpa as u64, kernel as u64, 1, 1, offset as u64];
                    p.helper("blur type 2", BLUR_TYPE2, &args)?;
                }
                3 | 4 => {
                    let prog = blur_program(p, ty, kernel);
                    program_apply(p, prog, w)?;
                    texture_quad(p, w, prog, ctx + 0x18, vpa, scale, offset)?;
                }
                _ => {}
            }
        }
        filter_unbind(p, ctx, w)?;
    }
    filter_end(p, ctx, old)
}
