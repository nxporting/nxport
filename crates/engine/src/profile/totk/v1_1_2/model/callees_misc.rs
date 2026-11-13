// Three callees of the scene renderer's passes that record NVN commands themselves: the mip copy quad (main+0xdf51fc),
// the half resolution pass of a view record (main+0xa548e0) and the footprint pass (main+0xe9a150, agl::utl::
// GPUFootPrintMgr's body). They run past their callers' commit point: what can stop them is checked by the `*_check`
// functions (callers add them to their own checks); the rest is applied. The half resolution pass calls the game's
// target setup (main+0x958304 / 0x956314 / 0xa3c634) itself.

use super::cubemap::{set_uniform, texture_quad};
use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw_n, SHAPES, STREAMS};
use super::scene_passes::{default_state, fullscreen};
use super::targets::{self, CONTEXT_TYPE_GUARD, RENDER_BUFFER_UNBIND};
use super::texture_slots::{assign, slot_count};
use super::vfx::{program_apply, program_block, program_texture};
use super::viewport;

/// [[[[[holder]]+0x10]+0x28]+8]: the program set of the mip copy quad (count +0x18, programs +0x20, 0x158 apart).
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[holder]]: the render buffer context table (*[+0x40] +0x810 entries, count at +0x38).
const BUFFER_TABLE: usize = 0x4639f00;
/// [[holder]] +0x50c / +0x780: the graphics settings.
const SETTINGS: usize = 0x462bbf0;
/// [[holder]]+0x40: the default texture.
const DEFAULT_TEXTURES: usize = 0x4631740;
/// libm stubs: the footprint's angle functions in the order the game calls them.
const LIBM_A: usize = 0x2b15760;
const LIBM_B: usize = 0x2b15770;
/// The footprint's float constants (degrees to radians, and the divisor's floor).
const DEG_TO_RAD: usize = 0x380f068;
const DIVISOR_FLOOR: usize = 0x380e988;

#[cfg(nx_native)]
unsafe fn libm(p: &Port, f: usize, x: f32) -> f32 {
    type F = unsafe extern "C" fn(f32) -> f32;
    std::mem::transmute::<usize, F>(p.base + f)(x)
}

#[cfg(not(nx_native))]
unsafe fn libm(_p: &Port, _f: usize, _x: f32) -> f32 {
    unimplemented!("guest calls with a float argument and result on a lifted host")
}

unsafe fn gf(p: &Port, a: usize) -> f32 {
    p.m.f32(p.base + a)
}

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

/// A program still being set up (+0x10 / +0x14).
unsafe fn program_ready(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// The context's unbind is possible (the type guard initialised).
unsafe fn unbind_ok(p: &Port) -> R<()> {
    if p.m.r8(p.m.r64(p.base + CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    Ok(())
}

/// Render-target locks the bind would stop at (a target another thread is setting up) and a previous buffer whose
/// unbind is not the known one.
unsafe fn bind_ok(p: &Port, rb: usize, w: usize) -> R<()> {
    let prev = p.m.r64(w + 0xf8);
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 && prev != 0 && p.vfn(prev, 0x28) != RENDER_BUFFER_UNBIND {
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

/// The program of the mip copy quad: the second of the set when it has more than one.
unsafe fn mip_program(p: &Port) -> usize {
    let set = p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER)) + 0x10) + 0x28) + 8);
    p.m.r64(set + 0x20) + if p.m.r32(set + 0x18) > 1 { 0x158 } else { 0 }
}

pub(super) unsafe fn mip_quad_check(p: &Port) -> R<()> {
    program_ready(p, mip_program(p))
}

/// main+0xdf51fc(w, tex, viewport, scale, offset, value in s0 as its bits): the mip copy program applied, its uniform 2
/// set to `value`, then the texture quad (main+0xbe1b2c).
pub(super) unsafe fn mip_quad(p: &mut Port, w: usize, tex: usize, vp: usize, scale: usize, offset: usize, value: u32) -> R<()> {
    let prog = mip_program(p);
    program_apply(p, prog, w)?;
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    set_uniform(p, prog, w, 2, &value.to_le_bytes());
    texture_quad(p, w, prog, tex, vp, scale, offset)
}

/// The view record `view` of the footprint context (0x128 bytes each, list +0x10, count +0x8).
unsafe fn footprint_record(p: &Port, ctx: usize, view: u32) -> usize {
    p.m.r64(ctx + 0x10).wrapping_add(clamp_index(view, p.m.r32(ctx + 8)).wrapping_mul(0x128))
}

/// What can stop main+0xe9a150: the shader's program being set up, a bind or unbind the port does not run, an
/// unregistered texture slot of `src` (registered again by the game).
pub(super) unsafe fn footprint_pass_check(p: &Port, ctx: usize, w: usize, view: u32, rb: usize, shader: usize, src: usize) -> R<()> {
    program_ready(p, p.m.r64(shader + 8))?;
    bind_ok(p, rb, w)?;
    if p.vfn(rb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    unbind_ok(p)?;
    let id = p.m.i32(src + 0x58);
    if id != -1 {
        let rec = footprint_record(p, ctx, view);
        let mut c = slot_count(p, id);
        if p.m.i32(rec + 0x58) == id {
            c -= 1;
        }
        if c == 0 {
            return Err("texture slot registered by the call");
        }
    }
    Ok(())
}

/// main+0xe9a150(ctx, w, view, rb, vp, shader, src): the view record's texture state replaced by `src`, the render
/// buffer `rb` bound with the viewport `vp` and a render state without blending, the shader's program with the record's
/// textures (+0, +0xf0, +0xf8, +0x100 by the context's flags +0x48) and uniforms 1, 2, 3 (a rotation of the record's
/// angle +0x124 and offsets), 5, 6, 7 (with ctx +0x18 = 5, from the floats ctx +0x28..+0x38), 4 (record +0x108), then
/// the full-screen triangle (shapes +0x10e8, streams +0x488) and the buffer's unbind.
pub(super) unsafe fn footprint_pass(p: &mut Port, ctx: usize, w: usize, view: u32, rb: usize, vp: usize, shader: usize, src: usize) -> R<()> {
    let rec = footprint_record(p, ctx, view);
    // main+0x1221968: the texture state assigned (slot references moved), then the setup flag.
    assign(p, rec, src)?;
    p.m.fetch_or32(rec + 0xe4, 1);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp, w, rb);
    let prog = p.m.r64(shader + 8);
    program_apply(p, prog, w)?;
    let flags = p.m.r32(ctx + 0x48);
    let have_a = p.m.r64(rec + 0xf0) != 0;
    let have_b = p.m.r64(rec + 0xf8) != 0;
    let have_c = p.m.r64(rec + 0x100) != 0;
    let s0 = if flags & 0x20 == 0 { 1.0f32 } else { p.m.f32(rec + 0x110).ceil() };
    let use_d = have_b && (flags >> 1) & 1 != 0;
    let use_e = have_a && (flags >> 4) & 1 != 0;
    let u0 = [s0, 1.0f32, p.m.f32(ctx + 0x20), p.m.f32(ctx + 0x24)];
    program_texture(p, prog, w, 1, rec)?;
    set_vec(p, prog, w, 1, u0);
    // The rotation of the record's angle.
    let s8 = p.m.f32(rec + 0x124) * gf(p, DEG_TO_RAD);
    let s9 = libm(p, LIBM_A, s8);
    let s0 = libm(p, LIBM_B, s8);
    let s1 = p.m.f32(rec + 0x114);
    let s2 = p.m.f32(rec + 0x118);
    let s3 = s9 * s1;
    let s1 = s0 * s1;
    let s4 = s9 * s2;
    let s5 = -(s0 * s2);
    let s0 = s0 * s2;
    let s6 = p.m.f32(rec + 0x120) + -0.5;
    let s5b = p.m.f32(rec + 0x11c) + 0.5;
    let s16 = s1 * s6;
    let s3b = s3 * s5b;
    let x = (s16 - s3b) + 0.5;
    let s1b = p.m.f32(rec + 0x11c) + 0.5;
    let s2b = p.m.f32(rec + 0x120) + -0.5;
    let y = (s0 * s1b + s4 * s2b) + 0.5;
    set_vec(p, prog, w, 2, [s3, s5, s1, s4]);
    set_vec(p, prog, w, 3, [x, y, 0.0, 0.0]);
    if p.m.r32(ctx + 0x18) == 5 {
        let (s0, s1) = (p.m.f32(ctx + 0x28), p.m.f32(ctx + 0x2c));
        let (s2, s3) = (p.m.f32(ctx + 0x30), p.m.f32(ctx + 0x34));
        let s8 = p.m.f32(ctx + 0x38);
        let s7 = 1.0 - s3;
        let s5 = 1.0 - s2;
        let s6 = s8 - s0;
        let s16 = s1 - s8;
        let s2n = -s2;
        let s1a = s1 * s7;
        let s6a = s5 * s6;
        let s17 = s7 * s16;
        let s1b = s1a - s8;
        let s16b = (s17 + s6a).max(gf(p, DIVISOR_FLOOR));
        let s6b = s6a / s16b;
        let a0 = s5 * s6b;
        let s17b = s6b * -s0;
        let a2 = s5 * s17b;
        let a3 = s8 - s0 * s5;
        let s0f = 1.0 - s6b;
        let b0 = s0f + s3 * s6b;
        let c0 = s1b * s6b - s8 * s0f;
        set_vec(p, prog, w, 5, [a0, s2n, a2, a3]);
        set_vec(p, prog, w, 6, [b0, s3, c0, s1b]);
        set_uniform(p, prog, w, 7, &s8.to_le_bytes());
    }
    let tex0 = if use_e { p.m.r64(rec + 0xf0) } else { p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES) + 0x40) };
    program_texture(p, prog, w, 0, tex0)?;
    if use_d {
        let mut pair = [0u8; 8];
        for (k, b) in pair.iter_mut().enumerate() {
            *b = p.m.r8(rec + 0x108 + k);
        }
        set_uniform(p, prog, w, 4, &pair);
        program_texture(p, prog, w, 3, p.m.r64(rec + 0xf8))?;
    }
    if flags & 1 != 0 && have_c {
        program_texture(p, prog, w, 2, p.m.r64(rec + 0x100))?;
    }
    // main+0xd29c48 is a return.
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);
    stream_draw_n(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x488, w, 1);
    targets::unbind(p, w)
}

unsafe fn set_vec(p: &mut Port, prog: usize, w: usize, k: usize, v: [f32; 4]) {
    let mut b = [0u8; 16];
    for (i, x) in v.iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    set_uniform(p, prog, w, k, &b);
}

/// The half resolution pass's parts: the bits for the two halves of a view record's work, the record and the shader's
/// index (the sum of the table entries' counters the context buffer table selects).
struct Half {
    lo: u32,
    hi: u32,
    m1: u32,
    m2: u32,
    rec: usize,
    k: usize,
    c1: usize,
}

unsafe fn half_parts(p: &Port, ctx: usize, view: u32, bits: u32, c: usize) -> Half {
    let c1 = c & 1;
    let lo = if c1 != 0 { 4 } else { 2 };
    let hi = if c1 != 0 { 0x10 } else { 8 };
    let rec = p.m.r64(ctx + 0x10).wrapping_add(clamp_index(view, p.m.r32(ctx + 8)).wrapping_mul(0x21b0));
    Half { lo, hi, m1: lo & bits, m2: hi & bits, rec, k: 1 + c1, c1 }
}

/// The context buffer table's entry for the pass (nothing with six entries or fewer).
unsafe fn table_entry(p: &Port) -> usize {
    let t = p.m.r64(p.m.r64(p.base + BUFFER_TABLE));
    if p.m.r32(t + 0x38) > 6 {
        p.m.r64(t + 0x40).wrapping_add(0x810)
    } else {
        0
    }
}

/// The 16-bit counter at `off` of the entry's table (+8 -> +0x30, 0x28 bytes apart) after the entry `n` of them.
unsafe fn counter(p: &Port, entry: usize, off: usize) -> u32 {
    let q = p.m.r64(entry + 8);
    p.m.r16(p.m.r64(q + 0x30) + off + 0x20) as u32
}

unsafe fn counts(p: &Port, entry: usize) -> u32 {
    p.m.r32(p.m.r64(entry + 8) + 0x28)
}

/// The shader index before the target setups' own terms: the record's bit 6, the table entries' counters by the flags.
unsafe fn shader_base(p: &Port, h: &Half, b: u32, d: u32) -> u32 {
    let e = table_entry(p);
    let n = counts(p, e);
    let mut s = if p.m.r8(h.rec) & 0x40 != 0 { counter(p, e, if n > 2 { 0x50 } else { 0 }) } else { 0 };
    if b & 1 != 0 {
        s = s.wrapping_add(counter(p, e, if n > 3 { 0x78 } else { 0 }));
    }
    let extra = if d & 1 != 0 { 0 } else { counter(p, e, if n > 5 { 0xc8 } else { 0 }) };
    extra.wrapping_add(s)
}

/// The shader of the pass with every term, from the table entries as they are now.
unsafe fn shader_of(p: &Port, ctx: usize, h: &Half, b: u32, d: u32) -> usize {
    let e = table_entry(p);
    let mut s = shader_base(p, h, b, d);
    if h.m1 != 0 {
        s = s.wrapping_add(counter(p, e, 0));
    }
    if h.m2 != 0 {
        s = s.wrapping_add(counter(p, e, if counts(p, e) > 1 { 0x28 } else { 0 }));
    }
    let n = p.m.r32(ctx + 0x108);
    let i = if n > s { s } else { 0 } as usize;
    p.m.r64(ctx + 0x110).wrapping_add(i.wrapping_mul(0x18))
}

/// What can stop main+0xa548e0 before its game calls: the shader's program being set up (computed with the table as
/// it is; the target setups change none of its terms), a bind or unbind the port does not run.
pub(super) unsafe fn half_res_pass_check(p: &Port, ctx: usize, w: usize, view: u32, bits: u32, b: u32, c: usize, d: u32) -> R<()> {
    let h = half_parts(p, ctx, view, bits, c);
    if h.m1 == 0 && h.m2 == 0 {
        return Ok(());
    }
    program_ready(p, p.m.r64(shader_of(p, ctx, &h, b, d) + 8))?;
    unbind_ok(p)?;
    let prev = p.m.r64(w + 0xf8);
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 && prev != 0 && p.vfn(prev, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer unbind");
    }
    Ok(())
}

/// main+0xa548e0(ctx, w, view, bits, b, c, d): the half resolution work of the view record (ctx +0x10, 0x21b0 each):
/// with `bits` & (c bit 0 ? 4 : 2) the first set of targets (the game's main+0x958304) and with `bits` & (c bit 0 ? 0x10 :
/// 8) the second (main+0x956314; without them main+0xa3c634 gives the second's resources back), then a render state, the
/// shader of the context's table (index from the buffer table's counters, flags b and d) with the record's block
/// (+0x2120) and texture, the buffer (+0x1c8 + the set * 0x740) bound with its viewport, the full-screen triangle
/// (unless ctx +0x1c bit 1), the buffer unbound, the targets' barriers, the record's flags (+8) and its slots restored.
pub(super) unsafe fn half_res_pass(p: &mut Port, ctx: usize, w: usize, view: u32, bits: u32, b: u32, c: usize, d: u32) -> R<()> {
    let h = half_parts(p, ctx, view, bits, c);
    if h.m1 == 0 && h.m2 == 0 {
        return Ok(());
    }
    let (rec, k, c1) = (h.rec, h.k, h.c1);
    let set = rec.wrapping_add(k.wrapping_mul(0x740));
    let slot = set + 0x1e8;
    let old = p.m.r64(slot);
    let mut g = default_state(p);
    g[1] = 0;
    let e = table_entry(p);
    let mut s = shader_base(p, &h, b, d);
    if h.m1 != 0 {
        p.helper("half resolution targets by the call", 0x958304, &[ctx as u64, w as u64, view as u64, c1 as u64])?;
        p.m.w::<u64>(slot, (set + 0x328) as u64);
        g[7] |= 0xf;
        s = s.wrapping_add(counter(p, e, 0));
    } else {
        p.m.w::<u64>(slot, 0);
        g[7] = 0;
    }
    if h.m2 != 0 {
        g[0] = (g[0] & 0xffff_0000) | 0x101;
        g[0x19] = (g[0x19] & 0xffff_ff00) | 8;
        p.helper("half resolution targets by the call", 0x956314, &[ctx as u64, w as u64, view as u64, c1 as u64])?;
        p.m.w::<u64>(set + 0x228, (set + 0x418) as u64);
        s = s.wrapping_add(counter(p, e, if counts(p, e) > 1 { 0x28 } else { 0 }));
    } else {
        p.helper("half resolution release by the call", 0xa3c634, &[ctx as u64, view as u64, h.hi as u64])?;
        g[0] &= 0xffff_0000;
        p.m.w::<u64>(set + 0x228, 0);
    }
    // main+0xc56c70(shader, w, 1), then the record's block and texture.
    let n = p.m.r32(ctx + 0x108);
    let i = if n > s { s } else { 0 } as usize;
    let shader = p.m.r64(ctx + 0x110).wrapping_add(i.wrapping_mul(0x18));
    let prog = p.m.r64(shader + 8);
    program_apply(p, prog, w)?;
    program_block(p, prog, w, 0, rec + 0x2120, 0)?;
    p.m.fetch_or32(rec + c1 * 0x740 + 0x6f4, 4);
    let bit = ((p.m.r32(ctx + 0x1c) >> 5) & 1) as u8;
    let base = rec + 0x1c8 + c1 * 0x740;
    p.m.w::<u8>(base + 0x51c, bit);
    p.m.w::<u8>(base + 0x51d, bit);
    p.m.w::<u8>(base + 0x51e, 0);
    let tex = base + if d & 1 != 0 { 0x2d0 } else { 0xf0 } + 0x358;
    program_texture(p, prog, w, 0, tex)?;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    // The buffer of the set, bound with its viewport (main+0x8660b4 with no clears).
    let rb = set + 0x1c8;
    let vp = rb + 0x718;
    viewport::apply(p, vp, w, rb);
    let values = [0u64; 0x11];
    targets::bind_clear(p, rb, w, vp, 0, values.as_ptr() as usize)?;
    if p.m.r8(ctx + 0x1c) & 2 == 0 {
        fullscreen(p, w);
    }
    targets::unbind(p, w)?;
    let cmd = p.m.r64(w + 0xb8);
    if h.m1 != 0 {
        p.m.w::<u32>(rec + 8, p.m.r32(rec + 8) | h.lo);
        let t = p.m.r64(slot);
        if p.m.i32(t + 0x60) != -1 {
            let bits = targets::barrier_bits(p);
            p.barrier(cmd, bits);
        }
    }
    if h.m2 != 0 {
        p.m.w::<u32>(rec + 8, p.m.r32(rec + 8) | h.hi);
        let t = p.m.r64(set + 0x228);
        if p.m.i32(t + 0x60) != -1 {
            let bits = targets::barrier_bits(p);
            p.barrier(cmd, bits);
        }
    }
    p.m.w::<u64>(slot, old as u64);
    if d & 1 == 0 && p.m.r8(ctx + 0x1c) & 0x40 == 0 {
        let settings = p.m.r64(p.m.r64(p.base + SETTINGS));
        p.barrier(cmd, if (p.m.r8(settings + 0x50c) >> 1) & 1 == 0 { 0x11 } else { 0x111 });
    }
    if c1 == 0 {
        let r = p.m.r64(rec + 0x2118);
        p.m.w::<u64>(r + 0xac0, (set + 0x328) as u64);
    }
    Ok(())
}
