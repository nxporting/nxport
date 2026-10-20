use super::instruction::Instruction;
use super::operation::Operation;

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub index: usize,
    pub operations: Vec<Operation>,
    pub next: Option<usize>,
    pub branch: Option<usize>,
    pub predecessors: Vec<usize>,
}

impl BasicBlock {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            operations: Vec::new(),
            next: None,
            branch: None,
            predecessors: Vec::new(),
        }
    }

    pub fn get_last_op(&self) -> Option<&Operation> {
        self.operations.last()
    }

    pub fn is_control_flow_inst(inst: Instruction) -> bool {
        matches!(
            inst,
            Instruction::Branch
                | Instruction::BranchIfFalse
                | Instruction::BranchIfTrue
                | Instruction::Discard
                | Instruction::Return
        )
    }

    pub fn append(&mut self, op: Operation) {
        if let Some(last) = self.operations.last() {
            if Self::is_control_flow_inst(last.inst) {
                let len = self.operations.len();
                self.operations.insert(len - 1, op);
                return;
            }
        }
        self.operations.push(op);
    }
}
