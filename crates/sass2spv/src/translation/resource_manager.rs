// Gives constant buffers, storage buffers, textures and images their bindings and declares them.

use crate::accessor::GpuAccessor;
use crate::collections::{NetMap, NetSet};
use crate::ir::{Instruction, TextureFlags};
use crate::translation::definitions::AggregateType;
use crate::translation::properties::*;
use crate::types::{SamplerType, SetBindingPair, ShaderStage, TextureFormat};

const DEFAULT_LOCAL_MEMORY_SIZE: i32 = 128;
const DEFAULT_SHARED_MEMORY_SIZE: i32 = 4096;
pub const CONSTANT_BUFFER_SIZE: i32 = 0x10000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct TextureInfo {
    cbuf_slot: i32,
    handle: i32,
    array_length: i32,
    separate: bool,
    ty: SamplerType,
    format: TextureFormat,
}

#[derive(Clone, Copy, Debug, Default)]
struct TextureMeta {
    set: i32,
    binding: i32,
    accurate_type: bool,
    ty: SamplerType,
    usage_flags: TextureUsageFlags,
}

impl Default for SamplerType {
    fn default() -> Self {
        SamplerType::NONE
    }
}

pub struct ResourceManager {
    stage: ShaderStage,
    stage_prefix: &'static str,
    cb_slot_to_binding_map: [SetBindingPair; 18],
    sb_slot_to_binding_map: [SetBindingPair; 16],
    sb_slot_written: u32,
    sb_slots: NetMap<i32, i32>,
    used_constant_buffer_bindings: NetSet<i32>,
    used_textures: NetMap<TextureInfo, TextureMeta>,
    used_images: NetMap<TextureInfo, TextureMeta>,
    pub local_memory_id: i32,
    pub shared_memory_id: i32,
    pub properties: ShaderProperties,
}

pub fn shader_stage_prefix(stage: ShaderStage) -> &'static str {
    ["cp", "vp", "tcp", "tep", "gp", "fp"][stage as usize]
}

impl ResourceManager {
    /// `support_set`: the descriptor set of the support buffer (GpuAccessor::set_for(0)); `cbuf_limits`: it holds the
    /// constant buffers' bound sizes (Host::cbuf_limits).
    pub fn new(stage: ShaderStage, support_set: i32, cbuf_limits: bool) -> ResourceManager {
        let unset = SetBindingPair::new(-1, -1);
        let mut rm = ResourceManager {
            stage,
            stage_prefix: shader_stage_prefix(stage),
            cb_slot_to_binding_map: [unset; 18],
            sb_slot_to_binding_map: [unset; 16],
            sb_slot_written: 0,
            sb_slots: NetMap::new(),
            used_constant_buffer_bindings: NetSet::new(),
            used_textures: NetMap::new(),
            used_images: NetMap::new(),
            local_memory_id: -1,
            shared_memory_id: -1,
            properties: ShaderProperties::default(),
        };
        rm.properties.add_or_update_constant_buffer(BufferDefinition {
            layout: BufferLayout::Std140,
            set: support_set,
            binding: SUPPORT_BUFFER_BINDING,
            name: "support_buffer".into(),
            ty: crate::translation::properties::support_buffer_structure_type_for(cbuf_limits),
        });
        rm
    }

    pub fn set_current_local_memory(&mut self, size: i32, is_used: bool) {
        self.local_memory_id = if is_used {
            let size = if size <= 0 { DEFAULT_LOCAL_MEMORY_SIZE } else { size };
            self.properties.add_local_memory(MemoryDefinition {
                name: "local_memory",
                ty: AggregateType::ARRAY | AggregateType::U32,
                array_length: (size + 3) / 4,
            })
        } else {
            -1
        };
    }

    pub fn set_current_shared_memory(&mut self, size: i32, is_used: bool) {
        self.shared_memory_id = if is_used {
            let size = if size <= 0 { DEFAULT_SHARED_MEMORY_SIZE } else { size };
            self.properties.add_shared_memory(MemoryDefinition {
                name: "shared_memory",
                ty: AggregateType::ARRAY | AggregateType::U32,
                array_length: (size + 3) / 4,
            })
        } else {
            -1
        };
    }

    /// Fails for slots past the 18 the map holds (the original's array index throws).
    pub fn constant_buffer_binding(&mut self, gpu: &GpuAccessor, slot: i32) -> Result<i32, String> {
        let mut sb = *self.cb_slot_to_binding_map.get(slot as usize).ok_or_else(|| format!("constant buffer slot {slot} out of range"))?;
        if sb.binding < 0 {
            sb = gpu.create_constant_buffer_binding(slot);
            self.cb_slot_to_binding_map[slot as usize] = sb;
            let name = format!("{}_c{slot}", self.stage_prefix);
            self.properties.add_or_update_constant_buffer(BufferDefinition {
                layout: BufferLayout::Std140,
                set: sb.set_index,
                binding: sb.binding,
                name,
                ty: StructureType {
                    fields: vec![StructureField::new(
                        AggregateType::ARRAY | AggregateType::VECTOR4 | AggregateType::FP32,
                        "data",
                        CONSTANT_BUFFER_SIZE / 16,
                    )],
                },
            });
        }
        Ok(sb.binding)
    }

    pub fn try_get_storage_buffer_binding(&mut self, gpu: &GpuAccessor, sb_cb_slot: i32, sb_cb_offset: i32, write: bool) -> Option<i32> {
        let slot = self.try_get_sb_slot(sb_cb_slot as u8, sb_cb_offset as u16)?;
        let mut sb = self.sb_slot_to_binding_map[slot as usize];
        if sb.binding < 0 {
            sb = gpu.create_storage_buffer_binding(slot);
            self.sb_slot_to_binding_map[slot as usize] = sb;
            let name = format!("{}_s{slot}", self.stage_prefix);
            self.properties.add_or_update_storage_buffer(BufferDefinition {
                layout: BufferLayout::Std430,
                set: sb.set_index,
                binding: sb.binding,
                name,
                ty: StructureType { fields: vec![StructureField::new(AggregateType::ARRAY | AggregateType::U32, "data", 0)] },
            });
        }
        if write {
            self.sb_slot_written |= 1 << slot;
        }
        Some(sb.binding)
    }

    fn try_get_sb_slot(&mut self, sb_cb_slot: u8, sb_cb_offset: u16) -> Option<i32> {
        let key = pack_sb_cb_info(sb_cb_slot as i32, sb_cb_offset as i32);
        if let Some(&slot) = self.sb_slots.get(&key) {
            return Some(slot);
        }
        let slot = self.sb_slots.len() as i32;
        if slot as usize >= self.sb_slot_to_binding_map.len() {
            return None;
        }
        self.sb_slots.try_add(key, slot);
        Some(slot)
    }

    pub fn try_get_constant_buffer_slot(&self, binding: i32) -> Option<i32> {
        self.cb_slot_to_binding_map.iter().position(|sb| sb.binding == binding).map(|s| s as i32)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn texture_or_image_binding(
        &mut self,
        gpu: &GpuAccessor,
        inst: Instruction,
        ty: SamplerType,
        mut format: TextureFormat,
        flags: TextureFlags,
        cbuf_slot: i32,
        handle: i32,
        array_length: i32,
        separate: bool,
    ) -> SetBindingPair {
        let inst = inst.base();
        let is_image = inst.is_image();
        let is_write = inst.is_image_store();
        let accurate_type = !inst.is_texture_query();
        let int_coords = is_image || flags.has(TextureFlags::INT_COORDS) || inst == Instruction::TEXTURE_QUERY_SIZE;
        let coherent = flags.has(TextureFlags::COHERENT);
        if !is_image {
            format = TextureFormat::Unknown;
        }
        self.binding_for(gpu, cbuf_slot, handle, array_length, ty, format, is_image, int_coords, is_write, accurate_type, coherent, separate)
    }

    #[allow(clippy::too_many_arguments)]
    fn binding_for(
        &mut self,
        gpu: &GpuAccessor,
        cbuf_slot: i32,
        handle: i32,
        array_length: i32,
        ty: SamplerType,
        format: TextureFormat,
        is_image: bool,
        int_coords: bool,
        write: bool,
        accurate_type: bool,
        coherent: bool,
        separate: bool,
    ) -> SetBindingPair {
        let dimensions = if ty == SamplerType::NONE { 0 } else { ty.dimensions() };
        let mut usage_flags = TextureUsageFlags::NONE;
        if int_coords {
            usage_flags |= TextureUsageFlags::NEEDS_SCALE_VALUE;
            let can_scale = self.stage.supports_render_scale() && array_length == 1 && !write && dimensions == 2;
            if !can_scale {
                usage_flags |= TextureUsageFlags::RES_SCALE_UNSUPPORTED;
            }
        }
        if write {
            usage_flags |= TextureUsageFlags::IMAGE_STORE;
        }
        if coherent {
            usage_flags |= TextureUsageFlags::IMAGE_COHERENT;
        }

        // Array textures key by type as well: handles in one buffer can be textures of different types.
        let key_type = if array_length > 1 { ty } else { SamplerType::NONE };
        let info = TextureInfo { cbuf_slot, handle, array_length, separate, ty: key_type, format };
        let mut meta = TextureMeta { set: 0, binding: 0, accurate_type, ty, usage_flags };

        let dict = if is_image { &mut self.used_images } else { &mut self.used_textures };
        let (set_index, binding);
        if let Some(&existing) = dict.get(&info) {
            meta = merge_texture_meta(meta, existing);
            dict.set(info, meta);
            set_index = existing.set;
            binding = existing.binding;
        } else {
            let extra = if array_length > 1 { gpu.create_extra_set() } else { -1 };
            if array_length > 1 && extra >= 0 {
                // Arrays get an extra set of their own, binding 0.
                set_index = extra;
                binding = 0;
            } else {
                let is_buffer = ty.base() == SamplerType::TEXTURE_BUFFER;
                let sb = if is_image {
                    gpu.create_image_binding(array_length, is_buffer)
                } else {
                    gpu.create_texture_binding(array_length, is_buffer)
                };
                set_index = sb.set_index;
                binding = sb.binding;
            }
            meta.set = set_index;
            meta.binding = binding;
            dict.try_add(info, meta);
        }

        let mut prefix = String::from(if is_image { "i" } else { "t" });
        if array_length != 1 && ty != SamplerType::NONE {
            prefix += &short_type_name(ty);
        }
        let name_suffix = if is_image {
            if cbuf_slot < 0 {
                format!("{prefix}_tcb_{handle:X}_{}", glsl_format(format))
            } else {
                format!("{prefix}_cb{cbuf_slot}_{handle:X}_{}", glsl_format(format))
            }
        } else if ty == SamplerType::NONE {
            if cbuf_slot < 0 { format!("s_tcb_{handle:X}") } else { format!("s_cb{cbuf_slot}_{handle:X}") }
        } else if cbuf_slot < 0 {
            format!("{prefix}_tcb_{handle:X}")
        } else {
            format!("{prefix}_cb{cbuf_slot}_{handle:X}")
        };

        let definition = TextureDefinition {
            set: set_index,
            binding,
            array_length,
            separate,
            name: format!("{}_{name_suffix}", self.stage_prefix),
            ty: meta.ty,
            format: info.format,
            flags: meta.usage_flags,
        };
        if is_image {
            self.properties.add_or_update_image(definition);
        } else {
            self.properties.add_or_update_texture(definition);
        }
        SetBindingPair::new(set_index, binding)
    }

    pub fn set_usage_flags_for_texture_query(&mut self, binding: i32, ty: SamplerType) {
        let found = self.used_textures.iter().find(|(_, m)| m.binding == binding).map(|(i, m)| (*i, *m));
        if let Some((info, mut meta)) = found {
            meta.usage_flags |= TextureUsageFlags::NEEDS_SCALE_VALUE;
            let can_scale = self.stage.supports_render_scale() && info.array_length == 1 && ty.dimensions() == 2;
            if !can_scale {
                meta.usage_flags |= TextureUsageFlags::RES_SCALE_UNSUPPORTED;
            }
            self.used_textures.set(info, meta);
        }
    }

    pub fn set_used_constant_buffer_binding(&mut self, binding: i32) {
        self.used_constant_buffer_bindings.insert(binding);
    }

    pub fn constant_buffer_descriptors(&self) -> Vec<BufferDescriptor> {
        let mut v = Vec::new();
        for (slot, sb) in self.cb_slot_to_binding_map.iter().enumerate() {
            if sb.binding >= 0 && self.used_constant_buffer_bindings.contains(&sb.binding) {
                v.push(BufferDescriptor { set: sb.set_index, binding: sb.binding, slot: slot as u8, sb_cb_slot: 0, sb_cb_offset: 0, write: false });
            }
        }
        v
    }

    pub fn storage_buffer_descriptors(&self) -> Vec<BufferDescriptor> {
        let mut v = Vec::new();
        for (&key, &slot) in self.sb_slots.iter() {
            let sb = self.sb_slot_to_binding_map[slot as usize];
            if sb.binding >= 0 {
                v.push(BufferDescriptor {
                    set: sb.set_index,
                    binding: sb.binding,
                    slot: slot as u8,
                    sb_cb_slot: (key >> 16) as u8,
                    sb_cb_offset: key as u16,
                    write: self.sb_slot_written & (1 << slot) != 0,
                });
            }
        }
        v
    }

    pub fn texture_descriptors(&self) -> Vec<TextureDescriptor> {
        descriptors(&self.used_textures)
    }

    /// GetTextureDescriptors(includeArrays: false).Length.
    pub fn texture_descriptors_without_arrays_count(&self) -> i32 {
        self.used_textures.iter().filter(|(i, _)| i.array_length <= 1).count() as i32
    }

    pub fn image_descriptors(&self) -> Vec<TextureDescriptor> {
        descriptors(&self.used_images)
    }

    pub fn cbuf_slot_and_handle_for_texture(&self, binding: i32) -> Option<(i32, i32)> {
        self.used_textures.iter().find(|(_, m)| m.binding == binding).map(|(i, _)| (i.cbuf_slot, i.handle))
    }

    pub fn find_texture_descriptor_index(&self, binding: i32) -> i32 {
        self.texture_descriptors().iter().position(|d| d.binding == binding).map_or(-1, |p| p as i32)
    }

    pub fn find_image_descriptor_index(&self, binding: i32) -> i32 {
        self.image_descriptors().iter().position(|d| d.binding == binding).map_or(-1, |p| p as i32)
    }

    pub fn is_array_of_textures_or_images(&self, binding: i32, is_image: bool) -> bool {
        let dict = if is_image { &self.used_images } else { &self.used_textures };
        dict.iter().find(|(_, m)| m.binding == binding).is_some_and(|(i, _)| i.array_length != 1)
    }
}

fn merge_texture_meta(mut meta: TextureMeta, existing: TextureMeta) -> TextureMeta {
    meta.set = existing.set;
    meta.binding = existing.binding;
    meta.usage_flags |= existing.usage_flags;
    // Prefer the more accurate type information.
    if existing.accurate_type {
        meta.accurate_type = true;
        meta.ty = existing.ty;
    }
    meta
}

fn descriptors(used: &NetMap<TextureInfo, TextureMeta>) -> Vec<TextureDescriptor> {
    let d = |i: &TextureInfo, m: &TextureMeta| TextureDescriptor {
        set: m.set,
        binding: m.binding,
        ty: m.ty,
        format: i.format,
        cbuf_slot: i.cbuf_slot,
        handle_index: i.handle,
        array_length: i.array_length,
        separate: i.separate,
        flags: m.usage_flags,
    };
    let mut v: Vec<TextureDescriptor> = used.iter().filter(|(i, _)| i.array_length <= 1).map(|(i, m)| d(i, m)).collect();
    v.extend(used.iter().filter(|(i, _)| i.array_length > 1).map(|(i, m)| d(i, m)));
    v
}

fn pack_sb_cb_info(sb_cb_slot: i32, sb_cb_offset: i32) -> i32 {
    sb_cb_offset | (sb_cb_slot << 16)
}

pub fn short_type_name(ty: SamplerType) -> String {
    let mut s = String::from(match ty.base() {
        SamplerType::TEXTURE_1D => "1d",
        SamplerType::TEXTURE_BUFFER => "b",
        SamplerType::TEXTURE_2D => "2d",
        SamplerType::TEXTURE_3D => "3d",
        SamplerType::TEXTURE_CUBE => "cube",
        _ => panic!("invalid sampler type {ty}"),
    });
    if ty.has(SamplerType::MULTISAMPLE) {
        s += "ms";
    }
    if ty.has(SamplerType::ARRAY) {
        s += "a";
    }
    if ty.has(SamplerType::SHADOW) {
        s += "s";
    }
    s
}

pub fn glsl_format(f: TextureFormat) -> &'static str {
    use TextureFormat as F;
    match f {
        F::R8Unorm => "r8",
        F::R8Snorm => "r8_snorm",
        F::R8Uint => "r8ui",
        F::R8Sint => "r8i",
        F::R16Float => "r16f",
        F::R16Unorm => "r16",
        F::R16Snorm => "r16_snorm",
        F::R16Uint => "r16ui",
        F::R16Sint => "r16i",
        F::R32Float => "r32f",
        F::R32Uint => "r32ui",
        F::R32Sint => "r32i",
        F::R8G8Unorm => "rg8",
        F::R8G8Snorm => "rg8_snorm",
        F::R8G8Uint => "rg8ui",
        F::R8G8Sint => "rg8i",
        F::R16G16Float => "rg16f",
        F::R16G16Unorm => "rg16",
        F::R16G16Snorm => "rg16_snorm",
        F::R16G16Uint => "rg16ui",
        F::R16G16Sint => "rg16i",
        F::R32G32Float => "rg32f",
        F::R32G32Uint => "rg32ui",
        F::R32G32Sint => "rg32i",
        F::R8G8B8A8Unorm => "rgba8",
        F::R8G8B8A8Snorm => "rgba8_snorm",
        F::R8G8B8A8Uint => "rgba8ui",
        F::R8G8B8A8Sint => "rgba8i",
        F::R16G16B16A16Float => "rgba16f",
        F::R16G16B16A16Unorm => "rgba16",
        F::R16G16B16A16Snorm => "rgba16_snorm",
        F::R16G16B16A16Uint => "rgba16ui",
        F::R16G16B16A16Sint => "rgba16i",
        F::R32G32B32A32Float => "rgba32f",
        F::R32G32B32A32Uint => "rgba32ui",
        F::R32G32B32A32Sint => "rgba32i",
        F::R10G10B10A2Unorm => "rgb10_a2",
        F::R10G10B10A2Uint => "rgb10_a2ui",
        F::R11G11B10Float => "r11f_g11f_b10f",
        F::Unknown => "",
    }
}
