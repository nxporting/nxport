use crate::ir::instruction::Instruction;
use crate::ir::operand::{Operand, OperandType};
use crate::ir::operation::Operation;
use rustc_hash::FxHashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    Input,
    Output,
    ConstantBuffer,
    StorageBuffer,
    LocalMemory,
    SharedMemory,
}

pub struct EmitterContext {
    pub operations: Vec<Operation>,
    pub labels: FxHashMap<u64, Operand>,
    next_var_id: u32,
    next_label_id: u32,
}

impl Default for EmitterContext {
    fn default() -> Self {
        Self::new()
    }
}

impl EmitterContext {
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            labels: FxHashMap::default(),
            next_var_id: 1,
            next_label_id: 1,
        }
    }

    pub fn alloc_var(&mut self) -> Operand {
        let id = self.next_var_id;
        self.next_var_id += 1;
        Operand::local_var(id)
    }

    pub fn alloc_label(&mut self) -> Operand {
        let id = self.next_label_id;
        self.next_label_id += 1;
        Operand::new(OperandType::Label, id as i32)
    }

    pub fn get_or_create_label(&mut self, address: u64) -> Operand {
        if let Some(lbl) = self.labels.get(&address) {
            *lbl
        } else {
            let lbl = self.alloc_label();
            self.labels.insert(address, lbl);
            lbl
        }
    }

    pub fn mark_label(&mut self, label: Operand) {
        self.operations.push(Operation::new(Instruction::MarkLabel, None, vec![label]));
    }

    pub fn add(&mut self, inst: Instruction, dest: Option<Operand>, sources: Vec<Operand>) -> Option<Operand> {
        self.operations.push(Operation::new(inst, dest, sources));
        dest
    }

    pub fn copy(&mut self, dest: Operand, src: Operand) {
        self.add(Instruction::Copy, Some(dest), vec![src]);
    }

    // ALU Operations
    pub fn i_add(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Add, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn i_sub(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Subtract, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn i_mul(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Multiply, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn f_add(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Add, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn f_mul(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Multiply, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn f_fma(&mut self, src_a: Operand, src_b: Operand, src_c: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::FusedMultiplyAdd, Some(res), vec![src_a, src_b, src_c]);
        res
    }

    pub fn bitwise_and(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::BitwiseAnd, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn bitwise_or(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::BitwiseOr, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn bitwise_xor(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::BitwiseExclusiveOr, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn bitwise_not(&mut self, src_a: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::BitwiseNot, Some(res), vec![src_a]);
        res
    }

    pub fn shift_left(&mut self, src_a: Operand, shift: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ShiftLeft, Some(res), vec![src_a, shift]);
        res
    }

    pub fn shift_right_u32(&mut self, src_a: Operand, shift: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ShiftRightU32, Some(res), vec![src_a, shift]);
        res
    }

    pub fn shift_right_s32(&mut self, src_a: Operand, shift: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ShiftRightS32, Some(res), vec![src_a, shift]);
        res
    }

    // Comparison & Branching
    pub fn compare_equal(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::CompareEqual, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn compare_less(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::CompareLess, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn conditional_select(&mut self, cond: Operand, true_val: Operand, false_val: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ConditionalSelect, Some(res), vec![cond, true_val, false_val]);
        res
    }

    pub fn branch(&mut self, label: Operand) {
        self.add(Instruction::Branch, None, vec![label]);
    }

    pub fn branch_if_true(&mut self, label: Operand, cond: Operand) {
        self.add(Instruction::BranchIfTrue, None, vec![label, cond]);
    }

    pub fn branch_if_false(&mut self, label: Operand, cond: Operand) {
        self.add(Instruction::BranchIfFalse, None, vec![label, cond]);
    }

    pub fn f_min(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Minimum, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn f_max(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Maximum, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn i_min_s32(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Minimum, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn i_min_u32(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::MinimumU32, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn i_max_s32(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Maximum, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn i_max_u32(&mut self, src_a: Operand, src_b: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::MaximumU32, Some(res), vec![src_a, src_b]);
        res
    }

    pub fn f_cos(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Cosine, Some(res), vec![src]);
        res
    }

    pub fn f_sin(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::Sine, Some(res), vec![src]);
        res
    }

    pub fn f_exp2(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ExponentB2, Some(res), vec![src]);
        res
    }

    pub fn f_log2(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::LogarithmB2, Some(res), vec![src]);
        res
    }

    pub fn f_rsqrt(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ReciprocalSquareRoot, Some(res), vec![src]);
        res
    }

    pub fn f_sqrt(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::SquareRoot, Some(res), vec![src]);
        res
    }

    pub fn convert_fp_to_s32(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ConvertFP32ToS32, Some(res), vec![src]);
        res
    }

    pub fn convert_fp_to_u32(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ConvertFP32ToU32, Some(res), vec![src]);
        res
    }

    pub fn convert_s32_to_fp(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ConvertS32ToFP32, Some(res), vec![src]);
        res
    }

    pub fn convert_u32_to_fp(&mut self, src: Operand) -> Operand {
        let res = self.alloc_var();
        self.add(Instruction::ConvertU32ToFP32, Some(res), vec![src]);
        res
    }

    pub fn discard(&mut self) {
        self.add(Instruction::Discard, None, vec![]);
    }

    pub fn return_op(&mut self) {
        self.add(Instruction::Return, None, vec![]);
    }
}
