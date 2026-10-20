// Basic blocks of a function's operations (ControlFlowGraph) and dominators (Dominance).

use std::collections::{HashMap, HashSet};

use crate::ir::{Instruction, Ir, NodeId, OperandId};

pub struct ControlFlowGraph {
    pub blocks: Vec<crate::ir::BlockId>,
    pub post_order_blocks: Vec<crate::ir::BlockId>,
    pub post_order_map: Vec<i32>,
}

impl ControlFlowGraph {
    fn new(ir: &Ir, blocks: Vec<crate::ir::BlockId>) -> ControlFlowGraph {
        let mut visited = HashSet::new();
        let mut stack = Vec::new();
        let mut post_order_blocks = Vec::with_capacity(blocks.len());
        let mut post_order_map = vec![0; blocks.len()];
        if let Some(&first) = blocks.first() {
            visited.insert(first);
            stack.push(first);
        }
        while let Some(block) = stack.pop() {
            match (ir.next(block), ir.branch(block)) {
                (Some(next), _) if visited.insert(next) => {
                    stack.push(block);
                    stack.push(next);
                }
                (_, Some(branch)) if visited.insert(branch) => {
                    stack.push(block);
                    stack.push(branch);
                }
                _ => {
                    post_order_map[ir.block(block).index as usize] = post_order_blocks.len() as i32;
                    post_order_blocks.push(block);
                }
            }
        }
        ControlFlowGraph { blocks, post_order_blocks, post_order_map }
    }

    pub fn create(ir: &mut Ir, operations: &[NodeId]) -> ControlFlowGraph {
        let mut labels: HashMap<OperandId, crate::ir::BlockId> = HashMap::new();
        let mut blocks: Vec<crate::ir::BlockId> = Vec::new();
        let mut current: Option<crate::ir::BlockId> = None;

        fn next_block(ir: &mut Ir, current: &mut Option<crate::ir::BlockId>, next: crate::ir::BlockId) {
            if let Some(c) = *current {
                if !ends_with_unconditional_inst(ir, ir.last_node(c)) {
                    ir.set_next(c, Some(next));
                }
            }
            *current = Some(next);
        }

        let mut needs_new_block = true;
        for &operation in operations {
            let inst = ir.inst(operation);
            if inst == Instruction::MARK_LABEL {
                let label = ir.dest(operation).expect("label");
                if let Some(&next) = labels.get(&label) {
                    ir.block_mut(next).index = blocks.len() as i32;
                    blocks.push(next);
                    next_block(ir, &mut current, next);
                } else {
                    let b = ir.new_block(blocks.len() as i32);
                    blocks.push(b);
                    next_block(ir, &mut current, b);
                    labels.insert(label, b);
                }
            } else {
                if needs_new_block {
                    let b = ir.new_block(blocks.len() as i32);
                    blocks.push(b);
                    next_block(ir, &mut current, b);
                }
                ir.add_last(current.unwrap(), operation);
            }
            needs_new_block = matches!(inst, Instruction::BRANCH | Instruction::BRANCH_IF_TRUE | Instruction::BRANCH_IF_FALSE);
            if needs_new_block {
                let label = ir.dest(operation).expect("branch target");
                let branch_block = match labels.get(&label) {
                    Some(&b) => b,
                    None => {
                        let b = ir.new_block(0);
                        labels.insert(label, b);
                        b
                    }
                };
                ir.set_branch(current.unwrap(), Some(branch_block));
            }
        }

        // Remove unreachable blocks.
        loop {
            let mut has_unreachable = false;
            let mut blk_index = 1;
            while blk_index < blocks.len() {
                let block = blocks[blk_index];
                if ir.block(block).predecessors.is_empty() {
                    ir.set_next(block, None);
                    ir.set_branch(block, None);
                    blocks.remove(blk_index);
                    has_unreachable = true;
                } else {
                    ir.block_mut(block).index = blk_index as i32;
                    blk_index += 1;
                }
            }
            if !has_unreachable {
                break;
            }
        }
        ControlFlowGraph::new(ir, blocks)
    }
}

fn ends_with_unconditional_inst(ir: &Ir, node: Option<NodeId>) -> bool {
    node.and_then(|n| ir.inst_of(n)).is_some_and(|i| matches!(i, Instruction::BRANCH | Instruction::DISCARD | Instruction::RETURN))
}

/// "A Simple, Fast Dominance Algorithm" (Cooper, Harvey, Kennedy).
pub fn find_dominators(ir: &mut Ir, cfg: &ControlFlowGraph) {
    let intersect = |ir: &Ir, mut b1: crate::ir::BlockId, mut b2: crate::ir::BlockId| {
        let po = |ir: &Ir, b: crate::ir::BlockId| cfg.post_order_map[ir.block(b).index as usize];
        while b1 != b2 {
            while po(ir, b1) < po(ir, b2) {
                b1 = ir.block(b1).immediate_dominator.unwrap();
            }
            while po(ir, b2) < po(ir, b1) {
                b2 = ir.block(b2).immediate_dominator.unwrap();
            }
        }
        b1
    };
    let entry = cfg.blocks[0];
    ir.block_mut(entry).immediate_dominator = Some(entry);
    loop {
        let mut modified = false;
        for blk_index in (0..cfg.post_order_blocks.len().saturating_sub(1)).rev() {
            let block = cfg.post_order_blocks[blk_index];
            let mut new_idom = None;
            for i in 0..ir.block(block).predecessors.len() {
                let predecessor = ir.block(block).predecessors[i];
                if ir.block(predecessor).immediate_dominator.is_some() {
                    new_idom = Some(match new_idom {
                        Some(n) => intersect(ir, predecessor, n),
                        None => predecessor,
                    });
                }
            }
            if ir.block(block).immediate_dominator != new_idom {
                ir.block_mut(block).immediate_dominator = new_idom;
                modified = true;
            }
        }
        if !modified {
            break;
        }
    }
}

pub fn find_dominance_frontiers(ir: &mut Ir, blocks: &[crate::ir::BlockId]) {
    for &block in blocks {
        if ir.block(block).predecessors.len() < 2 {
            continue;
        }
        for p in 0..ir.block(block).predecessors.len() {
            let mut current = Some(ir.block(block).predecessors[p]);
            while current != ir.block(block).immediate_dominator {
                let c = current.unwrap();
                ir.block_mut(c).dominance_frontiers.insert(block);
                current = ir.block(c).immediate_dominator;
            }
        }
    }
}
