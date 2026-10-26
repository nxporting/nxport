// Tegra X1 block-linear texture layout, as used by NVN (nvnTextureBuilderGetStorageSize,
// nvnTextureBuilderGetViewOffset, nvnTextureGetViewOffset).
//
// The game uploads pre-swizzled BNTX/txtg data straight into texture storage at offsets it gets
// from NVN, so these must match the toolchain's layout exactly. The rules below were validated
// against all 216 BNTX files in TexToGo (mip offsets and image sizes, including array and 3D
// textures); see the test vectors at the bottom.
//
// Layout summary (per mip level, in 4x4-block / pixel units of the format):
//   pitch  = align(widthInBlocks * bytesPerBlock, 64)           (a GOB is 64B x 8 rows = 512B)
//   rows   = align(heightInBlocks, 8 * blockHeightGobs)
//   size   = pitch * rows * align(depth, blockDepthGobs)
// Levels are packed back to back in one layer; array/cube layers are each a unit whose stride
// is the level chain aligned to a GOB block of the base level.

pub const NVN_TARGET_1D: i32 = 0;
pub const NVN_TARGET_2D: i32 = 1;
pub const NVN_TARGET_3D: i32 = 2;
pub const NVN_TARGET_1D_ARRAY: i32 = 3;
pub const NVN_TARGET_2D_ARRAY: i32 = 4;
pub const NVN_TARGET_2D_MULTISAMPLE: i32 = 5;
pub const NVN_TARGET_2D_MULTISAMPLE_ARRAY: i32 = 6;
pub const NVN_TARGET_RECTANGLE: i32 = 7;
pub const NVN_TARGET_CUBEMAP: i32 = 8;
pub const NVN_TARGET_CUBEMAP_ARRAY: i32 = 9;
pub const NVN_TARGET_BUFFER: i32 = 10;

const NVN_TEXTURE_FLAGS_COMPRESSIBLE_BIT: i32 = 0x08;
const NVN_TEXTURE_FLAGS_LINEAR_BIT: i32 = 0x10;
const NVN_TEXTURE_FLAGS_LINEAR_RENDER_TARGET_BIT: i32 = 0x40;
const NVN_TEXTURE_FLAGS_MINIMAL_LAYOUT_BIT: i32 = 0x200;

/// Texture storage must be at least this aligned / padded.
pub const TEXTURE_STORAGE_ALIGNMENT: usize = 0x1000;
/// Alignment for textures with the compressible bit (extra hardware mappings): 64 KB, as the real NVN driver places
/// TotK's compressible render targets (Ryujinx render-target addresses: 64 KB steps, e.g. 0x2147b0000). 128 KB wasted
/// enough of the game's 111 MB dynamic texture pool that UltraCam's 1080p targets no longer fit (null G-buffer).
pub const COMPRESSIBLE_STORAGE_ALIGNMENT: usize = 0x10000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatInfo {
    pub block_w: u32,
    pub block_h: u32,
    pub bytes: u32,
}

const fn fi(block_w: u32, block_h: u32, bytes: u32) -> Option<FormatInfo> {
    Some(FormatInfo {
        block_w,
        block_h,
        bytes,
    })
}

/// Block size and bytes per block for an NVNformat value.
pub fn format_info(fmt: i32) -> Option<FormatInfo> {
    match fmt {
        1..=4 | 49 | 81 | 82 => fi(1, 1, 1),
        5..=9 | 13..=16 | 50 | 57..=60 | 83 | 84 | 87 | 88 | 149..=152 => fi(1, 1, 2),
        25..=28 | 93 | 94 => fi(1, 1, 3),
        10..=12
        | 17..=21
        | 37..=40
        | 51..=53
        | 55
        | 56
        | 61..=64
        | 85
        | 86
        | 89
        | 90
        | 99
        | 100
        | 105..=112
        | 153..=156 => fi(1, 1, 4),
        29..=33 | 95 | 96 => fi(1, 1, 6),
        22..=24 | 41..=45 | 54 | 91 | 92 | 101 | 102 | 113..=117 => fi(1, 1, 8),
        34..=36 | 97 | 98 => fi(1, 1, 12),
        46..=48 | 103 | 104 | 118..=120 => fi(1, 1, 16),
        // 65/66 RGB(A)_DXT1, 67 DXT3, 68 DXT5, 69/70 DXT1_SRGB, 71 DXT3_SRGB, 72 DXT5_SRGB, 73/74 RGTC1,
        // 75/76 RGTC2, 77/78 BPTC (BC7), 79/80 BPTC float (BC6H).
        65 | 66 | 69 | 70 | 73 | 74 => fi(4, 4, 8), // BC1 (DXT1), BC4 (RGTC1)
        67 | 68 | 71 | 72 | 75..=80 => fi(4, 4, 16), // BC2 (DXT3), BC3 (DXT5), BC5 (RGTC2), BC7, BC6H
        121..=148 => {
            // ASTC: 14 block sizes, the sRGB variants repeat them.
            const DIMS: [(u32, u32); 14] = [
                (4, 4),
                (5, 4),
                (5, 5),
                (6, 5),
                (6, 6),
                (8, 5),
                (8, 6),
                (8, 8),
                (10, 5),
                (10, 6),
                (10, 8),
                (10, 10),
                (12, 10),
                (12, 12),
            ];
            let (w, h) = DIMS[((fmt - 121) % 14) as usize];
            fi(w, h, 16)
        }
        _ => None,
    }
}

#[derive(Clone, Debug, Default)]
pub struct TextureLayout {
    pub level_offsets: Vec<u64>,
    pub level_sizes: Vec<u64>,
    /// Byte distance between consecutive array layers / cube faces.
    pub layer_stride: u64,
    pub layers: u32,
    /// Bytes of texture data (before storage padding).
    pub total: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TextureParams {
    pub target: i32,
    pub width: i32,
    pub height: i32,
    pub depth: i32,
    pub levels: i32,
    pub format: i32,
    pub flags: i32,
    pub samples: i32,
    /// Row stride for linear textures (0 = tightly packed, 32-byte aligned).
    pub stride: i64,
}

fn align(v: u64, a: u64) -> u64 {
    (v + a - 1) / a * a
}

/// Block height in GOBs (8 rows each) chosen by the toolchain for a level of `hb` block rows.
fn block_height_gobs(hb: u64) -> u64 {
    let mut bh = 1;
    while bh < 16 && hb * 3 > 32 * bh {
        bh *= 2;
    }
    bh
}

/// Level-0 block height in GOBs of a 2D texture. MINIMAL_LAYOUT ("prefer the smallest tile size") takes one GOB.
/// Ground truth is the size only: TotK packs its 320x320 R16 Depths terrain maps (minimal layout) 0x32000 bytes apart,
/// the size with a block height of at most 8 GOBs; the usual 16 made each 0x3c000 bytes and its texel writes overran
/// the next map. Heights 1 and 8 render the same there (nothing reads those maps raw); which one the driver picks is
/// unverified.
fn level0_bh(hb0: u64, flags: i32) -> u64 {
    if flags & NVN_TEXTURE_FLAGS_MINIMAL_LAYOUT_BIT != 0 {
        1
    } else {
        block_height_gobs(hb0)
    }
}

fn block_depth_gobs(d: u64) -> u64 {
    let mut bd = 16;
    while bd > 1 && d <= bd / 2 {
        bd /= 2;
    }
    bd
}

/// (block height, block depth) in GOBs for a 3D texture level: blocks hold at most 16 GOBs, so the height shrinks as
/// the depth grows. Ground truth: the texture packager's layout descriptor for TotK's 64x64x64 BC4 volume says
/// height 1, depth 16 (descriptor byte 0x40), and the 256x32x16 RGBA16F atmosphere table (no descriptor: the driver's
/// default) is only smooth deswizzled with height 1, depth 16. The 2D rule alone (height 4 there) scrambled 3D
/// textures, which turned the sky an olive grey.
fn block_dims_3d(hb: u64, d: u64) -> (u64, u64) {
    let bd = block_depth_gobs(d);
    let mut bh = block_height_gobs(hb);
    while bh > 1 && bh * bd > 16 {
        bh /= 2;
    }
    (bh, bd)
}

/// (block height, block depth) in GOBs of every level: the texture's block shape is chosen for level 0 and the hardware
/// derives each level's from it (Ryujinx SizeCalculator): the height halves while the level's block rows fit in half
/// a block, the depth halves at most once per level. Recomputing the level-0 rule per level matched only for
/// power-of-two sizes: a 160x160 BC3 (40 block rows, 4 GOBs) has 4 / 2 GOBs at levels 1 / 2, not 2 / 1, and its
/// scrambled mips put dark streaks and a black rim on the shrine travel-gate disc.
fn level_block_dims(
    hb0: u64,
    d0: u64,
    is_3d: bool,
    levels: usize,
    block_h: u64,
    flags: i32,
) -> Vec<(u64, u64)> {
    let (mut bh, mut bd) = if is_3d {
        block_dims_3d(hb0, d0)
    } else {
        (level0_bh(hb0, flags), 1)
    };
    let h0 = hb0 * block_h;
    (0..levels)
        .map(|l| {
            let hb = ((h0 >> l).max(1) + block_h - 1) / block_h;
            while bh > 1 && hb <= (bh / 2) * 8 {
                bh /= 2;
            }
            if is_3d && l > 0 && (d0 >> l).max(1) <= bd / 2 && bd > 1 {
                bd /= 2;
            }
            (bh, bd)
        })
        .collect()
}

/// Block height in GOBs of one level of a 2D (or array / cube) texture (see level_block_dims).
fn level_bh(p: &TextureParams, info: &FormatInfo, level: usize) -> usize {
    let hb0 = (p.height.max(1) as u64 + info.block_h as u64 - 1) / info.block_h as u64;
    level_block_dims(hb0, 1, false, level + 1, info.block_h as u64, p.flags)[level].0 as usize
}

pub fn sample_dims(samples: i32) -> (u32, u32) {
    match samples {
        2 => (2, 1),
        4 => (2, 2),
        8 => (4, 2),
        16 => (4, 4),
        _ => (1, 1),
    }
}

pub fn compute_layout(p: &TextureParams) -> TextureLayout {
    let info = format_info(p.format).unwrap_or(FormatInfo {
        block_w: 1,
        block_h: 1,
        bytes: 4,
    });
    let (sx, sy) = sample_dims(p.samples);
    let w = (p.width.max(1) as u64) * sx as u64;
    let mut h = (p.height.max(1) as u64) * sy as u64;
    let mut d = p.depth.max(1) as u64;
    let mut levels = p.levels.max(1) as usize;

    let (layers, is_3d) = match p.target {
        NVN_TARGET_3D => (1u64, true),
        NVN_TARGET_1D_ARRAY => {
            h = 1;
            let l = d;
            d = 1;
            (l, false)
        }
        NVN_TARGET_2D_ARRAY | NVN_TARGET_2D_MULTISAMPLE_ARRAY | NVN_TARGET_CUBEMAP_ARRAY => {
            let l = d;
            d = 1;
            (l, false)
        }
        NVN_TARGET_CUBEMAP => {
            d = 1;
            (6, false)
        }
        NVN_TARGET_1D => {
            h = 1;
            d = 1;
            (1, false)
        }
        _ => {
            d = 1;
            (1, false)
        }
    };
    if p.target == NVN_TARGET_RECTANGLE || p.target == NVN_TARGET_BUFFER {
        levels = 1;
    }

    // Buffer textures: a flat run of texels.
    if p.target == NVN_TARGET_BUFFER {
        let size = w * info.bytes as u64;
        return TextureLayout {
            level_offsets: vec![0],
            level_sizes: vec![size],
            layer_stride: size,
            layers: 1,
            total: size,
        };
    }

    // Linear textures: a single level with an explicit (or 32-byte aligned) pitch.
    if p.flags & (NVN_TEXTURE_FLAGS_LINEAR_BIT | NVN_TEXTURE_FLAGS_LINEAR_RENDER_TARGET_BIT) != 0 {
        let wb = (w + info.block_w as u64 - 1) / info.block_w as u64;
        let hb = (h + info.block_h as u64 - 1) / info.block_h as u64;
        let pitch = if p.stride > 0 {
            p.stride as u64
        } else {
            align(wb * info.bytes as u64, 32)
        };
        let size = pitch * hb;
        return TextureLayout {
            level_offsets: vec![0],
            level_sizes: vec![size],
            layer_stride: size,
            layers: layers as u32,
            total: size * layers,
        };
    }

    let mut offsets = Vec::with_capacity(levels);
    let mut sizes = Vec::with_capacity(levels);
    let mut off = 0u64;
    let dims = level_block_dims(
        (h + info.block_h as u64 - 1) / info.block_h as u64,
        d,
        is_3d,
        levels,
        info.block_h as u64,
        p.flags,
    );
    for l in 0..levels {
        let lw = (w >> l).max(1);
        let lh = (h >> l).max(1);
        let ld = if is_3d { (d >> l).max(1) } else { 1 };
        let wb = (lw + info.block_w as u64 - 1) / info.block_w as u64;
        let hb = (lh + info.block_h as u64 - 1) / info.block_h as u64;
        let pitch = align(wb * info.bytes as u64, 64);
        let (gh, gd) = dims[l];
        let size = pitch * align(hb, 8 * gh) * align(ld, gd);
        offsets.push(off);
        sizes.push(size);
        off += size;
    }

    let mut layer_stride = off;
    if layers > 1 {
        let hb0 = (h + info.block_h as u64 - 1) / info.block_h as u64;
        layer_stride = align(off, 512 * level0_bh(hb0, p.flags));
    }
    TextureLayout {
        level_offsets: offsets,
        level_sizes: sizes,
        layer_stride,
        layers: layers as u32,
        total: layer_stride * layers,
    }
}

/// Bytes NVN reports for a texture's storage (padded to the storage granule).
pub fn storage_size(p: &TextureParams) -> usize {
    let t = compute_layout(p).total as usize;
    align(t.max(1) as u64, TEXTURE_STORAGE_ALIGNMENT as u64) as usize
}

pub fn storage_alignment(flags: i32) -> usize {
    if flags & NVN_TEXTURE_FLAGS_COMPRESSIBLE_BIT != 0 {
        COMPRESSIBLE_STORAGE_ALIGNMENT
    } else {
        TEXTURE_STORAGE_ALIGNMENT
    }
}

/// Offset of the first byte of a view (min level / min layer) from the start of storage.
pub fn view_offset(layout: &TextureLayout, min_level: u32, min_layer: u32) -> u64 {
    let lvl = layout
        .level_offsets
        .get(min_level as usize)
        .copied()
        .unwrap_or(0);
    lvl + min_layer as u64 * layout.layer_stride
}

/// Byte offset of (x_bytes, y) inside a block-linear surface of GOBs (64B x 8 rows, 512B each),
/// grouped into blocks of `block_h_gobs` GOBs vertically. `pitch` is the row pitch in bytes
/// (a multiple of 64). Within a GOB the 512 bytes follow the Tegra X1 swizzle.
pub fn block_linear_offset(x_bytes: usize, y: usize, pitch: usize, block_h_gobs: usize) -> usize {
    let gob_x = x_bytes / 64;
    let gob_y = y / 8;
    let block_y = gob_y / block_h_gobs;
    let gob_in_block = gob_y % block_h_gobs;
    let gobs_per_row = pitch / 64;
    let gob_index = (block_y * gobs_per_row + gob_x) * block_h_gobs + gob_in_block;
    let xx = x_bytes % 64;
    let yy = y % 8;
    let in_gob =
        (xx / 32) * 256 + (yy / 2) * 64 + ((xx % 32) / 16) * 32 + (yy % 2) * 16 + (xx % 16);
    gob_index * 512 + in_gob
}

/// Copy linear `data` (tightly packed rows of `width` texels) into one 2D slice of tiled storage.
/// `region` is in texels; compressed formats address 4x4 (or ASTC-sized) blocks. `slice` is the
/// start of the level/layer within storage. Returns false for layouts it can't address (3D with a
/// block depth > 1).
///
/// # Safety
/// `storage` must be valid for writes across the level's size.
/// Linear textures (NVN_TEXTURE_FLAGS_LINEAR / LINEAR_RENDER_TARGET): one level of rows `pitch` bytes apart, the pitch
/// compute_layout uses. None for block-linear textures.
fn linear_pitch(p: &TextureParams, info: &FormatInfo) -> Option<usize> {
    if p.flags & (NVN_TEXTURE_FLAGS_LINEAR_BIT | NVN_TEXTURE_FLAGS_LINEAR_RENDER_TARGET_BIT) == 0 {
        return None;
    }
    let wb = (p.width.max(1) as u64 + info.block_w as u64 - 1) / info.block_w as u64;
    Some(if p.stride > 0 { p.stride as usize } else { align(wb * info.bytes as u64, 32) as usize })
}

pub unsafe fn write_texels_2d(
    storage: *mut u8,
    slice: usize,
    p: &TextureParams,
    level: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    data: *const u8,
) -> bool {
    let info = format_info(p.format).unwrap_or(FormatInfo {
        block_w: 1,
        block_h: 1,
        bytes: 4,
    });
    let layout = compute_layout(p);
    if level >= layout.level_offsets.len() {
        return false;
    }
    let lw = ((p.width.max(1) as u64) >> level).max(1);
    let wb = (lw + info.block_w as u64 - 1) / info.block_w as u64;
    let pitch = align(wb * info.bytes as u64, 64) as usize;
    let bh = level_bh(p, &info, level);
    let base = storage.add(slice + layout.level_offsets[level] as usize);
    let (bx, by) = (
        (x.max(0) as u32 / info.block_w) as usize,
        (y.max(0) as u32 / info.block_h) as usize,
    );
    let cols = ((w.max(0) as u32 + info.block_w - 1) / info.block_w) as usize;
    let rows = ((h.max(0) as u32 + info.block_h - 1) / info.block_h) as usize;
    let bpb = info.bytes as usize;
    let row_bytes = cols * bpb;
    let x0 = bx * bpb;
    if let Some(pitch) = linear_pitch(p, &info) {
        if level != 0 {
            return false;
        }
        for r in 0..rows {
            std::ptr::copy_nonoverlapping(data.add(r * row_bytes), base.add((by + r) * pitch + x0), row_bytes);
        }
        return true;
    }
    // Aligned 16-byte runs within a GOB row are contiguous: copy runs (see read_texels_2d).
    for r in 0..rows {
        let src_row = data.add(r * row_bytes);
        let mut x = 0usize;
        while x < row_bytes {
            let xb = x0 + x;
            let n = (16 - xb % 16).min(row_bytes - x);
            let off = block_linear_offset(xb, by + r, pitch, bh);
            std::ptr::copy_nonoverlapping(src_row.add(x), base.add(off), n);
            x += n;
        }
    }
    true
}

/// Byte offset of (x_bytes, y, z) in a 3D block-linear surface (Ryujinx BlockLinearLayout): blocks of 1 x `bh` x `bd`
/// GOBs, ordered x, then y, then z; inside a block the GOBs of one slice are stacked in y, slices follow in z.
fn block_linear_offset_3d(
    x_bytes: usize,
    y: usize,
    z: usize,
    pitch: usize,
    rows_aligned: usize,
    bh: usize,
    bd: usize,
) -> usize {
    let gobs_per_row = pitch / 64;
    let blocks_y = rows_aligned / (8 * bh);
    let block_size = 512 * bh * bd;
    let rob = block_size * gobs_per_row; // one row of blocks
    let slice = rob * blocks_y; // one layer of blocks (bd slices deep)
    let yh = y / 8;
    let xx = x_bytes % 64;
    let yy = y % 8;
    let in_gob =
        (xx / 32) * 256 + (yy / 2) * 64 + ((xx % 32) / 16) * 32 + (yy % 2) * 16 + (xx % 16);
    (yh / bh) * rob
        + (z / bd) * slice
        + (x_bytes / 64) * block_size
        + (yh % bh) * 512
        + (z % bd) * 512 * bh
        + in_gob
}

/// Read a whole level of a 3D block-linear texture into `out` (tightly packed rows, then slices).
/// # Safety
/// `storage` must be valid for reads across the texture's layout; `out` must hold the level's linear size.
pub unsafe fn read_texels_3d(
    storage: *const u8,
    p: &TextureParams,
    level: usize,
    out: *mut u8,
) -> bool {
    let info = format_info(p.format).unwrap_or(FormatInfo {
        block_w: 1,
        block_h: 1,
        bytes: 4,
    });
    let layout = compute_layout(p);
    if level >= layout.level_offsets.len() {
        return false;
    }
    let lw = ((p.width.max(1) as u64) >> level).max(1);
    let lh = ((p.height.max(1) as u64) >> level).max(1);
    let ld = ((p.depth.max(1) as u64) >> level).max(1) as usize;
    let wb = ((lw + info.block_w as u64 - 1) / info.block_w as u64) as usize;
    let hb = (lh + info.block_h as u64 - 1) / info.block_h as u64;
    let bpb = info.bytes as usize;
    let pitch = align(wb as u64 * bpb as u64, 64) as usize;
    let hb0 = (p.height.max(1) as u64 + info.block_h as u64 - 1) / info.block_h as u64;
    let (bh, bd) = level_block_dims(
        hb0,
        p.depth.max(1) as u64,
        true,
        level + 1,
        info.block_h as u64,
        p.flags,
    )[level];
    let (bh, bd) = (bh as usize, bd as usize);
    let rows_aligned = align(hb, 8 * bh as u64) as usize;
    let base = storage.add(layout.level_offsets[level] as usize);
    let row_bytes = wb * bpb;
    for z in 0..ld {
        for r in 0..hb as usize {
            let dst = out.add((z * hb as usize + r) * row_bytes);
            for xb in 0..row_bytes {
                *dst.add(xb) = *base.add(block_linear_offset_3d(
                    xb,
                    r,
                    z,
                    pitch,
                    rows_aligned,
                    bh,
                    bd,
                ));
            }
        }
    }
    true
}

/// Write depth slice `z` of a level of a 3D block-linear texture from `data` (tightly packed rows).
/// # Safety
/// `storage` must be valid for writes across the texture's layout; `data` must hold one slice of the level.
pub unsafe fn write_texels_3d_slice(
    storage: *mut u8,
    p: &TextureParams,
    level: usize,
    z: usize,
    data: *const u8,
) -> bool {
    let info = format_info(p.format).unwrap_or(FormatInfo {
        block_w: 1,
        block_h: 1,
        bytes: 4,
    });
    let layout = compute_layout(p);
    if level >= layout.level_offsets.len() || z >= ((p.depth.max(1) as usize) >> level).max(1) {
        return false;
    }
    let lw = ((p.width.max(1) as u64) >> level).max(1);
    let lh = ((p.height.max(1) as u64) >> level).max(1);
    let wb = ((lw + info.block_w as u64 - 1) / info.block_w as u64) as usize;
    let hb = (lh + info.block_h as u64 - 1) / info.block_h as u64;
    let bpb = info.bytes as usize;
    let pitch = align(wb as u64 * bpb as u64, 64) as usize;
    let hb0 = (p.height.max(1) as u64 + info.block_h as u64 - 1) / info.block_h as u64;
    let (bh, bd) = level_block_dims(hb0, p.depth.max(1) as u64, true, level + 1, info.block_h as u64, p.flags)[level];
    let (bh, bd) = (bh as usize, bd as usize);
    let rows_aligned = align(hb, 8 * bh as u64) as usize;
    let base = storage.add(layout.level_offsets[level] as usize);
    let row_bytes = wb * bpb;
    for r in 0..hb as usize {
        let src = data.add(r * row_bytes);
        for xb in 0..row_bytes {
            *base.add(block_linear_offset_3d(xb, r, z, pitch, rows_aligned, bh, bd)) = *src.add(xb);
        }
    }
    true
}

/// Read block-linear tiled `storage` into linear `out_data` (tightly packed rows).
/// # Safety
/// `storage` must be valid for reads across the level's size, and `out_data` must have space for w*h.
pub unsafe fn read_texels_2d(
    storage: *const u8,
    slice: usize,
    p: &TextureParams,
    level: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    out_data: *mut u8,
) -> bool {
    let info = format_info(p.format).unwrap_or(FormatInfo {
        block_w: 1,
        block_h: 1,
        bytes: 4,
    });
    let layout = compute_layout(p);
    if level >= layout.level_offsets.len() {
        return false;
    }
    let lw = ((p.width.max(1) as u64) >> level).max(1);
    let wb = (lw + info.block_w as u64 - 1) / info.block_w as u64;
    let pitch = align(wb * info.bytes as u64, 64) as usize;
    let bh = level_bh(p, &info, level);
    let base = storage.add(slice + layout.level_offsets[level] as usize);
    let (bx, by) = (
        (x.max(0) as u32 / info.block_w) as usize,
        (y.max(0) as u32 / info.block_h) as usize,
    );
    let cols = ((w.max(0) as u32 + info.block_w - 1) / info.block_w) as usize;
    let rows = ((h.max(0) as u32 + info.block_h - 1) / info.block_h) as usize;
    let bpb = info.bytes as usize;
    let row_bytes = cols * bpb;
    let x0 = bx * bpb;
    if let Some(pitch) = linear_pitch(p, &info) {
        if level != 0 {
            return false;
        }
        for r in 0..rows {
            std::ptr::copy_nonoverlapping(base.add((by + r) * pitch + x0), out_data.add(r * row_bytes), row_bytes);
        }
        return true;
    }
    // Within a GOB row, every aligned 16-byte run is contiguous: copy runs, not bytes (one address computation per
    // 16 bytes; the per-byte loop made streamed-texture uploads a visible share of the encoding thread).
    for r in 0..rows {
        let dst_row = out_data.add(r * row_bytes);
        let mut x = 0usize;
        while x < row_bytes {
            let xb = x0 + x;
            let n = (16 - xb % 16).min(row_bytes - x);
            let off = block_linear_offset(xb, by + r, pitch, bh);
            std::ptr::copy_nonoverlapping(base.add(off), dst_row.add(x), n);
            x += n;
        }
    }
    true
}

/// The per-byte reference implementation of read_texels_2d's copy (tests only).
#[cfg(test)]
mod block_dims_tests {
    #[test]
    fn volume_blocks_hold_16_gobs() {
        // 256x32x16 RGBA16F (32 block rows, depth 16) and 64x64x64 BC4 (16 block rows, depth 64): height 1, depth 16.
        assert_eq!(super::block_dims_3d(32, 16), (1, 16));
        assert_eq!(super::block_dims_3d(16, 64), (1, 16));
        // Thin volumes keep the 2D height.
        assert_eq!(
            super::block_dims_3d(32, 1),
            (super::block_height_gobs(32), 1)
        );
        assert_eq!(super::block_dims_3d(128, 2), (8, 2));
    }
}

#[cfg(test)]
unsafe fn read_texels_2d_reference(
    storage: *const u8,
    slice: usize,
    p: &TextureParams,
    level: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    out_data: *mut u8,
) {
    let info = format_info(p.format).unwrap_or(FormatInfo {
        block_w: 1,
        block_h: 1,
        bytes: 4,
    });
    let layout = compute_layout(p);
    let lw = ((p.width.max(1) as u64) >> level).max(1);
    let wb = (lw + info.block_w as u64 - 1) / info.block_w as u64;
    let pitch = align(wb * info.bytes as u64, 64) as usize;
    let bh = level_bh(p, &info, level);
    let base = storage.add(slice + layout.level_offsets[level] as usize);
    let (bx, by) = (
        (x.max(0) as u32 / info.block_w) as usize,
        (y.max(0) as u32 / info.block_h) as usize,
    );
    let cols = ((w.max(0) as u32 + info.block_w - 1) / info.block_w) as usize;
    let rows = ((h.max(0) as u32 + info.block_h - 1) / info.block_h) as usize;
    let bpb = info.bytes as usize;
    for r in 0..rows {
        for c in 0..cols {
            let dst = out_data.add((r * cols + c) * bpb);
            for b in 0..bpb {
                *dst.add(b) = *base.add(block_linear_offset((bx + c) * bpb + b, by + r, pitch, bh));
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn read_texels_2d_matches_reference() {
        let mut seed = 0x9E37_79B9u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        // (format, width, height, levels): R8, RG8, RGBA8, RGBA16F, RGBA32F, BC1, BC3
        for &(fmt, w, h, levels) in &[
            (1, 37, 19, 3),
            (13, 260, 260, 2),
            (37, 68, 68, 4),
            (41, 100, 33, 1),
            (46, 9, 70, 2),
            (66, 128, 64, 5),
            (70, 260, 130, 3),
        ] {
            let p = TextureParams {
                target: NVN_TARGET_2D,
                width: w,
                height: h,
                depth: 1,
                levels,
                format: fmt,
                flags: 0,
                samples: 1,
                stride: 0,
            };
            let layout = compute_layout(&p);
            let mem: Vec<u8> = (0..layout.total as usize + 64)
                .map(|_| rnd() as u8)
                .collect();
            for level in 0..levels as usize {
                let (lw, lh) = (((w >> level).max(1)), ((h >> level).max(1)));
                for &(x, y, rw, rh) in &[
                    (0, 0, lw, lh),
                    (lw / 3, lh / 4, lw - lw / 3, lh - lh / 4),
                    (1, 1, 1, 1),
                ] {
                    let n = (rw as usize + 3) * (rh as usize + 3) * 16;
                    let (mut a, mut b) = (vec![0u8; n], vec![0u8; n]);
                    unsafe {
                        read_texels_2d(mem.as_ptr(), 0, &p, level, x, y, rw, rh, a.as_mut_ptr());
                        read_texels_2d_reference(
                            mem.as_ptr(),
                            0,
                            &p,
                            level,
                            x,
                            y,
                            rw,
                            rh,
                            b.as_mut_ptr(),
                        );
                    }
                    assert_eq!(
                        a,
                        b,
                        "fmt {} {}x{} level {} region {:?}",
                        fmt,
                        w,
                        h,
                        level,
                        (x, y, rw, rh)
                    );
                }
            }
        }
    }

    use super::*;

    // (w, h, d, mips, array layers, block w, block h, bytes per block, expected level offsets, expected total)
    // Taken from BNTX headers in the game's TexToGo directory (Nintendo toolchain output).
    #[allow(clippy::type_complexity)]
    const BNTX_VECTORS: &[(i32, i32, i32, i32, i32, u32, u32, u32, &[u64], u64)] = &[
        (
            64,
            64,
            64,
            7,
            1,
            4,
            4,
            8,
            &[0, 131072, 147456, 155648, 159744, 161792, 162816],
            163328,
        ),
        (7, 1, 1, 1, 12, 1, 1, 1, &[0], 6144),
        (
            512,
            512,
            1,
            10,
            12,
            4,
            4,
            8,
            &[
                0, 131072, 163840, 172032, 174080, 174592, 175104, 175616, 176128, 176640,
            ],
            2162688,
        ),
        (
            128,
            128,
            1,
            8,
            14,
            4,
            4,
            16,
            &[0, 16384, 20480, 21504, 22016, 22528, 23040, 23552],
            344064,
        ),
        (1, 1, 1, 1, 1, 1, 1, 1, &[0], 512),
        (32, 64, 1, 1, 1, 1, 1, 8, &[0], 16384),
        (32, 180, 1, 1, 1, 1, 1, 8, &[0], 65536),
        (19, 3096, 1, 1, 1, 1, 1, 8, &[0], 614400),
        (
            2048,
            2048,
            1,
            12,
            1,
            4,
            4,
            8,
            &[
                0, 2097152, 2621440, 2752512, 2785280, 2793472, 2795520, 2796032, 2796544, 2797056,
                2797568, 2798080,
            ],
            2798592,
        ),
        (30, 1, 1, 1, 1, 1, 1, 8, &[0], 2048),
        (100, 1, 1, 1, 1, 1, 1, 8, &[0], 6656),
        (8, 13, 1, 1, 1, 1, 1, 1, &[0], 1024),
        (120, 1, 1, 1, 1, 1, 1, 8, &[0], 7680),
    ];

    fn fmt_for(bw: u32, bh: u32, bytes: u32) -> i32 {
        // Pick an NVNformat with the wanted block shape.
        match (bw, bh, bytes) {
            (1, 1, 1) => 1,   // R8
            (1, 1, 2) => 13,  // RG8
            (1, 1, 4) => 37,  // RGBA8
            (1, 1, 8) => 41,  // RGBA16F
            (1, 1, 16) => 46, // RGBA32F
            (4, 4, 8) => 65,  // RGB_DXT1
            (4, 4, 16) => 68, // RGBA_DXT5
            _ => panic!("no test format"),
        }
    }

    #[test]
    fn matches_bntx_toolchain_layout() {
        for &(w, h, d, mips, arr, bw, bh, bytes, offs, total) in BNTX_VECTORS {
            let (target, depth) = if d > 1 {
                (NVN_TARGET_3D, d)
            } else if arr > 1 {
                (NVN_TARGET_2D_ARRAY, arr)
            } else {
                (NVN_TARGET_2D, 1)
            };
            let l = compute_layout(&TextureParams {
                target,
                width: w,
                height: h,
                depth,
                levels: mips,
                format: fmt_for(bw, bh, bytes),
                flags: 0,
                samples: 1,
                stride: 0,
            });
            assert_eq!(
                l.level_offsets, offs,
                "level offsets for {w}x{h}x{d} mips={mips} arr={arr}"
            );
            assert_eq!(
                l.total, total,
                "total for {w}x{h}x{d} mips={mips} arr={arr}"
            );
        }
    }

    /// Non-power-of-two mip chains: level offsets from Ryujinx's SizeCalculator (level block heights derived from
    /// level 0's), which the per-level rule got wrong.
    #[test]
    fn npot_mip_chains_follow_the_hardware_rule() {
        let cases: &[(i32, i32, i32, i32, &[u64], u64)] = &[
            (
                160,
                160,
                8,
                68,
                &[0, 40960, 51200, 54272, 55296, 55808, 56320, 56832],
                57344,
            ),
            (
                320,
                180,
                9,
                37,
                &[
                    0, 327680, 409600, 430080, 436224, 438272, 438784, 439296, 439808,
                ],
                440320,
            ),
            (
                96,
                96,
                7,
                65,
                &[0, 6144, 8192, 8704, 9216, 9728, 10240],
                10752,
            ),
        ];
        for &(w, h, levels, format, offs, total) in cases {
            let l = compute_layout(&TextureParams {
                target: NVN_TARGET_2D,
                width: w,
                height: h,
                depth: 1,
                levels,
                format,
                flags: 0,
                samples: 1,
                stride: 0,
            });
            assert_eq!(l.level_offsets, offs, "{w}x{h} format {format}");
            assert_eq!(l.total, total, "{w}x{h} format {format}");
        }
    }

    #[test]
    fn storage_is_padded_and_never_smaller() {
        let p = TextureParams {
            target: NVN_TARGET_2D,
            width: 256,
            height: 256,
            depth: 1,
            levels: 9,
            format: 65,
            flags: 0,
            samples: 1,
            stride: 0,
        };
        let s = storage_size(&p);
        assert_eq!(s % TEXTURE_STORAGE_ALIGNMENT, 0);
        assert!(s as u64 >= compute_layout(&p).total);
    }

    #[test]
    fn view_offsets_follow_levels_and_layers() {
        let l = compute_layout(&TextureParams {
            target: NVN_TARGET_2D_ARRAY,
            width: 64,
            height: 64,
            depth: 4,
            levels: 3,
            format: 37,
            flags: 0,
            samples: 1,
            stride: 0,
        });
        assert_eq!(view_offset(&l, 0, 0), 0);
        assert_eq!(view_offset(&l, 1, 0), l.level_offsets[1]);
        assert_eq!(view_offset(&l, 0, 2), 2 * l.layer_stride);
        assert_eq!(
            view_offset(&l, 2, 3),
            l.level_offsets[2] + 3 * l.layer_stride
        );
    }

    #[test]
    fn block_linear_gob_swizzle() {
        // Tegra X1 GOB: 64B x 8 rows in 16B-wide stripes; offsets from the published layout.
        assert_eq!(block_linear_offset(0, 0, 64, 1), 0);
        assert_eq!(block_linear_offset(1, 0, 64, 1), 1);
        assert_eq!(block_linear_offset(0, 1, 64, 1), 16);
        assert_eq!(block_linear_offset(0, 2, 64, 1), 64);
        assert_eq!(block_linear_offset(16, 0, 64, 1), 32);
        assert_eq!(block_linear_offset(32, 0, 64, 1), 256);
        assert_eq!(block_linear_offset(63, 7, 64, 1), 511);
        // Next GOB to the right, then next GOB row (block height 2 stacks them vertically).
        assert_eq!(block_linear_offset(64, 0, 128, 2), 2 * 512);
        assert_eq!(block_linear_offset(0, 8, 128, 2), 512);
        assert_eq!(block_linear_offset(0, 16, 128, 2), 4 * 512);
    }

    #[test]
    fn swizzle_is_a_bijection_within_a_surface() {
        let (pitch, rows, bh) = (128usize, 32usize, 2usize);
        let mut seen = vec![false; pitch * rows];
        for y in 0..rows {
            for x in 0..pitch {
                let o = block_linear_offset(x, y, pitch, bh);
                assert!(o < pitch * rows && !seen[o], "collision at ({x},{y})");
                seen[o] = true;
            }
        }
    }

    #[test]
    fn linear_render_target_texels_are_rows() {
        // R16 (format 6) linear render target with an explicit stride: rows `stride` bytes apart, no swizzle.
        let p = TextureParams {
            target: NVN_TARGET_2D,
            width: 256,
            height: 256,
            depth: 1,
            levels: 1,
            format: 6,
            flags: NVN_TEXTURE_FLAGS_LINEAR_RENDER_TARGET_BIT,
            samples: 1,
            stride: 512,
        };
        let mut mem = vec![0u8; storage_size(&p)];
        let data: Vec<u8> = (0..16 * 4 * 2).map(|i| i as u8).collect();
        let mut back = vec![0u8; data.len()];
        unsafe {
            assert!(write_texels_2d(mem.as_mut_ptr(), 0, &p, 0, 8, 3, 16, 4, data.as_ptr()));
            assert!(read_texels_2d(mem.as_ptr(), 0, &p, 0, 8, 3, 16, 4, back.as_mut_ptr()));
        }
        assert_eq!(back, data);
        for r in 0..4 {
            assert_eq!(&mem[(3 + r) * 512 + 16..(3 + r) * 512 + 48], &data[r * 32..r * 32 + 32]);
        }
    }

    #[test]
    fn write_texels_lands_at_swizzled_offsets() {
        let p = TextureParams {
            target: NVN_TARGET_2D,
            width: 64,
            height: 16,
            depth: 1,
            levels: 1,
            format: 37,
            flags: 0,
            samples: 1,
            stride: 0,
        };
        let size = storage_size(&p);
        let mut mem = vec![0u8; size];
        let data: Vec<u8> = (0..64 * 16 * 4).map(|i| (i % 251) as u8).collect();
        unsafe {
            assert!(write_texels_2d(
                mem.as_mut_ptr(),
                0,
                &p,
                0,
                0,
                0,
                64,
                16,
                data.as_ptr()
            ));
        }
        let l = compute_layout(&p);
        let pitch = 256;
        let bh = 2usize; // 16 rows -> 2 GOB rows
        for &(x, y) in &[(0usize, 0usize), (5, 3), (63, 15), (10, 9)] {
            let o = block_linear_offset(x * 4, y, pitch, bh);
            for b in 0..4 {
                assert_eq!(
                    mem[l.level_offsets[0] as usize + o + b],
                    data[(y * 64 + x) * 4 + b],
                    "texel ({x},{y}) byte {b}"
                );
            }
        }
    }

    #[test]
    fn real_formats_have_sane_sizes() {
        assert_eq!(
            format_info(37),
            Some(FormatInfo {
                block_w: 1,
                block_h: 1,
                bytes: 4
            })
        ); // RGBA8
        assert_eq!(
            format_info(68),
            Some(FormatInfo {
                block_w: 4,
                block_h: 4,
                bytes: 16
            })
        ); // DXT5
        assert_eq!(
            format_info(121),
            Some(FormatInfo {
                block_w: 4,
                block_h: 4,
                bytes: 16
            })
        ); // ASTC 4x4
        assert_eq!(
            format_info(148),
            Some(FormatInfo {
                block_w: 12,
                block_h: 12,
                bytes: 16
            })
        ); // ASTC 12x12 sRGB
        assert_eq!(format_info(0), None);
    }

    #[test]
    fn roundtrip_write_and_read_texels() {
        let p = TextureParams {
            target: NVN_TARGET_2D,
            width: 64,
            height: 32,
            depth: 1,
            levels: 1,
            format: 37,
            flags: 0,
            samples: 1,
            stride: 0,
        };
        let size = storage_size(&p);
        let mut mem = vec![0u8; size];
        let original: Vec<u8> = (0..64 * 32 * 4)
            .map(|i| ((i * 37 + 13) % 256) as u8)
            .collect();
        let mut readback = vec![0u8; 64 * 32 * 4];
        unsafe {
            assert!(write_texels_2d(
                mem.as_mut_ptr(),
                0,
                &p,
                0,
                0,
                0,
                64,
                32,
                original.as_ptr()
            ));
            assert!(read_texels_2d(
                mem.as_ptr(),
                0,
                &p,
                0,
                0,
                0,
                64,
                32,
                readback.as_mut_ptr()
            ));
        }
        assert_eq!(original, readback);
    }
}
