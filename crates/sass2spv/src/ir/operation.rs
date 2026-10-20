use super::instruction::Instruction;
use super::operand::Operand;
use super::storage_kind::StorageKind;

#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    pub inst: Instruction,
    pub storage_kind: StorageKind,
    pub dest: Option<Operand>,
    pub sources: Vec<Operand>,
    pub index: i32,
    pub force_precise: bool,
}

impl Operation {
    pub fn new(inst: Instruction, dest: Option<Operand>, sources: Vec<Operand>) -> Self {
        Self {
            inst,
            storage_kind: StorageKind::None,
            dest,
            sources,
            index: 0,
            force_precise: false,
        }
    }

    pub fn with_storage(mut self, storage_kind: StorageKind) -> Self {
        self.storage_kind = storage_kind;
        self
    }

    pub fn with_index(mut self, index: i32) -> Self {
        self.index = index;
        self
    }

    pub fn get_source(&self, index: usize) -> Option<Operand> {
        self.sources.get(index).copied()
    }
}
