// A grid delegate target (main+0x12e3774, virtual +0x48 of the drawable main+0x27da4cc gates): per visible cell of
// the grid's level 0 up to four parts, each with its own LOD entry, faded by distance, between the command buffer's
// tiled caching on and off.

use super::draw::{load_resource, Port, R};
use super::grid::{bind_slot, bind_texture_table, draw_indices, fmax, fmin};

const PFN_SET_TILED_CACHE_ACTION: usize = 0x466e138;
const PFN_SET_TILED_CACHE_TILE_SIZE: usize = 0x466e140;
const ENV_HOLDER: usize = 0x46353f8;
const NVN_MGR_HOLDER: usize = 0x4635188;
const MIN_SCALE: usize = 0x380eeb0;
const RANGE_SCALE: usize = 0x380ea80;
const FADE_MIN_DIST: usize = 0x380f934;
const FADE_BIAS: usize = 0x380f3e8;
const ENV_TEXTURE: usize = 0x745e10;
const TEXTURE_PRESENT: usize = 0x745630;
const TEXTURE_OF: usize = 0x745674;
const CELL_INDEX: usize = 0x113b2d4;
const TILE_AT: usize = 0x113b468;
const TILE_READY: usize = 0x113b4f4;

/// main+0x113b3e0 / main+0x113b2ac: the NVN manager's tiled caching on (tile size 64x128 unless the manager sets its
/// own) / off, unless the manager has it disabled (+8 bits 0-1).
unsafe fn tiled_caching(p: &mut Port, w: usize, on: bool) {
    let mgr = p.m.r64(p.m.r64(p.base + NVN_MGR_HOLDER));
    let flags = p.m.r32(mgr + 8);
    if flags & 3 != 0 {
        return;
    }
    let cmd = p.m.r64(w + 0xb8) as u64;
    if !on {
        p.call(p.pfn(PFN_SET_TILED_CACHE_ACTION), &[cmd, 2]);
        return;
    }
    let (tw, th) = if flags & 4 != 0 { (p.m.r16(mgr + 0x14) as u64, p.m.r16(mgr + 0x16) as u64) } else { (64, 128) };
    p.call(p.pfn(PFN_SET_TILED_CACHE_ACTION), &[cmd, 1]);
    p.call(p.pfn(PFN_SET_TILED_CACHE_TILE_SIZE), &[cmd, tw, th]);
}

/// main+0x12e40a0 / main+0x12e4010 through main+0x18cfa74 (layer 0, unit 12), main+0x84da58 / main+0x84dc00: part
/// `key[1]` of the cell's range in `layer` (`split`: one of four LOD sub-ranges, else the part's whole range) scaled by
/// `fade` in whole units; the remainder unit is drawn when the fraction reaches the cell's dither threshold (key / 32); a
/// positive key[2] draws from the range's end.
#[allow(clippy::too_many_arguments)]
unsafe fn draw_part(p: &mut Port, s: usize, w: usize, key: [u32; 3], fade: f32, split: bool, layer: u32, unit: u32) -> u32 {
    let a = if key[0] < 4 { key[0] } else { 0 } as usize;
    let e = s + if layer < 2 { layer } else { 0 } as usize * 0x140 + a * 0x50;
    let (first, count) = if split {
        let e = e + if key[1] < 4 { key[1] } else { 0 } as usize * 0x10;
        (p.m.r32(e + 0x4c8), p.m.r32(e + 0x4cc))
    } else {
        (p.m.r32(e + 0x508), p.m.r32(e + 0x50c))
    };
    let v = (fade * count as f32) as u32 as f32 / unit as f32;
    let mut n = v as i32;
    if v != (v as i32) as f32 && !(v > 0.0) {
        n -= 1;
    }
    let h = (key[0] as i32).wrapping_mul(4).wrapping_add((key[2] as i32).wrapping_mul(16)).wrapping_add(key[1] as i32);
    let extra = if v - n as f32 >= h as f32 * 0.03125 { unit } else { 0 };
    let total = (n as u32).wrapping_mul(unit).wrapping_add(extra);
    let start = if key[2] as i32 > 0 { first.wrapping_add(count).wrapping_sub(total) } else { first };
    if total == 0 {
        return 0;
    }
    draw_indices(p, s, w, start, total)
}

/// The level texture holder's texture (its virtual +0x210).
unsafe fn level_texture(p: &mut Port) -> R<u64> {
    let holder = p.m.r64(p.m.r64(p.m.r64(p.base + LEVEL_TEXTURE_HOLDER)) + 0xd0);
    let f = p.vfn(holder, 0x210);
    // main+0x287bf48: the holder's +0x260 object's texture at +0x190.
    Ok(if f == 0x287bf48 { (p.m.r64(holder + 0x260) + 0x190) as u64 } else { p.helper("level texture", f, &[holder as u64])? })
}

/// Column (`o` 0) or row (`o` 2) `v` of level `l` relative to its origin, wrapped by its step.
unsafe fn cell_pos(p: &Port, l: usize, v: u32, o: usize) -> i32 {
    let step = if (v as i32) < p.m.r16(l + 0x28 + o) as i16 as i32 { p.m.i32(l + 0x24) } else { 0 };
    (v as i32).wrapping_sub(p.m.r16(l + 0x30 + o) as i32).wrapping_add(step)
}

/// The functions sharing main+0x12e3774's code, by what differs.
pub(super) struct Tiles {
    target: usize,
    /// Static guard and type token of the shader object cast.
    guard: usize,
    cast_type: usize,
    /// Level 0's flag (+8) that enables the draw.
    level_flag: u8,
    /// The LOD record's uniform blocks.
    lod_blocks: usize,
    /// The LOD entry's record id and part scale.
    lod_id: usize,
    lod_scale: usize,
    /// The LOD records' count (the array follows).
    lod_count: usize,
    /// The cell's detail level (+0xe) below which it is not drawn.
    min_detail: u8,
    /// Records of 0x38 bytes whose part fade this game function computes (lods, LOD row, part), drawn in the layer of
    /// the record's bit 1 in units of this static table (12-byte entries, unit at +8); 0: records of 0x70 bytes with
    /// the fade inline, layer 0, 12-index units.
    lod_fade: usize,
    layer_units: usize,
    /// The level texture and the record's texture at roles 4 / 5.
    level_roles: bool,
}

pub(super) const TILES_12E3774: Tiles = Tiles {
    target: 0x12e3774,
    guard: 0x4635450,
    cast_type: 0x4635458,
    level_flag: 0x40,
    lod_blocks: 0x280,
    lod_id: 0x11,
    lod_scale: 0x13,
    lod_count: 0x80,
    min_detail: 7,
    lod_fade: 0,
    layer_units: 0,
    level_roles: false,
};
pub(super) const TILES_113AA14: Tiles = Tiles {
    target: 0x113aa14,
    guard: 0x4635410,
    cast_type: 0x4635418,
    level_flag: 0x20,
    lod_blocks: 0x300,
    lod_id: 0x10,
    lod_scale: 0x12,
    lod_count: 0x90,
    ..TILES_12E3774
};
pub(super) const TILES_1276504: Tiles = Tiles {
    target: 0x1276504,
    guard: 0x4635438,
    cast_type: 0x4635440,
    level_flag: 0x10,
    lod_blocks: 0x200,
    lod_id: 8,
    lod_scale: 0,
    lod_count: 0x70,
    min_detail: 8,
    lod_fade: 0xf2f438,
    layer_units: 0x381f1c8,
    level_roles: true,
};

/// main+0x12e3774(this, arg, k) and the functions sharing its code (`v`).
pub(super) unsafe fn draw(p: &mut Port, v: &Tiles, this: usize, arg: usize, k: u32) -> R<()> {
    if p.m.r8(p.m.r64(p.base + v.guard)) & 1 == 0 {
        p.helper("grid tiles (static init)", v.target, &[this as u64, arg as u64, k as u64])?;
        return Ok(());
    }
    let ctx = p.m.r64(arg);
    let w = p.m.r64(ctx + 0x20);
    let g = p.m.r64(this + 0x10);
    let ri = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
    let rec = (p.m.r64(g + 0x208) as i64 + ri * 0xa8) as usize;
    let s0 = p.m.r64(this + 0x18);
    let shader = if s0 == 0 { 0 } else { p.query(p.base + p.vfn(s0, 0), &[s0 as u64, p.m.r64(p.base + v.cast_type) as u64]) as usize };
    let tiles = p.m.r64(rec + 0x28);
    let lv0 = p.m.r64(p.m.r64(tiles + 0x58));
    let (lv, level) = if p.m.r32(tiles + 0x50) == 0 { (0, 0x1058) } else { (lv0, lv0 + 0x1058) };
    if p.m.r8(lv0 + 8) & v.level_flag == 0 {
        return Ok(());
    }
    let g = p.m.r64(this + 0x10);
    let si = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
    let scale = p.m.f32((p.m.r64(g + 0x208) as i64 + si * 0xa8) as usize + 0x4c);
    let inv = 1.0 / (fmax(scale, p.m.f32(p.base + MIN_SCALE)) * p.m.f32(p.base + RANGE_SCALE));
    if inv.is_nan() {
        return Ok(());
    }
    let lods = p.m.r64(rec + 0x18);
    let env_on = |p: &Port| {
        let env = p.m.r64(p.m.r64(p.base + ENV_HOLDER));
        env == 0 || p.m.r8(env + 9) & 2 == 0
    };
    if env_on(p) {
        tiled_caching(p, w, true);
    }
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    bind_slot(p, w, arg, 7, p.m.r64(rec + 0x90), 0)?;
    let mut mask = u32::MAX;
    let mut table = [0u64; 32];
    let role = |p: &Port, n: u32, off: usize| p.m.i32(p.m.r64(shader + 0x78) + if p.m.r32(shader + 0x70) > n { off } else { 0 }) as usize;
    let set = |idx: usize, v: u64, mask: &mut u32, table: &mut [u64; 32]| {
        if let Some(e) = table.get_mut(idx) {
            if *e != v {
                *e = v;
                *mask |= 1u32.wrapping_shl(idx as u32);
            }
        }
    };
    if v.level_roles {
        let tex = level_texture(p)?;
        set(role(p, 4, 0x10), tex, &mut mask, &mut table);
        set(role(p, 5, 0x14), (p.m.r64(p.m.r64(p.m.r64(rec + 0x30) + 8) + 0xb8) + 0x138) as u64, &mut mask, &mut table);
    }
    let flags = p.m.r32(p.m.r64(this + 0x10) + 8);
    if flags & 4 == 0 {
        let e = p.query(p.base + ENV_TEXTURE, &[p.m.r64(p.m.r64(p.base + ENV_HOLDER)) as u64]);
        set(role(p, 3, 0xc), e, &mut mask, &mut table);
    }
    let n = p.m.r32(level + 8);
    let mut drawn = 0u32;
    let min_dist = p.m.f32(p.base + FADE_MIN_DIST);
    let bias = p.m.f32(p.base + FADE_BIAS);
    let blocks = lv + 0x3078;
    let (mut prev_x, mut prev_y) = (0xffffu32, 0xffffu32);
    let mut last_lod = -1i32;
    let mut last_index = -1i32;
    let mut it = 0u32;
    while it < n {
        let l = p.m.r64(level);
        let h = p.m.r16(level + 0xc + it as usize * 4) as u32;
        let (x, y) = (h & 0xff, h >> 8);
        let ci = p.m.r32(l + 8).wrapping_mul(y).wrapping_add(x) as i32 as i64;
        let c = (p.m.r64(l + 0x10) as i64 + ci * 0x14) as usize;
        'cell: {
            if p.m.r8(c + 0xc) & 1 != 0 || p.m.r8(c + 0xe) < 5 {
                break 'cell;
            }
            let at = p.m.f32(c + 8);
            if at.is_nan() {
                break 'cell;
            }
            let t = if at < min_dist {
                0.0
            } else {
                let v = bias - inv * p.m.f32(p.m.r64(this + 0x10) + 0x210) / at;
                if v < 0.0 { 0.0 } else { fmin(v, 1.0) }
            };
            let fade = 1.0 - t;
            if fade <= 0.0 {
                it = n;
                break 'cell;
            }
            if fade.is_nan() {
                break 'cell;
            }
            let (cx, cy) = (p.m.r32(tiles + 0x34d0), p.m.r32(tiles + 0x34d4));
            let lodk = if cy > y && cx > x { cx.wrapping_mul(y).wrapping_add(x) as i32 as i64 } else { 0 };
            if p.m.r8(c + 0xe) < v.min_detail {
                break 'cell;
            }
            let lod_row = (p.m.r64(tiles + 0x34d8) as i64 + lodk * 0x70) as usize;
            let sh = p.m.r32(l + 0x1c) & 0x1f;
            let tsh = p.m.r32(tiles + 0x34) & 0x1f;
            let (px, py) = (cell_pos(p, l, x, 0), cell_pos(p, l, y, 2));
            let gx = ((px << sh) as i16 as i32) >> tsh;
            let gy = ((py << sh) as i16 as i32) >> tsh;
            if prev_x != gx as u32 & 0xffff || prev_y != gy as u32 & 0xffff {
                let tkey = (gx as u32 & 0xffff) | ((gy as u32) << 16);
                let tt = p.query(p.base + TILE_AT, &[tiles as u64, tkey as u64]) as usize;
                if tt == 0 || p.query(p.base + TILE_READY, &[tt as u64]) & 1 == 0 {
                    break 'cell;
                }
                let present = |p: &Port, o: usize| p.query(p.base + TEXTURE_PRESENT, &[(tt + o) as u64]) & 1 != 0;
                let mut parts = vec![(0x10, 0x90, 1, 4), (0x30, 0xb0, 2, 8)];
                if flags & 4 != 0 {
                    parts.push((0x70, 0xf0, 3, 0xc));
                }
                for (o, alt, rn, off) in parts {
                    let src = if present(p, o) { tt + o } else { tt + alt };
                    let v = p.query(p.base + TEXTURE_OF, &[src as u64]);
                    set(role(p, rn, off), v, &mut mask, &mut table);
                }
                if mask != 0 {
                    let m = mask;
                    mask = 0;
                    bind_texture_table(p, w, arg, &table, m)?;
                }
                prev_x = gx as u32 & 0xffff;
                prev_y = gy as u32 & 0xffff;
            }
            // main+0x113b324 / main+0x113b37c: the cell's position in the level, unscaled and scaled to the tiles'.
            let pos = (px as u32 & 0xffff) | ((py as u32) << 16);
            let ckey = ((px << sh) as u32 & 0xffff) | (((py << sh) as u32) << 16);
            let index = p.query(p.base + CELL_INDEX, &[tiles as u64, 0, ckey as u64]) as i32;
            let srem2 = |v: u32| (v as i16 as i32).wrapping_rem(2);
            let (a, b) = (srem2(pos), srem2(pos >> 16));
            let mut key = [(a | (b << 1)) as u32, 0, ((pos >> 1).wrapping_add(pos >> 17)) & 1];
            let cell_parts = p.m.r8(c + 0x11) as u32;
            for j in 0..4u32 {
                let bit = cell_parts & (1 << j);
                if bit == 0 && j != 0 {
                    continue;
                }
                let e = lod_row + j as usize * 0x18;
                let (id, a, layer, unit);
                if v.lod_fade != 0 {
                    id = p.m.r8(e + v.lod_id) as u32;
                    let li = if p.m.r32(lods + v.lod_count) > id { id } else { 0 } as usize;
                    let rr = p.m.r64(lods + v.lod_count + 8) + li * 0x38;
                    if p.m.r8(rr) & 1 == 0 {
                        continue;
                    }
                    a = p.query_ret_f(p.base + v.lod_fade, &[lods as u64, lod_row as u64, j as u64]);
                    layer = (p.m.r8(rr) as u32 >> 1) & 1;
                    unit = p.m.r32((p.base as i64 + v.layer_units as i64 + layer as i32 as i64 * 12) as usize + 8);
                } else {
                    if p.m.r8(e) & 0x20 != 0 {
                        continue;
                    }
                    id = p.m.r8(e + v.lod_id) as u32;
                    if id == 0xff {
                        continue;
                    }
                    let li = if p.m.r32(lods + v.lod_count) > id { id } else { 0 } as usize;
                    let rr = p.m.r64(lods + v.lod_count + 8) + li * 0x70;
                    if p.m.r8(rr + 0x64) & 1 == 0 {
                        continue;
                    }
                    let sel = p.m.r8(rr + 0x65) as u32;
                    let sel = if sel < 4 { sel as usize } else { 0 };
                    let (lo, hi) = (p.m.f32(rr + 0x68), p.m.f32(rr + 0x6c));
                    let x = lo + p.m.r8(e + sel + 0x14) as f32 / 255.0 * (hi - lo);
                    a = x * (p.m.r8(e + v.lod_scale) as f32 / 255.0);
                    (layer, unit) = (0, 12);
                }
                let f = if p.m.r8(this + 0x90) & 1 == 0 { a } else { p.m.f32(this + 0x98) };
                if f <= 0.0 {
                    continue;
                }
                if last_index != index {
                    bind_slot(p, w, arg, 8, blocks, index)?;
                }
                if last_lod != id as i32 {
                    bind_slot(p, w, arg, 9, lods + v.lod_blocks, id as i32)?;
                    last_lod = id as i32;
                }
                key[1] = j;
                drawn = drawn.wrapping_add(draw_part(p, shader, w, key, f, bit != 0, layer, unit));
                last_index = index;
            }
        }
        it += 1;
    }
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(drawn / 3));
    if env_on(p) {
        tiled_caching(p, w, false);
    }
    Ok(())
}

const CAST_GUARD2: usize = 0x46354d0;
const CAST_TYPE2: usize = 0x46354d8;
const RANGE_SCALE2: usize = 0x380f470;
const FADE_BIAS2: usize = 0x380f2a8;
const LAYER_UNITS: usize = 0x381f6d0;
const LEVEL_TEXTURE_HOLDER: usize = 0x462ada0;

/// main+0x84cd40(this, arg, k): like main+0x12e3774 without tiled caching, a fifth texture role, two LOD layers per
/// part (layer from the LOD entry's bit 2, or always 1 with this+0x90 bit 1) and per-part fades from the LOD entry.
pub(super) unsafe fn draw_layers(p: &mut Port, this: usize, arg: usize, k: u32) -> R<()> {
    if p.m.r8(p.m.r64(p.base + CAST_GUARD2)) & 1 == 0 {
        p.helper("grid layers (static init)", 0x84cd40, &[this as u64, arg as u64, k as u64])?;
        return Ok(());
    }
    let ctx = p.m.r64(arg);
    let w = p.m.r64(ctx + 0x20);
    let g = p.m.r64(this + 0x10);
    let ri = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
    let rec = (p.m.r64(g + 0x208) as i64 + ri * 0xa8) as usize;
    let s0 = p.m.r64(this + 0x18);
    let shader = if s0 == 0 { 0 } else { p.query(p.base + p.vfn(s0, 0), &[s0 as u64, p.m.r64(p.base + CAST_TYPE2) as u64]) as usize };
    let tiles = p.m.r64(rec + 0x28);
    let lv0 = p.m.r64(p.m.r64(tiles + 0x58));
    let (lv, level) = if p.m.r32(tiles + 0x50) == 0 { (0, 0x1058) } else { (lv0, lv0 + 0x1058) };
    if p.m.r8(lv0 + 8) & 2 == 0 {
        return Ok(());
    }
    let wide = p.m.f32(rec + 0x50) > 1.0;
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    bind_slot(p, w, arg, 7, p.m.r64(rec + if wide { 0x98 } else { 0x90 }), 0)?;
    bind_slot(p, w, arg, 9, this + 0x98, 0)?;
    let mut mask = u32::MAX;
    let mut table = [0u64; 32];
    let role = |p: &Port, n: u32, off: usize| p.m.i32(p.m.r64(shader + 0x78) + if p.m.r32(shader + 0x70) > n { off } else { 0 }) as usize;
    let set = |idx: usize, v: u64, mask: &mut u32, table: &mut [u64; 32]| {
        if let Some(e) = table.get_mut(idx) {
            if *e != v {
                *e = v;
                *mask |= 1u32.wrapping_shl(idx as u32);
            }
        }
    };
    let tex = level_texture(p)?;
    set(role(p, 5, 0x14), tex, &mut mask, &mut table);
    set(role(p, 6, 0x18), (p.m.r64(p.m.r64(p.m.r64(rec + 0x30) + 8) + 0xb8) + 0x138) as u64, &mut mask, &mut table);
    let t = p.m.r64(rec + 0x28);
    let t7 = if p.m.r32(t + 0x50) == 0 { 0 } else { p.m.r64(p.m.r64(t + 0x58)) };
    set(role(p, 7, 0x1c), (t7 + 0x3230) as u64, &mut mask, &mut table);
    set(role(p, 8, 0x20), (p.m.r64(p.m.r64(rec + 0x30) + 0x10) + wide as usize * 0x820 + 0x1f8) as u64, &mut mask, &mut table);
    let flags = p.m.r32(p.m.r64(this + 0x10) + 8);
    if flags & 4 == 0 {
        let v = p.query(p.base + ENV_TEXTURE, &[p.m.r64(p.m.r64(p.base + ENV_HOLDER)) as u64]);
        set(role(p, 4, 0x10), v, &mut mask, &mut table);
    }
    let g = p.m.r64(this + 0x10);
    let si = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
    let scale = p.m.f32((p.m.r64(g + 0x208) as i64 + si * 0xa8) as usize + 0x4c);
    let inv = 1.0 / (fmax(scale, p.m.f32(p.base + MIN_SCALE)) * p.m.f32(p.base + RANGE_SCALE2));
    let mut drawn = 0u32;
    let n = p.m.r32(level + 8);
    if !inv.is_nan() {
        let min_dist = p.m.f32(p.base + FADE_MIN_DIST);
        let bias = p.m.f32(p.base + FADE_BIAS2);
        let blocks = lv + 0x3078;
        let (mut prev_x, mut prev_y) = (0xffffu32, 0xffffu32);
        let mut last_index = -1i32;
        let mut it = 0u32;
        while it < n {
            let l = p.m.r64(level);
            let h = p.m.r16(level + 0xc + it as usize * 4) as u32;
            let (x, y) = (h & 0xff, h >> 8);
            let ci = p.m.r32(l + 8).wrapping_mul(y).wrapping_add(x) as i32 as i64;
            let c = (p.m.r64(l + 0x10) as i64 + ci * 0x14) as usize;
            'cell: {
                if p.m.r8(c + 0xc) & 1 != 0 || p.m.r8(c + 0xe) < 5 {
                    break 'cell;
                }
                let at = p.m.f32(c + 8);
                if at.is_nan() || p.m.r8(c + 0xc) & 0x40 != 0 {
                    break 'cell;
                }
                let t = if at < min_dist {
                    0.0
                } else {
                    let v = bias - inv * p.m.f32(p.m.r64(this + 0x10) + 0x210) / at;
                    if v < 0.0 { 0.0 } else { fmin(v, 1.0) }
                };
                let fade = 1.0 - t;
                if fade <= 0.0 {
                    it = n;
                    break 'cell;
                }
                if fade.is_nan() {
                    break 'cell;
                }
                let (cx, cy) = (p.m.r32(tiles + 0x34d0), p.m.r32(tiles + 0x34d4));
                let lodk = if cy > y && cx > x { cx.wrapping_mul(y).wrapping_add(x) as i32 as i64 } else { 0 };
                if p.m.r8(c + 0xe) < 7 {
                    break 'cell;
                }
                let lod_row = (p.m.r64(tiles + 0x34d8) as i64 + lodk * 0x70) as usize;
                let m1 = p.m.r8(lod_row + 0xa).max(p.m.r8(lod_row + 0x22));
                let m2 = p.m.r8(lod_row + 0x3a).max(p.m.r8(lod_row + 0x52));
                if m1.max(m2) == 0 {
                    break 'cell;
                }
                let all = p.m.r16(lod_row + 0x62) & p.m.r16(lod_row + 0x64) == 0xffff;
                let sh = p.m.r32(l + 0x1c) & 0x1f;
                let tsh = p.m.r32(tiles + 0x34) & 0x1f;
                let (px, py) = (cell_pos(p, l, x, 0), cell_pos(p, l, y, 2));
                let gx = ((px << sh) as i16 as i32) >> tsh;
                let gy = ((py << sh) as i16 as i32) >> tsh;
                if prev_x != gx as u32 & 0xffff || prev_y != gy as u32 & 0xffff {
                    let tkey = (gx as u32 & 0xffff) | ((gy as u32) << 16);
                    let tt = p.query(p.base + TILE_AT, &[tiles as u64, tkey as u64]) as usize;
                    if tt == 0 || p.query(p.base + TILE_READY, &[tt as u64]) & 1 == 0 {
                        break 'cell;
                    }
                    let present = |p: &Port, o: usize| p.query(p.base + TEXTURE_PRESENT, &[(tt + o) as u64]) & 1 != 0;
                    let mut parts = vec![(0x10, 0x90, 1, 4), (0x30, 0xb0, 2, 8), (0x50, 0xd0, 3, 0xc)];
                    if flags & 4 != 0 {
                        parts.push((0x70, 0xf0, 4, 0x10));
                    }
                    for (o, alt, rn, off) in parts {
                        let src = if present(p, o) { tt + o } else { tt + alt };
                        let v = p.query(p.base + TEXTURE_OF, &[src as u64]);
                        set(role(p, rn, off), v, &mut mask, &mut table);
                    }
                    if mask != 0 {
                        let m = mask;
                        mask = 0;
                        bind_texture_table(p, w, arg, &table, m)?;
                    }
                    prev_x = gx as u32 & 0xffff;
                    prev_y = gy as u32 & 0xffff;
                }
                let ckey = ((px << sh) as u32 & 0xffff) | (((py << sh) as u32) << 16);
                let index = p.query(p.base + CELL_INDEX, &[tiles as u64, 0, ckey as u64]) as i32;
                let srem2 = |v: i32| (v as i16 as i32).wrapping_rem(2);
                let mut key = [(srem2(px) | (srem2(py) << 1)) as u32, 0, ((py as u32 >> 1).wrapping_add(px as u32 >> 1)) & 1];
                let cell_parts = p.m.r8(c + 0x11) as u32;
                let fl = p.m.r32(this + 0x90);
                for j in 0..4u32 {
                    let bit = cell_parts & (1 << j);
                    if bit == 0 && j != 0 {
                        continue;
                    }
                    let e = lod_row + j as usize * 0x18;
                    let hw = p.m.r16(e) as u32;
                    if hw & 0x60 != 0 || (all && hw & 2 != 0) {
                        continue;
                    }
                    let v = if hw & 0x80 == 0 { p.m.r8(e + 0xa) as f32 / 255.0 } else { 1.0 };
                    let f = if fl & 1 != 0 { p.m.f32(this + 0x138) } else { fade * v } * f32::from_bits(0x3f4ccccd);
                    if f <= 0.0 {
                        continue;
                    }
                    let layer = if fl & 2 != 0 { 1 } else { (hw >> 2) & 1 };
                    if last_index != index {
                        bind_slot(p, w, arg, 8, blocks, index)?;
                    }
                    last_index = index;
                    key[1] = j;
                    let unit = if fade < 1.0 { 3 } else { p.m.r32((p.base as i64 + LAYER_UNITS as i64 + layer as i32 as i64 * 12) as usize + 8) };
                    drawn = drawn.wrapping_add(draw_part(p, shader, w, key, f, bit != 0, layer, unit));
                }
            }
            it += 1;
        }
    }
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(drawn / 3));
    Ok(())
}
