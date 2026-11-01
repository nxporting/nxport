// The frame's binding defaults, main+0x9c7b60's (graphics settings +0x782 bits 1 / 2): one texture at all 32 texture
// slots and one uniform block at the 14 uniform slots of the vertex, tessellation and fragment stages, so no draw
// reads an unbound slot.

use super::draw::{Port, R};

/// [[holder]]+0x18: the default texture.
const DEFAULT_TEXTURE_HOLDER: usize = 0x4631740;
const DEVICE_HOLDER: usize = 0x462bbe8;
const PFN_DEVICE_GET_TEXTURE_HANDLE: usize = 0x466d428;
const STAGES: [i32; 4] = [0, 1, 2, 5];

/// main+0x9c7e04(settings, w).
pub(super) unsafe fn default_textures(p: &mut Port, settings: usize, w: usize) -> R<()> {
    if (p.m.r8(settings + 0x782) >> 1) & 1 == 0 {
        return Ok(());
    }
    let tex = p.m.r64(p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURE_HOLDER)) + 0x18);
    let cmd = p.m.r64(w + 0xb8);
    for slot in 0..32 {
        p.texture_ready(tex, "default texture still finishing")?;
        let (id, smp) = (p.m.i32(tex + 0x58), p.m.r16(tex + 0xea) as i16 as i32);
        if id == -1 || smp == -1 {
            continue;
        }
        let dev = p.m.r64(p.m.r64(p.m.r64(p.base + DEVICE_HOLDER)) + 0x30);
        let h = p.query(p.pfn(PFN_DEVICE_GET_TEXTURE_HANDLE), &[dev as u64, id as u32 as u64, smp as u32 as u64]);
        for st in STAGES {
            p.bind_texture(cmd, st, slot, h);
        }
    }
    Ok(())
}

/// main+0x9c7c80(settings, w): the block (+0x4d8) at uniform slots 0..13.
pub(super) unsafe fn default_uniforms(p: &mut Port, settings: usize, w: usize) -> R<()> {
    if (p.m.r8(settings + 0x782) >> 2) & 1 == 0 {
        return Ok(());
    }
    let block = p.m.r64(settings + 0x4d8);
    for slot in 0..14u32 {
        p.bind_uniform_block(block, w, slot * 0x0101_0101, 0)?;
    }
    Ok(())
}
