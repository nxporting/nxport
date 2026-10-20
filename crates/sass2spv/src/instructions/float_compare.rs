// Float comparisons (InstEmitFloatComparison).

use super::helper::*;
use crate::decoders::inst_decoders::*;
use crate::ir::{ir_consts, Instruction as I, OperandId};
use crate::tri;
use crate::translation::emitter::{Emit, EmitterContext as Ctx};

macro_rules! dset {
    ($name:ident, $op:ident, $b:expr) => {
        pub fn $name(c: &mut Ctx) {
            let op = $op(c.raw_op());
            let a = get_src_reg(c, op.src_a(), true);
            #[allow(clippy::redundant_closure_call)]
            let b = ($b)(c, &op);
            tri!(c, emit_fset(c, op.f_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), op.b_val(), op.write_cc(), true));
        }
    };
}

dset!(dset_r, InstDsetR, |c: &mut Ctx, op: &InstDsetR| get_src_reg(c, op.src_b(), true));
dset!(dset_i, InstDsetI, |c: &mut Ctx, op: &InstDsetI| get_src_imm(c, imm20_to_float(op.imm20()), true));
dset!(dset_c, InstDsetC, |c: &mut Ctx, op: &InstDsetC| get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), true));

macro_rules! dsetp {
    ($name:ident, $op:ident, $b:expr) => {
        pub fn $name(c: &mut Ctx) {
            let op = $op(c.raw_op());
            let a = get_src_reg(c, op.src_a(), true);
            #[allow(clippy::redundant_closure_call)]
            let b = ($b)(c, &op);
            tri!(c, emit_fsetp(c, op.f_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest_pred(), op.dest_pred_inv(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), false, true));
        }
    };
}

dsetp!(dsetp_r, InstDsetpR, |c: &mut Ctx, op: &InstDsetpR| get_src_reg(c, op.src_b(), true));
dsetp!(dsetp_i, InstDsetpI, |c: &mut Ctx, op: &InstDsetpI| get_src_imm(c, imm20_to_float(op.imm20()), true));
dsetp!(dsetp_c, InstDsetpC, |c: &mut Ctx, op: &InstDsetpC| get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), true));

pub fn fcmp_r(c: &mut Ctx) {
    let op = InstFcmpR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    tri!(c, emit_fcmp(c, op.f_comp(), a, b, s, op.dest()));
}

pub fn fcmp_i(c: &mut Ctx) {
    let op = InstFcmpI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_float(op.imm20()), false);
    let s = get_src_reg(c, op.src_c(), false);
    tri!(c, emit_fcmp(c, op.f_comp(), a, b, s, op.dest()));
}

pub fn fcmp_c(c: &mut Ctx) {
    let op = InstFcmpC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let s = get_src_reg(c, op.src_c(), false);
    tri!(c, emit_fcmp(c, op.f_comp(), a, b, s, op.dest()));
}

pub fn fcmp_rc(c: &mut Ctx) {
    let op = InstFcmpRc(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_c(), false);
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    tri!(c, emit_fcmp(c, op.f_comp(), a, b, s, op.dest()));
}

macro_rules! fset {
    ($name:ident, $op:ident, $b:expr) => {
        pub fn $name(c: &mut Ctx) {
            let op = $op(c.raw_op());
            let a = get_src_reg(c, op.src_a(), false);
            #[allow(clippy::redundant_closure_call)]
            let b = ($b)(c, &op);
            tri!(c, emit_fset(c, op.f_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), op.b_val(), op.write_cc(), false));
        }
    };
}

fset!(fset_r, InstFsetR, |c: &mut Ctx, op: &InstFsetR| get_src_reg(c, op.src_b(), false));
fset!(fset_c, InstFsetC, |c: &mut Ctx, op: &InstFsetC| get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false));
fset!(fset_i, InstFsetI, |c: &mut Ctx, op: &InstFsetI| get_src_imm(c, imm20_to_float(op.imm20()), false));

macro_rules! fsetp {
    ($name:ident, $op:ident, $b:expr) => {
        pub fn $name(c: &mut Ctx) {
            let op = $op(c.raw_op());
            let a = get_src_reg(c, op.src_a(), false);
            #[allow(clippy::redundant_closure_call)]
            let b = ($b)(c, &op);
            tri!(c, emit_fsetp(c, op.f_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest_pred(), op.dest_pred_inv(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), op.write_cc(), false));
        }
    };
}

fsetp!(fsetp_r, InstFsetpR, |c: &mut Ctx, op: &InstFsetpR| get_src_reg(c, op.src_b(), false));
fsetp!(fsetp_i, InstFsetpI, |c: &mut Ctx, op: &InstFsetpI| get_src_imm(c, imm20_to_float(op.imm20()), false));
fsetp!(fsetp_c, InstFsetpC, |c: &mut Ctx, op: &InstFsetpC| get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false));

pub fn hset2_r(c: &mut Ctx) {
    let op = InstHset2R(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = tri!(c, get_half_src_reg(c, op.b_swizzle(), op.src_b(), op.neg_b(), op.abs_b()));
    tri!(c, emit_hset2(c, op.cmp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest(), op.bval()));
}

pub fn hset2_i(c: &mut Ctx) {
    let op = InstHset2I(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = get_half_src_imm2(c, op.bimm_h0(), op.bimm_h1());
    tri!(c, emit_hset2(c, op.cmp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest(), op.bval()));
}

pub fn hset2_c(c: &mut Ctx) {
    let op = InstHset2C(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = tri!(c, get_half_src_cbuf(c, HalfSwizzle::F32, op.cbuf_slot(), op.cbuf_offset(), op.neg_b(), false));
    tri!(c, emit_hset2(c, op.cmp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest(), op.bval()));
}

pub fn hsetp2_r(c: &mut Ctx) {
    let op = InstHsetp2R(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = tri!(c, get_half_src_reg(c, op.b_swizzle(), op.src_b(), op.neg_b(), op.abs_b()));
    tri!(c, emit_hsetp2(c, op.f_comp2(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest_pred(), op.dest_pred_inv(), op.h_and()));
}

pub fn hsetp2_i(c: &mut Ctx) {
    let op = InstHsetp2I(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = get_half_src_imm2(c, op.bimm_h0(), op.bimm_h1());
    tri!(c, emit_hsetp2(c, op.f_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest_pred(), op.dest_pred_inv(), op.h_and()));
}

pub fn hsetp2_c(c: &mut Ctx) {
    let op = InstHsetp2C(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = tri!(c, get_half_src_cbuf(c, HalfSwizzle::F32, op.cbuf_slot(), op.cbuf_offset(), op.neg_b(), op.abs_b()));
    tri!(c, emit_hsetp2(c, op.f_comp(), op.bop(), a, b, op.src_pred(), op.src_pred_inv(), op.dest_pred(), op.dest_pred_inv(), op.h_and()));
}

fn emit_fcmp(c: &mut Ctx, cmp: FComp, a: OperandId, b: OperandId, s: OperandId, rd: i32) -> Result<(), String> {
    let z = c.const_f(0.0);
    let cmp_res = fp_comparison(c, cmp, s, z, I::FP32)?;
    let res = c.conditional_select(cmp_res, a, b);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_fset(
    c: &mut Ctx,
    cmp: FComp,
    logic_op: BoolOp,
    a: OperandId,
    b: OperandId,
    src_pred: i32,
    src_pred_inv: bool,
    rd: i32,
    abs_a: bool,
    abs_b: bool,
    neg_a: bool,
    neg_b: bool,
    bool_float: bool,
    write_cc: bool,
    fp64: bool,
) -> Result<(), String> {
    let fp = if fp64 { I::FP64 } else { I::FP32 };
    let a = c.fp_abs_neg(a, abs_a, neg_a, fp);
    let b = c.fp_abs_neg(b, abs_b, neg_b, fp);
    let res = fp_comparison(c, cmp, a, b, fp)?;
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
        set_zn_flags(c, res, write_cc, false);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_fsetp(
    c: &mut Ctx,
    cmp: FComp,
    logic_op: BoolOp,
    a: OperandId,
    b: OperandId,
    src_pred: i32,
    src_pred_inv: bool,
    dest_pred: i32,
    dest_pred_inv: i32,
    abs_a: bool,
    abs_b: bool,
    neg_a: bool,
    neg_b: bool,
    _write_cc: bool,
    fp64: bool,
) -> Result<(), String> {
    let fp = if fp64 { I::FP64 } else { I::FP32 };
    let a = c.fp_abs_neg(a, abs_a, neg_a, fp);
    let b = c.fp_abs_neg(b, abs_b, neg_b, fp);
    let p0 = fp_comparison(c, cmp, a, b, fp)?;
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

#[allow(clippy::too_many_arguments)]
fn emit_hset2(
    c: &mut Ctx,
    cmp: FComp,
    logic_op: BoolOp,
    a: [OperandId; 2],
    b: [OperandId; 2],
    src_pred: i32,
    src_pred_inv: bool,
    rd: i32,
    bool_float: bool,
) -> Result<(), String> {
    let r0 = fp_comparison(c, cmp, a[0], b[0], I::FP32)?;
    let r1 = fp_comparison(c, cmp, a[1], b[1], I::FP32)?;
    let pred = get_predicate(c, src_pred, src_pred_inv);
    let mut r0 = get_pred_logical_op(c, logic_op, r0, pred);
    let mut r1 = get_pred_logical_op(c, logic_op, r1, pred);
    if bool_float {
        let (one, zero) = (c.const_f(1.0), c.const_i(0));
        r0 = c.conditional_select(r0, one, zero);
        let (one, zero) = (c.const_f(1.0), c.const_i(0));
        r1 = c.conditional_select(r1, one, zero);
        let d = get_dest(c, rd);
        let p = c.pack_half_2x16(r0, r1);
        c.copy_to(d, p);
    } else {
        let m = c.const_i(0xffff);
        let low = c.bitwise_and(r0, m);
        let k = c.const_i(16);
        let high = c.shift_left(r1, k);
        let packed = c.bitwise_or(low, high);
        let d = get_dest(c, rd);
        c.copy_to(d, packed);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_hsetp2(
    c: &mut Ctx,
    cmp: FComp,
    logic_op: BoolOp,
    a: [OperandId; 2],
    b: [OperandId; 2],
    src_pred: i32,
    src_pred_inv: bool,
    dest_pred: i32,
    dest_pred_inv: i32,
    h_and: bool,
) -> Result<(), String> {
    let mut p0 = fp_comparison(c, cmp, a[0], b[0], I::FP32)?;
    let mut p1 = fp_comparison(c, cmp, a[1], b[1], I::FP32)?;
    if h_and {
        p0 = c.bitwise_and(p0, p1);
        p1 = c.bitwise_not(p0);
    }
    let pred = get_predicate(c, src_pred, src_pred_inv);
    let p0 = get_pred_logical_op(c, logic_op, p0, pred);
    let p1 = get_pred_logical_op(c, logic_op, p1, pred);
    let d0 = c.pred(dest_pred);
    c.copy_to(d0, p0);
    let d1 = c.pred(dest_pred_inv);
    c.copy_to(d1, p1);
    Ok(())
}

pub fn fp_comparison(c: &mut Ctx, cond: FComp, a: OperandId, b: OperandId, fp: I) -> Result<OperandId, String> {
    if cond == FComp::T {
        return Ok(c.const_i(ir_consts::TRUE));
    }
    if cond == FComp::F {
        return Ok(c.const_i(ir_consts::FALSE));
    }
    if cond == FComp::NAN || cond == FComp::NUM {
        let na = c.is_nan(a, fp);
        let nb = c.is_nan(b, fp);
        let mut res = c.bitwise_or(na, nb);
        if cond == FComp::NUM {
            res = c.bitwise_not(res);
        }
        return Ok(res);
    }
    let inst = match FComp(cond.0 & !FComp::NAN.0) {
        FComp::LT => I::COMPARE_LESS,
        FComp::EQ => I::COMPARE_EQUAL,
        FComp::LE => I::COMPARE_LESS_OR_EQUAL,
        FComp::GT => I::COMPARE_GREATER,
        FComp::NE => I::COMPARE_NOT_EQUAL,
        FComp::GE => I::COMPARE_GREATER_OR_EQUAL,
        _ => return Err(format!("Unexpected condition \"{}\".", cond.0)),
    };
    let d = c.local();
    c.add(inst | fp, Some(d), &[a, b]);
    let mut res = d;
    if cond.0 & FComp::NAN.0 != 0 {
        let na = c.is_nan(a, fp);
        res = c.bitwise_or(res, na);
        let nb = c.is_nan(b, fp);
        res = c.bitwise_or(res, nb);
    }
    Ok(res)
}
