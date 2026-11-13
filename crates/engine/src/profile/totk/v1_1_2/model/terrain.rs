// TotK's terrain drawable (main+0x2a47078), ported: a dispatcher into 32 instantiations of one tile loop (a member
// function table at main+0x45c2800, indexed by five feature bits). Each loop walks the terrain object's tiles, tests
// them (enable bit, pass mask, flags, bounding sphere against the view frustum), sets the draw context up on the first
// visible one (load_resource) and draws the shared grid mesh per tile with the tile's uniform block. Variant 0 is
// ported; the others call the game's function.

use super::draw::{load_resource, Port, R};

const VARIANTS: usize = 0x45c2800;

/// Calls of the variants the port does not draw, by index.
static OTHER: std::sync::Mutex<[u64; 32]> = std::sync::Mutex::new([0; 32]);

pub(super) fn report() -> String {
    let g = std::mem::take(&mut *OTHER.lock().unwrap());
    g.iter().enumerate().filter(|(_, n)| **n > 0).map(|(i, n)| format!("{i}: {n}")).collect::<Vec<_>>().join(", ")
}

/// main+0x2a47078(drawable, ctx, sub, flags).
pub(super) unsafe fn draw(p: &mut Port, d: usize, c: usize, sub: u32, flags: u32) -> R<()> {
    let m = &p.m;
    let b4 = if m.r8(m.r64(c + 0x10) + 10) & 1 == 0 { 0 } else { ((m.r64(c + 0x100) != 0) as u32) << 4 };
    let s40 = m.r16(c + 0x40) as u32;
    let b3 = if (s40 >> 8) & 1 == 0 { 0 } else { (m.r8(m.r64(m.r64(d + 0x68) + 0x8c8) + 0x50) as u32 & 1) << 3 };
    let f = flags & m.r8(d + 0x47) as u32;
    let idx = b4 | ((f >> 2) & 2 | (f >> 4) & 1) | ((s40 >> 8) & 4) | b3;
    if p.m.r64(c + 0x100) != 0 {
        let cmd = p.m.r64(p.m.r64(c + 0x20) + 0xb8) as u64;
        p.render_enable(cmd as usize, true);
        p.m.w::<u64>(c + 0x100, 0);
    }
    let n = match p.m.r32(c + 8) {
        0 => 1,
        n => n,
    };
    let entry = p.base + VARIANTS + idx as usize * 16;
    let (func, adj) = (p.m.r64(entry), p.m.r64(entry + 8) as i64);
    let this = (p.m.r64(p.m.r64(d + 8) + 0x40) as i64 + (adj >> 1)) as usize;
    if (idx == 0 || idx == 8) && adj & 1 == 0 {
        tiles(p, this, c, d, sub, n, idx == 8)?;
    } else if idx == 12 && adj & 1 == 0 {
        tile_lists(p, this, c, d, n)?;
    } else {
        OTHER.lock().unwrap()[idx as usize] += 1;
        let target = if adj & 1 != 0 { p.m.r64(p.m.r64(this) + func) } else { func };
        p.helper("terrain variant", target.wrapping_sub(p.base), &[this as u64, c as u64, d as u64, sub as u64, n as u64])?;
    }
    Ok(())
}

/// Variant 0 (main+0x2246be4): the tile loop without any of the feature bits; variant 8 (main+0x224790c, feature bit
/// 3) draws each tile with the LOD `sub`'s prebuilt display list (main+0x2247ab8) instead. Returns the tiles drawn (-1
/// when the context could not be set up).
unsafe fn tiles(p: &mut Port, t: usize, c: usize, d: usize, sub: u32, n: u32, lod_lists: bool) -> R<i32> {
    let count = p.m.r16(t + 0x22) as usize;
    if count == 0 {
        return Ok(0);
    }
    let view = p.m.r8(c + 0x46) as usize;
    let w = p.m.r64(c + 0x20);
    let frustum = p.m.r64(p.m.r64(c + 0x10) + 0x18) + view * 0x430 + 0x60;
    let mut drawn = 0i32;
    let mut slots = 0xffff_ffffu32;
    for i in 0..count {
        let tile = p.m.r64(p.m.r64(t + 0x10) + i * 8);
        if p.m.r8(tile + 0x90) & 1 == 0
            || (p.m.r32(tile + 0x74) >> (p.m.r8(c + 0x30) & 0x1f)) & 1 == 0
            || n & p.m.r16(tile + 0x78) as u32 != 0
            || !sphere_visible(p, frustum, p.m.r64(tile + 0x28))
        {
            continue;
        }
        if drawn == 0 {
            if !load_resource(p, c, d)? {
                return Ok(-1);
            }
            slots = p.m.r32(p.m.r64(p.m.r64(c + 0x110)) + 0x28);
        }
        let b92 = p.m.r8(tile + 0x92) as i32;
        let index = (b92 & 3) + ((b92 >> 2) & 3) * p.m.r16(tile + 0x7a) as i32;
        p.bind_uniform_block(p.m.r64(tile + 0x10), w, slots, index)?;
        if lod_lists {
            lod_list(p, d, w, c, sub as usize);
        } else {
            tile_draw(p, d, w, c, sub);
        }
        drawn += 1;
    }
    Ok(drawn)
}

/// Variant 12 (main+0x22482b0: feature bits 2 and 3): the view's list of tiles already found visible (+0x38, 0x20-byte
/// records: uniform block object, view mask at +0x10, flags +0x14, block index +0x16, LOD +0x18), each drawn with its
/// LOD's prebuilt display list. Returns the tiles drawn (-1 when the context could not be set up).
unsafe fn tile_lists(p: &mut Port, t: usize, c: usize, d: usize, n: u32) -> R<i32> {
    let mut view = p.m.r8(c + 0x30) as u32;
    if p.m.r32(t + 0x30) <= view {
        view = 0;
    }
    let rec = p.m.r64(t + 0x38) + view as usize * 0x20;
    let list = p.m.r64(rec + 8);
    let count = p.m.i32(rec);
    if list == 0 || count < 1 {
        return Ok(0);
    }
    let bit = p.m.r8(c + 0x46) & 0x1f;
    let w = p.m.r64(c + 0x20);
    let mut drawn = 0i32;
    let mut slots = 0xffff_ffffu32;
    for i in 0..count as usize {
        let tile = list + i * 0x20;
        if p.m.r32(tile + 0x10) & (1 << bit) == 0 || p.m.r16(tile + 0x14) as u32 & n != 0 {
            continue;
        }
        if drawn == 0 {
            if !load_resource(p, c, d)? {
                return Ok(-1);
            }
            slots = p.m.r32(p.m.r64(p.m.r64(c + 0x110)) + 0x28);
        }
        p.bind_uniform_block(p.m.r64(tile), w, slots, p.m.r16(tile + 0x16) as i32)?;
        lod_list(p, d, w, c, p.m.r8(tile + 0x18) as usize);
        drawn += 1;
    }
    Ok(drawn)
}

/// sub_2246d3c(drawable, w, ctx, lod): the drawable's grid mesh at that LOD, counted into the context.
unsafe fn tile_draw(p: &mut Port, d: usize, w: usize, c: usize, lod: u32) {
    let model = p.m.r64(d + 0x68);
    let res = p.m.r64(model + 8);
    if (lod as i32) < p.m.r8(res + 0x5b) as i32 {
        let sh = p.m.r64(res + 0x18) + lod as usize * 0x38;
        p.draw_shape(sh, w + 8, 0, p.m.r16(sh + 0x34) as u32, 1, 0);
        let tris = p.m.r32(p.m.r64(model + 0x8c8) + lod as usize * 0x268 + 0x260);
        p.m.w::<u32>(c + 0x34, p.m.r32(c + 0x34).wrapping_add(tris));
    }
}

/// sub_b60490(frustum, sphere): the bounding sphere (centre xyz, radius) against the view's near / far depths and
/// four side planes, in the game's operation order (no fused multiply-adds).
unsafe fn sphere_visible(p: &Port, f: usize, s: usize) -> bool {
    let m = &p.m;
    let (x, y, z, r) = (m.f32(s), m.f32(s + 4), m.f32(s + 8), m.f32(s + 12));
    let vz = m.f32(f + 0xcc) + ((m.f32(f + 0xc0) * x + m.f32(f + 0xc4) * y) + m.f32(f + 0xc8) * z);
    let nz = -vz;
    if m.f32(f + 0x180) - r > nz || m.f32(f + 0x184) < nz - r {
        return false;
    }
    let vx = m.f32(f + 0xac) + ((x * m.f32(f + 0xa0) + y * m.f32(f + 0xa4)) + z * m.f32(f + 0xa8));
    let vy = m.f32(f + 0xbc) + ((x * m.f32(f + 0xb0) + y * m.f32(f + 0xb4)) + z * m.f32(f + 0xb8));
    for k in 0..4 {
        let pl = f + k * 0x10;
        let dist = ((vx * m.f32(pl) + vy * m.f32(pl + 4)) + vz * m.f32(pl + 8)) - m.f32(pl + 12);
        if !(dist < r) {
            return false;
        }
    }
    true
}

/// main+0x2247ab8(d, w, c, lod): the model's prebuilt display list of LOD `lod`, its triangles counted.
unsafe fn lod_list(p: &mut Port, d: usize, w: usize, c: usize, lod: usize) {
    let model = p.m.r64(d + 0x68);
    if (lod as i32) < p.m.r8(p.m.r64(model + 8) + 0x5b) as i32 {
        let lists = p.m.r64(model + 0x8c8);
        if p.native {
            // The list is sub_758644's draw of the LOD's sub-meshes, recorded at load.
            let sh = p.m.r64(p.m.r64(model + 8) + 0x18) + lod * 0x38;
            p.draw_shape(sh, w + 8, 0, p.m.r16(sh + 0x34) as u32, 1, 0);
        } else {
            p.call_commands(p.m.r64(w + 0xb8), lists + lod * 0x268 + 0x48);
        }
        let tris = p.m.r32(lists + lod * 0x268 + 0x260);
        p.m.w::<u32>(c + 0x34, p.m.r32(c + 0x34).wrapping_add(tris));
    }
}
