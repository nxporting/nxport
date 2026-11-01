// The item lists main+0xc64774 walks (17 drawer slots, main+0x27cc2c4 calling a delegate per item). Drawing items have
// two virtual functions this ports: +0x60, main+0x895ce8 (an occlusion query around the item: a query object taken from
// a pool, its sphere drawn as a box or, near the camera, as points, its sample count reported into the object) and +0x68
// (the item drawn under conditional rendering on that query).

use super::draw::{Port, R};
use super::lights::{shape_buffers, SHAPES, STREAMS};
use super::vfx::{program_apply, program_block, program_texture};

/// The query pool (lock-free ring: capacity +0x30, elements +0x38, head +0x48, tails +0x40 / +0x44 by +0x80, empty flag
/// +0x4c, taken count +0x50).
const QUERY_POOL: usize = 0x46350c8;
/// Static-init guard and object of the pool's take functor.
const QUERY_GUARD: usize = 0x46350d0;
/// The query object class: take (vfn 0, returns the object for the pool's functor), near test (+0x28: the point within
/// the sphere +0x80 / radius +0x8c), program (+0x30), setup (+0x38), draw (+0x40), sphere as data (+0x48).
const QUERY_TAKE: usize = 0x112ea24;
const QUERY_NEAR: usize = 0x2a37f14;
const QUERY_PROGRAM: usize = 0x2a37ef4;
const QUERY_SETUP: usize = 0x22084bc;
const QUERY_DRAW: usize = 0x2208b54;
const QUERY_SPHERE: usize = 0x2a37f54;
/// [[[[[holder]]+0x10]+0x3e8]+8]: +0x20 the query program, also the graphics context of near queries (+0x2b0 from 3
/// targets on).
const NEAR_CONTEXT_HOLDER: usize = 0x4634e40;

/// main+0x895ce8(item, ctx): ctx = (graphics context w, view data, view): the item's record for the view (+0x48 count,
/// +0x50 records of 16 bytes: query object, flags +8).
pub(super) unsafe fn query_item(p: &mut Port, item: usize, ctx: usize) -> R<()> {
    let view = p.m.r32(p.m.r64(ctx + 0x10) + 0x20);
    let slot = |p: &Port| p.m.r64(item + 0x50) + if p.m.r32(item + 0x48) > view { view as usize } else { 0 } * 16;
    let flags = slot(p) + 8;
    if p.m.r8(flags) & 1 != 0 {
        return Ok(());
    }
    let w = p.m.r64(ctx);
    // Take an object: the head moves first, the object is the one at the new head.
    let pool = p.m.r64(p.m.r64(p.base + QUERY_POOL));
    let n = p.m.r32(pool + 0x30);
    let next = p.m.r32(pool + 0x48) + 1;
    let head = next - if n != 0 { next / n } else { 0 } * n;
    p.m.w::<u32>(pool + 0x48, head);
    let tail = p.m.r32(pool + 0x40 + 4 * (1 - p.m.r8(pool + 0x80) as usize));
    let q = if head == tail {
        p.m.w::<u32>(pool + 0x4c, p.m.r32(pool + 0x4c) | 1);
        0
    } else if p.m.r32(pool + 0x4c) & 1 != 0 {
        0
    } else {
        let i = if p.m.r32(pool + 0x30) > head { head as usize } else { 0 };
        p.m.w::<u32>(pool + 0x50, p.m.r32(pool + 0x50) + 1);
        p.m.r64(p.m.r64(pool + 0x38) + i * 8)
    };
    if p.m.r8(p.m.r64(p.base + QUERY_GUARD)) & 1 == 0 {
        return Err("query pool functor not initialized");
    }
    if q == 0 {
        return Ok(());
    }
    if p.vfn(q, 0) != QUERY_TAKE {
        return Err("query take");
    }
    let cmd = p.m.r64(w + 0xb8);
    p.point_size(cmd, 1.0);
    p.m.w::<u64>(q + 0x80, p.m.r64(item + 0xe0) as u64);
    p.m.w::<u32>(q + 0x88, p.m.r32(item + 0xe8));
    p.m.w::<u32>(q + 0x8c, p.m.r32(item + 0x118));
    let f = p.m.r8(flags);
    p.m.w::<u8>(q + 9, if f & 4 != 0 { 1 } else { (f >> 2) & 2 });
    p.point_size(cmd, 5.0);
    p.reset_counter(cmd, 1);
    query_box(p, q, w, p.m.r64(ctx + 8))?;
    let addr = p.buffer_address_of(q + 0x10);
    p.report_counter(cmd, 1, addr + p.m.r8(q + 8) as u64 * 0x20);
    let s = slot(p);
    p.m.w::<u64>(s, q as u64);
    Ok(())
}

/// main+0x895f34(q, w, view data): near the camera (view data +8) the shared box's corners as points with the near
/// context; else the sphere (+0x80, diameter) in a ring block, the object's setup with the view's block (+0x18) and its
/// draw with the sphere's block.
unsafe fn query_box(p: &mut Port, q: usize, w: usize, data: usize) -> R<()> {
    for (slot, f) in [(0x28, QUERY_NEAR), (0x30, QUERY_PROGRAM), (0x38, QUERY_SETUP), (0x40, QUERY_DRAW), (0x48, QUERY_SPHERE)] {
        if p.vfn(q, slot) != f {
            return Err("query object class");
        }
    }
    let cmd = p.m.r64(w + 0xb8);
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let holder = p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.base + NEAR_CONTEXT_HOLDER)) + 0x10) + 0x3e8) + 8);
    // main+0x2a37f14 (no fused operations).
    let d = |k: usize| p.m.f32(data + 8 + 4 * k) - p.m.f32(q + 0x80 + 4 * k);
    let (dx, dy, dz, r) = (d(0), d(1), d(2), p.m.f32(q + 0x8c));
    if dx * dx + dy * dy + dz * dz < r * r {
        let g = p.m.r64(holder + 0x20) + if p.m.r32(holder + 0x18) < 3 { 0 } else { 0x2b0 };
        program_apply(p, g, w)?;
        p.bind_vertex_attribs(cmd, 0x10, shapes + 0xa84);
        p.bind_vertex_streams(cmd, 0x10, shapes + 0xac8);
        shape_buffers(p, shapes + 0x968, w);
        // main+0x896510(stream, w, POINTS, 1).
        let stream = p.m.r64(p.m.r64(p.base + STREAMS)) + 0x728;
        let count = p.m.i32(stream + 0x28);
        if count != 0 {
            let addr = p.buffer_address_of(stream + 0x30);
            p.m.w::<u8>(cmd + 0x10, 1);
            p.draw_elements(cmd, 0, p.m.r32(stream + 8) as i32, count, addr);
        }
        return Ok(());
    }
    // main+0x2a37f54: centre and diameter; the game copies 32 bytes of which these 16 are written.
    let mut sphere = [0u8; 32];
    for k in 0..12 {
        sphere[k] = p.m.r8(q + 0x80 + k);
    }
    sphere[12..16].copy_from_slice(&(p.m.f32(q + 0x8c) * 2.0).to_le_bytes());
    let block = p.ring_block_inline_known(sphere.as_ptr() as usize, 16, 32);
    // main+0x2a37ef4.
    let prog = p.m.r64(holder + 0x20);
    // main+0x22084bc: the program, the view's block at its slots of index 0, the box shape's vertex states and buffers.
    program_apply(p, prog, w)?;
    program_block(p, prog, w, 0, data + 0x18, 0)?;
    p.bind_vertex_attribs(cmd, 0x10, shapes + 0xa84);
    p.bind_vertex_streams(cmd, 0x10, shapes + 0xac8);
    shape_buffers(p, shapes + 0x968, w);
    // main+0x2208b54: the sphere's block at the slots of index 2, then the box (index stream 0x15e8 + 0x120 * kind).
    program_block(p, prog, w, 2, block, 0)?;
    let kind = match p.m.r8(q + 9) {
        k @ 0..=2 => k as usize,
        _ => 0,
    };
    super::lights::stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + kind * 0x120 + 0x15e8, w);
    Ok(())
}

/// main+0x7fddf0(item, ctx): the item drawn (main+0x7fdeb0), under conditional rendering on the query of its record for
/// the view when it has one.
pub(super) unsafe fn object_item(p: &mut Port, item: usize, ctx: usize) -> R<()> {
    let view = p.m.r32(p.m.r64(ctx + 0x10) + 0x20);
    let i = if p.m.r32(item + 0x48) > view { view as usize } else { 0 };
    let q = p.m.r64(p.m.r64(item + 0x50) + i * 16);
    let w = p.m.r64(ctx);
    let cmd = p.m.r64(w + 0xb8);
    if q == 0 {
        return object_draw(p, item, ctx);
    }
    let addr = p.buffer_address_of(q + 0x10);
    p.render_enable_conditional(cmd, 1, addr + p.m.r8(q + 8) as u64 * 0x20);
    object_draw(p, item, ctx)?;
    p.render_enable(cmd, true);
    Ok(())
}

/// main+0x7fdeb0: the item's program variation by its flags ([item+0x30]+0x40), the view state's graphics context when
/// its two cached choices change, the program with the view's two textures (+0x28 / +0x30, slots 1 / 0), the context's
/// texture (ctx+0x20, slot 5), the context's block (ctx+0x18, index 2), the item's block (+0x60, index 3), the item's
/// texture (+0x128, slot 6) when lit, then the box shape and its index stream (kind 2 for records flagged 0xc).
unsafe fn object_draw(p: &mut Port, item: usize, ctx: usize) -> R<()> {
    let v = p.m.r64(ctx + 0x10);
    let w = p.m.r64(ctx);
    let view = p.m.r32(v + 0x20);
    let lit = p.m.f32(item + 0x130) > 0.0;
    let flags = p.m.r32(p.m.r64(item + 0x30) + 0x40);
    let vs = p.m.r64(v);
    let near = p.m.r64(p.m.r64(v + 8) + 0x60) != 0;
    let unflagged = flags & 0x1000 == 0;
    let alt = (p.m.r8(vs + 9) >> 1) & 1 != 0;
    let choice = alt && unflagged;
    let cached = p.m.r64(vs + 0x20) + if p.m.r32(vs + 0x18) > view { view as usize } else { 0 } * 0x9e0;
    if (p.m.r8(cached + 0x94c) != 0) != near || (p.m.r8(cached + 0x94d) != 0) != choice {
        p.m.w::<u8>(cached + 0x94c, near as u8);
        p.m.w::<u8>(cached + 0x94d, choice as u8);
        let g = if p.m.r8(vs + 9) & 1 == 0 || !choice {
            if near { 0xcc } else { 0x58 }
        } else if near {
            0x1b4
        } else {
            0x140
        };
        p.apply_graphics_context(vs + g, w);
    }
    // The variation: one count per option the flags leave on (+0x30 option table, 0x28-byte entries, +0x20 count).
    let set = p.m.r64(p.m.r64(p.m.r64(p.m.r64(item + 0x30) + 0x30)) + 8);
    let (options, n) = (p.m.r64(set + 0x30), p.m.r32(set + 0x28));
    let option = |p: &Port, k: u32| p.m.r16(options + if n > k { k as usize * 0x28 } else { 0 } + 0x20) as u32;
    let shadowed = lit && flags & 0x8000 == 0;
    let mut variation = 0;
    if unflagged {
        variation += option(p, 0);
    }
    if shadowed {
        variation += option(p, 1);
    }
    if p.m.f32(item + 0x104) == 0.0 || flags & 0x2000 != 0 {
        variation += option(p, 3);
    }
    if lit && flags & 0x10000 == 0 {
        variation += option(p, 4);
    }
    let prog = if variation == 0 {
        p.m.r64(set + 0x10)
    } else {
        let k = variation - 1;
        p.m.r64(set + 0x20) + if p.m.r32(set + 0x18) > k { k as usize } else { 0 } * 0x158
    };
    program_apply(p, prog, w)?;
    program_texture(p, prog, w, 1, p.m.r64(v + 0x28))?;
    program_texture(p, prog, w, 0, p.m.r64(v + 0x30))?;
    program_texture(p, prog, w, 5, p.m.r64(ctx + 0x20))?;
    program_block(p, prog, w, 2, p.m.r64(ctx + 0x18), 0)?;
    program_block(p, prog, w, 3, item + 0x60, 0)?;
    if shadowed {
        let t = p.m.r64(item + 0x128);
        if t == 0 {
            return Err("item without its texture");
        }
        program_texture(p, prog, w, 6, t + 0x70)?;
    }
    let cmd = p.m.r64(w + 0xb8);
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    p.bind_vertex_attribs(cmd, 0x10, shapes + 0xa84);
    p.bind_vertex_streams(cmd, 0x10, shapes + 0xac8);
    shape_buffers(p, shapes + 0x968, w);
    let i = if p.m.r32(item + 0x48) > view { view as usize } else { 0 };
    let kind = if p.m.r8(p.m.r64(item + 0x50) + i * 16 + 8) & 0xc != 0 { 2 } else { 1 };
    super::lights::stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + kind * 0x120 + 0x1528, w);
    Ok(())
}
