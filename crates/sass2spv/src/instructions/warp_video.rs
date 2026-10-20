// Warp-level operations and video (byte / half) arithmetic (InstEmitWarp, InstEmitVideoArithmetic,
// InstEmitVideoMinMax).

use super::helper::*;
use super::int_compare::int_comparison_ext;
use crate::decoders::inst_decoders::*;
use crate::decoders::register::REGISTER_ZERO_INDEX;
use crate::ir::{ir_consts, Instruction as I, IoVariable, OperandId, StorageKind};
use crate::tri;
use crate::translation::emitter::{Emit, EmitterContext as Ctx};

pub fn fswzadd(c: &mut Ctx) {
    let op = InstFswzadd(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let dest = get_dest(c, op.dest());
    let r = c.fp_swizzle_add(a, b, op.pn_word());
    c.copy_to(dest, r);
    set_fp_zn_flags(c, dest, op.write_cc(), I::FP32);
}

pub fn shfl(c: &mut Ctx) {
    let op = InstShfl(c.raw_op());
    let pred = c.pred(op.dest_pred());
    let a = get_src_reg(c, op.src_a(), false);
    let b = if op.b_fix_shfl() { c.const_i(op.src_b_imm()) } else { get_src_reg(c, op.src_b(), false) };
    let s = if op.c_fix_shfl() { c.const_i(op.src_c_imm()) } else { get_src_reg(c, op.src_c(), false) };
    let inst = match op.shfl_mode() {
        ShflMode::IDX => I::SHUFFLE,
        ShflMode::UP => I::SHUFFLE_UP,
        ShflMode::DOWN => I::SHUFFLE_DOWN,
        _ => I::SHUFFLE_XOR,
    };
    let (res, valid) = c.shuffle_pair(inst, a, b, s);
    let d = get_dest(c, op.dest());
    c.copy_to(d, res);
    c.copy_to(pred, valid);
}

pub fn vote(c: &mut Ctx) {
    let op = InstVote(c.raw_op());
    let pred = get_predicate(c, op.src_pred(), op.src_pred_inv());
    match emit_vote(c, op.vote_mode(), pred) {
        Some(res) => {
            let d = c.pred(op.vp_dest());
            c.copy_to(d, res);
        }
        None => c.log(&format!("Invalid vote operation: {}.", op.vote_mode().0)),
    }
    if op.dest() != REGISTER_ZERO_INDEX {
        let d = get_dest(c, op.dest());
        let b = emit_ballot(c, pred);
        c.copy_to(d, b);
    }
}

fn emit_vote(c: &mut Ctx, mode: VoteMode, pred: OperandId) -> Option<OperandId> {
    if c.tc.gpu.host_subgroup_size() <= 32 {
        return match mode {
            VoteMode::ALL => Some(c.vote_all(pred)),
            VoteMode::ANY => Some(c.vote_any(pred)),
            VoteMode::EQ => Some(c.vote_all_equal(pred)),
            _ => None,
        };
    }
    // The code assumes 32 threads: emulate with ballot masks.
    let ballot_mask = emit_ballot(c, pred);
    fn all_true(c: &mut Ctx, mask: OperandId) -> OperandId {
        let t = c.const_i(ir_consts::TRUE);
        let all = emit_ballot(c, t);
        c.icompare_equal(mask, all)
    }
    match mode {
        VoteMode::ALL => Some(all_true(c, ballot_mask)),
        VoteMode::ANY => {
            let z = c.const_i(0);
            Some(c.icompare_not_equal(ballot_mask, z))
        }
        VoteMode::EQ => {
            let a = all_true(c, ballot_mask);
            let z = c.const_i(0);
            let e = c.icompare_equal(ballot_mask, z);
            Some(c.bitwise_or(a, e))
        }
        _ => None,
    }
}

fn emit_ballot(c: &mut Ctx, pred: OperandId) -> OperandId {
    let size = c.tc.gpu.host_subgroup_size();
    if size <= 32 {
        return c.ballot(pred, 0);
    }
    let lane = c.load_io(StorageKind::Input, IoVariable::SubgroupLaneId, None, None);
    if size == 64 {
        let low = c.ballot(pred, 0);
        let high = c.ballot(pred, 1);
        let k = c.const_i(32);
        let sel = c.bitwise_and(lane, k);
        return c.conditional_select(sel, high, low);
    }
    let k5 = c.const_i(5);
    let element = c.shift_right_u32(lane, k5);
    let mut res = c.ballot(pred, 0);
    for e in 1..4 {
        let k = c.const_i(e);
        let eq = c.icompare_equal(element, k);
        let b = c.ballot(pred, e);
        res = c.conditional_select(eq, b, res);
    }
    res
}

// InstEmitVideoArithmetic.

pub fn vmad(c: &mut Ctx) {
    let op = InstVmad(c.raw_op());
    let a_signed = op.a_select().0 & VectorSelect::S8_B0.0 != 0;
    let b_signed = op.b_select().0 & VectorSelect::S8_B0.0 != 0;
    let ra = get_src_reg(c, op.src_a(), false);
    let a = extend(c, ra, op.a_select());
    let rc = get_src_reg(c, op.src_c(), false);
    let s = c.inegate_if(rc, op.avg_mode() == AvgMode::NEG_B);
    let b = if op.b_video() {
        let rb = get_src_reg(c, op.src_b(), false);
        extend(c, rb, op.b_select())
    } else {
        let mut imm = op.imm16();
        if b_signed {
            imm = (imm << 16) >> 16;
        }
        c.const_i(imm)
    };
    let mut product_low = c.imultiply(a, b);
    let mut product_high = if a_signed == b_signed {
        if a_signed { c.multiply_high_s32(a, b) } else { c.multiply_high_u32(a, b) }
    } else {
        let temp = if a_signed {
            let k = c.const_i(31);
            let sh = c.shift_right_s32(a, k);
            c.imultiply(b, sh)
        } else {
            let k = c.const_i(31);
            let sh = c.shift_right_s32(b, k);
            c.imultiply(a, sh)
        };
        let hi = c.multiply_high_u32(a, b);
        c.iadd(temp, hi)
    };
    if op.avg_mode() == AvgMode::NEG_A {
        (product_low, product_high) = negate_long(c, product_low, product_high);
    }
    let (mut res_low, sum_carry) = add_with_carry(c, product_low, s);
    let mut res_high = c.iadd(product_high, sum_carry);
    if op.avg_mode() == AvgMode::PLUS_ONE {
        let one = c.const_i(1);
        let (rl, po_carry) = add_with_carry(c, res_low, one);
        res_low = rl;
        res_high = c.iadd(res_high, po_carry);
    }
    let res_signed = op.a_select() == VectorSelect::S32
        || op.b_select() == VectorSelect::S32
        || op.avg_mode() == AvgMode::NEG_B
        || op.avg_mode() == AvgMode::NEG_A;
    let shift = match op.video_scale() {
        VideoScale::SHR7 => 7,
        VideoScale::SHR15 => 15,
        _ => 0,
    };
    if shift != 0 {
        // Low = (Low >> Shift) | (High << (32 - Shift)); High >>= Shift
        let k = c.const_i(shift);
        res_low = c.shift_right_u32(res_low, k);
        let k = c.const_i(32 - shift);
        let hs = c.shift_left(res_high, k);
        res_low = c.bitwise_or(res_low, hs);
        let k = c.const_i(shift);
        res_high = if res_signed { c.shift_right_s32(res_high, k) } else { c.shift_right_u32(res_high, k) };
    }
    let mut res = res_low;
    if op.sat() {
        let k31 = c.const_i(31);
        let sign = c.shift_right_s32(res_high, k31);
        if res_signed {
            let k31 = c.const_i(31);
            let lsign = c.shift_right_s32(res_low, k31);
            let overflow = c.icompare_not_equal(res_high, lsign);
            let (min, max) = (c.const_i(i32::MIN), c.const_i(i32::MAX));
            let clamp = c.conditional_select(sign, min, max);
            res = c.conditional_select(overflow, clamp, res_low);
        } else {
            let z = c.const_i(0);
            let overflow = c.icompare_not_equal(res_high, z);
            let ns = c.bitwise_not(sign);
            res = c.conditional_select(overflow, ns, res_low);
        }
    }
    let d = get_dest(c, op.dest());
    c.copy_to(d, res);
}

// InstEmitVideoMinMax.

fn video_src_b(c: &mut Ctx, b_video: bool, src_b: i32, b_select: VectorSelect, imm16: i32) -> OperandId {
    if b_video {
        let rb = get_src_reg(c, src_b, false);
        extend(c, rb, b_select)
    } else {
        let mut imm = imm16;
        if b_select.0 & VectorSelect::S8_B0.0 != 0 {
            imm = (imm << 16) >> 16;
        }
        c.const_i(imm)
    }
}

pub fn vmnmx(c: &mut Ctx) {
    let op = InstVmnmx(c.raw_op());
    let ra = get_src_reg(c, op.src_a(), false);
    let a = extend(c, ra, op.a_select());
    let s = get_src_reg(c, op.src_c(), false);
    let b = video_src_b(c, op.b_video(), op.src_b(), op.b_select(), op.imm16());
    let sa = op.a_select().0 & VectorSelect::S8_B0.0;
    let sb = op.b_select().0 & VectorSelect::S8_B0.0;
    let mut res;
    let res_signed;
    if sa != sb {
        // Different signedness: a max always fits U32 (one input is not negative and the result is the larger),
        // a min always fits S32 (one input is at most INT_MAX and the result is the smaller).
        res_signed = !op.mn();
        res = if op.mn() { c.imaximum_u32(a, b) } else { c.iminimum_s32(a, b) };
        let z = c.const_i(0);
        if sa != 0 {
            let b_gt_int_max = c.icompare_less(b, z);
            res = c.conditional_select(b_gt_int_max, b, res);
        } else {
            let a_gt_int_max = c.icompare_less(a, z);
            res = c.conditional_select(a_gt_int_max, a, res);
        }
    } else {
        // Same signedness: either input tells.
        res_signed = sa != 0;
        res = if op.mn() {
            if res_signed { c.imaximum_s32(a, b) } else { c.imaximum_u32(a, b) }
        } else if res_signed {
            c.iminimum_s32(a, b)
        } else {
            c.iminimum_u32(a, b)
        };
    }
    if op.sat() {
        if op.d_format() && !res_signed {
            let m = c.const_i(i32::MAX);
            res = c.iminimum_u32(res, m);
        } else if !op.d_format() && res_signed {
            let z = c.const_i(0);
            res = c.imaximum_s32(res, z);
        }
    }
    res = match op.video_op() {
        VideoOp::ACC => c.iadd(res, s),
        VideoOp::MAX => {
            if op.d_format() { c.imaximum_s32(res, s) } else { c.imaximum_u32(res, s) }
        }
        VideoOp::MIN => {
            if op.d_format() { c.iminimum_s32(res, s) } else { c.iminimum_u32(res, s) }
        }
        VideoOp::MRG16H => {
            let (o, l) = (c.const_i(16), c.const_i(16));
            c.bitfield_insert(s, res, o, l)
        }
        VideoOp::MRG16L => {
            let (o, l) = (c.const_i(0), c.const_i(16));
            c.bitfield_insert(s, res, o, l)
        }
        VideoOp::MRG8B0 => {
            let (o, l) = (c.const_i(0), c.const_i(8));
            c.bitfield_insert(s, res, o, l)
        }
        VideoOp::MRG8B2 => {
            let (o, l) = (c.const_i(16), c.const_i(8));
            c.bitfield_insert(s, res, o, l)
        }
        _ => res,
    };
    let d = get_dest(c, op.dest());
    c.copy_to(d, res);
}

pub fn vsetp(c: &mut Ctx) {
    let op = InstVsetp(c.raw_op());
    let ra = get_src_reg(c, op.src_a(), false);
    let a = extend(c, ra, op.a_select());
    let b = video_src_b(c, op.b_video(), op.src_b(), op.b_select(), op.imm16());
    let signed_a = op.a_select().0 & VectorSelect::S8_B0.0 != 0;
    let signed_b = op.b_select().0 & VectorSelect::S8_B0.0 != 0;
    let p0 = if signed_a != signed_b {
        let a32 = op.a_select().0 & !VectorSelect::S8_B0.0 == VectorSelect::U32.0;
        let b32 = op.b_select().0 & !VectorSelect::S8_B0.0 == VectorSelect::U32.0;
        if !a32 && !b32 {
            // Both are extended small integers and fit S32: a signed comparison.
            tri!(c, int_comparison_ext(c, op.v_comp(), a, b, true, false))
        } else {
            // TODO in the original: the mismatching sign case.
            c.const_i(0)
        }
    } else {
        tri!(c, int_comparison_ext(c, op.v_comp(), a, b, signed_a, false))
    };
    let p1 = c.bitwise_not(p0);
    let pred = get_predicate(c, op.src_pred(), op.src_pred_inv());
    let p0 = get_pred_logical_op(c, op.bool_op(), p0, pred);
    let p1 = get_pred_logical_op(c, op.bool_op(), p1, pred);
    let d0 = c.pred(op.dest_pred());
    c.copy_to(d0, p0);
    let d1 = c.pred(op.dest_pred_inv());
    c.copy_to(d1, p1);
}
