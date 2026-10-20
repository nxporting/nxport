// Algebraic identities: x + 0, x & ~0, x | 0, selects on constants and similar (Simplification).

use crate::ir::{Instruction as I, Ir, NodeId, OperandId, OperandType};

const ALL_ONES: i32 = !0;

fn is_const_equal(ir: &Ir, o: OperandId, comparand: i32) -> bool {
    ir.ty(o) == OperandType::Constant && ir.value(o) == comparand
}

pub fn run_pass(ir: &mut Ir, n: NodeId) {
    match ir.inst(n) {
        I::ADD => try_eliminate_binary_op_commutative(ir, n, 0),
        I::BITWISE_AND => try_eliminate_bitwise_and(ir, n),
        I::BITWISE_EXCLUSIVE_OR => {
            if !try_eliminate_xor_swap(ir, n) {
                try_eliminate_binary_op_commutative(ir, n, 0);
            }
        }
        I::BITWISE_OR => try_eliminate_bitwise_or(ir, n),
        I::COMPARE_NOT_EQUAL => try_eliminate_compare_not_equal(ir, n),
        I::CONDITIONAL_SELECT => try_eliminate_conditional_select(ir, n),
        I::DIVIDE => try_eliminate_binary_op_y(ir, n, 1),
        I::MULTIPLY => try_eliminate_binary_op_commutative(ir, n, 1),
        I::SHIFT_LEFT | I::SHIFT_RIGHT_S32 | I::SHIFT_RIGHT_U32 | I::SUBTRACT => try_eliminate_binary_op_y(ir, n, 0),
        _ => {}
    }
}

fn try_eliminate_bitwise_and(ir: &mut Ir, n: NodeId) {
    // x & ~0 == x, ~0 & y == y, x & 0 == 0 & y == 0 (in that order).
    let (x, y) = (ir.src(n, 0), ir.src(n, 1));
    if is_const_equal(ir, x, ALL_ONES) {
        ir.turn_into_copy(n, y);
    } else if is_const_equal(ir, y, ALL_ONES) {
        ir.turn_into_copy(n, x);
    } else if is_const_equal(ir, x, 0) || is_const_equal(ir, y, 0) {
        let z = ir.const_i(0);
        ir.turn_into_copy(n, z);
    }
}

/// x2 = x ^ y; y2 = x2 ^ y; x3 = x2 ^ y2 becomes temp = y; y = x; x = temp (one of the commutative forms).
fn try_eliminate_xor_swap(ir: &mut Ir, x_copy_op: NodeId) -> bool {
    let (x, y) = (ir.src(x_copy_op, 0), ir.src(x_copy_op, 1));
    let t_copy_op = ir.asg_op(x).filter(|&a| ir.inst_of(a) == Some(I::BITWISE_EXCLUSIVE_OR));
    let y_copy_op = ir.asg_op(y).filter(|&a| ir.inst_of(a) == Some(I::BITWISE_EXCLUSIVE_OR));
    let (Some(t_copy_op), Some(y_copy_op)) = (t_copy_op, y_copy_op) else { return false };
    if t_copy_op == y_copy_op {
        return false;
    }
    if ir.src(y_copy_op, 0) != x || ir.src(y_copy_op, 1) != ir.src(t_copy_op, 1) || ir.use_ops(x).len() != 2 {
        return false;
    }
    let (x, y) = (ir.src(t_copy_op, 0), ir.src(t_copy_op, 1));
    ir.turn_into_copy(t_copy_op, y);
    ir.turn_into_copy(y_copy_op, x);
    let t = ir.dest(t_copy_op).unwrap();
    ir.turn_into_copy(x_copy_op, t);
    true
}

fn try_eliminate_bitwise_or(ir: &mut Ir, n: NodeId) {
    // x | 0 == x, 0 | y == y, x | ~0 == ~0 | y == ~0 (in that order).
    let (x, y) = (ir.src(n, 0), ir.src(n, 1));
    if is_const_equal(ir, x, 0) {
        ir.turn_into_copy(n, y);
    } else if is_const_equal(ir, y, 0) {
        ir.turn_into_copy(n, x);
    } else if is_const_equal(ir, x, ALL_ONES) || is_const_equal(ir, y, ALL_ONES) {
        let c = ir.const_i(ALL_ONES);
        ir.turn_into_copy(n, c);
    }
}

fn try_eliminate_binary_op_y(ir: &mut Ir, n: NodeId, comparand: i32) {
    let (x, y) = (ir.src(n, 0), ir.src(n, 1));
    if is_const_equal(ir, y, comparand) {
        ir.turn_into_copy(n, x);
    }
}

fn try_eliminate_binary_op_commutative(ir: &mut Ir, n: NodeId, comparand: i32) {
    let (x, y) = (ir.src(n, 0), ir.src(n, 1));
    if is_const_equal(ir, x, comparand) {
        ir.turn_into_copy(n, y);
    } else if is_const_equal(ir, y, comparand) {
        ir.turn_into_copy(n, x);
    }
}

/// Comparisons give 0 or -1, so comparing one's result with zero for inequality is the result itself.
fn try_eliminate_compare_not_equal(ir: &mut Ir, n: NodeId) {
    let (mut lhs, mut rhs) = (ir.src(n, 0), ir.src(n, 1));
    if ir.ty(lhs) == OperandType::Constant {
        std::mem::swap(&mut lhs, &mut rhs);
    }
    if ir.ty(rhs) != OperandType::Constant || ir.value(rhs) != 0 {
        return;
    }
    if !ir.asg_op(lhs).and_then(|a| ir.inst_of(a)).is_some_and(|i| i.is_comparison()) {
        return;
    }
    ir.turn_into_copy(n, lhs);
}

fn try_eliminate_conditional_select(ir: &mut Ir, n: NodeId) {
    let cond = ir.src(n, 0);
    if ir.ty(cond) != OperandType::Constant {
        return;
    }
    let src_index = if ir.value(cond) != 0 { 1 } else { 2 };
    let source = ir.src(n, src_index);
    ir.turn_into_copy(n, source);
}
