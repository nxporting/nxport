use super::emit_helper::*;
use crate::decoders::inst_decoders::*;
use crate::translation::emitter_context::EmitterContext;

pub fn emit_mov_r(context: &mut EmitterContext, op: &InstMovR) {
    let src = get_src_reg(op.src_a());
    context.copy(get_dest(op.dest()), src);
}

pub fn emit_mov_i(context: &mut EmitterContext, op: &InstMovI) {
    let src = get_src_imm(op.imm20());
    context.copy(get_dest(op.dest()), src);
}

pub fn emit_mov_c(context: &mut EmitterContext, op: &InstMovC) {
    let src = get_src_cbuf(op.cbuf_slot(), op.cbuf_offset());
    context.copy(get_dest(op.dest()), src);
}

pub fn emit_mov32i(context: &mut EmitterContext, op: &InstMov32i) {
    let src = get_src_imm(op.imm32());
    context.copy(get_dest(op.dest()), src);
}

pub fn emit_sel_r(context: &mut EmitterContext, op: &InstSelR) {
    let a = get_src_reg(op.src_a());
    let b = get_src_reg(op.src_b());
    let pred = get_predicate(op.src_pred(), op.src_pred_inv());
    let res = context.conditional_select(pred, a, b);
    context.copy(get_dest(op.dest()), res);
}

pub fn emit_sel_i(context: &mut EmitterContext, op: &InstSelI) {
    let a = get_src_reg(op.src_a());
    let b = get_src_imm(op.imm20());
    let pred = get_predicate(op.src_pred(), op.src_pred_inv());
    let res = context.conditional_select(pred, a, b);
    context.copy(get_dest(op.dest()), res);
}

pub fn emit_sel_c(context: &mut EmitterContext, op: &InstSelC) {
    let a = get_src_reg(op.src_a());
    let b = get_src_cbuf(op.cbuf_slot(), op.cbuf_offset());
    let pred = get_predicate(op.src_pred(), op.src_pred_inv());
    let res = context.conditional_select(pred, a, b);
    context.copy(get_dest(op.dest()), res);
}
