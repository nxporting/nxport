use super::emit_helper::*;
use crate::ir::instruction::Instruction;
use crate::ir::operand::Operand;
use crate::translation::emitter_context::EmitterContext;

pub fn emit_tex(
    context: &mut EmitterContext,
    handle: i32,
    src_coords: i32,
    rd: i32,
) {
    let handle_op = Operand::constant(handle);
    let coords_op = get_src_reg(src_coords);

    let res = context.alloc_var();
    context.add(Instruction::TextureSample, Some(res), vec![handle_op, coords_op]);
    context.copy(get_dest(rd), res);
}

pub fn emit_texs(
    context: &mut EmitterContext,
    handle: i32,
    src_a: i32,
    src_b: i32,
    rd: i32,
) {
    let handle_op = Operand::constant(handle);
    let ca = get_src_reg(src_a);
    let cb = get_src_reg(src_b);

    let res = context.alloc_var();
    context.add(Instruction::TextureSample, Some(res), vec![handle_op, ca, cb]);
    context.copy(get_dest(rd), res);
}
