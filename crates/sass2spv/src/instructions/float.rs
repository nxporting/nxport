// Float arithmetic, half-float pairs, min/max and the multifunction unit (InstEmitFloatArithmetic,
// InstEmitFloatMinMax, InstEmitMultifunction).

use super::helper::*;
use crate::decoders::inst_decoders::*;
use crate::ir::{Instruction as I, OperandId};
use crate::tri;
use crate::translation::emitter::{Emit, EmitterContext as Ctx};

pub fn dadd_r(c: &mut Ctx) {
    let op = InstDaddR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_reg(c, op.src_b(), true);
    emit_fadd(c, I::FP64, a, b, op.dest(), op.neg_a(), op.neg_b(), op.abs_a(), op.abs_b(), false, op.write_cc());
}

pub fn dadd_i(c: &mut Ctx) {
    let op = InstDaddI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_imm(c, imm20_to_float(op.imm20()), true);
    emit_fadd(c, I::FP64, a, b, op.dest(), op.neg_a(), op.neg_b(), op.abs_a(), op.abs_b(), false, op.write_cc());
}

pub fn dadd_c(c: &mut Ctx) {
    let op = InstDaddC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), true);
    emit_fadd(c, I::FP64, a, b, op.dest(), op.neg_a(), op.neg_b(), op.abs_a(), op.abs_b(), false, op.write_cc());
}

pub fn dfma_r(c: &mut Ctx) {
    let op = InstDfmaR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_reg(c, op.src_b(), true);
    let s = get_src_reg(c, op.src_c(), true);
    emit_ffma(c, I::FP64, a, b, s, op.dest(), op.neg_a(), op.neg_c(), false, op.write_cc());
}

pub fn dfma_i(c: &mut Ctx) {
    let op = InstDfmaI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_imm(c, imm20_to_float(op.imm20()), true);
    let s = get_src_reg(c, op.src_c(), true);
    emit_ffma(c, I::FP64, a, b, s, op.dest(), op.neg_a(), op.neg_c(), false, op.write_cc());
}

pub fn dfma_c(c: &mut Ctx) {
    let op = InstDfmaC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), true);
    let s = get_src_reg(c, op.src_c(), true);
    emit_ffma(c, I::FP64, a, b, s, op.dest(), op.neg_a(), op.neg_c(), false, op.write_cc());
}

pub fn dfma_rc(c: &mut Ctx) {
    let op = InstDfmaRc(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_reg(c, op.src_c(), true);
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), true);
    emit_ffma(c, I::FP64, a, b, s, op.dest(), op.neg_a(), op.neg_c(), false, op.write_cc());
}

pub fn dmul_r(c: &mut Ctx) {
    let op = InstDmulR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_reg(c, op.src_b(), true);
    emit_fmul(c, I::FP64, MultiplyScale::NO_SCALE, a, b, op.dest(), op.neg_a(), false, op.write_cc());
}

pub fn dmul_i(c: &mut Ctx) {
    let op = InstDmulI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_imm(c, imm20_to_float(op.imm20()), true);
    emit_fmul(c, I::FP64, MultiplyScale::NO_SCALE, a, b, op.dest(), op.neg_a(), false, op.write_cc());
}

pub fn dmul_c(c: &mut Ctx) {
    let op = InstDmulC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), true);
    emit_fmul(c, I::FP64, MultiplyScale::NO_SCALE, a, b, op.dest(), op.neg_a(), false, op.write_cc());
}

pub fn fadd_r(c: &mut Ctx) {
    let op = InstFaddR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    emit_fadd(c, I::FP32, a, b, op.dest(), op.neg_a(), op.neg_b(), op.abs_a(), op.abs_b(), op.sat(), op.write_cc());
}

pub fn fadd_i(c: &mut Ctx) {
    let op = InstFaddI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_float(op.imm20()), false);
    emit_fadd(c, I::FP32, a, b, op.dest(), op.neg_a(), op.neg_b(), op.abs_a(), op.abs_b(), op.sat(), op.write_cc());
}

pub fn fadd_c(c: &mut Ctx) {
    let op = InstFaddC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_fadd(c, I::FP32, a, b, op.dest(), op.neg_a(), op.neg_b(), op.abs_a(), op.abs_b(), op.sat(), op.write_cc());
}

pub fn fadd32i(c: &mut Ctx) {
    let op = InstFadd32i(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, op.imm32(), false);
    emit_fadd(c, I::FP32, a, b, op.dest(), op.neg_a(), op.neg_b(), op.abs_a(), op.abs_b(), false, op.write_cc());
}

pub fn ffma_r(c: &mut Ctx) {
    let op = InstFfmaR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_ffma(c, I::FP32, a, b, s, op.dest(), op.neg_a(), op.neg_c(), op.sat(), op.write_cc());
}

pub fn ffma_i(c: &mut Ctx) {
    let op = InstFfmaI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_float(op.imm20()), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_ffma(c, I::FP32, a, b, s, op.dest(), op.neg_a(), op.neg_c(), op.sat(), op.write_cc());
}

pub fn ffma_c(c: &mut Ctx) {
    let op = InstFfmaC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_ffma(c, I::FP32, a, b, s, op.dest(), op.neg_a(), op.neg_c(), op.sat(), op.write_cc());
}

pub fn ffma_rc(c: &mut Ctx) {
    let op = InstFfmaRc(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_c(), false);
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_ffma(c, I::FP32, a, b, s, op.dest(), op.neg_a(), op.neg_c(), op.sat(), op.write_cc());
}

pub fn ffma32i(c: &mut Ctx) {
    let op = InstFfma32i(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, op.imm32(), false);
    let s = get_src_reg(c, op.dest(), false);
    emit_ffma(c, I::FP32, a, b, s, op.dest(), op.neg_a(), op.neg_c(), op.sat(), op.write_cc());
}

pub fn fmul_r(c: &mut Ctx) {
    let op = InstFmulR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    emit_fmul(c, I::FP32, op.scale(), a, b, op.dest(), op.neg_a(), op.sat(), op.write_cc());
}

pub fn fmul_i(c: &mut Ctx) {
    let op = InstFmulI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_float(op.imm20()), false);
    emit_fmul(c, I::FP32, op.scale(), a, b, op.dest(), op.neg_a(), op.sat(), op.write_cc());
}

pub fn fmul_c(c: &mut Ctx) {
    let op = InstFmulC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_fmul(c, I::FP32, op.scale(), a, b, op.dest(), op.neg_a(), op.sat(), op.write_cc());
}

pub fn fmul32i(c: &mut Ctx) {
    let op = InstFmul32i(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, op.imm32(), false);
    emit_fmul(c, I::FP32, MultiplyScale::NO_SCALE, a, b, op.dest(), false, op.sat(), op.write_cc());
}

pub fn hadd2_r(c: &mut Ctx) {
    let op = InstHadd2R(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = tri!(c, get_half_src_reg(c, op.b_swizzle(), op.src_b(), op.neg_b(), op.abs_b()));
    emit_hadd2_hmul2(c, op.o_fmt(), a, b, true, op.dest(), op.sat());
}

pub fn hadd2_i(c: &mut Ctx) {
    let op = InstHadd2I(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = get_half_src_imm2(c, op.bimm_h0(), op.bimm_h1());
    emit_hadd2_hmul2(c, op.o_fmt(), a, b, true, op.dest(), op.sat());
}

pub fn hadd2_c(c: &mut Ctx) {
    let op = InstHadd2C(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = tri!(c, get_half_src_cbuf(c, HalfSwizzle::F32, op.cbuf_slot(), op.cbuf_offset(), op.neg_b(), op.abs_b()));
    emit_hadd2_hmul2(c, op.o_fmt(), a, b, true, op.dest(), op.sat());
}

pub fn hadd232i(c: &mut Ctx) {
    let op = InstHadd232i(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), false));
    let b = get_half_src_imm32(c, op.imm());
    emit_hadd2_hmul2(c, OFmt::F16, a, b, true, op.dest(), op.sat());
}

pub fn hfma2_r(c: &mut Ctx) {
    let op = InstHfma2R(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), false, false));
    let b = tri!(c, get_half_src_reg(c, op.b_swizzle(), op.src_b(), op.neg_a(), false));
    let s = tri!(c, get_half_src_reg(c, op.c_swizzle(), op.src_c(), op.neg_c(), false));
    emit_hfma2(c, op.o_fmt(), a, b, s, op.dest(), op.sat());
}

pub fn hfma2_i(c: &mut Ctx) {
    let op = InstHfma2I(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), false, false));
    let b = get_half_src_imm2(c, op.bimm_h0(), op.bimm_h1());
    let s = tri!(c, get_half_src_reg(c, op.c_swizzle(), op.src_c(), op.neg_c(), false));
    emit_hfma2(c, op.o_fmt(), a, b, s, op.dest(), op.sat());
}

pub fn hfma2_c(c: &mut Ctx) {
    let op = InstHfma2C(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), false, false));
    let b = tri!(c, get_half_src_cbuf(c, HalfSwizzle::F32, op.cbuf_slot(), op.cbuf_offset(), op.neg_a(), false));
    let s = tri!(c, get_half_src_reg(c, op.c_swizzle(), op.src_c(), op.neg_c(), false));
    emit_hfma2(c, op.o_fmt(), a, b, s, op.dest(), op.sat());
}

pub fn hfma2_rc(c: &mut Ctx) {
    let op = InstHfma2Rc(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), false, false));
    let b = tri!(c, get_half_src_reg(c, op.c_swizzle(), op.src_c(), op.neg_a(), false));
    let s = tri!(c, get_half_src_cbuf(c, HalfSwizzle::F32, op.cbuf_slot(), op.cbuf_offset(), op.neg_c(), false));
    emit_hfma2(c, op.o_fmt(), a, b, s, op.dest(), op.sat());
}

pub fn hfma232i(c: &mut Ctx) {
    let op = InstHfma232i(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), false, false));
    let b = get_half_src_imm32(c, op.imm());
    let s = tri!(c, get_half_src_reg(c, HalfSwizzle::F16, op.dest(), op.neg_c(), false));
    emit_hfma2(c, OFmt::F16, a, b, s, op.dest(), false);
}

pub fn hmul2_r(c: &mut Ctx) {
    let op = InstHmul2R(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), false, op.abs_a()));
    let b = tri!(c, get_half_src_reg(c, op.b_swizzle(), op.src_b(), op.neg_a(), op.abs_b()));
    emit_hadd2_hmul2(c, op.o_fmt(), a, b, false, op.dest(), op.sat());
}

pub fn hmul2_i(c: &mut Ctx) {
    let op = InstHmul2I(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), op.neg_a(), op.abs_a()));
    let b = get_half_src_imm2(c, op.bimm_h0(), op.bimm_h1());
    emit_hadd2_hmul2(c, op.o_fmt(), a, b, false, op.dest(), op.sat());
}

pub fn hmul2_c(c: &mut Ctx) {
    let op = InstHmul2C(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), false, op.abs_a()));
    let b = tri!(c, get_half_src_cbuf(c, HalfSwizzle::F32, op.cbuf_slot(), op.cbuf_offset(), op.neg_a(), op.abs_b()));
    emit_hadd2_hmul2(c, op.o_fmt(), a, b, false, op.dest(), op.sat());
}

pub fn hmul232i(c: &mut Ctx) {
    let op = InstHmul232i(c.raw_op());
    let a = tri!(c, get_half_src_reg(c, op.a_swizzle(), op.src_a(), false, false));
    let b = get_half_src_imm32(c, op.imm32());
    emit_hadd2_hmul2(c, OFmt::F16, a, b, false, op.dest(), op.sat());
}

#[allow(clippy::too_many_arguments)]
fn emit_fadd(c: &mut Ctx, fp: I, a: OperandId, b: OperandId, rd: i32, neg_a: bool, neg_b: bool, abs_a: bool, abs_b: bool, sat: bool, write_cc: bool) {
    let a = c.fp_abs_neg(a, abs_a, neg_a, fp);
    let b = c.fp_abs_neg(b, abs_b, neg_b, fp);
    let sum = c.fp_add(a, b, fp);
    let res = c.fp_saturate_if(sum, sat, fp);
    set_dest(c, res, rd, fp == I::FP64);
    set_fp_zn_flags(c, res, write_cc, fp);
}

#[allow(clippy::too_many_arguments)]
fn emit_ffma(c: &mut Ctx, fp: I, a: OperandId, b: OperandId, s: OperandId, rd: i32, neg_b: bool, neg_c: bool, sat: bool, write_cc: bool) {
    let b = c.fp_negate_if(b, neg_b, fp);
    let s = c.fp_negate_if(s, neg_c, fp);
    let fma = c.fp_fused_multiply_add(a, b, s, fp);
    let res = c.fp_saturate_if(fma, sat, fp);
    set_dest(c, res, rd, fp == I::FP64);
    set_fp_zn_flags(c, res, write_cc, fp);
}

#[allow(clippy::too_many_arguments)]
fn emit_fmul(c: &mut Ctx, fp: I, scale: MultiplyScale, mut a: OperandId, b: OperandId, rd: i32, neg_b: bool, sat: bool, write_cc: bool) {
    let fp64 = fp == I::FP64;
    let b = c.fp_negate_if(b, neg_b, fp);
    if scale != MultiplyScale::NO_SCALE {
        let factor = match scale {
            MultiplyScale::D2 => 0.5,
            MultiplyScale::D4 => 0.25,
            MultiplyScale::D8 => 0.125,
            MultiplyScale::M2 => 2.0,
            MultiplyScale::M4 => 4.0,
            MultiplyScale::M8 => 8.0,
            // Invalid: as if there were no scale.
            _ => 1.0,
        };
        let mut scale_const = c.const_f(factor);
        if factor == 1.0 {
            c.log(&format!("Invalid FP multiply scale \"{}\".", scale.0));
        }
        if fp64 {
            scale_const = c.fp32_convert_to_fp64(scale_const);
        }
        a = c.fp_multiply(a, scale_const, fp);
    }
    let m = c.fp_multiply(a, b, fp);
    let res = c.fp_saturate_if(m, sat, fp);
    set_dest(c, res, rd, fp64);
    set_fp_zn_flags(c, res, write_cc, fp);
}

fn emit_hadd2_hmul2(c: &mut Ctx, swizzle: OFmt, a: [OperandId; 2], b: [OperandId; 2], is_add: bool, rd: i32, sat: bool) {
    let mut res = [a[0]; 2];
    for i in 0..2 {
        res[i] = if is_add { c.fp_add(a[i], b[i], I::FP32) } else { c.fp_multiply(a[i], b[i], I::FP32) };
        res[i] = c.fp_saturate_if(res[i], sat, I::FP32);
    }
    let d = get_dest(c, rd);
    let packed = tri!(c, get_half_packed(c, swizzle, res, rd));
    c.copy_to(d, packed);
}

pub fn emit_hfma2(c: &mut Ctx, swizzle: OFmt, a: [OperandId; 2], b: [OperandId; 2], s: [OperandId; 2], rd: i32, sat: bool) {
    let mut res = [a[0]; 2];
    for i in 0..2 {
        res[i] = c.fp_fused_multiply_add(a[i], b[i], s[i], I::FP32);
        res[i] = c.fp_saturate_if(res[i], sat, I::FP32);
    }
    let d = get_dest(c, rd);
    let packed = tri!(c, get_half_packed(c, swizzle, res, rd));
    c.copy_to(d, packed);
}

// InstEmitFloatMinMax.

pub fn dmnmx_r(c: &mut Ctx) {
    let op = InstDmnmxR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_reg(c, op.src_b(), true);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_fmnmx(c, a, b, p, op.dest(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), op.write_cc(), true);
}

pub fn dmnmx_i(c: &mut Ctx) {
    let op = InstDmnmxI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_imm(c, imm20_to_float(op.imm20()), true);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_fmnmx(c, a, b, p, op.dest(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), op.write_cc(), true);
}

pub fn dmnmx_c(c: &mut Ctx) {
    let op = InstDmnmxC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), true);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), true);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_fmnmx(c, a, b, p, op.dest(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), op.write_cc(), true);
}

pub fn fmnmx_r(c: &mut Ctx) {
    let op = InstFmnmxR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_fmnmx(c, a, b, p, op.dest(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), op.write_cc(), false);
}

pub fn fmnmx_i(c: &mut Ctx) {
    let op = InstFmnmxI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_float(op.imm20()), false);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_fmnmx(c, a, b, p, op.dest(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), op.write_cc(), false);
}

pub fn fmnmx_c(c: &mut Ctx) {
    let op = InstFmnmxC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_fmnmx(c, a, b, p, op.dest(), op.abs_a(), op.abs_b(), op.neg_a(), op.neg_b(), op.write_cc(), false);
}

#[allow(clippy::too_many_arguments)]
fn emit_fmnmx(c: &mut Ctx, a: OperandId, b: OperandId, pred: OperandId, rd: i32, abs_a: bool, abs_b: bool, neg_a: bool, neg_b: bool, write_cc: bool, fp64: bool) {
    let fp = if fp64 { I::FP64 } else { I::FP32 };
    let a = c.fp_abs_neg(a, abs_a, neg_a, fp);
    let b = c.fp_abs_neg(b, abs_b, neg_b, fp);
    let min = c.fp_minimum(a, b, fp);
    let max = c.fp_maximum(a, b, fp);
    let res = c.conditional_select(pred, min, max);
    set_dest(c, res, rd, fp64);
    set_fp_zn_flags(c, res, write_cc, fp);
}

// InstEmitMultifunction.

pub fn rro_r(c: &mut Ctx) {
    let op = InstRroR(c.raw_op());
    let b = get_src_reg(c, op.src_b(), false);
    emit_rro(c, b, op.dest(), op.abs_b(), op.neg_b());
}

pub fn rro_i(c: &mut Ctx) {
    let op = InstRroI(c.raw_op());
    let b = get_src_imm(c, imm20_to_float(op.imm20()), false);
    emit_rro(c, b, op.dest(), op.abs_b(), op.neg_b());
}

pub fn rro_c(c: &mut Ctx) {
    let op = InstRroC(c.raw_op());
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_rro(c, b, op.dest(), op.abs_b(), op.neg_b());
}

pub fn mufu(c: &mut Ctx) {
    let op = InstMufu(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let mut res = c.fp_abs_neg(a, op.abs_a(), op.neg_a(), I::FP32);
    match op.mufu_op() {
        MufuOp::COS => res = c.fp_cosine(res),
        MufuOp::SIN => res = c.fp_sine(res),
        MufuOp::EX2 => res = c.fp_exponent_b2(res),
        MufuOp::LG2 => res = c.fp_logarithm_b2(res),
        MufuOp::RCP => res = c.fp_reciprocal(res, I::FP32),
        MufuOp::RSQ => res = c.fp_reciprocal_square_root(res, I::FP32),
        MufuOp::RCP64H => {
            let z = c.const_i(0);
            res = c.pack_double_2x32(z, res);
            let r = c.fp_reciprocal(res, I::FP64);
            res = c.unpack_double_2x32_high(r);
        }
        MufuOp::RSQ64H => {
            let z = c.const_i(0);
            res = c.pack_double_2x32(z, res);
            let r = c.fp_reciprocal_square_root(res, I::FP64);
            res = c.unpack_double_2x32_high(r);
        }
        MufuOp::SQRT => res = c.fp_square_root(res),
        m => c.log(&format!("Invalid MUFU operation \"{}\".", m.0)),
    }
    let d = get_dest(c, op.dest());
    let r = c.fp_saturate_if(res, op.sat(), I::FP32);
    c.copy_to(d, r);
}

fn emit_rro(c: &mut Ctx, b: OperandId, rd: i32, abs_b: bool, neg_b: bool) {
    // Range reduction, translated as a move: a matching MUFU always follows.
    let b = c.fp_abs_neg(b, abs_b, neg_b, I::FP32);
    let d = get_dest(c, rd);
    c.copy_to(d, b);
}
