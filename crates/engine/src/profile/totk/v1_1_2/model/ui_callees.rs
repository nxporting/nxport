// Callees of the UI pane passes (ui_passes.rs): the capture pane's drawCapture (main+0x989510), the texture draw with
// its size block (main+0xc58404) and the buffer's viewport / scissor (main+0xa81ec8 / main+0x85dbe0).

use super::cubemap::{capture_quad, set_uniform};
use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, SHAPES, STREAMS};
use super::scene_passes::default_state;
use super::texture_slots::{alloc_target, barrier_if_slot, copy_target, release, release_sampler, texture_assign, Host};
use super::ui_passes::pane_draw;
use super::vfx::program_texture;
use super::{targets, viewport};

const PFN_SET_SCISSOR: usize = 0x466e110;
const PFN_SET_VIEWPORT: usize = 0x466e0f8;
const PFN_SET_DEPTH_RANGE: usize = 0x466e120;

/// main+0xc58404(w, prog, tex): the program's uniform 1 = (width, height, 1 / width, 1 / height) of the texture, the
/// texture at its slot 0, then the full-screen shape (shapes +0x10e8, streams +0x488).
pub(super) unsafe fn draw_texture(p: &mut Port, w: usize, prog: usize, tex: usize) -> R<()> {
    let width = (p.m.r16(tex + 0x30) as u32).max(1);
    let h = p.m.r16(tex + 0x32) as u32;
    let height = (if p.m.r16(tex + 0x3a) == 3 { h } else { 1 }).max(h);
    let (fw, fh) = (width as f32, height as f32);
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let mut size = [0u8; 16];
    for (k, v) in [fw, fh, 1.0 / fw, 1.0 / fh].iter().enumerate() {
        size[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    set_uniform(p, prog, w, 1, &size);
    program_texture(p, prog, w, 0, tex)?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);
    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x488, w);
    Ok(())
}

/// The rectangle sead::Viewport::apply maps the viewport to within the frame buffer (same operations as
/// viewport::apply).
unsafe fn rect(p: &Port, vp: usize, fb: usize) -> (i32, i32, i32, i32) {
    let f = |o: usize| p.m.f32(o);
    let (x, y) = (f(vp + 8), f(vp + 0xc));
    let (x2, y2) = (f(vp + 0x10), f(vp + 0x14));
    let (fw, fh) = (f(fb + 8), f(fb + 0xc));
    let mode = p.m.r32(vp + 0x18).wrapping_sub(1);
    let (mut a, mut b) = (y, x);
    match mode {
        0 => (a, b) = ((fh - (x2 - x)) - x, y),
        1 => (a, b) = (x, (fw - (y2 - y)) - y),
        2 => (a, b) = ((fh - (y2 - y)) - y, (fw - (x2 - x)) - x),
        3 => b = (fw - (x2 - x)) - x,
        4 => a = (fh - (y2 - y)) - y,
        _ => {}
    }
    let (r0, r1, r2, r3) = (f(fb + 0x10), f(fb + 0x14), f(fb + 0x18), f(fb + 0x1c));
    let hr = r3 - r1;
    let wr = r2 - r0;
    let x0 = r0 + (b / fw) * wr;
    let (hy, wx) = (y2 - y, x2 - x);
    let (sa, sb) = if mode > 1 { (hy, wx) } else { (wx, hy) };
    let ya = r1 + (a / fh) * hr;
    let hs = hr * (sa / fh);
    let xe = x0 + wr * (sb / fw);
    let y0 = (hr - hs) - ya;
    let rw = (xe - (x0 as i32) as f32) as u32 as i32;
    let rh = ((hs + y0) - (y0 as i32) as f32) as u32 as i32;
    (x0 as i32, y0 as i32, rw, rh)
}

/// main+0xa81ec8(viewport, w, frame buffer): the viewport's rectangle in the buffer set as the viewport, then the depth
/// range (+0x1c / +0x20). Calls the NVN entry points.
pub(super) unsafe fn viewport_set(p: &mut Port, vp: usize, w: usize, fb: usize) {
    let (x, y, rw, rh) = rect(p, vp, fb);
    let cmd = p.m.r64(w + 0xb8);
    let (near, far) = (p.m.f32(vp + 0x1c), p.m.f32(vp + 0x20));
    if p.native {
        use crate::nvn::cmd::FixedState;
        p.fixed(cmd, FixedState::Viewport { x, y, w: rw, h: rh });
        p.fixed(cmd, FixedState::DepthRange { near, far });
        return;
    }
    p.call(p.pfn(PFN_SET_VIEWPORT), &[cmd as u64, x as u64, y as u64, rw as u64, rh as u64]);
    p.flush();
    let set: unsafe extern "C" fn(u64, f32, f32) = std::mem::transmute(p.pfn(PFN_SET_DEPTH_RANGE));
    set(cmd as u64, near, far);
}

/// main+0x85dbe0(scissor, w, frame buffer): the same rectangle set as the scissor. Calls the NVN entry point.
pub(super) unsafe fn scissor_set(p: &mut Port, vp: usize, w: usize, fb: usize) {
    let (x, y, rw, rh) = rect(p, vp, fb);
    let cmd = p.m.r64(w + 0xb8);
    if p.native {
        return p.fixed(cmd, crate::nvn::cmd::FixedState::Scissor { x, y, w: rw, h: rh });
    }
    p.call(p.pfn(PFN_SET_SCISSOR), &[cmd as u64, x as u64, y as u64, rw as u64, rh as u64]);
}

/// "CapturePaneWork", "agl::TextureData", "agl::TextureSampler(from texture)".
const WORK_NAME: usize = 0x37211d2;
const TEXTURE_DATA_NAME: usize = 0x37ccf1f;
const SAMPLER_NAME: usize = 0x37a908e;
/// [[var]]: the temporary target pool; main+0x7f0354(pool, state) gives a temporary target back.
const TARGET_POOL: usize = 0x4634e70;
const RELEASE_TARGET: usize = 0x7f0354;
/// [GOT]: the quad's offset pair (two floats) the capture draw passes.
const OFFSET_PAIR: usize = 0x462cee0;
/// The capture program set (same as cubemap::capture_quad).
const PROGRAM_HOLDER: usize = 0x4634e40;
/// main+0x98a3f0(ctx; s0, s1 = viewport width, height) / main+0x117e700(ctx; s0, s1, s2 = the pane's float): the
/// pane's content drawn. Record commands.
const DRAW_CONTENT: usize = 0x98a3f0;
const DRAW_CONTENT_FLOAT: usize = 0x117e700;
/// main+0x98a614(holder, w, state): the holder's pending blur on the captured state. Records commands.
const HOLDER_BLUR: usize = 0x98a614;
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;
const VIEW_SETUP: usize = 0x2a2f550;
const VIEW_SETUP_ALL_LEVELS: usize = 0x2a2f89c;

/// A render buffer whose unbind and target setup virtuals are the ones `targets` ports.
unsafe fn known_buffer(p: &Port, rb: usize) -> R<()> {
    if p.vfn(rb, 0x28) != targets::RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    for o in (0x20..=0x60).step_by(8) {
        let t = p.m.r64(rb + o);
        if t != 0 && !matches!(p.vfn(t, 0x18), VIEW_SETUP | VIEW_SETUP_ALL_LEVELS) {
            return Err("render target setup");
        }
    }
    Ok(())
}

/// A game function taking float arguments, recording commands (stops like `helper`).
unsafe fn call_f(p: &mut Port, name: &'static str, addr: usize, ints: &[u64], floats: &[f32]) -> R<u64> {
    if p.m.stops() {
        return Err(name);
    }
    p.flush();
    crate::nvn::api::capture_note(|| format!("> call main+{addr:#x}"));
    let r = p.query_f(p.base + addr, ints, floats);
    crate::nvn::api::capture_note(|| format!("< main+{addr:#x}"));
    Ok(r)
}

/// main+0x989510(this, ctx, flags pointer, holder, buffer, target object, clear colour, pane flags, s0 = the pane's
/// float at +0xec): the capture pane's render. The size is the pane's size (+0x50 / +0x54, scaled by the matrix and
/// +0x48 / +0x4c with flag bit 7), or with bit 4 the pane in the current region's pixels, with bit 5 at least that;
/// nothing when either is 0 (returns 0). A temporary target "CapturePaneWork" (format 0x1d, 0x22 with bit 2) becomes
/// the target object's state, the buffer's size is set, the viewport is that size. With a clear colour the buffer is
/// bound, cleared in the colour and unbound; else the render state's first colour target (barrier when it has storage)
/// is drawn into the buffer as a texture object built on the stack (main+0x98aa34 = cubemap::capture_quad) and the
/// buffer unbound, the object's slot and sampler given back. Then for both: the context's graphics state, its counters
/// reset, the render state record replaced by one (buffer, graphics state, viewport, no scissor, w), the content drawn
/// (main+0x98a3f0, or with flags byte 1 bit 0 main+0x117e700), the buffer bound, the pane tree (main+0x85dae4) drawn
/// with `flags`, the buffer unbound, the record and the context's matrices (+0x10 / +0x50) restored, the barrier of
/// the target object's slot, then the holder's blur (main+0x98a614) and, when it holds a target (+0x560), the captured
/// one given back and the holder's returned. Returns the captured state.
/// Prechecked before the commit point: virtuals of the buffer and target object, the context type, the capture program.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn draw_capture(
    p: &mut Port,
    this: usize,
    ctx: usize,
    flagp: usize,
    holder: usize,
    rb: usize,
    obj: usize,
    color: usize,
    flags: usize,
    s0: f32,
) -> R<usize> {
    let st = p.m.r64(ctx + 0x1a0);
    if st == 0 {
        return Err("no render state record");
    }
    let (rb0, gctx, w) = (p.m.r64(st), p.m.r64(st + 8), p.m.r64(st + 0x20));
    let m = p.m.r64(this + 0x18);
    let fl = p.m.r16(flagp) as u32;
    let (mut sa, mut sb) = (p.m.f32(this + 0x50), p.m.f32(this + 0x54));
    if (fl >> 7) & 1 != 0 {
        sa *= p.m.f32(m + 0x70) * p.m.f32(this + 0x48);
        sb *= p.m.f32(m + 0x84) * p.m.f32(this + 0x4c);
    }
    let cs = p.m.r64(ctx + 0xc0);
    let region = |p: &Port| -> (u32, u32) {
        let rw = (sa * (p.m.f32(rb0 + 0x18) - p.m.f32(rb0 + 0x10))) / p.m.f32(cs + 0x28);
        let rh = (sb * (p.m.f32(rb0 + 0x1c) - p.m.f32(rb0 + 0x14))) / p.m.f32(cs + 0x2c);
        (rw as u32, rh as u32)
    };
    let (uw, uh) = if (fl >> 4) & 1 != 0 {
        region(p)
    } else {
        let (mut uw, mut uh) = (sa as u32, sb as u32);
        if (fl >> 5) & 1 != 0 {
            let (rw, rh) = region(p);
            uw = uw.max(rw);
            uh = uh.max(rh);
        }
        (uw, uh)
    };
    if uw == 0 || uh == 0 {
        return Ok(0);
    }

    // Prechecks.
    known_buffer(p, rb)?;
    if !matches!(p.vfn(obj, 0x10), SETUP_COLOR | SETUP_DEPTH) {
        return Err("target object setup");
    }
    if p.m.r8(p.m.r64(p.base + targets::CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 {
        let prev = p.m.r64(w + 0xf8);
        if prev != 0 && p.vfn(prev, 0x28) != targets::RENDER_BUFFER_UNBIND {
            return Err("render buffer unbind");
        }
    }
    if color == 0 {
        if p.m.r64(rb0 + 0x20) == 0 {
            return Err("render state without a colour target");
        }
        let set = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
        if p.m.r32(set + 8) < 0x2e {
            return Err("capture program missing");
        }
        let prog = p.m.r64(p.m.r64(set + 0x10) + 0x168);
        if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
            return Err("agl program setup pending");
        }
    }
    p.m.commit();
    // commit

    let t = alloc_target(p, w, WORK_NAME, if fl & 4 != 0 { 0x22 } else { 0x1d }, uw, uh)?;
    copy_target(p, obj, t)?;
    p.m.w::<u64>(rb + 0x10, 0);
    p.m.w::<f32>(rb + 8, sa);
    p.m.w::<f32>(rb + 0xc, sb);
    p.m.w::<f32>(rb + 0x18, uw as f32);
    p.m.w::<f32>(rb + 0x1c, uh as f32);
    let vp = viewport::full(p, sa, sb);
    let vpa = vp.as_ptr() as usize;

    if color != 0 {
        targets::bind(p, rb, w)?;
        targets::clear(p, rb, w, 0, 1, color, 0, vpa, 0.0)?;
        targets::unbind(p, w)?;
    } else {
        let o = Host::new(p, 0xf0);
        let r = capture_texture(p, this, ctx, flagp, rb, rb0, st, o.addr(), vpa, (sa, sb), w);
        o.release(p);
        r?;
    }

    p.apply_graphics_context(gctx, w);
    // main+0xa2bcc8(ctx): the context's counters reset.
    p.m.w::<u64>(ctx + 0x190, 0);
    p.m.w::<u16>(ctx + 0x198, 0);
    p.m.w::<u8>(ctx + 0x18c, 1);
    let saved1: [usize; 8] = std::array::from_fn(|k| p.m.r64(ctx + 0x10 + k * 8));
    let saved2: [usize; 6] = std::array::from_fn(|k| p.m.r64(ctx + 0x50 + k * 8));
    let rec = Host::new(p, 0x30);
    p.m.w::<u64>(rec.addr(), rb as u64);
    p.m.w::<u64>(rec.addr() + 8, gctx as u64);
    p.m.w::<u64>(rec.addr() + 0x10, vpa as u64);
    p.m.w::<u64>(rec.addr() + 0x18, 0);
    p.m.w::<u64>(rec.addr() + 0x20, w as u64);
    p.m.w::<u8>(rec.addr() + 0x28, 1);
    p.m.w::<u64>(ctx + 0x1a0, rec.addr() as u64);
    let dx = f32::from_bits(vp[4]) - f32::from_bits(vp[2]);
    let dy = f32::from_bits(vp[5]) - f32::from_bits(vp[3]);
    let r = content(p, this, ctx, flagp, rb, w, flags, (dx, dy), s0);
    p.m.w::<u64>(ctx + 0x1a0, st as u64);
    rec.release(p);
    r?;
    // main+0x98a5d0(ctx, saved): the matrices restored.
    for (k, v) in saved1.iter().enumerate() {
        p.m.w::<u64>(ctx + 0x10 + k * 8, *v as u64);
    }
    for (k, v) in saved2.iter().enumerate() {
        p.m.w::<u64>(ctx + 0x50 + k * 8, *v as u64);
    }
    barrier_if_slot(p, obj + 0x60, w);
    let mut t = t;
    if holder != 0 {
        p.helper("holder blur drawn by the call", HOLDER_BLUR, &[holder as u64, w as u64, t as u64])?;
        let kept = p.m.r64(holder + 0x560);
        if kept != 0 {
            let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
            p.cpu_helper(RELEASE_TARGET, &[pool as u64, t as u64]);
            t = kept;
        }
    }
    Ok(t)
}

/// The pane content, the buffer bound, the pane tree and the buffer unbound (render state record already replaced).
#[allow(clippy::too_many_arguments)]
unsafe fn content(p: &mut Port, this: usize, ctx: usize, flagp: usize, rb: usize, w: usize, flags: usize, d: (f32, f32), s0: f32) -> R<()> {
    if p.m.r8(flagp + 1) & 1 == 0 {
        call_f(p, "pane content drawn by the call", DRAW_CONTENT, &[ctx as u64], &[d.0, d.1])?;
    } else {
        call_f(p, "pane content drawn by the call", DRAW_CONTENT_FLOAT, &[ctx as u64], &[d.0, d.1, s0])?;
    }
    targets::bind(p, rb, w)?;
    pane_draw(p, this, ctx, flags)?;
    targets::unbind(p, w)
}

/// The render state's first colour target drawn into the buffer: the buffer bound with the viewport, the target's
/// state made a texture object `o` on the stack, drawn as a quad (cubemap::capture_quad with the pane's transform), the
/// buffer unbound, the object's sampler and slot given back.
#[allow(clippy::too_many_arguments)]
unsafe fn capture_texture(
    p: &mut Port,
    this: usize,
    ctx: usize,
    flagp: usize,
    rb: usize,
    rb0: usize,
    st: usize,
    o: usize,
    vp: usize,
    size: (f32, f32),
    w: usize,
) -> R<()> {
    let (sa, sb) = size;
    let src = p.m.r64(rb0 + 0x20);
    barrier_if_slot(p, src + 0x60, w);
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp, w, rb);

    p.m.w::<i32>(o + 0x58, -1);
    p.m.w::<u8>(o + 0xa1, 1);
    p.m.w::<u8>(o + 0xa2, 0);
    p.m.w::<u32>(o + 0xd0, 0);
    p.m.w::<u64>(o + 0xa8, (p.base + TEXTURE_DATA_NAME) as u64);
    p.m.w::<u64>(o + 0xb0, (p.base + SAMPLER_NAME) as u64);
    p.m.w::<u64>(o + 0xb8, 0);
    p.m.w::<u64>(o + 0xc0, 0x3f80_0000_0000_0000);
    p.m.w::<u64>(o + 0xc8, 0x4170_0000_0000_0000);
    p.m.w::<u64>(o + 0xd4, 0x0101_0707_0702_0101);
    p.m.w::<u8>(o + 0xdc, 0);
    p.m.w::<u16>(o + 0xde, 0);
    p.m.w::<u64>(o + 0xe0, 0xff_0504_0302);
    p.m.w::<u8>(o + 0xe8, 0x20);
    p.m.w::<u16>(o + 0xea, 0xffff);
    texture_assign(p, o, src + 8)?;
    // The sampler setters the game inlines.
    p.m.w::<u8>(o + 0xe0, 2);
    p.m.w::<u8>(o + 0xe1, 3);
    p.m.w::<u8>(o + 0xe2, 4);
    p.m.w::<u8>(o + 0xe3, (p.m.r8(flagp) >> 1) & 1);
    p.m.w::<u8>(o + 0xe8, p.m.r8(o + 0xe8) | 2);
    p.m.fetch_or32(o + 0xe4, 1);
    p.m.w::<u8>(o + 0xd4, 1);
    p.m.w::<u8>(o + 0xd5, 1);
    p.m.w::<u8>(o + 0xd6, 1);
    p.m.fetch_or32(o + 0xe4, 4);
    p.m.w::<u8>(o + 0xd7, 7);
    p.m.w::<u8>(o + 0xd8, 7);
    p.m.w::<u8>(o + 0xd9, 7);
    p.m.fetch_or32(o + 0xe4, 2);

    let mut g = default_state(p);
    *(g.as_mut_ptr() as *mut u8) = 0;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);

    // The quad: scale, offset and texel size from the pane's transform and the render state's viewport.
    let f = |p: &Port, a: usize| p.m.f32(a);
    let mat = p.m.r64(this + 0x18);
    let vr = p.m.r64(st + 0x10);
    let cs = p.m.r64(ctx + 0xc0);
    let (t30, t34, t38) = (f(p, this + 0x30), f(p, this + 0x34), f(p, this + 0x38));
    let (m70, m74, m78, m7c) = (f(p, mat + 0x70), f(p, mat + 0x74), f(p, mat + 0x78), f(p, mat + 0x7c));
    let (m80, m84, m88, m8c) = (f(p, mat + 0x80), f(p, mat + 0x84), f(p, mat + 0x88), f(p, mat + 0x8c));
    let (n5, n1, n0, n2, n3, n4) = (m78 * t38, m74 * t34, t38 * m88, m70 * t30, t34 * m84, t30 * m80);
    let mut s1 = n5 + n1;
    let sx = f(p, vr + 8) + f(p, vr + 0x10);
    let mut s0 = n0 + n3;
    let sy = f(p, vr + 0xc) + f(p, vr + 0x14);
    let cy = sy * 0.5;
    s1 += n2;
    let cx = sx * 0.5;
    s0 += n4;
    let (rbw, rbh) = (f(p, rb0 + 8), f(p, rb0 + 0xc));
    let (hw, hh) = (rbw * 0.5, rbh * 0.5);
    s1 = m7c + s1;
    s0 = m8c + s0;
    let (s4, s5) = (cx - hw, cy - hh);
    let (c28, c2c) = (f(p, cs + 0x28), f(p, cs + 0x2c));
    let ratio_y = c2c / rbh;
    let ratio_x = c28 / rbw;
    let a0 = sa / c28;
    let s3 = s5 * ratio_y;
    let s2 = s4 * ratio_x;
    s0 = s3 + s0;
    s1 = s2 + s1;
    let a1 = sb / c2c;
    s0 = -s0;
    let a: [f32; 2] = [a0, a1];
    let b: [f32; 2] = [s1 / sa, s0 / sb];
    let src_state = src + 8;
    let width = (p.m.r16(src_state + 0x30) as u32).max(1);
    let hgt = p.m.r16(src_state + 0x32) as u32;
    let height = (if p.m.r16(src_state + 0x3a) == 3 { hgt } else { 1 }).max(hgt);
    let c: [f32; 2] = [sa / width as f32, sb / height as f32];
    let offset = p.m.r64(p.base + OFFSET_PAIR);
    capture_quad(p, w, o, vp, a.as_ptr() as usize, b.as_ptr() as usize, c.as_ptr() as usize, offset, 0f32.to_bits())?;
    targets::unbind(p, w)?;
    release_sampler(p, o + 0xea);
    release(p, o);
    Ok(())
}
