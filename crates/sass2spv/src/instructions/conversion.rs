// Conversions between float and integer formats (InstEmitConversion).

use super::helper::*;
use crate::decoders::inst_decoders::*;
use crate::ir::{Instruction as I, OperandId};
use crate::tri;
use crate::translation::emitter::{Emit, EmitterContext as Ctx};

fn fp_type(t: DstFmt) -> I {
    if t == DstFmt::F64 { I::FP64 } else { I::FP32 }
}

pub fn f2f_r(c: &mut Ctx) {
    let op = InstF2fR(c.raw_op());
    let src = tri!(c, unpack_reg(c, op.src_fmt(), op.sh(), op.src_b()));
    emit_f2f(c, op.src_fmt(), op.dst_fmt(), op.round_mode(), src, op.dest(), op.abs_b(), op.neg_b(), op.sat());
}

pub fn f2f_i(c: &mut Ctx) {
    let op = InstF2fI(c.raw_op());
    let src = tri!(c, unpack_imm(c, op.src_fmt(), op.sh(), imm20_to_float(op.imm20())));
    emit_f2f(c, op.src_fmt(), op.dst_fmt(), op.round_mode(), src, op.dest(), op.abs_b(), op.neg_b(), op.sat());
}

pub fn f2f_c(c: &mut Ctx) {
    let op = InstF2fC(c.raw_op());
    let src = tri!(c, unpack_cbuf(c, op.src_fmt(), op.sh(), op.cbuf_slot(), op.cbuf_offset()));
    emit_f2f(c, op.src_fmt(), op.dst_fmt(), op.round_mode(), src, op.dest(), op.abs_b(), op.neg_b(), op.sat());
}

pub fn f2i_r(c: &mut Ctx) {
    let op = InstF2iR(c.raw_op());
    let src = tri!(c, unpack_reg(c, op.src_fmt(), op.sh(), op.src_b()));
    tri!(c, emit_f2i(c, op.src_fmt(), op.i_dst_fmt(), op.round_mode(), src, op.dest(), op.abs_b(), op.neg_b()));
}

pub fn f2i_i(c: &mut Ctx) {
    let op = InstF2iI(c.raw_op());
    let src = tri!(c, unpack_imm(c, op.src_fmt(), op.sh(), imm20_to_float(op.imm20())));
    tri!(c, emit_f2i(c, op.src_fmt(), op.i_dst_fmt(), op.round_mode(), src, op.dest(), op.abs_b(), op.neg_b()));
}

pub fn f2i_c(c: &mut Ctx) {
    let op = InstF2iC(c.raw_op());
    let src = tri!(c, unpack_cbuf(c, op.src_fmt(), op.sh(), op.cbuf_slot(), op.cbuf_offset()));
    tri!(c, emit_f2i(c, op.src_fmt(), op.i_dst_fmt(), op.round_mode(), src, op.dest(), op.abs_b(), op.neg_b()));
}

pub fn i2f_r(c: &mut Ctx) {
    let op = InstI2fR(c.raw_op());
    let src = get_src_reg(c, op.src_b(), false);
    emit_i2f(c, op.i_src_fmt(), op.dst_fmt(), src, op.byte_sel(), op.dest(), op.abs_b(), op.neg_b());
}

pub fn i2f_i(c: &mut Ctx) {
    let op = InstI2fI(c.raw_op());
    let src = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_i2f(c, op.i_src_fmt(), op.dst_fmt(), src, op.byte_sel(), op.dest(), op.abs_b(), op.neg_b());
}

pub fn i2f_c(c: &mut Ctx) {
    let op = InstI2fC(c.raw_op());
    let src = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_i2f(c, op.i_src_fmt(), op.dst_fmt(), src, op.byte_sel(), op.dest(), op.abs_b(), op.neg_b());
}

pub fn i2i_r(c: &mut Ctx) {
    let op = InstI2iR(c.raw_op());
    let src = get_src_reg(c, op.src_b(), false);
    tri!(c, emit_i2i(c, op.i_src_fmt(), op.i_dst_fmt(), src, op.byte_sel(), op.dest(), op.abs_b(), op.neg_b(), op.sat(), op.write_cc()));
}

pub fn i2i_i(c: &mut Ctx) {
    let op = InstI2iI(c.raw_op());
    let src = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    tri!(c, emit_i2i(c, op.i_src_fmt(), op.i_dst_fmt(), src, op.byte_sel(), op.dest(), op.abs_b(), op.neg_b(), op.sat(), op.write_cc()));
}

pub fn i2i_c(c: &mut Ctx) {
    let op = InstI2iC(c.raw_op());
    let src = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    tri!(c, emit_i2i(c, op.i_src_fmt(), op.i_dst_fmt(), src, op.byte_sel(), op.dest(), op.abs_b(), op.neg_b(), op.sat(), op.write_cc()));
}

#[allow(clippy::too_many_arguments)]
fn emit_f2f(c: &mut Ctx, src_type: DstFmt, dst_type: DstFmt, rounding: IntegerRound, src: OperandId, rd: i32, abs: bool, neg: bool, sat: bool) {
    let st = fp_type(src_type);
    let mut b = c.fp_abs_neg(src, abs, neg, st);
    if src_type == dst_type {
        b = match rounding {
            IntegerRound::ROUND => c.fp_round(b, st),
            IntegerRound::FLOOR => c.fp_floor(b, st),
            IntegerRound::CEIL => c.fp_ceiling(b, st),
            IntegerRound::TRUNC => c.fp_truncate(b, st),
            _ => b,
        };
    }
    // FP16 works as FP32; FP16 <-> FP64 is invalid.
    if src_type == DstFmt::F32 && dst_type == DstFmt::F64 {
        b = c.fp32_convert_to_fp64(b);
    } else if src_type == DstFmt::F64 && dst_type == DstFmt::F32 {
        b = c.fp64_convert_to_fp32(b);
    }
    let b = c.fp_saturate_if(b, sat, fp_type(dst_type));
    write_fp(c, dst_type, b, rd);
}

#[allow(clippy::too_many_arguments)]
fn emit_f2i(c: &mut Ctx, src_type: DstFmt, dst_type: IDstFmt, rounding: RoundMode2, src: OperandId, rd: i32, abs: bool, neg: bool) -> Result<(), String> {
    if dst_type == IDstFmt::U64 {
        c.log("Unimplemented 64-bits F2I.");
    }
    let fp = fp_type(src_type);
    let signed = matches!(dst_type, IDstFmt::S16 | IDstFmt::S32 | IDstFmt::S64);
    let small = matches!(dst_type, IDstFmt::U16 | IDstFmt::S16);
    let mut b = c.fp_abs_neg(src, abs, neg, fp);
    b = match rounding {
        RoundMode2::ROUND => c.fp_round(b, fp),
        RoundMode2::FLOOR => c.fp_floor(b, fp),
        RoundMode2::CEIL => c.fp_ceiling(b, fp),
        RoundMode2::TRUNC => c.fp_truncate(b, fp),
        _ => b,
    };
    if !signed {
        // A negative float converted to uint is undefined: clamp first.
        let c0 = if src_type == DstFmt::F64 { c.pack_double_2x32_const(0.0) } else { c.const_f(0.0) };
        b = c.fp_maximum(b, c0, fp);
    }
    b = if src_type == DstFmt::F64 {
        if signed { c.fp64_convert_to_s32(b) } else { c.fp64_convert_to_u32(b) }
    } else if signed {
        c.fp32_convert_to_s32(b)
    } else {
        c.fp32_convert_to_u32(b)
    };
    if small {
        let min = int_min_idst(dst_type)? as i32;
        let max = int_max_idst(dst_type)? as i32;
        let (lo, hi) = (c.const_i(min), c.const_i(max));
        b = if signed { c.iclamp_s32(b, lo, hi) } else { c.iclamp_u32(b, lo, hi) };
    }
    let d = get_dest(c, rd);
    c.copy_to(d, b);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_i2f(c: &mut Ctx, src_type: ISrcFmt, dst_type: DstFmt, src: OperandId, byte_sel: ByteSel, rd: i32, abs: bool, neg: bool) {
    let signed = matches!(src_type, ISrcFmt::S8 | ISrcFmt::S16 | ISrcFmt::S32 | ISrcFmt::S64);
    let small = matches!(src_type, ISrcFmt::U16 | ISrcFmt::S16 | ISrcFmt::U8 | ISrcFmt::S8);
    let mut b = c.iabs_neg(src, abs, neg);
    if small {
        let size = if matches!(src_type, ISrcFmt::U16 | ISrcFmt::S16) { 16 } else { 8 };
        let (off, sz) = (c.const_i(byte_sel.0 as i32 * 8), c.const_i(size));
        b = if signed { c.bitfield_extract_s32(b, off, sz) } else { c.bitfield_extract_u32(b, off, sz) };
    }
    b = if dst_type == DstFmt::F64 {
        if signed { c.iconvert_s32_to_fp64(b) } else { c.iconvert_u32_to_fp64(b) }
    } else if signed {
        c.iconvert_s32_to_fp32(b)
    } else {
        c.iconvert_u32_to_fp32(b)
    };
    write_fp(c, dst_type, b, rd);
}

#[allow(clippy::too_many_arguments)]
fn emit_i2i(
    c: &mut Ctx,
    src_type: ISrcDstFmt,
    dst_type: ISrcDstFmt,
    mut src: OperandId,
    byte_sel: ByteSel,
    rd: i32,
    abs: bool,
    neg: bool,
    sat: bool,
    write_cc: bool,
) -> Result<(), String> {
    if (src_type.0 & !ISrcDstFmt::S8.0) > ISrcDstFmt::U32.0 || (dst_type.0 & !ISrcDstFmt::S8.0) > ISrcDstFmt::U32.0 {
        c.log("Invalid I2I encoding.");
        return Ok(());
    }
    let src_signed = matches!(src_type, ISrcDstFmt::S8 | ISrcDstFmt::S16 | ISrcDstFmt::S32);
    let dst_signed = matches!(dst_type, ISrcDstFmt::S8 | ISrcDstFmt::S16 | ISrcDstFmt::S32);
    let src_small = matches!(src_type, ISrcDstFmt::U16 | ISrcDstFmt::S16 | ISrcDstFmt::U8 | ISrcDstFmt::S8);
    if src_small {
        let size = if matches!(src_type, ISrcDstFmt::U16 | ISrcDstFmt::S16) { 16 } else { 8 };
        let (off, sz) = (c.const_i(byte_sel.0 as i32 * 8), c.const_i(size));
        src = if src_signed { c.bitfield_extract_s32(src, off, sz) } else { c.bitfield_extract_u32(src, off, sz) };
    }
    src = c.iabs_neg(src, abs, neg);
    if sat {
        let min = int_min_isrc(dst_type)? as i32;
        let max = int_max_isrc(dst_type)? as i32;
        let (lo, hi) = (c.const_i(min), c.const_i(max));
        src = if dst_signed { c.iclamp_s32(src, lo, hi) } else { c.iclamp_u32(src, lo, hi) };
    }
    let d = get_dest(c, rd);
    c.copy_to(d, src);
    set_zn_flags(c, src, write_cc, false);
    Ok(())
}

fn unpack_reg(c: &mut Ctx, t: DstFmt, h: bool, reg: i32) -> Result<OperandId, String> {
    match t {
        DstFmt::F32 => Ok(get_src_reg(c, reg, false)),
        DstFmt::F16 => {
            let s = get_src_reg(c, reg, false);
            Ok(get_half_unpacked(c, s, HalfSwizzle::F16)?[h as usize])
        }
        DstFmt::F64 => Ok(get_src_reg(c, reg, true)),
        _ => Err(format!("Invalid floating point type \"{}\".", t.0)),
    }
}

fn unpack_cbuf(c: &mut Ctx, t: DstFmt, h: bool, slot: i32, offset: i32) -> Result<OperandId, String> {
    match t {
        DstFmt::F32 => Ok(get_src_cbuf(c, slot, offset, false)),
        DstFmt::F16 => {
            let s = get_src_cbuf(c, slot, offset, false);
            Ok(get_half_unpacked(c, s, HalfSwizzle::F16)?[h as usize])
        }
        DstFmt::F64 => Ok(get_src_cbuf(c, slot, offset, true)),
        _ => Err(format!("Invalid floating point type \"{}\".", t.0)),
    }
}

fn unpack_imm(c: &mut Ctx, t: DstFmt, h: bool, imm: i32) -> Result<OperandId, String> {
    match t {
        DstFmt::F32 => Ok(get_src_imm(c, imm, false)),
        DstFmt::F16 => {
            let s = get_src_imm(c, imm, false);
            Ok(get_half_unpacked(c, s, HalfSwizzle::F16)?[h as usize])
        }
        DstFmt::F64 => Ok(get_src_imm(c, imm, true)),
        _ => Err(format!("Invalid floating point type \"{}\".", t.0)),
    }
}

fn write_fp(c: &mut Ctx, t: DstFmt, b: OperandId, rd: i32) {
    let dest = get_dest(c, rd);
    if t == DstFmt::F32 {
        c.copy_to(dest, b);
    } else if t == DstFmt::F16 {
        let z = c.const_f(0.0);
        let p = c.pack_half_2x16(b, z);
        c.copy_to(dest, p);
    } else {
        let dest2 = get_dest2(c, rd);
        let lo = c.unpack_double_2x32_low(b);
        c.copy_to(dest, lo);
        let hi = c.unpack_double_2x32_high(b);
        c.copy_to(dest2, hi);
    }
}
