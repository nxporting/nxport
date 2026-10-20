// Pattern helpers shared by the optimisation passes (Optimizations.Utils).

use crate::ir::{BlockId, Instruction, Ir, IoVariable, NodeId, OperandId, OperandType, StorageKind};

/// "Object reference not set to an instance of an object": where the original dereferences null, the stage fails.
pub const NRE: &str = "System.NullReferenceException: Object reference not set to an instance of an object.";

pub fn is_input_load(ir: &Ir, node: Option<NodeId>) -> bool {
    node.and_then(|n| ir.as_op(n)).is_some_and(|o| o.inst == Instruction::LOAD && o.storage_kind == StorageKind::Input)
}

pub fn is_input_load_of(ir: &Ir, node: Option<NodeId>, io: IoVariable, elem_index: i32) -> bool {
    let Some(o) = node.and_then(|n| ir.as_op(n)) else { return false };
    if o.inst != Instruction::LOAD || o.storage_kind != StorageKind::Input || o.sources.len() != 2 {
        return false;
    }
    let io_src = o.sources[0].unwrap();
    if ir.ty(io_src) != OperandType::Constant || IoVariable::from_value(ir.value(io_src)) != io {
        return false;
    }
    let elem = o.sources[1].unwrap();
    ir.ty(elem) == OperandType::Constant && ir.value(elem) == elem_index
}

fn is_same_operand(ir: &Ir, x: OperandId, y: OperandId) -> bool {
    if ir.ty(x) != ir.ty(y) || ir.value(x) != ir.value(y) {
        return false;
    }
    // Loads with the same storage and constant parameters are not recognised.
    x == y || matches!(ir.ty(x), OperandType::Constant | OperandType::ConstantBuffer)
}

fn are_all_sources_equal(ir: &Ir, a: NodeId, b: NodeId) -> bool {
    let (n, m) = (ir.sources_count(a), ir.sources_count(b));
    n == m && (0..n).all(|i| is_same_operand(ir, ir.src(a, i), ir.src(b, i)))
}

pub fn are_all_sources_the_same_operand(ir: &Ir, node: NodeId) -> bool {
    let first = ir.src(node, 0);
    (1..ir.sources_count(node)).all(|i| is_same_operand(ir, first, ir.src(node, i)))
}

fn find_branch_source(ir: &Ir, block: BlockId) -> Option<NodeId> {
    for &source_block in &ir.block(block).predecessors {
        if ir.node_count(source_block) > 0 {
            if let Some(last) = ir.last_node(source_block) {
                if ir.inst_of(last).is_some_and(is_conditional_branch) && ir.next(source_block) == Some(block) {
                    return Some(last);
                }
            }
        }
    }
    None
}

fn is_conditional_branch(inst: Instruction) -> bool {
    matches!(inst, Instruction::BRANCH_IF_FALSE | Instruction::BRANCH_IF_TRUE)
}

fn is_same_condition(ir: &Ir, current: OperandId, query: OperandId) -> bool {
    if current == query {
        return true;
    }
    match (ir.asg_op(current).filter(|&n| ir.is_op(n)), ir.asg_op(query).filter(|&n| ir.is_op(n))) {
        (Some(c), Some(q)) => ir.inst(c) == ir.inst(q) && are_all_sources_equal(ir, c, q),
        _ => false,
    }
}

fn block_conditions_match(ir: &Ir, current_block: BlockId, query_block: BlockId) -> bool {
    // Only the top-most conditional is checked.
    let (Some(cb), Some(qb)) = (find_branch_source(ir, current_block), find_branch_source(ir, query_block)) else { return false };
    ir.inst(cb) == ir.inst(qb) && is_same_condition(ir, ir.src(cb, 0), ir.src(qb, 0))
}

/// The value a source most likely holds in `block`, looking through phis whose branch condition matches.
pub fn find_last_operation(ir: &Ir, source: OperandId, block: BlockId, recurse: bool) -> OperandId {
    if let Some(phi) = ir.asg_op(source).filter(|&n| ir.is_phi(n)) {
        // Prefer the latest sources; the branch that leads here must hold for the current block too.
        let count = ir.sources_count(phi);
        let mut undef_count = 0;
        for i in (0..count).rev() {
            let phi_block = ir.phi_block(phi, i);
            let phi_source = ir.src(phi, i);
            if block_conditions_match(ir, block, phi_block) {
                return phi_source;
            } else if recurse && ir.asg_op(phi_source).is_some_and(|n| ir.is_phi(n)) {
                // A phi of phis: one of its blocks may match.
                let m = find_last_operation(ir, phi_source, block, false);
                if m != phi_source {
                    return m;
                }
            } else if ir.ty(phi_source) == OperandType::Undefined {
                undef_count += 1;
            }
        }
        // All sources but one undefined: that one.
        if undef_count == count as i32 - 1 {
            for i in (0..count).rev() {
                let s = ir.src(phi, i);
                if ir.ty(s) != OperandType::Undefined {
                    return s;
                }
            }
        }
    }
    source
}

/// Utils.DeleteNode: out of its list, sources cleared, no destination.
pub fn delete_node(ir: &mut Ir, node: NodeId) {
    ir.remove_node(node);
    for i in 0..ir.sources_count(node) {
        ir.set_source(node, i, None);
    }
    ir.set_dest_single(node, None);
}
