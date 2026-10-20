// Integer arithmetic and logic (InstEmitIntegerArithmetic, InstEmitIntegerLogical, Lop3Expression).

use super::helper::*;
use crate::decoders::inst_decoders::*;
use crate::decoders::register::PREDICATE_TRUE_INDEX as PT;
use crate::ir::{ir_consts, OperandId, OperandType};
use crate::translation::emitter::{Emit, EmitterContext as Ctx};

pub fn iadd_r(c: &mut Ctx) {
    let op = InstIaddR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    emit_iadd(c, a, b, op.dest(), op.avg_mode(), op.x(), op.write_cc());
}

pub fn iadd_i(c: &mut Ctx) {
    let op = InstIaddI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_iadd(c, a, b, op.dest(), op.avg_mode(), op.x(), op.write_cc());
}

pub fn iadd_c(c: &mut Ctx) {
    let op = InstIaddC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_iadd(c, a, b, op.dest(), op.avg_mode(), op.x(), op.write_cc());
}

pub fn iadd32i(c: &mut Ctx) {
    let op = InstIadd32i(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, op.imm32(), false);
    emit_iadd(c, a, b, op.dest(), op.avg_mode(), op.x(), op.write_cc());
}

pub fn iadd3_r(c: &mut Ctx) {
    let op = InstIadd3R(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_iadd3(c, op.lrs(), a, b, s, op.apart(), op.bpart(), op.cpart(), op.dest(), op.neg_a(), op.neg_b(), op.neg_c());
}

pub fn iadd3_i(c: &mut Ctx) {
    let op = InstIadd3I(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    let s = get_src_reg(c, op.src_c(), false);
    let h = HalfSelect::B32;
    emit_iadd3(c, Lrs::NONE, a, b, s, h, h, h, op.dest(), op.neg_a(), op.neg_b(), op.neg_c());
}

pub fn iadd3_c(c: &mut Ctx) {
    let op = InstIadd3C(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let s = get_src_reg(c, op.src_c(), false);
    let h = HalfSelect::B32;
    emit_iadd3(c, Lrs::NONE, a, b, s, h, h, h, op.dest(), op.neg_a(), op.neg_b(), op.neg_c());
}

pub fn imad_r(c: &mut Ctx) {
    let op = InstImadR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_imad(c, a, b, s, op.dest(), op.avg_mode(), op.a_signed(), op.b_signed(), op.hilo());
}

pub fn imad_i(c: &mut Ctx) {
    let op = InstImadI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_imad(c, a, b, s, op.dest(), op.avg_mode(), op.a_signed(), op.b_signed(), op.hilo());
}

pub fn imad_c(c: &mut Ctx) {
    let op = InstImadC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_imad(c, a, b, s, op.dest(), op.avg_mode(), op.a_signed(), op.b_signed(), op.hilo());
}

pub fn imad_rc(c: &mut Ctx) {
    let op = InstImadRc(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_c(), false);
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_imad(c, a, b, s, op.dest(), op.avg_mode(), op.a_signed(), op.b_signed(), op.hilo());
}

pub fn imad32i(c: &mut Ctx) {
    let op = InstImad32i(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, op.imm32(), false);
    let s = get_src_reg(c, op.dest(), false);
    emit_imad(c, a, b, s, op.dest(), op.avg_mode(), op.a_signed(), op.b_signed(), op.hilo());
}

pub fn imul_r(c: &mut Ctx) {
    let op = InstImulR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let z = c.const_i(0);
    emit_imad(c, a, b, z, op.dest(), AvgMode::NO_NEG, op.a_signed(), op.b_signed(), op.hilo());
}

pub fn imul_i(c: &mut Ctx) {
    let op = InstImulI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    let z = c.const_i(0);
    emit_imad(c, a, b, z, op.dest(), AvgMode::NO_NEG, op.a_signed(), op.b_signed(), op.hilo());
}

pub fn imul_c(c: &mut Ctx) {
    let op = InstImulC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let z = c.const_i(0);
    emit_imad(c, a, b, z, op.dest(), AvgMode::NO_NEG, op.a_signed(), op.b_signed(), op.hilo());
}

pub fn imul32i(c: &mut Ctx) {
    let op = InstImul32i(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, op.imm32(), false);
    let z = c.const_i(0);
    emit_imad(c, a, b, z, op.dest(), AvgMode::NO_NEG, op.a_signed(), op.b_signed(), op.hilo());
}

pub fn iscadd_r(c: &mut Ctx) {
    let op = InstIscaddR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    emit_iscadd(c, a, b, op.dest(), op.imm5(), op.avg_mode(), op.write_cc());
}

pub fn iscadd_i(c: &mut Ctx) {
    let op = InstIscaddI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_iscadd(c, a, b, op.dest(), op.imm5(), op.avg_mode(), op.write_cc());
}

pub fn iscadd_c(c: &mut Ctx) {
    let op = InstIscaddC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_iscadd(c, a, b, op.dest(), op.imm5(), op.avg_mode(), op.write_cc());
}

pub fn iscadd32i(c: &mut Ctx) {
    let op = InstIscadd32i(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, op.imm32(), false);
    emit_iscadd(c, a, b, op.dest(), op.imm5(), AvgMode::NO_NEG, op.write_cc());
}

pub fn lea_r(c: &mut Ctx) {
    let op = InstLeaR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    emit_lea(c, a, b, op.dest(), op.neg_a(), op.imm_u5());
}

pub fn lea_i(c: &mut Ctx) {
    let op = InstLeaI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_lea(c, a, b, op.dest(), op.neg_a(), op.imm_u5());
}

pub fn lea_c(c: &mut Ctx) {
    let op = InstLeaC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_lea(c, a, b, op.dest(), op.neg_a(), op.imm_u5());
}

pub fn lea_hi_r(c: &mut Ctx) {
    let op = InstLeaHiR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_lea_hi(c, a, b, s, op.dest(), op.neg_a(), op.imm_u5());
}

pub fn lea_hi_c(c: &mut Ctx) {
    let op = InstLeaHiC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_lea_hi(c, a, b, s, op.dest(), op.neg_a(), op.imm_u5());
}

pub fn xmad_r(c: &mut Ctx) {
    let op = InstXmadR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_xmad(c, op.xmad_cop(), a, b, s, op.dest(), op.a_signed(), op.b_signed(), op.hilo_a(), op.hilo_b(), op.psl(), op.mrg(), op.x(), op.write_cc());
}

pub fn xmad_i(c: &mut Ctx) {
    let op = InstXmadI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, op.imm16(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_xmad(c, op.xmad_cop(), a, b, s, op.dest(), op.a_signed(), op.b_signed(), op.hilo_a(), false, op.psl(), op.mrg(), op.x(), op.write_cc());
}

pub fn xmad_c(c: &mut Ctx) {
    let op = InstXmadC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_xmad2(c, op.xmad_cop(), a, b, s, op.dest(), op.a_signed(), op.b_signed(), op.hilo_a(), op.hilo_b(), op.psl(), op.mrg(), op.x(), op.write_cc());
}

pub fn xmad_rc(c: &mut Ctx) {
    let op = InstXmadRc(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_c(), false);
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_xmad2(c, op.xmad_cop(), a, b, s, op.dest(), op.a_signed(), op.b_signed(), op.hilo_a(), op.hilo_b(), false, false, op.x(), op.write_cc());
}

fn emit_iadd(c: &mut Ctx, a: OperandId, b: OperandId, rd: i32, avg_mode: AvgMode, extended: bool, write_cc: bool) {
    let a = c.inegate_if(a, avg_mode == AvgMode::NEG_A);
    let b = c.inegate_if(b, avg_mode == AvgMode::NEG_B);
    let mut res = c.iadd(a, b);
    if extended {
        let cf = get_cf(c);
        let one = c.const_i(1);
        let carry = c.bitwise_and(cf, one);
        res = c.iadd(res, carry);
    }
    set_iadd_flags(c, res, a, b, write_cc, extended);
    // TODO in the original: SAT.
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

#[allow(clippy::too_many_arguments)]
fn emit_iadd3(
    c: &mut Ctx,
    mode: Lrs,
    a: OperandId,
    b: OperandId,
    s: OperandId,
    part_a: HalfSelect,
    part_b: HalfSelect,
    part_c: HalfSelect,
    rd: i32,
    neg_a: bool,
    neg_b: bool,
    neg_c: bool,
) {
    fn ext(c: &mut Ctx, src: OperandId, part: HalfSelect) -> OperandId {
        if part == HalfSelect::B32 {
            return src;
        }
        if part == HalfSelect::H0 {
            let m = c.const_i(0xffff);
            return c.bitwise_and(src, m);
        } else if part == HalfSelect::H1 {
            let s16 = c.const_i(16);
            return c.shift_right_u32(src, s16);
        }
        c.log(&format!("Iadd3 has invalid component selection {}.", part.0));
        src
    }
    let ea = ext(c, a, part_a);
    let a = c.inegate_if(ea, neg_a);
    let eb = ext(c, b, part_b);
    let b = c.inegate_if(eb, neg_b);
    let es = ext(c, s, part_c);
    let s = c.inegate_if(es, neg_c);
    let mut res = c.iadd(a, b);
    if mode != Lrs::NONE {
        if mode == Lrs::LEFT_SHIFT {
            let s16 = c.const_i(16);
            res = c.shift_left(res, s16);
        } else if mode == Lrs::RIGHT_SHIFT {
            let s16 = c.const_i(16);
            res = c.shift_right_u32(res, s16);
        }
    }
    let res = c.iadd(res, s);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

#[allow(clippy::too_many_arguments)]
fn emit_imad(c: &mut Ctx, a: OperandId, b: OperandId, s: OperandId, rd: i32, avg_mode: AvgMode, signed_a: bool, signed_b: bool, high: bool) {
    let b = c.inegate_if(b, avg_mode == AvgMode::NEG_A);
    let s = c.inegate_if(s, avg_mode == AvgMode::NEG_B);
    let mut res;
    if high {
        if signed_a && signed_b {
            res = c.multiply_high_s32(a, b);
        } else {
            res = c.multiply_high_u32(a, b);
            if signed_a {
                let k31 = c.const_i(31);
                let sh = c.shift_right_s32(a, k31);
                let m = c.imultiply(b, sh);
                res = c.iadd(res, m);
            } else if signed_b {
                let k31 = c.const_i(31);
                let sh = c.shift_right_s32(b, k31);
                let m = c.imultiply(a, sh);
                res = c.iadd(res, m);
            }
        }
    } else {
        res = c.imultiply(a, b);
    }
    if c.ir.ty(s) != OperandType::Constant || c.ir.value(s) != 0 {
        res = c.iadd(res, s);
    }
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

fn emit_iscadd(c: &mut Ctx, a: OperandId, b: OperandId, rd: i32, shift: i32, avg_mode: AvgMode, write_cc: bool) {
    let k = c.const_i(shift);
    let a = c.shift_left(a, k);
    let a = c.inegate_if(a, avg_mode == AvgMode::NEG_A);
    let b = c.inegate_if(b, avg_mode == AvgMode::NEG_B);
    let res = c.iadd(a, b);
    set_iadd_flags(c, res, a, b, write_cc, false);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

pub fn emit_lea(c: &mut Ctx, a: OperandId, b: OperandId, rd: i32, negate_a: bool, shift: i32) {
    let k = c.const_i(shift);
    let a = c.shift_left(a, k);
    let a = c.inegate_if(a, negate_a);
    let res = c.iadd(a, b);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

fn emit_lea_hi(c: &mut Ctx, a: OperandId, b: OperandId, s: OperandId, rd: i32, negate_a: bool, shift: i32) {
    let k = c.const_i(shift);
    let mut a_low = c.shift_left(a, k);
    let mut a_high = if shift == 0 {
        c.const_i(0)
    } else {
        let k = c.const_i(32 - shift);
        c.shift_right_u32(a, k)
    };
    let k = c.const_i(shift);
    let cs = c.shift_left(s, k);
    a_high = c.bitwise_or(a_high, cs);
    if negate_a {
        // 64-bit negation: not, then add 1 carrying from low to high.
        a_low = c.bitwise_not(a_low);
        a_high = c.bitwise_not(a_high);
        let one = c.const_i(1);
        let (_, carry) = add_with_carry(c, a_low, one);
        a_high = c.iadd(a_high, carry);
    }
    let _ = a_low;
    let res = c.iadd(a_high, b);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

#[allow(clippy::too_many_arguments)]
pub fn emit_xmad2(
    c: &mut Ctx,
    mode: XmadCop2,
    a: OperandId,
    b: OperandId,
    s: OperandId,
    rd: i32,
    signed_a: bool,
    signed_b: bool,
    high_a: bool,
    high_b: bool,
    psl: bool,
    merge: bool,
    extended: bool,
    write_cc: bool,
) {
    let conv = match mode {
        XmadCop2::CFULL => XmadCop::CFULL,
        XmadCop2::CLO => XmadCop::CLO,
        XmadCop2::CHI => XmadCop::CHI,
        XmadCop2::CSFU => XmadCop::CSFU,
        m => {
            c.log(&format!("Invalid XMAD mode \"{}\".", m.0));
            return;
        }
    };
    emit_xmad(c, conv, a, b, s, rd, signed_a, signed_b, high_a, high_b, psl, merge, extended, write_cc);
}

#[allow(clippy::too_many_arguments)]
pub fn emit_xmad(
    c: &mut Ctx,
    mode: XmadCop,
    a: OperandId,
    b: OperandId,
    mut s: OperandId,
    rd: i32,
    signed_a: bool,
    signed_b: bool,
    high_a: bool,
    high_b: bool,
    product_shift_left: bool,
    merge: bool,
    extended: bool,
    write_cc: bool,
) {
    let b_unmodified = b;
    fn ext16(c: &mut Ctx, src: OperandId, high: bool, signed: bool) -> OperandId {
        if signed && high {
            let k = c.const_i(16);
            c.shift_right_s32(src, k)
        } else if signed {
            let (z, k) = (c.const_i(0), c.const_i(16));
            c.bitfield_extract_s32(src, z, k)
        } else if high {
            let k = c.const_i(16);
            c.shift_right_u32(src, k)
        } else {
            let m = c.const_i(0xffff);
            c.bitwise_and(src, m)
        }
    }
    let a = ext16(c, a, high_a, signed_a);
    let b = ext16(c, b, high_b, signed_b);
    let mut res = c.imultiply(a, b);
    if product_shift_left {
        let k = c.const_i(16);
        res = c.shift_left(res, k);
    }
    match mode {
        XmadCop::CFULL => {}
        XmadCop::CLO => s = ext16(c, s, false, false),
        XmadCop::CHI => s = ext16(c, s, true, false),
        XmadCop::CBCC => {
            let k = c.const_i(16);
            let sh = c.shift_left(b_unmodified, k);
            s = c.iadd(s, sh);
        }
        XmadCop::CSFU => {
            let k31 = c.const_i(31);
            let ua = c.shift_right_u32(a, k31);
            let k16 = c.const_i(16);
            let adj_a = c.shift_left(ua, k16);
            let k31 = c.const_i(31);
            let ub = c.shift_right_u32(b, k31);
            let k16 = c.const_i(16);
            let adj_b = c.shift_left(ub, k16);
            let sum = c.iadd(adj_a, adj_b);
            s = c.isubtract(s, sum);
        }
        m => {
            c.log(&format!("Invalid XMAD mode \"{}\".", m.0));
            return;
        }
    }
    let product = res;
    if extended {
        // Add with carry.
        let cf = get_cf(c);
        let one = c.const_i(1);
        let carry = c.bitwise_and(cf, one);
        res = c.iadd(res, carry);
    } else {
        res = c.iadd(res, s);
    }
    set_iadd_flags(c, res, product, s, write_cc, extended);
    if merge {
        let m = c.const_i(0xffff);
        res = c.bitwise_and(res, m);
        let k = c.const_i(16);
        let sh = c.shift_left(b_unmodified, k);
        res = c.bitwise_or(res, sh);
    }
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

fn set_iadd_flags(c: &mut Ctx, res: OperandId, a: OperandId, b: OperandId, set_cc: bool, extended: bool) {
    if !set_cc {
        return;
    }
    if extended {
        // C = (d == a && CIn) || d < a
        let mut t0 = c.icompare_equal(res, a);
        let t1 = c.icompare_less_unsigned(res, a);
        let cf = get_cf(c);
        t0 = c.bitwise_and(t0, cf);
        let cf = get_cf(c);
        let or = c.bitwise_or(t0, t1);
        c.copy_to(cf, or);
    } else {
        // C = d < a
        let cf = get_cf(c);
        let lt = c.icompare_less_unsigned(res, a);
        c.copy_to(cf, lt);
    }
    // V = (d ^ a) & ~(a ^ b) < 0
    let v0 = c.bitwise_exclusive_or(res, a);
    let v1 = c.bitwise_exclusive_or(a, b);
    let v1 = c.bitwise_not(v1);
    let v = c.bitwise_and(v0, v1);
    let vf = get_vf(c);
    let zero = c.const_i(0);
    let lt = c.icompare_less(v, zero);
    c.copy_to(vf, lt);
    set_zn_flags(c, res, true, extended);
}

// InstEmitIntegerLogical.

pub fn lop_r(c: &mut Ctx) {
    let op = InstLopR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    emit_lop(c, op.lop(), op.predicate_op(), a, b, op.dest(), op.dest_pred(), op.neg_a(), op.neg_b(), op.x(), op.write_cc());
}

pub fn lop_i(c: &mut Ctx) {
    let op = InstLopI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_lop(c, op.logic_op(), op.predicate_op(), a, b, op.dest(), op.dest_pred(), op.neg_a(), op.neg_b(), op.x(), op.write_cc());
}

pub fn lop_c(c: &mut Ctx) {
    let op = InstLopC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_lop(c, op.logic_op(), op.predicate_op(), a, b, op.dest(), op.dest_pred(), op.neg_a(), op.neg_b(), op.x(), op.write_cc());
}

pub fn lop32i(c: &mut Ctx) {
    let op = InstLop32i(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, op.imm32(), false);
    emit_lop(c, op.logic_op(), PredicateOp::F, a, b, op.dest(), PT, op.neg_a(), op.neg_b(), op.x(), op.write_cc());
}

pub fn lop3_r(c: &mut Ctx) {
    let op = InstLop3R(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_lop3(c, op.imm(), op.predicate_op(), a, b, s, op.dest(), op.dest_pred(), op.x(), op.write_cc());
}

pub fn lop3_i(c: &mut Ctx) {
    let op = InstLop3I(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_lop3(c, op.imm(), PredicateOp::F, a, b, s, op.dest(), PT, false, op.write_cc());
}

pub fn lop3_c(c: &mut Ctx) {
    let op = InstLop3C(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_lop3(c, op.imm(), PredicateOp::F, a, b, s, op.dest(), PT, false, op.write_cc());
}

#[allow(clippy::too_many_arguments)]
fn emit_lop(
    c: &mut Ctx,
    logic_op: LogicOp,
    pred_op: PredicateOp,
    a: OperandId,
    b: OperandId,
    rd: i32,
    dest_pred: i32,
    invert_a: bool,
    invert_b: bool,
    extended: bool,
    write_cc: bool,
) {
    let a = c.bitwise_not_if(a, invert_a);
    let b = c.bitwise_not_if(b, invert_b);
    let res = match logic_op {
        LogicOp::AND => c.bitwise_and(a, b),
        LogicOp::OR => c.bitwise_or(a, b),
        LogicOp::XOR => c.bitwise_exclusive_or(a, b),
        _ => b,
    };
    emit_lop_pred_write(c, res, pred_op, dest_pred);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
    set_zn_flags(c, res, write_cc, extended);
}

#[allow(clippy::too_many_arguments)]
fn emit_lop3(c: &mut Ctx, truth_table: i32, pred_op: PredicateOp, a: OperandId, b: OperandId, s: OperandId, rd: i32, dest_pred: i32, extended: bool, write_cc: bool) {
    let Some(res) = lop3_from_truth_table(c, a, b, s, truth_table) else {
        c.fail("Lop3 truth table without an expression");
        return;
    };
    emit_lop_pred_write(c, res, pred_op, dest_pred);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
    set_zn_flags(c, res, write_cc, extended);
}

fn emit_lop_pred_write(c: &mut Ctx, result: OperandId, pred_op: PredicateOp, pred: i32) {
    if pred != PT {
        let p_res = if pred_op == PredicateOp::F {
            c.const_i(ir_consts::FALSE)
        } else if pred_op == PredicateOp::T {
            c.const_i(ir_consts::TRUE)
        } else if pred_op == PredicateOp::Z {
            let z = c.const_i(0);
            c.icompare_equal(result, z)
        } else {
            let z = c.const_i(0);
            c.icompare_not_equal(result, z)
        };
        let p = c.pred(pred);
        c.copy_to(p, p_res);
    }
}

fn permute_table(imm: i32, bits: [i32; 8]) -> i32 {
    // bits[0] is where bit 7 goes, ... bits[7] where bit 0 goes.
    let mut r = 0;
    for (i, &to) in bits.iter().rev().enumerate() {
        r |= ((imm >> i) & 1) << to;
    }
    r
}

/// An expression of a, b and c with the given 3-input truth table, trying operand permutations and inversions until
/// a known form matches. The inversions it tries stay in the code even when they did not match.
fn lop3_from_truth_table(c: &mut Ctx, a: OperandId, b: OperandId, s: OperandId, imm: i32) -> Option<OperandId> {
    for i in 0..0x40 {
        let mut curr = imm;
        let (mut x, mut y, mut z) = (a, b, s);
        if i & 0x01 != 0 {
            std::mem::swap(&mut x, &mut y);
            curr = permute_table(curr, [7, 6, 3, 2, 5, 4, 1, 0]);
        }
        if i & 0x02 != 0 {
            std::mem::swap(&mut x, &mut z);
            curr = permute_table(curr, [7, 3, 5, 1, 6, 2, 4, 0]);
        }
        if i & 0x04 != 0 {
            std::mem::swap(&mut y, &mut z);
            curr = permute_table(curr, [7, 5, 6, 4, 3, 1, 2, 0]);
        }
        if i & 0x08 != 0 {
            x = c.bitwise_not(x);
            curr = permute_table(curr, [3, 2, 1, 0, 7, 6, 5, 4]);
        }
        if i & 0x10 != 0 {
            y = c.bitwise_not(y);
            curr = permute_table(curr, [5, 4, 7, 6, 1, 0, 3, 2]);
        }
        if i & 0x20 != 0 {
            z = c.bitwise_not(z);
            curr = permute_table(curr, [6, 7, 4, 5, 2, 3, 0, 1]);
        }
        if let Some(r) = lop3_expr(curr, c, x, y, z) {
            return Some(r);
        }
        if let Some(r) = lop3_expr(!curr & 0xff, c, x, y, z) {
            return Some(c.bitwise_not(r));
        }
    }
    None
}

fn lop3_expr(imm: i32, c: &mut Ctx, x: OperandId, y: OperandId, z: OperandId) -> Option<OperandId> {
    Some(match imm {
        0x00 => c.const_i(0),
        0xff => c.const_i(-1),
        0xf0 => x,
        0xc0 => c.bitwise_and(x, y),
        0xfc => c.bitwise_or(x, y),
        0x3c => c.bitwise_exclusive_or(x, y),
        0x80 => {
            let t = c.bitwise_and(y, z);
            c.bitwise_and(x, t)
        }
        0xfe => {
            let t = c.bitwise_or(y, z);
            c.bitwise_or(x, t)
        }
        0x60 => {
            let t = c.bitwise_exclusive_or(y, z);
            c.bitwise_and(x, t)
        }
        0xf6 => {
            let t = c.bitwise_exclusive_or(y, z);
            c.bitwise_or(x, t)
        }
        0xe0 => {
            let t = c.bitwise_or(y, z);
            c.bitwise_and(x, t)
        }
        0xf8 => {
            let t = c.bitwise_and(y, z);
            c.bitwise_or(x, t)
        }
        0x16 => {
            let t0 = c.bitwise_or(x, y);
            let t1 = c.bitwise_and(x, y);
            let t2 = c.bitwise_or(z, t1);
            c.bitwise_exclusive_or(t0, t2)
        }
        0xe8 => {
            let t0 = c.bitwise_or(x, y);
            let t1 = c.bitwise_and(x, y);
            let t2 = c.bitwise_or(z, t1);
            c.bitwise_and(t0, t2)
        }
        0x7e => {
            let t0 = c.bitwise_exclusive_or(x, y);
            let t1 = c.bitwise_exclusive_or(x, z);
            c.bitwise_or(t0, t1)
        }
        0x1a => {
            let t0 = c.bitwise_exclusive_or(x, z);
            let ny = c.bitwise_not(y);
            let t1 = c.bitwise_or(ny, z);
            c.bitwise_and(t0, t1)
        }
        0xca => {
            let t0 = c.bitwise_and(x, y);
            let nx = c.bitwise_not(x);
            let t1 = c.bitwise_and(nx, z);
            c.bitwise_or(t0, t1)
        }
        0x78 => {
            let t = c.bitwise_and(y, z);
            c.bitwise_exclusive_or(x, t)
        }
        0x1e => {
            let t = c.bitwise_or(y, z);
            c.bitwise_exclusive_or(x, t)
        }
        0x96 => {
            let t = c.bitwise_exclusive_or(y, z);
            c.bitwise_exclusive_or(x, t)
        }
        _ => return None,
    })
}
