use super::emit_helper::*;
use crate::decoders::inst_decoders::*;
use crate::decoders::register::RegisterType;
use crate::ir::instruction::Instruction;
use crate::ir::operand::Operand;
use crate::translation::emitter_context::EmitterContext;

pub fn get_int_comparison(
    context: &mut EmitterContext,
    cond: IComp,
    src_a: Operand,
    src_b: Operand,
    is_signed: bool,
) -> Operand {
    let inst = match cond {
        IComp::F => return Operand::constant(0),
        IComp::T => return Operand::constant(1),
        IComp::Eq => Instruction::CompareEqual,
        IComp::Ne => Instruction::CompareNotEqual,
        IComp::Lt => if is_signed { Instruction::CompareLess } else { Instruction::CompareLessU32 },
        IComp::Le => if is_signed { Instruction::CompareLessOrEqual } else { Instruction::CompareLessOrEqualU32 },
        IComp::Gt => if is_signed { Instruction::CompareGreater } else { Instruction::CompareGreaterU32 },
        IComp::Ge => if is_signed { Instruction::CompareGreaterOrEqual } else { Instruction::CompareGreaterOrEqualU32 },
    };

    let res = context.alloc_var();
    context.add(inst, Some(res), vec![src_a, src_b]);
    res
}

pub fn get_float_comparison(
    context: &mut EmitterContext,
    cond: FComp,
    src_a: Operand,
    src_b: Operand,
) -> Operand {
    let inst = match cond {
        FComp::F => return Operand::constant(0),
        FComp::T => return Operand::constant(1),
        FComp::Eq | FComp::Equ => Instruction::CompareEqual,
        FComp::Ne | FComp::Neu => Instruction::CompareNotEqual,
        FComp::Lt | FComp::Ltu => Instruction::CompareLess,
        FComp::Le | FComp::Leu => Instruction::CompareLessOrEqual,
        FComp::Gt | FComp::Gtu => Instruction::CompareGreater,
        FComp::Ge | FComp::Geu => Instruction::CompareGreaterOrEqual,
        _ => Instruction::CompareEqual,
    };

    let res = context.alloc_var();
    context.add(inst, Some(res), vec![src_a, src_b]);
    res
}

pub fn apply_bool_op(context: &mut EmitterContext, op: BoolOp, a: Operand, b: Operand) -> Operand {
    match op {
        BoolOp::And => context.bitwise_and(a, b),
        BoolOp::Or => context.bitwise_or(a, b),
        BoolOp::Xor => context.bitwise_xor(a, b),
    }
}

pub fn emit_iset(
    context: &mut EmitterContext,
    cond: IComp,
    bop: BoolOp,
    src_a: Operand,
    src_b: Operand,
    src_pred: i32,
    src_pred_inv: bool,
    rd: i32,
    bool_float: bool,
    signed: bool,
) {
    let mut cmp = get_int_comparison(context, cond, src_a, src_b, signed);
    let pred = get_predicate(src_pred, src_pred_inv);
    cmp = apply_bool_op(context, bop, cmp, pred);

    let res = if bool_float {
        let one = Operand::constant(1.0f32.to_bits() as i32);
        let zero = Operand::constant(0);
        context.conditional_select(cmp, one, zero)
    } else {
        cmp
    };

    context.copy(get_dest(rd), res);
}

pub fn emit_isetp(
    context: &mut EmitterContext,
    cond: IComp,
    bop: BoolOp,
    src_a: Operand,
    src_b: Operand,
    src_pred: i32,
    src_pred_inv: bool,
    dest_pred: i32,
    dest_pred_inv: i32,
    signed: bool,
) {
    let cmp = get_int_comparison(context, cond, src_a, src_b, signed);
    let pred = get_predicate(src_pred, src_pred_inv);
    let p0 = apply_bool_op(context, bop, cmp, pred);
    let not_cmp = context.bitwise_not(cmp);
    let p1 = apply_bool_op(context, bop, not_cmp, pred);

    context.copy(Operand::register(dest_pred as usize, RegisterType::Predicate), p0);
    context.copy(Operand::register(dest_pred_inv as usize, RegisterType::Predicate), p1);
}

pub fn emit_fset(
    context: &mut EmitterContext,
    cond: FComp,
    bop: BoolOp,
    src_a: Operand,
    src_b: Operand,
    src_pred: i32,
    src_pred_inv: bool,
    rd: i32,
    bool_float: bool,
) {
    let mut cmp = get_float_comparison(context, cond, src_a, src_b);
    let pred = get_predicate(src_pred, src_pred_inv);
    cmp = apply_bool_op(context, bop, cmp, pred);

    let res = if bool_float {
        let one = Operand::constant(1.0f32.to_bits() as i32);
        let zero = Operand::constant(0);
        context.conditional_select(cmp, one, zero)
    } else {
        cmp
    };

    context.copy(get_dest(rd), res);
}

pub fn emit_fsetp(
    context: &mut EmitterContext,
    cond: FComp,
    bop: BoolOp,
    src_a: Operand,
    src_b: Operand,
    src_pred: i32,
    src_pred_inv: bool,
    dest_pred: i32,
    dest_pred_inv: i32,
) {
    let cmp = get_float_comparison(context, cond, src_a, src_b);
    let pred = get_predicate(src_pred, src_pred_inv);
    let p0 = apply_bool_op(context, bop, cmp, pred);
    let not_cmp = context.bitwise_not(cmp);
    let p1 = apply_bool_op(context, bop, not_cmp, pred);

    context.copy(Operand::register(dest_pred as usize, RegisterType::Predicate), p0);
    context.copy(Operand::register(dest_pred_inv as usize, RegisterType::Predicate), p1);
}
