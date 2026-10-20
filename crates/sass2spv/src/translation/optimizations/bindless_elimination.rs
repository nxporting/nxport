// Bindless texture accesses become regular ones where the handle is recognisably a constant buffer value (separate
// texture and sampler handles), else accesses into the whole texture pool; failing that they are removed
// (BindlessElimination).

use super::bindless_to_array::MINIMUM_ARRAY_LENGTH;
use super::utils::{delete_node, find_last_operation};
use crate::accessor::GpuAccessor;
use crate::instructions::SAMPLE_1D_AS_2D;
use crate::ir::{BlockId, Instruction as I, Ir, NodeId, OperandId, OperandType, StorageKind, TextureFlags};
use crate::translation::properties::{texture_format, texture_format_atomic};
use crate::translation::resource_manager::ResourceManager;
use crate::types::{pack_offsets, pack_slots, SamplerType, TextureFormat, TextureHandleType};

pub fn run_pass(ir: &mut Ir, block: BlockId, rm: &mut ResourceManager, gpu: &GpuAccessor) -> Result<(), String> {
    let mut node = ir.first_node(block);
    while let Some(n) = node {
        let next_node = ir.next_node(n);
        let Some(tex) = ir.tex(n).copied() else {
            node = next_node;
            continue;
        };
        if tex.flags.has(TextureFlags::BINDLESS) && !try_convert_bindless(ir, block, rm, gpu, n)? && !generate_bindless_access(ir, rm, gpu, n) {
            // Neither works: the access goes and its destinations read zero.
            let inst = ir.inst(n);
            let type_name = if inst.is_image() { format!("image {}", tex.ty) } else { format!("sampler {}", tex.ty) };
            gpu.log_msg(&format!("Failed to find handle source for bindless access of type \"{type_name}\"."));
            for i in 0..ir.dests_count(n) {
                let d = ir.get_dest(n, i);
                let z = ir.const_i(0);
                let copy = ir.operation(I::COPY, d, &[z]);
                ir.add_before(n, copy);
            }
            delete_node(ir, n);
        }
        node = next_node;
    }
    Ok(())
}

fn op_of(ir: &Ir, o: OperandId) -> Option<NodeId> {
    ir.asg_op(o).filter(|&a| ir.is_op(a))
}

/// Accesses through the whole texture (and sampler) pool, for handles from inputs, storage buffers and constants.
fn generate_bindless_access(ir: &mut Ir, rm: &mut ResourceManager, gpu: &GpuAccessor, n: NodeId) -> bool {
    if !gpu.host_supports_separate_sampler() {
        // Combining samplers and textures in the shader is needed.
        return false;
    }
    let bindless_handle = ir.src(n, 0);
    if let Some(phi) = ir.asg_op(bindless_handle).filter(|&a| ir.is_phi(a)) {
        for i in 0..ir.sources_count(phi) {
            let s = ir.src(phi, i);
            if !ir.asg_op(s).is_some_and(|a| ir.is_phi(a)) && !is_bindless_access_allowed(ir, s) {
                return false;
            }
        }
    } else if !is_bindless_access_allowed(ir, bindless_handle) {
        return false;
    }
    let (texture_handle, sampler_handle, texture_index) = (ir.local(), ir.local(), ir.local());
    let k = ir.const_i(0xfffff);
    let a = ir.operation(I::BITWISE_AND, Some(texture_handle), &[bindless_handle, k]);
    ir.add_before(n, a);
    let k = ir.const_i(20);
    let b = ir.operation(I::SHIFT_RIGHT_U32, Some(sampler_handle), &[bindless_handle, k]);
    ir.add_before(n, b);
    let texture_pool_length = MINIMUM_ARRAY_LENGTH.max(gpu.texture_array_length_from_pool());
    let k = ir.const_i(texture_pool_length - 1);
    let c = ir.operation(I::MINIMUM_U32, Some(texture_index), &[texture_handle, k]);
    ir.add_before(n, c);
    ir.set_source(n, 0, Some(texture_index));

    let inst = ir.inst(n);
    let tex = *ir.tex(n).unwrap();
    let has_sampler = !inst.is_image();
    let direct = pack_offsets(0, 0, TextureHandleType::Direct);
    let texture_sb =
        rm.texture_or_image_binding(gpu, inst, tex.ty, tex.format, tex.flags & !TextureFlags::BINDLESS, 0, direct, texture_pool_length, has_sampler);
    if has_sampler {
        let sampler_index = ir.local();
        let sampler_pool_length = MINIMUM_ARRAY_LENGTH.max(gpu.sampler_array_length_from_pool());
        let k = ir.const_i(sampler_pool_length - 1);
        let d = ir.operation(I::MINIMUM_U32, Some(sampler_index), &[sampler_handle, k]);
        ir.add_before(n, d);
        ir.insert_source(n, 1, sampler_index);
        let sampler_sb = rm.texture_or_image_binding(gpu, inst, SamplerType::NONE, tex.format, TextureFlags::NONE, 0, direct, sampler_pool_length, false);
        ir.turn_into_array_with_sampler(n, texture_sb, sampler_sb);
    } else {
        ir.turn_into_array(n, texture_sb);
    }
    true
}

fn is_bindless_access_allowed(ir: &Ir, bindless_handle: OperandId) -> bool {
    if ir.ty(bindless_handle) == OperandType::ConstantBuffer {
        return true;
    }
    // Only handles from shader inputs or storage buffers: loading the whole pool would be slow otherwise.
    op_of(ir, bindless_handle).is_some_and(|h| {
        let o = ir.op(h);
        o.inst == I::LOAD && matches!(o.storage_kind, StorageKind::Input | StorageKind::StorageBuffer)
    })
}

fn try_convert_bindless(ir: &mut Ir, block: BlockId, rm: &mut ResourceManager, gpu: &GpuAccessor, n: NodeId) -> Result<bool, String> {
    let inst = ir.inst(n);
    let tex = *ir.tex(n).unwrap();
    if inst == I::TEXTURE_SAMPLE || inst.is_texture_query() {
        let mut bindless_handle = ir.src(n, 0);
        // Some textureGrad forms get the handle through a shuffle: skip it.
        if let Some(shuffle_op) = op_of(ir, bindless_handle).filter(|&s| ir.inst(s) == I::SHUFFLE) {
            bindless_handle = ir.src(shuffle_op, 0);
        }
        bindless_handle = find_last_operation(ir, bindless_handle, block, true);
        // Instructions without an accurate sampler type (1D vs buffer, queries) take it from the GPU state once the
        // descriptor's location is known.
        let rewrite_sampler_type = tex.ty == SamplerType::TEXTURE_BUFFER || inst == I::TEXTURE_QUERY_SAMPLES || inst == I::TEXTURE_QUERY_SIZE;
        if ir.ty(bindless_handle) == OperandType::ConstantBuffer {
            let (offset, slot) = (ir.opnd(bindless_handle).cbuf_offset(), ir.opnd(bindless_handle).cbuf_slot());
            set_handle(ir, rm, gpu, n, offset, slot, rewrite_sampler_type, false);
            return Ok(true);
        }
        let Some(handle_combine_op) = try_get_operation(ir, ir.asg_op(bindless_handle)) else { return Ok(false) };
        if ir.inst(handle_combine_op) != I::BITWISE_OR {
            return Ok(false);
        }
        let mut src0 = find_last_operation(ir, ir.src(handle_combine_op, 0), block, true);
        let mut src1 = find_last_operation(ir, ir.src(handle_combine_op, 1), block, true);
        // A constant goes second (the OR commutes).
        if ir.ty(src0) == OperandType::Constant && ir.ty(src1) != OperandType::Constant {
            std::mem::swap(&mut src0, &mut src1);
        }
        let mut handle_type = TextureHandleType::SeparateSamplerHandle;
        // Masked: (sampler & 0xFFF00000) | (texture & 0xFFFFF); shifted: (samplerId << 20) | texture; constant:
        // samplerHandleConstant | texture; the handles and ids from constant buffers.
        if let Some(src0_asg_op) = op_of(ir, src0) {
            let src1_asg_op = op_of(ir, src1);
            if src1_asg_op.is_some_and(|s| ir.inst(s) == I::BITWISE_AND) && ir.inst(src0_asg_op) == I::BITWISE_AND {
                let src1_asg_op = src1_asg_op.unwrap();
                let mut a = source_for_masked_handle(ir, src0_asg_op, 0xFFFFF);
                let mut b = source_for_masked_handle(ir, src1_asg_op, 0xFFF00000);
                if a.is_none() || b.is_none() {
                    a = source_for_masked_handle(ir, src1_asg_op, 0xFFFFF);
                    b = source_for_masked_handle(ir, src0_asg_op, 0xFFF00000);
                }
                let (Some(a), Some(b)) = (a, b) else { return Ok(false) };
                src0 = a;
                src1 = b;
            } else if ir.inst(src0_asg_op) == I::SHIFT_LEFT {
                let shift = ir.src(src0_asg_op, 1);
                if ir.ty(shift) == OperandType::Constant && ir.value(shift) == 20 {
                    src0 = src1;
                    src1 = ir.src(src0_asg_op, 0);
                    handle_type = TextureHandleType::SeparateSamplerId;
                }
            }
        } else if let Some(src1_asg_op) = op_of(ir, src1).filter(|&s| ir.inst(s) == I::SHIFT_LEFT) {
            let shift = ir.src(src1_asg_op, 1);
            if ir.ty(shift) == OperandType::Constant && ir.value(shift) == 20 {
                src1 = ir.src(src1_asg_op, 0);
                handle_type = TextureHandleType::SeparateSamplerId;
            }
        } else if ir.ty(src1) == OperandType::Constant && ir.value(src1) & 0xfffff == 0 {
            handle_type = TextureHandleType::SeparateConstantSamplerHandle;
        }
        if ir.ty(src0) != OperandType::ConstantBuffer {
            return Ok(false);
        }
        if handle_type == TextureHandleType::SeparateConstantSamplerHandle {
            let offsets = pack_offsets(ir.opnd(src0).cbuf_offset(), (ir.value(src1) >> 20) & 0xfff, handle_type);
            let slots = pack_slots(ir.opnd(src0).cbuf_slot(), 0);
            set_handle(ir, rm, gpu, n, offsets, slots, rewrite_sampler_type, false);
            return Ok(true);
        } else if ir.ty(src1) == OperandType::ConstantBuffer {
            let offsets = pack_offsets(ir.opnd(src0).cbuf_offset(), ir.opnd(src1).cbuf_offset(), handle_type);
            let slots = pack_slots(ir.opnd(src0).cbuf_slot(), ir.opnd(src1).cbuf_slot());
            set_handle(ir, rm, gpu, n, offsets, slots, rewrite_sampler_type, false);
            return Ok(true);
        }
    } else if inst.is_image() {
        let src0 = find_last_operation(ir, ir.src(n, 0), block, true);
        if ir.ty(src0) == OperandType::ConstantBuffer {
            let (cbuf_offset, cbuf_slot) = (ir.opnd(src0).cbuf_offset(), ir.opnd(src0).cbuf_slot());
            if tex.format == TextureFormat::Unknown {
                ir.tex_mut(n).format = if inst == I::IMAGE_ATOMIC {
                    texture_format_atomic(gpu, cbuf_offset, cbuf_slot)
                } else {
                    texture_format(gpu, cbuf_offset, cbuf_slot)
                };
            }
            let rewrite_sampler_type = tex.ty == SamplerType::TEXTURE_BUFFER;
            set_handle(ir, rm, gpu, n, cbuf_offset, cbuf_slot, rewrite_sampler_type, true);
            return Ok(true);
        }
    }
    Ok(false)
}

/// Sees through a phi whose sources are all the same value.
fn try_get_operation(ir: &Ir, asg_op: Option<NodeId>) -> Option<NodeId> {
    let mut asg_op = asg_op?;
    if ir.is_phi(asg_op) {
        let first_src = ir.src(asg_op, 0);
        for i in 1..ir.sources_count(asg_op) {
            if !is_same_operand(ir, first_src, ir.src(asg_op, i)) {
                return None;
            }
        }
        asg_op = ir.asg_op(first_src)?;
    }
    ir.is_op(asg_op).then_some(asg_op)
}

fn is_same_operand(ir: &Ir, x: OperandId, y: OperandId) -> bool {
    if ir.ty(x) == ir.ty(y) && ir.ty(x) == OperandType::LocalVariable {
        let (Some(xo), Some(yo)) = (op_of(ir, x), op_of(ir, y)) else { return false };
        return ir.inst(xo) == I::BITWISE_OR
            && ir.inst(yo) == I::BITWISE_OR
            && are_both_equal_constant_buffers(ir, ir.src(xo, 0), ir.src(yo, 0))
            && are_both_equal_constant_buffers(ir, ir.src(xo, 1), ir.src(yo, 1));
    }
    false
}

fn are_both_equal_constant_buffers(ir: &Ir, x: OperandId, y: OperandId) -> bool {
    ir.ty(x) == ir.ty(y) && ir.value(x) == ir.value(y) && ir.ty(x) == OperandType::ConstantBuffer
}

/// The constant buffer value of an AND with `mask` (the operation is known to be an AND).
fn source_for_masked_handle(ir: &Ir, asg_op: NodeId, mask: u32) -> Option<OperandId> {
    let (s0, s1) = (ir.src(asg_op, 0), ir.src(asg_op, 1));
    match (ir.ty(s0), ir.ty(s1)) {
        // Both from constant buffers: assume the mask matches; avoid buffer 1, where drivers keep compiler constants.
        (OperandType::ConstantBuffer, OperandType::ConstantBuffer) => Some(if ir.opnd(s0).cbuf_slot() == 1 { s1 } else { s0 }),
        (OperandType::ConstantBuffer, OperandType::Constant) => (ir.value(s1) as u32 == mask).then_some(s0),
        (OperandType::Constant, OperandType::ConstantBuffer) => (ir.value(s0) as u32 == mask).then_some(s1),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn set_handle(ir: &mut Ir, rm: &mut ResourceManager, gpu: &GpuAccessor, n: NodeId, cbuf_offset: i32, cbuf_slot: i32, rewrite_sampler_type: bool, is_image: bool) {
    let inst = ir.inst(n);
    if rewrite_sampler_type {
        let mut new_type = gpu.sampler_type(cbuf_offset, cbuf_slot);
        let tex = *ir.tex(n).unwrap();
        if inst.is_texture_query() {
            ir.tex_mut(n).ty = new_type;
        } else if tex.ty == SamplerType::TEXTURE_BUFFER && new_type == SamplerType::TEXTURE_1D {
            let mut coords_count = 2;
            if SAMPLE_1D_AS_2D {
                new_type = SamplerType::TEXTURE_2D;
                let z = ir.const_i(0);
                ir.insert_source(n, coords_count, z);
                coords_count += 1;
            }
            if !is_image && tex.flags.has(TextureFlags::INT_COORDS) && !tex.flags.has(TextureFlags::LOD_LEVEL) {
                // Integer coordinates always need an explicit level.
                ir.set_lod_level_flag(n);
                let z = ir.const_i(0);
                ir.insert_source(n, coords_count, z);
            }
            ir.tex_mut(n).ty = new_type;
        }
    }
    let tex = *ir.tex(n).unwrap();
    let sb = rm.texture_or_image_binding(gpu, inst, tex.ty, tex.format, tex.flags & !TextureFlags::BINDLESS, cbuf_slot, cbuf_offset, 1, false);
    ir.set_binding(n, sb);
}
