// TotK's deferred light volumes, main+0x9777c4(obj, w, view, arg), called twice by the lighting pass main+0x973b40
// (stencil marking, then shading, each with its own graphics context). Every light is a 0x30-byte record (its volume's
// transform) put into a ring block, bound at the volume program's slots and drawn as one of three volume shapes.

use super::draw::{Port, R};
use super::vfx::{program_apply, program_block, shape_draw};

/// Pointer to the volume shapes (vertex states and buffers, 0x5a8 apart from +8) and to their index streams.
pub(super) const SHAPES: usize = 0x46323a0;
pub(super) const STREAMS: usize = 0x46323a8;

pub(super) unsafe fn draw(p: &mut Port, obj: usize, w: usize, view: u32, arg: usize) -> R<()> {
    let v = if p.m.r32(obj + 0x38) <= view { 0 } else { view } as usize;
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let prog = p.m.r64(p.m.r64(p.m.r64(obj + 0x68) + 8) + 0x10);
    program_apply(p, prog, w)?;
    let scene = p.m.r64(p.m.r64(arg + 8) + 0x18);
    p.m.w::<u8>(scene + 0x421, p.m.r8(scene + 0x421) | 1);
    program_block(p, prog, w, 2, scene + 0x390, 0)?;
    let rec = p.m.r64(obj + 0x40) + v * 0x440;
    // Single lights: the shape by the record's kind (+0x30).
    let list = p.m.r64(rec + 0x428);
    for k in 0..p.m.r32(rec + 0x420) as usize {
        let r = p.m.r64(list + k * 8);
        light(p, prog, w, r)?;
        let (shape, stream) = match p.m.r32(r + 0x30) {
            0 => (shapes + 0x968, streams + 0x1648),
            1 => (shapes + 8, streams + 0x728),
            _ => (shapes + 0x5a8, streams + 0x22c8),
        };
        shape_draw(p, shape, stream, w, 1);
    }
    // Groups seen by this view (+0x68): five record lists (count, records), the first two binding their shape per
    // light, the others once per list.
    let groups = p.m.r64(rec + 0x438);
    for k in 0..p.m.r32(rec + 0x430) as usize {
        let g = p.m.r64(groups + k * 8);
        if (p.m.r32(g + 0x68) >> (view & 31)) & 1 == 0 {
            continue;
        }
        for o in [0usize, 0x10] {
            let records = p.m.r64(g + o + 8);
            for j in 0..p.m.r32(g + o) as usize {
                // The first list's ring blocks come from an inlined copy of the allocator.
                let r = records + j * 0x30;
                let block = if o == 0 { p.ring_block_inline(r, 0x30) } else { p.ring_block(r, 0x30) };
                program_block(p, prog, w, 1, block, 0)?;
                shape_draw(p, shapes + 8, streams + 0x728, w, 1);
            }
        }
        for (o, shape, stream) in [(0x20usize, 8usize, 0x728usize), (0x30, 0x968, 0x1648), (0x40, 0x5a8, 0x22c8)] {
            shape_state(p, shapes + shape, w);
            let records = p.m.r64(g + o + 8);
            for j in 0..p.m.r32(g + o) as usize {
                light(p, prog, w, records + j * 0x30)?;
                stream_draw(p, streams + stream, w);
            }
        }
    }
    Ok(())
}

/// The light's record in a ring block (main+0x96b068), bound at the program's slots of index 1 (main+0x9771a8).
unsafe fn light(p: &mut Port, prog: usize, w: usize, record: usize) -> R<()> {
    let block = p.ring_block(record, 0x30);
    program_block(p, prog, w, 1, block, 0)?;
    Ok(())
}

/// The shape's vertex states and buffers (main+0xf216b4 with the states before it).
pub(super) unsafe fn shape_state(p: &mut Port, shape: usize, w: usize) {
    let cmd = p.m.r64(w + 0xb8);
    p.bind_vertex_attribs(cmd, 0x10, shape + 0x11c);
    p.bind_vertex_streams(cmd, 0x10, shape + 0x160);
    let bufs = p.m.r64(shape + 0x110);
    for i in 0..p.m.r32(shape + 0x108) as usize {
        let b = p.m.r64(bufs + i * 8);
        if b == 0 {
            continue;
        }
        let addr = p.buffer_address_of(b + 8);
        p.m.w::<u8>(cmd + 0x10, 1);
        p.bind_vertex_buffer(cmd, i as i32, addr, p.m.r32(b + 0x118) as usize);
    }
}

/// main+0x966e70(stream, w, 1): the index stream's draw (prim +0xc, type +8, count +0x28, buffer +0x30).
pub(super) unsafe fn stream_draw(p: &mut Port, stream: usize, w: usize) {
    let count = p.m.i32(stream + 0x28);
    if count == 0 {
        return;
    }
    let cmd = p.m.r64(w + 0xb8);
    let addr = p.buffer_address_of(stream + 0x30);
    p.m.w::<u8>(cmd + 0x10, 1);
    p.draw_elements(cmd, p.m.r32(stream + 0xc) as i32, p.m.r32(stream + 8) as i32, count, addr);
}
