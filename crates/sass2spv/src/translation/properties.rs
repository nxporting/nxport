// The resources a shader declares (StructuredIr.ShaderProperties and its definitions) and the descriptors the
// manifest lists.

use crate::collections::NetMap;
use crate::translation::definitions::AggregateType;
use crate::types::{SamplerType, SetBindingPair, TextureFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferLayout {
    Std140,
    Std430,
}

#[derive(Clone, Debug)]
pub struct StructureField {
    pub ty: AggregateType,
    pub name: &'static str,
    pub array_length: i32,
}

impl StructureField {
    pub fn new(ty: AggregateType, name: &'static str, array_length: i32) -> Self {
        StructureField { ty, name, array_length }
    }
}

#[derive(Clone, Debug)]
pub struct StructureType {
    pub fields: Vec<StructureField>,
}

#[derive(Clone, Debug)]
pub struct BufferDefinition {
    pub layout: BufferLayout,
    pub set: i32,
    pub binding: i32,
    pub name: String,
    pub ty: StructureType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TextureUsageFlags(pub u32);

impl TextureUsageFlags {
    pub const NONE: Self = Self(0);
    pub const RES_SCALE_UNSUPPORTED: Self = Self(1 << 0);
    pub const NEEDS_SCALE_VALUE: Self = Self(1 << 1);
    pub const IMAGE_STORE: Self = Self(1 << 2);
    pub const IMAGE_COHERENT: Self = Self(1 << 3);

    pub fn has(self, f: Self) -> bool {
        self.0 & f.0 == f.0
    }
}

impl std::ops::BitOrAssign for TextureUsageFlags {
    fn bitor_assign(&mut self, o: Self) {
        self.0 |= o.0;
    }
}

#[derive(Clone, Debug)]
pub struct TextureDefinition {
    pub set: i32,
    pub binding: i32,
    pub array_length: i32,
    pub separate: bool,
    pub name: String,
    pub ty: SamplerType,
    pub format: TextureFormat,
    pub flags: TextureUsageFlags,
}

#[derive(Clone, Debug)]
pub struct MemoryDefinition {
    pub name: &'static str,
    pub ty: AggregateType,
    pub array_length: i32,
}

#[derive(Default)]
pub struct ShaderProperties {
    pub constant_buffers: NetMap<i32, BufferDefinition>,
    pub storage_buffers: NetMap<i32, BufferDefinition>,
    pub textures: NetMap<SetBindingPair, TextureDefinition>,
    pub images: NetMap<SetBindingPair, TextureDefinition>,
    pub local_memories: NetMap<i32, MemoryDefinition>,
    pub shared_memories: NetMap<i32, MemoryDefinition>,
}

impl ShaderProperties {
    pub fn add_or_update_constant_buffer(&mut self, d: BufferDefinition) {
        self.constant_buffers.set(d.binding, d);
    }

    pub fn add_or_update_storage_buffer(&mut self, d: BufferDefinition) {
        self.storage_buffers.set(d.binding, d);
    }

    pub fn add_or_update_texture(&mut self, d: TextureDefinition) {
        self.textures.set(SetBindingPair::new(d.set, d.binding), d);
    }

    pub fn add_or_update_image(&mut self, d: TextureDefinition) {
        self.images.set(SetBindingPair::new(d.set, d.binding), d);
    }

    pub fn add_local_memory(&mut self, d: MemoryDefinition) -> i32 {
        let id = self.local_memories.len() as i32;
        self.local_memories.try_add(id, d);
        id
    }

    pub fn add_shared_memory(&mut self, d: MemoryDefinition) -> i32 {
        let id = self.shared_memories.len() as i32;
        self.shared_memories.try_add(id, d);
        id
    }
}

/// The host loads images without a declared format.
pub fn texture_format(_gpu: &crate::accessor::GpuAccessor, _handle: i32, _cbuf_slot: i32) -> TextureFormat {
    TextureFormat::Unknown
}

/// Image atomics need a format: R32 integer formats only (the engine's answer, R8G8B8A8Unorm, is replaced).
pub fn texture_format_atomic(gpu: &crate::accessor::GpuAccessor, handle: i32, cbuf_slot: i32) -> TextureFormat {
    let format = gpu.texture_format(handle, cbuf_slot);
    if matches!(format, TextureFormat::R32Sint | TextureFormat::R32Uint) {
        format
    } else {
        gpu.log_msg(&format!("Unsupported format for texture {handle}: {format:?}."));
        TextureFormat::R32Sint
    }
}

pub const SUPPORT_BUFFER_BINDING: i32 = 0;
pub const FRAGMENT_IS_BGRA_COUNT: i32 = 8;
pub const RENDER_SCALE_MAX_COUNT: i32 = 1 + 64 + 8;

/// SupportBufferField: the support buffer's fields in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportBufferField {
    FragmentAlphaTest,
    FragmentIsBgra,
    ViewportInverse,
    ViewportSize,
    FragmentRenderScaleCount,
    RenderScale,
    TfeOffset,
    TfeVertexCount,
}

pub fn support_buffer_structure_type() -> StructureType {
    use AggregateType as A;
    StructureType {
        fields: vec![
            StructureField::new(A::U32, "alpha_test", 1),
            StructureField::new(A::ARRAY | A::U32, "is_bgra", FRAGMENT_IS_BGRA_COUNT),
            StructureField::new(A::VECTOR4 | A::FP32, "viewport_inverse", 1),
            StructureField::new(A::VECTOR4 | A::FP32, "viewport_size", 1),
            StructureField::new(A::S32, "frag_scale_count", 1),
            StructureField::new(A::ARRAY | A::FP32, "render_scale", RENDER_SCALE_MAX_COUNT),
            StructureField::new(A::VECTOR4 | A::S32, "tfe_offset", 1),
            StructureField::new(A::S32, "tfe_vertex_count", 1),
        ],
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BufferDescriptor {
    pub set: i32,
    pub binding: i32,
    pub slot: u8,
    pub sb_cb_slot: u8,
    pub sb_cb_offset: u16,
    pub write: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct TextureDescriptor {
    pub set: i32,
    pub binding: i32,
    pub ty: SamplerType,
    pub format: TextureFormat,
    pub cbuf_slot: i32,
    pub handle_index: i32,
    pub array_length: i32,
    pub separate: bool,
    pub flags: TextureUsageFlags,
}
