// TotK's deferred light volumes, main+0x9777c4(obj, w, view, arg), called twice by the lighting pass main+0x973b40
// (stencil marking, then shading, each with its own graphics context). Every light is a 0x30-byte record (its volume's
// transform) put into a ring block, bound at the volume program's slots and drawn as one of three volume shapes.

use super::draw::{Port, R};
use super::vfx::{program_apply, program_block, program_storage_block, program_texture, shape_draw};

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
    shape_buffers(p, shape, w);
}

/// main+0xf216b4(shape, w): the shape's vertex buffers (+0x108 count, +0x110 buffers: address, size +0x118).
pub(super) unsafe fn shape_buffers(p: &mut Port, shape: usize, w: usize) {
    let cmd = p.m.r64(w + 0xb8);
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
    stream_draw_n(p, stream, w, 1);
}

/// main+0x966e70(stream, w, instances): instanced from two instances on (without the command buffer's +0x10 flag).
pub(super) unsafe fn stream_draw_n(p: &mut Port, stream: usize, w: usize, instances: u32) {
    let count = p.m.i32(stream + 0x28);
    if count == 0 {
        return;
    }
    let cmd = p.m.r64(w + 0xb8);
    let addr = p.buffer_address_of(stream + 0x30);
    let (mode, ty) = (p.m.r32(stream + 0xc) as i32, p.m.r32(stream + 8) as i32);
    if instances < 2 {
        p.m.w::<u8>(cmd + 0x10, 1);
        p.draw_elements(cmd, mode, ty, count, addr);
    } else {
        p.draw_elements_instanced(cmd, mode, ty, count, addr, 0, 0, instances as i32);
    }
}

/// main+0x96a918 / main+0x96aa20 / main+0x96a9c8: a light entry's id, intensity and second value (inherited from its
/// parent unless set); main+0x96a970: the light's entry tree.
const ENTRY_ID: usize = 0x96a918;
const ENTRY_SCALE: usize = 0x96aa20;
const ENTRY_VALUE: usize = 0x96a9c8;
const ENTRY_TREE: usize = 0x96a970;
/// main+0x96af6c / main+0x96afc0 / main+0x96b014: further inherited float values of a light entry (+0x34, +0x3c, +0x40
/// unless set on the entry: bits 0 of +0x4e / +0x4f / +0x52), pointers to the float.
const ENTRY_F34: usize = 0x96af6c;
const ENTRY_F3C: usize = 0x96afc0;
const ENTRY_F40: usize = 0x96b014;
/// The pure 3x4 inverse (out, in) and product (out, a, b).
const INVERSE_3X4: usize = 0x93a4c4;
const MATRIX_MUL_3X4: usize = 0x9f3524;
/// The type light entries' parents are cast to, and its guard.
const ENTRY_TYPE_GUARD: usize = 0x4633c58;
const ENTRY_TYPE: usize = 0x4633c60;
/// The id remap table (count, values) and its once-initialised guard.
const ID_TABLE_GUARD: usize = 0x4633c28;
const ID_TABLE: usize = 0x4633c30;
/// The type the shape object is cast to for its radius (+0xa4), and its guard.
const SHAPE_TYPE_GUARD: usize = 0x462b978;
const SHAPE_TYPE: usize = 0x462b980;
/// The radius scale (main+0x380f978).
const RADIUS_SCALE: usize = 0x380f978;

/// main+0x969d28(ctx, light, view): one light drawn with its own program (main+0x96aa74). The light's entry is the first
/// in its entry tree (deepest level first) whose remapped id equals the light's (+0x58); its record holds the light's
/// 0x30 bytes, its position through the view transform (ctx[4] +0x108) and radius, and the entry's values. Shape kinds
/// 0, 3 and 4 (the others draw nothing).
pub(super) unsafe fn light_shape(p: &mut Port, ctx: usize, light: usize, view: u32) -> R<()> {
    let obj = p.m.r64(light + 0x50);
    if (p.m.r64(obj + 0x68) >> 10) & 1 == 0 {
        return Ok(());
    }
    let shape = p.m.r64(obj + 0x28);
    let owner = p.m.r64(ctx + 8);
    let id = p.m.r32(light + 0x58);
    let variant = ((id != 0) as u32 * 16 + view * 4).wrapping_add(p.m.r8(p.m.r64(ctx)) as u32 * 8) as i32;
    let tree = p.query(p.base + ENTRY_TREE, &[p.m.r64(owner + 0x130) as u64]) as usize;
    let entry = find_entry(p, tree, id)?;
    let mut scale = if id == 1 { p.m.f32(owner + 0x13c) } else { 1.0 };
    match p.query(p.base + p.vfn(shape, 0x10), &[shape as u64]) as u32 {
        0 => {}
        kind @ (3 | 4) => return light_volume(p, ctx, light, id, variant, kind, entry, scale),
        _ => return Ok(()),
    }
    let progs = p.m.r64(p.m.r64(owner + 0x20) + 8);
    let prog = if variant <= 0 {
        p.m.r64(progs + 0x10)
    } else {
        let v = (variant - 1) as u32;
        p.m.r64(progs + 0x20) + if p.m.r32(progs + 0x18) > v { v } else { 0 } as usize * 0x158
    };
    light_program(p, p.m.r64(ctx + 0x18), prog)?;
    let (x, y, z) = (p.m.f32(light + 0xc), p.m.f32(light + 0x1c), p.m.f32(light + 0x2c));
    if p.m.r8(p.m.r64(p.base + SHAPE_TYPE_GUARD)) & 1 == 0 {
        return Err("light shape type not initialised");
    }
    let cast = p.query(p.base + p.vfn(shape, 0), &[shape as u64, p.m.r64(p.base + SHAPE_TYPE) as u64]) as usize;
    let m = p.m.r64(ctx + 0x20);
    let mf: [f32; 12] = std::array::from_fn(|k| p.m.f32(m + 0x108 + k * 4));
    let f = |o: usize| mf[(o - 0x108) / 4];
    let mut rec = [0u8; 0x50];
    for (k, b) in rec[..0x30].iter_mut().enumerate() {
        *b = p.m.r8(light + k);
    }
    // As the game: products, then the sums in its order.
    let r0 = f(0x114) + ((x * f(0x108) + y * f(0x10c)) + z * f(0x110));
    let r1 = f(0x124) + ((x * f(0x118) + y * f(0x11c)) + z * f(0x120));
    let r2 = f(0x134) + ((x * f(0x128) + y * f(0x12c)) + z * f(0x130));
    let radius = p.m.f32(cast + 0xa4) * p.m.f32(p.base + RADIUS_SCALE);
    let (value, s) = if entry == 0 {
        (0u32, 0.0)
    } else {
        scale *= p.m.f32(p.query(p.base + ENTRY_SCALE, &[entry as u64]) as usize);
        (p.m.r32(p.query(p.base + ENTRY_VALUE, &[entry as u64]) as usize), scale)
    };
    for (k, v) in [r0.to_bits(), r1.to_bits(), r2.to_bits(), radius.to_bits(), s.to_bits(), value, p.m.r32(light + 0x60), 0]
        .into_iter()
        .enumerate()
    {
        rec[0x30 + k * 4..0x34 + k * 4].copy_from_slice(&v.to_le_bytes());
    }
    let w = p.m.r64(p.m.r64(ctx + 0x10));
    let block = p.ring_block(rec.as_ptr() as usize, 0x50);
    program_block(p, prog, w, 1, block, 0)?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x968, w);
    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x1768, w);
    Ok(())
}

/// Light shape kinds 3 and 4 of main+0x969d28: the program of variant `variant | 1` (kind 3) or `variant | 2` (kind 4),
/// the light's 0x30 bytes, the light's inverse times the view's inverse (3x4, main+0x9f3524), the entry's values, in a
/// 0x70 / 0x80-byte ring block at slot 1; kind 3 draws shapes +8 / stream +0x728, kind 4 shapes +0x5a8 / stream +0x2328.
#[allow(clippy::too_many_arguments)]
unsafe fn light_volume(p: &mut Port, ctx: usize, light: usize, id: u32, variant: i32, kind: u32, entry: usize, scale: f32) -> R<()> {
    let owner = p.m.r64(ctx + 8);
    let progs = p.m.r64(p.m.r64(owner + 0x20) + 8);
    let v = variant | (kind - 2) as i32;
    let prog = if v < 1 {
        p.m.r64(progs + 0x10)
    } else {
        let i = (v - 1) as u32;
        p.m.r64(progs + 0x20) + if p.m.r32(progs + 0x18) > i { i } else { 0 } as usize * 0x158
    };
    light_program(p, p.m.r64(ctx + 0x18), prog)?;
    // Host copies for the pure matrix functions.
    let m = p.m.r64(ctx + 0x20);
    let view: [f32; 12] = std::array::from_fn(|k| p.m.f32(m + 0x108 + k * 4));
    let own: [f32; 12] = std::array::from_fn(|k| p.m.f32(light + k * 4));
    let (mut view_inv, mut own_inv, mut prod) = ([0f32; 12], [0f32; 12], [0f32; 12]);
    p.query(p.base + INVERSE_3X4, &[view_inv.as_mut_ptr() as u64, view.as_ptr() as u64]);
    p.query(p.base + INVERSE_3X4, &[own_inv.as_mut_ptr() as u64, own.as_ptr() as u64]);
    p.query(p.base + MATRIX_MUL_3X4, &[prod.as_mut_ptr() as u64, own_inv.as_ptr() as u64, view_inv.as_ptr() as u64]);
    let mut rec = [0u8; 0x80];
    for (k, b) in rec[..0x30].iter_mut().enumerate() {
        *b = p.m.r8(light + k);
    }
    for (k, f) in prod.iter().enumerate() {
        rec[0x30 + k * 4..0x34 + k * 4].copy_from_slice(&f.to_bits().to_le_bytes());
    }
    let put = |rec: &mut [u8; 0x80], off: usize, v: u32| rec[off..off + 4].copy_from_slice(&v.to_le_bytes());
    let tail = p.m.r32(light + 0x60);
    let size;
    if kind == 3 {
        let (a, b) = if entry == 0 {
            (0, 0)
        } else {
            (p.m.r32(p.query(p.base + ENTRY_SCALE, &[entry as u64]) as usize), p.m.r32(p.query(p.base + ENTRY_VALUE, &[entry as u64]) as usize))
        };
        put(&mut rec, 0x60, a);
        put(&mut rec, 0x64, b);
        put(&mut rec, 0x68, tail);
        size = 0x70;
    } else {
        let one = 1f32.to_bits();
        if entry == 0 {
            put(&mut rec, 0x6c, tail);
            put(&mut rec, 0x70, one);
            put(&mut rec, 0x74, one);
        } else if id & !3 != 8 {
            let s = p.m.f32(p.query(p.base + ENTRY_SCALE, &[entry as u64]) as usize);
            put(&mut rec, 0x60, (scale * s).to_bits());
            put(&mut rec, 0x64, (scale * s).to_bits());
            put(&mut rec, 0x68, p.m.r32(p.query(p.base + ENTRY_VALUE, &[entry as u64]) as usize));
            put(&mut rec, 0x6c, tail);
            put(&mut rec, 0x70, one);
            put(&mut rec, 0x74, one);
        } else {
            let a = p.m.f32(p.query(p.base + ENTRY_F34, &[entry as u64]) as usize);
            let b = p.m.f32(p.query(p.base + ENTRY_F3C, &[entry as u64]) as usize);
            put(&mut rec, 0x60, (a + -1.0).to_bits());
            put(&mut rec, 0x64, (b + -1.0).to_bits());
            put(&mut rec, 0x68, p.m.r32(p.query(p.base + ENTRY_VALUE, &[entry as u64]) as usize));
            put(&mut rec, 0x6c, tail);
            // The first ancestor entry (or the entry) with +0x51 bit 0 set gives +0x38.
            let mut e = entry;
            if p.m.r8(e + 0x51) & 1 == 0 {
                if p.m.r8(p.m.r64(p.base + ENTRY_TYPE_GUARD)) & 1 == 0 {
                    return Err("light entry type not initialised");
                }
                let ty = p.m.r64(p.base + ENTRY_TYPE);
                while p.m.r64(e + 0x10) != 0 {
                    let h = p.m.r64(e + 0x18);
                    if h == 0 || p.m.r32(h + 0xc) != p.m.r32(e + 0x20) {
                        break;
                    }
                    let parent = p.m.r64(h);
                    if parent == 0 {
                        break;
                    }
                    let up = p.query(p.base + p.vfn(parent, 0), &[parent as u64, ty as u64]) as usize;
                    if up == 0 {
                        break;
                    }
                    e = up;
                    if p.m.r8(e + 0x51) & 1 != 0 {
                        break;
                    }
                }
            }
            put(&mut rec, 0x70, p.m.r32(e + 0x38));
            put(&mut rec, 0x74, p.m.r32(p.query(p.base + ENTRY_F40, &[entry as u64]) as usize));
        }
        size = 0x80;
    }
    let w = p.m.r64(p.m.r64(ctx + 0x10));
    let block = p.ring_block(rec.as_ptr() as usize, size);
    program_block(p, prog, w, 1, block, 0)?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    if kind == 3 {
        shape_state(p, shapes + 8, w);
        stream_draw(p, streams + 0x728, w);
    } else {
        shape_state(p, shapes + 0x5a8, w);
        stream_draw(p, streams + 0x2328, w);
    }
    Ok(())
}

/// The first entry, from the tree's deepest level up, whose remapped id equals the remapped `id` (0: none).
unsafe fn find_entry(p: &mut Port, root: usize, id: u32) -> R<usize> {
    if p.m.r8(p.m.r64(p.base + ID_TABLE_GUARD)) & 1 == 0 {
        return Err("light id table not initialised");
    }
    let table = p.m.r64(p.base + ID_TABLE);
    let remap = |p: &Port, i: u32| p.m.i32(p.m.r64(table + 8) + if p.m.r32(table) > i { i } else { 0 } as i32 as isize as usize * 4);
    let mut levels = 0;
    let mut n = root;
    loop {
        n = p.m.r64(n + 0x18);
        levels += 1;
        if n == 0 {
            break;
        }
    }
    for level in (0..levels).rev() {
        let mut node = root;
        for _ in 0..level {
            node = p.m.r64(node + 0x18);
            if node == 0 {
                return Ok(0);
            }
        }
        let mut i = 0u32;
        while i < p.m.r32(node + 8) {
            let e = p.m.r64(p.m.r64(node + 0x10) + i as i32 as isize as usize * 8);
            let eid = p.m.r32(p.query(p.base + ENTRY_ID, &[e as u64]) as usize);
            if remap(p, id) == remap(p, eid) {
                return Ok(e);
            }
            i += 1;
        }
    }
    Ok(0)
}

/// main+0x96aa74(ctx, prog): the light program with its textures (ctx[1], [ctx[2]], three shadow cascades from ctx[3]),
/// the view's uniforms (ctx[4] +0xc0, 0x1a0 each, index [ctx[5]]), the scene uniforms and the storage block ctx[7] +0x1c0.
pub(super) unsafe fn light_program(p: &mut Port, c: usize, prog: usize) -> R<()> {
    let r: [usize; 8] = std::array::from_fn(|k| p.m.r64(c + k * 8));
    let w = p.m.r64(r[0]);
    program_apply(p, prog, w)?;
    program_texture(p, prog, w, 0, r[1])?;
    program_texture(p, prog, w, 1, p.m.r64(r[2]))?;
    let sh = r[3];
    let (n, arr) = (p.m.r32(sh + 8), p.m.r64(sh + 0x10));
    program_texture(p, prog, w, 4, arr + if n > 1 { 0x1c0 } else { 0 } + 0xb8)?;
    program_texture(p, prog, w, 5, arr + if n > 2 { 0x380 } else { 0 } + 0xb8)?;
    program_texture(p, prog, w, 6, arr + if n > 3 { 0x540 } else { 0 } + 0xb8)?;
    let v = r[4];
    let i = p.m.r32(r[5]);
    let i = if p.m.r32(v + 0xb8) > i { i } else { 0 } as i32 as isize as usize;
    program_block(p, prog, w, 0, p.m.r64(v + 0xc0).wrapping_add(i.wrapping_mul(0x1a0)), 0)?;
    let scene = p.m.r64(r[6] + 0x18);
    p.m.w::<u8>(scene + 0x421, p.m.r8(scene + 0x421) | 1);
    program_block(p, prog, w, 2, p.m.r64(r[6] + 0x18) + 0x390, 0)?;
    program_storage_block(p, prog, w, 0, r[7] + 0x1c0, 0)?;
    Ok(())
}
