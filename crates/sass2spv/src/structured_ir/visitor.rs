use super::ast::{AstArena, AstBlockType, AstNodeKind, NodeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstVisitEvent {
    BlockEntered(NodeId),
    BlockLeft(NodeId),
    Node(NodeId),
}

pub struct AstBlockVisitor {
    pub main_block: NodeId,
}

impl AstBlockVisitor {
    pub fn new(main_block: NodeId) -> Self {
        Self { main_block }
    }

    pub fn traverse<F>(&self, arena: &AstArena, mut callback: F)
    where
        F: FnMut(AstVisitEvent),
    {
        let mut curr_block = self.main_block;
        let mut node = arena.get_block(curr_block).first;

        while node.is_some() || curr_block != self.main_block {
            // Traverse into child blocks
            while let Some(curr_node) = node {
                if let AstNodeKind::Block(_) = &arena.nodes[curr_node.0].kind {
                    curr_block = curr_node;
                    callback(AstVisitEvent::BlockEntered(curr_block));
                    node = arena.get_block(curr_block).first;
                } else {
                    break;
                }
            }

            if let Some(curr_node) = node {
                let next = arena.next(curr_node);
                callback(AstVisitEvent::Node(curr_node));
                node = next;
            }

            // Ascend to parent when end of block reached
            while node.is_none() {
                let block = arena.get_block(curr_block);
                if block.block_type == AstBlockType::Main {
                    return;
                }
                callback(AstVisitEvent::BlockLeft(curr_block));
                node = arena.next(curr_block);
                curr_block = arena.nodes[curr_block.0].parent.expect("Block must have parent");
            }
        }
    }
}
