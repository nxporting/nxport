use super::emit_helper::*;
use crate::ir::operand::Operand;
use crate::translation::emitter_context::EmitterContext;

pub fn emit_fadd(context: &mut EmitterContext, src_a: Operand, src_b: Operand, rd: i32) {
    let res = context.f_add(src_a, src_b);
    context.copy(get_dest(rd), res);
}

pub fn emit_fmul(context: &mut EmitterContext, src_a: Operand, src_b: Operand, rd: i32) {
    let res = context.f_mul(src_a, src_b);
    context.copy(get_dest(rd), res);
}

pub fn emit_ffma(context: &mut EmitterContext, src_a: Operand, src_b: Operand, src_c: Operand, rd: i32) {
    let res = context.f_fma(src_a, src_b, src_c);
    context.copy(get_dest(rd), res);
}

pub fn emit_fmnmx(
    context: &mut EmitterContext,
    src_a: Operand,
    src_b: Operand,
    src_pred: i32,
    src_pred_inv: bool,
    rd: i32,
) {
    let min_val = context.f_min(src_a, src_b);
    let max_val = context.f_max(src_a, src_b);
    let pred = get_predicate(src_pred, src_pred_inv);
    let res = context.conditional_select(pred, min_val, max_val);
    context.copy(get_dest(rd), res);
}

pub fn emit_mufu(
    context: &mut EmitterContext,
    mufu_op: crate::decoders::inst_decoders::MufuOp,
    src_a: Operand,
    rd: i32,
) {
    use crate::decoders::inst_decoders::MufuOp;
    let res = match mufu_op {
        MufuOp::Cos => context.f_cos(src_a),
        MufuOp::Sin => context.f_sin(src_a),
        MufuOp::Ex2 => context.f_exp2(src_a),
        MufuOp::Lg2 => context.f_log2(src_a),
        MufuOp::Rsq => context.f_rsqrt(src_a),
        MufuOp::Sqrt => context.f_sqrt(src_a),
        _ => src_a,
    };
    context.copy(get_dest(rd), res);
}

pub fn emit_rro(context: &mut EmitterContext, src_b: Operand, rd: i32) {
    context.copy(get_dest(rd), src_b);
}

