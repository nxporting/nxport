use super::context::CodeGenContext;
use crate::ir::instruction::Instruction;
use crate::ir::storage_kind::{AggregateType, IoVariable, StorageKind};
use crate::structured_ir::ast::{AstArena, AstNodeKind, AstOperation};
use crate::structured_ir::program::IoDefinition;
use rspirv::spirv;
use rspirv::spirv::Word;

pub struct Instructions;

impl Instructions {
    pub fn generate(context: &mut CodeGenContext, arena: &AstArena, operation: &AstOperation) -> Option<Word> {
        let inst = operation.inst;
        let sources = &operation.sources;

        match inst {
            Instruction::Copy => {
                if let Some(&src) = sources.first() {
                    Some(context.get_value(arena, src, AggregateType::FP32))
                } else {
                    None
                }
            }

            Instruction::Add => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_add(context.f32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::Subtract => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_sub(context.f32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::Multiply => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_mul(context.f32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::Divide => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_div(context.f32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::FusedMultiplyAdd => {
                if sources.len() >= 3 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    let c = context.get_value(arena, sources[2], AggregateType::FP32);
                    context
                        .builder
                        .ext_inst(
                            context.f32_type,
                            None,
                            context.glsl_ext_inst,
                            26, // Fma
                            vec![
                                rspirv::dr::Operand::IdRef(a),
                                rspirv::dr::Operand::IdRef(b),
                                rspirv::dr::Operand::IdRef(c),
                            ],
                        )
                        .ok()
                } else {
                    None
                }
            }

            Instruction::BitwiseAnd => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::U32);
                    let b = context.get_value(arena, sources[1], AggregateType::U32);
                    context.builder.bitwise_and(context.u32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::BitwiseOr => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::U32);
                    let b = context.get_value(arena, sources[1], AggregateType::U32);
                    context.builder.bitwise_or(context.u32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::BitwiseExclusiveOr => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::U32);
                    let b = context.get_value(arena, sources[1], AggregateType::U32);
                    context.builder.bitwise_xor(context.u32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::BitwiseNot => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::U32);
                    context.builder.not(context.u32_type, None, a).ok()
                } else {
                    None
                }
            }

            Instruction::ShiftLeft => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::U32);
                    let b = context.get_value(arena, sources[1], AggregateType::U32);
                    context.builder.shift_left_logical(context.u32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::ShiftRightU32 => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::U32);
                    let b = context.get_value(arena, sources[1], AggregateType::U32);
                    context.builder.shift_right_logical(context.u32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::ShiftRightS32 => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::S32);
                    let b = context.get_value(arena, sources[1], AggregateType::U32);
                    context.builder.shift_right_arithmetic(context.s32_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::CompareEqual => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_ord_equal(context.bool_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::CompareNotEqual => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_ord_not_equal(context.bool_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::CompareLess => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_ord_less_than(context.bool_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::CompareGreater => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_ord_greater_than(context.bool_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::CompareLessOrEqual => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_ord_less_than_equal(context.bool_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::CompareGreaterOrEqual => {
                if sources.len() >= 2 {
                    let a = context.get_value(arena, sources[0], AggregateType::FP32);
                    let b = context.get_value(arena, sources[1], AggregateType::FP32);
                    context.builder.f_ord_greater_than_equal(context.bool_type, None, a, b).ok()
                } else {
                    None
                }
            }

            Instruction::ConditionalSelect => {
                if sources.len() >= 3 {
                    let cond = context.get_value(arena, sources[0], AggregateType::BOOL);
                    let true_v = context.get_value(arena, sources[1], AggregateType::FP32);
                    let false_v = context.get_value(arena, sources[2], AggregateType::FP32);
                    context.builder.select(context.f32_type, None, cond, true_v, false_v).ok()
                } else {
                    None
                }
            }

            Instruction::Cosine => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::FP32);
                    context
                        .builder
                        .ext_inst(context.f32_type, None, context.glsl_ext_inst, 14, vec![rspirv::dr::Operand::IdRef(a)])
                        .ok()
                } else {
                    None
                }
            }

            Instruction::Sine => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::FP32);
                    context
                        .builder
                        .ext_inst(context.f32_type, None, context.glsl_ext_inst, 13, vec![rspirv::dr::Operand::IdRef(a)])
                        .ok()
                } else {
                    None
                }
            }

            Instruction::ExponentB2 => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::FP32);
                    context
                        .builder
                        .ext_inst(context.f32_type, None, context.glsl_ext_inst, 30, vec![rspirv::dr::Operand::IdRef(a)])
                        .ok()
                } else {
                    None
                }
            }

            Instruction::LogarithmB2 => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::FP32);
                    context
                        .builder
                        .ext_inst(context.f32_type, None, context.glsl_ext_inst, 29, vec![rspirv::dr::Operand::IdRef(a)])
                        .ok()
                } else {
                    None
                }
            }

            Instruction::SquareRoot => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::FP32);
                    context
                        .builder
                        .ext_inst(context.f32_type, None, context.glsl_ext_inst, 31, vec![rspirv::dr::Operand::IdRef(a)])
                        .ok()
                } else {
                    None
                }
            }

            Instruction::ReciprocalSquareRoot => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::FP32);
                    context
                        .builder
                        .ext_inst(context.f32_type, None, context.glsl_ext_inst, 32, vec![rspirv::dr::Operand::IdRef(a)])
                        .ok()
                } else {
                    None
                }
            }

            Instruction::ConvertFP32ToS32 => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::FP32);
                    context.builder.convert_f_to_s(context.s32_type, None, a).ok()
                } else {
                    None
                }
            }

            Instruction::ConvertFP32ToU32 => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::FP32);
                    context.builder.convert_f_to_u(context.u32_type, None, a).ok()
                } else {
                    None
                }
            }

            Instruction::ConvertS32ToFP32 => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::S32);
                    context.builder.convert_s_to_f(context.f32_type, None, a).ok()
                } else {
                    None
                }
            }

            Instruction::ConvertU32ToFP32 => {
                if let Some(&src) = sources.first() {
                    let a = context.get_value(arena, src, AggregateType::U32);
                    context.builder.convert_u_to_f(context.f32_type, None, a).ok()
                } else {
                    None
                }
            }

            Instruction::Load => {
                Self::generate_load(context, arena, operation)
            }

            Instruction::Store => {
                Self::generate_store(context, arena, operation);
                None
            }

            Instruction::Discard => {
                let _ = context.builder.kill();
                None
            }

            Instruction::Return => {
                let _ = context.builder.ret();
                None
            }

            _ => {
                // Fallback: 0
                Some(context.const_f32(0.0))
            }
        }
    }

    fn generate_load(context: &mut CodeGenContext, arena: &AstArena, operation: &AstOperation) -> Option<Word> {
        match operation.storage_kind {
            StorageKind::ConstantBuffer => {
                // sources: [slot, field(0), vec_idx, elem_idx]
                if operation.sources.len() >= 4 {
                    let slot_val = match &arena.nodes[operation.sources[0].0].kind {
                        AstNodeKind::Operand(op) => op.value,
                        _ => 0,
                    };
                    let vec_idx = context.get_value(arena, operation.sources[2], AggregateType::U32);
                    let elem_idx = context.get_value(arena, operation.sources[3], AggregateType::U32);

                    if let Some(&cb_var) = context.constant_buffers.get(&slot_val) {
                        let zero = context.const_u32(0);
                        let ptr_type = context.builder.type_pointer(None, spirv::StorageClass::Uniform, context.f32_type);
                        let ptr = context
                            .builder
                            .access_chain(ptr_type, None, cb_var, vec![zero, vec_idx, elem_idx])
                            .ok()?;
                        context.builder.load(context.f32_type, None, ptr, None, vec![]).ok()
                    } else {
                        Some(context.const_f32(0.0))
                    }
                } else {
                    Some(context.const_f32(0.0))
                }
            }
            StorageKind::Input | StorageKind::InputPerPatch => {
                // sources: [var_id, loc, comp]
                let var_id_val = match arena.nodes[operation.sources[0].0].kind {
                    AstNodeKind::Operand(ref op) => op.value,
                    _ => 0,
                };
                let loc = operation.sources.get(1).map(|&s| match arena.nodes[s.0].kind {
                    AstNodeKind::Operand(ref op) => op.value,
                    _ => 0,
                }).unwrap_or(0);
                let comp = operation.sources.get(2).map(|&s| match arena.nodes[s.0].kind {
                    AstNodeKind::Operand(ref op) => op.value,
                    _ => 0,
                }).unwrap_or(0);

                let io_var = unsafe { std::mem::transmute::<u32, IoVariable>(var_id_val as u32) };
                let io_def = IoDefinition::new(operation.storage_kind, io_var, loc, comp);

                if let Some(&in_var) = context.inputs.get(&io_def) {
                    let comp_val = context.const_u32(comp as u32);
                    let ptr_type = context.builder.type_pointer(None, spirv::StorageClass::Input, context.f32_type);
                    let ptr = context
                        .builder
                        .access_chain(ptr_type, None, in_var, vec![comp_val])
                        .ok()?;
                    context.builder.load(context.f32_type, None, ptr, None, vec![]).ok()
                } else {
                    Some(context.const_f32(0.0))
                }
            }
            _ => Some(context.const_f32(0.0)),
        }
    }

    fn generate_store(context: &mut CodeGenContext, arena: &AstArena, operation: &AstOperation) {
        if !operation.storage_kind.is_output() || operation.sources.len() < 2 {
            return;
        }

        let var_id_val = match arena.nodes[operation.sources[0].0].kind {
            AstNodeKind::Operand(ref op) => op.value,
            _ => 0,
        };
        let loc = operation.sources.get(1).map(|&s| match arena.nodes[s.0].kind {
            AstNodeKind::Operand(ref op) => op.value,
            _ => 0,
        }).unwrap_or(0);
        let comp = operation.sources.get(2).map(|&s| match arena.nodes[s.0].kind {
            AstNodeKind::Operand(ref op) => op.value,
            _ => 0,
        }).unwrap_or(0);

        let io_var = unsafe { std::mem::transmute::<u32, IoVariable>(var_id_val as u32) };
        let io_def = IoDefinition::new(operation.storage_kind, io_var, loc, comp);

        let val_node = *operation.sources.last().unwrap();
        let val = context.get_value(arena, val_node, AggregateType::FP32);

        if let Some(&out_var) = context.outputs.get(&io_def) {
            let comp_val = context.const_u32(comp as u32);
            let ptr_type = context.builder.type_pointer(None, spirv::StorageClass::Output, context.f32_type);
            if let Ok(ptr) = context.builder.access_chain(ptr_type, None, out_var, vec![comp_val]) {
                let _ = context.builder.store(ptr, val, None, vec![]);
            }
        }
    }
}
