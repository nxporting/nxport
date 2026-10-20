// Bindless accesses whose handle is loaded with a computed index from a constant buffer become indexed accesses to an
// array of textures (BindlessToArray.RunPass; the OpenGL variant is not used).

use super::utils::find_last_operation;
use crate::accessor::GpuAccessor;
use crate::ir::{BlockId, Instruction as I, Ir, NodeId, OperandType, StorageKind, TextureFlags};
use crate::translation::resource_manager::ResourceManager;
use crate::types::{pack_offsets, pack_slots, TextureHandleType};

/// One or zero elements are not arrays.
pub const MINIMUM_ARRAY_LENGTH: i32 = 2;

pub fn run_pass(ir: &mut Ir, block: BlockId, rm: &mut ResourceManager, gpu: &GpuAccessor) {
    let mut node = ir.first_node(block);
    while let Some(n) = node {
        try_turn_into_array(ir, block, rm, gpu, n);
        node = ir.next_node(n);
    }
}

fn try_turn_into_array(ir: &mut Ir, block: BlockId, rm: &mut ResourceManager, gpu: &GpuAccessor, n: NodeId) {
    let Some(tex) = ir.tex(n).copied() else { return };
    if !tex.flags.has(TextureFlags::BINDLESS) {
        return;
    }
    let bindless_handle = find_last_operation(ir, ir.src(n, 0), block, true);
    let Some(mut handle_asg_op) = ir.asg_op(bindless_handle).filter(|&a| ir.is_op(a)) else { return };
    let (mut secondary_cbuf_slot, mut secondary_cbuf_offset, mut has_secondary_handle) = (0, 0, false);
    if ir.inst(handle_asg_op) == I::BITWISE_OR {
        let src0 = find_last_operation(ir, ir.src(handle_asg_op, 0), block, true);
        let src1 = find_last_operation(ir, ir.src(handle_asg_op, 1), block, true);
        let op_of = |o| ir.asg_op(o).filter(|&a| ir.is_op(a));
        if ir.ty(src0) == OperandType::ConstantBuffer && op_of(src1).is_some() {
            handle_asg_op = op_of(src1).unwrap();
            secondary_cbuf_slot = ir.opnd(src0).cbuf_slot();
            secondary_cbuf_offset = ir.opnd(src0).cbuf_offset();
            has_secondary_handle = true;
        } else if op_of(src0).is_some() && ir.ty(src1) == OperandType::ConstantBuffer {
            handle_asg_op = op_of(src0).unwrap();
            secondary_cbuf_slot = ir.opnd(src1).cbuf_slot();
            secondary_cbuf_offset = ir.opnd(src1).cbuf_offset();
            has_secondary_handle = true;
        }
    }
    let h = ir.op(handle_asg_op);
    if h.inst != I::LOAD || h.storage_kind != StorageKind::ConstantBuffer || h.sources.len() != 4 {
        return;
    }
    let ldc_src0 = ir.src(handle_asg_op, 0);
    if ir.ty(ldc_src0) != OperandType::Constant {
        return;
    }
    let Some(src0_cbuf_slot) = rm.try_get_constant_buffer_slot(ir.value(ldc_src0)) else { return };
    // Field index 0 is expected.
    let ldc_src1 = ir.src(handle_asg_op, 1);
    if ir.ty(ldc_src1) != OperandType::Constant || ir.value(ldc_src1) != 0 {
        return;
    }
    let (ldc_vec_index, ldc_elem_index) = (ir.src(handle_asg_op, 2), ir.src(handle_asg_op, 3));
    if ir.ty(ldc_vec_index) != OperandType::LocalVariable || ir.ty(ldc_elem_index) != OperandType::LocalVariable {
        return;
    }
    let (cbuf_slot, handle_index) = if has_secondary_handle {
        (pack_slots(src0_cbuf_slot, secondary_cbuf_slot), pack_offsets(0, secondary_cbuf_offset, TextureHandleType::SeparateSamplerHandle))
    } else {
        (src0_cbuf_slot, 0)
    };
    let length = MINIMUM_ARRAY_LENGTH.max(gpu.texture_array_length_from_buffer(src0_cbuf_slot));
    let sb = rm.texture_or_image_binding(gpu, ir.inst(n), tex.ty, tex.format, tex.flags & !TextureFlags::BINDLESS, cbuf_slot, handle_index, length, false);
    ir.turn_into_array(n, sb);

    let (vec_index, elem_index, index, index_min) = (ir.local(), ir.local(), ir.local(), ir.local());
    let k1 = ir.const_i(1);
    let a = ir.operation(I::SHIFT_LEFT, Some(vec_index), &[ldc_vec_index, k1]);
    ir.add_before(n, a);
    let k1 = ir.const_i(1);
    let b = ir.operation(I::SHIFT_RIGHT_U32, Some(elem_index), &[ldc_elem_index, k1]);
    ir.add_before(n, b);
    let c = ir.operation(I::ADD, Some(index), &[vec_index, elem_index]);
    ir.add_before(n, c);
    let k = ir.const_i(length - 1);
    let d = ir.operation(I::MINIMUM_U32, Some(index_min), &[index, k]);
    ir.add_before(n, d);
    ir.set_source(n, 0, Some(index_min));
}
