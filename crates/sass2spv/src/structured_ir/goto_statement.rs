use super::ast::{AstArena, AstNodeKind, NodeId};
use crate::ir::instruction::Instruction;

#[derive(Debug, Clone)]
pub struct GotoStatement {
    pub goto_node: NodeId,
    pub label_node: NodeId,
    pub is_loop: bool,
}

impl GotoStatement {
    pub fn new(goto_node: NodeId, label_node: NodeId, is_loop: bool) -> Self {
        Self {
            goto_node,
            label_node,
            is_loop,
        }
    }

    pub fn condition(&self, arena: &AstArena) -> NodeId {
        match &arena.nodes[self.label_node.0].kind {
            AstNodeKind::Assignment(asg) => asg.destination,
            _ => panic!("Expected Assignment node for Goto label"),
        }
    }

    pub fn is_unconditional(&self, arena: &AstArena) -> bool {
        match &arena.nodes[self.goto_node.0].kind {
            AstNodeKind::Operation(op) => op.inst == Instruction::Branch,
            _ => false,
        }
    }
}
