// Expression propagation and dead assignment removal on the structured form, then empty block removal (AstOptimizer,
// with the translator patches: call arguments, subgroup operations and atomics stay where they are).

use std::collections::VecDeque;

use super::instruction_info::is_unary;
use super::{Ast, AstBlockType, AstBlockVisitor, AstId, AstKind};
use crate::collections::NetSet;
use crate::ir::{Instruction as I, OperandType};

type R<T> = Result<T, String>;

pub fn optimize(ast: &mut Ast, main_block: AstId, locals: &mut NetSet<AstId>) -> R<()> {
    let mut visitor = AstBlockVisitor::new(main_block);
    while let Some(node) = visitor.next(ast) {
        let Some((prop_var, source)) = ast.assignment(node) else { continue };
        let Some(pv) = ast.as_operand(prop_var) else { continue };
        let is_worth_propagating = pv.uses.len() == 1 || is_worth_propagating(ast, source)?;
        // SPIR-V call arguments must stay local variables, so a temporary feeding a call keeps its value.
        let feeds_call = pv.uses.iter().any(|&u| ast.as_operation(u).is_some_and(|o| o.inst.base() == I::CALL));
        // Subgroup operations depend on which invocations are active where they run: they are not moved into branches.
        // Atomics and calls stay too: their assignment is kept for the side effect, so a copy at the use would run them twice.
        if pv.defs.len() == 1 && is_worth_propagating && !feeds_call && !contains_subgroup_op(ast, source) && !has_side_effects(ast, source) {
            propagate_expression(ast, prop_var, source);
        }
        // An atomic with an unused result (a reduction) keeps its side effect.
        let source = ast.assignment(node).unwrap().1;
        let pv = ast.opnd(prop_var);
        if pv.ty == OperandType::LocalVariable && pv.uses.is_empty() && !has_side_effects(ast, source) {
            ast.remove(visitor.block, node)?;
            locals.remove(&prop_var);
        }
    }
    remove_empty_blocks(ast, main_block)
}

fn has_side_effects(ast: &Ast, source: Option<AstId>) -> bool {
    let Some(op) = source.and_then(|s| ast.as_operation(s)) else { return false };
    let inst = op.inst.base();
    inst.is_atomic() || inst == I::IMAGE_ATOMIC || inst == I::CALL
}

fn contains_subgroup_op(ast: &Ast, node: Option<AstId>) -> bool {
    let Some(op) = node.and_then(|n| ast.as_operation(n)) else { return false };
    if matches!(
        op.inst.base(),
        I::BALLOT | I::SHUFFLE | I::SHUFFLE_DOWN | I::SHUFFLE_UP | I::SHUFFLE_XOR | I::VOTE_ALL | I::VOTE_ALL_EQUAL | I::VOTE_ANY
    ) {
        return true;
    }
    op.sources.iter().any(|&s| contains_subgroup_op(ast, s))
}

fn is_worth_propagating(ast: &Ast, source: Option<AstId>) -> R<bool> {
    let Some(op) = source.and_then(|s| ast.as_operation(s)) else { return Ok(false) };
    if !is_unary(op.inst)? {
        return Ok(false);
    }
    let s0 = *op.sources.first().ok_or("System.IndexOutOfRangeException: Index was outside the bounds of the array.")?;
    Ok(s0.is_some_and(|s| ast.as_operand(s).is_some()) || op.inst == I::COPY)
}

fn propagate_expression(ast: &mut Ast, prop_var: AstId, source: Option<AstId>) {
    let uses = ast.opnd(prop_var).uses.to_vec();
    for use_node in uses {
        match ast.kind(use_node) {
            AstKind::Block(_) => ast.set_condition(use_node, source),
            AstKind::Operation(op) => {
                let count = op.sources.len();
                for i in 0..count {
                    if ast.op(use_node).sources[i] == Some(prop_var) {
                        ast.set_source(use_node, i, source);
                    }
                }
            }
            AstKind::Assignment { .. } => ast.set_assignment_source(use_node, source),
            _ => {}
        }
    }
}

fn remove_empty_blocks(ast: &mut Ast, main_block: AstId) -> R<()> {
    let mut pending = VecDeque::from([main_block]);
    while let Some(block) = pending.pop_front() {
        for n in ast.children(block) {
            if ast.is_block(n) {
                pending.push_back(n);
            }
        }
        let Some(parent) = ast.parent(block) else { continue };
        let next_block = ast.next(block)?.filter(|&n| ast.is_block(n));
        let has_else = next_block.is_some_and(|b| ast.block_type(b) == AstBlockType::Else);
        let is_if = ast.block_type(block) == AstBlockType::If;
        if ast.count(block) == 0 {
            if is_if {
                if has_else {
                    let c = ast.condition(block);
                    let inv = ast.inverse_cond(c);
                    ast.turn_into_if(next_block.unwrap(), Some(inv));
                }
                ast.remove(parent, block)?;
            } else if ast.block_type(block) == AstBlockType::Else {
                ast.remove(parent, block)?;
            }
        }
        // else { if (c) ... else ... } is not turned into an else-if chain: the SPIR-V generator has no else-if
        // support, and the nested form is equivalent.
    }
    Ok(())
}
