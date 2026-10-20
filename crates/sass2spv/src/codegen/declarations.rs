// Global and function-level declarations: buffers, memories, samplers, images, inputs and outputs, parameters and
// locals (CodeGen.Spirv.Declarations).

use std::collections::HashSet;

use super::{CodeGenContext, ImageDeclaration, SamplerDeclaration, R};
use crate::ir::{IoVariable, StorageKind};
use crate::spv::*;
use crate::structured::IoDefinition;
use crate::translation::definitions::{AggregateType as A, PixelImap};
use crate::translation::properties::{BufferDefinition, BufferLayout, TextureDefinition, TextureUsageFlags};
use crate::types::{SamplerType, SetBindingPair, ShaderStage, TextureFormat};

pub fn declare_parameters(c: &mut CodeGenContext, func_index: usize) -> R<()> {
    let info = c.info;
    let f = &info.functions[func_index];
    let mut arg_index = 0;
    for &arg_type in f.in_arguments.iter().chain(&f.out_arguments) {
        let t = c.get_type(arg_type, 1)?;
        let pointer_type = c.m.type_pointer(StorageClass::FUNCTION, &t, false);
        let spv_arg = c.m.function_parameter(&pointer_type);
        c.declare_argument(arg_index, spv_arg);
        arg_index += 1;
    }
    Ok(())
}

pub fn declare_locals(c: &mut CodeGenContext, func_index: usize) -> R<()> {
    let info = c.info;
    for &local in info.functions[func_index].locals.iter() {
        let t = c.get_type(info.ast.opnd(local).var_type, 1)?;
        let pointer_type = c.m.type_pointer(StorageClass::FUNCTION, &t, false);
        let spv_local = c.m.variable(&pointer_type, StorageClass::FUNCTION, None);
        c.m.add_local_variable(&spv_local);
        c.declare_local(local, spv_local)?;
    }
    Ok(())
}

pub fn declare_all(c: &mut CodeGenContext) -> R<()> {
    let props = c.properties;
    let cbs: Vec<&BufferDefinition> = props.constant_buffers.values().collect();
    declare_buffers(c, &cbs, false)?;
    let sbs: Vec<&BufferDefinition> = props.storage_buffers.values().collect();
    declare_buffers(c, &sbs, true)?;
    for (&id, memory) in props.local_memories.iter() {
        let v = declare_memory(c, memory.ty, memory.array_length, StorageClass::PRIVATE)?;
        c.local_memories.insert(id, v);
    }
    for (&id, memory) in props.shared_memories.iter() {
        let v = declare_memory(c, memory.ty, memory.array_length, StorageClass::WORKGROUP)?;
        c.shared_memories.insert(id, v);
    }
    let textures: Vec<&TextureDefinition> = props.textures.values().collect();
    declare_samplers(c, &textures)?;
    let images: Vec<&TextureDefinition> = props.images.values().collect();
    declare_images(c, &images)?;
    declare_inputs_and_outputs(c)
}

fn declare_memory(c: &mut CodeGenContext, ty: A, array_length: i32, storage: StorageClass) -> R<Inst> {
    let t = c.get_type(ty, array_length)?;
    let pointer_type = c.m.type_pointer(storage, &t, false);
    let variable = c.m.variable(&pointer_type, storage, None);
    c.m.add_global_variable(&variable);
    Ok(variable)
}

fn declare_buffers(c: &mut CodeGenContext, buffers: &[&BufferDefinition], is_buffer: bool) -> R<()> {
    // A type is decorated once (types are deduplicated, so equal types have one id).
    let mut decorated_types: HashSet<u32> = HashSet::new();
    for buffer in buffers {
        let set_index = buffer.set;
        let alignment = if buffer.layout == BufferLayout::Std140 { 16 } else { 4 };
        let alignment_mask = alignment - 1;
        let mut offset = 0;
        let fields = &buffer.ty.fields;
        let mut struct_field_types = Vec::with_capacity(fields.len());
        let mut struct_field_offsets = Vec::with_capacity(fields.len());
        for field in fields {
            let field_size = (field.ty.size_in_bytes() + alignment_mask) & !alignment_mask;
            let t = c.get_type(field.ty, field.array_length)?;
            struct_field_offsets.push(offset);
            if field.ty.has(A::ARRAY) {
                if decorated_types.insert(t.id()) {
                    c.m.decorate(&t, Decoration::ARRAY_STRIDE, &[Lit::i32(field_size).into()]);
                }
                // Zero length: a runtime array, sized by the bound buffer (only the last member).
                offset += field_size * field.array_length;
            } else {
                offset += field_size;
            }
            struct_field_types.push(t);
        }
        let struct_type = c.m.type_struct(false, &struct_field_types);
        if decorated_types.insert(struct_type.id()) {
            c.m.decorate(&struct_type, if is_buffer { Decoration::BUFFER_BLOCK } else { Decoration::BLOCK }, &[]);
            for (i, &o) in struct_field_offsets.iter().enumerate() {
                c.m.member_decorate(&struct_type, Lit::i32(i as i32), Decoration::OFFSET, &[Lit::i32(o).into()]);
            }
        }
        let pointer_type = c.m.type_pointer(StorageClass::UNIFORM, &struct_type, false);
        let variable = c.m.variable(&pointer_type, StorageClass::UNIFORM, None);
        c.m.name(&variable, &buffer.name);
        c.m.decorate(&variable, Decoration::DESCRIPTOR_SET, &[Lit::i32(set_index).into()]);
        c.m.decorate(&variable, Decoration::BINDING, &[Lit::i32(buffer.binding).into()]);
        c.m.add_global_variable(&variable);
        if is_buffer {
            c.storage_buffers.insert(buffer.binding, variable);
        } else {
            c.constant_buffers.insert(buffer.binding, variable);
        }
    }
    Ok(())
}

fn dim_of(ty: SamplerType) -> R<Dim> {
    Ok(match ty.base() {
        SamplerType::TEXTURE_1D => Dim::DIM1_D,
        SamplerType::TEXTURE_2D => Dim::DIM2_D,
        SamplerType::TEXTURE_3D => Dim::DIM3_D,
        SamplerType::TEXTURE_CUBE => Dim::CUBE,
        SamplerType::TEXTURE_BUFFER => Dim::BUFFER,
        t => return Err(format!("System.ArgumentException: Invalid sampler type \"{t}\".")),
    })
}

fn declare_samplers(c: &mut CodeGenContext, samplers: &[&TextureDefinition]) -> R<()> {
    for sampler in samplers {
        let set_index = sampler.set;
        let (image_type, sampled_image_type) = if sampler.ty != SamplerType::NONE {
            let dim = dim_of(sampler.ty)?;
            let f = c.type_fp32();
            let image_type = c.m.type_image(
                &f,
                dim,
                Lit::bool(sampler.ty.has(SamplerType::SHADOW)),
                Lit::bool(sampler.ty.has(SamplerType::ARRAY)),
                Lit::bool(sampler.ty.has(SamplerType::MULTISAMPLE)),
                Lit::i32(1),
                ImageFormat::UNKNOWN,
                None,
                false,
            );
            let sampled = c.m.type_sampled_image(&image_type, false);
            (image_type, sampled)
        } else {
            let s = c.m.type_sampler(false);
            (s.clone(), s)
        };
        let sampled_or_separate_image_type = if sampler.separate { image_type.clone() } else { sampled_image_type.clone() };
        let sampled_image_pointer_type = c.m.type_pointer(StorageClass::UNIFORM_CONSTANT, &sampled_or_separate_image_type, false);
        let mut sampled_image_array_pointer_type = sampled_image_pointer_type.clone();
        if sampler.array_length == 0 {
            let at = c.m.type_runtime_array(&sampled_or_separate_image_type, false);
            sampled_image_array_pointer_type = c.m.type_pointer(StorageClass::UNIFORM_CONSTANT, &at, false);
        } else if sampler.array_length != 1 {
            let len = c.const_u32_i(sampler.array_length);
            let at = c.m.type_array(&sampled_or_separate_image_type, &len, false);
            sampled_image_array_pointer_type = c.m.type_pointer(StorageClass::UNIFORM_CONSTANT, &at, false);
        }
        let sampled_image_variable = c.m.variable(&sampled_image_array_pointer_type, StorageClass::UNIFORM_CONSTANT, None);
        let key = SetBindingPair::new(sampler.set, sampler.binding);
        c.samplers.insert(
            key,
            SamplerDeclaration {
                image_type,
                sampled_image_type,
                sampled_image_pointer_type,
                image: sampled_image_variable.clone(),
                is_indexed: sampler.array_length != 1,
            },
        );
        c.samplers_types.insert(key, sampler.ty);
        c.m.name(&sampled_image_variable, &sampler.name);
        c.m.decorate(&sampled_image_variable, Decoration::DESCRIPTOR_SET, &[Lit::i32(set_index).into()]);
        c.m.decorate(&sampled_image_variable, Decoration::BINDING, &[Lit::i32(sampler.binding).into()]);
        c.m.add_global_variable(&sampled_image_variable);
    }
    Ok(())
}

fn declare_images(c: &mut CodeGenContext, images: &[&TextureDefinition]) -> R<()> {
    for image in images {
        let set_index = image.set;
        let dim = dim_of(image.ty)?;
        let ct = c.get_type(image.format.component_type(), 1)?;
        // AccessQualifier.ReadWrite goes into the "sampled" operand in the original (2: a storage image).
        let image_type = c.m.type_image(
            &ct,
            dim,
            Lit::bool(image.ty.has(SamplerType::SHADOW)),
            Lit::bool(image.ty.has(SamplerType::ARRAY)),
            Lit::bool(image.ty.has(SamplerType::MULTISAMPLE)),
            Lit::en(AccessQualifier::READ_WRITE.0),
            image_format(image.format),
            None,
            false,
        );
        let image_pointer_type = c.m.type_pointer(StorageClass::UNIFORM_CONSTANT, &image_type, false);
        let mut image_array_pointer_type = image_pointer_type.clone();
        if image.array_length == 0 {
            let at = c.m.type_runtime_array(&image_type, false);
            image_array_pointer_type = c.m.type_pointer(StorageClass::UNIFORM_CONSTANT, &at, false);
        } else if image.array_length != 1 {
            let len = c.const_u32_i(image.array_length);
            let at = c.m.type_array(&image_type, &len, false);
            image_array_pointer_type = c.m.type_pointer(StorageClass::UNIFORM_CONSTANT, &at, false);
        }
        let image_variable = c.m.variable(&image_array_pointer_type, StorageClass::UNIFORM_CONSTANT, None);
        c.images.insert(
            SetBindingPair::new(image.set, image.binding),
            ImageDeclaration { image_type, image_pointer_type, image: image_variable.clone(), is_indexed: image.array_length != 1 },
        );
        c.m.name(&image_variable, &image.name);
        c.m.decorate(&image_variable, Decoration::DESCRIPTOR_SET, &[Lit::i32(set_index).into()]);
        c.m.decorate(&image_variable, Decoration::BINDING, &[Lit::i32(image.binding).into()]);
        if image.flags.has(TextureUsageFlags::IMAGE_COHERENT) {
            c.m.decorate(&image_variable, Decoration::COHERENT, &[]);
        }
        c.m.add_global_variable(&image_variable);
    }
    Ok(())
}

fn image_format(format: TextureFormat) -> ImageFormat {
    use TextureFormat as F;
    match format {
        F::Unknown => ImageFormat::UNKNOWN,
        F::R8Unorm => ImageFormat::R8,
        F::R8Snorm => ImageFormat::R8_SNORM,
        F::R8Uint => ImageFormat::R8UI,
        F::R8Sint => ImageFormat::R8I,
        F::R16Float => ImageFormat::R16F,
        F::R16Unorm => ImageFormat::R16,
        F::R16Snorm => ImageFormat::R16_SNORM,
        F::R16Uint => ImageFormat::R16UI,
        F::R16Sint => ImageFormat::R16I,
        F::R32Float => ImageFormat::R32F,
        F::R32Uint => ImageFormat::R32UI,
        F::R32Sint => ImageFormat::R32I,
        F::R8G8Unorm => ImageFormat::RG8,
        F::R8G8Snorm => ImageFormat::RG8_SNORM,
        F::R8G8Uint => ImageFormat::RG8UI,
        F::R8G8Sint => ImageFormat::RG8I,
        F::R16G16Float => ImageFormat::RG16F,
        F::R16G16Unorm => ImageFormat::RG16,
        F::R16G16Snorm => ImageFormat::RG16_SNORM,
        F::R16G16Uint => ImageFormat::RG16UI,
        F::R16G16Sint => ImageFormat::RG16I,
        F::R32G32Float => ImageFormat::RG32F,
        F::R32G32Uint => ImageFormat::RG32UI,
        F::R32G32Sint => ImageFormat::RG32I,
        F::R8G8B8A8Unorm => ImageFormat::RGBA8,
        F::R8G8B8A8Snorm => ImageFormat::RGBA8_SNORM,
        F::R8G8B8A8Uint => ImageFormat::RGBA8UI,
        F::R8G8B8A8Sint => ImageFormat::RGBA8I,
        F::R16G16B16A16Float => ImageFormat::RGBA16F,
        F::R16G16B16A16Unorm => ImageFormat::RGBA16,
        F::R16G16B16A16Snorm => ImageFormat::RGBA16_SNORM,
        F::R16G16B16A16Uint => ImageFormat::RGBA16UI,
        F::R16G16B16A16Sint => ImageFormat::RGBA16I,
        F::R32G32B32A32Float => ImageFormat::RGBA32F,
        F::R32G32B32A32Uint => ImageFormat::RGBA32UI,
        F::R32G32B32A32Sint => ImageFormat::RGBA32I,
        F::R10G10B10A2Unorm => ImageFormat::RGB10_A2,
        F::R10G10B10A2Uint => ImageFormat::RGB10A2UI,
        F::R11G11B10Float => ImageFormat::R11F_G11F_B10F,
    }
}

fn declare_inputs_and_outputs(c: &mut CodeGenContext) -> R<()> {
    let d = c.definitions;
    let info = c.info;
    let mut first_location = i32::MAX;
    if d.stage == ShaderStage::Fragment && d.dual_source_blend() {
        for io in info.io_definitions.iter() {
            if io.io_variable == IoVariable::FragmentOutputColor && io.location < first_location {
                first_location = io.location;
            }
        }
    }
    for io in info.io_definitions.iter() {
        let mut iq = PixelImap::Unused;
        if d.stage == ShaderStage::Fragment {
            if io.io_variable == IoVariable::UserDefined {
                iq = d.imap_types.as_ref().ok_or(crate::translation::optimizations::utils::NRE)?[io.location as usize].first_used_type();
            } else {
                let (_, var_type) = builtin_of(io.io_variable);
                if matches!(var_type.element(), A::S32 | A::U32) {
                    iq = PixelImap::Constant;
                }
            }
        } else if is_per_vertex_builtin(io.io_variable) {
            continue;
        }
        declare_input_or_output(c, *io, io.storage_kind.is_output(), io.storage_kind.is_per_patch(), iq, first_location)?;
    }
    declare_per_vertex_block(c)
}

fn declare_per_vertex_block(c: &mut CodeGenContext) -> R<()> {
    let d = c.definitions;
    if !d.stage.is_vtg() {
        return Ok(());
    }
    if d.stage != ShaderStage::Vertex {
        let input_struct = create_per_vertex_struct_type(c);
        let array_size = if d.stage == ShaderStage::Geometry { d.input_topology().input_vertex_count() } else { 32 };
        let len = c.const_u32_i(array_size);
        let array_type = c.m.type_array(&input_struct, &len, false);
        let pointer_type = c.m.type_pointer(StorageClass::INPUT, &array_type, false);
        let variable = c.m.variable(&pointer_type, StorageClass::INPUT, None);
        c.m.name(&variable, "gl_in");
        c.m.add_global_variable(&variable);
        c.inputs.try_add(IoDefinition { storage_kind: StorageKind::Input, io_variable: IoVariable::Position, location: 0, component: 0 }, variable);
        // No geometry shader passthrough on the host.
    }
    let output_struct = create_per_vertex_struct_type(c);
    // No transform feedback outputs to decorate.
    let output_array_type = if d.stage == ShaderStage::TessellationControl {
        let len = c.const_u32_i(d.threads_per_input_primitive);
        c.m.type_array(&output_struct, &len, false)
    } else {
        output_struct
    };
    let pointer_type = c.m.type_pointer(StorageClass::OUTPUT, &output_array_type, false);
    let variable = c.m.variable(&pointer_type, StorageClass::OUTPUT, None);
    c.m.add_global_variable(&variable);
    c.outputs.try_add(IoDefinition { storage_kind: StorageKind::Output, io_variable: IoVariable::Position, location: 0, component: 0 }, variable);
    Ok(())
}

fn create_per_vertex_struct_type(c: &mut CodeGenContext) -> Inst {
    let f = c.type_fp32();
    let vec4 = c.m.type_vector(&f, Lit::i32(4), false);
    let float_type = c.type_fp32();
    let f = c.type_fp32();
    let len8 = c.const_u32_i(8);
    let array8 = c.m.type_array(&f, &len8, false);
    let f = c.type_fp32();
    let len1 = c.const_u32_i(1);
    let array1 = c.m.type_array(&f, &len1, false);
    let s = c.m.type_struct(true, &[vec4, float_type, array8, array1]);
    c.m.name(&s, "gl_PerVertex");
    c.m.member_name(&s, Lit::i32(0), "gl_Position");
    c.m.member_name(&s, Lit::i32(1), "gl_PointSize");
    c.m.member_name(&s, Lit::i32(2), "gl_ClipDistance");
    c.m.member_name(&s, Lit::i32(3), "gl_CullDistance");
    c.m.decorate(&s, Decoration::BLOCK, &[]);
    if c.host.reduced_precision {
        c.m.member_decorate(&s, Lit::i32(0), Decoration::INVARIANT, &[]);
    }
    c.m.member_decorate(&s, Lit::i32(0), Decoration::BUILT_IN, &[Lit::en(BuiltIn::POSITION.0).into()]);
    c.m.member_decorate(&s, Lit::i32(1), Decoration::BUILT_IN, &[Lit::en(BuiltIn::POINT_SIZE.0).into()]);
    c.m.member_decorate(&s, Lit::i32(2), Decoration::BUILT_IN, &[Lit::en(BuiltIn::CLIP_DISTANCE.0).into()]);
    c.m.member_decorate(&s, Lit::i32(3), Decoration::BUILT_IN, &[Lit::en(BuiltIn::CULL_DISTANCE.0).into()]);
    s
}

fn declare_input_or_output(c: &mut CodeGenContext, io: IoDefinition, is_output: bool, is_per_patch: bool, iq: PixelImap, first_location: i32) -> R<()> {
    let d = c.definitions;
    let io_variable = io.io_variable;
    let storage_class = if is_output { StorageClass::OUTPUT } else { StorageClass::INPUT };
    let (is_builtin, builtin, mut var_type) = if io_variable == IoVariable::UserDefined {
        (false, BuiltIn(0), d.user_defined_type(io.location, is_output))
    } else if io_variable == IoVariable::FragmentOutputColor {
        (false, BuiltIn(0), d.fragment_output_color_type(io.location))
    } else {
        let (b, t) = builtin_of(io_variable);
        if t == A::INVALID {
            return Err(format!("System.InvalidOperationException: Unknown variable {io_variable:?}."));
        }
        (true, b, t)
    };
    let has_component = d.has_per_location_input_or_output_component(io_variable, io.location, io.component, is_output);
    if has_component {
        var_type = var_type.element();
    }
    let mut spv_type = c.get_type(var_type, builtin_array_length(io_variable))?;
    if !is_per_patch && is_per_vertex(io_variable, d.stage, is_output) {
        let array_size = if d.stage == ShaderStage::Geometry { d.input_topology().input_vertex_count() } else { 32 };
        let len = c.const_u32_i(array_size);
        spv_type = c.m.type_array(&spv_type, &len, false);
    }
    if d.stage == ShaderStage::TessellationControl && is_output && !is_per_patch {
        let len = c.const_u32_i(d.threads_per_input_primitive);
        spv_type = c.m.type_array(&spv_type, &len, false);
    }
    let pointer_type = c.m.type_pointer(storage_class, &spv_type, false);
    let spv_var = c.m.variable(&pointer_type, storage_class, None);
    if is_builtin {
        if is_per_patch {
            c.m.decorate(&spv_var, Decoration::PATCH, &[]);
        }
        if c.host.reduced_precision && io_variable == IoVariable::Position {
            c.m.decorate(&spv_var, Decoration::INVARIANT, &[]);
        }
        c.m.decorate(&spv_var, Decoration::BUILT_IN, &[Lit::en(builtin.0).into()]);
    } else if is_per_patch {
        c.m.decorate(&spv_var, Decoration::PATCH, &[]);
        if io_variable == IoVariable::UserDefined {
            c.m.decorate(&spv_var, Decoration::LOCATION, &[Lit::i32(io.location).into()]);
        }
    } else if io_variable == IoVariable::UserDefined {
        c.m.decorate(&spv_var, Decoration::LOCATION, &[Lit::i32(io.location).into()]);
        if has_component {
            c.m.decorate(&spv_var, Decoration::COMPONENT, &[Lit::i32(io.component).into()]);
        }
    } else if io_variable == IoVariable::FragmentOutputColor {
        let location = io.location;
        if d.stage == ShaderStage::Fragment && d.dual_source_blend() {
            let index = location - first_location;
            if (index as u32) < 2 {
                c.m.decorate(&spv_var, Decoration::LOCATION, &[Lit::i32(first_location).into()]);
                c.m.decorate(&spv_var, Decoration::INDEX, &[Lit::i32(index).into()]);
            } else {
                c.m.decorate(&spv_var, Decoration::LOCATION, &[Lit::i32(location).into()]);
            }
        } else {
            c.m.decorate(&spv_var, Decoration::LOCATION, &[Lit::i32(location).into()]);
        }
    }
    if !is_output {
        match iq {
            PixelImap::Constant => {
                c.m.decorate(&spv_var, Decoration::FLAT, &[]);
            }
            PixelImap::ScreenLinear => {
                c.m.decorate(&spv_var, Decoration::NO_PERSPECTIVE, &[]);
            }
            _ => {}
        }
    }
    c.m.add_global_variable(&spv_var);
    let dict = match (is_per_patch, is_output) {
        (true, true) => &mut c.outputs_per_patch,
        (true, false) => &mut c.inputs_per_patch,
        (false, true) => &mut c.outputs,
        (false, false) => &mut c.inputs,
    };
    if !dict.try_add(io, spv_var) {
        return Err("System.ArgumentException: An item with the same key has already been added.".into());
    }
    Ok(())
}

/// IoMap.GetSpirvBuiltIn.
pub fn builtin_of(v: IoVariable) -> (BuiltIn, A) {
    use IoVariable as V;
    match v {
        V::BaseInstance => (BuiltIn::BASE_INSTANCE, A::S32),
        V::BaseVertex => (BuiltIn::BASE_VERTEX, A::S32),
        V::ClipDistance => (BuiltIn::CLIP_DISTANCE, A::ARRAY | A::FP32),
        V::CtaId => (BuiltIn::WORKGROUP_ID, A::VECTOR3 | A::U32),
        V::DrawIndex => (BuiltIn::DRAW_INDEX, A::S32),
        V::FragmentCoord => (BuiltIn::FRAG_COORD, A::VECTOR4 | A::FP32),
        V::FragmentOutputDepth => (BuiltIn::FRAG_DEPTH, A::FP32),
        V::FrontFacing => (BuiltIn::FRONT_FACING, A::BOOL),
        V::GlobalId => (BuiltIn::GLOBAL_INVOCATION_ID, A::VECTOR3 | A::U32),
        V::InstanceId => (BuiltIn::INSTANCE_ID, A::S32),
        V::InstanceIndex => (BuiltIn::INSTANCE_INDEX, A::S32),
        V::InvocationId => (BuiltIn::INVOCATION_ID, A::S32),
        V::Layer => (BuiltIn::LAYER, A::S32),
        V::PatchVertices => (BuiltIn::PATCH_VERTICES, A::S32),
        V::PointCoord => (BuiltIn::POINT_COORD, A::VECTOR2 | A::FP32),
        V::PointSize => (BuiltIn::POINT_SIZE, A::FP32),
        V::Position => (BuiltIn::POSITION, A::VECTOR4 | A::FP32),
        V::PrimitiveId => (BuiltIn::PRIMITIVE_ID, A::S32),
        V::SubgroupEqMask => (BuiltIn::SUBGROUP_EQ_MASK, A::VECTOR4 | A::U32),
        V::SubgroupGeMask => (BuiltIn::SUBGROUP_GE_MASK, A::VECTOR4 | A::U32),
        V::SubgroupGtMask => (BuiltIn::SUBGROUP_GT_MASK, A::VECTOR4 | A::U32),
        V::SubgroupLaneId => (BuiltIn::SUBGROUP_LOCAL_INVOCATION_ID, A::U32),
        V::SubgroupLeMask => (BuiltIn::SUBGROUP_LE_MASK, A::VECTOR4 | A::U32),
        V::SubgroupLtMask => (BuiltIn::SUBGROUP_LT_MASK, A::VECTOR4 | A::U32),
        V::TessellationCoord => (BuiltIn::TESS_COORD, A::VECTOR3 | A::FP32),
        V::TessellationLevelInner => (BuiltIn::TESS_LEVEL_INNER, A::ARRAY | A::FP32),
        V::TessellationLevelOuter => (BuiltIn::TESS_LEVEL_OUTER, A::ARRAY | A::FP32),
        V::ThreadId => (BuiltIn::LOCAL_INVOCATION_ID, A::VECTOR3 | A::U32),
        V::ThreadKill => (BuiltIn::HELPER_INVOCATION, A::BOOL),
        V::VertexId => (BuiltIn::VERTEX_ID, A::S32),
        V::VertexIndex => (BuiltIn::VERTEX_INDEX, A::S32),
        V::ViewportIndex => (BuiltIn::VIEWPORT_INDEX, A::S32),
        V::ViewportMask => (BuiltIn::VIEWPORT_MASK_NV, A::ARRAY | A::S32),
        _ => (BuiltIn(0), A::INVALID),
    }
}

fn builtin_array_length(v: IoVariable) -> i32 {
    match v {
        IoVariable::ClipDistance => 8,
        IoVariable::TessellationLevelInner => 2,
        IoVariable::TessellationLevelOuter => 4,
        IoVariable::ViewportMask => 1,
        IoVariable::UserDefined => 16,
        _ => 1,
    }
}

fn is_per_vertex(v: IoVariable, stage: ShaderStage, is_output: bool) -> bool {
    use IoVariable as V;
    matches!(v, V::Layer | V::ViewportIndex | V::PointSize | V::Position | V::UserDefined | V::ClipDistance | V::PointCoord | V::ViewportMask)
        && !is_output
        && matches!(stage, ShaderStage::TessellationControl | ShaderStage::TessellationEvaluation | ShaderStage::Geometry)
}

pub fn is_per_vertex_builtin(v: IoVariable) -> bool {
    matches!(v, IoVariable::Position | IoVariable::PointSize | IoVariable::ClipDistance)
}

pub fn is_per_vertex_array_builtin(sk: StorageKind, stage: ShaderStage) -> bool {
    if sk == StorageKind::Output {
        stage == ShaderStage::TessellationControl
    } else {
        matches!(stage, ShaderStage::TessellationControl | ShaderStage::TessellationEvaluation | ShaderStage::Geometry)
    }
}

pub fn per_vertex_struct_field_index(v: IoVariable) -> R<i32> {
    Ok(match v {
        IoVariable::Position => 0,
        IoVariable::PointSize => 1,
        IoVariable::ClipDistance => 2,
        v => return Err(format!("System.ArgumentException: Invalid built-in variable {v:?}.")),
    })
}
