// Integer comparisons and min/max (InstEmitIntegerComparison, InstEmitIntegerMinMax).

use super::helper::*;
use crate::decoders::inst_decoders::*;
use crate::ir::{ir_consts, Instruction as I, OperandId};
use crate::tri;
use crate::translation::emitter::{Emit, EmitterContext as Ctx};

pub fn icmp_r(c: &mut Ctx) {
    let op = InstIcmpR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    tri!(c, emit_icmp(c, op.i_comp(), a, b, s, op.dest(), op.signed()));
}

pub fn icmp_i(c: &mut Ctx) {
    let op = InstIcmpI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    let s = get_src_reg(c, op.src_c(), false);
    tri!(c, emit_icmp(c, op.i_comp(), a, b, s, op.dest(), op.signed()));
}

pub fn icmp_c(c: &mut Ctx) {
    let op = InstIcmpC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let s = get_src_reg(c, op.src_c(), false);
    tri!(c, emit_icmp(c, op.i_comp(), a, b, s, op.dest(), op.signed()));
}

pub fn icmp_rc(c: &mut Ctx) {
    let op = InstIcmpRc(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_c(), false);
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    tri!(c, emit_icmp(c, op.i_comp(), a, b, s, op.dest(), op.signed()));
}

pub fn iset_r(c: &mut Ctx) {
    let op = InstIsetR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    tri!(c, emit_iset(c, op.i_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest(), op.b_val(), op.signed(), op.x(), op.write_cc()));
}

pub fn iset_i(c: &mut Ctx) {
    let op = InstIsetI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    tri!(c, emit_iset(c, op.i_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest(), op.b_val(), op.signed(), op.x(), op.write_cc()));
}

pub fn iset_c(c: &mut Ctx) {
    let op = InstIsetC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    tri!(c, emit_iset(c, op.i_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest(), op.b_val(), op.signed(), op.x(), op.write_cc()));
}

pub fn isetp_r(c: &mut Ctx) {
    let op = InstIsetpR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    tri!(c, emit_isetp(c, op.i_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest_pred(), op.dest_pred_inv(), op.signed(), op.x()));
}

pub fn isetp_i(c: &mut Ctx) {
    let op = InstIsetpI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    tri!(c, emit_isetp(c, op.i_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest_pred(), op.dest_pred_inv(), op.signed(), op.x()));
}

pub fn isetp_c(c: &mut Ctx) {
    let op = InstIsetpC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    tri!(c, emit_isetp(c, op.i_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest_pred(), op.dest_pred_inv(), op.signed(), op.x()));
}

fn emit_icmp(c: &mut Ctx, cmp: IComp, a: OperandId, b: OperandId, s: OperandId, rd: i32, signed: bool) -> Result<(), String> {
    let z = c.const_i(0);
    let cmp_res = int_comparison(c, cmp, s, z, signed)?;
    let res = c.conditional_select(cmp_res, a, b);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_iset(
    c: &mut Ctx,
    cmp: IComp,
    logic_op: BoolOp,
    a: OperandId,
    b: OperandId,
    src_pred: i32,
    src_pred_inv: bool,
    rd: i32,
    bool_float: bool,
    signed: bool,
    extended: bool,
    write_cc: bool,
) -> Result<(), String> {
    let res = int_comparison_ext(c, cmp, a, b, signed, extended)?;
    let pred = get_predicate(c, src_pred, src_pred_inv);
    let mut res = get_pred_logical_op(c, logic_op, res, pred);
    let dest = get_dest(c, rd);
    if bool_float {
        let (one, zero) = (c.const_f(1.0), c.const_i(0));
        res = c.conditional_select(res, one, zero);
        c.copy_to(dest, res);
        set_fp_zn_flags(c, res, write_cc, I::FP32);
    } else {
        c.copy_to(dest, res);
        set_zn_flags(c, res, write_cc, extended);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_isetp(
    c: &mut Ctx,
    cmp: IComp,
    logic_op: BoolOp,
    a: OperandId,
    b: OperandId,
    src_pred: i32,
    src_pred_inv: bool,
    dest_pred: i32,
    dest_pred_inv: i32,
    signed: bool,
    extended: bool,
) -> Result<(), String> {
    let p0 = int_comparison_ext(c, cmp, a, b, signed, extended)?;
    let p1 = c.bitwise_not(p0);
    let pred = get_predicate(c, src_pred, src_pred_inv);
    let p0 = get_pred_logical_op(c, logic_op, p0, pred);
    let p1 = get_pred_logical_op(c, logic_op, p1, pred);
    let d0 = c.pred(dest_pred);
    c.copy_to(d0, p0);
    let d1 = c.pred(dest_pred_inv);
    c.copy_to(d1, p1);
    Ok(())
}

pub fn int_comparison_ext(c: &mut Ctx, cond: IComp, a: OperandId, b: OperandId, signed: bool, extended: bool) -> Result<OperandId, String> {
    if extended { int_comparison_extended(c, cond, a, b, signed) } else { int_comparison(c, cond, a, b, signed) }
}

fn int_comparison_extended(c: &mut Ctx, cond: IComp, a: OperandId, b: OperandId, signed: bool) -> Result<OperandId, String> {
    if cond == IComp::T {
        return Ok(c.const_i(ir_consts::TRUE));
    }
    if cond == IComp::F {
        return Ok(c.const_i(ir_consts::FALSE));
    }
    // Computed and then not used, as in the original.
    let sub = c.isubtract(a, b);
    let cf = get_cf(c);
    let ncf = c.bitwise_not(cf);
    c.iadd(sub, ncf);

    Ok(match cond {
        // r = xh == yh && xl == yl
        IComp::EQ => {
            let eq = c.icompare_equal(a, b);
            let zf = get_zf(c);
            c.bitwise_and(eq, zf)
        }
        // r = xh < yh || (xh == yh && xl < yl)
        IComp::LT => {
            let cf = get_cf(c);
            let not_c = c.bitwise_not(cf);
            let eq = c.icompare_equal(a, b);
            let prev = c.bitwise_and(eq, not_c);
            let lt = if signed { c.icompare_less(a, b) } else { c.icompare_less_unsigned(a, b) };
            c.bitwise_or(lt, prev)
        }
        // r = xh < yh || (xh == yh && xl <= yl)
        IComp::LE => {
            let zf = get_zf(c);
            let cf = get_cf(c);
            let ncf = c.bitwise_not(cf);
            let z_or_not_c = c.bitwise_or(zf, ncf);
            let eq = c.icompare_equal(a, b);
            let prev = c.bitwise_and(eq, z_or_not_c);
            let lt = if signed { c.icompare_less(a, b) } else { c.icompare_less_unsigned(a, b) };
            c.bitwise_or(lt, prev)
        }
        // r = xh > yh || (xh == yh && xl > yl)
        IComp::GT => {
            let zf = get_zf(c);
            let nzf = c.bitwise_not(zf);
            let cf = get_cf(c);
            let not_z_and_c = c.bitwise_and(nzf, cf);
            let eq = c.icompare_equal(a, b);
            let prev = c.bitwise_and(eq, not_z_and_c);
            let gt = if signed { c.icompare_greater(a, b) } else { c.icompare_greater_unsigned(a, b) };
            c.bitwise_or(gt, prev)
        }
        // r = xh > yh || (xh == yh && xl >= yl)
        IComp::GE => {
            let eq = c.icompare_equal(a, b);
            let cf = get_cf(c);
            let prev = c.bitwise_and(eq, cf);
            let gt = if signed { c.icompare_greater(a, b) } else { c.icompare_greater_unsigned(a, b) };
            c.bitwise_or(gt, prev)
        }
        // r = xh != yh || xl != yl
        IComp::NE => {
            let ne = c.icompare_not_equal(a, b);
            let zf = get_zf(c);
            let nzf = c.bitwise_not(zf);
            c.bitwise_or(ne, nzf)
        }
        _ => return Err(format!("Unexpected condition \"{}\".", cond.0)),
    })
}

fn int_comparison(c: &mut Ctx, cond: IComp, a: OperandId, b: OperandId, signed: bool) -> Result<OperandId, String> {
    if cond == IComp::T {
        return Ok(c.const_i(ir_consts::TRUE));
    }
    if cond == IComp::F {
        return Ok(c.const_i(ir_consts::FALSE));
    }
    let mut inst = match cond {
        IComp::LT => I::COMPARE_LESS_U32,
        IComp::EQ => I::COMPARE_EQUAL,
        IComp::LE => I::COMPARE_LESS_OR_EQUAL_U32,
        IComp::GT => I::COMPARE_GREATER_U32,
        IComp::NE => I::COMPARE_NOT_EQUAL,
        IComp::GE => I::COMPARE_GREATER_OR_EQUAL_U32,
        _ => return Err(format!("Unexpected condition \"{}\".", cond.0)),
    };
    if signed {
        match cond {
            IComp::LT => inst = I::COMPARE_LESS,
            IComp::LE => inst = I::COMPARE_LESS_OR_EQUAL,
            IComp::GT => inst = I::COMPARE_GREATER,
            IComp::GE => inst = I::COMPARE_GREATER_OR_EQUAL,
            _ => {}
        }
    }
    let d = c.local();
    c.add(inst, Some(d), &[a, b]);
    Ok(d)
}

pub fn imnmx_r(c: &mut Ctx) {
    let op = InstImnmxR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_imnmx(c, a, b, p, op.dest(), op.signed(), op.write_cc());
}

pub fn imnmx_i(c: &mut Ctx) {
    let op = InstImnmxI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_imnmx(c, a, b, p, op.dest(), op.signed(), op.write_cc());
}

pub fn imnmx_c(c: &mut Ctx) {
    let op = InstImnmxC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_imnmx(c, a, b, p, op.dest(), op.signed(), op.write_cc());
}

fn emit_imnmx(c: &mut Ctx, a: OperandId, b: OperandId, pred: OperandId, rd: i32, signed: bool, write_cc: bool) {
    let min = if signed { c.iminimum_s32(a, b) } else { c.iminimum_u32(a, b) };
    let max = if signed { c.imaximum_s32(a, b) } else { c.imaximum_u32(a, b) };
    let res = c.conditional_select(pred, min, max);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
    set_zn_flags(c, res, write_cc, false);
}
