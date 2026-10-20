use super::emit_helper::*;
use crate::ir::operand::Operand;
use crate::translation::emitter_context::EmitterContext;

pub fn emit_f2f(
    context: &mut EmitterContext,
    src: Operand,
    rd: i32,
    _abs: bool,
    _neg: bool,
) {
    context.copy(get_dest(rd), src);
}

pub fn emit_f2i(
    context: &mut EmitterContext,
    src: Operand,
    rd: i32,
    is_signed: bool,
) {
    let res = if is_signed {
        context.convert_fp_to_s32(src)
    } else {
        context.convert_fp_to_u32(src)
    };
    context.copy(get_dest(rd), res);
}

pub fn emit_i2f(
    context: &mut EmitterContext,
    src: Operand,
    rd: i32,
    is_signed: bool,
) {
    let res = if is_signed {
        context.convert_s32_to_fp(src)
    } else {
        context.convert_u32_to_fp(src)
    };
    context.copy(get_dest(rd), res);
}

pub fn emit_i2i(
    context: &mut EmitterContext,
    src: Operand,
    rd: i32,
) {
    context.copy(get_dest(rd), src);
}
