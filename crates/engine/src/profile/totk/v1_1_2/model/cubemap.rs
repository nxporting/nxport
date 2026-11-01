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
unsafe fn set_uniform(p: &mut Port, prog: usize, w: usize, k: usize, data: &[u8]) {
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
