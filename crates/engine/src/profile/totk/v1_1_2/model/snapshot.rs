// main+0xcc75ec(snap, w, index, ctx): a snapshot's mip levels drawn into its record's render buffers. The record (+0x178,
// 0x1d10 bytes, +0x170 count) is the render buffer itself; per level its size, colour targets (one, two or six by the
// root's kind +0x860) and a full-viewport viewport are filled in, then the level's uniforms and the full-screen shape
// are drawn and the buffer unbound. Snapshots copied from another one first (+0x325 set) are left to the game.

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, SHAPES, STREAMS};
use super::vfx::{program_apply, program_block, program_texture};

/// The viewport's vtable pointer and the default orientation.
const VIEWPORT_VTABLE: usize = 0x46323b0;
const ORIENTATION: usize = 0x46312d8;
/// The context type the unbind casts the command buffer wrapper to (initialised once, guard byte bit 0).
const CONTEXT_TYPE_GUARD: usize = 0x4634f30;
const CONTEXT_TYPE: usize = 0x4634f38;
const UNBOUND_NAME: usize = 0x37acc70;

unsafe fn root(p: &Port, mut s: usize) -> usize {
    while p.m.r64(s + 0x168) != 0 {
        s = p.m.r64(s + 0x168);
    }
    s
}

pub(super) unsafe fn draw(p: &mut Port, snap: usize, w: usize, index: u32, ctx: usize) -> R<()> {
    if p.m.r8(snap + 0x325) as i8 != -1 {
        return Err("snapshot copy");
    }
    let i = if p.m.r32(snap + 0x170) <= index { 0 } else { index } as i32 as isize as usize;
    let record = p.m.r64(snap + 0x178).wrapping_add(i.wrapping_mul(0x1d10));
    let top = root(p, snap);
    let kind = p.m.i32(top + 0x860) as isize as usize;
    let c: [usize; 8] = std::array::from_fn(|k| p.m.r64(ctx + k * 8));
    let shader = if p.m.r8(snap + 0x323) == 0 {
        c[3].wrapping_add(kind.wrapping_mul(0x30))
    } else {
        let mut v = p.m.r8(snap + 0x323) as i32 * 8;
        v += match p.m.i32(top + 0x708) {
            1 => -6,
            2 => -2,
            3 => -4,
            _ => -8,
        };
        if p.m.r8(snap + 0x728) == 0 {
            v += 1;
        }
        c[2].wrapping_add((v as isize as usize).wrapping_mul(0x90)).wrapping_add(kind.wrapping_mul(0x30))
    };
    p.apply_graphics_context(c[0].wrapping_add(kind.wrapping_mul(0xe8)), w);
    let prog = p.m.r64(shader + 8);
    program_apply(p, prog, w)?;
    program_texture(p, prog, w, 1, p.m.r64(ctx.wrapping_add(4usize.wrapping_add(kind).wrapping_mul(8))))?;
    program_texture(p, prog, w, 0, c[7])?;
    program_block(p, prog, w, 0, snap + 0x638, 0)?;
    program_block(p, prog, w, 1, record + 0x1c90, 0)?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);
    let targets = match kind {
        2 => 6,
        1 => 2,
        _ => 1,
    };
    let levels = p.m.r8(snap + 0x2a9) as usize;
    for k in 0..levels {
        let s = ((p.m.r16(snap + 0x2a0) as u32).max(1) >> (k as u32 & 31)) as f32;
        p.m.w::<u32>(record + 0x10, 0);
        p.m.w::<u32>(record + 0x14, 0);
        for o in [8, 0xc, 0x18, 0x1c] {
            p.m.w::<f32>(record + o, s);
        }
        for o in (0..0x48).step_by(8) {
            p.m.w::<u64>(record + 0x20 + o, 0);
        }
        let level = if k < 5 { record + 0x70 + k * 0x5a0 } else { record + 0x70 };
        for j in 0..targets {
            p.m.w::<u64>(record + 0x20 + j * 8, (level + j * 0xf0) as u64);
        }
        // The viewport covering the buffer (sead::Viewport: vtable, x, y, far corner, orientation, depth range).
        let orientation = p.m.i32(p.m.r64(p.base + ORIENTATION));
        let (bw, bh) = (p.m.f32(record + 8), p.m.f32(record + 0xc));
        let span = |v: f32| if v <= 0.0 { (v, 0.0) } else { (0.0, v) };
        let (x, y, x2, y2) = match orientation as u32 {
            0 | 3..=5 => {
                let ((x, x2), (y, y2)) = (span(bw), span(bh));
                (x, y, x2, y2)
            }
            1 | 2 => {
                let ((x, x2), (y, y2)) = (span(bh), span(bw));
                (x, y, x2, y2)
            }
            _ => (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX),
        };
        let mut vp = [0u32; 10];
        let vt = (p.m.r64(p.base + VIEWPORT_VTABLE) + 0x10) as u64;
        vp[0] = vt as u32;
        vp[1] = (vt >> 32) as u32;
        for (n, v) in [x, y, x2, y2].into_iter().enumerate() {
            vp[2 + n] = v.to_bits();
        }
        vp[6] = orientation as u32;
        vp[7] = 0f32.to_bits();
        vp[8] = 1f32.to_bits();
        super::targets::bind(p, record, w)?;
        super::viewport::apply(p, vp.as_ptr() as usize, w, record);
        let n = if (k as u32) < p.m.r32(snap + 0x6b8) { k } else { 0 };
        program_block(p, prog, w, 2, p.m.r64(snap + 0x6c0) + n * 0x80, 0)?;
        stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x488, w);
        unbind(p, w)?;
    }
    Ok(())
}

/// The wrapper's render buffer unbound (current buffer cleared, name "Unbound", the buffer saved at +0x100 bound again).
unsafe fn unbind(p: &mut Port, w: usize) -> R<()> {
    if p.m.r8(p.m.r64(p.base + CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    if w == 0 {
        return Ok(());
    }
    let g = p.query(p.base + p.vfn(w, 0), &[w as u64, p.m.r64(p.base + CONTEXT_TYPE) as u64]) as usize;
    if g == 0 {
        return Ok(());
    }
    p.m.w::<u64>(g + 0xf8, 0);
    p.m.w::<u64>(g + 0x110, (p.base + UNBOUND_NAME) as u64);
    p.m.w::<u8>(g + 0x108, p.m.r8(g + 0x108) & !2);
    let saved = p.m.r64(g + 0x100);
    if saved != 0 {
        super::targets::bind(p, saved, g)?;
        p.m.w::<u64>(g + 0x100, 0);
    }
    Ok(())
}
