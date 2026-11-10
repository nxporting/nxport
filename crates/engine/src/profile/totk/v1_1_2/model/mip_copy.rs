// TotK's mip chain copy of a render buffer texture, main+0xbd99e0(this, w): the texture selected from the list (+8) is
// copied by full-screen quads into every mip level (except the last) and face of the buffer object at this+0x10 (render
// buffer +0x368 over the colour state +0xf0, set up by main+0x7ee01c), then, with the first quad's program, into the last
// level of the same texture from this+0x268. main+0xbda288 / 0xbda640 / 0xbda574 (its per-face setup, resolve and
// finish) are inlined: this call's prelude zeroes the textures they would allocate or free (+0x1d0 / +0x288), so only
// their steady paths run. The game's stack objects (colour target copy, render buffer) are host objects.

use super::cubemap::copy_quad;
use super::draw::{Port, R};
use super::scene_passes::default_state;
use super::targets;
use super::texture_slots::{barrier_if_slot, copy_target, level_size, new_buffer, new_target, release, set_extent, Host, BUFFER_BYTES, COLOR_VTABLE, TARGET_BYTES};
use super::viewport;

/// [var]: the 0x88-byte default clear values (eight colours, depth / stencil word).
const CLEAR_VALUES: usize = 0x4634f28;
/// [var]: the 4-vector table the copy quads' rows come from.
const QUAD_ROWS: usize = 0x462bc10;
/// [[[[[holder]]+0x10] +0x148 / 0x150] +8] +0x20: the copy program (see cubemap::copy_quad).
const PROGRAM_HOLDER: usize = 0x4634e40;

/// The host objects of one run, given back at its end.
struct Objs(Vec<Host>);

impl Objs {
    fn keep(&mut self, h: Host) -> usize {
        let a = h.addr();
        self.0.push(h);
        a
    }
}

/// main+0x7ee01c(this, w, color, level, layer, depth, extra, flags, clear): the render buffer at this+0x368 set up with
/// a colour target object (this+0x3d8: the state `color`, layer, level) and, with `depth`, a second target object
/// (this+0x4c8: the state `depth`, layer / level 0; `extra` = 4 words copied to this+0x5a0), then bound and (with
/// `clear`, a pointer to one colour of 4 floats) target 0 cleared in a viewport of the target's size (color +0x30 >>
/// level, at least 1). `clear` is the ninth argument, on the stack. With flags bit 0 a barrier after each target that
/// has a texture slot.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn buffer_setup(
    p: &mut Port,
    this: usize,
    w: usize,
    color: usize,
    level: u32,
    layer: u32,
    depth: usize,
    extra: usize,
    flags: u32,
    clear: usize,
) -> R<()> {
    let mut size = (p.m.r16(color + 0x30) as u32 >> (level & 0x1f)) as u16;
    if size < 2 {
        size = 1;
    }
    let f = size as f32;
    // The buffer's target pointers (this+0x388 .. 0x3d0) cleared.
    for o in (0..0x48).step_by(8) {
        p.m.w::<u64>(this + 0x388 + o, 0);
    }
    // Colour object: state + slot reference, the setup flags.
    let obj = this + 0x3d8;
    set_target(p, obj, color, layer, level)?;
    p.m.w::<u64>(this + 0x388, obj as u64);
    if flags & 1 != 0 {
        barrier_if_slot(p, this + 0x438, w);
    }
    if depth != 0 {
        let obj = this + 0x4c8;
        set_target(p, obj, depth, 0, 0)?;
        // extra: (int, int, long, long) -> this+0x5a0.. when any of the first three differ.
        let (a, b, c, d) = (p.m.r32(extra), p.m.r32(extra + 4), p.m.r64(extra + 8), p.m.r64(extra + 0x10));
        if c != p.m.r64(this + 0x5a8) || a != p.m.r32(this + 0x5a0) || b != p.m.r32(this + 0x5a4) {
            p.m.w::<u32>(this + 0x5a0, a);
            p.m.w::<u32>(this + 0x5a4, b);
            p.m.w::<u64>(this + 0x5a8, c as u64);
            p.m.w::<u64>(this + 0x5b0, d as u64);
            p.m.fetch_or32(this + 0x590, 8);
        }
        p.m.w::<u64>(this + 0x3c8, obj as u64);
        if flags & 1 != 0 {
            barrier_if_slot(p, this + 0x528, w);
        }
    }
    // The buffer's size (0x370 / 0x374) and region (0x378 .. 0x384).
    p.m.w::<u64>(this + 0x378, 0);
    p.m.w::<f32>(this + 0x380, f);
    p.m.w::<f32>(this + 0x384, f);
    p.m.w::<f32>(this + 0x370, f);
    p.m.w::<f32>(this + 0x374, f);
    // Clear values: the default ones, or the given colour for all eight targets and depth 1.
    let mut values = [0u64; 17];
    if clear == 0 {
        let src = p.pfn(CLEAR_VALUES);
        for (k, v) in values.iter_mut().enumerate() {
            *v = p.m.r64(src + k * 8) as u64;
        }
    } else {
        let (lo, hi) = (p.m.r64(clear) as u64, p.m.r64(clear + 8) as u64);
        for k in 0..8 {
            values[k * 2] = lo;
            values[k * 2 + 1] = hi;
        }
        values[16] = 0x3f80_0000;
    }
    let vp = viewport::full(p, f, f);
    targets::bind_clear(p, this + 0x368, w, vp.as_ptr() as usize, (clear != 0) as u32, values.as_ptr() as usize)
}

/// Target object `obj` given state `src`, with the given layer / level: the copy, flag 4, flag 2 / 1 and the stored
/// value when they differ from the object's, the object's virtual +0x10 (copy_target does the copy, flag 4 and the call).
unsafe fn set_target(p: &mut Port, obj: usize, src: usize, layer: u32, level: u32) -> R<()> {
    if p.m.r16(obj + 0xcc) as u32 != layer {
        p.m.fetch_or32(obj + 0xc8, 2);
        p.m.w::<u16>(obj + 0xcc, layer as u16);
    }
    if p.m.r8(obj + 0xce) as u32 != level {
        p.m.fetch_or32(obj + 0xc8, 1);
        p.m.w::<u8>(obj + 0xce, level as u8);
    }
    copy_target(p, obj, src)
}

/// A viewport of the orientation's vtable from (0, 0) to (f, f) as the game builds it on its stack (no orientation
/// swap, no unset marker).
unsafe fn square_viewport(p: &Port, f: f32) -> [u32; 10] {
    let mut vp = viewport::full(p, f, f);
    vp[2] = 0f32.to_bits();
    vp[3] = 0f32.to_bits();
    vp[4] = f.to_bits();
    vp[5] = f.to_bits();
    vp
}

/// main+0xbda288(this, w, 0, face, level): with +0x5b8 bit 5, the buffer set up over the colour state at +0xf0 for
/// `level` / `face` (layer = face) and its size remembered (+0x358 .. 0x364); true when it ran. The paths that
/// allocate the buffer's textures (+0x1d0 / +0x288 set) never occur here: the caller cleared them.
unsafe fn face_setup(p: &mut Port, this: usize, w: usize, face: u32, level: u32) -> R<bool> {
    let flags = p.m.r8(this + 0x5b8);
    if flags & 0x20 == 0 {
        return Ok(false);
    }
    if p.m.r64(this + 0x288) != 0 || p.m.r64(this + 0x1d0) != 0 {
        return Err("render buffer textures allocated by the call");
    }
    let size = ((p.m.r16(this + 0x120) as u32) >> (level & 0x1f)).max(1) * (1 + (flags & 0x40 != 0) as u32);
    p.m.w::<u32>(this + 0x358, 0);
    p.m.w::<u32>(this + 0x35c, face);
    p.m.w::<u32>(this + 0x360, level);
    p.m.w::<u32>(this + 0x364, size);
    let extra = [0u64; 3];
    buffer_setup(p, this, w, this + 0xf0, level, face, 0, extra.as_ptr() as usize, 1, 0)?;
    p.m.w::<u8>(this + 0x5b8, p.m.r8(this + 0x5b8) | 0x10);
    Ok(true)
}

/// main+0xbda640(this, w, 0): with +0x5b8 bit 5, the wrapper's buffer unbound and the remembered face / level reset.
unsafe fn face_done(p: &mut Port, this: usize, w: usize) -> R<()> {
    if p.m.r8(this + 0x5b8) & 0x20 == 0 {
        return Ok(());
    }
    targets::unbind(p, w)?;
    if p.m.r64(this + 0x1d0) != 0 {
        return Err("render buffer textures resolved by the call");
    }
    p.m.w::<u64>(this + 0x358, u64::MAX);
    p.m.w::<u32>(this + 0x360, u32::MAX);
    Ok(())
}

/// main+0xbda574(this, w): with +0x5b8 bit 3, a barrier after the colour target (when it has a slot), the textures
/// +0x1d0 / +0x288 given back (none here) and the setup state cleared.
unsafe fn buffers_done(p: &mut Port, this: usize, w: usize) -> R<()> {
    if p.m.r8(this + 0x5b8) & 8 == 0 {
        return Ok(());
    }
    barrier_if_slot(p, this + 0x438, w);
    if p.m.r64(this + 0x1d0) != 0 || p.m.r64(this + 0x288) != 0 {
        return Err("render buffer textures freed by the call");
    }
    p.m.w::<u64>(this + 0x1d0, 0);
    p.m.w::<u64>(this + 0x288, 0);
    p.m.w::<u8>(this + 0x5b8, p.m.r8(this + 0x5b8) & 0x87);
    p.m.w::<u32>(this + 0x290, 0);
    p.m.w::<u64>(this + 0x298, 0);
    p.m.w::<u64>(this + 0x2a0, 0);
    Ok(())
}

/// The copy quad's program (cubemap::copy_quad with a value other than -1) and `tex` must be ready, else copy_quad
/// would stop or call the game inside the passes.
unsafe fn quad_ready(p: &Port, tex: usize) -> R<()> {
    let off = if p.m.r16(tex + 0x3a) != 9 { 0x148 } else { 0x150 };
    let h = p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER)) + 0x10) + off);
    let prog = p.m.r64(p.m.r64(h + 8) + 0x20);
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    if p.m.r32(tex + 0xe4) != 0 {
        return Err("agl texture still finishing");
    }
    Ok(())
}

/// main+0xbd99e0(this, w): the texture of the list entry (+0x46b) copied into the buffer object's mip levels and faces,
/// then this+0x268 into its last level; see the file comment. Everything the port stops at is in the undoable part: the
/// commit comes with the last step (the temporary state's slot reference given back, which locks the slot table).
pub(super) unsafe fn mip_copy_pass(p: &mut Port, this: usize, w: usize) -> R<()> {
    let flags = p.m.r32(this + 0x470);
    if flags & 1 == 0 {
        return Ok(());
    }
    let list = p.m.r64(this + 8);
    let count = p.m.r32(list + 0x210);
    if count < 2 || flags & 0x80 != 0 {
        return Ok(());
    }
    let idx = p.m.r8(this + 0x46b) as u32;
    if idx >= count {
        return Err("list node missing");
    }
    let mut node = p.m.r64(list + 0x208);
    for _ in 0..idx {
        if node == 0 {
            return Err("list node missing");
        }
        node = p.m.r64(node + 8);
    }
    if node == 0 {
        return Err("list node missing");
    }
    let src = p.m.r64((node as isize - p.m.i32(list + 0x214) as isize) as usize + 0x10);
    let this2 = p.m.r64(this + 0x10);
    if p.m.r8(this2 + 0x39) == 0 {
        return Err("render buffer without levels");
    }
    quad_ready(p, src)?;
    quad_ready(p, this + 0x268)?;

    let mut o = Objs(Vec::new());
    let r = run(p, this, w, this2, src, &mut o);
    for h in o.0 {
        h.release(p);
    }
    r
}

unsafe fn run(p: &mut Port, this: usize, w: usize, this2: usize, src: usize, o: &mut Objs) -> R<()> {
    // The render state: blending (colour target 0 only, no depth) for the first call with a fade value of at least 1
    // (six faces, the value stepped down), else the mask state with `faces` from +0x46a.
    let mut g = default_state(p);
    let fade = p.m.f32(this + 0x448);
    let faces: u32;
    if !(fade < 1.0) {
        g[1] &= !1;
        faces = 6;
        let next = fade + -1.0;
        let limit = p.m.f32(this + 0x44c);
        p.m.w::<f32>(this + 0x448, if next > limit { next } else { limit });
    } else {
        g[10] = 0x6464_6363;
        g[2] = 1f32.to_bits();
        g[3] = 1f32.to_bits();
        g[1] |= 1;
        faces = p.m.r8(this + 0x46a) as u32;
        g[4] = 1f32.to_bits();
        g[5] = fade.to_bits();
    }
    g[0] &= 0xffff_0000;
    p.apply_graphics_context(g.as_ptr() as usize, w);

    // The buffer object's setup state; the matrix the quads are drawn with (2 on the diagonal).
    p.m.w::<u32>(this2 + 0x290, 0);
    p.m.w::<u64>(this2 + 0x2a0, 0);
    p.m.w::<u64>(this2 + 0x298, 0);
    p.m.w::<u64>(this2 + 0x1d0, 0);
    p.m.w::<u64>(this2 + 0x288, 0);
    p.m.w::<u8>(this2 + 0x5b8, p.m.r8(this2 + 0x5b8) | 0x28);
    p.m.w::<u32>(this2 + 0x364, 0xffff_ffff);
    let levels = (p.m.r8(this2 + 0x39) as u32).wrapping_sub(1);
    let m: [f32; 12] = [2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0];
    let rows = p.pfn(QUAD_ROWS);
    let base = p.m.r8(this + 0x469) as u32;

    if levels != 0 && faces != 0 {
        for level in 0..levels {
            let size = ((p.m.r16(src + 0x30) as u32) >> (level & 0x1f)).max(1);
            let vp = square_viewport(p, size as f32);
            let mut first = true;
            for k in 0..faces {
                let face = (k + base) % 6;
                if face_setup(p, this2, w, face, level)? {
                    if first {
                        viewport::apply(p, vp.as_ptr() as usize, w, this2 + 0x368);
                    }
                    copy_quad(p, w, src, m.as_ptr() as usize, rows, 0, face, level as f32)?;
                    face_done(p, this2, w)?;
                    first = false;
                }
            }
        }
    }
    buffers_done(p, this2, w)?;

    // The last level of the buffer object's texture: a temporary colour target over a copy of its state (layer by
    // layer), in a buffer of the level's size, drawn from this+0x268.
    let t = o.keep(Host::new(p, TARGET_BYTES));
    new_target(p, t, COLOR_VTABLE);
    p.m.w::<u8>(t + 0xce, levels as u8);
    copy_target(p, t, this2)?;
    let rb = o.keep(Host::new(p, BUFFER_BYTES));
    new_buffer(p, rb);
    let (tw, th) = level_size(p, t, false);
    set_extent(p, rb, tw, th);
    p.m.w::<u64>(rb + 0x20, t as u64);
    let vp = square_viewport(p, tw as f32);
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    for k in 0..faces {
        let face = (k + base) % 6;
        if p.m.r16(t + 0xcc) as u32 != face {
            p.m.fetch_or32(t + 0xc8, 2);
            p.m.w::<u16>(t + 0xcc, face as u16);
        }
        // The bind's own viewport (a full one of the buffer's size) is not used by the port's bind.
        targets::bind(p, rb, w)?;
        copy_quad(p, w, this + 0x268, m.as_ptr() as usize, rows, 0, face, 0.0)?;
        targets::unbind(p, w)?;
    }
    p.m.commit();
    // The temporary state's slot reference given back.
    release(p, t + 8);
    let next = faces + base;
    p.m.w::<u8>(this + 0x469, (next % 6) as u8);
    Ok(())
}
