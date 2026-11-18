// NVN state objects (nvn/state.rs bit layouts) as Vulkan pipeline state.

use ash::vk;

fn get(bits: u64, shift: u32, width: u32) -> u64 {
    (bits >> shift) & ((1u64 << width) - 1)
}

pub fn compare(f: u64) -> vk::CompareOp {
    match f {
        1 => vk::CompareOp::NEVER,
        2 => vk::CompareOp::LESS,
        3 => vk::CompareOp::EQUAL,
        4 => vk::CompareOp::LESS_OR_EQUAL,
        5 => vk::CompareOp::GREATER,
        6 => vk::CompareOp::NOT_EQUAL,
        7 => vk::CompareOp::GREATER_OR_EQUAL,
        _ => vk::CompareOp::ALWAYS,
    }
}

fn stencil_op(o: u64) -> vk::StencilOp {
    match o {
        2 => vk::StencilOp::ZERO,
        3 => vk::StencilOp::REPLACE,
        4 => vk::StencilOp::INCREMENT_AND_CLAMP,
        5 => vk::StencilOp::DECREMENT_AND_CLAMP,
        6 => vk::StencilOp::INVERT,
        7 => vk::StencilOp::INCREMENT_AND_WRAP,
        8 => vk::StencilOp::DECREMENT_AND_WRAP,
        _ => vk::StencilOp::KEEP,
    }
}

fn blend_factor(f: u64) -> vk::BlendFactor {
    use vk::BlendFactor as B;
    match crate::nvn::state::blend_func_decode(f) {
        1 => B::ZERO,
        2 => B::ONE,
        3 => B::SRC_COLOR,
        4 => B::ONE_MINUS_SRC_COLOR,
        5 => B::SRC_ALPHA,
        6 => B::ONE_MINUS_SRC_ALPHA,
        7 => B::DST_ALPHA,
        8 => B::ONE_MINUS_DST_ALPHA,
        9 => B::DST_COLOR,
        10 => B::ONE_MINUS_DST_COLOR,
        11 => B::SRC_ALPHA_SATURATE,
        0x10 => B::SRC1_COLOR,
        0x11 => B::ONE_MINUS_SRC1_COLOR,
        0x12 => B::SRC1_ALPHA,
        0x13 => B::ONE_MINUS_SRC1_ALPHA,
        0x61 => B::CONSTANT_COLOR,
        0x62 => B::ONE_MINUS_CONSTANT_COLOR,
        0x63 => B::CONSTANT_ALPHA,
        0x64 => B::ONE_MINUS_CONSTANT_ALPHA,
        _ => B::ONE,
    }
}

fn blend_op(e: u64) -> vk::BlendOp {
    match e {
        2 => vk::BlendOp::SUBTRACT,
        3 => vk::BlendOp::REVERSE_SUBTRACT,
        4 => vk::BlendOp::MIN,
        5 => vk::BlendOp::MAX,
        _ => vk::BlendOp::ADD,
    }
}

/// Color target `i`'s blend state: from the blend state bound for it (bits 0..4 of the state name its target; 0 = never
/// bound: NVN's defaults), the color state's enable bit and the channel mask.
pub fn attachment(i: usize, blend: u64, color_state: u32, channel_mask: u32) -> vk::PipelineColorBlendAttachmentState {
    let enabled = color_state >> i & 1 != 0;
    let (src, dst, src_a, dst_a, eq, eq_a) = if blend == 0 {
        (2, 1, 2, 1, 1, 1)
    } else {
        (get(blend, 4, 6), get(blend, 10, 6), get(blend, 16, 6), get(blend, 22, 6), get(blend, 28, 4), get(blend, 32, 4))
    };
    let m = channel_mask >> (i * 4) & 0xF;
    let mut wm = vk::ColorComponentFlags::empty();
    for (bit, f) in [(1, vk::ColorComponentFlags::R), (2, vk::ColorComponentFlags::G), (4, vk::ColorComponentFlags::B), (8, vk::ColorComponentFlags::A)] {
        if m & bit != 0 {
            wm |= f;
        }
    }
    vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(enabled)
        .src_color_blend_factor(blend_factor(src))
        .dst_color_blend_factor(blend_factor(dst))
        .color_blend_op(blend_op(eq))
        .src_alpha_blend_factor(blend_factor(src_a))
        .dst_alpha_blend_factor(blend_factor(dst_a))
        .alpha_blend_op(blend_op(eq_a))
        .color_write_mask(wm)
}

/// NVNdepthStencilState's defaults: depth test off, depth write on, LESS; stencil ALWAYS, KEEP.
pub fn default_depth_stencil() -> u64 {
    let mut b = 1 << 1 | 2 << 2;
    for base in [7, 23] {
        b |= 8 << base | 1 << (base + 4) | 1 << (base + 8) | 1 << (base + 12);
    }
    b
}

/// The depth / stencil state, with reference and masks set dynamically.
pub fn depth_stencil(bits: u64, has_depth: bool, has_stencil: bool) -> vk::PipelineDepthStencilStateCreateInfo<'static> {
    let face = |base: u32| {
        vk::StencilOpState::default()
            .compare_op(compare(get(bits, base, 4)))
            .fail_op(stencil_op(get(bits, base + 4, 4)))
            .depth_fail_op(stencil_op(get(bits, base + 8, 4)))
            .pass_op(stencil_op(get(bits, base + 12, 4)))
            .compare_mask(0xFF)
            .write_mask(0xFF)
    };
    vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(has_depth && get(bits, 0, 1) != 0)
        .depth_write_enable(has_depth && get(bits, 1, 1) != 0)
        .depth_compare_op(compare(get(bits, 2, 4)))
        .stencil_test_enable(has_stencil && get(bits, 6, 1) != 0)
        .front(face(7))
        .back(face(23))
}

/// (cull mode, front face, polygon mode, depth bias enabled) from an NVNpolygonState (0: never bound: defaults).
/// `flip_y`: the viewport flips y (upper-left window origin), which turns the winding around.
pub fn polygon(bits: u32, flip_y: bool) -> (vk::CullModeFlags, vk::FrontFace, vk::PolygonMode, bool) {
    let b = bits as u64;
    let b = if bits == 0 { 1 << 2 | 2 << 3 } else { b };
    let cull = match get(b, 0, 2) {
        1 => vk::CullModeFlags::FRONT,
        2 => vk::CullModeFlags::BACK,
        3 => vk::CullModeFlags::FRONT_AND_BACK,
        _ => vk::CullModeFlags::NONE,
    };
    let ccw = get(b, 2, 1) != 0;
    // NVN's windings are in window coordinates with y up; Vulkan's in framebuffer coordinates with y down.
    let front = if ccw != flip_y { vk::FrontFace::CLOCKWISE } else { vk::FrontFace::COUNTER_CLOCKWISE };
    let mode = get(b, 3, 2);
    let pm = match mode {
        0 => vk::PolygonMode::POINT,
        1 => vk::PolygonMode::LINE,
        _ => vk::PolygonMode::FILL,
    };
    let offsets = get(b, 5, 3);
    let bias = match mode {
        0 => offsets & 1 != 0,
        1 => offsets & 2 != 0,
        _ => offsets & 4 != 0,
    };
    (cull, front, pm, bias)
}

/// (topology, primitive restart allowed) for an NVNdrawPrimitive; None for ones Vulkan has no topology for.
pub fn topology(mode: i32) -> Option<vk::PrimitiveTopology> {
    use vk::PrimitiveTopology as T;
    Some(match mode {
        0 => T::POINT_LIST,
        1 => T::LINE_LIST,
        3 => T::LINE_STRIP,
        4 => T::TRIANGLE_LIST,
        5 => T::TRIANGLE_STRIP,
        6 | 9 => T::TRIANGLE_FAN,
        10 => T::LINE_LIST_WITH_ADJACENCY,
        11 => T::LINE_STRIP_WITH_ADJACENCY,
        12 => T::TRIANGLE_LIST_WITH_ADJACENCY,
        13 => T::TRIANGLE_STRIP_WITH_ADJACENCY,
        14 => T::PATCH_LIST,
        _ => return None,
    })
}
