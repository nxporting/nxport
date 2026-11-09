// The remaining Err stops of the render list, occlusion item and decal drawer ports:
// - the render list variants 1-3, 5-7 and 8-15 (table main+0x45bdd28: bit 0 events to the pass delegate at args+0x50,
//   bit 1 flag select, bit 2 one model index, bit 3 conditional rendering on the drawable's query),
// - occlusion items whose virtual +0x28 is not constant 0 (near points) and the second item class,
// - the decal drawer's per-decal draws (virtual +0x10).

use super::draw::{Port, R};
use super::lights::{shape_buffers, shape_state, stream_draw, SHAPES, STREAMS};
use super::vfx::{program_apply, program_block, shadow_mask_setup};

const RET: u32 = 0xd65f03c0;

/// render_list.rs entry_state / the entry's virtual draw: (p, ctx, entry) / (p, ctx, entry, flags).
pub(super) type EntryState = unsafe fn(&mut Port, usize, usize) -> R<()>;
pub(super) type EntryDraw = unsafe fn(&mut Port, usize, usize, u32) -> R<()>;

/// main+0x12de2a0 / 0x221d720 / 0x221d890 / 0x221d960 / 0xe46da0 / 0x221dae0 / 0x221dc50 / 0x221dd30 and the eight
/// 0x221dec0.. variants (table main+0x45bdd28, index `variant`): the render list walk of main+0x931960.
/// `delegate` = args+0x50 (events), `model` = args+0x10 (variants with bit 2), `flags` = the entry draw flags.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn draw_list_variant(
    p: &mut Port,
    variant: u32,
    list: usize,
    count: u32,
    c: usize,
    mask: u32,
    model: u32,
    delegate: usize,
    flags: u32,
    state: EntryState,
    draw: EntryDraw,
) -> R<()> {
    let (events, split, only, cond) = (variant & 1 != 0, variant & 2 != 0, variant & 4 != 0, variant & 8 != 0);
    // The event block: context, drawable (+8), lod (+0x10), kind (+0x14), count (+0x18), entry index (+0x1c).
    let mut arg = [c as u64, 0u64, 0u64, count as u64 | (u32::MAX as u64) << 32];
    if events {
        event(p, delegate, &arg)?;
    }
    let mut sub_last = 0u64;
    for i in 0..(count as i32).max(0) as usize {
        let e = p.m.r64(list + i * 8);
        if p.m.r32(e + 8) & mask == 0 {
            continue;
        }
        if only && p.m.r16(e + 0xc) as u32 != model {
            continue;
        }
        state(p, c, e)?;
        let d = p.m.r64(e + 0x10);
        let sub = ((p.m.r16(e + 0xe) >> 6) & 7) as u64;
        if cond && p.m.r8(p.m.r64(c + 0x10) + 0xa) & 1 != 0 {
            conditional(p, c, p.m.r64(p.m.r64(d + 8) + 0x50));
        }
        let f = if split && p.m.r32(c + 0x50) & p.m.r32(e + 8) != 0 { flags | 0x10 } else { flags };
        sub_last = sub;
        if events {
            arg[1] = d as u64;
            arg[2] = sub | 1 << 32;
            arg[3] = count as u64 | (i as u64) << 32;
            event(p, delegate, &arg)?;
        }
        draw(p, c, e, f)?;
        if events {
            arg[2] = sub | 2 << 32;
            event(p, delegate, &arg)?;
        }
    }
    if events {
        arg[1] = 0;
        arg[2] = sub_last | 3 << 32;
        arg[3] = count as u64 | (count as u64) << 32;
        event(p, delegate, &arg)?;
    }
    Ok(())
}

/// The pass delegate (a sead::Delegate1 at args+0x50) invoked with an event block: an empty one or one whose target
/// starts with `ret` does nothing; others are the game's call.
unsafe fn event(p: &mut Port, o: usize, arg: &[u64; 4]) -> R<()> {
    match p.delegate_target(o) {
        Some(None) => return Ok(()),
        Some(Some(f)) if p.m.r32(p.base + f) == RET => return Ok(()),
        _ => {}
    }
    p.helper("render list delegate by the call", p.vfn(o, 0), &[o as u64, arg.as_ptr() as u64])?;
    Ok(())
}

/// main+0x221df94(c, q): the conditional rendering of the drawable's query object `q` (ctx +0x100 holds the one in
/// force): the old one ends, the new one begins when it is visible in the context's view (+0x10 bit of the view
/// index, ctx +0x30), else none.
unsafe fn conditional(p: &mut Port, c: usize, q: usize) {
    let cur = p.m.r64(c + 0x100);
    if cur == q {
        return;
    }
    let cmd = p.m.r64(p.m.r64(c + 0x20) + 0xb8);
    if cur != 0 {
        p.render_enable(cmd, true);
    }
    let mut q = q;
    if q != 0 {
        if (p.m.r16(q + 0xa) as u32 >> (p.m.r8(c + 0x30) as u32 & 31)) & 1 != 0 {
            let at = p.buffer_address_of(q + 0x10) + p.m.r8(q + 8) as u64 * 0x20;
            p.render_enable_conditional(cmd, 1, at);
        } else {
            q = 0;
        }
    }
    p.m.w::<u64>(c + 0x100, q as u64);
}

// ---------------------------------------------------------------------------------------------------------------
// Occlusion items, main+0xa6c3a4(q, w, view, flags).

const ITEM_PROGRAM: usize = 0x2a3819c;
const ITEM_SETUP: usize = 0x2208e68;
const ITEM_DRAW: usize = 0x2208ff8;
/// The query object class used as an occlusion item: near test, program, setup, draw.
const QUERY_NEAR: usize = 0x2a37f14;
const QUERY_PROGRAM: usize = 0x2a37ef4;
const QUERY_SETUP: usize = 0x22084bc;
const QUERY_DRAW: usize = 0x2208b54;
const NEAR_CONTEXT_HOLDER: usize = 0x4634e40;
/// The pool object type id and the functor's guard (items.rs).
const QUERY_TYPE_ID: usize = 0x46350d8;

/// main+0xa6c3a4(q, w, view, flags): replaces occlusion.rs `draw`. Items whose near test (+0x28) answers yes draw
/// the shared box's corners as points; the others through their program getter (+0x30), setup (+0x38) and draw
/// (+0x40): the two classes known are ported, other classes are the game's own call.
pub(super) unsafe fn draw(p: &mut Port, q: usize, w: usize, view: u32, flags: u64) -> R<()> {
    let v = if p.m.r32(q + 0x98) <= view { 0 } else { view } as usize;
    let data = p.m.r64(q + 0xa0) + v * 0x98;
    let cmd = p.m.r64(w + 0xb8);
    p.point_size(cmd, p.m.f32(q + 0xa8));
    let items = p.m.r64(q + 0x10);
    let mut last = 0usize;
    for k in 0..p.m.r32(q + 8) as usize {
        let it = p.m.r64(items + k * 8);
        if (1u32 << (view & 31)) & p.m.r16(it + 10) as u32 == 0 {
            continue;
        }
        if flags & 1 != 0 {
            p.reset_counter(cmd, 1);
        }
        let f = p.vfn(it, 0x28);
        let near = match p.constant_result(f) {
            Some(r) => r != 0,
            None if f == QUERY_NEAR => near_test(p, it, data + 8),
            // An item class of its own: its test of the camera position (data +8), a pure function.
            None => p.query(p.base + f, &[it as u64, (data + 8) as u64]) & 1 != 0,
        };
        if near {
            near_points(p, w)?;
            last = 0;
        } else {
            let (get, setup, draw) = (p.vfn(it, 0x30), p.vfn(it, 0x38), p.vfn(it, 0x40));
            let prog = p.query(p.base + get, &[it as u64]) as usize;
            match (get, setup, draw) {
                (ITEM_PROGRAM, ITEM_SETUP, ITEM_DRAW) => {
                    if prog != last {
                        // main+0x2208e68: the program, its block of this view, the box shape's vertex states and buffers.
                        program_apply(p, prog, w)?;
                        program_block(p, prog, w, 0, data + 0x18, 0)?;
                        shape_state(p, p.m.r64(p.m.r64(p.base + SHAPES)) + 0x5a8, w);
                        last = p.query(p.base + get, &[it as u64]) as usize;
                    }
                    // main+0x2208ff8: the item's element of the items' block at the program's slots of index 3, then its
                    // box (index stream 0x2268 + 0x60 * kind +9, kinds above 2 as 0).
                    program_block(p, prog, w, 3, q + 0x18, k as i32)?;
                    let kind = match p.m.r8(it + 9) {
                        k @ 0..=2 => k as usize,
                        _ => 0,
                    };
                    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + kind * 0x60 + 0x2268, w);
                }
                (QUERY_PROGRAM, QUERY_SETUP, QUERY_DRAW) => {
                    if prog != last {
                        // main+0x22084bc: the program, its block of this view, the box shape's vertex states and buffers.
                        program_apply(p, prog, w)?;
                        program_block(p, prog, w, 0, data + 0x18, 0)?;
                        shape_state(p, p.m.r64(p.m.r64(p.base + SHAPES)) + 0x968, w);
                        last = p.query(p.base + get, &[it as u64]) as usize;
                    }
                    // main+0x2208b54: the item's element at the slots of index 2, the box (index stream
                    // 0x15e8 + 0x120 * kind, kinds above 2 as 0).
                    program_block(p, prog, w, 2, q + 0x18, k as i32)?;
                    let kind = match p.m.r8(it + 9) {
                        k @ 0..=2 => k as usize,
                        _ => 0,
                    };
                    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + kind * 0x120 + 0x15e8, w);
                }
                _ => {
                    if prog != last {
                        p.helper("occlusion item setup by the call", setup, &[it as u64, w as u64, (data + 0x18) as u64])?;
                        last = p.query(p.base + get, &[it as u64]) as usize;
                    }
                    p.helper("occlusion item draw by the call", draw, &[it as u64, w as u64, (q + 0x18) as u64, k as u64])?;
                }
            }
        }
        if flags & 1 != 0 {
            let at = p.buffer_address_of(it + 0x10) + p.m.r8(it + 8) as u64 * 0x20;
            p.report_counter(cmd, 1, at);
        }
    }
    p.barrier(cmd, 1);
    Ok(())
}

/// main+0x2a37f14(item, camera): the camera within the item's sphere (+0x80 centre, +0x8c radius), no fused operations.
unsafe fn near_test(p: &Port, it: usize, cam: usize) -> bool {
    let d = |k: usize| p.m.f32(cam + 4 * k) - p.m.f32(it + 0x80 + 4 * k);
    let (dx, dy, dz, r) = (d(0), d(1), d(2), p.m.f32(it + 0x8c));
    dx * dx + dy * dy + dz * dz < r * r
}

/// The near branch of main+0xa6c3a4: the shared box shape's vertex states and buffers, the near context's program
/// (the one after the program of the view's context list; +0x2b0 from 3 targets on), and the box corners as points
/// (main+0x896510(stream +0x728, w, POINTS, 1)).
unsafe fn near_points(p: &mut Port, w: usize) -> R<()> {
    let cmd = p.m.r64(w + 0xb8);
    let holder = p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.base + NEAR_CONTEXT_HOLDER)) + 0x10) + 0x3e8) + 8);
    let g = p.m.r64(holder + 0x20) + if p.m.r32(holder + 0x18) < 3 { 0 } else { 0x2b0 };
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    p.bind_vertex_attribs(cmd, 0x10, shapes + 0xa84);
    p.bind_vertex_streams(cmd, 0x10, shapes + 0xac8);
    shape_buffers(p, shapes + 0x968, w);
    program_apply(p, g, w)?;
    let stream = p.m.r64(p.m.r64(p.base + STREAMS)) + 0x728;
    let count = p.m.i32(stream + 0x28);
    if count != 0 {
        let addr = p.buffer_address_of(stream + 0x30);
        p.m.w::<u8>(cmd + 0x10, 1);
        p.draw_elements(cmd, 0, p.m.r32(stream + 8) as i32, count, addr);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Query pool object (items.rs query_item, main+0x895ce8): the pool's functor vfn 0 = main+0x112ea24 returns the object
// when its argument is the type id; replaces `if p.vfn(q, 0) != QUERY_TAKE { return Err("query take") }` and the
// following uses of q.

const QUERY_TAKE: usize = 0x112ea24;

/// The object the pool's functor gives for `q` (the game calls it with the type id object): `q` itself for the known
/// functor, else the game's pure function.
pub(super) unsafe fn query_object(p: &mut Port, q: usize) -> usize {
    let f = p.vfn(q, 0);
    if f == QUERY_TAKE {
        return q;
    }
    p.query(p.base + f, &[q as u64, p.m.r64(p.base + QUERY_TYPE_ID) as u64]) as usize
}

/// items.rs object_draw: the item's texture (+0x128, +0x70 when set) at slot 6 of the program. replaces
/// `let t = ...; if t == 0 { return Err("item without its texture") } program_texture(.., 6, t + 0x70)?`: the game
/// passes 0 when there is none (main+0x96ac04 returns before touching it when the program has no slot, and reads
/// null +0xe4 otherwise, a crash of the game itself).
pub(super) unsafe fn item_texture(p: &mut Port, prog: usize, w: usize, item: usize) -> R<()> {
    let t = p.m.r64(item + 0x128);
    super::vfx::program_texture(p, prog, w, 6, if t == 0 { 0 } else { t + 0x70 })?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Decal drawer (main+0xed7a84 pass 1), vfx.rs decal_drawers.

const DECAL_BLOCK_BASE: usize = 0x418;

/// The decal drawer loop of main+0xed7a84: agl::fx::DecalDrawer::draw(drawer, w, view, rec+0x418, rec+0x610,
/// rec+0x700, decal, 1) per decal (main+0xed81a0): with the drawer enabled (+0x220) the view's mask is set up from
/// rec+0x700 (agl::sdw::ShadowMask::setupOffscrBuffer) and the decal's own draw (virtual +0x10: decal, w, view,
/// rec+0x418, mask, drawer mode +0x240, 1) is the game's call (NVN-emitting; the decal classes are not ported).
/// Replaces vfx.rs decal_drawers (which stopped at the first decal).
pub(super) unsafe fn decal_drawers(p: &mut Port, obj: usize, w: usize, view: u32) -> R<()> {
    let dd = p.m.r64(obj + 0x1e60);
    let n = p.m.i32(dd + 0x210);
    if n < 1 {
        return Ok(());
    }
    let drawer = p.m.r64(obj + 0x1e50);
    let views = p.m.r64(obj + 0x1ec0);
    let vk = if p.m.r32(views + 8) > view { view } else { 0 } as usize;
    let rec = p.m.r64(views + 0x10) + vk * 0x21b0 + (p.m.r32(drawer + 0x240) == 1) as usize * 0x740;
    for i in 0..n as usize {
        let d = p.m.r64(p.m.r64(dd + 0x218) + i * 8);
        if p.m.r8(drawer + 0x220) == 0 {
            continue;
        }
        let k = if p.m.r32(drawer + 0x1c0) > view { view } else { 0 } as usize;
        let mask = p.m.r64(drawer + 0x1c8) + k * 0x440;
        shadow_mask_setup(p, mask, rec + 0x700)?;
        if d == 0 {
            continue;
        }
        let f = p.vfn(d, 0x10);
        if p.m.r32(p.base + f) == RET {
            continue;
        }
        let mode = p.m.r32(drawer + 0x240) as u64;
        p.helper(
            "decal draw by the call",
            f,
            &[d as u64, w as u64, view as u64, (rec + DECAL_BLOCK_BASE) as u64, mask as u64, mode, 1],
        )?;
    }
    Ok(())
}
