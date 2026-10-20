use super::ast::{AstArena, AstBlockType, AstNodeKind, NodeId};
use super::goto_elimination::GotoElimination;
use super::goto_statement::GotoStatement;
use super::optimizer::AstOptimizer;
use crate::ir::basic_block::BasicBlock;
use crate::ir::cfg::ControlFlowGraph;
use crate::ir::instruction::Instruction;
use crate::ir::operand::{Operand, OperandType};
use crate::ir::operation::Operation;
use crate::ir::storage_kind::{AggregateType, IoVariable, StorageKind};
use crate::options::ShaderStage;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IoDefinition {
    pub storage_kind: StorageKind,
    pub io_variable: IoVariable,
    pub location: i32,
    pub component: i32,
}

impl IoDefinition {
    pub fn new(storage_kind: StorageKind, io_variable: IoVariable, location: i32, component: i32) -> Self {
        Self {
            storage_kind,
            io_variable,
            location,
            component,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StructuredFunction {
    pub main_block: NodeId,
    pub name: String,
    pub return_type: AggregateType,
    pub in_arguments: Vec<AggregateType>,
    pub out_arguments: Vec<AggregateType>,
    pub locals: FxHashSet<NodeId>,
}

#[derive(Debug, Clone)]
pub struct StructuredProgramInfo {
    pub functions: Vec<StructuredFunction>,
    pub io_definitions: FxHashSet<IoDefinition>,
    pub cbuf_bindings: FxHashSet<i32>,
}

pub struct StructuredProgramContext {
    pub arena: AstArena,
    pub info: StructuredProgramInfo,
    _stage: ShaderStage,
    debug_mode: bool,

    loop_tails: FxHashSet<usize>,
    block_stack: Vec<(NodeId, usize, usize)>,
    locals_map: FxHashMap<Operand, NodeId>,
    goto_temp_asgs: FxHashMap<usize, NodeId>,
    gotos: Vec<GotoStatement>,

    curr_block: NodeId,
    curr_end_index: usize,
    loop_end_index: usize,
    current_function: Option<StructuredFunction>,
}

impl StructuredProgramContext {
    pub fn new(stage: ShaderStage, debug_mode: bool) -> Self {
        Self {
            arena: AstArena::new(),
            info: StructuredProgramInfo {
                functions: Vec::new(),
                io_definitions: FxHashSet::default(),
                cbuf_bindings: FxHashSet::default(),
            },
            _stage: stage,
            debug_mode,
            loop_tails: FxHashSet::default(),
            block_stack: Vec::new(),
            locals_map: FxHashMap::default(),
            goto_temp_asgs: FxHashMap::default(),
            gotos: Vec::new(),
            curr_block: NodeId(0),
            curr_end_index: 0,
            loop_end_index: 0,
            current_function: None,
        }
    }

    pub fn enter_function(&mut self, blocks_count: usize, name: &str) {
        self.loop_tails.clear();
        self.block_stack.clear();
        self.locals_map.clear();
        self.goto_temp_asgs.clear();
        self.gotos.clear();

        let main_block = self.arena.new_block(AstBlockType::Main, None);
        self.curr_block = main_block;
        self.curr_end_index = blocks_count;
        self.loop_end_index = blocks_count;

        self.current_function = Some(StructuredFunction {
            main_block,
            name: name.to_string(),
            return_type: AggregateType::VOID,
            in_arguments: Vec::new(),
            out_arguments: Vec::new(),
            locals: FxHashSet::default(),
        });
    }

    pub fn leave_function(&mut self) {
        if let Some(func) = self.current_function.take() {
            self.info.functions.push(func);
        }
    }

    pub fn enter_block(&mut self, block: &BasicBlock, blocks: &[BasicBlock]) {
        while self.curr_end_index == block.index {
            if let Some((b, curr_end, loop_end)) = self.block_stack.pop() {
                self.curr_block = b;
                self.curr_end_index = curr_end;
                self.loop_end_index = loop_end;
            } else {
                break;
            }
        }

        if let Some(&goto_temp_asg) = self.goto_temp_asgs.get(&block.index) {
            self.add_goto_temp_reset(block.index, goto_temp_asg);
        }

        self.look_for_do_while_statements(block, blocks);
    }

    pub fn leave_block(&mut self, block: &BasicBlock, branch_op: &Operation) {
        self.look_for_if_statements(block, branch_op);
    }

    fn look_for_do_while_statements(&mut self, block: &BasicBlock, blocks: &[BasicBlock]) {
        let mut sorted_preds = block.predecessors.clone();
        sorted_preds.sort_by(|a, b| b.cmp(a));

        let mut done = false;
        for &pred_idx in &sorted_preds {
            if pred_idx < block.index {
                break;
            }

            if pred_idx < self.curr_end_index && !done {
                let pred = &blocks[pred_idx];
                if let Some(branch_op) = pred.get_last_op() {
                    let cond = self.get_branch_cond(AstBlockType::DoWhile, branch_op);
                    self.new_block(AstBlockType::DoWhile, cond, pred_idx + 1);
                    self.loop_tails.insert(pred_idx);
                    done = true;
                }
            } else {
                let asg = self.get_goto_temp_asg(block.index);
                self.add_goto_temp_reset(block.index, asg);
                break;
            }
        }
    }

    fn look_for_if_statements(&mut self, block: &BasicBlock, branch_op: &Operation) {
        let branch_idx = match block.branch {
            Some(idx) => idx,
            None => return,
        };

        let in_range = branch_idx < self.curr_end_index
            || (branch_idx == self.curr_end_index && branch_idx < self.loop_end_index);

        let is_loop = branch_idx <= block.index;

        if in_range && !is_loop {
            let cond = self.get_branch_cond(AstBlockType::If, branch_op);
            self.new_block(AstBlockType::If, cond, branch_idx);
        } else if !self.loop_tails.contains(&block.index) {
            let goto_temp_asg = self.get_goto_temp_asg(branch_idx);
            let dest = match &self.arena.nodes[goto_temp_asg.0].kind {
                AstNodeKind::Assignment(asg) => asg.destination,
                _ => panic!("Expected Assignment"),
            };

            let cond = self.get_branch_cond(AstBlockType::DoWhile, branch_op);
            let asg_node = self.arena.new_assignment(dest, cond);
            self.arena.add(self.curr_block, asg_node);

            let branch_node = self.arena.new_operation(branch_op.inst, vec![]);
            self.arena.add(self.curr_block, branch_node);

            self.gotos.push(GotoStatement::new(branch_node, goto_temp_asg, is_loop));
        }
    }

    fn get_goto_temp_asg(&mut self, index: usize) -> NodeId {
        if let Some(&asg) = self.goto_temp_asgs.get(&index) {
            return asg;
        }

        let goto_temp = self.arena.new_temp(AggregateType::BOOL);
        if let Some(func) = &mut self.current_function {
            func.locals.insert(goto_temp);
        }

        let false_const = self.arena.new_const(0);
        let asg = self.arena.new_assignment(goto_temp, false_const);
        self.goto_temp_asgs.insert(index, asg);
        asg
    }

    fn add_goto_temp_reset(&mut self, block_idx: usize, goto_temp_asg: NodeId) {
        if self.arena.nodes[goto_temp_asg.0].parent.is_some() {
            return;
        }

        self.arena.add(self.curr_block, goto_temp_asg);

        if block_idx != 0 {
            if let Some(func) = &self.current_function {
                let dest = match &self.arena.nodes[goto_temp_asg.0].kind {
                    AstNodeKind::Assignment(asg) => asg.destination,
                    _ => panic!("Expected Assignment"),
                };
                let false_const = self.arena.new_const(0);
                let reset_asg = self.arena.new_assignment(dest, false_const);
                self.arena.add_first(func.main_block, reset_asg);
            }
        }
    }

    fn new_block(&mut self, block_type: AstBlockType, cond: NodeId, end_index: usize) {
        let child_block = self.arena.new_block(block_type, Some(cond));
        self.arena.add(self.curr_block, child_block);

        self.block_stack.push((self.curr_block, self.curr_end_index, self.loop_end_index));
        self.curr_block = child_block;
        self.curr_end_index = end_index;

        if block_type == AstBlockType::DoWhile {
            self.loop_end_index = end_index;
        }
    }

    fn get_branch_cond(&mut self, _block_type: AstBlockType, branch_op: &Operation) -> NodeId {
        if branch_op.inst == Instruction::Branch {
            self.arena.new_const(1) // True
        } else {
            let cond_op = branch_op.get_source(1).or_else(|| branch_op.get_source(0)).unwrap();
            let mut cond = self.get_operand_or_cb_load(&cond_op);
            if branch_op.inst == Instruction::BranchIfFalse {
                cond = self.arena.inverse_cond(cond);
            }
            cond
        }
    }

    pub fn get_operand(&mut self, operand: &Operand) -> NodeId {
        if operand.op_type == OperandType::LocalVariable || operand.op_type == OperandType::Register {
            if let Some(&node_id) = self.locals_map.get(operand) {
                return node_id;
            }
            let node_id = self.arena.new_operand(operand);
            self.locals_map.insert(*operand, node_id);
            if let Some(func) = &mut self.current_function {
                func.locals.insert(node_id);
            }
            node_id
        } else {
            self.arena.new_operand(operand)
        }
    }

    pub fn get_operand_or_cb_load(&mut self, operand: &Operand) -> NodeId {
        if operand.op_type == OperandType::ConstantBuffer {
            let slot = operand.get_cbuf_slot();
            let offset = operand.get_cbuf_offset();
            self.info.cbuf_bindings.insert(slot);

            let slot_node = self.arena.new_const(slot);
            let field_node = self.arena.new_const(0);
            let vec_node = self.arena.new_const(offset >> 4);
            let elem_node = self.arena.new_const((offset >> 2) & 3);

            self.arena.new_operation_ext(
                Instruction::Load,
                StorageKind::ConstantBuffer,
                false,
                0,
                vec![slot_node, field_node, vec_node, elem_node],
            )
        } else {
            self.get_operand(operand)
        }
    }

    pub fn add_operation(&mut self, operation: &Operation) {
        if operation.inst == Instruction::Load || operation.inst == Instruction::Store {
            if operation.storage_kind.is_input_or_output() {
                let io_var_val = operation.get_source(0).map(|s| s.value).unwrap_or(0);
                let io_var = unsafe { std::mem::transmute::<u32, IoVariable>(io_var_val as u32) };
                let loc = operation.get_source(1).map(|s| s.value).unwrap_or(0);
                let comp = operation.get_source(2).map(|s| s.value).unwrap_or(0);
                self.info.io_definitions.insert(IoDefinition::new(
                    operation.storage_kind,
                    io_var,
                    loc,
                    comp,
                ));
            } else if operation.storage_kind == StorageKind::ConstantBuffer {
                if let Some(src0) = operation.get_source(0) {
                    if src0.op_type == OperandType::Constant {
                        self.info.cbuf_bindings.insert(src0.value);
                    }
                }
            }
        }

        let mut sources = Vec::new();
        for src in &operation.sources {
            let node = self.get_operand_or_cb_load(src);
            sources.push(node);
        }

        let ast_op = self.arena.new_operation_ext(
            operation.inst,
            operation.storage_kind,
            operation.force_precise,
            operation.index,
            sources,
        );

        if let Some(dest) = operation.dest {
            let dest_node = self.get_operand(&dest);
            let asg = self.arena.new_assignment(dest_node, ast_op);
            self.arena.add(self.curr_block, asg);
        } else {
            self.arena.add(self.curr_block, ast_op);
        }
    }

    pub fn make_structured_program(mut self, cfg: &ControlFlowGraph) -> (AstArena, StructuredProgramInfo) {
        let blocks = &cfg.blocks;
        if blocks.is_empty() {
            return (self.arena, self.info);
        }

        self.enter_function(blocks.len(), "main");

        for blk in blocks {
            self.enter_block(blk, blocks);

            for op in &blk.operations {
                if BasicBlock::is_control_flow_inst(op.inst) {
                    self.leave_block(blk, op);
                } else {
                    self.add_operation(op);
                }
            }
        }

        let mut gotos = std::mem::take(&mut self.gotos);
        GotoElimination::eliminate(&mut self.arena, &mut gotos);

        let main_block = self.current_function.as_ref().unwrap().main_block;
        AstOptimizer::optimize(&mut self.arena, main_block, self.debug_mode);

        self.leave_function();

        (self.arena, self.info)
    }
}
