use super::emit_helper::*;
use crate::decoders::inst_decoders::*;
use crate::ir::operand::Operand;
use crate::translation::emitter_context::EmitterContext;

pub fn emit_iadd(context: &mut EmitterContext, src_a: Operand, src_b: Operand, rd: i32) {
    let res = context.i_add(src_a, src_b);
    context.copy(get_dest(rd), res);
}

pub fn emit_xmad(
    context: &mut EmitterContext,
    src_a: Operand,
    src_b: Operand,
    src_c: Operand,
    rd: i32,
    psl: bool,
    merge: bool,
) {
    // XMAD is a 16x16 bit multiplication with 32-bit accumulate
    // Product = (src_a & 0xffff) * (src_b & 0xffff)
    // If PSL: Product <<= 16
    // If merge: Product |= (src_a & 0xffff0000)
    // Result = Product + src_c
    let mask_16 = Operand::constant(0xffff);
    let a_16 = context.bitwise_and(src_a, mask_16);
    let b_16 = context.bitwise_and(src_b, mask_16);
    let mut prod = context.i_mul(a_16, b_16);

    if psl {
        let shift_16 = Operand::constant(16);
        prod = context.shift_left(prod, shift_16);
    }

    if merge {
        let mask_hi = Operand::constant(0xffff0000u32 as i32);
        let a_hi = context.bitwise_and(src_a, mask_hi);
        prod = context.bitwise_or(prod, a_hi);
    }

    let res = context.i_add(prod, src_c);
    context.copy(get_dest(rd), res);
}

pub fn emit_lop(context: &mut EmitterContext, op_lop: LogicOp, src_a: Operand, src_b: Operand, rd: i32, inv_a: bool, inv_b: bool) {
    let mut a = src_a;
    let mut b = src_b;
    if inv_a {
        a = context.bitwise_not(a);
    }
    if inv_b {
        b = context.bitwise_not(b);
    }

    let res = match op_lop {
        LogicOp::And => context.bitwise_and(a, b),
        LogicOp::Or => context.bitwise_or(a, b),
        LogicOp::Xor => context.bitwise_xor(a, b),
        _ => context.bitwise_and(a, b),
    };
    context.copy(get_dest(rd), res);
}

pub fn emit_shl(context: &mut EmitterContext, src_a: Operand, shift: Operand, rd: i32) {
    let res = context.shift_left(src_a, shift);
    context.copy(get_dest(rd), res);
}

pub fn emit_shr(context: &mut EmitterContext, src_a: Operand, shift: Operand, rd: i32, signed: bool) {
    let res = if signed {
        context.shift_right_s32(src_a, shift)
    } else {
        context.shift_right_u32(src_a, shift)
    };
    context.copy(get_dest(rd), res);
}

pub fn emit_imnmx(
    context: &mut EmitterContext,
    src_a: Operand,
    src_b: Operand,
    src_pred: i32,
    src_pred_inv: bool,
    rd: i32,
    is_signed: bool,
) {
    let min_val = if is_signed {
        context.i_min_s32(src_a, src_b)
    } else {
        context.i_min_u32(src_a, src_b)
    };
    let max_val = if is_signed {
        context.i_max_s32(src_a, src_b)
    } else {
        context.i_max_u32(src_a, src_b)
    };
    let pred = get_predicate(src_pred, src_pred_inv);
    let res = context.conditional_select(pred, min_val, max_val);
    context.copy(get_dest(rd), res);
}

pub fn emit_iscadd(
    context: &mut EmitterContext,
    src_a: Operand,
    src_b: Operand,
    scale: i32,
    rd: i32,
) {
    let shift_op = Operand::constant(scale);
    let shifted = context.shift_left(src_a, shift_op);
    let res = context.i_add(shifted, src_b);
    context.copy(get_dest(rd), res);
}

pub fn emit_iadd3(
    context: &mut EmitterContext,
    src_a: Operand,
    src_b: Operand,
    src_c: Operand,
    rd: i32,
) {
    let ab = context.i_add(src_a, src_b);
    let res = context.i_add(ab, src_c);
    context.copy(get_dest(rd), res);
}

pub fn emit_bfe(
    context: &mut EmitterContext,
    src_a: Operand,
    src_b: Operand,
    rd: i32,
    signed: bool,
) {
    // BFE extracts a bitfield: src_b[7:0] = offset, src_b[15:8] = count
    let mask_8 = Operand::constant(0xff);
    let offset = context.bitwise_and(src_b, mask_8);
    let shift_8 = Operand::constant(8);
    let shifted = context.shift_right_u32(src_b, shift_8);
    let count = context.bitwise_and(shifted, mask_8);

    let res = context.alloc_var();
    let inst = if signed {
        crate::ir::instruction::Instruction::BitfieldExtractS32
    } else {
        crate::ir::instruction::Instruction::BitfieldExtractU32
    };
    context.add(inst, Some(res), vec![src_a, offset, count]);
    context.copy(get_dest(rd), res);
}

