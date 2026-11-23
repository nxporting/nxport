// The two functions main+0xc4a904 calls for each enabled object of its list: main+0xc573c4 (the object's own pass: its
// record of the view takes the scene state and is drawn with the object's shape into a render buffer it clears) and the
// object's virtual +0x108, main+0x220c080 (the objects' parts of a view drawn into a given render buffer: for each listed
// part its program variant, blocks, textures and the shape its record selects).
// Everything that can stop the port (programs still being set up, the render buffers' unbind and setup virtuals, the
// context type, tables the game would read out of range) is checked before the commit point.

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw};
use super::scene_passes::default_state;
use super::targets::{self, CONTEXT_TYPE_GUARD, RENDER_BUFFER_UNBIND};
use super::texture_slots::texture_assign;
use super::vfx::{program_apply, program_block, program_storage_block, program_texture};
use super::viewport;

/// [var]: the clear values (0x88 bytes: four floats per colour target, depth and stencil at +0x80 / +0x84) of main+0xc573c4.
const CLEAR_VALUES: usize = 0x4634f28;
/// [[var]]: the global with the slot table (+0x50c / +0x780 graphics settings; barrier bits).
const HOLDER: usize = 0x462bbf0;
/// [[var]]: the program set holder (+8 count, [+0x10] +0x400 the object programs); [var]: the type guard and the type
/// the object manager is cast to (virtual +0x48 of [this +0x118]).
const PROGRAM_HOLDER: usize = 0x4634e40;
const CAST_GUARD: usize = 0x4639e70;
const CAST_TYPE: usize = 0x4639e78;

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

/// A program still being set up (+0x10 / +0x14).
unsafe fn program_ready(p: &Port, prog: usize) -> R<()> {
    if prog == 0 {
        return Err("object program missing");
    }
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// A target object's view rebuild (virtual +0x18) is one of the two the bind knows.
unsafe fn view_ok(p: &Port, t: usize) -> R<()> {
    if p.m.r64(t) == 0 {
        return Err("render target setup");
    }
    match p.vfn(t, 0x18) {
        0x2a2f550 | 0x2a2f89c => Ok(()),
        _ => Err("render target setup"),
    }
}

/// What targets::bind can stop at in the buffer's targets: a lock held by another thread, an unknown view rebuild.
unsafe fn bind_ok(p: &Port, rb: usize) -> R<()> {
    for k in 0..9 {
        let t = p.m.r64(rb + 0x20 + k * 8);
        if t == 0 || p.m.i32(t + 0x60) == -1 {
            continue;
        }
        let f = p.m.r32(t + 0xc8);
        if f == 0 {
            continue;
        }
        if (f as i32) < 0 {
            return Err("render target locked");
        }
        if f & 0x7fff_ffff != 0 {
            view_ok(p, t)?;
        }
    }
    Ok(())
}

/// The context's type is known; a buffer still bound unbinds with the known virtual (binding the buffer saved at +0x100
/// again).
unsafe fn context_ok(p: &Port, w: usize) -> R<()> {
    if p.m.r8(p.m.r64(p.base + CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 {
        let prev = p.m.r64(w + 0xf8);
        if prev != 0 {
            if p.m.r64(prev) == 0 || p.vfn(prev, 0x28) != RENDER_BUFFER_UNBIND {
                return Err("render buffer unbind");
            }
            let saved = p.m.r64(w + 0x100);
            if saved != 0 {
                bind_ok(p, saved)?;
            }
        }
    }
    Ok(())
}

/// The unbind after the pass binds the buffer the bind kept at +0x100 again: the one that was bound, else the one saved
/// before.
unsafe fn restore_ok(p: &Port, w: usize) -> R<()> {
    let saved = if (p.m.r8(w + 0x108) >> 1) & 1 != 0 { p.m.r64(w + 0xf8) } else { p.m.r64(w + 0x100) };
    if saved != 0 {
        bind_ok(p, saved)?;
    }
    Ok(())
}

/// A render buffer's virtual +0x28 is the wrapper's unbind.
unsafe fn unbind_ok(p: &Port, rb: usize) -> R<()> {
    if rb == 0 || p.m.r64(rb) == 0 || p.vfn(rb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    Ok(())
}

/// main+0xc573c4(this, w, view, state): with +0x8d set, the view's record (this [+0x10] + 0x550 each, +8 count) bound as
/// the target (+0x4d0, cleared with the mask 1, or 3 with +0x8f, at its viewport +0x4a8), the render state at +0x18, the
/// scene state (`state` +8) copied to the record's texture state +0x3b8 (flags +0x49c: bits 0 and 5), the colour words
/// +0x470 (1.0, or 0.0 with +0x8e, and alpha 1.0), the program of the variant (+0x8f | +0x90 << 1, table +0xd8, 0x18
/// each) with the record as block 0 and the texture state in slot 0, the shape at +0xe8 and its stream +0x468; then the
/// buffer unbound and the barriers for the record's targets (+0x540 / +0x548, 0x220 each, id at +0x190).
pub(super) unsafe fn object_pass(p: &mut Port, this: usize, w: usize, view: u32, state: usize) -> R<()> {
    if p.m.r8(this + 0x8d) == 0 {
        return Ok(());
    }
    let records = p.m.r64(this + 0x10);
    if records == 0 || state == 0 {
        return Err("object pass record missing");
    }
    let rec = records.wrapping_add(clamp_index(view, p.m.r32(this + 8)).wrapping_mul(0x550));
    let mask: u32 = if p.m.r8(this + 0x8f) == 0 { 1 } else { 3 };
    let (rb, vp) = (rec + 0x4d0, rec + 0x4a8);
    let mut k = p.m.r8(this + 0x8f) as u32 | (p.m.r8(this + 0x90) as u32) << 1;
    if p.m.r32(this + 0xd0) <= k {
        k = 0;
    }
    let shaders = p.m.r64(this + 0xd8);
    if shaders == 0 {
        return Err("object pass program missing");
    }
    let prog = p.m.r64(shaders.wrapping_add(k as usize * 0x18) + 8);
    let values = p.m.r64(p.base + CLEAR_VALUES);
    // The checks before the commit point.
    context_ok(p, w)?;
    bind_ok(p, rb)?;
    restore_ok(p, w)?;
    program_ready(p, prog)?;
    if values == 0 {
        return Err("object pass clear values missing");
    }
    p.m.commit();
    // commit

    targets::bind_clear(p, rb, w, vp, mask, values)?;
    p.apply_graphics_context(this + 0x18, w);
    texture_assign(p, rec + 0x3b8, state + 8)?;
    let dark = p.m.r8(this + 0x8e) != 0;
    p.m.fetch_or32(rec + 0x49c, 0x20);
    let c: u32 = if dark { 0 } else { 0x3f80_0000 };
    p.m.w::<u32>(rec + 0x470, c);
    p.m.w::<u32>(rec + 0x474, c);
    p.m.w::<u32>(rec + 0x478, c);
    p.m.w::<u32>(rec + 0x47c, 0x3f80_0000);
    viewport::apply(p, vp, w, rb);
    program_apply(p, prog, w)?;
    program_block(p, prog, w, 0, rec, 0)?;
    program_texture(p, prog, w, 0, rec + 0x3b8)?;
    shape_state(p, this + 0xe8, w);
    stream_draw(p, this + 0x468, w);
    targets::unbind(p, w)?;
    // The barriers: one per target of the record with storage, then the closing one.
    let cmd = p.m.r64(w + 0xb8);
    let ents = p.m.r64(rec + 0x548);
    for j in 0..p.m.r32(rec + 0x540) as usize {
        if p.m.i32(ents + 400 + j * 0x220) != -1 {
            let bits = targets::barrier_bits(p);
            p.barrier(cmd, bits);
        }
    }
    let s = p.m.r64(p.m.r64(p.base + HOLDER));
    let bits = 0x11 | if (p.m.r8(s + 0x50c) >> 1) & 1 != 0 { 0x100 } else { 0 };
    p.barrier(cmd, bits);
    Ok(())
}

/// One listed part of main+0x220c080 resolved before anything is drawn.
struct Part {
    /// The render state kind (+0x120): 0 and 1 blend, others keep the defaults.
    mode: i32,
    /// The program variant.
    prog: usize,
    /// The part's block 0 (the view's list +0x10, 0x80 each), the view's scene texture (slot 0), the two object textures
    /// (slots 1 and 2, the second optional).
    block: usize,
    tex0: usize,
    tex1: usize,
    tex2: usize,
    /// The shape (its vertex states and buffers) and the stream drawn.
    shape: usize,
    stream: usize,
}

/// The object manager the game casts [this +0x118] to (its virtual +0x48 with the type at [0x4639e78]), pure.
unsafe fn manager(p: &Port, this: usize) -> R<usize> {
    if p.m.r8(p.m.r64(p.base + CAST_GUARD)) & 1 == 0 {
        return Err("object manager type not initialised");
    }
    let obj = p.m.r64(this + 0x118);
    if obj == 0 || p.m.r64(obj) == 0 {
        return Err("object manager missing");
    }
    let m = p.query(p.base + p.vfn(obj, 0x48), &[obj as u64, p.m.r64(p.base + CAST_TYPE) as u64]) as usize;
    if m == 0 {
        return Err("object manager missing");
    }
    Ok(m)
}

/// Entry `id` of the table [this +0x650] +0xdb8 (count +0xdb0).
unsafe fn table_entry(p: &Port, this: usize, id: u32) -> R<usize> {
    let t = p.m.r64(this + 0x650);
    if t == 0 || id >= p.m.r32(t + 0xdb0) {
        return Err("object table index");
    }
    Ok(p.m.r64(p.m.r64(t + 0xdb8) + id as usize * 8))
}

/// The parts of the list (this +0x6f8, count +0x6f0) that are drawn: a part's record is entry [part +0x10] of the manager's
/// table (+0x2d8, count +0x2d0); enabled with +0x140; its textures the part's own (+0, +8) or the ones the record's ids
/// (+0x18, +0x38) name (drawn only with +0x238 set, +0x148 into the object).
#[allow(clippy::too_many_arguments)]
unsafe fn parts(p: &Port, this: usize, view: u32, local: usize, ps: usize, mgr: usize, ctx: usize) -> R<Vec<Part>> {
    let count = p.m.i32(this + 0x6f0);
    if count < 0 {
        return Err("object part count");
    }
    let list = p.m.r64(this + 0x6f8);
    let mut out = Vec::new();
    for j in 0..count as usize {
        let item = p.m.r64(list + j * 8);
        if item == 0 {
            return Err("object part missing");
        }
        let idx = p.m.r32(item + 0x10);
        if idx >= p.m.r32(mgr + 0x2d0) {
            return Err("object record missing");
        }
        let rec = p.m.r64(p.m.r64(mgr + 0x2d8) + idx as i32 as isize as usize * 8);
        if rec == 0 {
            return Err("object record missing");
        }
        if p.m.r8(rec + 0x140) == 0 {
            continue;
        }
        let tex1 = match p.m.r64(item) {
            0 => {
                let id = p.m.i32(rec + 0x18);
                if id < 0 {
                    continue;
                }
                let o = table_entry(p, this, id as u32)?;
                if p.m.r8(o + 0x238) == 0 {
                    continue;
                }
                o + 0x148
            }
            t => t,
        };
        let tex2 = match p.m.r64(item + 8) {
            0 => {
                let id = p.m.i32(rec + 0x38);
                if id < 0 {
                    0
                } else {
                    let o = table_entry(p, this, id as u32)?;
                    if p.m.r8(o + 0x238) == 0 {
                        0
                    } else {
                        o + 0x148
                    }
                }
            }
            t => t,
        };
        let f = |o: usize| p.m.f32(rec + o);
        let plain = f(0xf8) == 1.0 && f(0xfc) == 1.0 && f(0x100) == 1.0 && f(0x104) == 1.0 && f(0x98) == 1.0;
        let mut v: u32 = if p.m.r64(this + 0xe98) != 0 { 0x60 } else { 0 };
        v = v.wrapping_add((p.m.r32(mgr + 0x328)).wrapping_shl(5));
        v |= ((p.m.r8(rec + 0x1a0) != 0) as u32) << 4;
        v |= ((p.m.r8(rec + 0x1c0) != 0) as u32) << 3;
        v |= ((p.m.r8(rec + 0x1e0) != 0) as u32) << 2;
        v |= if plain { 0 } else { 2 };
        v |= (tex2 != 0) as u32;
        if ps == 0 {
            return Err("object program set missing");
        }
        let z = p.m.r64(ps + 8);
        let prog = if (v as i32) <= 0 {
            p.m.r64(z + 0x10)
        } else {
            let i = v - 1;
            let i = if p.m.r32(z + 0x18) > i { i } else { 0 };
            p.m.r64(z + 0x20).wrapping_add(i as usize * 0x158)
        };
        if local == 0 || ctx == 0 || p.m.r64(ctx + 0x10) == 0 {
            return Err("object part blocks missing");
        }
        let b = if p.m.r32(local) > idx { idx } else { 0 } as i32 as isize as usize;
        let block = p.m.r64(local + 8).wrapping_add(b.wrapping_mul(0x80));
        let sv = clamp_index(view, p.m.r32(ctx + 8));
        let tex0 = p.m.r64(p.m.r64(ctx + 0x10).wrapping_add(sv.wrapping_mul(0x550)) + 0x548) + 0x40;
        // The shape: the record's flag (+0x300) and the object's flag (+0x2c8) pick one of four.
        let id = p.m.i32(rec + 0x18);
        let big = id >= 0 && p.m.r8(table_entry(p, this, id as u32)? + 0x2c8) != 0;
        let t = p.m.r64(this + 0x650);
        let (shape, stream) = match (p.m.r8(rec + 0x300) != 0, big) {
            (false, false) => (this + 0x718, t + 0x2248),
            (false, true) => (this + 0x8f8, t + 0x2448),
            (true, true) => (this + 0xcb8, t + 0x2848),
            (true, false) => (this + 0xad8, t + 0x2648),
        };
        out.push(Part { mode: p.m.i32(rec + 0x120), prog, block, tex0, tex1, tex2, shape, stream });
    }
    Ok(out)
}

/// main+0x220c080(this, w, view, rb, vp, _, ctx): the render buffer `rb` bound with its viewport `vp`, then each listed
/// part (see `parts`) drawn: the render state (changed when the part's kind +0x120 differs from the last one's: no depth,
/// target 0 blended, kinds 0 and 1 with their factors), the program variant, its block 0, the textures of slots 0 (the
/// view's scene texture: ctx [+0x10] + 0x550 each, +0x548 +0x40), 1 and 2, the optional storage block (this +0xe98) and
/// the shape; then `rb` unbound (its virtual +0x28).
pub(super) unsafe fn object_draw(p: &mut Port, this: usize, w: usize, view: u32, rb: usize, vp: usize, ctx: usize) -> R<()> {
    let local = if view < p.m.r32(this + 0x6e0) { p.m.r64(p.m.r64(this + 0x6e8) + view as i32 as isize as usize * 8) } else { 0 };
    let h = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    let ps = if p.m.r32(h + 8) < 0x81 { 0 } else { p.m.r64(p.m.r64(h + 0x10) + 0x400) };
    // The checks before the commit point.
    context_ok(p, w)?;
    unbind_ok(p, rb)?;
    bind_ok(p, rb)?;
    restore_ok(p, w)?;
    let list = if p.m.i32(this + 0x6f0) != 0 {
        let mgr = manager(p, this)?;
        parts(p, this, view, local, ps, mgr, ctx)?
    } else {
        Vec::new()
    };
    for part in &list {
        program_ready(p, part.prog)?;
    }
    p.m.commit();
    // commit

    targets::bind(p, rb, w)?;
    viewport::apply(p, vp, w, rb);
    let mut mode = 2i32;
    for part in &list {
        if part.mode != mode {
            let mut g = default_state(p);
            let b = g.as_mut_ptr() as *mut u8;
            g[0] &= 0xffff_0000;
            g[1] |= 1;
            *b.add(0x2c) = 1;
            *b.add(0x2d) = 1;
            if part.mode == 0 || part.mode == 1 {
                let c = if part.mode == 0 { 2 } else { 6 };
                *b.add(0x28) = 5;
                *b.add(0x29) = 5;
                *b.add(0x2a) = c;
                *b.add(0x2b) = c;
            }
            p.apply_graphics_context(g.as_ptr() as usize, w);
            mode = part.mode;
        }
        program_apply(p, part.prog, w)?;
        program_block(p, part.prog, w, 0, part.block, 0)?;
        program_texture(p, part.prog, w, 0, part.tex0)?;
        program_texture(p, part.prog, w, 1, part.tex1)?;
        if part.tex2 != 0 {
            program_texture(p, part.prog, w, 2, part.tex2)?;
        }
        let storage = p.m.r64(this + 0xe98);
        if storage != 0 {
            program_storage_block(p, part.prog, w, 0, storage, 0)?;
        }
        shape_state(p, part.shape, w);
        stream_draw(p, part.stream, w);
    }
    targets::unbind(p, w)
}
