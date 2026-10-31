// A model unit's material state (main+0x7b42f0), as the game records it into the material's cached command list
// (main+0x7b3c80: begin recording, this, end recording, the handle kept at +0x48 of the material's slot). The draw
// path calls those lists; this is what they contain, from the game's material objects.

use super::draw::{Port, R};

const PFN_DEVICE_GET_TEXTURE_HANDLE: usize = 0x466d428;
const PFN_BIND_TEXTURE: usize = 0x466e008;
const PFN_BUFFER_GET_ADDRESS: usize = 0x466d818;
const DEVICE_HOLDER: usize = 0x462bbe8;

/// main+0x7b42f0(d, w, entry, shader set, a, b): the unit `d`'s material (entry +8): the material's two extra
/// textures, the shader set's textures from the unit's defaults, the material's blocks (+0x1c slots: the defaults'
/// buffer, +0x40 block objects of the model's +0xd8 mask, +0x28 slots: the unit's shape buffer, +0x24 slots: the
/// model's +0x170 buffer).
pub(super) unsafe fn bind(p: &mut Port, d: usize, w: usize, entry: usize, set: usize, a: i32, b: i32) -> R<()> {
    let mat = p.m.r64(entry + 8);
    if mat == 0 {
        return Ok(());
    }
    let defaults = p.m.r64(p.m.r64(d + 8) + 0x180) + p.m.r16(d + 0x18) as usize * 0x80;
    // The defaults' two optional textures at the material's slots (+0x10 -> +0x8c / +0x90).
    for (o, so) in [(0x18usize, 0x8cusize), (0x20, 0x90)] {
        let tex = p.m.r64(p.m.r64(defaults + 0x68) + o);
        if tex == 0 {
            continue;
        }
        let slots = p.m.r32(p.m.r64(mat + 0x10) + so);
        if slots == u32::MAX {
            continue;
        }
        p.texture_ready(tex, "material texture still finishing")?;
        bind_texture_slots(p, w, slots, tex + 0x58, tex + 0xea);
    }
    if set != 0 && p.m.i32(set + 0x308) >= 1 {
        super::grid::bind_textures(p, w, set, defaults, &[0; 32], u32::MAX)?;
    }
    let cmd = p.m.r64(w + 0xb8);
    let defaults = p.m.r64(p.m.r64(d + 8) + 0x180) + p.m.r16(d + 0x18) as usize * 0x80;
    // The defaults' buffer (+0x60 bytes, +0x40 buffers 0x48 apart when +8 bit 0), element b.
    let size = p.m.r64(defaults + 0x60);
    if size != 0 {
        let bufs = p.m.r64(defaults + 0x40);
        let e = if bufs != 0 && p.m.r16(defaults + 8) & 1 != 0 { bufs + b as i64 as usize * 0x48 } else { 0 };
        let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[p.m.r64(e + 8) as u64]);
        bind_slots(p, cmd, p.m.r32(p.m.r64(mat) + 0x1c), addr, size as u32 as u64);
    }
    // The model's block objects (+0x98, eight, those with material slots at +0x14 of [mat+8]).
    let model = p.m.r64(d + 8);
    if p.m.r16(model + 0xd8) & 0xf != 0 {
        return Err("material blocks (main+0x766ef4)");
    }
    if p.m.r16(model + 0xd8) & 0xff0 != 0 {
        for k in 0..8usize {
            let slots = p.m.r32(p.m.r64(mat + 8) + k * 4 + 0x14);
            if slots == u32::MAX {
                continue;
            }
            let o = p.m.r64(model + 0x98 + k * 8);
            if o == 0 {
                continue;
            }
            let n = (p.m.r8(o + 0x42) & 0xf) as i32;
            // SDIV by zero gives 0 (the remainder is then b).
            let sel = if b != -1 { if n == 0 { b } else { b.wrapping_rem(n) } } else { (p.m.r8(o + 0x42) >> 4) as i32 };
            let count = p.m.r16(o + 0x40) as i64;
            let size = p.m.r32(o + 0x38);
            let al = p.block_alignment(o)?;
            let al2 = p.block_alignment(o)?;
            let stride = ((size as i32).wrapping_add(al).wrapping_sub(1) & al2.wrapping_neg()) as u32 as u64;
            let addr = (p.m.r64(o + 0x78) as u64).wrapping_add(stride.wrapping_mul((sel as i64 * count) as u64));
            for (stage, sh) in [(0, 0), (1, 8), (2, 16), (5, 24)] {
                let s = (slots >> sh) & 0xff;
                if s != 0xff {
                    p.bind_block(o, cmd, stage, s as i8 as i32, addr, size)?;
                }
            }
        }
    }
    // The unit's shape buffer (the model's +0x178 table, 0x70 apart by +0x16; +0x20 buffers; per-unit element when
    // +0xd: +0xf * a + b), 0x100 bytes.
    let t = p.m.r64(model + 0x178) + p.m.r16(d + 0x16) as usize * 0x70;
    let idx = if p.m.r8(t + 0xd) != 0 { (p.m.r8(t + 0xf) as i32).wrapping_mul(a).wrapping_add(b) } else { b };
    let bufs = p.m.r64(t + 0x20);
    let e = if bufs != 0 { bufs + idx as i64 as usize * 0x48 } else { 0 };
    let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[p.m.r64(e + 8) as u64]);
    bind_slots(p, cmd, p.m.r32(p.m.r64(mat) + 0x28), addr, 0x100);
    // The model's +0x170 buffer (+0x40 bytes, +0x38 buffers), element b.
    let m170 = p.m.r64(model + 0x170);
    let size = p.m.r64(m170 + 0x40);
    if size != 0 {
        let bufs = p.m.r64(m170 + 0x38);
        let e = if bufs != 0 { bufs + b as i64 as usize * 0x48 } else { 0 };
        let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[p.m.r64(e + 8) as u64]);
        bind_slots(p, cmd, p.m.r32(p.m.r64(mat) + 0x24), addr, size as u32 as u64);
    }
    extra_shapes(p, d)
}

/// The unit's extra shape list ([d+0x68]+0xb00): not ported.
unsafe fn extra_shapes(p: &Port, d: usize) -> R<()> {
    if p.m.r64(p.m.r64(d + 0x68) + 0xb00) != 0 {
        return Err("material extra shapes");
    }
    Ok(())
}

/// A uniform buffer at the stage slots packed in `slots` (stages 0, 1, 2 and 5).
unsafe fn bind_slots(p: &mut Port, cmd: usize, slots: u32, addr: u64, size: u64) {
    for (stage, sh) in [(0, 0), (1, 8), (2, 16), (5, 24)] {
        let s = (slots >> sh) & 0xff;
        if s != 0xff {
            p.bind_uniform_buffer(cmd, stage, s as i8 as i32, addr, size);
        }
    }
}

/// main+0xcc9f14(w, slots, &texture id, &sampler id): the texture handle at the stage slots packed in `slots`.
unsafe fn bind_texture_slots(p: &mut Port, w: usize, slots: u32, id: usize, sampler: usize) {
    let tid = p.m.i32(id);
    let smp = p.m.r16(sampler) as i16 as i32;
    if tid == -1 || smp == -1 {
        return;
    }
    let dev = p.m.r64(p.m.r64(p.m.r64(p.base + DEVICE_HOLDER)) + 0x30);
    let h = p.query(p.pfn(PFN_DEVICE_GET_TEXTURE_HANDLE), &[dev as u64, tid as u32 as u64, smp as u32 as u64]);
    let cmd = p.m.r64(w + 0xb8) as u64;
    for (stage, sh) in [(0u64, 0), (1, 8), (2, 16), (5, 24)] {
        let s = (slots >> sh) & 0xff;
        if s != 0xff {
            p.call(p.pfn(PFN_BIND_TEXTURE), &[cmd, stage, s as i8 as i64 as u64, h]);
        }
    }
}
