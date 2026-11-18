// NVN formats and blend codes in Metal terms: pixel formats, renderability, texture parameters, view compatibility.

use super::*;

pub(crate) fn nvn_blend_op(val: u32) -> MTLBlendOperation {
    match val {
        2 => MTLBlendOperation::Subtract,
        3 => MTLBlendOperation::ReverseSubtract,
        4 => MTLBlendOperation::Min,
        5 => MTLBlendOperation::Max,
        _ => MTLBlendOperation::Add,
    }
}

/// `code` is a blend-state factor field (see nvn::state::blend_func_encode).
pub(crate) fn nvn_blend_factor(code: u32) -> MTLBlendFactor {
    let val = crate::nvn::state::blend_func_decode(code as u64);
    match val {
        1 => MTLBlendFactor::Zero,
        2 => MTLBlendFactor::One,
        3 => MTLBlendFactor::SourceColor,
        4 => MTLBlendFactor::OneMinusSourceColor,
        5 => MTLBlendFactor::SourceAlpha,
        6 => MTLBlendFactor::OneMinusSourceAlpha,
        7 => MTLBlendFactor::DestinationAlpha,
        8 => MTLBlendFactor::OneMinusDestinationAlpha,
        9 => MTLBlendFactor::DestinationColor,
        10 => MTLBlendFactor::OneMinusDestinationColor,
        11 => MTLBlendFactor::SourceAlphaSaturated,
        // NVNblendFunc dual-source and constant-color values
        16 => MTLBlendFactor::Source1Color,
        17 => MTLBlendFactor::OneMinusSource1Color,
        18 => MTLBlendFactor::Source1Alpha,
        19 => MTLBlendFactor::OneMinusSource1Alpha,
        97 => MTLBlendFactor::BlendColor,
        98 => MTLBlendFactor::OneMinusBlendColor,
        99 => MTLBlendFactor::BlendAlpha,
        100 => MTLBlendFactor::OneMinusBlendAlpha,
        _ => {
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                println!("[!] Gfx: unknown NVN blend factor {} (using ONE)", val);
            }
            MTLBlendFactor::One
        }
    }
}

/// NVNformat -> Metal pixel format for render targets / sampled textures.
pub(crate) fn metal_format(fmt: i32) -> (MTLPixelFormat, Option<MTLPixelFormat>) {
    // (format, optional non-sRGB twin used to pass raw bytes through when presenting)
    match fmt {
        1 => (MTLPixelFormat::R8Unorm, None),
        2 => (MTLPixelFormat::R8Snorm, None),
        3 => (MTLPixelFormat::R8Uint, None),
        // Integer and signed-normalized formats (NVN format order). Unlisted formats fall back to RGBA8Unorm, which silently
        // reinterprets the data, so every format with a Metal equivalent must be here.
        4 => (MTLPixelFormat::R8Sint, None),
        7 => (MTLPixelFormat::R16Snorm, None),
        8 => (MTLPixelFormat::R16Uint, None),
        9 => (MTLPixelFormat::R16Sint, None),
        11 => (MTLPixelFormat::R32Uint, None),
        12 => (MTLPixelFormat::R32Sint, None),
        14 => (MTLPixelFormat::RG8Snorm, None),
        15 => (MTLPixelFormat::RG8Uint, None),
        16 => (MTLPixelFormat::RG8Sint, None),
        19 => (MTLPixelFormat::RG16Snorm, None),
        20 => (MTLPixelFormat::RG16Uint, None),
        21 => (MTLPixelFormat::RG16Sint, None),
        23 => (MTLPixelFormat::RG32Uint, None),
        24 => (MTLPixelFormat::RG32Sint, None),
        39 => (MTLPixelFormat::RGBA8Uint, None),
        40 => (MTLPixelFormat::RGBA8Sint, None),
        43 => (MTLPixelFormat::RGBA16Snorm, None),
        44 => (MTLPixelFormat::RGBA16Uint, None),
        45 => (MTLPixelFormat::RGBA16Sint, None),
        47 => (MTLPixelFormat::RGBA32Uint, None),
        48 => (MTLPixelFormat::RGBA32Sint, None),
        62 => (MTLPixelFormat::RGB10A2Uint, None),
        64 => (MTLPixelFormat::RGB9E5Float, None),
        5 => (MTLPixelFormat::R16Float, None),
        6 => (MTLPixelFormat::R16Unorm, None),
        10 => (MTLPixelFormat::R32Float, None),
        13 => (MTLPixelFormat::RG8Unorm, None),
        17 => (MTLPixelFormat::RG16Float, None),
        18 => (MTLPixelFormat::RG16Unorm, None),
        22 => (MTLPixelFormat::RG32Float, None),
        37 | 109 => (MTLPixelFormat::RGBA8Unorm, None),
        55 => (
            MTLPixelFormat::RGBA8Unorm_sRGB,
            Some(MTLPixelFormat::RGBA8Unorm),
        ), // RGBX8_SRGB
        38 => (MTLPixelFormat::RGBA8Snorm, None),
        56 => (
            MTLPixelFormat::RGBA8Unorm_sRGB,
            Some(MTLPixelFormat::RGBA8Unorm),
        ),
        41 => (MTLPixelFormat::RGBA16Float, None),
        42 => (MTLPixelFormat::RGBA16Unorm, None),
        46 => (MTLPixelFormat::RGBA32Float, None),
        49 => (MTLPixelFormat::Stencil8, None),
        50 => (MTLPixelFormat::Depth16Unorm, None),
        51 | 52 => (MTLPixelFormat::Depth32Float, None),
        53 | 54 => (MTLPixelFormat::Depth32Float_Stencil8, None), // Apple GPUs have no D24S8
        // 16-bit packed formats. They fell through to RGBA8: uploads of their 2-byte texels were rejected and the
        // textures stayed empty (Mario Kart 8 Deluxe's cup emblems and countdown, RGB565, drew black). NVN names them
        // from the low bits (red lowest); Metal has no such layouts, so sampling reorders the channels
        // (translated::packed_channels).
        57 => (MTLPixelFormat::ABGR4Unorm, None),       // RGBA4
        58 | 59 => (MTLPixelFormat::BGR5A1Unorm, None), // RGB5, RGB5A1
        60 => (MTLPixelFormat::B5G6R5Unorm, None),      // RGB565
        61 => (MTLPixelFormat::RGB10A2Unorm, None),
        63 => (MTLPixelFormat::RG11B10Float, None),
        65 | 66 => (MTLPixelFormat::BC1_RGBA, None),
        67 => (MTLPixelFormat::BC2_RGBA, None),
        68 => (MTLPixelFormat::BC3_RGBA, None),
        69 | 70 => (MTLPixelFormat::BC1_RGBA_sRGB, None),
        71 => (MTLPixelFormat::BC2_RGBA_sRGB, None),
        72 => (MTLPixelFormat::BC3_RGBA_sRGB, None),
        73 => (MTLPixelFormat::BC4_RUnorm, None),
        74 => (MTLPixelFormat::BC4_RSnorm, None),
        75 => (MTLPixelFormat::BC5_RGUnorm, None),
        76 => (MTLPixelFormat::BC5_RGSnorm, None),
        77 => (MTLPixelFormat::BC7_RGBAUnorm, None),
        78 => (MTLPixelFormat::BC7_RGBAUnorm_sRGB, None),
        79 => (MTLPixelFormat::BC6H_RGBFloat, None),
        80 => (MTLPixelFormat::BC6H_RGBUfloat, None),
        121 => (MTLPixelFormat::ASTC_4x4_LDR, None),
        122 => (MTLPixelFormat::ASTC_5x4_LDR, None),
        123 => (MTLPixelFormat::ASTC_5x5_LDR, None),
        124 => (MTLPixelFormat::ASTC_6x5_LDR, None),
        125 => (MTLPixelFormat::ASTC_6x6_LDR, None),
        126 => (MTLPixelFormat::ASTC_8x5_LDR, None),
        127 => (MTLPixelFormat::ASTC_8x6_LDR, None),
        128 => (MTLPixelFormat::ASTC_8x8_LDR, None),
        129 => (MTLPixelFormat::ASTC_10x5_LDR, None),
        130 => (MTLPixelFormat::ASTC_10x6_LDR, None),
        131 => (MTLPixelFormat::ASTC_10x8_LDR, None),
        132 => (MTLPixelFormat::ASTC_10x10_LDR, None),
        133 => (MTLPixelFormat::ASTC_12x10_LDR, None),
        134 => (MTLPixelFormat::ASTC_12x12_LDR, None),
        135 => (MTLPixelFormat::ASTC_4x4_sRGB, None),
        136 => (MTLPixelFormat::ASTC_5x4_sRGB, None),
        137 => (MTLPixelFormat::ASTC_5x5_sRGB, None),
        138 => (MTLPixelFormat::ASTC_6x5_sRGB, None),
        139 => (MTLPixelFormat::ASTC_6x6_sRGB, None),
        140 => (MTLPixelFormat::ASTC_8x5_sRGB, None),
        141 => (MTLPixelFormat::ASTC_8x6_sRGB, None),
        142 => (MTLPixelFormat::ASTC_8x8_sRGB, None),
        143 => (MTLPixelFormat::ASTC_10x5_sRGB, None),
        144 => (MTLPixelFormat::ASTC_10x6_sRGB, None),
        145 => (MTLPixelFormat::ASTC_10x8_sRGB, None),
        146 => (MTLPixelFormat::ASTC_10x10_sRGB, None),
        147 => (MTLPixelFormat::ASTC_12x10_sRGB, None),
        148 => (MTLPixelFormat::ASTC_12x12_sRGB, None),
        153 | 154 => (MTLPixelFormat::BGRA8Unorm, None),
        155 | 156 => (
            MTLPixelFormat::BGRA8Unorm_sRGB,
            Some(MTLPixelFormat::BGRA8Unorm),
        ),
        _ => (MTLPixelFormat::RGBA8Unorm, None),
    }
}

/// Formats Metal can use as a color attachment: not depth/stencil, not block-compressed.
pub(crate) fn is_color_renderable(tex: &ProtocolObject<dyn MTLTexture>) -> bool {
    let pf = tex.pixelFormat();
    let depth_stencil = matches!(
        pf,
        MTLPixelFormat::Depth16Unorm
            | MTLPixelFormat::Depth32Float
            | MTLPixelFormat::Depth32Float_Stencil8
            | MTLPixelFormat::Stencil8
    );
    tex.usage().contains(MTLTextureUsage::RenderTarget) && !depth_stencil
}

/// Snapshot of the NVNtexture at `addr` (see horizon/hle.rs `texture_snapshot`); falls back to the live struct for objects the
/// HLE never saw initialized.
pub(crate) unsafe fn tex_info(addr: usize) -> Option<(u64, NvnTexture)> {
    if addr == 0 {
        return None;
    }
    crate::nvn::api::texture_snapshot(addr)
        .or_else(|| Some((0, std::ptr::read(addr as *const NvnTexture))))
}

/// A texture's guest layout, cached by shape (it allocates, and texture resolution needs it for a texture and every
/// alias candidate each time an object is resolved: TotK re-initializes ~1,400 bound objects per frame).
pub(crate) fn layout_of(t: &NvnTexture) -> std::sync::Arc<crate::nvn::layout::TextureLayout> {
    type Key = (i32, i32, i32, i32, i32, i32, i32, i32, i64);
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<Key, std::sync::Arc<crate::nvn::layout::TextureLayout>>> = Default::default();
    }
    let p = texture_params(t);
    let key: Key = (
        p.target, p.width, p.height, p.depth, p.levels, p.format, p.flags, p.samples, p.stride,
    );
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(l) = c.get(&key) {
            return l.clone();
        }
        if c.len() > 1 << 14 {
            c.clear();
        }
        let l = std::sync::Arc::new((*crate::nvn::layout::compute_layout(&p)).clone());
        c.insert(key, l.clone());
        l
    })
}

pub(crate) fn texture_params(t: &NvnTexture) -> crate::nvn::layout::TextureParams {
    crate::nvn::layout::TextureParams {
        target: t.target,
        width: t.width,
        height: t.height,
        depth: t.depth,
        levels: t.levels,
        format: t.format,
        flags: t.flags,
        samples: t.samples,
        stride: t.stride,
    }
}

pub(crate) fn is_view_compatible(a: MTLPixelFormat, b: MTLPixelFormat) -> bool {
    match (a, b) {
        (MTLPixelFormat::RGBA8Unorm, MTLPixelFormat::RGBA8Unorm_sRGB)
        | (MTLPixelFormat::RGBA8Unorm_sRGB, MTLPixelFormat::RGBA8Unorm)
        | (MTLPixelFormat::BGRA8Unorm, MTLPixelFormat::BGRA8Unorm_sRGB)
        | (MTLPixelFormat::BGRA8Unorm_sRGB, MTLPixelFormat::BGRA8Unorm)
        | (MTLPixelFormat::BC1_RGBA, MTLPixelFormat::BC1_RGBA_sRGB)
        | (MTLPixelFormat::BC1_RGBA_sRGB, MTLPixelFormat::BC1_RGBA)
        | (MTLPixelFormat::BC3_RGBA, MTLPixelFormat::BC3_RGBA_sRGB)
        | (MTLPixelFormat::BC3_RGBA_sRGB, MTLPixelFormat::BC3_RGBA)
        | (MTLPixelFormat::BC7_RGBAUnorm, MTLPixelFormat::BC7_RGBAUnorm_sRGB)
        | (MTLPixelFormat::BC7_RGBAUnorm_sRGB, MTLPixelFormat::BC7_RGBAUnorm) => true,
        _ => a == b,
    }
}

/// --log-view-formats: prints each distinct (texture format, view format) pair that needs MTLTextureUsagePixelFormatView
/// (formats that differ by more than sRGB-ness), with the creating site.
#[track_caller]
pub(crate) fn log_view_format(from: MTLPixelFormat, to: MTLPixelFormat) {
    static SEEN: OnceLock<Mutex<std::collections::HashSet<(usize, usize)>>> = OnceLock::new();
    if from == to || !crate::settings::get().log.log_view_formats || srgb_pair(from, to) {
        return;
    }
    if SEEN
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert((from.0, to.0))
    {
        println!(
            "[view-fmt] {:?} -> {:?} at {}",
            from,
            to,
            std::panic::Location::caller()
        );
    }
}

/// Pixel formats (raw values) whose textures are created with MTLTextureUsagePixelFormatView. The usage makes Metal store
/// a texture without its lossless compression / optimized layout (a 1080p RGBA8 copy took 0.16 ms with it, 0.045 ms
/// without, on an M4), and TotK needs it for no view at all: same-format, sRGB <-> linear, slice, level and swizzle views
/// work without it. A format joins this set the first time a view needing the usage is requested from a texture of that
/// format (that one view is not made).
pub(crate) static VIEW_FLAG_FORMATS: OnceLock<Mutex<std::collections::HashSet<usize>>> = OnceLock::new();

pub(crate) fn wants_view_flag(pf: MTLPixelFormat) -> bool {
    VIEW_FLAG_FORMATS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .contains(&pf.0)
}

/// Whether `t` can have a view of format `pf`: false (with a warning, and later textures of its format get the usage)
/// when that needs MTLTextureUsagePixelFormatView and `t` was created without it.
#[track_caller]
pub(crate) fn view_format_ok(t: &ProtocolObject<dyn MTLTexture>, pf: MTLPixelFormat) -> bool {
    let from = t.pixelFormat();
    log_view_format(from, pf);
    if from == pf || srgb_pair(from, pf) || t.usage().contains(MTLTextureUsage::PixelFormatView) {
        return true;
    }
    if VIEW_FLAG_FORMATS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(from.0)
    {
        println!("[!] Gfx: a {:?} view of a {:?} texture needs PixelFormatView usage ({}): not made; new {:?} textures get the usage",
            pf, from, std::panic::Location::caller(), from);
    }
    false
}

/// Two formats that differ only in sRGB-ness (Metal creates views between them without PixelFormatView usage).
pub(crate) fn srgb_pair(a: MTLPixelFormat, b: MTLPixelFormat) -> bool {
    let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
    hi == lo + 1 && matches!(lo, 10 | 30 | 70 | 80 | 130 | 132 | 134 | 152)
}
