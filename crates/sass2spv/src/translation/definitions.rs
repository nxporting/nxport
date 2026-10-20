// What the translator knows about the stage: its header, definitions, attribute usage and used features.

use crate::accessor::{GpuAccessor, GraphicsState};
use crate::ir::IoVariable;
use crate::types::{AlphaTestOp, AttributeType, InputTopology, OutputTopology, ShaderStage, TessPatchType, TessSpacing};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct AggregateType(pub u32);

impl AggregateType {
    pub const INVALID: Self = Self(0);
    pub const VOID: Self = Self(1);
    pub const BOOL: Self = Self(2);
    pub const FP32: Self = Self(3);
    pub const FP64: Self = Self(4);
    pub const S32: Self = Self(5);
    pub const U32: Self = Self(6);
    pub const ELEMENT_TYPE_MASK: Self = Self(0xff);
    pub const ELEMENT_COUNT_SHIFT: u32 = 8;
    pub const ELEMENT_COUNT_MASK: Self = Self(3 << 8);
    pub const SCALAR: Self = Self(0);
    pub const VECTOR2: Self = Self(1 << 8);
    pub const VECTOR3: Self = Self(2 << 8);
    pub const VECTOR4: Self = Self(3 << 8);
    pub const ARRAY: Self = Self(1 << 10);

    pub fn element(self) -> Self {
        Self(self.0 & Self::ELEMENT_TYPE_MASK.0)
    }

    pub fn count(self) -> Self {
        Self(self.0 & Self::ELEMENT_COUNT_MASK.0)
    }

    pub fn has(self, f: Self) -> bool {
        self.0 & f.0 != 0
    }

    pub fn size_in_bytes(self) -> i32 {
        let element = match self.element() {
            Self::BOOL | Self::FP32 | Self::S32 | Self::U32 => 4,
            Self::FP64 => 8,
            _ => 0,
        };
        match self.count() {
            Self::VECTOR2 => element * 2,
            Self::VECTOR3 => element * 3,
            Self::VECTOR4 => element * 4,
            _ => element,
        }
    }
}

impl std::ops::BitOr for AggregateType {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

impl std::ops::BitAnd for AggregateType {
    type Output = Self;
    fn bitand(self, o: Self) -> Self {
        Self(self.0 & o.0)
    }
}

impl std::ops::Not for AggregateType {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

impl AttributeType {
    pub fn as_aggregate(self, supports_scaled_formats: bool) -> AggregateType {
        match self {
            AttributeType::Float => AggregateType::FP32,
            AttributeType::Sint => AggregateType::S32,
            AttributeType::Uint => AggregateType::U32,
            AttributeType::Sscaled => if supports_scaled_formats { AggregateType::FP32 } else { AggregateType::S32 },
            AttributeType::Uscaled => if supports_scaled_formats { AggregateType::FP32 } else { AggregateType::U32 },
        }
    }

    pub fn aggregate(self) -> AggregateType {
        match self {
            AttributeType::Float => AggregateType::FP32,
            AttributeType::Sint => AggregateType::S32,
            AttributeType::Uint => AggregateType::U32,
            t => panic!("invalid attribute type {t:?}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FeatureFlags(pub u32);

impl FeatureFlags {
    pub const NONE: Self = Self(0);
    pub const FRAG_COORD_XY: Self = Self(1 << 1);
    pub const BINDLESS: Self = Self(1 << 2);
    pub const INSTANCE_ID: Self = Self(1 << 3);
    pub const DRAW_PARAMETERS: Self = Self(1 << 4);
    pub const RT_LAYER: Self = Self(1 << 5);
    pub const SHUFFLE: Self = Self(1 << 6);
    pub const VIEWPORT_INDEX: Self = Self(1 << 7);
    pub const VIEWPORT_MASK: Self = Self(1 << 8);
    pub const FIXED_FUNC_ATTR: Self = Self(1 << 9);
    pub const LOCAL_MEMORY: Self = Self(1 << 10);
    pub const SHARED_MEMORY: Self = Self(1 << 11);
    pub const STORE: Self = Self(1 << 12);
    pub const VTG_AS_COMPUTE: Self = Self(1 << 13);

    pub fn has(self, f: Self) -> bool {
        self.0 & f.0 == f.0
    }
}

impl std::ops::BitOr for FeatureFlags {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

impl std::ops::BitOrAssign for FeatureFlags {
    fn bitor_assign(&mut self, o: Self) {
        self.0 |= o.0;
    }
}

pub mod attribute_consts {
    pub const PRIMITIVE_ID: i32 = 0x060;
    pub const LAYER: i32 = 0x064;
    pub const VIEWPORT_INDEX: i32 = 0x068;
    pub const POSITION_X: i32 = 0x070;
    pub const POSITION_Y: i32 = 0x074;
    pub const FRONT_COLOR_DIFFUSE_R: i32 = 0x280;
    pub const BACK_COLOR_DIFFUSE_R: i32 = 0x2a0;
    pub const CLIP_DISTANCE0: i32 = 0x2c0;
    pub const CLIP_DISTANCE1: i32 = 0x2c4;
    pub const CLIP_DISTANCE2: i32 = 0x2c8;
    pub const CLIP_DISTANCE3: i32 = 0x2cc;
    pub const CLIP_DISTANCE4: i32 = 0x2d0;
    pub const CLIP_DISTANCE5: i32 = 0x2d4;
    pub const CLIP_DISTANCE6: i32 = 0x2d8;
    pub const CLIP_DISTANCE7: i32 = 0x2dc;
    pub const FOG_COORD: i32 = 0x2e8;
    pub const TESS_COORD_X: i32 = 0x2f0;
    pub const TESS_COORD_Y: i32 = 0x2f4;
    pub const INSTANCE_ID: i32 = 0x2f8;
    pub const VERTEX_ID: i32 = 0x2fc;
    pub const TEX_COORD_COUNT: i32 = 10;
    pub const TEX_COORD_BASE: i32 = 0x300;
    pub const TEX_COORD_END: i32 = TEX_COORD_BASE + TEX_COORD_COUNT * 16;
    pub const VIEWPORT_MASK: i32 = 0x3a0;
    pub const FRONT_FACING: i32 = 0x3fc;
    pub const USER_ATTRIBUTES_COUNT: i32 = 32;
    pub const USER_ATTRIBUTE_BASE: i32 = 0x80;
    pub const USER_ATTRIBUTE_END: i32 = USER_ATTRIBUTE_BASE + USER_ATTRIBUTES_COUNT * 16;
    pub const USER_ATTRIBUTE_PER_PATCH_BASE: i32 = 0x18;
    pub const USER_ATTRIBUTE_PER_PATCH_END: i32 = 0x200;
}

pub const MAX_ATTRIBUTES: i32 = 16;
pub const ALL_ATTRIBUTES_MASK: i32 = (u32::MAX >> (32 - MAX_ATTRIBUTES)) as i32;
pub const NVN_BASE_VERTEX_BYTE_OFFSET: i32 = 0x640;
pub const NVN_BASE_INSTANCE_BYTE_OFFSET: i32 = 0x644;
pub const NVN_DRAW_INDEX_BYTE_OFFSET: i32 = 0x648;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelImap {
    Unused,
    Constant,
    Perspective,
    ScreenLinear,
}

impl PixelImap {
    fn from_bits(v: u8) -> PixelImap {
        [PixelImap::Unused, PixelImap::Constant, PixelImap::Perspective, PixelImap::ScreenLinear][(v & 3) as usize]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImapPixelType {
    pub x: PixelImap,
    pub y: PixelImap,
    pub z: PixelImap,
    pub w: PixelImap,
}

impl ImapPixelType {
    pub fn first_used_type(&self) -> PixelImap {
        [self.x, self.y, self.z].into_iter().find(|&t| t != PixelImap::Unused).unwrap_or(self.w)
    }
}

/// The shader program header (SPH) of a graphics stage.
pub struct ShaderHeader {
    pub stage: ShaderStage,
    pub gp_passthrough: bool,
    pub shader_local_memory_low_size: i32,
    pub shader_local_memory_high_size: i32,
    pub threads_per_input_primitive: i32,
    pub shader_local_memory_crs_size: i32,
    pub output_topology: OutputTopology,
    pub max_output_vertex_count: i32,
    pub imap_types: [ImapPixelType; 32],
    pub omap_targets: i32,
    pub omap_sample_mask: bool,
    pub omap_depth: bool,
}

fn extract(v: i32, lsb: u32, len: u32) -> i32 {
    ((v as u32 >> lsb) & ((1u32 << len) - 1)) as i32
}

fn bit(v: i32, b: u32) -> bool {
    (v >> b) & 1 != 0
}

impl ShaderHeader {
    pub fn new(gpu: &GpuAccessor, address: u64) -> Result<ShaderHeader, String> {
        let code = gpu.code(address);
        let mut words = [0i32; 20];
        for (i, w) in words.iter_mut().enumerate() {
            let q = *code.get(i / 2).ok_or("shader header truncated")?;
            *w = (q >> ((i % 2) * 32)) as u32 as i32;
        }
        let stage = match extract(words[0], 10, 4) {
            // Compute (0) is invalid in a header and read as vertex.
            0 | 1 => ShaderStage::Vertex,
            2 => ShaderStage::TessellationControl,
            3 => ShaderStage::TessellationEvaluation,
            4 => ShaderStage::Geometry,
            5 => ShaderStage::Fragment,
            s => return Err(format!("invalid shader stage {s} in header")),
        };
        let output_topology = match extract(words[3], 24, 4) {
            6 => OutputTopology::LineStrip,
            7 => OutputTopology::TriangleStrip,
            _ => OutputTopology::PointList,
        };
        let imap_types = std::array::from_fn(|i| {
            let imap = (words[6 + (i >> 2)] >> ((i & 3) * 8)) as u8;
            ImapPixelType {
                x: PixelImap::from_bits(imap),
                y: PixelImap::from_bits(imap >> 2),
                z: PixelImap::from_bits(imap >> 4),
                w: PixelImap::from_bits(imap >> 6),
            }
        });
        Ok(ShaderHeader {
            stage,
            gp_passthrough: bit(words[0], 24),
            shader_local_memory_low_size: extract(words[1], 0, 24),
            shader_local_memory_high_size: extract(words[2], 0, 24),
            threads_per_input_primitive: extract(words[2], 24, 8),
            shader_local_memory_crs_size: extract(words[3], 0, 24),
            output_topology,
            max_output_vertex_count: extract(words[4], 0, 12),
            imap_types,
            omap_targets: words[18],
            omap_sample_mask: bit(words[19], 0),
            omap_depth: bit(words[19], 1),
        })
    }
}

pub struct ShaderDefinitions {
    graphics_state: GraphicsState,
    pub stage: ShaderStage,
    pub compute_local_size: [i32; 3],
    pub gp_passthrough: bool,
    pub last_in_vertex_pipeline: bool,
    pub threads_per_input_primitive: i32,
    pub output_topology: OutputTopology,
    pub max_output_vertices: i32,
    pub imap_types: Option<[ImapPixelType; 32]>,
    pub ia_indexing: bool,
    pub oa_indexing: bool,
    pub omap_targets: i32,
    pub omap_sample_mask: bool,
    pub omap_depth: bool,
    pub supports_scaled_vertex_formats: bool,
}

impl ShaderDefinitions {
    pub fn compute(local_size: [i32; 3]) -> ShaderDefinitions {
        ShaderDefinitions {
            graphics_state: GraphicsState::zeroed(),
            stage: ShaderStage::Compute,
            compute_local_size: local_size,
            gp_passthrough: false,
            last_in_vertex_pipeline: false,
            threads_per_input_primitive: 0,
            output_topology: OutputTopology::PointList,
            max_output_vertices: 0,
            imap_types: None,
            ia_indexing: false,
            oa_indexing: false,
            omap_targets: 0,
            omap_sample_mask: false,
            omap_depth: false,
            supports_scaled_vertex_formats: false,
        }
    }

    pub fn graphics(gpu: &GpuAccessor, header: &ShaderHeader) -> ShaderDefinitions {
        let graphics_state = gpu.graphics_state();
        let gp_passthrough = header.stage == ShaderStage::Geometry && header.gp_passthrough;
        let max_output_vertices =
            if gp_passthrough { input_vertex_count_no_adjacency(graphics_state.topology) } else { header.max_output_vertex_count };
        ShaderDefinitions {
            graphics_state,
            stage: header.stage,
            compute_local_size: [0; 3],
            gp_passthrough,
            last_in_vertex_pipeline: header.stage < ShaderStage::Fragment,
            threads_per_input_primitive: header.threads_per_input_primitive,
            output_topology: header.output_topology,
            max_output_vertices,
            imap_types: Some(header.imap_types),
            ia_indexing: false,
            oa_indexing: false,
            omap_targets: header.omap_targets,
            omap_sample_mask: header.omap_sample_mask,
            omap_depth: header.omap_depth,
            supports_scaled_vertex_formats: gpu.host_supports_scaled_vertex_formats(),
        }
    }

    pub fn tess_cw(&self) -> bool {
        self.graphics_state.tess_cw
    }
    pub fn tess_patch_type(&self) -> TessPatchType {
        self.graphics_state.tess_patch_type
    }
    pub fn tess_spacing(&self) -> TessSpacing {
        self.graphics_state.tess_spacing
    }
    pub fn alpha_to_coverage_dither_enable(&self) -> bool {
        self.graphics_state.alpha_to_coverage_enable && self.graphics_state.alpha_to_coverage_dither_enable
    }
    pub fn viewport_transform_disable(&self) -> bool {
        self.graphics_state.viewport_transform_disable
    }
    pub fn depth_mode(&self) -> bool {
        self.graphics_state.depth_mode
    }
    pub fn point_size(&self) -> f32 {
        self.graphics_state.point_size
    }
    pub fn alpha_test_compare(&self) -> AlphaTestOp {
        self.graphics_state.alpha_test_compare
    }
    pub fn alpha_test_reference(&self) -> f32 {
        self.graphics_state.alpha_test_reference
    }
    pub fn input_topology(&self) -> InputTopology {
        self.graphics_state.topology
    }
    pub fn dual_source_blend(&self) -> bool {
        self.graphics_state.dual_source_blend_enable
    }
    pub fn early_z_force(&self) -> bool {
        self.graphics_state.early_z_force
    }
    pub fn y_negate_enabled(&self) -> bool {
        self.graphics_state.y_negate_enabled
    }
    pub fn origin_upper_left(&self) -> bool {
        self.graphics_state.origin_upper_left
    }
    pub fn halve_primitive_id(&self) -> bool {
        self.graphics_state.halve_primitive_id
    }
    pub fn has_constant_buffer_draw_parameters(&self) -> bool {
        self.graphics_state.has_constant_buffer_draw_parameters
    }
    /// Transform feedback is never enabled.
    pub fn transform_feedback_enabled(&self) -> bool {
        false
    }

    pub fn enable_input_indexing(&mut self) {
        self.ia_indexing = true;
    }

    pub fn enable_output_indexing(&mut self) {
        self.oa_indexing = true;
    }

    pub fn has_per_location_input_or_output(&self, io: IoVariable, is_output: bool) -> bool {
        if io == IoVariable::UserDefined {
            return (!is_output && !self.ia_indexing) || (is_output && !self.oa_indexing);
        }
        io == IoVariable::FragmentOutputColor
    }

    /// Only true for transform feedback outputs, which are never enabled.
    pub fn has_per_location_input_or_output_component(&self, _io: IoVariable, _location: i32, _component: i32, _is_output: bool) -> bool {
        false
    }

    pub fn fragment_output_color_type(&self, location: i32) -> AggregateType {
        AggregateType::VECTOR4 | self.graphics_state.fragment_output_types[location as usize].aggregate()
    }

    pub fn user_defined_type(&self, location: i32, is_output: bool) -> AggregateType {
        if (!is_output && self.ia_indexing) || (is_output && self.oa_indexing) {
            return AggregateType::ARRAY | AggregateType::VECTOR4 | AggregateType::FP32;
        }
        if self.stage == ShaderStage::Vertex && !is_output {
            AggregateType::VECTOR4 | self.graphics_state.attribute_types[location as usize].as_aggregate(self.supports_scaled_vertex_formats)
        } else {
            AggregateType::VECTOR4 | AggregateType::FP32
        }
    }

    pub fn attribute_type(&self, location: i32) -> AttributeType {
        self.graphics_state.attribute_types[location as usize]
    }

    pub fn is_attribute_sint(&self, location: i32) -> bool {
        self.graphics_state.attribute_types[location as usize] == AttributeType::Sint
    }

    /// The engine's attribute types are never packed.
    pub fn is_attribute_packed(&self, _location: i32) -> bool {
        false
    }

    pub fn is_attribute_packed_rgb10_a2_signed(&self, _location: i32) -> bool {
        false
    }

    pub fn geometry_output_index_buffer_stride_per_instance(&self) -> i32 {
        self.max_output_vertices
            + match self.output_topology {
                OutputTopology::LineStrip => self.max_output_vertices / 2,
                OutputTopology::TriangleStrip => self.max_output_vertices / 3,
                _ => self.max_output_vertices,
            }
    }
}

pub fn input_vertex_count_no_adjacency(t: InputTopology) -> i32 {
    match t {
        InputTopology::Points => 1,
        InputTopology::Lines | InputTopology::LinesAdjacency => 2,
        InputTopology::Triangles | InputTopology::TrianglesAdjacency => 3,
    }
}

/// Which attributes the stage reads and writes. A single stage is translated, so nothing comes from a next stage.
#[derive(Default)]
pub struct AttributeUsage {
    pub used_input_attributes: i32,
    pub used_output_attributes: i32,
    pub used_input_attributes_per_patch: crate::collections::NetSet<i32>,
    pub used_output_attributes_per_patch: crate::collections::NetSet<i32>,
    this_used_input_attributes: i32,
    pub this_input_attributes_components: u128,
}

impl AttributeUsage {
    pub fn set_input_user_attribute(&mut self, index: i32, component: i32) {
        let mask = 1 << index;
        self.used_input_attributes |= mask;
        self.this_used_input_attributes |= mask;
        self.this_input_attributes_components |= 1u128 << (index * 4 + component);
    }

    pub fn set_input_user_attribute_per_patch(&mut self, index: i32) {
        self.used_input_attributes_per_patch.insert(index);
    }

    pub fn set_output_user_attribute(&mut self, index: i32) {
        self.used_output_attributes |= 1 << index;
    }

    pub fn set_output_user_attribute_per_patch(&mut self, index: i32) {
        self.used_output_attributes_per_patch.insert(index);
    }

    pub fn per_patch_attribute_location(&self, index: i32) -> i32 {
        index
    }

    /// The next stage's inputs are unknown, so every output counts as used.
    pub fn is_used_output_attribute(&self, _attr: i32) -> bool {
        true
    }

    pub fn free_user_attribute(&self, is_output: bool, mut index: i32) -> i32 {
        let mut use_mask = if is_output { 0 } else { self.this_used_input_attributes };
        let mut bit = -1;
        while use_mask != -1 {
            bit = (!use_mask).trailing_zeros() as i32;
            if bit == 32 {
                bit = -1;
                break;
            } else if index < 1 {
                break;
            }
            use_mask |= 1 << bit;
            index -= 1;
        }
        bit
    }

    pub fn set_all_input_user_attributes(&mut self) {
        self.used_input_attributes |= ALL_ATTRIBUTES_MASK;
        self.this_input_attributes_components |= !0u128 >> (128 - MAX_ATTRIBUTES * 4);
    }

    pub fn set_all_output_user_attributes(&mut self) {
        self.used_output_attributes |= ALL_ATTRIBUTES_MASK;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoUsage {
    used_features: FeatureFlags,
    pub clip_distances_written: u8,
    pub user_defined_map: i32,
}

impl IoUsage {
    pub fn new(used_features: FeatureFlags, clip_distances_written: u8, user_defined_map: i32) -> IoUsage {
        IoUsage { used_features, clip_distances_written, user_defined_map }
    }

    pub fn uses_rt_layer(&self) -> bool {
        self.used_features.has(FeatureFlags::RT_LAYER)
    }

    pub fn uses_viewport_index(&self) -> bool {
        self.used_features.has(FeatureFlags::VIEWPORT_INDEX)
    }

    pub fn uses_viewport_mask(&self) -> bool {
        self.used_features.has(FeatureFlags::VIEWPORT_MASK)
    }
}
