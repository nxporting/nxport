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

/// main+0x12e40a0 / main+0x12e4010 (layer 0, unit 12), main+0x84da58 / main+0x84dc00, main+0x1276e04 /
/// main+0x1276ec0: part `key[1]` of the cell's range in `layer` (`split`: one of four LOD sub-ranges, else the part's
/// whole range), drawn scaled by `fade`.
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
    draw_scaled(p, s, w, key, fade, first, count, unit)
}

/// main+0x18cfa74 + main+0x18cfb1c: `count` indices from `first` scaled by `fade` in whole units; the remainder unit is
/// drawn when the fraction reaches the cell's dither threshold (key / 32); a positive key[2] draws from the range's end.
#[allow(clippy::too_many_arguments)]
unsafe fn draw_scaled(p: &mut Port, s: usize, w: usize, key: [u32; 3], fade: f32, first: u32, count: u32, unit: u32) -> u32 {
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
    /// How LOD records give a part's fade, layer and units.
    lod: Lod,
    /// Units per layer (static table, 12-byte entries, unit at +8).
    layer_units: usize,
    /// The level texture and the record's texture at roles 4 / 5.
    level_roles: bool,
    /// Draws between tiled caching on and off.
    tiled: bool,
}

pub(super) enum Lod {
    /// 0x70-byte records, the fade inline, layer 0, 12-index units.
    Inline,
    /// 0x38-byte records whose part fade this game function computes (lods, LOD row, part); layer = record bit 1.
    Fade(usize),
    /// 0x58-byte records, the fade inline times 0.8; layer 1 when the record's bit 1 is clear, or forced by this+0x90
    /// bit 1 (0) / bit 2 (1).
    Scaled,
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
    lod: Lod::Inline,
    layer_units: 0,
    level_roles: false,
    tiled: true,
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
    lod: Lod::Fade(0xf2f438),
    layer_units: 0x381f1c8,
    level_roles: true,
    tiled: true,
};
pub(super) const TILES_744C7C: Tiles = Tiles {
    target: 0x744c7c,
    guard: 0x4635460,
    cast_type: 0x4635468,
    level_flag: 8,
    lod_blocks: 0x180,
    lod_id: 7,
    lod_scale: 0xc,
    lod_count: 0x60,
    min_detail: 7,
    lod: Lod::Scaled,
    layer_units: 0x381f1f8,
    level_roles: false,
    tiled: false,
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
    if v.tiled && env_on(p) {
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
                let units = |p: &Port, layer: u32| p.m.r32((p.base as i64 + v.layer_units as i64 + layer as i32 as i64 * 12) as usize + 8);
                let fl = p.m.r32(this + 0x90);
                let (id, f, layer, unit);
                match v.lod {
                    Lod::Fade(fade) => {
                        id = p.m.r8(e + v.lod_id) as u32;
                        let li = if p.m.r32(lods + v.lod_count) > id { id } else { 0 } as usize;
                        let rr = p.m.r64(lods + v.lod_count + 8) + li * 0x38;
                        if p.m.r8(rr) & 1 == 0 {
                            continue;
                        }
                        let a = p.query_ret_f(p.base + fade, &[lods as u64, lod_row as u64, j as u64]);
                        f = if fl & 1 == 0 { a } else { p.m.f32(this + 0x98) };
                        layer = (p.m.r8(rr) as u32 >> 1) & 1;
                        unit = units(p, layer);
                    }
                    Lod::Inline | Lod::Scaled => {
                        if p.m.r8(e) & 0x20 != 0 {
                            continue;
                        }
                        id = p.m.r8(e + v.lod_id) as u32;
                        let inline = matches!(v.lod, Lod::Inline);
                        if inline && id == 0xff {
                            continue;
                        }
                        let li = if p.m.r32(lods + v.lod_count) > id { id } else { 0 } as usize;
                        let (stride, at) = if inline { (0x70, 0x64) } else { (0x58, 0) };
                        let rr = p.m.r64(lods + v.lod_count + 8) + li * stride + at;
                        if p.m.r8(rr) & 1 == 0 {
                            continue;
                        }
                        let sel = p.m.r8(rr + 1) as u32;
                        let sel = if sel < 4 { sel as usize } else { 0 };
                        let (lo, hi) = (p.m.f32(rr + 4), p.m.f32(rr + 8));
                        let x = lo + p.m.r8(e + sel + 0x14) as f32 / 255.0 * (hi - lo);
                        let a = x * (p.m.r8(e + v.lod_scale) as f32 / 255.0);
                        let a = if fl & 1 == 0 { a } else { p.m.f32(this + 0x98) };
                        if inline {
                            (f, layer, unit) = (a, 0, 12);
                        } else {
                            f = a * f32::from_bits(0x3f4ccccd);
                            layer = if fl & 2 != 0 {
                                0
                            } else if fl & 4 != 0 {
                                1
                            } else {
                                !(p.m.r8(rr) as u32 >> 1) & 1
                            };
                            unit = units(p, layer);
                        }
                    }
                }
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
    if v.tiled && env_on(p) {
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

const CAST_GUARD3: usize = 0x46354e0;
const CAST_TYPE3: usize = 0x46354e8;
/// Distance window of the level (far start / end, near start / end).
const FAR_A: usize = 0x46b3b40;
const FAR_B: usize = 0x46b3b3c;
const NEAR_A: usize = 0x46b3b38;
const NEAR_B: usize = 0x46b3b34;
const MIN_WIDTH: usize = 0x380e988;
const LEVEL2_UNITS: usize = 0x381f768;

/// main+0x18ceb14(this, arg, k): the grid's level 2 inside a static distance window, per visible cell whose LOD entry
/// (tiles+0x9dc0, 0x40-byte entries) has detail and no part at 200 or more; one draw of the cell's sub-range in the
/// layer of the entry's +0x30 bit 3, between tiled caching on and off.
pub(super) unsafe fn draw_level2(p: &mut Port, this: usize, arg: usize, k: u32) -> R<()> {
    if p.m.r8(p.m.r64(p.base + CAST_GUARD3)) & 1 == 0 {
        p.helper("grid level 2 (static init)", 0x18ceb14, &[this as u64, arg as u64, k as u64])?;
        return Ok(());
    }
    let ctx = p.m.r64(arg);
    let w = p.m.r64(ctx + 0x20);
    let g = p.m.r64(this + 0x10);
    let ri = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
    let rec = (p.m.r64(g + 0x208) as i64 + ri * 0xa8) as usize;
    let s0 = p.m.r64(this + 0x18);
    let shader = if s0 == 0 { 0 } else { p.query(p.base + p.vfn(s0, 0), &[s0 as u64, p.m.r64(p.base + CAST_TYPE3) as u64]) as usize };
    let tiles = p.m.r64(rec + 0x28);
    let lv2 = p.m.r64(p.m.r64(tiles + 0x58) + 0x10);
    let (lv, level) = if p.m.r32(tiles + 0x50) > 2 { (lv2, lv2 + 0x48) } else { (0, 0x48) };
    if p.m.r8(lv2 + 8) & 2 == 0 {
        return Ok(());
    }
    let min_width = p.m.f32(p.base + MIN_WIDTH);
    let window = |a: f32, b: f32| {
        let lo = fmax(a, 0.0);
        let mut hi = if lo > b { lo } else { b };
        let mut width = hi - lo;
        if width <= 0.0 {
            width = min_width;
            hi = lo + width;
        }
        (hi, width)
    };
    let (far_hi, far_w) = window(p.m.f32(p.base + FAR_A), p.m.f32(p.base + FAR_B));
    let ratio = far_hi / far_w;
    let (near_hi, near_w) = window(p.m.f32(p.base + NEAR_A), p.m.f32(p.base + NEAR_B));
    if ratio.is_nan() {
        return Ok(());
    }
    let g = p.m.r64(this + 0x10);
    let si = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
    let scale = fmax(p.m.f32((p.m.r64(g + 0x208) as i64 + si * 0xa8) as usize + 0x4c), p.m.f32(p.base + MIN_SCALE));
    let inv = 1.0 / (scale * far_w);
    if inv.is_nan() || (near_hi / near_w).is_nan() || (1.0 / (scale * near_w)).is_nan() {
        return Ok(());
    }
    let env_on = |p: &Port| {
        let env = p.m.r64(p.m.r64(p.base + ENV_HOLDER));
        env == 0 || p.m.r8(env + 9) & 2 == 0
    };
    if env_on(p) {
        tiled_caching(p, w, true);
    }
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    bind_slot(p, w, arg, 7, p.m.r64(rec + 0x98), 0)?;
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
    let t7 = if p.m.r32(t + 0x50) >= 3 { p.m.r64(p.m.r64(t + 0x58) + 0x10) } else { 0 };
    set(role(p, 7, 0x1c), (t7 + 0x3230) as u64, &mut mask, &mut table);
    set(role(p, 8, 0x20), (p.m.r64(p.m.r64(rec + 0x30) + 0x10) + 0xa18) as u64, &mut mask, &mut table);
    let flags = p.m.r32(p.m.r64(this + 0x10) + 8);
    if flags & 4 == 0 {
        let e = p.query(p.base + ENV_TEXTURE, &[p.m.r64(p.m.r64(p.base + ENV_HOLDER)) as u64]);
        set(role(p, 3, 0xc), e, &mut mask, &mut table);
    }
    let n = p.m.r32(level + 8);
    let mut drawn = 0u32;
    let min_dist = p.m.f32(p.base + FADE_MIN_DIST);
    let ids = tiles + 0x9dc0;
    let blocks = lv + 0x3078;
    let (mut prev_x, mut prev_y) = (0xffffu32, 0xffffu32);
    let mut last_index = -1i32;
    let fade_of = |p: &Port, at: f32| {
        if at < min_dist {
            0.0
        } else {
            let v = ratio - inv * p.m.f32(p.m.r64(this + 0x10) + 0x210) / at;
            if v < 0.0 { 0.0 } else { fmin(v, 1.0) }
        }
    };
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
            let fade = 1.0 - fade_of(p, at);
            if fade <= 0.0 {
                it = n;
                break 'cell;
            }
            if fade.is_nan() {
                break 'cell;
            }
            let (cx, cy) = (p.m.r32(ids), p.m.r32(ids + 4));
            let lodk = if cy > y && cx > x { cx.wrapping_mul(y).wrapping_add(x) as i32 as i64 } else { 0 };
            if p.m.r8(c + 0xe) < 6 {
                break 'cell;
            }
            let ent = (p.m.r64(ids + 8) as i64 + lodk * 0x40) as usize;
            if [0, 0xc, 0x18, 0x24, 0x30].iter().any(|&o| p.m.r8(ent + o) & 4 != 0) {
                break 'cell;
            }
            let m1 = p.m.r8(ent + 2).max(p.m.r8(ent + 0xe));
            let m2 = p.m.r8(ent + 0x1a).max(p.m.r8(ent + 0x26));
            if m1.max(p.m.r8(ent + 0x32)).max(m2) == 0 {
                break 'cell;
            }
            if [4, 0x10, 0x1c, 0x28, 0x34].iter().all(|&o| p.m.r8(ent + o) >= 200) {
                break 'cell;
            }
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
                let mut parts = vec![(0x10, 0x90, 1, 4), (0x30, 0xb0, 2, 8), (0x50, 0xd0, 4, 0x10)];
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
            let srem2 = |v: i32| (v as i16 as i32).wrapping_rem(2);
            let key = [(srem2(px) | (srem2(py) << 1)) as u32, 0, ((py as u32 >> 1).wrapping_add(px as u32 >> 1)) & 1];
            let t = fade_of(p, p.m.f32(c + 8));
            let f = if p.m.r8(this + 0x90) & 1 != 0 { p.m.f32(this + 0x98) } else { 1.0 - t };
            if f <= 0.0 {
                break 'cell;
            }
            let ckey = ((px << sh) as u32 & 0xffff) | (((py << sh) as u32) << 16);
            let index = p.query(p.base + CELL_INDEX, &[tiles as u64, 2, ckey as u64]) as i32;
            let layer = (p.m.r8(ent + 0x30) as u32 >> 3) & 1;
            if last_index != index {
                bind_slot(p, w, arg, 8, blocks, index)?;
                last_index = index;
            }
            // main+0x18cf714: the cell's sub-range key[0] of the layer (0x40 bytes per layer).
            let e = shader + layer as usize * 0x40 + if key[0] < 4 { key[0] } else { 0 } as usize * 0x10;
            let unit = p.m.r32((p.base as i64 + LEVEL2_UNITS as i64 + layer as i32 as i64 * 12) as usize + 8);
            let (first, count) = (p.m.r32(e + 0x4c8), p.m.r32(e + 0x4cc));
            drawn = drawn.wrapping_add(draw_scaled(p, shader, w, key, f, first, count, unit));
        }
        it += 1;
    }
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(drawn / 3));
    if env_on(p) {
        tiled_caching(p, w, false);
    }
    Ok(())
}

const CAST_GUARD4: usize = 0x46354a0;
const CAST_TYPE4: usize = 0x46354a8;
/// Two static levels (0x28-byte entries: LOD +0xc, near window +0x18 / +0x1c, fade scale +0x20).
const LEVELS2: usize = 0x4371bf0;

/// main+0x18cbb18(this, arg, k): the grid's level 0 cells for each of two static levels inside the level's distance
/// window, faded by distance; per cell its tile's textures, its block and one draw of the level's LOD range for the
/// cell (main+0x18cc7a0), between tiled caching on and off.
pub(super) unsafe fn draw_static_levels(p: &mut Port, this: usize, arg: usize, k: u32) -> R<()> {
    if p.m.r8(p.m.r64(p.base + CAST_GUARD4)) & 1 == 0 {
        p.helper("grid static levels (static init)", 0x18cbb18, &[this as u64, arg as u64, k as u64])?;
        return Ok(());
    }
    let t98 = p.m.r64(this + 0x98);
    let ctx = p.m.r64(arg);
    let w = p.m.r64(ctx + 0x20);
    let g = p.m.r64(this + 0x10);
    let ri = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
    let ti = if p.m.r32(this + 0x90) > k { k } else { 0 } as i32 as i64;
    let rec = (p.m.r64(g + 0x208) as i64 + ri * 0xa8) as usize;
    let s0 = p.m.r64(this + 0x18);
    let shader = if s0 == 0 { 0 } else { p.query(p.base + p.vfn(s0, 0), &[s0 as u64, p.m.r64(p.base + CAST_TYPE4) as u64]) as usize };
    let tiles = p.m.r64(rec + 0x28);
    if p.m.r8(p.m.r64(p.m.r64(tiles + 0x58)) + 8) & 1 == 0 {
        return Ok(());
    }
    let env_on = |p: &Port| {
        let env = p.m.r64(p.m.r64(p.base + ENV_HOLDER));
        env == 0 || p.m.r8(env + 9) & 2 == 0
    };
    if env_on(p) {
        tiled_caching(p, w, true);
    }
    let wide = p.m.f32(rec + 0x50) > 1.0;
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    bind_slot(p, w, arg, 7, p.m.r64(rec + if wide { 0x98 } else { 0x90 }), 0)?;
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
        let e = p.query(p.base + ENV_TEXTURE, &[p.m.r64(p.m.r64(p.base + ENV_HOLDER)) as u64]);
        set(role(p, 4, 0x10), e, &mut mask, &mut table);
    }
    let min_scale = p.m.f32(p.base + MIN_SCALE);
    let min_width = p.m.f32(p.base + MIN_WIDTH);
    let mut drawn = 0u32;
    for e in 0..2usize {
        let lv = p.base + LEVELS2 + e * 0x28;
        let (lv0, list) = if p.m.r32(tiles + 0x50) == 0 {
            (0, 0x48)
        } else {
            let l = p.m.r64(p.m.r64(tiles + 0x58));
            (l, l + 0x48)
        };
        let a = p.m.f32(lv + 0x1c);
        let g = p.m.r64(this + 0x10);
        let si = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
        let scale = fmax(p.m.f32((p.m.r64(g + 0x208) as i64 + si * 0xa8) as usize + 0x4c), min_scale);
        let reach = p.m.f32(g + 0x210) / (a * scale);
        let lo = fmax(a, 0.0);
        let b = p.m.f32(lv + 0x18);
        let mut hi = if lo > b { lo } else { b };
        let mut width = hi - lo;
        if width <= 0.0 {
            width = min_width;
            hi = lo + width;
        }
        let ratio = hi / width;
        let inv = 1.0 / (scale * width);
        if ratio.is_nan() || inv.is_nan() {
            continue;
        }
        let cell_size = p.m.f32(p.m.r64(rec + 0x28) + 8);
        let cells_i = p.m.i32(p.m.r64(list) + 0x20);
        let tb = (t98 as i64 + ti * 0x10) as usize;
        let ei = if (e as u64) < p.m.r32(tb) as u64 { e } else { 0 };
        bind_slot(p, w, arg, 9, p.m.r64(tb + 8) + ei * 0x80, 0)?;
        let n = p.m.r32(list + 8);
        let end = cell_size * cells_i as f32 + reach;
        let blocks = lv0 + 0x3078;
        for it in 0..n as usize {
            let l = p.m.r64(list);
            let h = p.m.r16(list + 0xc + it * 4) as u32;
            let (x, y) = (h & 0xff, h >> 8);
            let ci = p.m.r32(l + 8).wrapping_mul(y).wrapping_add(x) as i32 as i64;
            let c = (p.m.r64(l + 0x10) as i64 + ci * 0x14) as usize;
            let cb = p.m.r8(c + 0xc);
            if cb & 1 != 0 || p.m.r8(c + 0xe) < 5 {
                continue;
            }
            let at = p.m.f32(c + 8);
            if end < at {
                break;
            }
            if cb & 0x10 == 0 {
                continue;
            }
            let t = if at < p.m.f32(p.base + FADE_MIN_DIST) {
                0.0
            } else {
                let v = ratio - inv * p.m.f32(p.m.r64(this + 0x10) + 0x210) / at;
                if v < 0.0 { 0.0 } else { fmin(v, 1.0) }
            };
            let f = p.m.f32(lv + 0x20) * (1.0 - t);
            if f <= 0.0 {
                continue;
            }
            let sh = p.m.r32(l + 0x1c) & 0x1f;
            let tsh = p.m.r32(tiles + 0x34) & 0x1f;
            let (px, py) = (cell_pos(p, l, x, 0), cell_pos(p, l, y, 2));
            let gx = ((px << sh) as i16 as i32) >> tsh;
            let gy = ((py << sh) as i16 as i32) >> tsh;
            let tkey = (gx as u32 & 0xffff) | ((gy as u32) << 16);
            let tt = p.query(p.base + TILE_AT, &[tiles as u64, tkey as u64]) as usize;
            if tt == 0 || p.query(p.base + TILE_READY, &[tt as u64]) & 1 == 0 {
                continue;
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
            let ckey = ((px << sh) as u32 & 0xffff) | (((py << sh) as u32) << 16);
            let index = p.query(p.base + CELL_INDEX, &[tiles as u64, 0, ckey as u64]) as i32;
            let srem2 = |v: i32| (v as i16 as i32).wrapping_rem(2);
            let key = [(srem2(px) | (srem2(py) << 1)) as u32, 0, ((py as u32 >> 1).wrapping_add(px as u32 >> 1)) & 1];
            bind_slot(p, w, arg, 8, blocks, index)?;
            // main+0x18cc7a0: the shader's range for (key[0], LOD), in triangles.
            let lod = p.m.r32(lv + 0xc);
            let at_ = |rows: usize, cols: usize| {
                if p.m.r32(shader + rows) > key[0] && p.m.r32(shader + cols) > lod {
                    p.m.r32(shader + cols).wrapping_mul(key[0]).wrapping_add(lod) as i32 as i64
                } else {
                    0
                }
            };
            let (ci, fi) = (at_(0x4d4, 0x4d0), at_(0x4c4, 0x4c0));
            let count = p.m.r32((p.m.r64(shader + 0x4d8) as i64 + ci * 4) as usize);
            let first = p.m.r32((p.m.r64(shader + 0x4c8) as i64 + fi * 4) as usize);
            drawn = drawn.wrapping_add(draw_scaled(p, shader, w, key, f, first, count, 3));
        }
    }
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(drawn / 3));
    if env_on(p) {
        tiled_caching(p, w, false);
    }
    Ok(())
}

/// The functions sharing main+0x18ca004's code, by what differs.
pub(super) struct Records {
    target: usize,
    guard: usize,
    cast_type: usize,
    /// Offset of the two sub-records (0x1a4 apart) in a 0x350-byte LOD record.
    sub: usize,
    /// Block 9's index within the record's six: (low sub-record, high sub-record).
    block: (u32, u32),
    /// Units per layer (12-byte entries, unit at +8).
    units: usize,
}

pub(super) const RECORDS_18CA004: Records =
    Records { target: 0x18ca004, guard: 0x4635490, cast_type: 0x4635498, sub: 0x88, block: (3, 0), units: 0x381f350 };
pub(super) const RECORDS_18C8560: Records =
    Records { target: 0x18c8560, guard: 0x4635480, cast_type: 0x4635488, sub: 0x114, block: (4, 1), units: 0x381f2f8 };
pub(super) const RECORDS_CF48B0: Records =
    Records { target: 0xcf48b0, guard: 0x4635470, cast_type: 0x4635478, sub: 0x1a0, block: (5, 2), units: 0x381f2a0 };

/// main+0x18ca004(this, arg, k): like main+0x84cd40 with LOD records of 0x350 bytes holding two sub-records (the
/// entry's +0xb at most 0x40 picks the first), block 9 per record and sub-record, and the part draws inline.
pub(super) unsafe fn draw_records(p: &mut Port, v: &Records, this: usize, arg: usize, k: u32) -> R<()> {
    if p.m.r8(p.m.r64(p.base + v.guard)) & 1 == 0 {
        p.helper("grid records (static init)", v.target, &[this as u64, arg as u64, k as u64])?;
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
    if p.m.r8(lv0 + 8) & 4 == 0 {
        return Ok(());
    }
    let g = p.m.r64(this + 0x10);
    let si = if p.m.r32(g + 0x200) > k { k } else { 0 } as i32 as i64;
    let scale = p.m.f32((p.m.r64(g + 0x208) as i64 + si * 0xa8) as usize + 0x4c);
    let inv = 1.0 / (fmax(scale, p.m.f32(p.base + MIN_SCALE)) * p.m.f32(p.base + RANGE_SCALE2));
    if inv.is_nan() {
        return Ok(());
    }
    let lods = p.m.r64(rec + 0x18);
    let wide = p.m.f32(rec + 0x50) > 1.0;
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    bind_slot(p, w, arg, 7, p.m.r64(rec + if wide { 0x98 } else { 0x90 }), 0)?;
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
        let e = p.query(p.base + ENV_TEXTURE, &[p.m.r64(p.m.r64(p.base + ENV_HOLDER)) as u64]);
        set(role(p, 4, 0x10), e, &mut mask, &mut table);
    }
    let n = p.m.r32(level + 8);
    let mut drawn = 0u32;
    let min_dist = p.m.f32(p.base + FADE_MIN_DIST);
    let bias = p.m.f32(p.base + FADE_BIAS2);
    let blocks = lv + 0x3078;
    let (mut prev_x, mut prev_y) = (0xffffu32, 0xffffu32);
    let mut last_index = -1i32;
    let mut last_block = -1i32;
    // The entry's record: (record, sub-record) when the record is live and the sub-record enabled.
    let record = |p: &Port, e: usize| -> Option<(u32, usize)> {
        let id = p.m.r8(e + 5) as u32;
        let li = if p.m.r32(lods + 0x50) > id { id } else { 0 } as usize;
        let r = p.m.r64(lods + 0x58) + li * 0x350;
        let o = p.m.r64(r);
        if o == 0 || p.m.r64(o + 0x150) == 0 {
            return None;
        }
        let sub = r + (p.m.r8(e + 0xb) < 0x41) as usize * 0x1a4 + v.sub;
        (p.m.r8(sub) & 1 != 0).then_some((id, sub))
    };
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
                let Some((id, sub)) = record(p, e) else { continue };
                let a = if p.m.r8(e) & 0x60 != 0 {
                    0.0
                } else {
                    let sel = p.m.r8(sub + 1) as u32;
                    let sel = if sel < 4 { sel as usize } else { 0 };
                    let (lo, hi) = (p.m.f32(sub + 4), p.m.f32(sub + 8));
                    let x = lo + p.m.r8(e + sel + 0x14) as f32 / 255.0 * (hi - lo);
                    x * (p.m.r8(e + 9) as f32 / 255.0) * f32::from_bits(0x3f4ccccd)
                };
                let f = if fl & 1 != 0 { p.m.f32(this + 0x98) } else { fade * a };
                if f <= 0.0 {
                    continue;
                }
                let layer = if fl & 2 != 0 {
                    0
                } else if fl & 4 != 0 {
                    1
                } else {
                    !(p.m.r8(sub) as u32 >> 1) & 1
                };
                if last_index != index {
                    bind_slot(p, w, arg, 8, blocks, index)?;
                }
                let b = (id * 6 + if p.m.r8(e + 0xb) < 0x41 { v.block.0 } else { v.block.1 }) as i32;
                if last_block != b {
                    bind_slot(p, w, arg, 9, lods + 0x100, b)?;
                    last_block = b;
                }
                key[1] = j;
                let unit = if fade >= 1.0 { p.m.r32((p.base as i64 + v.units as i64 + layer as i32 as i64 * 12) as usize + 8) } else { 3 };
                drawn = drawn.wrapping_add(draw_part(p, shader, w, key, f, bit != 0, layer, unit));
                last_index = index;
            }
        }
        it += 1;
    }
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(drawn / 3));
    Ok(())
}
