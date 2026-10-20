// Generated from Ryujinx's Spv.Generator/spirv.cs (SPIR-V 1.5 revision 4 grammar) - do not edit.

#![allow(dead_code)]

pub const MAGIC_NUMBER: u32 = 0x07230203;
pub const VERSION: u32 = 0x00010500;
pub const REVISION: u32 = 4;
pub const OP_CODE_MASK: u32 = 0xffff;
pub const WORD_COUNT_SHIFT: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceLanguage(pub u32);

#[rustfmt::skip]
impl SourceLanguage {
    pub const UNKNOWN: Self = Self(0);
    pub const ESSL: Self = Self(1);
    pub const GLSL: Self = Self(2);
    pub const OPEN_CL_C: Self = Self(3);
    pub const OPEN_CL_CPP: Self = Self(4);
    pub const HLSL: Self = Self(5);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExecutionModel(pub u32);

#[rustfmt::skip]
impl ExecutionModel {
    pub const VERTEX: Self = Self(0);
    pub const TESSELLATION_CONTROL: Self = Self(1);
    pub const TESSELLATION_EVALUATION: Self = Self(2);
    pub const GEOMETRY: Self = Self(3);
    pub const FRAGMENT: Self = Self(4);
    pub const GL_COMPUTE: Self = Self(5);
    pub const KERNEL: Self = Self(6);
    pub const TASK_NV: Self = Self(5267);
    pub const MESH_NV: Self = Self(5268);
    pub const RAY_GENERATION_KHR: Self = Self(5313);
    pub const RAY_GENERATION_NV: Self = Self(Self::RAY_GENERATION_KHR.0);
    pub const INTERSECTION_KHR: Self = Self(5314);
    pub const INTERSECTION_NV: Self = Self(Self::INTERSECTION_KHR.0);
    pub const ANY_HIT_KHR: Self = Self(5315);
    pub const ANY_HIT_NV: Self = Self(Self::ANY_HIT_KHR.0);
    pub const CLOSEST_HIT_KHR: Self = Self(5316);
    pub const CLOSEST_HIT_NV: Self = Self(Self::CLOSEST_HIT_KHR.0);
    pub const MISS_KHR: Self = Self(5317);
    pub const MISS_NV: Self = Self(Self::MISS_KHR.0);
    pub const CALLABLE_KHR: Self = Self(5318);
    pub const CALLABLE_NV: Self = Self(Self::CALLABLE_KHR.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AddressingModel(pub u32);

#[rustfmt::skip]
impl AddressingModel {
    pub const LOGICAL: Self = Self(0);
    pub const PHYSICAL32: Self = Self(1);
    pub const PHYSICAL64: Self = Self(2);
    pub const PHYSICAL_STORAGE_BUFFER64: Self = Self(5348);
    pub const PHYSICAL_STORAGE_BUFFER64_EXT: Self = Self(Self::PHYSICAL_STORAGE_BUFFER64.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemoryModel(pub u32);

#[rustfmt::skip]
impl MemoryModel {
    pub const SIMPLE: Self = Self(0);
    pub const GLSL450: Self = Self(1);
    pub const OPEN_CL: Self = Self(2);
    pub const VULKAN: Self = Self(3);
    pub const VULKAN_KHR: Self = Self(Self::VULKAN.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExecutionMode(pub u32);

#[rustfmt::skip]
impl ExecutionMode {
    pub const INVOCATIONS: Self = Self(0);
    pub const SPACING_EQUAL: Self = Self(1);
    pub const SPACING_FRACTIONAL_EVEN: Self = Self(2);
    pub const SPACING_FRACTIONAL_ODD: Self = Self(3);
    pub const VERTEX_ORDER_CW: Self = Self(4);
    pub const VERTEX_ORDER_CCW: Self = Self(5);
    pub const PIXEL_CENTER_INTEGER: Self = Self(6);
    pub const ORIGIN_UPPER_LEFT: Self = Self(7);
    pub const ORIGIN_LOWER_LEFT: Self = Self(8);
    pub const EARLY_FRAGMENT_TESTS: Self = Self(9);
    pub const POINT_MODE: Self = Self(10);
    pub const XFB: Self = Self(11);
    pub const DEPTH_REPLACING: Self = Self(12);
    pub const DEPTH_GREATER: Self = Self(14);
    pub const DEPTH_LESS: Self = Self(15);
    pub const DEPTH_UNCHANGED: Self = Self(16);
    pub const LOCAL_SIZE: Self = Self(17);
    pub const LOCAL_SIZE_HINT: Self = Self(18);
    pub const INPUT_POINTS: Self = Self(19);
    pub const INPUT_LINES: Self = Self(20);
    pub const INPUT_LINES_ADJACENCY: Self = Self(21);
    pub const TRIANGLES: Self = Self(22);
    pub const INPUT_TRIANGLES_ADJACENCY: Self = Self(23);
    pub const QUADS: Self = Self(24);
    pub const ISOLINES: Self = Self(25);
    pub const OUTPUT_VERTICES: Self = Self(26);
    pub const OUTPUT_POINTS: Self = Self(27);
    pub const OUTPUT_LINE_STRIP: Self = Self(28);
    pub const OUTPUT_TRIANGLE_STRIP: Self = Self(29);
    pub const VEC_TYPE_HINT: Self = Self(30);
    pub const CONTRACTION_OFF: Self = Self(31);
    pub const INITIALIZER: Self = Self(33);
    pub const FINALIZER: Self = Self(34);
    pub const SUBGROUP_SIZE: Self = Self(35);
    pub const SUBGROUPS_PER_WORKGROUP: Self = Self(36);
    pub const SUBGROUPS_PER_WORKGROUP_ID: Self = Self(37);
    pub const LOCAL_SIZE_ID: Self = Self(38);
    pub const LOCAL_SIZE_HINT_ID: Self = Self(39);
    pub const POST_DEPTH_COVERAGE: Self = Self(4446);
    pub const DENORM_PRESERVE: Self = Self(4459);
    pub const DENORM_FLUSH_TO_ZERO: Self = Self(4460);
    pub const SIGNED_ZERO_INF_NAN_PRESERVE: Self = Self(4461);
    pub const ROUNDING_MODE_RTE: Self = Self(4462);
    pub const ROUNDING_MODE_RTZ: Self = Self(4463);
    pub const STENCIL_REF_REPLACING_EXT: Self = Self(5027);
    pub const OUTPUT_LINES_NV: Self = Self(5269);
    pub const OUTPUT_PRIMITIVES_NV: Self = Self(5270);
    pub const DERIVATIVE_GROUP_QUADS_NV: Self = Self(5289);
    pub const DERIVATIVE_GROUP_LINEAR_NV: Self = Self(5290);
    pub const OUTPUT_TRIANGLES_NV: Self = Self(5298);
    pub const PIXEL_INTERLOCK_ORDERED_EXT: Self = Self(5366);
    pub const PIXEL_INTERLOCK_UNORDERED_EXT: Self = Self(5367);
    pub const SAMPLE_INTERLOCK_ORDERED_EXT: Self = Self(5368);
    pub const SAMPLE_INTERLOCK_UNORDERED_EXT: Self = Self(5369);
    pub const SHADING_RATE_INTERLOCK_ORDERED_EXT: Self = Self(5370);
    pub const SHADING_RATE_INTERLOCK_UNORDERED_EXT: Self = Self(5371);
    pub const MAX_WORKGROUP_SIZE_INTEL: Self = Self(5893);
    pub const MAX_WORK_DIM_INTEL: Self = Self(5894);
    pub const NO_GLOBAL_OFFSET_INTEL: Self = Self(5895);
    pub const NUM_SIMD_WORKITEMS_INTEL: Self = Self(5896);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StorageClass(pub u32);

#[rustfmt::skip]
impl StorageClass {
    pub const UNIFORM_CONSTANT: Self = Self(0);
    pub const INPUT: Self = Self(1);
    pub const UNIFORM: Self = Self(2);
    pub const OUTPUT: Self = Self(3);
    pub const WORKGROUP: Self = Self(4);
    pub const CROSS_WORKGROUP: Self = Self(5);
    pub const PRIVATE: Self = Self(6);
    pub const FUNCTION: Self = Self(7);
    pub const GENERIC: Self = Self(8);
    pub const PUSH_CONSTANT: Self = Self(9);
    pub const ATOMIC_COUNTER: Self = Self(10);
    pub const IMAGE: Self = Self(11);
    pub const STORAGE_BUFFER: Self = Self(12);
    pub const CALLABLE_DATA_KHR: Self = Self(5328);
    pub const CALLABLE_DATA_NV: Self = Self(Self::CALLABLE_DATA_KHR.0);
    pub const INCOMING_CALLABLE_DATA_KHR: Self = Self(5329);
    pub const INCOMING_CALLABLE_DATA_NV: Self = Self(Self::INCOMING_CALLABLE_DATA_KHR.0);
    pub const RAY_PAYLOAD_KHR: Self = Self(5338);
    pub const RAY_PAYLOAD_NV: Self = Self(Self::RAY_PAYLOAD_KHR.0);
    pub const HIT_ATTRIBUTE_KHR: Self = Self(5339);
    pub const HIT_ATTRIBUTE_NV: Self = Self(Self::HIT_ATTRIBUTE_KHR.0);
    pub const INCOMING_RAY_PAYLOAD_KHR: Self = Self(5342);
    pub const INCOMING_RAY_PAYLOAD_NV: Self = Self(Self::INCOMING_RAY_PAYLOAD_KHR.0);
    pub const SHADER_RECORD_BUFFER_KHR: Self = Self(5343);
    pub const SHADER_RECORD_BUFFER_NV: Self = Self(Self::SHADER_RECORD_BUFFER_KHR.0);
    pub const PHYSICAL_STORAGE_BUFFER: Self = Self(5349);
    pub const PHYSICAL_STORAGE_BUFFER_EXT: Self = Self(Self::PHYSICAL_STORAGE_BUFFER.0);
    pub const CODE_SECTION_INTEL: Self = Self(5605);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Dim(pub u32);

#[rustfmt::skip]
impl Dim {
    pub const DIM1_D: Self = Self(0);
    pub const DIM2_D: Self = Self(1);
    pub const DIM3_D: Self = Self(2);
    pub const CUBE: Self = Self(3);
    pub const RECT: Self = Self(4);
    pub const BUFFER: Self = Self(5);
    pub const SUBPASS_DATA: Self = Self(6);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SamplerAddressingMode(pub u32);

#[rustfmt::skip]
impl SamplerAddressingMode {
    pub const NONE: Self = Self(0);
    pub const CLAMP_TO_EDGE: Self = Self(1);
    pub const CLAMP: Self = Self(2);
    pub const REPEAT: Self = Self(3);
    pub const REPEAT_MIRRORED: Self = Self(4);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SamplerFilterMode(pub u32);

#[rustfmt::skip]
impl SamplerFilterMode {
    pub const NEAREST: Self = Self(0);
    pub const LINEAR: Self = Self(1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageFormat(pub u32);

#[rustfmt::skip]
impl ImageFormat {
    pub const UNKNOWN: Self = Self(0);
    pub const RGBA32F: Self = Self(1);
    pub const RGBA16F: Self = Self(2);
    pub const R32F: Self = Self(3);
    pub const RGBA8: Self = Self(4);
    pub const RGBA8_SNORM: Self = Self(5);
    pub const RG32F: Self = Self(6);
    pub const RG16F: Self = Self(7);
    pub const R11F_G11F_B10F: Self = Self(8);
    pub const R16F: Self = Self(9);
    pub const RGBA16: Self = Self(10);
    pub const RGB10_A2: Self = Self(11);
    pub const RG16: Self = Self(12);
    pub const RG8: Self = Self(13);
    pub const R16: Self = Self(14);
    pub const R8: Self = Self(15);
    pub const RGBA16_SNORM: Self = Self(16);
    pub const RG16_SNORM: Self = Self(17);
    pub const RG8_SNORM: Self = Self(18);
    pub const R16_SNORM: Self = Self(19);
    pub const R8_SNORM: Self = Self(20);
    pub const RGBA32I: Self = Self(21);
    pub const RGBA16I: Self = Self(22);
    pub const RGBA8I: Self = Self(23);
    pub const R32I: Self = Self(24);
    pub const RG32I: Self = Self(25);
    pub const RG16I: Self = Self(26);
    pub const RG8I: Self = Self(27);
    pub const R16I: Self = Self(28);
    pub const R8I: Self = Self(29);
    pub const RGBA32UI: Self = Self(30);
    pub const RGBA16UI: Self = Self(31);
    pub const RGBA8UI: Self = Self(32);
    pub const R32UI: Self = Self(33);
    pub const RGB10A2UI: Self = Self(34);
    pub const RG32UI: Self = Self(35);
    pub const RG16UI: Self = Self(36);
    pub const RG8UI: Self = Self(37);
    pub const R16UI: Self = Self(38);
    pub const R8UI: Self = Self(39);
    pub const R64UI: Self = Self(40);
    pub const R64I: Self = Self(41);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageChannelOrder(pub u32);

#[rustfmt::skip]
impl ImageChannelOrder {
    pub const R: Self = Self(0);
    pub const A: Self = Self(1);
    pub const RG: Self = Self(2);
    pub const RA: Self = Self(3);
    pub const RGB: Self = Self(4);
    pub const RGBA: Self = Self(5);
    pub const BGRA: Self = Self(6);
    pub const ARGB: Self = Self(7);
    pub const INTENSITY: Self = Self(8);
    pub const LUMINANCE: Self = Self(9);
    pub const RX: Self = Self(10);
    pub const R_GX: Self = Self(11);
    pub const RG_BX: Self = Self(12);
    pub const DEPTH: Self = Self(13);
    pub const DEPTH_STENCIL: Self = Self(14);
    pub const S_RGB: Self = Self(15);
    pub const S_RG_BX: Self = Self(16);
    pub const S_RGBA: Self = Self(17);
    pub const S_BGRA: Self = Self(18);
    pub const ABGR: Self = Self(19);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageChannelDataType(pub u32);

#[rustfmt::skip]
impl ImageChannelDataType {
    pub const SNORM_INT8: Self = Self(0);
    pub const SNORM_INT16: Self = Self(1);
    pub const UNORM_INT8: Self = Self(2);
    pub const UNORM_INT16: Self = Self(3);
    pub const UNORM_SHORT565: Self = Self(4);
    pub const UNORM_SHORT555: Self = Self(5);
    pub const UNORM_INT101010: Self = Self(6);
    pub const SIGNED_INT8: Self = Self(7);
    pub const SIGNED_INT16: Self = Self(8);
    pub const SIGNED_INT32: Self = Self(9);
    pub const UNSIGNED_INT8: Self = Self(10);
    pub const UNSIGNED_INT16: Self = Self(11);
    pub const UNSIGNED_INT32: Self = Self(12);
    pub const HALF_FLOAT: Self = Self(13);
    pub const FLOAT: Self = Self(14);
    pub const UNORM_INT24: Self = Self(15);
    pub const UNORM_INT101010_2: Self = Self(16);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageOperandsShift(pub u32);

#[rustfmt::skip]
impl ImageOperandsShift {
    pub const BIAS: Self = Self(0);
    pub const LOD: Self = Self(1);
    pub const GRAD: Self = Self(2);
    pub const CONST_OFFSET: Self = Self(3);
    pub const OFFSET: Self = Self(4);
    pub const CONST_OFFSETS: Self = Self(5);
    pub const SAMPLE: Self = Self(6);
    pub const MIN_LOD: Self = Self(7);
    pub const MAKE_TEXEL_AVAILABLE: Self = Self(8);
    pub const MAKE_TEXEL_AVAILABLE_KHR: Self = Self(Self::MAKE_TEXEL_AVAILABLE.0);
    pub const MAKE_TEXEL_VISIBLE: Self = Self(9);
    pub const MAKE_TEXEL_VISIBLE_KHR: Self = Self(Self::MAKE_TEXEL_VISIBLE.0);
    pub const NON_PRIVATE_TEXEL: Self = Self(10);
    pub const NON_PRIVATE_TEXEL_KHR: Self = Self(Self::NON_PRIVATE_TEXEL.0);
    pub const VOLATILE_TEXEL: Self = Self(11);
    pub const VOLATILE_TEXEL_KHR: Self = Self(Self::VOLATILE_TEXEL.0);
    pub const SIGN_EXTEND: Self = Self(12);
    pub const ZERO_EXTEND: Self = Self(13);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageOperandsMask(pub u32);

#[rustfmt::skip]
impl ImageOperandsMask {
    pub const MASK_NONE: Self = Self(0);
    pub const BIAS: Self = Self(0x00000001);
    pub const LOD: Self = Self(0x00000002);
    pub const GRAD: Self = Self(0x00000004);
    pub const CONST_OFFSET: Self = Self(0x00000008);
    pub const OFFSET: Self = Self(0x00000010);
    pub const CONST_OFFSETS: Self = Self(0x00000020);
    pub const SAMPLE: Self = Self(0x00000040);
    pub const MIN_LOD: Self = Self(0x00000080);
    pub const MAKE_TEXEL_AVAILABLE: Self = Self(0x00000100);
    pub const MAKE_TEXEL_AVAILABLE_KHR: Self = Self(Self::MAKE_TEXEL_AVAILABLE.0);
    pub const MAKE_TEXEL_VISIBLE: Self = Self(0x00000200);
    pub const MAKE_TEXEL_VISIBLE_KHR: Self = Self(Self::MAKE_TEXEL_VISIBLE.0);
    pub const NON_PRIVATE_TEXEL: Self = Self(0x00000400);
    pub const NON_PRIVATE_TEXEL_KHR: Self = Self(Self::NON_PRIVATE_TEXEL.0);
    pub const VOLATILE_TEXEL: Self = Self(0x00000800);
    pub const VOLATILE_TEXEL_KHR: Self = Self(Self::VOLATILE_TEXEL.0);
    pub const SIGN_EXTEND: Self = Self(0x00001000);
    pub const ZERO_EXTEND: Self = Self(0x00002000);
    pub const OFFSETS: Self = Self(0x00010000);
}

impl std::ops::BitOr for ImageOperandsMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FPFastMathModeShift(pub u32);

#[rustfmt::skip]
impl FPFastMathModeShift {
    pub const NOT_NA_N: Self = Self(0);
    pub const NOT_INF: Self = Self(1);
    pub const NSZ: Self = Self(2);
    pub const ALLOW_RECIP: Self = Self(3);
    pub const FAST: Self = Self(4);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FPFastMathModeMask(pub u32);

#[rustfmt::skip]
impl FPFastMathModeMask {
    pub const MASK_NONE: Self = Self(0);
    pub const NOT_NA_N: Self = Self(0x00000001);
    pub const NOT_INF: Self = Self(0x00000002);
    pub const NSZ: Self = Self(0x00000004);
    pub const ALLOW_RECIP: Self = Self(0x00000008);
    pub const FAST: Self = Self(0x00000010);
}

impl std::ops::BitOr for FPFastMathModeMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FPRoundingMode(pub u32);

#[rustfmt::skip]
impl FPRoundingMode {
    pub const RTE: Self = Self(0);
    pub const RTZ: Self = Self(1);
    pub const RTP: Self = Self(2);
    pub const RTN: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LinkageType(pub u32);

#[rustfmt::skip]
impl LinkageType {
    pub const EXPORT: Self = Self(0);
    pub const IMPORT: Self = Self(1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccessQualifier(pub u32);

#[rustfmt::skip]
impl AccessQualifier {
    pub const READ_ONLY: Self = Self(0);
    pub const WRITE_ONLY: Self = Self(1);
    pub const READ_WRITE: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionParameterAttribute(pub u32);

#[rustfmt::skip]
impl FunctionParameterAttribute {
    pub const ZEXT: Self = Self(0);
    pub const SEXT: Self = Self(1);
    pub const BY_VAL: Self = Self(2);
    pub const SRET: Self = Self(3);
    pub const NO_ALIAS: Self = Self(4);
    pub const NO_CAPTURE: Self = Self(5);
    pub const NO_WRITE: Self = Self(6);
    pub const NO_READ_WRITE: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Decoration(pub u32);

#[rustfmt::skip]
impl Decoration {
    pub const RELAXED_PRECISION: Self = Self(0);
    pub const SPEC_ID: Self = Self(1);
    pub const BLOCK: Self = Self(2);
    pub const BUFFER_BLOCK: Self = Self(3);
    pub const ROW_MAJOR: Self = Self(4);
    pub const COL_MAJOR: Self = Self(5);
    pub const ARRAY_STRIDE: Self = Self(6);
    pub const MATRIX_STRIDE: Self = Self(7);
    pub const GLSL_SHARED: Self = Self(8);
    pub const GLSL_PACKED: Self = Self(9);
    pub const C_PACKED: Self = Self(10);
    pub const BUILT_IN: Self = Self(11);
    pub const NO_PERSPECTIVE: Self = Self(13);
    pub const FLAT: Self = Self(14);
    pub const PATCH: Self = Self(15);
    pub const CENTROID: Self = Self(16);
    pub const SAMPLE: Self = Self(17);
    pub const INVARIANT: Self = Self(18);
    pub const RESTRICT: Self = Self(19);
    pub const ALIASED: Self = Self(20);
    pub const VOLATILE: Self = Self(21);
    pub const CONSTANT: Self = Self(22);
    pub const COHERENT: Self = Self(23);
    pub const NON_WRITABLE: Self = Self(24);
    pub const NON_READABLE: Self = Self(25);
    pub const UNIFORM: Self = Self(26);
    pub const UNIFORM_ID: Self = Self(27);
    pub const SATURATED_CONVERSION: Self = Self(28);
    pub const STREAM: Self = Self(29);
    pub const LOCATION: Self = Self(30);
    pub const COMPONENT: Self = Self(31);
    pub const INDEX: Self = Self(32);
    pub const BINDING: Self = Self(33);
    pub const DESCRIPTOR_SET: Self = Self(34);
    pub const OFFSET: Self = Self(35);
    pub const XFB_BUFFER: Self = Self(36);
    pub const XFB_STRIDE: Self = Self(37);
    pub const FUNC_PARAM_ATTR: Self = Self(38);
    pub const FP_ROUNDING_MODE: Self = Self(39);
    pub const FP_FAST_MATH_MODE: Self = Self(40);
    pub const LINKAGE_ATTRIBUTES: Self = Self(41);
    pub const NO_CONTRACTION: Self = Self(42);
    pub const INPUT_ATTACHMENT_INDEX: Self = Self(43);
    pub const ALIGNMENT: Self = Self(44);
    pub const MAX_BYTE_OFFSET: Self = Self(45);
    pub const ALIGNMENT_ID: Self = Self(46);
    pub const MAX_BYTE_OFFSET_ID: Self = Self(47);
    pub const NO_SIGNED_WRAP: Self = Self(4469);
    pub const NO_UNSIGNED_WRAP: Self = Self(4470);
    pub const EXPLICIT_INTERP_AMD: Self = Self(4999);
    pub const OVERRIDE_COVERAGE_NV: Self = Self(5248);
    pub const PASSTHROUGH_NV: Self = Self(5250);
    pub const VIEWPORT_RELATIVE_NV: Self = Self(5252);
    pub const SECONDARY_VIEWPORT_RELATIVE_NV: Self = Self(5256);
    pub const PER_PRIMITIVE_NV: Self = Self(5271);
    pub const PER_VIEW_NV: Self = Self(5272);
    pub const PER_TASK_NV: Self = Self(5273);
    pub const PER_VERTEX_NV: Self = Self(5285);
    pub const NON_UNIFORM: Self = Self(5300);
    pub const NON_UNIFORM_EXT: Self = Self(Self::NON_UNIFORM.0);
    pub const RESTRICT_POINTER: Self = Self(5355);
    pub const RESTRICT_POINTER_EXT: Self = Self(Self::RESTRICT_POINTER.0);
    pub const ALIASED_POINTER: Self = Self(5356);
    pub const ALIASED_POINTER_EXT: Self = Self(Self::ALIASED_POINTER.0);
    pub const REFERENCED_INDIRECTLY_INTEL: Self = Self(5602);
    pub const COUNTER_BUFFER: Self = Self(5634);
    pub const HLSL_COUNTER_BUFFER_GOOGLE: Self = Self(Self::COUNTER_BUFFER.0);
    pub const HLSL_SEMANTIC_GOOGLE: Self = Self(5635);
    pub const USER_SEMANTIC: Self = Self(Self::HLSL_SEMANTIC_GOOGLE.0);
    pub const USER_TYPE_GOOGLE: Self = Self(5636);
    pub const REGISTER_INTEL: Self = Self(5825);
    pub const MEMORY_INTEL: Self = Self(5826);
    pub const NUMBANKS_INTEL: Self = Self(5827);
    pub const BANKWIDTH_INTEL: Self = Self(5828);
    pub const MAX_PRIVATE_COPIES_INTEL: Self = Self(5829);
    pub const SINGLEPUMP_INTEL: Self = Self(5830);
    pub const DOUBLEPUMP_INTEL: Self = Self(5831);
    pub const MAX_REPLICATES_INTEL: Self = Self(5832);
    pub const SIMPLE_DUAL_PORT_INTEL: Self = Self(5833);
    pub const MERGE_INTEL: Self = Self(5834);
    pub const BANK_BITS_INTEL: Self = Self(5835);
    pub const FORCE_POW2_DEPTH_INTEL: Self = Self(5836);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BuiltIn(pub u32);

#[rustfmt::skip]
impl BuiltIn {
    pub const POSITION: Self = Self(0);
    pub const POINT_SIZE: Self = Self(1);
    pub const CLIP_DISTANCE: Self = Self(3);
    pub const CULL_DISTANCE: Self = Self(4);
    pub const VERTEX_ID: Self = Self(5);
    pub const INSTANCE_ID: Self = Self(6);
    pub const PRIMITIVE_ID: Self = Self(7);
    pub const INVOCATION_ID: Self = Self(8);
    pub const LAYER: Self = Self(9);
    pub const VIEWPORT_INDEX: Self = Self(10);
    pub const TESS_LEVEL_OUTER: Self = Self(11);
    pub const TESS_LEVEL_INNER: Self = Self(12);
    pub const TESS_COORD: Self = Self(13);
    pub const PATCH_VERTICES: Self = Self(14);
    pub const FRAG_COORD: Self = Self(15);
    pub const POINT_COORD: Self = Self(16);
    pub const FRONT_FACING: Self = Self(17);
    pub const SAMPLE_ID: Self = Self(18);
    pub const SAMPLE_POSITION: Self = Self(19);
    pub const SAMPLE_MASK: Self = Self(20);
    pub const FRAG_DEPTH: Self = Self(22);
    pub const HELPER_INVOCATION: Self = Self(23);
    pub const NUM_WORKGROUPS: Self = Self(24);
    pub const WORKGROUP_SIZE: Self = Self(25);
    pub const WORKGROUP_ID: Self = Self(26);
    pub const LOCAL_INVOCATION_ID: Self = Self(27);
    pub const GLOBAL_INVOCATION_ID: Self = Self(28);
    pub const LOCAL_INVOCATION_INDEX: Self = Self(29);
    pub const WORK_DIM: Self = Self(30);
    pub const GLOBAL_SIZE: Self = Self(31);
    pub const ENQUEUED_WORKGROUP_SIZE: Self = Self(32);
    pub const GLOBAL_OFFSET: Self = Self(33);
    pub const GLOBAL_LINEAR_ID: Self = Self(34);
    pub const SUBGROUP_SIZE: Self = Self(36);
    pub const SUBGROUP_MAX_SIZE: Self = Self(37);
    pub const NUM_SUBGROUPS: Self = Self(38);
    pub const NUM_ENQUEUED_SUBGROUPS: Self = Self(39);
    pub const SUBGROUP_ID: Self = Self(40);
    pub const SUBGROUP_LOCAL_INVOCATION_ID: Self = Self(41);
    pub const VERTEX_INDEX: Self = Self(42);
    pub const INSTANCE_INDEX: Self = Self(43);
    pub const SUBGROUP_EQ_MASK: Self = Self(4416);
    pub const SUBGROUP_EQ_MASK_KHR: Self = Self(Self::SUBGROUP_EQ_MASK.0);
    pub const SUBGROUP_GE_MASK: Self = Self(4417);
    pub const SUBGROUP_GE_MASK_KHR: Self = Self(Self::SUBGROUP_GE_MASK.0);
    pub const SUBGROUP_GT_MASK: Self = Self(4418);
    pub const SUBGROUP_GT_MASK_KHR: Self = Self(Self::SUBGROUP_GT_MASK.0);
    pub const SUBGROUP_LE_MASK: Self = Self(4419);
    pub const SUBGROUP_LE_MASK_KHR: Self = Self(Self::SUBGROUP_LE_MASK.0);
    pub const SUBGROUP_LT_MASK: Self = Self(4420);
    pub const SUBGROUP_LT_MASK_KHR: Self = Self(Self::SUBGROUP_LT_MASK.0);
    pub const BASE_VERTEX: Self = Self(4424);
    pub const BASE_INSTANCE: Self = Self(4425);
    pub const DRAW_INDEX: Self = Self(4426);
    pub const PRIMITIVE_SHADING_RATE_KHR: Self = Self(4432);
    pub const DEVICE_INDEX: Self = Self(4438);
    pub const VIEW_INDEX: Self = Self(4440);
    pub const SHADING_RATE_KHR: Self = Self(4444);
    pub const BARY_COORD_NO_PERSP_AMD: Self = Self(4992);
    pub const BARY_COORD_NO_PERSP_CENTROID_AMD: Self = Self(4993);
    pub const BARY_COORD_NO_PERSP_SAMPLE_AMD: Self = Self(4994);
    pub const BARY_COORD_SMOOTH_AMD: Self = Self(4995);
    pub const BARY_COORD_SMOOTH_CENTROID_AMD: Self = Self(4996);
    pub const BARY_COORD_SMOOTH_SAMPLE_AMD: Self = Self(4997);
    pub const BARY_COORD_PULL_MODEL_AMD: Self = Self(4998);
    pub const FRAG_STENCIL_REF_EXT: Self = Self(5014);
    pub const VIEWPORT_MASK_NV: Self = Self(5253);
    pub const SECONDARY_POSITION_NV: Self = Self(5257);
    pub const SECONDARY_VIEWPORT_MASK_NV: Self = Self(5258);
    pub const POSITION_PER_VIEW_NV: Self = Self(5261);
    pub const VIEWPORT_MASK_PER_VIEW_NV: Self = Self(5262);
    pub const FULLY_COVERED_EXT: Self = Self(5264);
    pub const TASK_COUNT_NV: Self = Self(5274);
    pub const PRIMITIVE_COUNT_NV: Self = Self(5275);
    pub const PRIMITIVE_INDICES_NV: Self = Self(5276);
    pub const CLIP_DISTANCE_PER_VIEW_NV: Self = Self(5277);
    pub const CULL_DISTANCE_PER_VIEW_NV: Self = Self(5278);
    pub const LAYER_PER_VIEW_NV: Self = Self(5279);
    pub const MESH_VIEW_COUNT_NV: Self = Self(5280);
    pub const MESH_VIEW_INDICES_NV: Self = Self(5281);
    pub const BARY_COORD_NV: Self = Self(5286);
    pub const BARY_COORD_NO_PERSP_NV: Self = Self(5287);
    pub const FRAG_SIZE_EXT: Self = Self(5292);
    pub const FRAGMENT_SIZE_NV: Self = Self(Self::FRAG_SIZE_EXT.0);
    pub const FRAG_INVOCATION_COUNT_EXT: Self = Self(5293);
    pub const INVOCATIONS_PER_PIXEL_NV: Self = Self(Self::FRAG_INVOCATION_COUNT_EXT.0);
    pub const LAUNCH_ID_KHR: Self = Self(5319);
    pub const LAUNCH_ID_NV: Self = Self(Self::LAUNCH_ID_KHR.0);
    pub const LAUNCH_SIZE_KHR: Self = Self(5320);
    pub const LAUNCH_SIZE_NV: Self = Self(Self::LAUNCH_SIZE_KHR.0);
    pub const WORLD_RAY_ORIGIN_KHR: Self = Self(5321);
    pub const WORLD_RAY_ORIGIN_NV: Self = Self(Self::WORLD_RAY_ORIGIN_KHR.0);
    pub const WORLD_RAY_DIRECTION_KHR: Self = Self(5322);
    pub const WORLD_RAY_DIRECTION_NV: Self = Self(Self::WORLD_RAY_DIRECTION_KHR.0);
    pub const OBJECT_RAY_ORIGIN_KHR: Self = Self(5323);
    pub const OBJECT_RAY_ORIGIN_NV: Self = Self(Self::OBJECT_RAY_ORIGIN_KHR.0);
    pub const OBJECT_RAY_DIRECTION_KHR: Self = Self(5324);
    pub const OBJECT_RAY_DIRECTION_NV: Self = Self(Self::OBJECT_RAY_DIRECTION_KHR.0);
    pub const RAY_TMIN_KHR: Self = Self(5325);
    pub const RAY_TMIN_NV: Self = Self(Self::RAY_TMIN_KHR.0);
    pub const RAY_TMAX_KHR: Self = Self(5326);
    pub const RAY_TMAX_NV: Self = Self(Self::RAY_TMAX_KHR.0);
    pub const INSTANCE_CUSTOM_INDEX_KHR: Self = Self(5327);
    pub const INSTANCE_CUSTOM_INDEX_NV: Self = Self(Self::INSTANCE_CUSTOM_INDEX_KHR.0);
    pub const OBJECT_TO_WORLD_KHR: Self = Self(5330);
    pub const OBJECT_TO_WORLD_NV: Self = Self(Self::OBJECT_TO_WORLD_KHR.0);
    pub const WORLD_TO_OBJECT_KHR: Self = Self(5331);
    pub const WORLD_TO_OBJECT_NV: Self = Self(Self::WORLD_TO_OBJECT_KHR.0);
    pub const HIT_TNV: Self = Self(5332);
    pub const HIT_KIND_KHR: Self = Self(5333);
    pub const HIT_KIND_NV: Self = Self(Self::HIT_KIND_KHR.0);
    pub const INCOMING_RAY_FLAGS_KHR: Self = Self(5351);
    pub const INCOMING_RAY_FLAGS_NV: Self = Self(Self::INCOMING_RAY_FLAGS_KHR.0);
    pub const RAY_GEOMETRY_INDEX_KHR: Self = Self(5352);
    pub const WARPS_PER_SMNV: Self = Self(5374);
    pub const SM_COUNT_NV: Self = Self(5375);
    pub const WARP_IDNV: Self = Self(5376);
    pub const SMIDNV: Self = Self(5377);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SelectionControlShift(pub u32);

#[rustfmt::skip]
impl SelectionControlShift {
    pub const FLATTEN: Self = Self(0);
    pub const DONT_FLATTEN: Self = Self(1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SelectionControlMask(pub u32);

#[rustfmt::skip]
impl SelectionControlMask {
    pub const MASK_NONE: Self = Self(0);
    pub const FLATTEN: Self = Self(0x00000001);
    pub const DONT_FLATTEN: Self = Self(0x00000002);
}

impl std::ops::BitOr for SelectionControlMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LoopControlShift(pub u32);

#[rustfmt::skip]
impl LoopControlShift {
    pub const UNROLL: Self = Self(0);
    pub const DONT_UNROLL: Self = Self(1);
    pub const DEPENDENCY_INFINITE: Self = Self(2);
    pub const DEPENDENCY_LENGTH: Self = Self(3);
    pub const MIN_ITERATIONS: Self = Self(4);
    pub const MAX_ITERATIONS: Self = Self(5);
    pub const ITERATION_MULTIPLE: Self = Self(6);
    pub const PEEL_COUNT: Self = Self(7);
    pub const PARTIAL_COUNT: Self = Self(8);
    pub const INITIATION_INTERVAL_INTEL: Self = Self(16);
    pub const MAX_CONCURRENCY_INTEL: Self = Self(17);
    pub const DEPENDENCY_ARRAY_INTEL: Self = Self(18);
    pub const PIPELINE_ENABLE_INTEL: Self = Self(19);
    pub const LOOP_COALESCE_INTEL: Self = Self(20);
    pub const MAX_INTERLEAVING_INTEL: Self = Self(21);
    pub const SPECULATED_ITERATIONS_INTEL: Self = Self(22);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LoopControlMask(pub u32);

#[rustfmt::skip]
impl LoopControlMask {
    pub const MASK_NONE: Self = Self(0);
    pub const UNROLL: Self = Self(0x00000001);
    pub const DONT_UNROLL: Self = Self(0x00000002);
    pub const DEPENDENCY_INFINITE: Self = Self(0x00000004);
    pub const DEPENDENCY_LENGTH: Self = Self(0x00000008);
    pub const MIN_ITERATIONS: Self = Self(0x00000010);
    pub const MAX_ITERATIONS: Self = Self(0x00000020);
    pub const ITERATION_MULTIPLE: Self = Self(0x00000040);
    pub const PEEL_COUNT: Self = Self(0x00000080);
    pub const PARTIAL_COUNT: Self = Self(0x00000100);
    pub const INITIATION_INTERVAL_INTEL: Self = Self(0x00010000);
    pub const MAX_CONCURRENCY_INTEL: Self = Self(0x00020000);
    pub const DEPENDENCY_ARRAY_INTEL: Self = Self(0x00040000);
    pub const PIPELINE_ENABLE_INTEL: Self = Self(0x00080000);
    pub const LOOP_COALESCE_INTEL: Self = Self(0x00100000);
    pub const MAX_INTERLEAVING_INTEL: Self = Self(0x00200000);
    pub const SPECULATED_ITERATIONS_INTEL: Self = Self(0x00400000);
}

impl std::ops::BitOr for LoopControlMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionControlShift(pub u32);

#[rustfmt::skip]
impl FunctionControlShift {
    pub const INLINE: Self = Self(0);
    pub const DONT_INLINE: Self = Self(1);
    pub const PURE: Self = Self(2);
    pub const CONST: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionControlMask(pub u32);

#[rustfmt::skip]
impl FunctionControlMask {
    pub const MASK_NONE: Self = Self(0);
    pub const INLINE: Self = Self(0x00000001);
    pub const DONT_INLINE: Self = Self(0x00000002);
    pub const PURE: Self = Self(0x00000004);
    pub const CONST: Self = Self(0x00000008);
}

impl std::ops::BitOr for FunctionControlMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemorySemanticsShift(pub u32);

#[rustfmt::skip]
impl MemorySemanticsShift {
    pub const ACQUIRE: Self = Self(1);
    pub const RELEASE: Self = Self(2);
    pub const ACQUIRE_RELEASE: Self = Self(3);
    pub const SEQUENTIALLY_CONSISTENT: Self = Self(4);
    pub const UNIFORM_MEMORY: Self = Self(6);
    pub const SUBGROUP_MEMORY: Self = Self(7);
    pub const WORKGROUP_MEMORY: Self = Self(8);
    pub const CROSS_WORKGROUP_MEMORY: Self = Self(9);
    pub const ATOMIC_COUNTER_MEMORY: Self = Self(10);
    pub const IMAGE_MEMORY: Self = Self(11);
    pub const OUTPUT_MEMORY: Self = Self(12);
    pub const OUTPUT_MEMORY_KHR: Self = Self(Self::OUTPUT_MEMORY.0);
    pub const MAKE_AVAILABLE: Self = Self(13);
    pub const MAKE_AVAILABLE_KHR: Self = Self(Self::MAKE_AVAILABLE.0);
    pub const MAKE_VISIBLE: Self = Self(14);
    pub const MAKE_VISIBLE_KHR: Self = Self(Self::MAKE_VISIBLE.0);
    pub const VOLATILE: Self = Self(15);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemorySemanticsMask(pub u32);

#[rustfmt::skip]
impl MemorySemanticsMask {
    pub const MASK_NONE: Self = Self(0);
    pub const ACQUIRE: Self = Self(0x00000002);
    pub const RELEASE: Self = Self(0x00000004);
    pub const ACQUIRE_RELEASE: Self = Self(0x00000008);
    pub const SEQUENTIALLY_CONSISTENT: Self = Self(0x00000010);
    pub const UNIFORM_MEMORY: Self = Self(0x00000040);
    pub const SUBGROUP_MEMORY: Self = Self(0x00000080);
    pub const WORKGROUP_MEMORY: Self = Self(0x00000100);
    pub const CROSS_WORKGROUP_MEMORY: Self = Self(0x00000200);
    pub const ATOMIC_COUNTER_MEMORY: Self = Self(0x00000400);
    pub const IMAGE_MEMORY: Self = Self(0x00000800);
    pub const OUTPUT_MEMORY: Self = Self(0x00001000);
    pub const OUTPUT_MEMORY_KHR: Self = Self(Self::OUTPUT_MEMORY.0);
    pub const MAKE_AVAILABLE: Self = Self(0x00002000);
    pub const MAKE_AVAILABLE_KHR: Self = Self(Self::MAKE_AVAILABLE.0);
    pub const MAKE_VISIBLE: Self = Self(0x00004000);
    pub const MAKE_VISIBLE_KHR: Self = Self(Self::MAKE_VISIBLE.0);
    pub const VOLATILE: Self = Self(0x00008000);
}

impl std::ops::BitOr for MemorySemanticsMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemoryAccessShift(pub u32);

#[rustfmt::skip]
impl MemoryAccessShift {
    pub const VOLATILE: Self = Self(0);
    pub const ALIGNED: Self = Self(1);
    pub const NONTEMPORAL: Self = Self(2);
    pub const MAKE_POINTER_AVAILABLE: Self = Self(3);
    pub const MAKE_POINTER_AVAILABLE_KHR: Self = Self(Self::MAKE_POINTER_AVAILABLE.0);
    pub const MAKE_POINTER_VISIBLE: Self = Self(4);
    pub const MAKE_POINTER_VISIBLE_KHR: Self = Self(Self::MAKE_POINTER_VISIBLE.0);
    pub const NON_PRIVATE_POINTER: Self = Self(5);
    pub const NON_PRIVATE_POINTER_KHR: Self = Self(Self::NON_PRIVATE_POINTER.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemoryAccessMask(pub u32);

#[rustfmt::skip]
impl MemoryAccessMask {
    pub const MASK_NONE: Self = Self(0);
    pub const VOLATILE: Self = Self(0x00000001);
    pub const ALIGNED: Self = Self(0x00000002);
    pub const NONTEMPORAL: Self = Self(0x00000004);
    pub const MAKE_POINTER_AVAILABLE: Self = Self(0x00000008);
    pub const MAKE_POINTER_AVAILABLE_KHR: Self = Self(Self::MAKE_POINTER_AVAILABLE.0);
    pub const MAKE_POINTER_VISIBLE: Self = Self(0x00000010);
    pub const MAKE_POINTER_VISIBLE_KHR: Self = Self(Self::MAKE_POINTER_VISIBLE.0);
    pub const NON_PRIVATE_POINTER: Self = Self(0x00000020);
    pub const NON_PRIVATE_POINTER_KHR: Self = Self(Self::NON_PRIVATE_POINTER.0);
}

impl std::ops::BitOr for MemoryAccessMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Scope(pub u32);

#[rustfmt::skip]
impl Scope {
    pub const CROSS_DEVICE: Self = Self(0);
    pub const DEVICE: Self = Self(1);
    pub const WORKGROUP: Self = Self(2);
    pub const SUBGROUP: Self = Self(3);
    pub const INVOCATION: Self = Self(4);
    pub const QUEUE_FAMILY: Self = Self(5);
    pub const QUEUE_FAMILY_KHR: Self = Self(Self::QUEUE_FAMILY.0);
    pub const SHADER_CALL_KHR: Self = Self(6);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupOperation(pub u32);

#[rustfmt::skip]
impl GroupOperation {
    pub const REDUCE: Self = Self(0);
    pub const INCLUSIVE_SCAN: Self = Self(1);
    pub const EXCLUSIVE_SCAN: Self = Self(2);
    pub const CLUSTERED_REDUCE: Self = Self(3);
    pub const PARTITIONED_REDUCE_NV: Self = Self(6);
    pub const PARTITIONED_INCLUSIVE_SCAN_NV: Self = Self(7);
    pub const PARTITIONED_EXCLUSIVE_SCAN_NV: Self = Self(8);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KernelEnqueueFlags(pub u32);

#[rustfmt::skip]
impl KernelEnqueueFlags {
    pub const NO_WAIT: Self = Self(0);
    pub const WAIT_KERNEL: Self = Self(1);
    pub const WAIT_WORK_GROUP: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KernelProfilingInfoShift(pub u32);

#[rustfmt::skip]
impl KernelProfilingInfoShift {
    pub const CMD_EXEC_TIME: Self = Self(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KernelProfilingInfoMask(pub u32);

#[rustfmt::skip]
impl KernelProfilingInfoMask {
    pub const MASK_NONE: Self = Self(0);
    pub const CMD_EXEC_TIME: Self = Self(0x00000001);
}

impl std::ops::BitOr for KernelProfilingInfoMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Capability(pub u32);

#[rustfmt::skip]
impl Capability {
    pub const MATRIX: Self = Self(0);
    pub const SHADER: Self = Self(1);
    pub const GEOMETRY: Self = Self(2);
    pub const TESSELLATION: Self = Self(3);
    pub const ADDRESSES: Self = Self(4);
    pub const LINKAGE: Self = Self(5);
    pub const KERNEL: Self = Self(6);
    pub const VECTOR16: Self = Self(7);
    pub const FLOAT16_BUFFER: Self = Self(8);
    pub const FLOAT16: Self = Self(9);
    pub const FLOAT64: Self = Self(10);
    pub const INT64: Self = Self(11);
    pub const INT64_ATOMICS: Self = Self(12);
    pub const IMAGE_BASIC: Self = Self(13);
    pub const IMAGE_READ_WRITE: Self = Self(14);
    pub const IMAGE_MIPMAP: Self = Self(15);
    pub const PIPES: Self = Self(17);
    pub const GROUPS: Self = Self(18);
    pub const DEVICE_ENQUEUE: Self = Self(19);
    pub const LITERAL_SAMPLER: Self = Self(20);
    pub const ATOMIC_STORAGE: Self = Self(21);
    pub const INT16: Self = Self(22);
    pub const TESSELLATION_POINT_SIZE: Self = Self(23);
    pub const GEOMETRY_POINT_SIZE: Self = Self(24);
    pub const IMAGE_GATHER_EXTENDED: Self = Self(25);
    pub const STORAGE_IMAGE_MULTISAMPLE: Self = Self(27);
    pub const UNIFORM_BUFFER_ARRAY_DYNAMIC_INDEXING: Self = Self(28);
    pub const SAMPLED_IMAGE_ARRAY_DYNAMIC_INDEXING: Self = Self(29);
    pub const STORAGE_BUFFER_ARRAY_DYNAMIC_INDEXING: Self = Self(30);
    pub const STORAGE_IMAGE_ARRAY_DYNAMIC_INDEXING: Self = Self(31);
    pub const CLIP_DISTANCE: Self = Self(32);
    pub const CULL_DISTANCE: Self = Self(33);
    pub const IMAGE_CUBE_ARRAY: Self = Self(34);
    pub const SAMPLE_RATE_SHADING: Self = Self(35);
    pub const IMAGE_RECT: Self = Self(36);
    pub const SAMPLED_RECT: Self = Self(37);
    pub const GENERIC_POINTER: Self = Self(38);
    pub const INT8: Self = Self(39);
    pub const INPUT_ATTACHMENT: Self = Self(40);
    pub const SPARSE_RESIDENCY: Self = Self(41);
    pub const MIN_LOD: Self = Self(42);
    pub const SAMPLED1_D: Self = Self(43);
    pub const IMAGE1_D: Self = Self(44);
    pub const SAMPLED_CUBE_ARRAY: Self = Self(45);
    pub const SAMPLED_BUFFER: Self = Self(46);
    pub const IMAGE_BUFFER: Self = Self(47);
    pub const IMAGE_MS_ARRAY: Self = Self(48);
    pub const STORAGE_IMAGE_EXTENDED_FORMATS: Self = Self(49);
    pub const IMAGE_QUERY: Self = Self(50);
    pub const DERIVATIVE_CONTROL: Self = Self(51);
    pub const INTERPOLATION_FUNCTION: Self = Self(52);
    pub const TRANSFORM_FEEDBACK: Self = Self(53);
    pub const GEOMETRY_STREAMS: Self = Self(54);
    pub const STORAGE_IMAGE_READ_WITHOUT_FORMAT: Self = Self(55);
    pub const STORAGE_IMAGE_WRITE_WITHOUT_FORMAT: Self = Self(56);
    pub const MULTI_VIEWPORT: Self = Self(57);
    pub const SUBGROUP_DISPATCH: Self = Self(58);
    pub const NAMED_BARRIER: Self = Self(59);
    pub const PIPE_STORAGE: Self = Self(60);
    pub const GROUP_NON_UNIFORM: Self = Self(61);
    pub const GROUP_NON_UNIFORM_VOTE: Self = Self(62);
    pub const GROUP_NON_UNIFORM_ARITHMETIC: Self = Self(63);
    pub const GROUP_NON_UNIFORM_BALLOT: Self = Self(64);
    pub const GROUP_NON_UNIFORM_SHUFFLE: Self = Self(65);
    pub const GROUP_NON_UNIFORM_SHUFFLE_RELATIVE: Self = Self(66);
    pub const GROUP_NON_UNIFORM_CLUSTERED: Self = Self(67);
    pub const GROUP_NON_UNIFORM_QUAD: Self = Self(68);
    pub const SHADER_LAYER: Self = Self(69);
    pub const SHADER_VIEWPORT_INDEX: Self = Self(70);
    pub const FRAGMENT_SHADING_RATE_KHR: Self = Self(4422);
    pub const SUBGROUP_BALLOT_KHR: Self = Self(4423);
    pub const DRAW_PARAMETERS: Self = Self(4427);
    pub const SUBGROUP_VOTE_KHR: Self = Self(4431);
    pub const STORAGE_BUFFER16_BIT_ACCESS: Self = Self(4433);
    pub const STORAGE_UNIFORM_BUFFER_BLOCK16: Self = Self(Self::STORAGE_BUFFER16_BIT_ACCESS.0);
    pub const STORAGE_UNIFORM16: Self = Self(4434);
    pub const UNIFORM_AND_STORAGE_BUFFER16_BIT_ACCESS: Self = Self(Self::STORAGE_UNIFORM16.0);
    pub const STORAGE_PUSH_CONSTANT16: Self = Self(4435);
    pub const STORAGE_INPUT_OUTPUT16: Self = Self(4436);
    pub const DEVICE_GROUP: Self = Self(4437);
    pub const MULTI_VIEW: Self = Self(4439);
    pub const VARIABLE_POINTERS_STORAGE_BUFFER: Self = Self(4441);
    pub const VARIABLE_POINTERS: Self = Self(4442);
    pub const ATOMIC_STORAGE_OPS: Self = Self(4445);
    pub const SAMPLE_MASK_POST_DEPTH_COVERAGE: Self = Self(4447);
    pub const STORAGE_BUFFER8_BIT_ACCESS: Self = Self(4448);
    pub const UNIFORM_AND_STORAGE_BUFFER8_BIT_ACCESS: Self = Self(4449);
    pub const STORAGE_PUSH_CONSTANT8: Self = Self(4450);
    pub const DENORM_PRESERVE: Self = Self(4464);
    pub const DENORM_FLUSH_TO_ZERO: Self = Self(4465);
    pub const SIGNED_ZERO_INF_NAN_PRESERVE: Self = Self(4466);
    pub const ROUNDING_MODE_RTE: Self = Self(4467);
    pub const ROUNDING_MODE_RTZ: Self = Self(4468);
    pub const RAY_QUERY_PROVISIONAL_KHR: Self = Self(4471);
    pub const RAY_QUERY_KHR: Self = Self(4472);
    pub const RAY_TRAVERSAL_PRIMITIVE_CULLING_KHR: Self = Self(4478);
    pub const RAY_TRACING_KHR: Self = Self(4479);
    pub const FLOAT16_IMAGE_AMD: Self = Self(5008);
    pub const IMAGE_GATHER_BIAS_LOD_AMD: Self = Self(5009);
    pub const FRAGMENT_MASK_AMD: Self = Self(5010);
    pub const STENCIL_EXPORT_EXT: Self = Self(5013);
    pub const IMAGE_READ_WRITE_LOD_AMD: Self = Self(5015);
    pub const INT64_IMAGE_EXT: Self = Self(5016);
    pub const SHADER_CLOCK_KHR: Self = Self(5055);
    pub const SAMPLE_MASK_OVERRIDE_COVERAGE_NV: Self = Self(5249);
    pub const GEOMETRY_SHADER_PASSTHROUGH_NV: Self = Self(5251);
    pub const SHADER_VIEWPORT_INDEX_LAYER_EXT: Self = Self(5254);
    pub const SHADER_VIEWPORT_INDEX_LAYER_NV: Self = Self(Self::SHADER_VIEWPORT_INDEX_LAYER_EXT.0);
    pub const SHADER_VIEWPORT_MASK_NV: Self = Self(5255);
    pub const SHADER_STEREO_VIEW_NV: Self = Self(5259);
    pub const PER_VIEW_ATTRIBUTES_NV: Self = Self(5260);
    pub const FRAGMENT_FULLY_COVERED_EXT: Self = Self(5265);
    pub const MESH_SHADING_NV: Self = Self(5266);
    pub const IMAGE_FOOTPRINT_NV: Self = Self(5282);
    pub const FRAGMENT_BARYCENTRIC_NV: Self = Self(5284);
    pub const COMPUTE_DERIVATIVE_GROUP_QUADS_NV: Self = Self(5288);
    pub const FRAGMENT_DENSITY_EXT: Self = Self(5291);
    pub const SHADING_RATE_NV: Self = Self(Self::FRAGMENT_DENSITY_EXT.0);
    pub const GROUP_NON_UNIFORM_PARTITIONED_NV: Self = Self(5297);
    pub const SHADER_NON_UNIFORM: Self = Self(5301);
    pub const SHADER_NON_UNIFORM_EXT: Self = Self(Self::SHADER_NON_UNIFORM.0);
    pub const RUNTIME_DESCRIPTOR_ARRAY: Self = Self(5302);
    pub const RUNTIME_DESCRIPTOR_ARRAY_EXT: Self = Self(Self::RUNTIME_DESCRIPTOR_ARRAY.0);
    pub const INPUT_ATTACHMENT_ARRAY_DYNAMIC_INDEXING: Self = Self(5303);
    pub const INPUT_ATTACHMENT_ARRAY_DYNAMIC_INDEXING_EXT: Self = Self(Self::INPUT_ATTACHMENT_ARRAY_DYNAMIC_INDEXING.0);
    pub const UNIFORM_TEXEL_BUFFER_ARRAY_DYNAMIC_INDEXING: Self = Self(5304);
    pub const UNIFORM_TEXEL_BUFFER_ARRAY_DYNAMIC_INDEXING_EXT: Self = Self(Self::UNIFORM_TEXEL_BUFFER_ARRAY_DYNAMIC_INDEXING.0);
    pub const STORAGE_TEXEL_BUFFER_ARRAY_DYNAMIC_INDEXING: Self = Self(5305);
    pub const STORAGE_TEXEL_BUFFER_ARRAY_DYNAMIC_INDEXING_EXT: Self = Self(Self::STORAGE_TEXEL_BUFFER_ARRAY_DYNAMIC_INDEXING.0);
    pub const UNIFORM_BUFFER_ARRAY_NON_UNIFORM_INDEXING: Self = Self(5306);
    pub const UNIFORM_BUFFER_ARRAY_NON_UNIFORM_INDEXING_EXT: Self = Self(Self::UNIFORM_BUFFER_ARRAY_NON_UNIFORM_INDEXING.0);
    pub const SAMPLED_IMAGE_ARRAY_NON_UNIFORM_INDEXING: Self = Self(5307);
    pub const SAMPLED_IMAGE_ARRAY_NON_UNIFORM_INDEXING_EXT: Self = Self(Self::SAMPLED_IMAGE_ARRAY_NON_UNIFORM_INDEXING.0);
    pub const STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING: Self = Self(5308);
    pub const STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING_EXT: Self = Self(Self::STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING.0);
    pub const STORAGE_IMAGE_ARRAY_NON_UNIFORM_INDEXING: Self = Self(5309);
    pub const STORAGE_IMAGE_ARRAY_NON_UNIFORM_INDEXING_EXT: Self = Self(Self::STORAGE_IMAGE_ARRAY_NON_UNIFORM_INDEXING.0);
    pub const INPUT_ATTACHMENT_ARRAY_NON_UNIFORM_INDEXING: Self = Self(5310);
    pub const INPUT_ATTACHMENT_ARRAY_NON_UNIFORM_INDEXING_EXT: Self = Self(Self::INPUT_ATTACHMENT_ARRAY_NON_UNIFORM_INDEXING.0);
    pub const UNIFORM_TEXEL_BUFFER_ARRAY_NON_UNIFORM_INDEXING: Self = Self(5311);
    pub const UNIFORM_TEXEL_BUFFER_ARRAY_NON_UNIFORM_INDEXING_EXT: Self = Self(Self::UNIFORM_TEXEL_BUFFER_ARRAY_NON_UNIFORM_INDEXING.0);
    pub const STORAGE_TEXEL_BUFFER_ARRAY_NON_UNIFORM_INDEXING: Self = Self(5312);
    pub const STORAGE_TEXEL_BUFFER_ARRAY_NON_UNIFORM_INDEXING_EXT: Self = Self(Self::STORAGE_TEXEL_BUFFER_ARRAY_NON_UNIFORM_INDEXING.0);
    pub const RAY_TRACING_NV: Self = Self(5340);
    pub const VULKAN_MEMORY_MODEL: Self = Self(5345);
    pub const VULKAN_MEMORY_MODEL_KHR: Self = Self(Self::VULKAN_MEMORY_MODEL.0);
    pub const VULKAN_MEMORY_MODEL_DEVICE_SCOPE: Self = Self(5346);
    pub const VULKAN_MEMORY_MODEL_DEVICE_SCOPE_KHR: Self = Self(Self::VULKAN_MEMORY_MODEL_DEVICE_SCOPE.0);
    pub const PHYSICAL_STORAGE_BUFFER_ADDRESSES: Self = Self(5347);
    pub const PHYSICAL_STORAGE_BUFFER_ADDRESSES_EXT: Self = Self(Self::PHYSICAL_STORAGE_BUFFER_ADDRESSES.0);
    pub const COMPUTE_DERIVATIVE_GROUP_LINEAR_NV: Self = Self(5350);
    pub const RAY_TRACING_PROVISIONAL_KHR: Self = Self(5353);
    pub const COOPERATIVE_MATRIX_NV: Self = Self(5357);
    pub const FRAGMENT_SHADER_SAMPLE_INTERLOCK_EXT: Self = Self(5363);
    pub const FRAGMENT_SHADER_SHADING_RATE_INTERLOCK_EXT: Self = Self(5372);
    pub const SHADER_SM_BUILTINS_NV: Self = Self(5373);
    pub const FRAGMENT_SHADER_PIXEL_INTERLOCK_EXT: Self = Self(5378);
    pub const DEMOTE_TO_HELPER_INVOCATION_EXT: Self = Self(5379);
    pub const SUBGROUP_SHUFFLE_INTEL: Self = Self(5568);
    pub const SUBGROUP_BUFFER_BLOCK_IOINTEL: Self = Self(5569);
    pub const SUBGROUP_IMAGE_BLOCK_IOINTEL: Self = Self(5570);
    pub const SUBGROUP_IMAGE_MEDIA_BLOCK_IOINTEL: Self = Self(5579);
    pub const INTEGER_FUNCTIONS2_INTEL: Self = Self(5584);
    pub const FUNCTION_POINTERS_INTEL: Self = Self(5603);
    pub const INDIRECT_REFERENCES_INTEL: Self = Self(5604);
    pub const SUBGROUP_AVC_MOTION_ESTIMATION_INTEL: Self = Self(5696);
    pub const SUBGROUP_AVC_MOTION_ESTIMATION_INTRA_INTEL: Self = Self(5697);
    pub const SUBGROUP_AVC_MOTION_ESTIMATION_CHROMA_INTEL: Self = Self(5698);
    pub const FPGA_MEMORY_ATTRIBUTES_INTEL: Self = Self(5824);
    pub const UNSTRUCTURED_LOOP_CONTROLS_INTEL: Self = Self(5886);
    pub const FPGA_LOOP_CONTROLS_INTEL: Self = Self(5888);
    pub const KERNEL_ATTRIBUTES_INTEL: Self = Self(5892);
    pub const FPGA_KERNEL_ATTRIBUTES_INTEL: Self = Self(5897);
    pub const BLOCKING_PIPES_INTEL: Self = Self(5945);
    pub const FPGA_REG_INTEL: Self = Self(5948);
    pub const ATOMIC_FLOAT32_ADD_EXT: Self = Self(6033);
    pub const ATOMIC_FLOAT64_ADD_EXT: Self = Self(6034);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RayFlagsShift(pub u32);

#[rustfmt::skip]
impl RayFlagsShift {
    pub const OPAQUE_KHR: Self = Self(0);
    pub const NO_OPAQUE_KHR: Self = Self(1);
    pub const TERMINATE_ON_FIRST_HIT_KHR: Self = Self(2);
    pub const SKIP_CLOSEST_HIT_SHADER_KHR: Self = Self(3);
    pub const CULL_BACK_FACING_TRIANGLES_KHR: Self = Self(4);
    pub const CULL_FRONT_FACING_TRIANGLES_KHR: Self = Self(5);
    pub const CULL_OPAQUE_KHR: Self = Self(6);
    pub const CULL_NO_OPAQUE_KHR: Self = Self(7);
    pub const SKIP_TRIANGLES_KHR: Self = Self(8);
    pub const SKIP_AAB_BS_KHR: Self = Self(9);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RayFlagsMask(pub u32);

#[rustfmt::skip]
impl RayFlagsMask {
    pub const MASK_NONE: Self = Self(0);
    pub const OPAQUE_KHR: Self = Self(0x00000001);
    pub const NO_OPAQUE_KHR: Self = Self(0x00000002);
    pub const TERMINATE_ON_FIRST_HIT_KHR: Self = Self(0x00000004);
    pub const SKIP_CLOSEST_HIT_SHADER_KHR: Self = Self(0x00000008);
    pub const CULL_BACK_FACING_TRIANGLES_KHR: Self = Self(0x00000010);
    pub const CULL_FRONT_FACING_TRIANGLES_KHR: Self = Self(0x00000020);
    pub const CULL_OPAQUE_KHR: Self = Self(0x00000040);
    pub const CULL_NO_OPAQUE_KHR: Self = Self(0x00000080);
    pub const SKIP_TRIANGLES_KHR: Self = Self(0x00000100);
    pub const SKIP_AAB_BS_KHR: Self = Self(0x00000200);
}

impl std::ops::BitOr for RayFlagsMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RayQueryIntersection(pub u32);

#[rustfmt::skip]
impl RayQueryIntersection {
    pub const RAY_QUERY_CANDIDATE_INTERSECTION_KHR: Self = Self(0);
    pub const RAY_QUERY_COMMITTED_INTERSECTION_KHR: Self = Self(1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RayQueryCommittedIntersectionType(pub u32);

#[rustfmt::skip]
impl RayQueryCommittedIntersectionType {
    pub const RAY_QUERY_COMMITTED_INTERSECTION_NONE_KHR: Self = Self(0);
    pub const RAY_QUERY_COMMITTED_INTERSECTION_TRIANGLE_KHR: Self = Self(1);
    pub const RAY_QUERY_COMMITTED_INTERSECTION_GENERATED_KHR: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RayQueryCandidateIntersectionType(pub u32);

#[rustfmt::skip]
impl RayQueryCandidateIntersectionType {
    pub const RAY_QUERY_CANDIDATE_INTERSECTION_TRIANGLE_KHR: Self = Self(0);
    pub const RAY_QUERY_CANDIDATE_INTERSECTION_AABBKHR: Self = Self(1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FragmentShadingRateShift(pub u32);

#[rustfmt::skip]
impl FragmentShadingRateShift {
    pub const VERTICAL2_PIXELS: Self = Self(0);
    pub const VERTICAL4_PIXELS: Self = Self(1);
    pub const HORIZONTAL2_PIXELS: Self = Self(2);
    pub const HORIZONTAL4_PIXELS: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FragmentShadingRateMask(pub u32);

#[rustfmt::skip]
impl FragmentShadingRateMask {
    pub const MASK_NONE: Self = Self(0);
    pub const VERTICAL2_PIXELS: Self = Self(0x00000001);
    pub const VERTICAL4_PIXELS: Self = Self(0x00000002);
    pub const HORIZONTAL2_PIXELS: Self = Self(0x00000004);
    pub const HORIZONTAL4_PIXELS: Self = Self(0x00000008);
}

impl std::ops::BitOr for FragmentShadingRateMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Op(pub u32);

#[rustfmt::skip]
impl Op {
    pub const NOP: Self = Self(0);
    pub const UNDEF: Self = Self(1);
    pub const SOURCE_CONTINUED: Self = Self(2);
    pub const SOURCE: Self = Self(3);
    pub const SOURCE_EXTENSION: Self = Self(4);
    pub const NAME: Self = Self(5);
    pub const MEMBER_NAME: Self = Self(6);
    pub const STRING: Self = Self(7);
    pub const LINE: Self = Self(8);
    pub const EXTENSION: Self = Self(10);
    pub const EXT_INST_IMPORT: Self = Self(11);
    pub const EXT_INST: Self = Self(12);
    pub const MEMORY_MODEL: Self = Self(14);
    pub const ENTRY_POINT: Self = Self(15);
    pub const EXECUTION_MODE: Self = Self(16);
    pub const CAPABILITY: Self = Self(17);
    pub const TYPE_VOID: Self = Self(19);
    pub const TYPE_BOOL: Self = Self(20);
    pub const TYPE_INT: Self = Self(21);
    pub const TYPE_FLOAT: Self = Self(22);
    pub const TYPE_VECTOR: Self = Self(23);
    pub const TYPE_MATRIX: Self = Self(24);
    pub const TYPE_IMAGE: Self = Self(25);
    pub const TYPE_SAMPLER: Self = Self(26);
    pub const TYPE_SAMPLED_IMAGE: Self = Self(27);
    pub const TYPE_ARRAY: Self = Self(28);
    pub const TYPE_RUNTIME_ARRAY: Self = Self(29);
    pub const TYPE_STRUCT: Self = Self(30);
    pub const TYPE_OPAQUE: Self = Self(31);
    pub const TYPE_POINTER: Self = Self(32);
    pub const TYPE_FUNCTION: Self = Self(33);
    pub const TYPE_EVENT: Self = Self(34);
    pub const TYPE_DEVICE_EVENT: Self = Self(35);
    pub const TYPE_RESERVE_ID: Self = Self(36);
    pub const TYPE_QUEUE: Self = Self(37);
    pub const TYPE_PIPE: Self = Self(38);
    pub const TYPE_FORWARD_POINTER: Self = Self(39);
    pub const CONSTANT_TRUE: Self = Self(41);
    pub const CONSTANT_FALSE: Self = Self(42);
    pub const CONSTANT: Self = Self(43);
    pub const CONSTANT_COMPOSITE: Self = Self(44);
    pub const CONSTANT_SAMPLER: Self = Self(45);
    pub const CONSTANT_NULL: Self = Self(46);
    pub const SPEC_CONSTANT_TRUE: Self = Self(48);
    pub const SPEC_CONSTANT_FALSE: Self = Self(49);
    pub const SPEC_CONSTANT: Self = Self(50);
    pub const SPEC_CONSTANT_COMPOSITE: Self = Self(51);
    pub const SPEC_CONSTANT_OP: Self = Self(52);
    pub const FUNCTION: Self = Self(54);
    pub const FUNCTION_PARAMETER: Self = Self(55);
    pub const FUNCTION_END: Self = Self(56);
    pub const FUNCTION_CALL: Self = Self(57);
    pub const VARIABLE: Self = Self(59);
    pub const IMAGE_TEXEL_POINTER: Self = Self(60);
    pub const LOAD: Self = Self(61);
    pub const STORE: Self = Self(62);
    pub const COPY_MEMORY: Self = Self(63);
    pub const COPY_MEMORY_SIZED: Self = Self(64);
    pub const ACCESS_CHAIN: Self = Self(65);
    pub const IN_BOUNDS_ACCESS_CHAIN: Self = Self(66);
    pub const PTR_ACCESS_CHAIN: Self = Self(67);
    pub const ARRAY_LENGTH: Self = Self(68);
    pub const GENERIC_PTR_MEM_SEMANTICS: Self = Self(69);
    pub const IN_BOUNDS_PTR_ACCESS_CHAIN: Self = Self(70);
    pub const DECORATE: Self = Self(71);
    pub const MEMBER_DECORATE: Self = Self(72);
    pub const DECORATION_GROUP: Self = Self(73);
    pub const GROUP_DECORATE: Self = Self(74);
    pub const GROUP_MEMBER_DECORATE: Self = Self(75);
    pub const VECTOR_EXTRACT_DYNAMIC: Self = Self(77);
    pub const VECTOR_INSERT_DYNAMIC: Self = Self(78);
    pub const VECTOR_SHUFFLE: Self = Self(79);
    pub const COMPOSITE_CONSTRUCT: Self = Self(80);
    pub const COMPOSITE_EXTRACT: Self = Self(81);
    pub const COMPOSITE_INSERT: Self = Self(82);
    pub const COPY_OBJECT: Self = Self(83);
    pub const TRANSPOSE: Self = Self(84);
    pub const SAMPLED_IMAGE: Self = Self(86);
    pub const IMAGE_SAMPLE_IMPLICIT_LOD: Self = Self(87);
    pub const IMAGE_SAMPLE_EXPLICIT_LOD: Self = Self(88);
    pub const IMAGE_SAMPLE_DREF_IMPLICIT_LOD: Self = Self(89);
    pub const IMAGE_SAMPLE_DREF_EXPLICIT_LOD: Self = Self(90);
    pub const IMAGE_SAMPLE_PROJ_IMPLICIT_LOD: Self = Self(91);
    pub const IMAGE_SAMPLE_PROJ_EXPLICIT_LOD: Self = Self(92);
    pub const IMAGE_SAMPLE_PROJ_DREF_IMPLICIT_LOD: Self = Self(93);
    pub const IMAGE_SAMPLE_PROJ_DREF_EXPLICIT_LOD: Self = Self(94);
    pub const IMAGE_FETCH: Self = Self(95);
    pub const IMAGE_GATHER: Self = Self(96);
    pub const IMAGE_DREF_GATHER: Self = Self(97);
    pub const IMAGE_READ: Self = Self(98);
    pub const IMAGE_WRITE: Self = Self(99);
    pub const IMAGE: Self = Self(100);
    pub const IMAGE_QUERY_FORMAT: Self = Self(101);
    pub const IMAGE_QUERY_ORDER: Self = Self(102);
    pub const IMAGE_QUERY_SIZE_LOD: Self = Self(103);
    pub const IMAGE_QUERY_SIZE: Self = Self(104);
    pub const IMAGE_QUERY_LOD: Self = Self(105);
    pub const IMAGE_QUERY_LEVELS: Self = Self(106);
    pub const IMAGE_QUERY_SAMPLES: Self = Self(107);
    pub const CONVERT_F_TO_U: Self = Self(109);
    pub const CONVERT_F_TO_S: Self = Self(110);
    pub const CONVERT_S_TO_F: Self = Self(111);
    pub const CONVERT_U_TO_F: Self = Self(112);
    pub const U_CONVERT: Self = Self(113);
    pub const S_CONVERT: Self = Self(114);
    pub const F_CONVERT: Self = Self(115);
    pub const QUANTIZE_TO_F16: Self = Self(116);
    pub const CONVERT_PTR_TO_U: Self = Self(117);
    pub const SAT_CONVERT_S_TO_U: Self = Self(118);
    pub const SAT_CONVERT_U_TO_S: Self = Self(119);
    pub const CONVERT_U_TO_PTR: Self = Self(120);
    pub const PTR_CAST_TO_GENERIC: Self = Self(121);
    pub const GENERIC_CAST_TO_PTR: Self = Self(122);
    pub const GENERIC_CAST_TO_PTR_EXPLICIT: Self = Self(123);
    pub const BITCAST: Self = Self(124);
    pub const S_NEGATE: Self = Self(126);
    pub const F_NEGATE: Self = Self(127);
    pub const I_ADD: Self = Self(128);
    pub const F_ADD: Self = Self(129);
    pub const I_SUB: Self = Self(130);
    pub const F_SUB: Self = Self(131);
    pub const I_MUL: Self = Self(132);
    pub const F_MUL: Self = Self(133);
    pub const U_DIV: Self = Self(134);
    pub const S_DIV: Self = Self(135);
    pub const F_DIV: Self = Self(136);
    pub const U_MOD: Self = Self(137);
    pub const S_REM: Self = Self(138);
    pub const S_MOD: Self = Self(139);
    pub const F_REM: Self = Self(140);
    pub const F_MOD: Self = Self(141);
    pub const VECTOR_TIMES_SCALAR: Self = Self(142);
    pub const MATRIX_TIMES_SCALAR: Self = Self(143);
    pub const VECTOR_TIMES_MATRIX: Self = Self(144);
    pub const MATRIX_TIMES_VECTOR: Self = Self(145);
    pub const MATRIX_TIMES_MATRIX: Self = Self(146);
    pub const OUTER_PRODUCT: Self = Self(147);
    pub const DOT: Self = Self(148);
    pub const I_ADD_CARRY: Self = Self(149);
    pub const I_SUB_BORROW: Self = Self(150);
    pub const U_MUL_EXTENDED: Self = Self(151);
    pub const S_MUL_EXTENDED: Self = Self(152);
    pub const ANY: Self = Self(154);
    pub const ALL: Self = Self(155);
    pub const IS_NAN: Self = Self(156);
    pub const IS_INF: Self = Self(157);
    pub const IS_FINITE: Self = Self(158);
    pub const IS_NORMAL: Self = Self(159);
    pub const SIGN_BIT_SET: Self = Self(160);
    pub const LESS_OR_GREATER: Self = Self(161);
    pub const ORDERED: Self = Self(162);
    pub const UNORDERED: Self = Self(163);
    pub const LOGICAL_EQUAL: Self = Self(164);
    pub const LOGICAL_NOT_EQUAL: Self = Self(165);
    pub const LOGICAL_OR: Self = Self(166);
    pub const LOGICAL_AND: Self = Self(167);
    pub const LOGICAL_NOT: Self = Self(168);
    pub const SELECT: Self = Self(169);
    pub const I_EQUAL: Self = Self(170);
    pub const I_NOT_EQUAL: Self = Self(171);
    pub const U_GREATER_THAN: Self = Self(172);
    pub const S_GREATER_THAN: Self = Self(173);
    pub const U_GREATER_THAN_EQUAL: Self = Self(174);
    pub const S_GREATER_THAN_EQUAL: Self = Self(175);
    pub const U_LESS_THAN: Self = Self(176);
    pub const S_LESS_THAN: Self = Self(177);
    pub const U_LESS_THAN_EQUAL: Self = Self(178);
    pub const S_LESS_THAN_EQUAL: Self = Self(179);
    pub const F_ORD_EQUAL: Self = Self(180);
    pub const F_UNORD_EQUAL: Self = Self(181);
    pub const F_ORD_NOT_EQUAL: Self = Self(182);
    pub const F_UNORD_NOT_EQUAL: Self = Self(183);
    pub const F_ORD_LESS_THAN: Self = Self(184);
    pub const F_UNORD_LESS_THAN: Self = Self(185);
    pub const F_ORD_GREATER_THAN: Self = Self(186);
    pub const F_UNORD_GREATER_THAN: Self = Self(187);
    pub const F_ORD_LESS_THAN_EQUAL: Self = Self(188);
    pub const F_UNORD_LESS_THAN_EQUAL: Self = Self(189);
    pub const F_ORD_GREATER_THAN_EQUAL: Self = Self(190);
    pub const F_UNORD_GREATER_THAN_EQUAL: Self = Self(191);
    pub const SHIFT_RIGHT_LOGICAL: Self = Self(194);
    pub const SHIFT_RIGHT_ARITHMETIC: Self = Self(195);
    pub const SHIFT_LEFT_LOGICAL: Self = Self(196);
    pub const BITWISE_OR: Self = Self(197);
    pub const BITWISE_XOR: Self = Self(198);
    pub const BITWISE_AND: Self = Self(199);
    pub const NOT: Self = Self(200);
    pub const BIT_FIELD_INSERT: Self = Self(201);
    pub const BIT_FIELD_S_EXTRACT: Self = Self(202);
    pub const BIT_FIELD_U_EXTRACT: Self = Self(203);
    pub const BIT_REVERSE: Self = Self(204);
    pub const BIT_COUNT: Self = Self(205);
    pub const D_PDX: Self = Self(207);
    pub const D_PDY: Self = Self(208);
    pub const FWIDTH: Self = Self(209);
    pub const D_PDX_FINE: Self = Self(210);
    pub const D_PDY_FINE: Self = Self(211);
    pub const FWIDTH_FINE: Self = Self(212);
    pub const D_PDX_COARSE: Self = Self(213);
    pub const D_PDY_COARSE: Self = Self(214);
    pub const FWIDTH_COARSE: Self = Self(215);
    pub const EMIT_VERTEX: Self = Self(218);
    pub const END_PRIMITIVE: Self = Self(219);
    pub const EMIT_STREAM_VERTEX: Self = Self(220);
    pub const END_STREAM_PRIMITIVE: Self = Self(221);
    pub const CONTROL_BARRIER: Self = Self(224);
    pub const MEMORY_BARRIER: Self = Self(225);
    pub const ATOMIC_LOAD: Self = Self(227);
    pub const ATOMIC_STORE: Self = Self(228);
    pub const ATOMIC_EXCHANGE: Self = Self(229);
    pub const ATOMIC_COMPARE_EXCHANGE: Self = Self(230);
    pub const ATOMIC_COMPARE_EXCHANGE_WEAK: Self = Self(231);
    pub const ATOMIC_I_INCREMENT: Self = Self(232);
    pub const ATOMIC_I_DECREMENT: Self = Self(233);
    pub const ATOMIC_I_ADD: Self = Self(234);
    pub const ATOMIC_I_SUB: Self = Self(235);
    pub const ATOMIC_S_MIN: Self = Self(236);
    pub const ATOMIC_U_MIN: Self = Self(237);
    pub const ATOMIC_S_MAX: Self = Self(238);
    pub const ATOMIC_U_MAX: Self = Self(239);
    pub const ATOMIC_AND: Self = Self(240);
    pub const ATOMIC_OR: Self = Self(241);
    pub const ATOMIC_XOR: Self = Self(242);
    pub const PHI: Self = Self(245);
    pub const LOOP_MERGE: Self = Self(246);
    pub const SELECTION_MERGE: Self = Self(247);
    pub const LABEL: Self = Self(248);
    pub const BRANCH: Self = Self(249);
    pub const BRANCH_CONDITIONAL: Self = Self(250);
    pub const SWITCH: Self = Self(251);
    pub const KILL: Self = Self(252);
    pub const RETURN: Self = Self(253);
    pub const RETURN_VALUE: Self = Self(254);
    pub const UNREACHABLE: Self = Self(255);
    pub const LIFETIME_START: Self = Self(256);
    pub const LIFETIME_STOP: Self = Self(257);
    pub const GROUP_ASYNC_COPY: Self = Self(259);
    pub const GROUP_WAIT_EVENTS: Self = Self(260);
    pub const GROUP_ALL: Self = Self(261);
    pub const GROUP_ANY: Self = Self(262);
    pub const GROUP_BROADCAST: Self = Self(263);
    pub const GROUP_I_ADD: Self = Self(264);
    pub const GROUP_F_ADD: Self = Self(265);
    pub const GROUP_F_MIN: Self = Self(266);
    pub const GROUP_U_MIN: Self = Self(267);
    pub const GROUP_S_MIN: Self = Self(268);
    pub const GROUP_F_MAX: Self = Self(269);
    pub const GROUP_U_MAX: Self = Self(270);
    pub const GROUP_S_MAX: Self = Self(271);
    pub const READ_PIPE: Self = Self(274);
    pub const WRITE_PIPE: Self = Self(275);
    pub const RESERVED_READ_PIPE: Self = Self(276);
    pub const RESERVED_WRITE_PIPE: Self = Self(277);
    pub const RESERVE_READ_PIPE_PACKETS: Self = Self(278);
    pub const RESERVE_WRITE_PIPE_PACKETS: Self = Self(279);
    pub const COMMIT_READ_PIPE: Self = Self(280);
    pub const COMMIT_WRITE_PIPE: Self = Self(281);
    pub const IS_VALID_RESERVE_ID: Self = Self(282);
    pub const GET_NUM_PIPE_PACKETS: Self = Self(283);
    pub const GET_MAX_PIPE_PACKETS: Self = Self(284);
    pub const GROUP_RESERVE_READ_PIPE_PACKETS: Self = Self(285);
    pub const GROUP_RESERVE_WRITE_PIPE_PACKETS: Self = Self(286);
    pub const GROUP_COMMIT_READ_PIPE: Self = Self(287);
    pub const GROUP_COMMIT_WRITE_PIPE: Self = Self(288);
    pub const ENQUEUE_MARKER: Self = Self(291);
    pub const ENQUEUE_KERNEL: Self = Self(292);
    pub const GET_KERNEL_N_DRANGE_SUB_GROUP_COUNT: Self = Self(293);
    pub const GET_KERNEL_N_DRANGE_MAX_SUB_GROUP_SIZE: Self = Self(294);
    pub const GET_KERNEL_WORK_GROUP_SIZE: Self = Self(295);
    pub const GET_KERNEL_PREFERRED_WORK_GROUP_SIZE_MULTIPLE: Self = Self(296);
    pub const RETAIN_EVENT: Self = Self(297);
    pub const RELEASE_EVENT: Self = Self(298);
    pub const CREATE_USER_EVENT: Self = Self(299);
    pub const IS_VALID_EVENT: Self = Self(300);
    pub const SET_USER_EVENT_STATUS: Self = Self(301);
    pub const CAPTURE_EVENT_PROFILING_INFO: Self = Self(302);
    pub const GET_DEFAULT_QUEUE: Self = Self(303);
    pub const BUILD_ND_RANGE: Self = Self(304);
    pub const IMAGE_SPARSE_SAMPLE_IMPLICIT_LOD: Self = Self(305);
    pub const IMAGE_SPARSE_SAMPLE_EXPLICIT_LOD: Self = Self(306);
    pub const IMAGE_SPARSE_SAMPLE_DREF_IMPLICIT_LOD: Self = Self(307);
    pub const IMAGE_SPARSE_SAMPLE_DREF_EXPLICIT_LOD: Self = Self(308);
    pub const IMAGE_SPARSE_SAMPLE_PROJ_IMPLICIT_LOD: Self = Self(309);
    pub const IMAGE_SPARSE_SAMPLE_PROJ_EXPLICIT_LOD: Self = Self(310);
    pub const IMAGE_SPARSE_SAMPLE_PROJ_DREF_IMPLICIT_LOD: Self = Self(311);
    pub const IMAGE_SPARSE_SAMPLE_PROJ_DREF_EXPLICIT_LOD: Self = Self(312);
    pub const IMAGE_SPARSE_FETCH: Self = Self(313);
    pub const IMAGE_SPARSE_GATHER: Self = Self(314);
    pub const IMAGE_SPARSE_DREF_GATHER: Self = Self(315);
    pub const IMAGE_SPARSE_TEXELS_RESIDENT: Self = Self(316);
    pub const NO_LINE: Self = Self(317);
    pub const ATOMIC_FLAG_TEST_AND_SET: Self = Self(318);
    pub const ATOMIC_FLAG_CLEAR: Self = Self(319);
    pub const IMAGE_SPARSE_READ: Self = Self(320);
    pub const SIZE_OF: Self = Self(321);
    pub const TYPE_PIPE_STORAGE: Self = Self(322);
    pub const CONSTANT_PIPE_STORAGE: Self = Self(323);
    pub const CREATE_PIPE_FROM_PIPE_STORAGE: Self = Self(324);
    pub const GET_KERNEL_LOCAL_SIZE_FOR_SUBGROUP_COUNT: Self = Self(325);
    pub const GET_KERNEL_MAX_NUM_SUBGROUPS: Self = Self(326);
    pub const TYPE_NAMED_BARRIER: Self = Self(327);
    pub const NAMED_BARRIER_INITIALIZE: Self = Self(328);
    pub const MEMORY_NAMED_BARRIER: Self = Self(329);
    pub const MODULE_PROCESSED: Self = Self(330);
    pub const EXECUTION_MODE_ID: Self = Self(331);
    pub const DECORATE_ID: Self = Self(332);
    pub const GROUP_NON_UNIFORM_ELECT: Self = Self(333);
    pub const GROUP_NON_UNIFORM_ALL: Self = Self(334);
    pub const GROUP_NON_UNIFORM_ANY: Self = Self(335);
    pub const GROUP_NON_UNIFORM_ALL_EQUAL: Self = Self(336);
    pub const GROUP_NON_UNIFORM_BROADCAST: Self = Self(337);
    pub const GROUP_NON_UNIFORM_BROADCAST_FIRST: Self = Self(338);
    pub const GROUP_NON_UNIFORM_BALLOT: Self = Self(339);
    pub const GROUP_NON_UNIFORM_INVERSE_BALLOT: Self = Self(340);
    pub const GROUP_NON_UNIFORM_BALLOT_BIT_EXTRACT: Self = Self(341);
    pub const GROUP_NON_UNIFORM_BALLOT_BIT_COUNT: Self = Self(342);
    pub const GROUP_NON_UNIFORM_BALLOT_FIND_LSB: Self = Self(343);
    pub const GROUP_NON_UNIFORM_BALLOT_FIND_MSB: Self = Self(344);
    pub const GROUP_NON_UNIFORM_SHUFFLE: Self = Self(345);
    pub const GROUP_NON_UNIFORM_SHUFFLE_XOR: Self = Self(346);
    pub const GROUP_NON_UNIFORM_SHUFFLE_UP: Self = Self(347);
    pub const GROUP_NON_UNIFORM_SHUFFLE_DOWN: Self = Self(348);
    pub const GROUP_NON_UNIFORM_I_ADD: Self = Self(349);
    pub const GROUP_NON_UNIFORM_F_ADD: Self = Self(350);
    pub const GROUP_NON_UNIFORM_I_MUL: Self = Self(351);
    pub const GROUP_NON_UNIFORM_F_MUL: Self = Self(352);
    pub const GROUP_NON_UNIFORM_S_MIN: Self = Self(353);
    pub const GROUP_NON_UNIFORM_U_MIN: Self = Self(354);
    pub const GROUP_NON_UNIFORM_F_MIN: Self = Self(355);
    pub const GROUP_NON_UNIFORM_S_MAX: Self = Self(356);
    pub const GROUP_NON_UNIFORM_U_MAX: Self = Self(357);
    pub const GROUP_NON_UNIFORM_F_MAX: Self = Self(358);
    pub const GROUP_NON_UNIFORM_BITWISE_AND: Self = Self(359);
    pub const GROUP_NON_UNIFORM_BITWISE_OR: Self = Self(360);
    pub const GROUP_NON_UNIFORM_BITWISE_XOR: Self = Self(361);
    pub const GROUP_NON_UNIFORM_LOGICAL_AND: Self = Self(362);
    pub const GROUP_NON_UNIFORM_LOGICAL_OR: Self = Self(363);
    pub const GROUP_NON_UNIFORM_LOGICAL_XOR: Self = Self(364);
    pub const GROUP_NON_UNIFORM_QUAD_BROADCAST: Self = Self(365);
    pub const GROUP_NON_UNIFORM_QUAD_SWAP: Self = Self(366);
    pub const COPY_LOGICAL: Self = Self(400);
    pub const PTR_EQUAL: Self = Self(401);
    pub const PTR_NOT_EQUAL: Self = Self(402);
    pub const PTR_DIFF: Self = Self(403);
    pub const TERMINATE_INVOCATION: Self = Self(4416);
    pub const SUBGROUP_BALLOT_KHR: Self = Self(4421);
    pub const SUBGROUP_FIRST_INVOCATION_KHR: Self = Self(4422);
    pub const SUBGROUP_ALL_KHR: Self = Self(4428);
    pub const SUBGROUP_ANY_KHR: Self = Self(4429);
    pub const SUBGROUP_ALL_EQUAL_KHR: Self = Self(4430);
    pub const SUBGROUP_READ_INVOCATION_KHR: Self = Self(4432);
    pub const TRACE_RAY_KHR: Self = Self(4445);
    pub const EXECUTE_CALLABLE_KHR: Self = Self(4446);
    pub const CONVERT_U_TO_ACCELERATION_STRUCTURE_KHR: Self = Self(4447);
    pub const IGNORE_INTERSECTION_KHR: Self = Self(4448);
    pub const TERMINATE_RAY_KHR: Self = Self(4449);
    pub const TYPE_RAY_QUERY_KHR: Self = Self(4472);
    pub const RAY_QUERY_INITIALIZE_KHR: Self = Self(4473);
    pub const RAY_QUERY_TERMINATE_KHR: Self = Self(4474);
    pub const RAY_QUERY_GENERATE_INTERSECTION_KHR: Self = Self(4475);
    pub const RAY_QUERY_CONFIRM_INTERSECTION_KHR: Self = Self(4476);
    pub const RAY_QUERY_PROCEED_KHR: Self = Self(4477);
    pub const RAY_QUERY_GET_INTERSECTION_TYPE_KHR: Self = Self(4479);
    pub const GROUP_I_ADD_NON_UNIFORM_AMD: Self = Self(5000);
    pub const GROUP_F_ADD_NON_UNIFORM_AMD: Self = Self(5001);
    pub const GROUP_F_MIN_NON_UNIFORM_AMD: Self = Self(5002);
    pub const GROUP_U_MIN_NON_UNIFORM_AMD: Self = Self(5003);
    pub const GROUP_S_MIN_NON_UNIFORM_AMD: Self = Self(5004);
    pub const GROUP_F_MAX_NON_UNIFORM_AMD: Self = Self(5005);
    pub const GROUP_U_MAX_NON_UNIFORM_AMD: Self = Self(5006);
    pub const GROUP_S_MAX_NON_UNIFORM_AMD: Self = Self(5007);
    pub const FRAGMENT_MASK_FETCH_AMD: Self = Self(5011);
    pub const FRAGMENT_FETCH_AMD: Self = Self(5012);
    pub const READ_CLOCK_KHR: Self = Self(5056);
    pub const IMAGE_SAMPLE_FOOTPRINT_NV: Self = Self(5283);
    pub const GROUP_NON_UNIFORM_PARTITION_NV: Self = Self(5296);
    pub const WRITE_PACKED_PRIMITIVE_INDICES4X8_NV: Self = Self(5299);
    pub const REPORT_INTERSECTION_KHR: Self = Self(5334);
    pub const REPORT_INTERSECTION_NV: Self = Self(Self::REPORT_INTERSECTION_KHR.0);
    pub const IGNORE_INTERSECTION_NV: Self = Self(5335);
    pub const TERMINATE_RAY_NV: Self = Self(5336);
    pub const TRACE_NV: Self = Self(5337);
    pub const TYPE_ACCELERATION_STRUCTURE_KHR: Self = Self(5341);
    pub const TYPE_ACCELERATION_STRUCTURE_NV: Self = Self(Self::TYPE_ACCELERATION_STRUCTURE_KHR.0);
    pub const EXECUTE_CALLABLE_NV: Self = Self(5344);
    pub const TYPE_COOPERATIVE_MATRIX_NV: Self = Self(5358);
    pub const COOPERATIVE_MATRIX_LOAD_NV: Self = Self(5359);
    pub const COOPERATIVE_MATRIX_STORE_NV: Self = Self(5360);
    pub const COOPERATIVE_MATRIX_MUL_ADD_NV: Self = Self(5361);
    pub const COOPERATIVE_MATRIX_LENGTH_NV: Self = Self(5362);
    pub const BEGIN_INVOCATION_INTERLOCK_EXT: Self = Self(5364);
    pub const END_INVOCATION_INTERLOCK_EXT: Self = Self(5365);
    pub const DEMOTE_TO_HELPER_INVOCATION_EXT: Self = Self(5380);
    pub const IS_HELPER_INVOCATION_EXT: Self = Self(5381);
    pub const SUBGROUP_SHUFFLE_INTEL: Self = Self(5571);
    pub const SUBGROUP_SHUFFLE_DOWN_INTEL: Self = Self(5572);
    pub const SUBGROUP_SHUFFLE_UP_INTEL: Self = Self(5573);
    pub const SUBGROUP_SHUFFLE_XOR_INTEL: Self = Self(5574);
    pub const SUBGROUP_BLOCK_READ_INTEL: Self = Self(5575);
    pub const SUBGROUP_BLOCK_WRITE_INTEL: Self = Self(5576);
    pub const SUBGROUP_IMAGE_BLOCK_READ_INTEL: Self = Self(5577);
    pub const SUBGROUP_IMAGE_BLOCK_WRITE_INTEL: Self = Self(5578);
    pub const SUBGROUP_IMAGE_MEDIA_BLOCK_READ_INTEL: Self = Self(5580);
    pub const SUBGROUP_IMAGE_MEDIA_BLOCK_WRITE_INTEL: Self = Self(5581);
    pub const U_COUNT_LEADING_ZEROS_INTEL: Self = Self(5585);
    pub const U_COUNT_TRAILING_ZEROS_INTEL: Self = Self(5586);
    pub const ABS_I_SUB_INTEL: Self = Self(5587);
    pub const ABS_U_SUB_INTEL: Self = Self(5588);
    pub const I_ADD_SAT_INTEL: Self = Self(5589);
    pub const U_ADD_SAT_INTEL: Self = Self(5590);
    pub const I_AVERAGE_INTEL: Self = Self(5591);
    pub const U_AVERAGE_INTEL: Self = Self(5592);
    pub const I_AVERAGE_ROUNDED_INTEL: Self = Self(5593);
    pub const U_AVERAGE_ROUNDED_INTEL: Self = Self(5594);
    pub const I_SUB_SAT_INTEL: Self = Self(5595);
    pub const U_SUB_SAT_INTEL: Self = Self(5596);
    pub const I_MUL32X16_INTEL: Self = Self(5597);
    pub const U_MUL32X16_INTEL: Self = Self(5598);
    pub const FUNCTION_POINTER_INTEL: Self = Self(5600);
    pub const FUNCTION_POINTER_CALL_INTEL: Self = Self(5601);
    pub const DECORATE_STRING: Self = Self(5632);
    pub const DECORATE_STRING_GOOGLE: Self = Self(Self::DECORATE_STRING.0);
    pub const MEMBER_DECORATE_STRING: Self = Self(5633);
    pub const MEMBER_DECORATE_STRING_GOOGLE: Self = Self(Self::MEMBER_DECORATE_STRING.0);
    pub const VME_IMAGE_INTEL: Self = Self(5699);
    pub const TYPE_VME_IMAGE_INTEL: Self = Self(5700);
    pub const TYPE_AVC_IME_PAYLOAD_INTEL: Self = Self(5701);
    pub const TYPE_AVC_REF_PAYLOAD_INTEL: Self = Self(5702);
    pub const TYPE_AVC_SIC_PAYLOAD_INTEL: Self = Self(5703);
    pub const TYPE_AVC_MCE_PAYLOAD_INTEL: Self = Self(5704);
    pub const TYPE_AVC_MCE_RESULT_INTEL: Self = Self(5705);
    pub const TYPE_AVC_IME_RESULT_INTEL: Self = Self(5706);
    pub const TYPE_AVC_IME_RESULT_SINGLE_REFERENCE_STREAMOUT_INTEL: Self = Self(5707);
    pub const TYPE_AVC_IME_RESULT_DUAL_REFERENCE_STREAMOUT_INTEL: Self = Self(5708);
    pub const TYPE_AVC_IME_SINGLE_REFERENCE_STREAMIN_INTEL: Self = Self(5709);
    pub const TYPE_AVC_IME_DUAL_REFERENCE_STREAMIN_INTEL: Self = Self(5710);
    pub const TYPE_AVC_REF_RESULT_INTEL: Self = Self(5711);
    pub const TYPE_AVC_SIC_RESULT_INTEL: Self = Self(5712);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_INTER_BASE_MULTI_REFERENCE_PENALTY_INTEL: Self = Self(5713);
    pub const SUBGROUP_AVC_MCE_SET_INTER_BASE_MULTI_REFERENCE_PENALTY_INTEL: Self = Self(5714);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_INTER_SHAPE_PENALTY_INTEL: Self = Self(5715);
    pub const SUBGROUP_AVC_MCE_SET_INTER_SHAPE_PENALTY_INTEL: Self = Self(5716);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_INTER_DIRECTION_PENALTY_INTEL: Self = Self(5717);
    pub const SUBGROUP_AVC_MCE_SET_INTER_DIRECTION_PENALTY_INTEL: Self = Self(5718);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_INTRA_LUMA_SHAPE_PENALTY_INTEL: Self = Self(5719);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_INTER_MOTION_VECTOR_COST_TABLE_INTEL: Self = Self(5720);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_HIGH_PENALTY_COST_TABLE_INTEL: Self = Self(5721);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_MEDIUM_PENALTY_COST_TABLE_INTEL: Self = Self(5722);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_LOW_PENALTY_COST_TABLE_INTEL: Self = Self(5723);
    pub const SUBGROUP_AVC_MCE_SET_MOTION_VECTOR_COST_FUNCTION_INTEL: Self = Self(5724);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_INTRA_LUMA_MODE_PENALTY_INTEL: Self = Self(5725);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_NON_DC_LUMA_INTRA_PENALTY_INTEL: Self = Self(5726);
    pub const SUBGROUP_AVC_MCE_GET_DEFAULT_INTRA_CHROMA_MODE_BASE_PENALTY_INTEL: Self = Self(5727);
    pub const SUBGROUP_AVC_MCE_SET_AC_ONLY_HAAR_INTEL: Self = Self(5728);
    pub const SUBGROUP_AVC_MCE_SET_SOURCE_INTERLACED_FIELD_POLARITY_INTEL: Self = Self(5729);
    pub const SUBGROUP_AVC_MCE_SET_SINGLE_REFERENCE_INTERLACED_FIELD_POLARITY_INTEL: Self = Self(5730);
    pub const SUBGROUP_AVC_MCE_SET_DUAL_REFERENCE_INTERLACED_FIELD_POLARITIES_INTEL: Self = Self(5731);
    pub const SUBGROUP_AVC_MCE_CONVERT_TO_IME_PAYLOAD_INTEL: Self = Self(5732);
    pub const SUBGROUP_AVC_MCE_CONVERT_TO_IME_RESULT_INTEL: Self = Self(5733);
    pub const SUBGROUP_AVC_MCE_CONVERT_TO_REF_PAYLOAD_INTEL: Self = Self(5734);
    pub const SUBGROUP_AVC_MCE_CONVERT_TO_REF_RESULT_INTEL: Self = Self(5735);
    pub const SUBGROUP_AVC_MCE_CONVERT_TO_SIC_PAYLOAD_INTEL: Self = Self(5736);
    pub const SUBGROUP_AVC_MCE_CONVERT_TO_SIC_RESULT_INTEL: Self = Self(5737);
    pub const SUBGROUP_AVC_MCE_GET_MOTION_VECTORS_INTEL: Self = Self(5738);
    pub const SUBGROUP_AVC_MCE_GET_INTER_DISTORTIONS_INTEL: Self = Self(5739);
    pub const SUBGROUP_AVC_MCE_GET_BEST_INTER_DISTORTIONS_INTEL: Self = Self(5740);
    pub const SUBGROUP_AVC_MCE_GET_INTER_MAJOR_SHAPE_INTEL: Self = Self(5741);
    pub const SUBGROUP_AVC_MCE_GET_INTER_MINOR_SHAPE_INTEL: Self = Self(5742);
    pub const SUBGROUP_AVC_MCE_GET_INTER_DIRECTIONS_INTEL: Self = Self(5743);
    pub const SUBGROUP_AVC_MCE_GET_INTER_MOTION_VECTOR_COUNT_INTEL: Self = Self(5744);
    pub const SUBGROUP_AVC_MCE_GET_INTER_REFERENCE_IDS_INTEL: Self = Self(5745);
    pub const SUBGROUP_AVC_MCE_GET_INTER_REFERENCE_INTERLACED_FIELD_POLARITIES_INTEL: Self = Self(5746);
    pub const SUBGROUP_AVC_IME_INITIALIZE_INTEL: Self = Self(5747);
    pub const SUBGROUP_AVC_IME_SET_SINGLE_REFERENCE_INTEL: Self = Self(5748);
    pub const SUBGROUP_AVC_IME_SET_DUAL_REFERENCE_INTEL: Self = Self(5749);
    pub const SUBGROUP_AVC_IME_REF_WINDOW_SIZE_INTEL: Self = Self(5750);
    pub const SUBGROUP_AVC_IME_ADJUST_REF_OFFSET_INTEL: Self = Self(5751);
    pub const SUBGROUP_AVC_IME_CONVERT_TO_MCE_PAYLOAD_INTEL: Self = Self(5752);
    pub const SUBGROUP_AVC_IME_SET_MAX_MOTION_VECTOR_COUNT_INTEL: Self = Self(5753);
    pub const SUBGROUP_AVC_IME_SET_UNIDIRECTIONAL_MIX_DISABLE_INTEL: Self = Self(5754);
    pub const SUBGROUP_AVC_IME_SET_EARLY_SEARCH_TERMINATION_THRESHOLD_INTEL: Self = Self(5755);
    pub const SUBGROUP_AVC_IME_SET_WEIGHTED_SAD_INTEL: Self = Self(5756);
    pub const SUBGROUP_AVC_IME_EVALUATE_WITH_SINGLE_REFERENCE_INTEL: Self = Self(5757);
    pub const SUBGROUP_AVC_IME_EVALUATE_WITH_DUAL_REFERENCE_INTEL: Self = Self(5758);
    pub const SUBGROUP_AVC_IME_EVALUATE_WITH_SINGLE_REFERENCE_STREAMIN_INTEL: Self = Self(5759);
    pub const SUBGROUP_AVC_IME_EVALUATE_WITH_DUAL_REFERENCE_STREAMIN_INTEL: Self = Self(5760);
    pub const SUBGROUP_AVC_IME_EVALUATE_WITH_SINGLE_REFERENCE_STREAMOUT_INTEL: Self = Self(5761);
    pub const SUBGROUP_AVC_IME_EVALUATE_WITH_DUAL_REFERENCE_STREAMOUT_INTEL: Self = Self(5762);
    pub const SUBGROUP_AVC_IME_EVALUATE_WITH_SINGLE_REFERENCE_STREAMINOUT_INTEL: Self = Self(5763);
    pub const SUBGROUP_AVC_IME_EVALUATE_WITH_DUAL_REFERENCE_STREAMINOUT_INTEL: Self = Self(5764);
    pub const SUBGROUP_AVC_IME_CONVERT_TO_MCE_RESULT_INTEL: Self = Self(5765);
    pub const SUBGROUP_AVC_IME_GET_SINGLE_REFERENCE_STREAMIN_INTEL: Self = Self(5766);
    pub const SUBGROUP_AVC_IME_GET_DUAL_REFERENCE_STREAMIN_INTEL: Self = Self(5767);
    pub const SUBGROUP_AVC_IME_STRIP_SINGLE_REFERENCE_STREAMOUT_INTEL: Self = Self(5768);
    pub const SUBGROUP_AVC_IME_STRIP_DUAL_REFERENCE_STREAMOUT_INTEL: Self = Self(5769);
    pub const SUBGROUP_AVC_IME_GET_STREAMOUT_SINGLE_REFERENCE_MAJOR_SHAPE_MOTION_VECTORS_INTEL: Self = Self(5770);
    pub const SUBGROUP_AVC_IME_GET_STREAMOUT_SINGLE_REFERENCE_MAJOR_SHAPE_DISTORTIONS_INTEL: Self = Self(5771);
    pub const SUBGROUP_AVC_IME_GET_STREAMOUT_SINGLE_REFERENCE_MAJOR_SHAPE_REFERENCE_IDS_INTEL: Self = Self(5772);
    pub const SUBGROUP_AVC_IME_GET_STREAMOUT_DUAL_REFERENCE_MAJOR_SHAPE_MOTION_VECTORS_INTEL: Self = Self(5773);
    pub const SUBGROUP_AVC_IME_GET_STREAMOUT_DUAL_REFERENCE_MAJOR_SHAPE_DISTORTIONS_INTEL: Self = Self(5774);
    pub const SUBGROUP_AVC_IME_GET_STREAMOUT_DUAL_REFERENCE_MAJOR_SHAPE_REFERENCE_IDS_INTEL: Self = Self(5775);
    pub const SUBGROUP_AVC_IME_GET_BORDER_REACHED_INTEL: Self = Self(5776);
    pub const SUBGROUP_AVC_IME_GET_TRUNCATED_SEARCH_INDICATION_INTEL: Self = Self(5777);
    pub const SUBGROUP_AVC_IME_GET_UNIDIRECTIONAL_EARLY_SEARCH_TERMINATION_INTEL: Self = Self(5778);
    pub const SUBGROUP_AVC_IME_GET_WEIGHTING_PATTERN_MINIMUM_MOTION_VECTOR_INTEL: Self = Self(5779);
    pub const SUBGROUP_AVC_IME_GET_WEIGHTING_PATTERN_MINIMUM_DISTORTION_INTEL: Self = Self(5780);
    pub const SUBGROUP_AVC_FME_INITIALIZE_INTEL: Self = Self(5781);
    pub const SUBGROUP_AVC_BME_INITIALIZE_INTEL: Self = Self(5782);
    pub const SUBGROUP_AVC_REF_CONVERT_TO_MCE_PAYLOAD_INTEL: Self = Self(5783);
    pub const SUBGROUP_AVC_REF_SET_BIDIRECTIONAL_MIX_DISABLE_INTEL: Self = Self(5784);
    pub const SUBGROUP_AVC_REF_SET_BILINEAR_FILTER_ENABLE_INTEL: Self = Self(5785);
    pub const SUBGROUP_AVC_REF_EVALUATE_WITH_SINGLE_REFERENCE_INTEL: Self = Self(5786);
    pub const SUBGROUP_AVC_REF_EVALUATE_WITH_DUAL_REFERENCE_INTEL: Self = Self(5787);
    pub const SUBGROUP_AVC_REF_EVALUATE_WITH_MULTI_REFERENCE_INTEL: Self = Self(5788);
    pub const SUBGROUP_AVC_REF_EVALUATE_WITH_MULTI_REFERENCE_INTERLACED_INTEL: Self = Self(5789);
    pub const SUBGROUP_AVC_REF_CONVERT_TO_MCE_RESULT_INTEL: Self = Self(5790);
    pub const SUBGROUP_AVC_SIC_INITIALIZE_INTEL: Self = Self(5791);
    pub const SUBGROUP_AVC_SIC_CONFIGURE_SKC_INTEL: Self = Self(5792);
    pub const SUBGROUP_AVC_SIC_CONFIGURE_IPE_LUMA_INTEL: Self = Self(5793);
    pub const SUBGROUP_AVC_SIC_CONFIGURE_IPE_LUMA_CHROMA_INTEL: Self = Self(5794);
    pub const SUBGROUP_AVC_SIC_GET_MOTION_VECTOR_MASK_INTEL: Self = Self(5795);
    pub const SUBGROUP_AVC_SIC_CONVERT_TO_MCE_PAYLOAD_INTEL: Self = Self(5796);
    pub const SUBGROUP_AVC_SIC_SET_INTRA_LUMA_SHAPE_PENALTY_INTEL: Self = Self(5797);
    pub const SUBGROUP_AVC_SIC_SET_INTRA_LUMA_MODE_COST_FUNCTION_INTEL: Self = Self(5798);
    pub const SUBGROUP_AVC_SIC_SET_INTRA_CHROMA_MODE_COST_FUNCTION_INTEL: Self = Self(5799);
    pub const SUBGROUP_AVC_SIC_SET_BILINEAR_FILTER_ENABLE_INTEL: Self = Self(5800);
    pub const SUBGROUP_AVC_SIC_SET_SKC_FORWARD_TRANSFORM_ENABLE_INTEL: Self = Self(5801);
    pub const SUBGROUP_AVC_SIC_SET_BLOCK_BASED_RAW_SKIP_SAD_INTEL: Self = Self(5802);
    pub const SUBGROUP_AVC_SIC_EVALUATE_IPE_INTEL: Self = Self(5803);
    pub const SUBGROUP_AVC_SIC_EVALUATE_WITH_SINGLE_REFERENCE_INTEL: Self = Self(5804);
    pub const SUBGROUP_AVC_SIC_EVALUATE_WITH_DUAL_REFERENCE_INTEL: Self = Self(5805);
    pub const SUBGROUP_AVC_SIC_EVALUATE_WITH_MULTI_REFERENCE_INTEL: Self = Self(5806);
    pub const SUBGROUP_AVC_SIC_EVALUATE_WITH_MULTI_REFERENCE_INTERLACED_INTEL: Self = Self(5807);
    pub const SUBGROUP_AVC_SIC_CONVERT_TO_MCE_RESULT_INTEL: Self = Self(5808);
    pub const SUBGROUP_AVC_SIC_GET_IPE_LUMA_SHAPE_INTEL: Self = Self(5809);
    pub const SUBGROUP_AVC_SIC_GET_BEST_IPE_LUMA_DISTORTION_INTEL: Self = Self(5810);
    pub const SUBGROUP_AVC_SIC_GET_BEST_IPE_CHROMA_DISTORTION_INTEL: Self = Self(5811);
    pub const SUBGROUP_AVC_SIC_GET_PACKED_IPE_LUMA_MODES_INTEL: Self = Self(5812);
    pub const SUBGROUP_AVC_SIC_GET_IPE_CHROMA_MODE_INTEL: Self = Self(5813);
    pub const SUBGROUP_AVC_SIC_GET_PACKED_SKC_LUMA_COUNT_THRESHOLD_INTEL: Self = Self(5814);
    pub const SUBGROUP_AVC_SIC_GET_PACKED_SKC_LUMA_SUM_THRESHOLD_INTEL: Self = Self(5815);
    pub const SUBGROUP_AVC_SIC_GET_INTER_RAW_SADS_INTEL: Self = Self(5816);
    pub const LOOP_CONTROL_INTEL: Self = Self(5887);
    pub const READ_PIPE_BLOCKING_INTEL: Self = Self(5946);
    pub const WRITE_PIPE_BLOCKING_INTEL: Self = Self(5947);
    pub const FPGA_REG_INTEL: Self = Self(5949);
    pub const RAY_QUERY_GET_RAY_T_MIN_KHR: Self = Self(6016);
    pub const RAY_QUERY_GET_RAY_FLAGS_KHR: Self = Self(6017);
    pub const RAY_QUERY_GET_INTERSECTION_TKHR: Self = Self(6018);
    pub const RAY_QUERY_GET_INTERSECTION_INSTANCE_CUSTOM_INDEX_KHR: Self = Self(6019);
    pub const RAY_QUERY_GET_INTERSECTION_INSTANCE_ID_KHR: Self = Self(6020);
    pub const RAY_QUERY_GET_INTERSECTION_INSTANCE_SHADER_BINDING_TABLE_RECORD_OFFSET_KHR: Self = Self(6021);
    pub const RAY_QUERY_GET_INTERSECTION_GEOMETRY_INDEX_KHR: Self = Self(6022);
    pub const RAY_QUERY_GET_INTERSECTION_PRIMITIVE_INDEX_KHR: Self = Self(6023);
    pub const RAY_QUERY_GET_INTERSECTION_BARYCENTRICS_KHR: Self = Self(6024);
    pub const RAY_QUERY_GET_INTERSECTION_FRONT_FACE_KHR: Self = Self(6025);
    pub const RAY_QUERY_GET_INTERSECTION_CANDIDATE_AABB_OPAQUE_KHR: Self = Self(6026);
    pub const RAY_QUERY_GET_INTERSECTION_OBJECT_RAY_DIRECTION_KHR: Self = Self(6027);
    pub const RAY_QUERY_GET_INTERSECTION_OBJECT_RAY_ORIGIN_KHR: Self = Self(6028);
    pub const RAY_QUERY_GET_WORLD_RAY_DIRECTION_KHR: Self = Self(6029);
    pub const RAY_QUERY_GET_WORLD_RAY_ORIGIN_KHR: Self = Self(6030);
    pub const RAY_QUERY_GET_INTERSECTION_OBJECT_TO_WORLD_KHR: Self = Self(6031);
    pub const RAY_QUERY_GET_INTERSECTION_WORLD_TO_OBJECT_KHR: Self = Self(6032);
    pub const ATOMIC_F_ADD_EXT: Self = Self(6035);
}

