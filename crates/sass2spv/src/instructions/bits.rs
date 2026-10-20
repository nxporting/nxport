// Bitfields, bit counts and shifts (InstEmitBitfield, InstEmitShift).

use super::helper::*;
use crate::decoders::inst_decoders::*;
use crate::ir::OperandId;
use crate::translation::emitter::{Emit, EmitterContext as Ctx};

pub fn bfe_r(c: &mut Ctx) {
    let op = InstBfeR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    emit_bfe(c, a, b, op.dest(), op.brev(), op.signed());
}

pub fn bfe_i(c: &mut Ctx) {
    let op = InstBfeI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_bfe(c, a, b, op.dest(), op.brev(), op.signed());
}

pub fn bfe_c(c: &mut Ctx) {
    let op = InstBfeC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_bfe(c, a, b, op.dest(), op.brev(), op.signed());
}

pub fn bfi_r(c: &mut Ctx) {
    let op = InstBfiR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_bfi(c, a, b, s, op.dest());
}

pub fn bfi_i(c: &mut Ctx) {
    let op = InstBfiI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_bfi(c, a, b, s, op.dest());
}

pub fn bfi_c(c: &mut Ctx) {
    let op = InstBfiC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_bfi(c, a, b, s, op.dest());
}

pub fn bfi_rc(c: &mut Ctx) {
    let op = InstBfiRc(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_c(), false);
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_bfi(c, a, b, s, op.dest());
}

pub fn flo_r(c: &mut Ctx) {
    let op = InstFloR(c.raw_op());
    let s = get_src_reg(c, op.src_b(), false);
    emit_flo(c, s, op.dest(), op.neg_b(), op.sh(), op.signed());
}

pub fn flo_i(c: &mut Ctx) {
    let op = InstFloI(c.raw_op());
    let s = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_flo(c, s, op.dest(), op.neg_b(), op.sh(), op.signed());
}

pub fn flo_c(c: &mut Ctx) {
    let op = InstFloC(c.raw_op());
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_flo(c, s, op.dest(), op.neg_b(), op.sh(), op.signed());
}

pub fn popc_r(c: &mut Ctx) {
    let op = InstPopcR(c.raw_op());
    let s = get_src_reg(c, op.src_b(), false);
    emit_popc(c, s, op.dest(), op.neg_b());
}

pub fn popc_i(c: &mut Ctx) {
    let op = InstPopcI(c.raw_op());
    let s = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_popc(c, s, op.dest(), op.neg_b());
}

pub fn popc_c(c: &mut Ctx) {
    let op = InstPopcC(c.raw_op());
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_popc(c, s, op.dest(), op.neg_b());
}

fn emit_bfe(c: &mut Ctx, mut a: OperandId, b: OperandId, rd: i32, bit_reverse: bool, signed: bool) {
    if bit_reverse {
        a = c.bitfield_reverse(a);
    }
    let m = c.const_i(0xff);
    let position = c.bitwise_and(b, m);
    let (k8, k8b) = (c.const_i(8), c.const_i(8));
    let size = c.bitfield_extract_u32(b, k8, k8b);
    let res = if signed { c.bitfield_extract_s32(a, position, size) } else { c.bitfield_extract_u32(a, position, size) };
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

fn emit_bfi(c: &mut Ctx, a: OperandId, b: OperandId, s: OperandId, rd: i32) {
    let m = c.const_i(0xff);
    let position = c.bitwise_and(b, m);
    let (k8, k8b) = (c.const_i(8), c.const_i(8));
    let size = c.bitfield_extract_u32(b, k8, k8b);
    let res = c.bitfield_insert(s, a, position, size);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

fn emit_flo(c: &mut Ctx, src: OperandId, rd: i32, invert: bool, sh: bool, signed: bool) {
    let b = c.bitwise_not_if(src, invert);
    let res = if sh {
        let r = c.bitfield_reverse(b);
        c.find_lsb(r)
    } else if signed {
        c.find_msb_s32(b)
    } else {
        c.find_msb_u32(b)
    };
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

fn emit_popc(c: &mut Ctx, src: OperandId, rd: i32, invert: bool) {
    let b = c.bitwise_not_if(src, invert);
    let res = c.bit_count(b);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

// InstEmitShift.

pub fn shf_lr(c: &mut Ctx) {
    let op = InstShfLR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_shf(c, op.max_shift(), a, b, s, op.dest(), op.m(), true, op.write_cc());
}

pub fn shf_rr(c: &mut Ctx) {
    let op = InstShfRR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let s = get_src_reg(c, op.src_c(), false);
    emit_shf(c, op.max_shift(), a, b, s, op.dest(), op.m(), false, op.write_cc());
}

pub fn shf_li(c: &mut Ctx) {
    let op = InstShfLI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = c.const_i(op.imm6());
    let s = get_src_reg(c, op.src_c(), false);
    emit_shf(c, op.max_shift(), a, b, s, op.dest(), op.m(), true, op.write_cc());
}

pub fn shf_ri(c: &mut Ctx) {
    let op = InstShfRI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = c.const_i(op.imm6());
    let s = get_src_reg(c, op.src_c(), false);
    emit_shf(c, op.max_shift(), a, b, s, op.dest(), op.m(), false, op.write_cc());
}

pub fn shl_r(c: &mut Ctx) {
    let op = InstShlR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    emit_shl(c, a, b, op.dest(), op.m());
}

pub fn shl_i(c: &mut Ctx) {
    let op = InstShlI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_shl(c, a, b, op.dest(), op.m());
}

pub fn shl_c(c: &mut Ctx) {
    let op = InstShlC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_shl(c, a, b, op.dest(), op.m());
}

pub fn shr_r(c: &mut Ctx) {
    let op = InstShrR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    emit_shr(c, a, b, op.dest(), op.m(), op.brev(), op.signed());
}

pub fn shr_i(c: &mut Ctx) {
    let op = InstShrI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_shr(c, a, b, op.dest(), op.m(), op.brev(), op.signed());
}

pub fn shr_c(c: &mut Ctx) {
    let op = InstShrC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_shr(c, a, b, op.dest(), op.m(), op.brev(), op.signed());
}

#[allow(clippy::too_many_arguments)]
fn emit_shf(c: &mut Ctx, max_shift: MaxShift, a: OperandId, mut b: OperandId, s: OperandId, rd: i32, mask: bool, left: bool, write_cc: bool) {
    let long_shift = max_shift == MaxShift::U64 || max_shift == MaxShift::S64;
    let signed_shift = max_shift == MaxShift::S64;
    let max_shift_const = if long_shift { 64 } else { 32 };
    if mask {
        let m = c.const_i(max_shift_const - 1);
        b = c.bitwise_and(b, m);
    }
    let mut res;
    if left {
        // res = (C << B) | (A >> (32 - B))
        res = c.shift_left(s, b);
        let k32 = c.const_i(32);
        let sub = c.isubtract(k32, b);
        let sh = c.shift_right_u32(a, sub);
        res = c.bitwise_or(res, sh);
        if long_shift {
            // res = B >= 32 ? A << (B - 32) : res
            let k32 = c.const_i(32);
            let sub = c.isubtract(b, k32);
            let lower = c.shift_left(a, sub);
            let k32 = c.const_i(32);
            let gt31 = c.icompare_greater_or_equal_unsigned(b, k32);
            res = c.conditional_select(gt31, lower, res);
        }
    } else {
        // res = (A >> B) | (C << (32 - B))
        res = c.shift_right_u32(a, b);
        let k32 = c.const_i(32);
        let sub = c.isubtract(k32, b);
        let sh = c.shift_left(s, sub);
        res = c.bitwise_or(res, sh);
        if long_shift {
            // res = B >= 32 ? C >> (B - 32) : res
            let k32 = c.const_i(32);
            let sub = c.isubtract(b, k32);
            let upper = if signed_shift { c.shift_right_s32(s, sub) } else { c.shift_right_u32(s, sub) };
            let k32 = c.const_i(32);
            let gt31 = c.icompare_greater_or_equal_unsigned(b, k32);
            res = c.conditional_select(gt31, upper, res);
        }
    }
    if !mask {
        // Clamped shift value.
        let k = c.const_i(max_shift_const);
        let lt = c.icompare_less_unsigned(b, k);
        let z = c.const_i(0);
        res = c.conditional_select(lt, res, z);
    }
    let d = get_dest(c, rd);
    c.copy_to(d, res);
    if write_cc {
        set_zn_flags(c, res, write_cc, false);
    }
}

fn emit_shl(c: &mut Ctx, a: OperandId, mut b: OperandId, rd: i32, mask: bool) {
    if mask {
        let m = c.const_i(0x1f);
        b = c.bitwise_and(b, m);
    }
    let mut res = c.shift_left(a, b);
    if !mask {
        // Clamped shift value.
        let k32 = c.const_i(32);
        let lt = c.icompare_less_unsigned(b, k32);
        let z = c.const_i(0);
        res = c.conditional_select(lt, res, z);
    }
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

fn emit_shr(c: &mut Ctx, mut a: OperandId, mut b: OperandId, rd: i32, mask: bool, bit_reverse: bool, signed: bool) {
    if bit_reverse {
        a = c.bitfield_reverse(a);
    }
    if mask {
        let m = c.const_i(0x1f);
        b = c.bitwise_and(b, m);
    }
    let mut res = if signed { c.shift_right_s32(a, b) } else { c.shift_right_u32(a, b) };
    if !mask {
        // Clamped shift value.
        let by32 = if signed {
            let k31 = c.const_i(31);
            c.shift_right_s32(a, k31)
        } else {
            c.const_i(0)
        };
        let k32 = c.const_i(32);
        let lt = c.icompare_less_unsigned(b, k32);
        res = c.conditional_select(lt, res, by32);
    }
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}
