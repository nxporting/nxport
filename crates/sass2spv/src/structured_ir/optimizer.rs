use super::ast::{AstArena, AstBlockType, AstNodeKind, NodeId};
use super::instruction_info::InstructionInfo;
use super::visitor::{AstBlockVisitor, AstVisitEvent};
use crate::ir::instruction::Instruction;
use crate::ir::operand::OperandType;
use std::collections::VecDeque;

pub struct AstOptimizer;

impl AstOptimizer {
    pub fn optimize(arena: &mut AstArena, main_block: NodeId, debug_mode: bool) {
        if !debug_mode {
            let visitor = AstBlockVisitor::new(main_block);
            let mut assignments_to_check = Vec::new();

            visitor.traverse(arena, |event| {
                if let AstVisitEvent::Node(node_id) = event {
                    if let AstNodeKind::Assignment(asg) = &arena.nodes[node_id.0].kind {
                        let dest = asg.destination;
                        let src = asg.source;
                        assignments_to_check.push((node_id, dest, src));
                    }
                }
            });

            for (asg_id, dest_id, src_id) in assignments_to_check {
                let (is_single_def, is_worth_propagating, is_local, uses_count) = {
                    if let AstNodeKind::Operand(oper) = &arena.nodes[dest_id.0].kind {
                        let single_def = oper.defs.len() == 1;
                        let worth = oper.uses.len() == 1 || Self::is_worth_propagating(arena, src_id);
                        let local = oper.op_type == OperandType::LocalVariable;
                        (single_def, worth, local, oper.uses.len())
                    } else {
                        (false, false, false, 0)
                    }
                };

                if is_single_def && is_worth_propagating {
                    Self::propagate_expression(arena, dest_id, src_id);
                }

                if is_local && uses_count == 0 {
                    arena.remove(asg_id);
                }
            }
        }

        Self::remove_empty_blocks(arena, main_block);
    }

    fn is_worth_propagating(arena: &AstArena, source: NodeId) -> bool {
        if let AstNodeKind::Operation(op) = &arena.nodes[source.0].kind {
            if !InstructionInfo::is_unary(op.inst) {
                return false;
            }
            if let Some(&first_src) = op.sources.first() {
                matches!(&arena.nodes[first_src.0].kind, AstNodeKind::Operand(_))
                    || op.inst == Instruction::Copy
            } else {
                false
            }
        } else {
            false
        }
    }

    fn propagate_expression(arena: &mut AstArena, prop_var: NodeId, source: NodeId) {
        let uses: Vec<NodeId> = if let AstNodeKind::Operand(oper) = &arena.nodes[prop_var.0].kind {
            oper.uses.iter().copied().collect()
        } else {
            return;
        };

        for use_node in uses {
            let mut replaced = false;
            match &mut arena.nodes[use_node.0].kind {
                AstNodeKind::Block(b) => {
                    if b.condition == Some(prop_var) {
                        b.condition = Some(source);
                        replaced = true;
                    }
                }
                AstNodeKind::Operation(op) => {
                    for src in &mut op.sources {
                        if *src == prop_var {
                            *src = source;
                            replaced = true;
                        }
                    }
                }
                AstNodeKind::Assignment(asg) => {
                    if asg.source == prop_var {
                        asg.source = source;
                        replaced = true;
                    }
                }
                _ => {}
            }
            if replaced {
                arena.remove_use(prop_var, use_node);
                arena.add_use(source, use_node);
            }
        }
    }

    fn remove_empty_blocks(arena: &mut AstArena, main_block: NodeId) {
        let mut queue = VecDeque::new();
        queue.push_back(main_block);

        let mut blocks_to_remove = Vec::new();

        while let Some(block_id) = queue.pop_front() {
            let mut curr = arena.get_block(block_id).first;
            while let Some(child_id) = curr {
                let next = arena.next(child_id);
                if let AstNodeKind::Block(b) = &arena.nodes[child_id.0].kind {
                    if b.count == 0 && b.block_type != AstBlockType::Main {
                        blocks_to_remove.push(child_id);
                    } else {
                        queue.push_back(child_id);
                    }
                }
                curr = next;
            }
        }

        for block_id in blocks_to_remove {
            arena.remove(block_id);
        }
    }
}
