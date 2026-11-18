// UI layout draws: the capture pane (main+0x98897c) and the pane tree's dispatch (main+0x85dae4). The capture pane draws
// its captured texture (game call) and, when the pane has a "CaptureScale", copies it into a scaled temporary target
// through a program; both stages call game code that records commands, so they run past the commit point (apply mode).

use super::draw::{Port, R};
use super::ui_callees::{draw_capture, draw_texture, scissor_set, viewport_set};
use super::scene_passes::default_state;
use super::texture_slots::{alloc_target, barrier_if_slot, copy_target, release, release_sampler, texture_assign, Host};
use super::vfx::program_apply;
use super::{targets, viewport};

/// [[holder]]: the program set; [[[[holder]+0x10]+0x28]+8]+0x20 is the capture copy's program.
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[var]]: the temporary target pool.
const TARGET_POOL: usize = 0x4634e70;
/// "CaptureScale" (pane extension user data), "DynamicCaptureScaled", "agl::TextureSampler(from texture)",
/// "agl::TextureData".
const SCALE_NAME: usize = 0x3718667;
const SCALED_NAME: usize = 0x37a471c;
const SAMPLER_NAME: usize = 0x37a908e;
const TEXTURE_DATA_NAME: usize = 0x37ccf1f;
/// nn::ui2d::Pane::FindExtUserDataByName(pane, name): pure.
const FIND_EXT: usize = 0x7c76b8;
/// main+0xa52b64(id, description, block, 0, tail, flag, 0): registers a texture state's slot.
const REGISTER_STATE: usize = 0xa52b64;
/// main+0x7f0354(pool, texture state): gives a temporary target back.
const RELEASE_TARGET: usize = 0x7f0354;
/// The static guard and value of the runtime type the pane's member +0x100 is matched against.
const TYPE_GUARD: usize = 0x4614f48;
const TYPE_ID: usize = 0x4614f50;
/// Target object virtual +0x10 (copy_target) and +0x18 (the view finish bind runs).
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;
const VIEW_SETUP: usize = 0x2a2f550;
const VIEW_SETUP_ALL_LEVELS: usize = 0x2a2f89c;

const TEXTURE_OBJECT_BYTES: usize = 0xf0;

/// The capture copy's program: null when the chain is not there.
unsafe fn copy_program(p: &Port) -> usize {
    let mut a = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    if a == 0 {
        return 0;
    }
    for o in [0x10, 0x28, 8] {
        a = p.m.r64(a + o);
        if a == 0 {
            return 0;
        }
    }
    p.m.r64(a + 0x20)
}

/// A render buffer's unbind virtual and its targets' finish virtuals are the ones `targets` ports.
unsafe fn buffer_known(p: &Port, rb: usize) -> R<()> {
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

/// main+0x98897c(this, ctx, flags), with +0x58 bit 0 and +0x59 set and a render state record at ctx +0x1a0 (rb, context,
/// viewport, scissor, w): the buffer unbound, the pane's capture drawn by the game (this +0x118 = the captured state; 0:
/// the buffer bound again and done). With the user data "CaptureScale" and a captured size different from the scaled
/// pane size (+0x50 / +0x54, truncated): a "DynamicCaptureScaled" target from the allocator becomes the pane's object
/// (+0x190, buffer +0x120 sized, bound, default state without blending), the capture's state is made a texture object
/// on the stack, drawn through the copy program (main+0xc58404), the buffer unbound, the barrier, the old capture
/// given back and the new state kept (+0x118). Then, for every path: the state registered when +0xe8 bit 3 is set
/// (swizzle word +0x50 set), the size recorded in the pane's member +0x100 when it is of the expected type (+0x110 /
/// +0x112), the pane entered in the context's list (+0xd8 into ctx +0x1a8, id at +0x108), the buffer bound, the viewport
/// (or the game's two viewport calls with a scissor), the context's render state and the context's counters reset (ctx
/// +0x190 / +0x198 / +0x18c).
/// Prechecked before the commit point: unbind virtuals, target setup virtuals, the program's setup, the static type guard.
pub(super) unsafe fn capture_pane(p: &mut Port, this: usize, ctx: usize, flags: usize) -> R<()> {
    if p.m.r8(this + 0x58) & 1 == 0 || p.m.r8(this + 0x59) == 0 {
        return Ok(());
    }
    let st = p.m.r64(ctx + 0x1a0);
    if st == 0 {
        return Ok(());
    }
    let (rb0, gctx, vp0, scissor, w) = (p.m.r64(st), p.m.r64(st + 8), p.m.r64(st + 0x10), p.m.r64(st + 0x18), p.m.r64(st + 0x20));
    buffer_known(p, rb0)?;
    buffer_known(p, this + 0x120)?;
    if !matches!(p.vfn(this + 0x190, 0x10), SETUP_COLOR | SETUP_DEPTH) || !matches!(p.vfn(this + 0x190, 0x18), VIEW_SETUP | VIEW_SETUP_ALL_LEVELS) {
        return Err("target object setup");
    }
    if p.m.r8(p.m.r64(p.base + TYPE_GUARD)) & 1 == 0 {
        return Err("pane type not initialised");
    }
    let prog = copy_program(p);
    if prog != 0 && (p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0) {
        return Err("agl program setup pending");
    }
    targets::unbind(p, w)?;
    // commit
    p.m.commit();
    let cap = draw_capture(
        p,
        this,
        ctx,
        this + 0xe8,
        p.m.r64(this + 0xf8),
        this + 0x120,
        this + 0x190,
        p.m.r64(this + 0xf0),
        flags,
        p.m.f32(this + 0xec),
    )?;
    p.m.w::<u64>(this + 0x118, cap as u64);
    if cap == 0 {
        return targets::bind(p, rb0, w);
    }
    let ext = p.query(p.base + FIND_EXT, &[this as u64, (p.base + SCALE_NAME) as u64]) as usize;
    if ext != 0 {
        let scale = p.m.f32(ext + p.m.r32(ext + 4) as usize);
        let (fw, fh) = (scale * p.m.f32(this + 0x50), scale * p.m.f32(this + 0x54));
        let (uw, uh) = (fw as u32, fh as u32);
        let cur = p.m.r64(this + 0x118);
        let width = (p.m.r16(cur + 0x30) as u32).max(1);
        let h = p.m.r16(cur + 0x32) as u32;
        let height = (if p.m.r16(cur + 0x3a) == 3 { h } else { 1 }).max(h);
        if width != uw || height != uh {
            let o = Host::new(p, TEXTURE_OBJECT_BYTES);
            let r = scaled_copy(p, this, ctx, w, prog, fw, fh, o.addr());
            o.release(p);
            r?;
        }
    }
    finish(p, this, ctx, rb0, vp0, scissor, gctx, w)
}

/// The scaled copy of the capture: `o` is the host texture object (the game's stack object, 0xf0 bytes).
#[allow(clippy::too_many_arguments)]
unsafe fn scaled_copy(p: &mut Port, this: usize, ctx: usize, w: usize, prog: usize, fw: f32, fh: f32, o: usize) -> R<()> {
    let _ = ctx;
    let cur = p.m.r64(this + 0x118);
    let t = alloc_target(p, w, SCALED_NAME, p.m.r8(cur + 0xa0) as u32, fw as u32, fh as u32)?;
    copy_target(p, this + 0x190, t)?;
    let (minx, maxx) = if !(fw > 0.0) { (fw, 0.0) } else { (0.0, fw) };
    let (miny, maxy) = if !(fh > 0.0) { (fh, 0.0) } else { (0.0, fh) };
    p.m.w::<f32>(this + 0x130, minx);
    p.m.w::<f32>(this + 0x134, miny);
    p.m.w::<f32>(this + 0x138, maxx);
    p.m.w::<f32>(this + 0x13c, maxy);
    p.m.w::<f32>(this + 0x128, fw);
    p.m.w::<f32>(this + 0x12c, fh);
    let vp = viewport::full(p, fw, fh);
    targets::bind(p, this + 0x120, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, this + 0x120);
    let mut g = default_state(p);
    *(g.as_mut_ptr() as *mut u8) = 0;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);

    // The stack texture object from the capture's state.
    let src = p.m.r64(this + 0x118);
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
    texture_assign(p, o, src)?;

    program_apply(p, prog, w)?;
    draw_texture(p, w, prog, o)?;
    targets::unbind(p, w)?;
    barrier_if_slot(p, this + 0x1f0, w);
    let holder = p.m.r64(this + 0xf8);
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    if holder == 0 || p.m.r64(holder + 0x560) == 0 {
        p.cpu_helper(RELEASE_TARGET, &[pool as u64, p.m.r64(this + 0x118) as u64]);
    } else {
        p.cpu_helper(RELEASE_TARGET, &[pool as u64, p.m.r64(holder + 0x560) as u64]);
        p.m.w::<u64>(holder + 0x560, 0);
    }
    p.m.w::<u64>(this + 0x118, t as u64);
    release_sampler(p, o + 0xea);
    release(p, o);
    Ok(())
}

/// The part every path runs (label at main+0x989064).
#[allow(clippy::too_many_arguments)]
unsafe fn finish(p: &mut Port, this: usize, ctx: usize, rb0: usize, vp0: usize, scissor: usize, gctx: usize, w: usize) -> R<()> {
    let holder = p.m.r64(this + 0xf8);
    if holder != 0 && p.m.r64(holder + 0x560) == p.m.r64(this + 0x118) {
        p.m.w::<u64>(holder + 0x560, 0);
    }
    if (p.m.r8(this + 0xe8) >> 3) & 1 != 0 {
        let s = p.m.r64(this + 0x118);
        let word = p.m.r16(s + 0x50) as u32 | (p.m.r8(s + 0x52) as u32) << 16;
        p.m.w::<u32>(s + 0x50, word | 0x0100_0000);
        let kind = p.m.r8(s + 0xa0);
        let flag = (p.m.r8(s + 0x54) >> 1) & 1 != 0 && kind > 0x11 && (kind as u32).wrapping_sub(0x2f) > 0x29;
        let desc: [u64; 3] = [p.m.r64(s) as u64, p.m.r64(s + 8) as u64, p.m.r64(s + 0x10) as u64];
        let tail = p.m.r64(s + 0xa8) as u64;
        p.helper("texture state registered by the call", REGISTER_STATE, &[(s + 0x58) as u64, desc.as_ptr() as u64, (s + 0x30) as u64, 0, tail, flag as u64, 0])?;
    }
    let s = p.m.r64(this + 0x118);
    let width = (p.m.r16(s + 0x30) as u32).max(1) as u16;
    let h = p.m.r16(s + 0x32);
    let height = (if p.m.r16(s + 0x3a) == 3 { h } else { 1 }).max(h);
    // The member +0x100's runtime type is the expected one or derives from it (first word chain).
    let member = this + 0x100;
    let ty = p.query(p.base + p.vfn(member, 0), &[member as u64]) as usize;
    let want = p.m.r64(p.base + TYPE_ID);
    let mut t = ty;
    if t != want {
        while {
            t = p.m.r64(t);
            t != 0 && t != want
        } {}
    }
    if t != 0 {
        p.m.w::<u16>(this + 0x110, width);
        p.m.w::<u16>(this + 0x112, height);
    }
    // The pane entered in the context's list.
    let node = this + 0xd8;
    let old = p.m.r64(node);
    p.m.w::<u64>(this + 0x108, p.m.i32(s + 0x58) as i64 as u64);
    let head = ctx + 0x1a8;
    let first = p.m.r64(head);
    p.m.w::<u64>(node, first as u64);
    p.m.w::<u64>(old + 8, head as u64);
    let first = p.m.r64(head);
    p.m.w::<u64>(first + 8, node as u64);
    p.m.w::<u64>(head, old as u64);
    targets::bind(p, rb0, w)?;
    if scissor == 0 {
        viewport::apply(p, vp0, w, rb0);
    } else {
        viewport_set(p, vp0, w, rb0);
        scissor_set(p, scissor, w, rb0);
    }
    p.apply_graphics_context(gctx, w);
    // main+0xa2bcc8(ctx): the context's counters reset.
    p.m.w::<u64>(ctx + 0x190, 0);
    p.m.w::<u16>(ctx + 0x198, 0);
    p.m.w::<u8>(ctx + 0x18c, 1);
    Ok(())
}

/// The pane tree's object call main+0x4a688(object, a1, a2).
const PANE_OBJECT_CALL: usize = 0x4a688;

/// main+0x85dae4(this, a1, a2), with +0x58 bits 0 and 7 set: with +0x5c bit 4 and +0x5a set, the pane's own draw (with
/// +0x62 bit 1: main+0x4a688 on the object found through the tables at [+0xa8], else virtual +0xe0); then, unless
/// +0x58 bit 1 is set with +0x5a clear, virtual +0xd8 of each child of the list at +0x20 (nodes 8 into the children,
/// the next read after each call). Every call is the game's (records commands): past the commit point.
pub(super) unsafe fn pane_draw(p: &mut Port, this: usize, a1: usize, a2: usize) -> R<()> {
    if (p.m.r8(this + 0x58) ^ 0xff) & 0x81 != 0 {
        return Ok(());
    }
    let own = (p.m.r8(this + 0x5c) >> 4) & 1 != 0 && p.m.r8(this + 0x5a) != 0;
    let children = ((p.m.r8(this + 0x58) >> 1) & 1 == 0 || p.m.r8(this + 0x5a) != 0) && p.m.r64(this + 0x28) != this + 0x20;
    if !own && !children {
        return Ok(());
    }
    if p.m.predict() {
        return Err("pane tree drawn by the call");
    }
    // commit
    p.m.commit();
    if own {
        if (p.m.r8(this + 0x62) >> 1) & 1 != 0 {
            let a = p.m.r64(this + 0xa8);
            let t = a + 0xc;
            let v = p.m.r32(a + 0x10) as usize;
            let i = v + 4;
            let k = p.m.r32(t + i).wrapping_add(v as u32) as usize;
            let b = p.m.r8(k + t + 0xc) as usize;
            let j = i + b * 4;
            let e = (p.m.r32(t + j).wrapping_add(v as u32) as usize) + t;
            p.call(p.base + PANE_OBJECT_CALL, &[p.m.r64(e + 8) as u64, a1 as u64, a2 as u64]);
        } else {
            p.call(p.m.r64(p.m.r64(this) + 0xe0), &[this as u64, a1 as u64, a2 as u64]);
        }
    }
    if (p.m.r8(this + 0x58) >> 1) & 1 == 0 || p.m.r8(this + 0x5a) != 0 {
        let end = this + 0x20;
        let mut n = p.m.r64(this + 0x28);
        while n != end {
            let c = n - 8;
            p.call(p.m.r64(p.m.r64(c) + 0xd8), &[c as u64, a1 as u64, a2 as u64]);
            n = p.m.r64(c + 0x10);
        }
    }
    Ok(())
}
