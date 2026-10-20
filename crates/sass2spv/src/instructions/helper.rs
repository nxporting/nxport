// Operand helpers shared by the emitters (InstEmitHelper and InstEmitAluHelper).

use crate::decoders::inst_decoders::*;
use crate::decoders::register::RegisterType;
use crate::ir::{Instruction as I, OperandId};
use crate::translation::emitter::{Emit, EmitterContext as Ctx};

pub fn get_zf(c: &mut Ctx) -> OperandId {
    c.reg(0, RegisterType::Flag)
}

pub fn get_nf(c: &mut Ctx) -> OperandId {
    c.reg(1, RegisterType::Flag)
}

pub fn get_cf(c: &mut Ctx) -> OperandId {
    c.reg(2, RegisterType::Flag)
}

pub fn get_vf(c: &mut Ctx) -> OperandId {
    c.reg(3, RegisterType::Flag)
}

pub fn get_dest(c: &mut Ctx, rd: i32) -> OperandId {
    c.gpr(rd)
}

pub fn get_dest2(c: &mut Ctx, rd: i32) -> OperandId {
    c.gpr(rd | 1)
}

pub fn get_src_cbuf(c: &mut Ctx, slot: i32, offset: i32, fp64: bool) -> OperandId {
    if fp64 {
        let (lo, hi) = (c.cbuf(slot, offset), c.cbuf(slot, offset + 1));
        c.pack_double_2x32(lo, hi)
    } else {
        c.cbuf(slot, offset)
    }
}

pub fn get_src_imm(c: &mut Ctx, imm: i32, fp64: bool) -> OperandId {
    if fp64 {
        let (lo, hi) = (c.const_i(0), c.const_i(imm));
        c.pack_double_2x32(lo, hi)
    } else {
        c.const_i(imm)
    }
}

pub fn get_src_reg(c: &mut Ctx, reg: i32, fp64: bool) -> OperandId {
    if fp64 {
        let (lo, hi) = (c.gpr(reg), c.gpr(reg | 1));
        c.pack_double_2x32(lo, hi)
    } else {
        c.gpr(reg)
    }
}

pub fn get_half_src_reg(c: &mut Ctx, swizzle: HalfSwizzle, ra: i32, negate: bool, absolute: bool) -> Result<[OperandId; 2], String> {
    let src = get_src_reg(c, ra, false);
    let ops = get_half_unpacked(c, src, swizzle)?;
    Ok(fp_abs_neg2(c, ops, absolute, negate))
}

pub fn get_half_src_cbuf(c: &mut Ctx, swizzle: HalfSwizzle, slot: i32, offset: i32, negate: bool, absolute: bool) -> Result<[OperandId; 2], String> {
    let src = get_src_cbuf(c, slot, offset, false);
    let ops = get_half_unpacked(c, src, swizzle)?;
    Ok(fp_abs_neg2(c, ops, absolute, negate))
}

pub fn get_half_src_imm2(c: &mut Ctx, imm_h0: i32, imm_h1: i32) -> [OperandId; 2] {
    let low = (imm_h0 << 6) as u16;
    let high = (imm_h1 << 6) as u16;
    [c.const_f(half_to_f32(low)), c.const_f(half_to_f32(high))]
}

pub fn get_half_src_imm32(c: &mut Ctx, imm32: i32) -> [OperandId; 2] {
    let low = imm32 as u16;
    let high = (imm32 >> 16) as u16;
    [c.const_f(half_to_f32(low)), c.const_f(half_to_f32(high))]
}

/// .NET's (float)Half: exact; NaNs keep sign and payload and come out quiet.
pub fn half_to_f32(h: u16) -> f32 {
    let sign = (h as u32 >> 15) << 31;
    let exp = (h as u32 >> 10) & 0x1f;
    let man = h as u32 & 0x3ff;
    let bits = if exp == 0x1f {
        if man != 0 { sign | 0x7fc0_0000 | (man << 13) } else { sign | 0x7f80_0000 }
    } else if exp == 0 {
        if man == 0 {
            sign
        } else {
            // Subnormal: normalise.
            let mut e = 127 - 15 + 1;
            let mut m = man;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | ((e as u32) << 23) | ((m & 0x3ff) << 13)
        }
    } else {
        sign | ((exp + 127 - 15) << 23) | (man << 13)
    };
    f32::from_bits(bits)
}

pub fn fp_abs_neg2(c: &mut Ctx, ops: [OperandId; 2], abs: bool, neg: bool) -> [OperandId; 2] {
    let a = c.fp_abs_neg(ops[0], abs, neg, I::FP32);
    let b = c.fp_abs_neg(ops[1], abs, neg, I::FP32);
    [a, b]
}

pub fn get_half_unpacked(c: &mut Ctx, src: OperandId, swizzle: HalfSwizzle) -> Result<[OperandId; 2], String> {
    Ok(match swizzle {
        HalfSwizzle::F16 => {
            let lo = c.unpack_half_2x16_low(src);
            let hi = c.unpack_half_2x16_high(src);
            [lo, hi]
        }
        HalfSwizzle::F32 => [src, src],
        HalfSwizzle::H0_H0 => {
            let a = c.unpack_half_2x16_low(src);
            let b = c.unpack_half_2x16_low(src);
            [a, b]
        }
        HalfSwizzle::H1_H1 => {
            let a = c.unpack_half_2x16_high(src);
            let b = c.unpack_half_2x16_high(src);
            [a, b]
        }
        s => return Err(format!("Invalid swizzle \"{}\".", s.0)),
    })
}

pub fn get_half_packed(c: &mut Ctx, swizzle: OFmt, results: [OperandId; 2], rd: i32) -> Result<OperandId, String> {
    Ok(match swizzle {
        OFmt::F16 => c.pack_half_2x16(results[0], results[1]),
        OFmt::F32 => results[0],
        OFmt::MRG_H0 => {
            let h1 = get_half_dest(c, rd, true);
            c.pack_half_2x16(results[0], h1)
        }
        OFmt::MRG_H1 => {
            let h0 = get_half_dest(c, rd, false);
            c.pack_half_2x16(h0, results[1])
        }
        s => return Err(format!("Invalid swizzle \"{}\".", s.0)),
    })
}

pub fn get_half_dest(c: &mut Ctx, rd: i32, is_high: bool) -> OperandId {
    let d = get_dest(c, rd);
    if is_high { c.unpack_half_2x16_high(d) } else { c.unpack_half_2x16_low(d) }
}

pub fn get_predicate(c: &mut Ctx, pred: i32, not: bool) -> OperandId {
    let local = c.pred(pred);
    if not { c.bitwise_not(local) } else { local }
}

pub fn set_dest(c: &mut Ctx, value: OperandId, rd: i32, fp64: bool) {
    if fp64 {
        let d = get_dest(c, rd);
        let lo = c.unpack_double_2x32_low(value);
        c.copy_to(d, lo);
        let d2 = get_dest2(c, rd);
        let hi = c.unpack_double_2x32_high(value);
        c.copy_to(d2, hi);
    } else {
        let d = get_dest(c, rd);
        c.copy_to(d, value);
    }
}

pub fn imm16_to_sint(imm16: i32) -> i32 {
    imm16 as i16 as i32
}

pub fn imm20_to_float(imm20: i32) -> i32 {
    imm20 << 12
}

pub fn imm20_to_sint(imm20: i32) -> i32 {
    (imm20 << 12) >> 12
}

pub fn imm24_to_sint(imm24: i32) -> i32 {
    (imm24 << 8) >> 8
}

pub fn sign_extend_to_32(c: &mut Ctx, src: OperandId, src_bits: i32) -> OperandId {
    let (z, b) = (c.const_i(0), c.const_i(src_bits));
    c.bitfield_extract_s32(src, z, b)
}

pub fn zero_extend_to_32(c: &mut Ctx, src: OperandId, src_bits: i32) -> OperandId {
    let mask = (u32::MAX >> (32 - src_bits)) as i32;
    let m = c.const_i(mask);
    c.bitwise_and(src, m)
}

// InstEmitAluHelper.

pub fn int_min_idst(t: IDstFmt) -> Result<i64, String> {
    Ok(match t {
        IDstFmt::U16 => 0,
        IDstFmt::S16 => i16::MIN as i64,
        IDstFmt::U32 => 0,
        IDstFmt::S32 => i32::MIN as i64,
        _ => return Err(format!("The type \"{}\" is not a supported integer type.", t.0)),
    })
}

pub fn int_max_idst(t: IDstFmt) -> Result<i64, String> {
    Ok(match t {
        IDstFmt::U16 => u16::MAX as i64,
        IDstFmt::S16 => i16::MAX as i64,
        IDstFmt::U32 => u32::MAX as i64,
        IDstFmt::S32 => i32::MAX as i64,
        _ => return Err(format!("The type \"{}\" is not a supported integer type.", t.0)),
    })
}

pub fn int_min_isrc(t: ISrcDstFmt) -> Result<i64, String> {
    Ok(match t {
        ISrcDstFmt::U8 => 0,
        ISrcDstFmt::S8 => i8::MIN as i64,
        ISrcDstFmt::U16 => 0,
        ISrcDstFmt::S16 => i16::MIN as i64,
        ISrcDstFmt::U32 => 0,
        ISrcDstFmt::S32 => i32::MIN as i64,
        _ => return Err(format!("The type \"{}\" is not a supported integer type.", t.0)),
    })
}

pub fn int_max_isrc(t: ISrcDstFmt) -> Result<i64, String> {
    Ok(match t {
        ISrcDstFmt::U8 => u8::MAX as i64,
        ISrcDstFmt::S8 => i8::MAX as i64,
        ISrcDstFmt::U16 => u16::MAX as i64,
        ISrcDstFmt::S16 => i16::MAX as i64,
        ISrcDstFmt::U32 => u32::MAX as i64,
        ISrcDstFmt::S32 => i32::MAX as i64,
        _ => return Err(format!("The type \"{}\" is not a supported integer type.", t.0)),
    })
}

pub fn get_pred_logical_op(c: &mut Ctx, logic_op: BoolOp, input: OperandId, pred: OperandId) -> OperandId {
    match logic_op {
        BoolOp::AND => c.bitwise_and(input, pred),
        BoolOp::OR => c.bitwise_or(input, pred),
        BoolOp::XOR => c.bitwise_exclusive_or(input, pred),
        _ => input,
    }
}

pub fn extend(c: &mut Ctx, src: OperandId, ty: VectorSelect) -> OperandId {
    let (shift, bits, signed) = match ty {
        VectorSelect::U8_B0 => (0, 8, false),
        VectorSelect::U8_B1 => (8, 8, false),
        VectorSelect::U8_B2 => (16, 8, false),
        VectorSelect::U8_B3 => (24, 8, false),
        VectorSelect::U16_H0 => (0, 16, false),
        VectorSelect::U16_H1 => (16, 16, false),
        VectorSelect::S8_B0 => (0, 8, true),
        VectorSelect::S8_B1 => (8, 8, true),
        VectorSelect::S8_B2 => (16, 8, true),
        VectorSelect::S8_B3 => (24, 8, true),
        VectorSelect::S16_H0 => (0, 16, true),
        VectorSelect::S16_H1 => (16, 16, true),
        _ => return src,
    };
    let s = c.const_i(shift);
    let shifted = c.shift_right_u32(src, s);
    if signed { sign_extend_to_32(c, shifted, bits) } else { zero_extend_to_32(c, shifted, bits) }
}

pub fn set_zn_flags(c: &mut Ctx, dest: OperandId, set_cc: bool, extended: bool) {
    if !set_cc {
        return;
    }
    if extended {
        // Extended (multi-word) operations: ZF only if every word is zero.
        let old_zf = get_zf(c);
        let zero = c.const_i(0);
        let eq = c.icompare_equal(dest, zero);
        let res = c.bitwise_and(eq, old_zf);
        let zf = get_zf(c);
        c.copy_to(zf, res);
    } else {
        let zf = get_zf(c);
        let zero = c.const_i(0);
        let eq = c.icompare_equal(dest, zero);
        c.copy_to(zf, eq);
    }
    let nf = get_nf(c);
    let zero = c.const_i(0);
    let lt = c.icompare_less(dest, zero);
    c.copy_to(nf, lt);
}

pub fn set_fp_zn_flags(c: &mut Ctx, dest: OperandId, set_cc: bool, fp: I) {
    if set_cc {
        let mut zero = c.const_f(0.0);
        if fp == I::FP64 {
            zero = c.fp32_convert_to_fp64(zero);
        }
        let zf = get_zf(c);
        let eq = c.fp_compare_equal(dest, zero, fp);
        c.copy_to(zf, eq);
        let nf = get_nf(c);
        let lt = c.fp_compare_less(dest, zero, fp);
        c.copy_to(nf, lt);
    }
}

pub fn negate_long(c: &mut Ctx, low: OperandId, high: OperandId) -> (OperandId, OperandId) {
    let low = c.bitwise_not(low);
    let high = c.bitwise_not(high);
    let one = c.const_i(1);
    let (low, carry_out) = add_with_carry(c, low, one);
    let high = c.iadd(high, carry_out);
    (low, high)
}

/// (lhs + rhs, carry out as 0 / -1).
pub fn add_with_carry(c: &mut Ctx, lhs: OperandId, rhs: OperandId) -> (OperandId, OperandId) {
    let result = c.iadd(lhs, rhs);
    // C = Rd < Rn
    let lt = c.icompare_less_unsigned(result, lhs);
    let carry_out = c.inegate(lt);
    (result, carry_out)
}
