// Moves, special registers, predicates, condition codes and barriers (InstEmitMove, InstEmitPredicate,
// InstEmitConditionCode, InstEmitBarrier).

use super::helper::*;
use crate::decoders::inst_decoders::*;
use crate::decoders::register::{RegisterType, FLAGS_COUNT, PREDS_COUNT};
use crate::ir::{ir_consts, IoVariable, OperandId, StorageKind};
use crate::translation::emitter::{Emit, EmitterContext as Ctx};
use crate::types::ShaderStage;

pub fn mov_r(c: &mut Ctx) {
    let op = InstMovR(c.raw_op());
    let d = get_dest(c, op.dest());
    let s = get_src_reg(c, op.src_a(), false);
    c.copy_to(d, s);
}

pub fn mov_i(c: &mut Ctx) {
    let op = InstMovI(c.raw_op());
    let d = get_dest(c, op.dest());
    let s = get_src_imm(c, op.imm20(), false);
    c.copy_to(d, s);
}

pub fn mov_c(c: &mut Ctx) {
    let op = InstMovC(c.raw_op());
    let d = get_dest(c, op.dest());
    let s = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    c.copy_to(d, s);
}

pub fn mov32i(c: &mut Ctx) {
    let op = InstMov32i(c.raw_op());
    let d = get_dest(c, op.dest());
    let s = get_src_imm(c, op.imm32(), false);
    c.copy_to(d, s);
}

pub fn r2p_r(c: &mut Ctx) {
    let op = InstR2pR(c.raw_op());
    let value = get_src_reg(c, op.src_a(), false);
    let mask = get_src_reg(c, op.src_b(), false);
    emit_r2p(c, value, mask, op.byte_sel(), op.ccpr());
}

pub fn r2p_i(c: &mut Ctx) {
    let op = InstR2pI(c.raw_op());
    let value = get_src_reg(c, op.src_a(), false);
    let mask = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    emit_r2p(c, value, mask, op.byte_sel(), op.ccpr());
}

pub fn r2p_c(c: &mut Ctx) {
    let op = InstR2pC(c.raw_op());
    let value = get_src_reg(c, op.src_a(), false);
    let mask = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_r2p(c, value, mask, op.byte_sel(), op.ccpr());
}

fn load_in(c: &mut Ctx, io: IoVariable) -> OperandId {
    c.load_io(StorageKind::Input, io, None, None)
}

fn load_in_elem(c: &mut Ctx, io: IoVariable, elem: i32) -> OperandId {
    let e = c.const_i(elem);
    c.load_io(StorageKind::Input, io, None, Some(e))
}

pub fn s2r(c: &mut Ctx) {
    let op = InstS2r(c.raw_op());
    let stage = c.stage();
    let src = match op.s_reg() {
        SReg::LANE_ID => load_subgroup_lane_id(c),
        SReg::INVOCATION_ID => load_in(c, IoVariable::InvocationId),
        // TODO in the original: the Y direction GPU register.
        SReg::Y_DIRECTION => c.const_f(1.0),
        SReg::THREAD_KILL => {
            if stage == ShaderStage::Fragment { load_in(c, IoVariable::ThreadKill) } else { c.const_i(0) }
        }
        SReg::INVOCATION_INFO => {
            if stage != ShaderStage::Compute && stage != ShaderStage::Fragment {
                // The low 8 bits hold a primitive index used only for ISBE offsets: left zero.
                if matches!(stage, ShaderStage::TessellationControl | ShaderStage::TessellationEvaluation) {
                    let pv = load_in(c, IoVariable::PatchVertices);
                    let k = c.const_i(16);
                    c.shift_left(pv, k)
                } else {
                    c.const_i(c.tc.definitions.input_topology().input_vertex_count() << 16)
                }
            } else {
                c.const_i(0)
            }
        }
        SReg::T_ID => {
            let x = load_in_elem(c, IoVariable::ThreadId, 0);
            let y = load_in_elem(c, IoVariable::ThreadId, 1);
            let z = load_in_elem(c, IoVariable::ThreadId, 2);
            let k16 = c.const_i(16);
            let y = c.shift_left(y, k16);
            let k26 = c.const_i(26);
            let z = c.shift_left(z, k26);
            let yz = c.bitwise_or(y, z);
            c.bitwise_or(x, yz)
        }
        SReg::T_ID_X => load_in_elem(c, IoVariable::ThreadId, 0),
        SReg::T_ID_Y => load_in_elem(c, IoVariable::ThreadId, 1),
        SReg::T_ID_Z => load_in_elem(c, IoVariable::ThreadId, 2),
        SReg::CTA_ID_X => load_in_elem(c, IoVariable::CtaId, 0),
        SReg::CTA_ID_Y => load_in_elem(c, IoVariable::CtaId, 1),
        SReg::CTA_ID_Z => load_in_elem(c, IoVariable::CtaId, 2),
        SReg::EQ_MASK => load_subgroup_mask(c, IoVariable::SubgroupEqMask),
        SReg::LT_MASK => load_subgroup_mask(c, IoVariable::SubgroupLtMask),
        SReg::LE_MASK => load_subgroup_mask(c, IoVariable::SubgroupLeMask),
        SReg::GT_MASK => load_subgroup_mask(c, IoVariable::SubgroupGtMask),
        SReg::GE_MASK => load_subgroup_mask(c, IoVariable::SubgroupGeMask),
        _ => c.const_i(0),
    };
    let d = get_dest(c, op.dest());
    c.copy_to(d, src);
}

fn load_subgroup_lane_id(c: &mut Ctx) -> OperandId {
    if c.tc.gpu.host_subgroup_size() <= 32 {
        return load_in(c, IoVariable::SubgroupLaneId);
    }
    let l = load_in(c, IoVariable::SubgroupLaneId);
    let m = c.const_i(0x1f);
    c.bitwise_and(l, m)
}

fn load_subgroup_mask(c: &mut Ctx, io: IoVariable) -> OperandId {
    let subgroup_size = c.tc.gpu.host_subgroup_size();
    if subgroup_size <= 32 {
        return load_in_elem(c, io, 0);
    }
    if subgroup_size == 64 {
        let lane = load_in(c, IoVariable::SubgroupLaneId);
        let low = load_in_elem(c, io, 0);
        let high = load_in_elem(c, io, 1);
        let k = c.const_i(32);
        let sel = c.bitwise_and(lane, k);
        return c.conditional_select(sel, high, low);
    }
    let lane = load_in(c, IoVariable::SubgroupLaneId);
    let k5 = c.const_i(5);
    let element = c.shift_right_u32(lane, k5);
    let mut res = load_in_elem(c, io, 0);
    for e in 1..4 {
        let k = c.const_i(e);
        let eq = c.icompare_equal(element, k);
        let v = load_in_elem(c, io, e);
        res = c.conditional_select(eq, v, res);
    }
    res
}

pub fn sel_r(c: &mut Ctx) {
    let op = InstSelR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_reg(c, op.src_b(), false);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_sel(c, a, b, p, op.dest());
}

pub fn sel_i(c: &mut Ctx) {
    let op = InstSelI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_imm(c, imm20_to_sint(op.imm20()), false);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_sel(c, a, b, p, op.dest());
}

pub fn sel_c(c: &mut Ctx) {
    let op = InstSelC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let b = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    let p = get_predicate(c, op.src_pred(), op.src_pred_inv());
    emit_sel(c, a, b, p, op.dest());
}

fn emit_r2p(c: &mut Ctx, value: OperandId, mask: OperandId, byte_sel: ByteSel, ccpr: bool) {
    fn test(c: &mut Ctx, v: OperandId, bit: i32) -> OperandId {
        let m = c.const_i(1 << bit);
        let a = c.bitwise_and(v, m);
        let z = c.const_i(0);
        c.icompare_not_equal(a, z)
    }
    let count = if ccpr { FLAGS_COUNT } else { PREDS_COUNT };
    let ty = if ccpr { RegisterType::Flag } else { RegisterType::Predicate };
    let shift = byte_sel.0 as i32 * 8;
    for bit in 0..count {
        let flag = c.reg(bit, ty);
        let tm = test(c, mask, bit);
        let tv = test(c, value, bit + shift);
        let res = c.conditional_select(tm, tv, flag);
        c.copy_to(flag, res);
    }
}

fn emit_sel(c: &mut Ctx, a: OperandId, b: OperandId, pred: OperandId, rd: i32) {
    let res = c.conditional_select(pred, a, b);
    let d = get_dest(c, rd);
    c.copy_to(d, res);
}

// InstEmitPredicate.

pub fn pset(c: &mut Ctx) {
    let op = InstPset(c.raw_op());
    let p2 = c.pred(op.src2_pred());
    let a = c.bitwise_not_if(p2, op.src2_pred_inv());
    let p1 = c.pred(op.src1_pred());
    let b = c.bitwise_not_if(p1, op.src1_pred_inv());
    let p0 = c.pred(op.src_pred());
    let s = c.bitwise_not_if(p0, op.src_pred_inv());
    let res = get_pred_logical_op(c, op.bool_op_ab(), a, b);
    let res = get_pred_logical_op(c, op.bool_op_c(), res, s);
    let dest = get_dest(c, op.dest());
    if op.b_val() {
        let (one, zero) = (c.const_f(1.0), c.const_f(0.0));
        let sel = c.conditional_select(res, one, zero);
        c.copy_to(dest, sel);
    } else {
        c.copy_to(dest, res);
    }
}

pub fn psetp(c: &mut Ctx) {
    let op = InstPsetp(c.raw_op());
    let p2 = c.pred(op.src2_pred());
    let a = c.bitwise_not_if(p2, op.src2_pred_inv());
    let p1 = c.pred(op.src1_pred());
    let b = c.bitwise_not_if(p1, op.src1_pred_inv());
    let p0 = get_pred_logical_op(c, op.bool_op_ab(), a, b);
    let p1 = c.bitwise_not(p0);
    let pred = get_predicate(c, op.src_pred(), op.src_pred_inv());
    let p0 = get_pred_logical_op(c, op.bool_op_c(), p0, pred);
    let p1 = get_pred_logical_op(c, op.bool_op_c(), p1, pred);
    let d0 = c.pred(op.dest_pred());
    c.copy_to(d0, p0);
    let d1 = c.pred(op.dest_pred_inv());
    c.copy_to(d1, p1);
}

pub fn p2r_c(c: &mut Ctx) {
    let op = InstP2rC(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let dest = get_src_reg(c, op.dest(), false);
    let mask = get_src_cbuf(c, op.cbuf_slot(), op.cbuf_offset(), false);
    emit_p2r(c, a, dest, mask, op.byte_sel(), op.ccpr());
}

pub fn p2r_i(c: &mut Ctx) {
    let op = InstP2rI(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let dest = get_src_reg(c, op.dest(), false);
    let mask = get_src_imm(c, op.imm20(), false);
    emit_p2r(c, a, dest, mask, op.byte_sel(), op.ccpr());
}

pub fn p2r_r(c: &mut Ctx) {
    let op = InstP2rR(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let dest = get_src_reg(c, op.dest(), false);
    let mask = get_src_reg(c, op.src_b(), false);
    emit_p2r(c, a, dest, mask, op.byte_sel(), op.ccpr());
}

fn emit_p2r(c: &mut Ctx, a: OperandId, dest: OperandId, mask: OperandId, byte_sel: ByteSel, ccpr: bool) {
    let count = if ccpr { FLAGS_COUNT } else { PREDS_COUNT };
    let shift = byte_sel.0 as i32 * 8;
    let m = c.const_i(0xff);
    let mask = c.bitwise_and(mask, m);
    let mut insert = c.const_i(0);
    for i in 0..count {
        let condition = if ccpr { c.reg(i, RegisterType::Flag) } else { c.reg(i, RegisterType::Predicate) };
        let (one, zero) = (c.const_i(1 << (i + shift)), c.const_i(0));
        let bit = c.conditional_select(condition, one, zero);
        insert = c.bitwise_or(insert, bit);
    }
    let k = c.const_i(shift);
    let mask_shifted = c.shift_left(mask, k);
    let not_mask = c.bitwise_not(mask_shifted);
    let masked = c.bitwise_and(a, not_mask);
    let ins = c.bitwise_and(insert, mask_shifted);
    let res = c.bitwise_or(masked, ins);
    c.copy_to(dest, res);
}

// InstEmitConditionCode.

pub fn cset(c: &mut Ctx) {
    let op = InstCset(c.raw_op());
    let res = get_condition(c, op.ccc(), ir_consts::TRUE);
    let pred = get_predicate(c, op.src_pred(), op.src_pred_inv());
    let res = get_pred_logical_op(c, op.bop(), res, pred);
    let dest = get_dest(c, op.dest());
    if op.b_val() {
        let (one, zero) = (c.const_f(1.0), c.const_i(0));
        let sel = c.conditional_select(res, one, zero);
        c.copy_to(dest, sel);
    } else {
        c.copy_to(dest, res);
    }
}

pub fn csetp(c: &mut Ctx) {
    let op = InstCsetp(c.raw_op());
    let p0 = get_condition(c, op.ccc(), ir_consts::TRUE);
    let p1 = c.bitwise_not(p0);
    let pred = get_predicate(c, op.src_pred(), op.src_pred_inv());
    let p0 = get_pred_logical_op(c, op.bop(), p0, pred);
    let p1 = get_pred_logical_op(c, op.bop(), p1, pred);
    let d0 = c.pred(op.dest_pred());
    c.copy_to(d0, p0);
    let d1 = c.pred(op.dest_pred_inv());
    c.copy_to(d1, p1);
}

pub fn get_condition(c: &mut Ctx, cond: Ccc, default_cond: i32) -> OperandId {
    match cond {
        Ccc::F => c.const_i(ir_consts::FALSE),
        Ccc::LT => {
            let (nf, zf) = (get_nf(c), get_zf(c));
            let nz = c.bitwise_not(zf);
            let a = c.bitwise_and(nf, nz);
            let vf = get_vf(c);
            c.bitwise_exclusive_or(a, vf)
        }
        Ccc::EQ => {
            let nf = get_nf(c);
            let nn = c.bitwise_not(nf);
            let zf = get_zf(c);
            c.bitwise_and(nn, zf)
        }
        Ccc::LE => {
            let (nf, zf, vf) = (get_nf(c), get_zf(c), get_vf(c));
            let o = c.bitwise_or(zf, vf);
            c.bitwise_exclusive_or(nf, o)
        }
        Ccc::GT => {
            let (nf, vf) = (get_nf(c), get_vf(c));
            let x = c.bitwise_exclusive_or(nf, vf);
            let zf = get_zf(c);
            let o = c.bitwise_or(x, zf);
            c.bitwise_not(o)
        }
        Ccc::NE => {
            let zf = get_zf(c);
            c.bitwise_not(zf)
        }
        Ccc::GE => {
            let (nf, vf) = (get_nf(c), get_vf(c));
            let x = c.bitwise_exclusive_or(nf, vf);
            c.bitwise_not(x)
        }
        Ccc::NUM => {
            let (nf, zf) = (get_nf(c), get_zf(c));
            let a = c.bitwise_and(nf, zf);
            c.bitwise_not(a)
        }
        Ccc::NAN => {
            let (nf, zf) = (get_nf(c), get_zf(c));
            c.bitwise_and(nf, zf)
        }
        Ccc::LTU => {
            let (nf, vf) = (get_nf(c), get_vf(c));
            c.bitwise_exclusive_or(nf, vf)
        }
        Ccc::EQU => get_zf(c),
        Ccc::LEU => {
            let (nf, vf) = (get_nf(c), get_vf(c));
            let x = c.bitwise_exclusive_or(nf, vf);
            let zf = get_zf(c);
            c.bitwise_or(x, zf)
        }
        Ccc::GTU => {
            let nf = get_nf(c);
            let nn = c.bitwise_not(nf);
            let (vf, zf) = (get_vf(c), get_zf(c));
            let o = c.bitwise_or(vf, zf);
            c.bitwise_exclusive_or(nn, o)
        }
        Ccc::NEU => {
            let nf = get_nf(c);
            let zf = get_zf(c);
            let nz = c.bitwise_not(zf);
            c.bitwise_or(nf, nz)
        }
        Ccc::GEU => {
            let nf = get_nf(c);
            let nn = c.bitwise_not(nf);
            let zf = get_zf(c);
            let o = c.bitwise_or(nn, zf);
            let vf = get_vf(c);
            c.bitwise_exclusive_or(o, vf)
        }
        Ccc::T => c.const_i(ir_consts::TRUE),
        Ccc::OFF => {
            let vf = get_vf(c);
            c.bitwise_not(vf)
        }
        Ccc::LO => {
            let cf = get_cf(c);
            c.bitwise_not(cf)
        }
        Ccc::SFF => {
            let nf = get_nf(c);
            c.bitwise_not(nf)
        }
        Ccc::LS => {
            let zf = get_zf(c);
            let cf = get_cf(c);
            let nc = c.bitwise_not(cf);
            c.bitwise_or(zf, nc)
        }
        Ccc::HI => {
            let cf = get_cf(c);
            let zf = get_zf(c);
            let nz = c.bitwise_not(zf);
            c.bitwise_and(cf, nz)
        }
        Ccc::SFT => get_nf(c),
        Ccc::HS => get_cf(c),
        Ccc::OFT => get_vf(c),
        Ccc::RLE => {
            let (nf, zf) = (get_nf(c), get_zf(c));
            c.bitwise_or(nf, zf)
        }
        Ccc::RGT => {
            let (nf, zf) = (get_nf(c), get_zf(c));
            let o = c.bitwise_or(nf, zf);
            c.bitwise_not(o)
        }
        _ => c.const_i(default_cond),
    }
}

// InstEmitBarrier.

pub fn bar(c: &mut Ctx) {
    let op = InstBar(c.raw_op());
    // TODO in the original: the other modes.
    if op.bar_op() == BarOp::SYNC {
        c.barrier();
    } else {
        c.log(&format!("Invalid barrier mode: {}.", op.bar_op().0));
    }
}

pub fn depbar(_c: &mut Ctx) {}

pub fn membar(c: &mut Ctx) {
    let op = InstMembar(c.raw_op());
    if op.membar() == Membar::CTA {
        c.group_memory_barrier();
    } else {
        c.memory_barrier();
    }
}
