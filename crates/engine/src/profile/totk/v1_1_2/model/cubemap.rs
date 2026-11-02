// agl::env::CubeMap's mip filtering (main+0x7ec9f4 -> main+0x7ed190): per face, main+0x7ee7a8 draws the face of one mip
// level from the cube texture with the filter program: the program, the texture, the program's uniforms written into
// the driver's uniform buffer, then the full-screen shape.

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, SHAPES, STREAMS};
use super::vfx::{program_apply, program_texture};

/// [[[[[[holder]]+0x10]+0x350]+8]+0x10]: the filter program.
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[holder]]: the driver uniform buffer (GPU address +0x78, size +0x38) program uniforms are written into.
const UNIFORMS_HOLDER: usize = 0x4632818;
/// Two floats per kind (the last argument).
const KIND_TABLE: usize = 0x386d4e8;

/// The program's uniform `k` (+0x40 locations, 16-byte entries, count +0x38) set to `data` (nothing without one).
pub(super) unsafe fn set_uniform(p: &mut Port, prog: usize, w: usize, k: usize, data: &[u8]) {
    let n = p.m.r32(prog + 0x38) as usize;
    let loc = p.m.i32(p.m.r64(prog + 0x40) + if n > k { k * 16 } else { 0 });
    if loc == -1 {
        return;
    }
    let g = p.m.r64(p.m.r64(p.base + UNIFORMS_HOLDER));
    let cmd = p.m.r64(w + 0xb8);
    p.m.w::<u8>(cmd + 0x10, 1);
    p.update_uniform(cmd, p.m.r64(g + 0x78) as u64, p.m.r64(g + 0x38) as u64, loc as i64, data.len(), data.as_ptr() as usize);
}

/// main+0x7ee7a8(w, tex, level, face, kind, value: f32 in s0, as its bits).
pub(super) unsafe fn face(p: &mut Port, w: usize, tex: usize, level: u32, face: u32, kind: u32, value: u32) -> R<()> {
    let h = p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER)) + 0x10) + 0x350) + 8);
    let prog = p.m.r64(h + 0x10);
    let size = ((p.m.r16(tex + 0x30) as u32) >> (level & 31)).max(1);
    program_apply(p, prog, w)?;
    program_texture(p, prog, w, 0, tex)?;
    set_uniform(p, prog, w, 0, &((p.m.r8(tex + 0xe8) & 1) as u32).to_le_bytes());
    set_uniform(p, prog, w, 3, &(level as f32).to_le_bytes());
    set_uniform(p, prog, w, 1, &face.to_le_bytes());
    set_uniform(p, prog, w, 5, &value.to_le_bytes());
    set_uniform(p, prog, w, 4, &(size as f32).to_le_bytes());
    let mut pair = [0u8; 8];
    for (k, b) in pair.iter_mut().enumerate() {
        *b = p.m.r8(p.base + KIND_TABLE + kind as usize * 8 + k);
    }
    set_uniform(p, prog, w, 6, &pair);
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0xd28, w);
    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x1e8, w);
    Ok(())
}

/// main+0x877418(w, prog, tex, m, rows, v): the full-screen shape with `prog`'s texture slot 0 set to `tex` and its
/// uniforms 0 (the four `rows` transformed by the 3x4 matrix `m`: each row's xyz through the matrix columns, its w added
/// to the last lane), 8 (16 bytes at `v`) and 10 (the texture's +0xe8 bit 0). Used by the frame's full-screen copies.
pub(super) unsafe fn transformed_quad(p: &mut Port, w: usize, prog: usize, tex: usize, m: usize, rows: usize, v: usize) -> R<()> {
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    program_texture(p, prog, w, 0, tex)?;
    let f = |p: &Port, a: usize| p.m.f32(a);
    let mut out = [0u8; 64];
    for i in 0..4 {
        let b = rows + i * 16;
        let (b0, b1, b2, b3) = (f(p, b), f(p, b + 4), f(p, b + 8), f(p, b + 12));
        for l in 0..4 {
            // fmul, fmla, fmla, then fadd with (0, 0, 0, w): as the game's vector code.
            let x = f(p, m + l * 4) * b0;
            let x = f(p, m + 16 + l * 4).mul_add(b1, x);
            let x = f(p, m + 32 + l * 4).mul_add(b2, x);
            let x = if l == 3 { b3 + x } else { 0.0 + x };
            out[i * 16 + l * 4..i * 16 + l * 4 + 4].copy_from_slice(&x.to_le_bytes());
        }
    }
    set_uniform(p, prog, w, 0, &out);
    let mut v16 = [0u8; 16];
    for (k, x) in v16.iter_mut().enumerate() {
        *x = p.m.r8(v + k);
    }
    set_uniform(p, prog, w, 8, &v16);
    set_uniform(p, prog, w, 10, &((p.m.r8(tex + 0xe8) & 1) as u32).to_le_bytes());
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0xd28, w);
    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x1e8, w);
    Ok(())
}

/// main+0x876e28(w, tex, m, rows, a, b, value in s0): a transformed full-screen copy of `tex` with the copy program for
/// the texture's kind (+0x3a 9: array) and for `value` -1 or not: uniforms 3 (`a` as float), 17 (`b`) and 2 (`value`),
/// then main+0x877418 with v = (1, 1, 1, 1).
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn copy_quad(p: &mut Port, w: usize, tex: usize, m: usize, rows: usize, a: i32, b: u32, value: f32) -> R<()> {
    let off = if p.m.r16(tex + 0x3a) != 9 { 0x148 } else { 0x150 };
    let slot = if value == -1.0 { 0x10 } else { 0x20 };
    let h = p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER)) + 0x10) + off);
    let prog = p.m.r64(p.m.r64(h + 8) + slot);
    program_apply(p, prog, w)?;
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    set_uniform(p, prog, w, 3, &(a as f32).to_le_bytes());
    set_uniform(p, prog, w, 17, &b.to_le_bytes());
    set_uniform(p, prog, w, 2, &value.to_le_bytes());
    let ones = [1.0f32; 4];
    transformed_quad(p, w, prog, tex, m, rows, ones.as_ptr() as usize)
}

/// sead's orthographic projection type (vtable holder), its matrix update and the frame's near / far.
const ORTHO_VTABLE: usize = 0x4634ef8;
const ORTHO_UPDATE: usize = 0xbe2488;
const ORTHO_NEAR: usize = 0x4634ee8;
const ORTHO_FAR: usize = 0x4634ef0;

/// main+0xbe1b2c(w, prog, tex, viewport, scale, offset): `tex` drawn as a quad of `scale` (in texture sizes) at
/// `offset` within the viewport: uniform 1 = (width, height, 1 / width, 1 / height) of the texture, uniform 0 = the
/// viewport's orthographic device matrix (sead's own functions, on a projection laid out as the game's) times the
/// quad's transform, texture slot 0, then the full-screen shape.
pub(super) unsafe fn texture_quad(p: &mut Port, w: usize, prog: usize, tex: usize, vp: usize, scale: usize, offset: usize) -> R<()> {
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let tw = (p.m.r16(tex + 0x30) as u32).max(1);
    let h = p.m.r16(tex + 0x32) as u32;
    let th = { let v = if p.m.r16(tex + 0x3a) == 3 { h } else { 1 }; if v > h { v } else { h } };
    let (fw, fh) = (tw as f32, th as f32);
    let mut size = [0u8; 16];
    for (k, v) in [fw, fh, 1.0 / fw, 1.0 / fh].iter().enumerate() {
        size[k * 4..k * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    set_uniform(p, prog, w, 1, &size);
    // The projection object (as at the game's sp+0x78).
    let mut proj = [0u32; 0x2c];
    let pp = proj.as_mut_ptr() as usize;
    let f = |o: usize| p.m.f32(vp + o);
    let (x, y, x2, y2) = (f(8), f(0xc), f(0x10), f(0x14));
    let vt = (p.m.r64(p.base + ORTHO_VTABLE) + 0x10) as u64;
    proj[0] = vt as u32;
    proj[1] = (vt >> 32) as u32;
    proj[2] = 0x101;
    proj[0x23] = p.m.r32(vp + 0x18);
    proj[0x24] = p.m.r32(p.m.r64(p.base + ORTHO_NEAR));
    proj[0x25] = p.m.r32(p.m.r64(p.base + ORTHO_FAR));
    proj[0x26] = 0f32.to_bits();
    proj[0x27] = 1f32.to_bits();
    let (dh, dw) = (y2 - y, x2 - x);
    proj[0x28] = (dh * 0.5).to_bits();
    proj[0x29] = (dh * -0.5).to_bits();
    proj[0x2a] = (dw * -0.5).to_bits();
    proj[0x2b] = (dw * 0.5).to_bits();
    let cy = (y2 + y) * 0.5 - y;
    let cx = (x2 + x) * 0.5 - x;
    let sx = p.m.f32(scale) * fw;
    let sy = p.m.f32(scale + 4) * fh;
    let tx = (p.m.f32(offset) - cx) + sx * 0.5;
    let ty = -((p.m.f32(offset + 4) - cy) + sy * 0.5);
    p.query(p.base + ORTHO_UPDATE, &[pp as u64, (pp + 0xc) as u64]);
    proj[2] = 0x100;
    let update_device = p.m.r64(p.m.r64(pp) + 0x60);
    p.query(update_device, &[pp as u64, (pp + 0x4c) as u64, (pp + 0xc) as u64, proj[0x23] as u64]);
    // Each device-matrix row r: (sx, 0, 0, tx) * r0, fused with (0, sy, 0, ty) * r1 and 0 * r2, plus (0, 0, 0, r3).
    let row_x = [sx, 0.0, 0.0, tx];
    let row_y = [0.0, sy, 0.0, ty];
    let mut m = [0u8; 64];
    for r in 0..4 {
        let d = |k: usize| f32::from_bits(proj[3 + r * 4 + k]);
        for l in 0..4 {
            let v = row_x[l] * d(0);
            let v = row_y[l].mul_add(d(1), v);
            let v = 0f32.mul_add(d(2), v);
            let v = (if l == 3 { d(3) } else { 0.0 }) + v;
            m[r * 16 + l * 4..r * 16 + l * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    program_texture(p, prog, w, 0, tex)?;
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    set_uniform(p, prog, w, 0, &m);
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0xd28, w);
    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x1e8, w);
    Ok(())
}
