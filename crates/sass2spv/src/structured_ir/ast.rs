use crate::ir::instruction::Instruction;
use crate::ir::operand::{Operand, OperandType};
use crate::ir::storage_kind::{AggregateType, StorageKind};
use rustc_hash::FxHashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstBlockType {
    Main,
    If,
    Else,
    ElseIf,
    DoWhile,
}

#[derive(Debug, Clone)]
pub struct AstBlock {
    pub block_type: AstBlockType,
    pub condition: Option<NodeId>,
    pub first: Option<NodeId>,
    pub last: Option<NodeId>,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct AstOperation {
    pub inst: Instruction,
    pub storage_kind: StorageKind,
    pub force_precise: bool,
    pub index: i32,
    pub sources: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct AstAssignment {
    pub destination: NodeId,
    pub source: NodeId,
}

#[derive(Debug, Clone)]
pub struct AstOperand {
    pub op_type: OperandType,
    pub var_type: AggregateType,
    pub value: i32,
    pub defs: FxHashSet<NodeId>,
    pub uses: FxHashSet<NodeId>,
}

#[derive(Debug, Clone)]
pub enum AstNodeKind {
    Block(AstBlock),
    Operation(AstOperation),
    Assignment(AstAssignment),
    Operand(AstOperand),
    Comment(String),
}

#[derive(Debug, Clone)]
pub struct AstNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub prev_sibling: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
    pub kind: AstNodeKind,
}

#[derive(Debug, Clone, Default)]
pub struct AstArena {
    pub nodes: Vec<AstNode>,
}

impl AstArena {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn alloc(&mut self, kind: AstNodeKind) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(AstNode {
            id,
            parent: None,
            prev_sibling: None,
            next_sibling: None,
            kind,
        });
        id
    }

    pub fn new_block(&mut self, block_type: AstBlockType, condition: Option<NodeId>) -> NodeId {
        let block = AstBlock {
            block_type,
            condition,
            first: None,
            last: None,
            count: 0,
        };
        let block_id = self.alloc(AstNodeKind::Block(block));
        if let Some(cond) = condition {
            self.add_use(cond, block_id);
        }
        block_id
    }

    pub fn new_operation(&mut self, inst: Instruction, sources: Vec<NodeId>) -> NodeId {
        let op = AstOperation {
            inst,
            storage_kind: StorageKind::None,
            force_precise: false,
            index: 0,
            sources: sources.clone(),
        };
        let op_id = self.alloc(AstNodeKind::Operation(op));
        for src in sources {
            self.add_use(src, op_id);
        }
        op_id
    }

    pub fn new_operation_ext(
        &mut self,
        inst: Instruction,
        storage_kind: StorageKind,
        force_precise: bool,
        index: i32,
        sources: Vec<NodeId>,
    ) -> NodeId {
        let op = AstOperation {
            inst,
            storage_kind,
            force_precise,
            index,
            sources: sources.clone(),
        };
        let op_id = self.alloc(AstNodeKind::Operation(op));
        for src in sources {
            self.add_use(src, op_id);
        }
        op_id
    }

    pub fn new_assignment(&mut self, destination: NodeId, source: NodeId) -> NodeId {
        let asg = AstAssignment {
            destination,
            source,
        };
        let asg_id = self.alloc(AstNodeKind::Assignment(asg));
        self.add_def(destination, asg_id);
        self.add_use(source, asg_id);
        asg_id
    }

    pub fn new_operand(&mut self, op: &Operand) -> NodeId {
        let oper = AstOperand {
            op_type: op.op_type,
            var_type: AggregateType::S32,
            value: op.value,
            defs: FxHashSet::default(),
            uses: FxHashSet::default(),
        };
        self.alloc(AstNodeKind::Operand(oper))
    }

    pub fn new_temp(&mut self, var_type: AggregateType) -> NodeId {
        let oper = AstOperand {
            op_type: OperandType::LocalVariable,
            var_type,
            value: 0,
            defs: FxHashSet::default(),
            uses: FxHashSet::default(),
        };
        self.alloc(AstNodeKind::Operand(oper))
    }

    pub fn new_const(&mut self, value: i32) -> NodeId {
        let oper = AstOperand {
            op_type: OperandType::Constant,
            var_type: AggregateType::S32,
            value,
            defs: FxHashSet::default(),
            uses: FxHashSet::default(),
        };
        self.alloc(AstNodeKind::Operand(oper))
    }

    pub fn add_use(&mut self, node: NodeId, parent: NodeId) {
        if let AstNodeKind::Operand(oper) = &mut self.nodes[node.0].kind {
            if oper.op_type == OperandType::LocalVariable {
                oper.uses.insert(parent);
            }
        }
    }

    pub fn add_def(&mut self, node: NodeId, parent: NodeId) {
        if let AstNodeKind::Operand(oper) = &mut self.nodes[node.0].kind {
            if oper.op_type == OperandType::LocalVariable {
                oper.defs.insert(parent);
            }
        }
    }

    pub fn remove_use(&mut self, node: NodeId, parent: NodeId) {
        if let AstNodeKind::Operand(oper) = &mut self.nodes[node.0].kind {
            if oper.op_type == OperandType::LocalVariable {
                oper.uses.remove(&parent);
            }
        }
    }

    pub fn remove_def(&mut self, node: NodeId, parent: NodeId) {
        if let AstNodeKind::Operand(oper) = &mut self.nodes[node.0].kind {
            if oper.op_type == OperandType::LocalVariable {
                oper.defs.remove(&parent);
            }
        }
    }

    pub fn get_block(&self, id: NodeId) -> &AstBlock {
        match &self.nodes[id.0].kind {
            AstNodeKind::Block(b) => b,
            _ => panic!("Expected Block node"),
        }
    }

    pub fn get_block_mut(&mut self, id: NodeId) -> &mut AstBlock {
        match &mut self.nodes[id.0].kind {
            AstNodeKind::Block(b) => b,
            _ => panic!("Expected Block node"),
        }
    }

    pub fn add(&mut self, block_id: NodeId, node_id: NodeId) {
        self.unlink_if_needed(node_id);
        let last = self.get_block(block_id).last;
        if let Some(prev) = last {
            self.nodes[prev.0].next_sibling = Some(node_id);
            self.nodes[node_id.0].prev_sibling = Some(prev);
            self.nodes[node_id.0].next_sibling = None;
            self.get_block_mut(block_id).last = Some(node_id);
        } else {
            self.nodes[node_id.0].prev_sibling = None;
            self.nodes[node_id.0].next_sibling = None;
            let blk = self.get_block_mut(block_id);
            blk.first = Some(node_id);
            blk.last = Some(node_id);
        }
        self.nodes[node_id.0].parent = Some(block_id);
        self.get_block_mut(block_id).count += 1;
    }

    pub fn add_first(&mut self, block_id: NodeId, node_id: NodeId) {
        self.unlink_if_needed(node_id);
        let first = self.get_block(block_id).first;
        if let Some(next) = first {
            self.nodes[next.0].prev_sibling = Some(node_id);
            self.nodes[node_id.0].next_sibling = Some(next);
            self.nodes[node_id.0].prev_sibling = None;
            self.get_block_mut(block_id).first = Some(node_id);
        } else {
            self.nodes[node_id.0].prev_sibling = None;
            self.nodes[node_id.0].next_sibling = None;
            let blk = self.get_block_mut(block_id);
            blk.first = Some(node_id);
            blk.last = Some(node_id);
        }
        self.nodes[node_id.0].parent = Some(block_id);
        self.get_block_mut(block_id).count += 1;
    }

    pub fn add_before(&mut self, next_id: NodeId, node_id: NodeId) {
        self.unlink_if_needed(node_id);
        let block_id = self.nodes[next_id.0].parent.expect("Next node must have a parent block");
        let prev = self.nodes[next_id.0].prev_sibling;
        self.nodes[node_id.0].next_sibling = Some(next_id);
        self.nodes[node_id.0].prev_sibling = prev;
        self.nodes[next_id.0].prev_sibling = Some(node_id);
        if let Some(p) = prev {
            self.nodes[p.0].next_sibling = Some(node_id);
        } else {
            self.get_block_mut(block_id).first = Some(node_id);
        }
        self.nodes[node_id.0].parent = Some(block_id);
        self.get_block_mut(block_id).count += 1;
    }

    pub fn add_after(&mut self, prev_id: NodeId, node_id: NodeId) {
        self.unlink_if_needed(node_id);
        let block_id = self.nodes[prev_id.0].parent.expect("Prev node must have a parent block");
        let next = self.nodes[prev_id.0].next_sibling;
        self.nodes[node_id.0].prev_sibling = Some(prev_id);
        self.nodes[node_id.0].next_sibling = next;
        self.nodes[prev_id.0].next_sibling = Some(node_id);
        if let Some(n) = next {
            self.nodes[n.0].prev_sibling = Some(node_id);
        } else {
            self.get_block_mut(block_id).last = Some(node_id);
        }
        self.nodes[node_id.0].parent = Some(block_id);
        self.get_block_mut(block_id).count += 1;
    }

    pub fn remove(&mut self, node_id: NodeId) {
        let block_id = match self.nodes[node_id.0].parent {
            Some(p) => p,
            None => return,
        };
        let prev = self.nodes[node_id.0].prev_sibling;
        let next = self.nodes[node_id.0].next_sibling;

        if let Some(p) = prev {
            self.nodes[p.0].next_sibling = next;
        } else {
            self.get_block_mut(block_id).first = next;
        }

        if let Some(n) = next {
            self.nodes[n.0].prev_sibling = prev;
        } else {
            self.get_block_mut(block_id).last = prev;
        }

        self.nodes[node_id.0].parent = None;
        self.nodes[node_id.0].prev_sibling = None;
        self.nodes[node_id.0].next_sibling = None;
        self.get_block_mut(block_id).count = self.get_block(block_id).count.saturating_sub(1);
    }

    fn unlink_if_needed(&mut self, node_id: NodeId) {
        if self.nodes[node_id.0].parent.is_some() {
            self.remove(node_id);
        }
    }

    pub fn set_condition(&mut self, block_id: NodeId, cond: Option<NodeId>) {
        let old_cond = self.get_block(block_id).condition;
        if let Some(old) = old_cond {
            self.remove_use(old, block_id);
        }
        if let Some(c) = cond {
            self.add_use(c, block_id);
        }
        self.get_block_mut(block_id).condition = cond;
    }

    pub fn and_condition(&mut self, block_id: NodeId, cond: NodeId) {
        let curr_cond = self.get_block(block_id).condition.unwrap();
        let new_cond = self.new_operation(Instruction::LogicalAnd, vec![curr_cond, cond]);
        self.set_condition(block_id, Some(new_cond));
    }

    pub fn or_condition(&mut self, block_id: NodeId, cond: NodeId) {
        let curr_cond = self.get_block(block_id).condition.unwrap();
        let new_cond = self.new_operation(Instruction::LogicalOr, vec![curr_cond, cond]);
        self.set_condition(block_id, Some(new_cond));
    }

    pub fn parent_block(&self, node_id: NodeId) -> NodeId {
        let mut curr = node_id;
        if let AstNodeKind::Block(_) = &self.nodes[curr.0].kind {
            if let Some(p) = self.nodes[curr.0].parent {
                curr = p;
            }
        }
        while !matches!(&self.nodes[curr.0].kind, AstNodeKind::Block(_)) {
            curr = self.nodes[curr.0].parent.expect("Node must have an ancestor block");
        }
        curr
    }

    pub fn level(&self, node_id: NodeId) -> usize {
        let mut lvl = 0;
        let mut curr = Some(node_id);
        while let Some(c) = curr {
            lvl += 1;
            curr = self.nodes[c.0].parent;
        }
        lvl
    }

    pub fn next(&self, node_id: NodeId) -> Option<NodeId> {
        self.nodes[node_id.0].next_sibling
    }

    pub fn prev(&self, node_id: NodeId) -> Option<NodeId> {
        self.nodes[node_id.0].prev_sibling
    }

    pub fn inverse_cond(&mut self, cond: NodeId) -> NodeId {
        self.new_operation(Instruction::LogicalNot, vec![cond])
    }
}
