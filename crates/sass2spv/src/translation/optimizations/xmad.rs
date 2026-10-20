// 32-bit multiplies the compiler split into 16-bit XMAD sequences become single multiplies again (XmadOptimizer).
// Where the original dereferences a missing operand, the stage fails (utils::NRE).

use super::utils::NRE;
use crate::ir::{BlockId, Instruction as I, Ir, NodeId, OperandId, OperandType};

type R<T> = Result<T, String>;

pub fn run_pass(ir: &mut Ir, block: BlockId) -> R<()> {
    let mut node = ir.first_node(block);
    while let Some(n) = node {
        if !ir.is_op(n) {
            node = ir.next_node(n);
            continue;
        }
        if let Some((x, y, addend)) = try_match_xmad_pattern(ir, n)? {
            let dest = ir.dest(n);
            let next_node = match addend {
                Some(addend) => {
                    let temp = ir.local();
                    let mul = ir.operation(I::MULTIPLY, Some(temp), &[x, y]);
                    let after = ir.add_after(n, mul);
                    let add = ir.operation(I::ADD, dest, &[temp, addend]);
                    ir.add_after(after, add)
                }
                None => {
                    let mul = ir.operation(I::MULTIPLY, dest, &[x, y]);
                    ir.add_after(n, mul)
                }
            };
            super::remove_node(ir, n);
            node = ir.next_node(next_node);
            continue;
        }
        node = ir.next_node(n);
    }
    Ok(())
}

type Match = Option<(OperandId, OperandId, Option<OperandId>)>;

fn try_match_xmad_pattern(ir: &Ir, n: NodeId) -> R<Match> {
    if let Some(m) = try_match_xmad_32x32(ir, n)? {
        return Ok(Some(m));
    }
    try_match_xmad_32x16(ir, n)
}

fn op_of(ir: &Ir, o: OperandId) -> Option<NodeId> {
    ir.asg_op(o).filter(|&a| ir.is_op(a))
}

fn try_match_xmad_32x32(ir: &Ir, n: NodeId) -> R<Match> {
    if ir.inst(n) != I::ADD {
        return Ok(None);
    }
    let (src1, src2) = (ir.src(n, 0), ir.src(n, 1));
    let Some(add_op) = op_of(ir, src2).filter(|&a| ir.inst(a) == I::ADD) else { return Ok(None) };
    let low_times_low_result = copy_source(ir, Some(ir.src(add_op, 0)))?;
    let Some(low_times_low_op) = op_of(ir, low_times_low_result) else { return Ok(None) };
    let Some((x, y, addend)) = try_match_low_times_low(ir, low_times_low_op)? else { return Ok(None) };
    let low_times_high_result = copy_source(ir, shifted16_source(ir, ir.src(add_op, 1), I::SHIFT_LEFT))?;
    let Some(low_times_high_op) = op_of(ir, low_times_high_result) else { return Ok(None) };
    if !try_match_low_times_high(ir, low_times_high_op, x, y)? {
        return Ok(None);
    }
    let Some(high_times_high_op) = op_of(ir, src1) else { return Ok(None) };
    if !try_match_high_times_high(ir, high_times_high_op, x, low_times_high_result)? {
        return Ok(None);
    }
    Ok(Some((x, y, addend)))
}

fn try_match_xmad_32x16(ir: &Ir, n: NodeId) -> R<Match> {
    if ir.inst(n) != I::ADD {
        return Ok(None);
    }
    let (src1, src2) = (ir.src(n, 0), ir.src(n, 1));
    let low_times_low_result = copy_source(ir, Some(src2))?;
    let Some(low_times_low_op) = op_of(ir, low_times_low_result) else { return Ok(None) };
    let Some((x, y, addend)) = try_match_low_times_low(ir, low_times_low_op)? else { return Ok(None) };
    let Some(high_times_low_op) = op_of(ir, src1) else { return Ok(None) };
    if !try_match_high_times_low(ir, high_times_low_op, x, y)? {
        return Ok(None);
    }
    if ir.ty(y) == OperandType::Constant && (ir.value(y) as u16) as i32 == ir.value(y) {
        Ok(Some((x, y, addend)))
    } else {
        Ok(None)
    }
}

/// x & 0xFFFF times y & 0xFFFF, optionally plus an addend.
fn try_match_low_times_low(ir: &Ir, mut op: NodeId) -> R<Match> {
    let mut addend = None;
    if ir.inst(op) == I::ADD {
        let Some(mul_op) = op_of(ir, ir.src(op, 0)) else { return Ok(None) };
        addend = Some(ir.src(op, 1));
        op = mul_op;
    }
    if ir.inst(op) != I::MULTIPLY {
        return Ok(None);
    }
    let (Some(s1), Some(s2)) = (masked16_source(ir, ir.src(op, 0)), masked16_source(ir, ir.src(op, 1))) else { return Ok(None) };
    Ok(Some((s1, s2, addend)))
}

/// (x & 0xFFFF) * (y >> 16), masked, or'ed with y << 16.
fn try_match_low_times_high(ir: &Ir, op: NodeId, x: OperandId, y: OperandId) -> R<bool> {
    if ir.inst(op) != I::BITWISE_OR {
        return Ok(false);
    }
    let Some(mul_result) = masked16_source(ir, ir.src(op, 0)) else { return Ok(false) };
    let mul_result = copy_source(ir, Some(mul_result))?;
    let Some(mul_op) = op_of(ir, mul_result).filter(|&m| ir.inst(m) == I::MULTIPLY) else { return Ok(false) };
    if masked16_source(ir, ir.src(mul_op, 0)) != Some(x) {
        return Ok(false);
    }
    if shifted16_source(ir, ir.src(mul_op, 1), I::SHIFT_RIGHT_U32) != Some(y) {
        return Ok(false);
    }
    Ok(shifted16_source(ir, ir.src(op, 1), I::SHIFT_LEFT) == Some(y))
}

/// ((x >> 16) * (y & 0xFFFF)) << 16.
fn try_match_high_times_low(ir: &Ir, op: NodeId, x: OperandId, y: OperandId) -> R<bool> {
    if ir.inst(op) != I::SHIFT_LEFT || !is_const(ir, ir.src(op, 1), 16) {
        return Ok(false);
    }
    let Some(mul_op) = op_of(ir, ir.src(op, 0)).filter(|&m| ir.inst(m) == I::MULTIPLY) else { return Ok(false) };
    if shifted16_source(ir, ir.src(mul_op, 0), I::SHIFT_RIGHT_U32) != Some(x) {
        return Ok(false);
    }
    let src2 = masked16_source(ir, ir.src(mul_op, 1)).ok_or(NRE)?;
    Ok(ir.ty(src2) == ir.ty(y) && ir.value(src2) == ir.value(y))
}

/// ((x >> 16) * (lowTimesHigh >> 16)) << 16.
fn try_match_high_times_high(ir: &Ir, op: NodeId, x: OperandId, low_times_high_result: OperandId) -> R<bool> {
    if ir.inst(op) != I::SHIFT_LEFT || !is_const(ir, ir.src(op, 1), 16) {
        return Ok(false);
    }
    let Some(mul_op) = op_of(ir, ir.src(op, 0)).filter(|&m| ir.inst(m) == I::MULTIPLY) else { return Ok(false) };
    if shifted16_source(ir, ir.src(mul_op, 0), I::SHIFT_RIGHT_U32) != Some(x) {
        return Ok(false);
    }
    Ok(copy_source(ir, shifted16_source(ir, ir.src(mul_op, 1), I::SHIFT_RIGHT_U32))? == low_times_high_result)
}

fn masked16_source(ir: &Ir, value: OperandId) -> Option<OperandId> {
    let mask_op = op_of(ir, value)?;
    if ir.inst(mask_op) != I::BITWISE_AND || !is_const(ir, ir.src(mask_op, 1), u16::MAX as i32) {
        return None;
    }
    Some(ir.src(mask_op, 0))
}

fn shifted16_source(ir: &Ir, value: OperandId, shift_inst: I) -> Option<OperandId> {
    let shift_op = op_of(ir, value)?;
    if ir.inst(shift_op) != shift_inst || !is_const(ir, ir.src(shift_op, 1), 16) {
        return None;
    }
    Some(ir.src(shift_op, 0))
}

/// Follows copies (and additions of zero); a missing value is the original's null dereference.
fn copy_source(ir: &Ir, value: Option<OperandId>) -> R<OperandId> {
    let mut value = value.ok_or(NRE)?;
    while let Some(op) = op_of(ir, value) {
        if !is_copy(ir, op) {
            break;
        }
        value = ir.src(op, 0);
    }
    Ok(value)
}

fn is_copy(ir: &Ir, op: NodeId) -> bool {
    ir.inst(op) == I::COPY || (ir.inst(op) == I::ADD && is_const(ir, ir.src(op, 1), 0))
}

fn is_const(ir: &Ir, o: OperandId, value: i32) -> bool {
    ir.ty(o) == OperandType::Constant && ir.value(o) == value
}
