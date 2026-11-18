// The bloom composite body (main+0x21eb800), the multi-filter expand draw (agl::utl::ExpandFilter::drawExpand_,
// main+0x2213440) and the four draw helpers the blur filter left to the game (main+0x7f2d48, main+0xbe1a74,
// main+0x21f6568, main+0x21f65f4).

use super::cubemap::{set_uniform, texture_quad};
use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, SHAPES, STREAMS};
use super::scene_passes::{default_state, fullscreen};
use super::texture_slots::{alloc_target, barrier_if_slot, copy_target, release, release_sampler, set_extent, texture_assign, Host};
use super::vfx::{program_apply, program_texture};
use super::{targets, viewport};

/// [[var]] +0x10: the program holder (+8 a version, +0x20 the bloom program, +0x340 the expand program set, ...).
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[var]]: the temporary target pool (+0x280 bytes per thread entry, flag +0x288 bit 0), main+0x7f0354 gives a target back.
const TARGET_POOL: usize = 0x4634e70;
const GATHER: usize = 0x7f0354;
/// [var] a key and main+0x2b15be0(key) / main+0x2b15c10(): the index of the thread's pool entry.
const THREAD_KEY: usize = 0x462af70;
const THREAD_VALUE: usize = 0x2b15be0;
const THREAD_FALLBACK: usize = 0x2b15c10;
/// "MultiFilterExpand".
const EXPAND_NAME: usize = 0x36de539;
/// The quad's scale and offset variables.
const QUAD_SCALE: usize = 0x4634f50;
const QUAD_OFFSET: usize = 0x462cee0;
/// agl::TextureData's constructor (0x100 bytes on the stack).
const TEXTURE_CTOR: usize = 0xbf2180;
/// Target object virtual +0x10 (the copy accepts these two) and +0x18 (the bind's view setup).
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;
const VIEW_SETUP: usize = 0x2a2f550;
const VIEW_SETUP_ALL_LEVELS: usize = 0x2a2f89c;
/// The bloom's uniform 2 / 3 hold up to 32 entries of 16 bytes.
const BLOOM_MAX: u32 = 32;

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

fn bytes_of(v: &[u32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4) }
}

// ---- the bloom composite body ----

/// The bloom program: [[[holder]+0x10]+0x20] (the holder's version must be at least 5; below it the game uses no program).
unsafe fn bloom_program(p: &Port) -> usize {
    let g = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    p.m.r64(p.m.r64(g + 0x10) + 0x20)
}

/// What main+0x21eb800 would stop at, checked before the caller's commit: a program holder older than version 5, a count
/// past the 32 entries of the stack tables, the program still being set up (+0x10 / +0x14).
pub(super) unsafe fn bloom_check(p: &Port, rb: usize, n: u32) -> R<()> {
    if p.m.r64(rb + 0x60) == 0 {
        return Ok(());
    }
    let g = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    if p.m.r32(g + 8) < 5 {
        return Err("bloom program holder version");
    }
    if n > BLOOM_MAX {
        return Err("bloom composite count");
    }
    pending(p, bloom_program(p))
}

/// main+0x21eb800(w, render buffer, n, floats, table, s0, s1): the bloom composite. Nothing without the buffer's target
/// (+0x60); the target's state (+8) copied to a stack texture; a default render state without blending bit and colour
/// mask but with the depth bit; the bloom program applied with uniform 0 = s0, 1 = s1, 4 = n, 2 = the n floats and 3 =
/// the n 16-byte table entries, both sorted by the float's rank (the number of smaller floats; a tie keeps the last
/// entry; the rest of the tables stays (0, 0, 0, 1) entries / the floats' zeros); the stack texture in slot 0 and the
/// full-screen triangle. The call of main+0x21f4afc at its start is a bare return. The game's float entries are 16 bytes
/// apart with the other three words uninitialised stack: zeros here. Applying only: the stack texture is finished by the
/// game's call.
pub(super) unsafe fn bloom_body(p: &mut Port, w: usize, rb: usize, n: u32, floats: usize, table: usize, s0: f32, s1: f32) -> R<()> {
    let target = p.m.r64(rb + 0x60);
    if target == 0 {
        return Ok(());
    }
    bloom_check(p, rb, n)?;
    if p.m.predict() {
        return Err("bloom composite stack texture finished by the call");
    }
    p.m.commit();
    // commit

    let prog = bloom_program(p);
    let tex = Host::new(p, 0x100);
    p.query(p.base + TEXTURE_CTOR, &[tex.addr() as u64]);
    let r = bloom_draw(p, w, tex.addr(), target, n, floats, table, s0, s1, prog);
    release_sampler(p, tex.addr() + 0xea);
    release(p, tex.addr());
    tex.release(p);
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn bloom_draw(p: &mut Port, w: usize, tex: usize, target: usize, n: u32, floats: usize, table: usize, s0: f32, s1: f32, prog: usize) -> R<()> {
    texture_assign(p, tex, target + 8)?;
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] |= 1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    // The entries sorted by rank: the floats at 16-byte steps, the table entries beside them.
    let n = n as usize;
    let mut sorted = vec![0u32; BLOOM_MAX as usize * 4];
    let mut entries: Vec<u32> = (0..BLOOM_MAX as usize).flat_map(|_| [0, 0, 0, 1f32.to_bits()]).collect();
    for i in 0..n {
        let si = p.m.f32(floats + i * 4);
        let mut rank = 0usize;
        for j in 0..n {
            if j != i && p.m.f32(floats + j * 4) < si {
                rank += 1;
            }
        }
        sorted[rank * 4] = si.to_bits();
        for k in 0..4 {
            entries[rank * 4 + k] = p.m.r32(table + i * 16 + k * 4);
        }
    }
    program_apply(p, prog, w)?;
    set_uniform(p, prog, w, 0, &s0.to_le_bytes());
    set_uniform(p, prog, w, 1, &s1.to_le_bytes());
    set_uniform(p, prog, w, 4, &(n as u32).to_le_bytes());
    set_uniform(p, prog, w, 2, &bytes_of(&sorted)[..n * 16]);
    set_uniform(p, prog, w, 3, &bytes_of(&entries)[..n * 16]);
    program_texture(p, prog, w, 0, tex)?;
    fullscreen(p, w);
    Ok(())
}

// ---- the multi-filter helpers ----

/// The pool entry of this thread is marked: the filter's draw is left out.
unsafe fn thread_skip(p: &Port) -> bool {
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    let key = p.m.r32(p.m.r64(p.base + THREAD_KEY)) as u64;
    let tls = p.query(p.base + THREAD_VALUE, &[key]);
    let slot = if tls == 0 { p.query(p.base + THREAD_FALLBACK, &[]) as u32 } else { (tls as u32).wrapping_sub(1) };
    p.m.r8(pool.wrapping_add((slot as i32 as isize as usize).wrapping_mul(0x280)) + 0x288) & 1 != 0
}

/// A texture state's width (at least 1) and height (as is for texture kind 3, else 1; at least the stored one).
unsafe fn tex_size(p: &Port, t: usize) -> (u32, u32) {
    let w = (p.m.r16(t + 0x30) as u32).max(1);
    let h = p.m.r16(t + 0x32) as u32;
    let hs = if p.m.r16(t + 0x3a) == 3 { h } else { 1 };
    (w, hs.max(h))
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

/// The set's first program for a selector that is not positive, else program selector - 1.
unsafe fn selected(p: &Port, set: usize, sel: i32) -> usize {
    if sel <= 0 {
        p.m.r64(set + 0x10)
    } else {
        set_program(p, set, sel as u32 - 1)
    }
}

/// main+0x2213440(this, w, ctx): agl::utl::ExpandFilter::drawExpand_, run this+0x148 times (nothing below one). Each run
/// doubles the source (ctx[1], else ctx[0]) into the temporary "MultiFilterExpand" of the smaller of ctx[0]'s size and
/// twice the source's: the buffer ctx +0x108 bound with the full viewport, the shader of this +0x150 applied when set
/// (else the first program of the set at +0x340), the program's uniforms 2 (this +0x128), 1 (1 / source width twice)
/// and 0 (0, 0, 1, 1), the source texture (ctx +0x18) in slot 0 and the quad shape; then unbind, barrier and the end of
/// the run (the previous temporary given back).
pub(super) unsafe fn expand_draw(p: &mut Port, this: usize, w: usize, ctx: usize) -> R<()> {
    if p.m.i32(this + 0x148) < 1 {
        return Ok(());
    }
    let skip = thread_skip(p);
    let shader = p.m.r64(this + 0x150);
    let prog = p.m.r64(program_set(p, 0x340) + 0x20);
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

    for _ in 0..p.m.i32(this + 0x148).max(0) {
        expand_pass(p, this, w, ctx, shader, prog, skip)?;
    }
    Ok(())
}

unsafe fn expand_pass(p: &mut Port, this: usize, w: usize, ctx: usize, shader: usize, prog: usize, skip: bool) -> R<()> {
    let first = p.m.r64(ctx);
    let previous = p.m.r64(ctx + 8);
    let src = if previous != 0 { previous } else { first };
    let ((fw, fh), (sw, sh)) = (tex_size(p, first), tex_size(p, src));
    let (width, height) = (fw.min(sw * 2), fh.min(sh * 2));
    let old = filter_target(p, w, ctx, EXPAND_NAME, src, width, height)?;
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
        let inv = 1.0 / (p.m.r16(src + 0x30) as u32).max(1) as f32;
        let mut pair = [0u8; 8];
        pair[..4].copy_from_slice(&inv.to_le_bytes());
        pair[4..].copy_from_slice(&inv.to_le_bytes());
        set_uniform(p, prog, w, 1, &pair);
        let mut quad = [0u8; 16];
        quad[8..12].copy_from_slice(&1f32.to_le_bytes());
        quad[12..].copy_from_slice(&1f32.to_le_bytes());
        set_uniform(p, prog, w, 0, &quad);
        program_texture(p, prog, w, 0, ctx + 0x18)?;
        let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
        shape_state(p, shapes + 0xd28, w);
        stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x1e8, w);
        targets::unbind(p, w)?;
        barrier_if_slot(p, ctx + 0x1d8, w);
    }
    filter_end(p, ctx, old)
}

// ---- the blur draw helpers ----

/// The program of the blur's type 1 helper (main+0x7f2d48): selector `index` of the set at +0x70.
pub(super) unsafe fn blur_type1_program(p: &Port, index: i32) -> usize {
    selected(p, program_set(p, 0x70), index)
}

/// The selector of the blur's type 2 helper (main+0xbe1a74): kernel << 2, bit 1 from `a`, bit 0 from `b`.
fn blur_type2_selector(kernel: u32, a: u32, b: u32) -> i32 {
    let mut sel = (a & 1) * 2;
    sel = (sel & 3) | (kernel << 2);
    ((sel & !1) | (b & 1)) as i32
}

/// The program of the blur's type 2 helper: selector `blur_type2_selector` of the set at +0x60.
pub(super) unsafe fn blur_type2_program(p: &Port, kernel: u32, a: u32, b: u32) -> usize {
    selected(p, program_set(p, 0x60), blur_type2_selector(kernel, a, b))
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

/// What the draw helpers of drawBlur_'s passes (type 4 runs types 4 and 3) would stop at, for the check before its commit
/// (replacing the loop over `passes` there): a filter state with a callback, a helper program still being set up.
pub(super) unsafe fn blur_check(p: &Port, this: usize, ty: u32, kernel: u32) -> R<()> {
    let passes: &[u32] = if ty == 4 { &[4, 3] } else { &[ty] };
    for &t in passes {
        let obj = blur_object(p, this, t, kernel);
        if obj != 0 {
            if p.m.r64(obj) != 0 {
                return Err("blur filter state callback");
            }
            pending(p, p.m.r64(obj + 8))?;
        } else {
            match t {
                1 => pending(p, blur_type1_program(p, 1))?,
                2 => pending(p, blur_type2_program(p, kernel, 1, 1))?,
                3 | 4 => pending(p, blur_program(p, t, kernel))?,
                _ => {}
            }
        }
    }
    Ok(())
}

/// The 8 bytes at `a` (the quad's offset), as the uniform data.
unsafe fn pair_at(p: &Port, a: usize) -> [u8; 8] {
    let mut d = [0u8; 8];
    d[..4].copy_from_slice(&p.m.r32(a).to_le_bytes());
    d[4..].copy_from_slice(&p.m.r32(a + 4).to_le_bytes());
    d
}

/// main+0x7f2d48(w, tex, viewport, index, offset, scale, offset2): the blur's type 1 draw without a filter state: the
/// program of selector `index` in the blur set at +0x70 applied, its uniform 0x12 (the first one when it has fewer) = the
/// 8 bytes at `offset`, and the quad of main+0xbe1b2c (texture_quad) with `scale` and `offset2`.
pub(super) unsafe fn blur_type1(p: &mut Port, w: usize, tex: usize, vp: usize, index: i32, offset: usize, scale: usize, offset2: usize) -> R<()> {
    let prog = blur_type1_program(p, index);
    pending(p, prog)?;
    p.m.commit();
    // commit

    program_apply(p, prog, w)?;
    set_uniform(p, prog, w, 0x12, &pair_at(p, offset));
    texture_quad(p, w, prog, tex, vp, scale, offset2)
}

/// main+0xbe1a74(w, tex, viewport, kernel, a, b, offset): the blur's type 2 draw without a filter state: the program of
/// selector (kernel << 2 | bit 1 from a | bit 0 from b) in the blur set at +0x60 applied and the quad with the global
/// scale ([0x4634f50]) and `offset`.
pub(super) unsafe fn blur_type2(p: &mut Port, w: usize, tex: usize, vp: usize, kernel: u32, a: u32, b: u32, offset: usize) -> R<()> {
    let prog = blur_type2_program(p, kernel, a, b);
    pending(p, prog)?;
    p.m.commit();
    // commit

    program_apply(p, prog, w)?;
    let scale = p.pfn(QUAD_SCALE);
    texture_quad(p, w, prog, tex, vp, scale, offset)
}

/// main+0x21f6568(w, filter state, tex, viewport, offset, scale, offset2): the blur's type 1 draw with a filter state
/// (its program at +8 applied, uniform 0x12 = the 8 bytes at `offset`, the quad with `scale` and `offset2`).
pub(super) unsafe fn gaussian(p: &mut Port, w: usize, obj: usize, tex: usize, vp: usize, offset: usize, scale: usize, offset2: usize) -> R<()> {
    let prog = p.m.r64(obj + 8);
    pending(p, prog)?;
    p.m.commit();
    // commit

    program_apply(p, prog, w)?;
    set_uniform(p, prog, w, 0x12, &pair_at(p, offset));
    texture_quad(p, w, prog, tex, vp, scale, offset2)
}

/// main+0x21f65f4(w, filter state, tex, viewport, offset): the other blur types with a filter state (its program at +8
/// applied, the quad with the global scale ([0x4634f50]) and `offset`).
pub(super) unsafe fn gaussian_other(p: &mut Port, w: usize, obj: usize, tex: usize, vp: usize, offset: usize) -> R<()> {
    let prog = p.m.r64(obj + 8);
    pending(p, prog)?;
    p.m.commit();
    // commit

    program_apply(p, prog, w)?;
    let scale = p.pfn(QUAD_SCALE);
    texture_quad(p, w, prog, tex, vp, scale, offset)
}

/// The scale and offset variables' addresses (as the blur pass passes them to the helpers).
pub(super) unsafe fn quad_variables(p: &Port) -> (usize, usize) {
    (p.pfn(QUAD_SCALE), p.pfn(QUAD_OFFSET))
}
