// NVN formats as Vulkan formats.

use ash::vk;

/// What a color format's shader values are: float (normalized and float formats), unsigned or signed integer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Float,
    Uint,
    Sint,
}

/// NVNformat -> the Vulkan format with the same texel encoding, for textures and render targets.
pub fn texture(fmt: i32) -> Option<vk::Format> {
    use vk::Format as F;
    Some(match fmt {
        1 => F::R8_UNORM,
        2 => F::R8_SNORM,
        3 => F::R8_UINT,
        4 => F::R8_SINT,
        5 => F::R16_SFLOAT,
        6 => F::R16_UNORM,
        7 => F::R16_SNORM,
        8 => F::R16_UINT,
        9 => F::R16_SINT,
        10 => F::R32_SFLOAT,
        11 => F::R32_UINT,
        12 => F::R32_SINT,
        13 => F::R8G8_UNORM,
        14 => F::R8G8_SNORM,
        15 => F::R8G8_UINT,
        16 => F::R8G8_SINT,
        17 => F::R16G16_SFLOAT,
        18 => F::R16G16_UNORM,
        19 => F::R16G16_SNORM,
        20 => F::R16G16_UINT,
        21 => F::R16G16_SINT,
        22 => F::R32G32_SFLOAT,
        23 => F::R32G32_UINT,
        24 => F::R32G32_SINT,
        37 | 109 => F::R8G8B8A8_UNORM,
        38 => F::R8G8B8A8_SNORM,
        39 => F::R8G8B8A8_UINT,
        40 => F::R8G8B8A8_SINT,
        41 => F::R16G16B16A16_SFLOAT,
        42 => F::R16G16B16A16_UNORM,
        43 => F::R16G16B16A16_SNORM,
        44 => F::R16G16B16A16_UINT,
        45 => F::R16G16B16A16_SINT,
        46 => F::R32G32B32A32_SFLOAT,
        47 => F::R32G32B32A32_UINT,
        48 => F::R32G32B32A32_SINT,
        49 => F::S8_UINT,
        50 => F::D16_UNORM,
        51 => F::X8_D24_UNORM_PACK32,
        52 => F::D32_SFLOAT,
        53 => F::D24_UNORM_S8_UINT,
        54 => F::D32_SFLOAT_S8_UINT,
        55 | 56 => F::R8G8B8A8_SRGB,
        // Packed 16-bit formats name their channels from the low bits (red lowest).
        57 => F::A4B4G4R4_UNORM_PACK16,
        60 => F::B5G6R5_UNORM_PACK16,
        61 => F::A2B10G10R10_UNORM_PACK32,
        62 => F::A2B10G10R10_UINT_PACK32,
        63 => F::B10G11R11_UFLOAT_PACK32,
        64 => F::E5B9G9R9_UFLOAT_PACK32,
        65 | 66 => F::BC1_RGBA_UNORM_BLOCK,
        67 => F::BC2_UNORM_BLOCK,
        68 => F::BC3_UNORM_BLOCK,
        69 | 70 => F::BC1_RGBA_SRGB_BLOCK,
        71 => F::BC2_SRGB_BLOCK,
        72 => F::BC3_SRGB_BLOCK,
        73 => F::BC4_UNORM_BLOCK,
        74 => F::BC4_SNORM_BLOCK,
        75 => F::BC5_UNORM_BLOCK,
        76 => F::BC5_SNORM_BLOCK,
        77 => F::BC7_UNORM_BLOCK,
        78 => F::BC7_SRGB_BLOCK,
        79 => F::BC6H_SFLOAT_BLOCK,
        80 => F::BC6H_UFLOAT_BLOCK,
        121 => F::ASTC_4X4_UNORM_BLOCK,
        122 => F::ASTC_5X4_UNORM_BLOCK,
        123 => F::ASTC_5X5_UNORM_BLOCK,
        124 => F::ASTC_6X5_UNORM_BLOCK,
        125 => F::ASTC_6X6_UNORM_BLOCK,
        126 => F::ASTC_8X5_UNORM_BLOCK,
        127 => F::ASTC_8X6_UNORM_BLOCK,
        128 => F::ASTC_8X8_UNORM_BLOCK,
        129 => F::ASTC_10X5_UNORM_BLOCK,
        130 => F::ASTC_10X6_UNORM_BLOCK,
        131 => F::ASTC_10X8_UNORM_BLOCK,
        132 => F::ASTC_10X10_UNORM_BLOCK,
        133 => F::ASTC_12X10_UNORM_BLOCK,
        134 => F::ASTC_12X12_UNORM_BLOCK,
        135 => F::ASTC_4X4_SRGB_BLOCK,
        136 => F::ASTC_5X4_SRGB_BLOCK,
        137 => F::ASTC_5X5_SRGB_BLOCK,
        138 => F::ASTC_6X5_SRGB_BLOCK,
        139 => F::ASTC_6X6_SRGB_BLOCK,
        140 => F::ASTC_8X5_SRGB_BLOCK,
        141 => F::ASTC_8X6_SRGB_BLOCK,
        142 => F::ASTC_8X8_SRGB_BLOCK,
        143 => F::ASTC_10X5_SRGB_BLOCK,
        144 => F::ASTC_10X6_SRGB_BLOCK,
        145 => F::ASTC_10X8_SRGB_BLOCK,
        146 => F::ASTC_10X10_SRGB_BLOCK,
        147 => F::ASTC_12X10_SRGB_BLOCK,
        148 => F::ASTC_12X12_SRGB_BLOCK,
        153 | 154 => F::B8G8R8A8_UNORM,
        155 | 156 => F::B8G8R8A8_SRGB,
        _ => return None,
    })
}

pub fn kind(fmt: i32) -> Kind {
    match fmt {
        3 | 8 | 11 | 15 | 20 | 23 | 39 | 44 | 47 | 62 => Kind::Uint,
        4 | 9 | 12 | 16 | 21 | 24 | 40 | 45 | 48 => Kind::Sint,
        _ => Kind::Float,
    }
}

pub fn is_depth(fmt: vk::Format) -> bool {
    matches!(
        fmt,
        vk::Format::D16_UNORM
            | vk::Format::X8_D24_UNORM_PACK32
            | vk::Format::D32_SFLOAT
            | vk::Format::D24_UNORM_S8_UINT
            | vk::Format::D32_SFLOAT_S8_UINT
            | vk::Format::S8_UINT
    )
}

pub fn aspects(fmt: vk::Format) -> vk::ImageAspectFlags {
    match fmt {
        vk::Format::D16_UNORM | vk::Format::X8_D24_UNORM_PACK32 | vk::Format::D32_SFLOAT => vk::ImageAspectFlags::DEPTH,
        vk::Format::S8_UINT => vk::ImageAspectFlags::STENCIL,
        vk::Format::D24_UNORM_S8_UINT | vk::Format::D32_SFLOAT_S8_UINT => {
            vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
        }
        _ => vk::ImageAspectFlags::COLOR,
    }
}

/// Bytes one element of a vertex format occupies.
pub fn vertex_size(f: vk::Format) -> u32 {
    use vk::Format as F;
    match f {
        F::R8_UNORM | F::R8_SNORM | F::R8_UINT | F::R8_SINT => 1,
        F::R16_SFLOAT | F::R16_UNORM | F::R16_SNORM | F::R16_UINT | F::R16_SINT => 2,
        F::R8G8_UNORM | F::R8G8_SNORM | F::R8G8_UINT | F::R8G8_SINT => 2,
        F::R8G8B8_UNORM | F::R8G8B8_SNORM | F::R8G8B8_UINT | F::R8G8B8_SINT => 3,
        F::R16G16B16_SFLOAT | F::R16G16B16_UNORM | F::R16G16B16_SNORM | F::R16G16B16_UINT | F::R16G16B16_SINT => 6,
        F::R32G32_SFLOAT | F::R32G32_UINT | F::R32G32_SINT => 8,
        F::R16G16B16A16_SFLOAT | F::R16G16B16A16_UNORM | F::R16G16B16A16_SNORM | F::R16G16B16A16_UINT | F::R16G16B16A16_SINT => 8,
        F::R32G32B32_SFLOAT | F::R32G32B32_UINT | F::R32G32B32_SINT => 12,
        F::R32G32B32A32_SFLOAT | F::R32G32B32A32_UINT | F::R32G32B32A32_SINT => 16,
        _ => 4,
    }
}

/// An NVN vertex attribute format: the Vulkan format the fetch uses and how the shader reads it.
pub fn vertex(fmt: u32) -> Option<(vk::Format, sass2spv::types::AttributeType)> {
    use sass2spv::types::AttributeType as A;
    use vk::Format as F;
    Some(match fmt {
        1 => (F::R8_UNORM, A::Float),
        2 => (F::R8_SNORM, A::Float),
        3 => (F::R8_UINT, A::Uint),
        4 => (F::R8_SINT, A::Sint),
        5 => (F::R16_SFLOAT, A::Float),
        6 => (F::R16_UNORM, A::Float),
        7 => (F::R16_SNORM, A::Float),
        8 => (F::R16_UINT, A::Uint),
        9 => (F::R16_SINT, A::Sint),
        10 => (F::R32_SFLOAT, A::Float),
        11 => (F::R32_UINT, A::Uint),
        12 => (F::R32_SINT, A::Sint),
        13 => (F::R8G8_UNORM, A::Float),
        14 => (F::R8G8_SNORM, A::Float),
        15 => (F::R8G8_UINT, A::Uint),
        16 => (F::R8G8_SINT, A::Sint),
        17 => (F::R16G16_SFLOAT, A::Float),
        18 => (F::R16G16_UNORM, A::Float),
        19 => (F::R16G16_SNORM, A::Float),
        20 => (F::R16G16_UINT, A::Uint),
        21 => (F::R16G16_SINT, A::Sint),
        22 => (F::R32G32_SFLOAT, A::Float),
        23 => (F::R32G32_UINT, A::Uint),
        24 => (F::R32G32_SINT, A::Sint),
        25 => (F::R8G8B8_UNORM, A::Float),
        26 => (F::R8G8B8_SNORM, A::Float),
        27 => (F::R8G8B8_UINT, A::Uint),
        28 => (F::R8G8B8_SINT, A::Sint),
        29 => (F::R16G16B16_SFLOAT, A::Float),
        30 => (F::R16G16B16_UNORM, A::Float),
        31 => (F::R16G16B16_SNORM, A::Float),
        32 => (F::R16G16B16_UINT, A::Uint),
        33 => (F::R16G16B16_SINT, A::Sint),
        34 => (F::R32G32B32_SFLOAT, A::Float),
        35 => (F::R32G32B32_UINT, A::Uint),
        36 => (F::R32G32B32_SINT, A::Sint),
        37 | 109 => (F::R8G8B8A8_UNORM, A::Float),
        38 | 110 => (F::R8G8B8A8_SNORM, A::Float),
        39 | 111 => (F::R8G8B8A8_UINT, A::Uint),
        40 | 112 => (F::R8G8B8A8_SINT, A::Sint),
        41 | 113 => (F::R16G16B16A16_SFLOAT, A::Float),
        42 | 114 => (F::R16G16B16A16_UNORM, A::Float),
        43 | 115 => (F::R16G16B16A16_SNORM, A::Float),
        44 | 116 => (F::R16G16B16A16_UINT, A::Uint),
        45 | 117 => (F::R16G16B16A16_SINT, A::Sint),
        46 | 118 => (F::R32G32B32A32_SFLOAT, A::Float),
        47 | 119 => (F::R32G32B32A32_UINT, A::Uint),
        48 | 120 => (F::R32G32B32A32_SINT, A::Sint),
        61 => (F::A2B10G10R10_UNORM_PACK32, A::Float),
        105 => (F::A2B10G10R10_SNORM_PACK32, A::Float),
        63 => (F::B10G11R11_UFLOAT_PACK32, A::Float),
        64 => (F::E5B9G9R9_UFLOAT_PACK32, A::Float),
        // *_UI2F / *_I2F: integers the fetch converts to float; the shader converts them (scaled attribute types).
        81 => (F::R8_UINT, A::Uscaled),
        82 => (F::R8_SINT, A::Sscaled),
        83 => (F::R16_UINT, A::Uscaled),
        84 => (F::R16_SINT, A::Sscaled),
        85 => (F::R32_UINT, A::Uscaled),
        86 => (F::R32_SINT, A::Sscaled),
        87 => (F::R8G8_UINT, A::Uscaled),
        88 => (F::R8G8_SINT, A::Sscaled),
        89 => (F::R16G16_UINT, A::Uscaled),
        90 => (F::R16G16_SINT, A::Sscaled),
        91 => (F::R32G32_UINT, A::Uscaled),
        92 => (F::R32G32_SINT, A::Sscaled),
        93 => (F::R8G8B8_UINT, A::Uscaled),
        94 => (F::R8G8B8_SINT, A::Sscaled),
        95 => (F::R16G16B16_UINT, A::Uscaled),
        96 => (F::R16G16B16_SINT, A::Sscaled),
        97 => (F::R32G32B32_UINT, A::Uscaled),
        98 => (F::R32G32B32_SINT, A::Sscaled),
        99 => (F::R8G8B8A8_UINT, A::Uscaled),
        100 => (F::R8G8B8A8_SINT, A::Sscaled),
        101 => (F::R16G16B16A16_UINT, A::Uscaled),
        102 => (F::R16G16B16A16_SINT, A::Sscaled),
        103 => (F::R32G32B32A32_UINT, A::Uscaled),
        104 => (F::R32G32B32A32_SINT, A::Sscaled),
        _ => return None,
    })
}

/// Whether an image of NVN format `image` can be viewed as NVN format `view`: same texel block (size and dimensions), as
/// Vulkan's format compatibility classes require. A compressed texture's one-block-per-texel views are not.
pub fn view_compatible(image: i32, view: i32) -> bool {
    match (crate::nvn::layout::format_info(image), crate::nvn::layout::format_info(view)) {
        (Some(a), Some(b)) => a.bytes == b.bytes && a.block_w == b.block_w && a.block_h == b.block_h,
        _ => false,
    }
}
