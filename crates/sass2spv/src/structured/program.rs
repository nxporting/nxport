// Builds the structured program from each function's basic blocks (StructuredProgram and StructuredProgramContext).

use std::collections::{HashMap, HashSet, VecDeque};

use super::instruction_info::{dest_var_type, src_var_type};
use super::{
    goto_elimination, optimizer, Ast, AstBlockType, AstId, AstTexture, HelperFunctionsMask, IoDefinition, StructuredFunction,
    StructuredProgramInfo,
};
use crate::accessor::GpuAccessor;
use crate::collections::NetSet;
use crate::ir::{ir_consts, BlockId, Function, Instruction as I, Ir, IoVariable, NodeId, OperandId, OperandType, StorageKind};
use crate::translation::definitions::{AggregateType, ShaderDefinitions};
use crate::translation::optimizations::utils::NRE;
use crate::translation::resource_manager::ResourceManager;

type R<T> = Result<T, String>;

/// Function parameters are all S32 for now.
const FUNC_PARAMETER_TYPE: AggregateType = AggregateType::S32;

/// A branch the structuring could not turn into a block (GotoStatement).
pub struct GotoStatement {
    pub goto: AstId,
    pub label: AstId,
    pub is_loop: bool,
}

impl GotoStatement {
    pub fn condition(&self, ast: &Ast) -> AstId {
        ast.assignment(self.label).unwrap().0
    }

    pub fn is_unconditional(&self, ast: &Ast) -> bool {
        ast.op(self.goto).inst == I::BRANCH
    }
}

struct Context<'a> {
    ir: &'a mut Ir,
    rm: &'a mut ResourceManager,
    gpu: &'a GpuAccessor,
    definitions: &'a ShaderDefinitions,
    ast: Ast,
    info_functions: Vec<StructuredFunction>,
    io_definitions: NetSet<IoDefinition>,
    helper_functions_mask: HelperFunctionsMask,

    loop_tails: HashSet<BlockId>,
    block_stack: Vec<(AstId, i32, i32)>,
    locals_map: HashMap<OperandId, AstId>,
    goto_temp_asgs: HashMap<i32, AstId>,
    gotos: Vec<GotoStatement>,
    curr_block: AstId,
    curr_end_index: i32,
    loop_end_index: i32,
    current_function: Option<StructuredFunction>,
}

impl Context<'_> {
    fn func(&mut self) -> &mut StructuredFunction {
        self.current_function.as_mut().unwrap()
    }

    fn enter_function(&mut self, blocks_count: i32, name: &str, return_type: AggregateType, ins: Vec<AggregateType>, outs: Vec<AggregateType>) {
        self.loop_tails.clear();
        self.block_stack.clear();
        self.locals_map.clear();
        self.goto_temp_asgs.clear();
        self.gotos.clear();
        self.curr_block = self.ast.block(AstBlockType::Main, None);
        self.curr_end_index = blocks_count;
        self.loop_end_index = blocks_count;
        self.current_function = Some(StructuredFunction {
            main_block: self.curr_block,
            name: name.to_string(),
            return_type,
            in_arguments: ins,
            out_arguments: outs,
            locals: NetSet::new(),
        });
    }

    fn leave_function(&mut self) {
        let f = self.current_function.take().unwrap();
        self.info_functions.push(f);
    }

    fn enter_block(&mut self, block: BlockId) -> R<()> {
        let index = self.ir.block(block).index;
        while self.curr_end_index == index {
            let (b, c, l) = self.block_stack.pop().ok_or("System.InvalidOperationException: Stack empty.")?;
            self.curr_block = b;
            self.curr_end_index = c;
            self.loop_end_index = l;
        }
        if let Some(&asg) = self.goto_temp_asgs.get(&index) {
            self.add_goto_temp_reset(block, asg)?;
        }
        self.look_for_do_while_statements(block)
    }

    fn look_for_do_while_statements(&mut self, block: BlockId) -> R<()> {
        // A predecessor after this block means a loop.
        let index = self.ir.block(block).index;
        let mut preds = self.ir.block(block).predecessors.clone();
        // OrderByDescending is stable.
        preds.sort_by_key(|&p| std::cmp::Reverse(self.ir.block(p).index));
        let mut done = false;
        for predecessor in preds {
            let p_index = self.ir.block(predecessor).index;
            if p_index < index {
                break;
            }
            // A do-while is possible only if the loop end is inside the current scope, else a goto. The tail is
            // remembered so no goto is added at the end of the loop when it is processed.
            if p_index < self.curr_end_index && !done {
                let branch_op = self.ir.last_node(predecessor).filter(|&n| self.ir.is_op(n)).ok_or("System.InvalidCastException")?;
                self.new_block_from_branch(AstBlockType::DoWhile, branch_op, p_index + 1)?;
                self.loop_tails.insert(predecessor);
                done = true;
            } else {
                // This block is the head of a loop that could not be made: the goto condition is reset here, at the
                // jump target.
                let asg = self.goto_temp_asg(index);
                self.add_goto_temp_reset(block, asg)?;
                break;
            }
        }
        Ok(())
    }

    fn leave_block(&mut self, block: BlockId, branch_op: NodeId) -> R<()> {
        let Some(branch) = self.ir.branch(block) else { return Ok(()) };
        let (bi, idx) = (self.ir.block(branch).index, self.ir.block(block).index);
        // The "if" can enclose a branch landing before the end of the current block, or right at its end when the
        // enclosing block is not a loop (there the loop condition would be evaluated and could jump back).
        let in_range = bi < self.curr_end_index || (bi == self.curr_end_index && bi < self.loop_end_index);
        let is_loop = bi <= idx;
        if in_range && !is_loop {
            self.new_block_from_branch(AstBlockType::If, branch_op, bi)?;
        } else if !self.loop_tails.contains(&block) {
            let goto_temp_asg = self.goto_temp_asg(bi);
            // DoWhile: the condition is true for unconditional branches, else true when the branch is taken.
            let cond = self.branch_cond(AstBlockType::DoWhile, branch_op)?;
            let dest = self.ast.assignment(goto_temp_asg).unwrap().0;
            let a = self.ast.assign(dest, Some(cond));
            self.add_node(a)?;
            let branch = self.ast.operation(self.ir.inst(branch_op), &[]);
            self.add_node(branch)?;
            self.gotos.push(GotoStatement { goto: branch, label: goto_temp_asg, is_loop });
        }
        Ok(())
    }

    fn goto_temp_asg(&mut self, index: i32) -> AstId {
        if let Some(&a) = self.goto_temp_asgs.get(&index) {
            return a;
        }
        let goto_temp = self.new_temp(AggregateType::BOOL);
        let f = self.ast.konst(ir_consts::FALSE);
        let a = self.ast.assign(goto_temp, Some(f));
        self.goto_temp_asgs.insert(index, a);
        a
    }

    fn add_goto_temp_reset(&mut self, block: BlockId, goto_temp_asg: AstId) -> R<()> {
        if self.ast.parent(goto_temp_asg).is_some() {
            return Ok(());
        }
        self.add_node(goto_temp_asg)?;
        // Block 0 runs first anyway, where the local's own initial assignment resets it.
        if self.ir.block(block).index != 0 {
            let dest = self.ast.assignment(goto_temp_asg).unwrap().0;
            let f = self.ast.konst(ir_consts::FALSE);
            let a = self.ast.assign(dest, Some(f));
            let main = self.func().main_block;
            self.ast.add_first(main, a)?;
        }
        Ok(())
    }

    fn new_block_from_branch(&mut self, ty: AstBlockType, branch_op: NodeId, end_index: i32) -> R<()> {
        let cond = self.branch_cond(ty, branch_op)?;
        self.new_block(ty, cond, end_index)
    }

    fn new_block(&mut self, ty: AstBlockType, cond: AstId, end_index: i32) -> R<()> {
        let child = self.ast.block(ty, Some(cond));
        self.add_node(child)?;
        self.block_stack.push((self.curr_block, self.curr_end_index, self.loop_end_index));
        self.curr_block = child;
        self.curr_end_index = end_index;
        if ty == AstBlockType::DoWhile {
            self.loop_end_index = end_index;
        }
        Ok(())
    }

    fn branch_cond(&mut self, ty: AstBlockType, branch_op: NodeId) -> R<AstId> {
        let inst = self.ir.inst(branch_op);
        if inst == I::BRANCH {
            // Unconditional: an "if" is never entered, a loop always repeats.
            return Ok(self.ast.konst(if ty == AstBlockType::If { ir_consts::FALSE } else { ir_consts::TRUE }));
        }
        let src = *self.ir.op(branch_op).sources.first().ok_or("System.IndexOutOfRangeException: Index was outside the bounds of the array.")?;
        let cond = self.get_operand(src).ok_or(NRE)?;
        let inv_inst = if ty == AstBlockType::If { I::BRANCH_IF_TRUE } else { I::BRANCH_IF_FALSE };
        Ok(if inst == inv_inst { self.ast.operation(I::LOGICAL_NOT, &[cond]) } else { cond })
    }

    fn add_node(&mut self, node: AstId) -> R<()> {
        self.ast.add(self.curr_block, node)
    }

    fn new_temp(&mut self, ty: AggregateType) -> AstId {
        let t = self.ast.local(ty);
        self.func().locals.insert(t);
        t
    }

    fn get_operand_or_cb_load(&mut self, operand: Option<OperandId>) -> R<Option<AstId>> {
        let Some(o) = operand else { return Ok(None) };
        if self.ir.ty(o) == OperandType::ConstantBuffer {
            let (slot, offset) = (self.ir.opnd(o).cbuf_slot(), self.ir.opnd(o).cbuf_offset());
            let binding = self.rm.constant_buffer_binding(self.gpu, slot)?;
            self.rm.set_used_constant_buffer_binding(binding);
            let sources = vec![
                Some(self.ast.operand(OperandType::Constant, binding)),
                Some(self.ast.operand(OperandType::Constant, 0)),
                Some(self.ast.operand(OperandType::Constant, offset >> 2)),
                Some(self.ast.operand(OperandType::Constant, offset & 3)),
            ];
            return Ok(Some(self.ast.operation_full(I::LOAD, StorageKind::ConstantBuffer, false, 0, sources, 4, None)));
        }
        Ok(self.get_operand(Some(o)))
    }

    fn get_operand(&mut self, operand: Option<OperandId>) -> Option<AstId> {
        let o = operand?;
        let (ty, value) = (self.ir.ty(o), self.ir.value(o));
        if ty != OperandType::LocalVariable {
            return Some(self.ast.operand(ty, value));
        }
        if let Some(&a) = self.locals_map.get(&o) {
            return Some(a);
        }
        let a = self.ast.operand(ty, value);
        self.locals_map.insert(o, a);
        self.func().locals.insert(a);
        Some(a)
    }
}

pub fn make_structured_program(
    ir: &mut Ir,
    functions: &[Function],
    definitions: &ShaderDefinitions,
    rm: &mut ResourceManager,
    gpu: &GpuAccessor,
) -> R<StructuredProgramInfo> {
    // Geometry passthrough is never used: no passthrough inputs to declare.
    let mut ast = Ast::default();
    let placeholder = ast.block(AstBlockType::Main, None);
    let mut c = Context {
        ir,
        rm,
        gpu,
        definitions,
        ast,
        info_functions: Vec::new(),
        io_definitions: NetSet::new(),
        helper_functions_mask: HelperFunctionsMask::default(),
        loop_tails: HashSet::new(),
        block_stack: Vec::new(),
        locals_map: HashMap::new(),
        goto_temp_asgs: HashMap::new(),
        gotos: Vec::new(),
        curr_block: placeholder,
        curr_end_index: 0,
        loop_end_index: 0,
        current_function: None,
    };
    for function in functions {
        let blocks = &function.blocks;
        let return_type = if function.returns_value { FUNC_PARAMETER_TYPE } else { AggregateType::VOID };
        let ins = vec![FUNC_PARAMETER_TYPE; function.in_arguments_count as usize];
        let outs = vec![FUNC_PARAMETER_TYPE; function.out_arguments_count as usize];
        c.enter_function(blocks.len() as i32, &function.name, return_type, ins, outs);
        remove_phis(c.ir, blocks);
        for &block in blocks {
            c.enter_block(block)?;
            let mut op_node = c.ir.first_node(block);
            while let Some(n) = op_node {
                if !c.ir.is_op(n) {
                    return Err("System.InvalidCastException: Unable to cast object of type 'PhiNode' to type 'Operation'.".into());
                }
                let inst = c.ir.inst(n);
                if matches!(inst, I::BRANCH | I::BRANCH_IF_FALSE | I::BRANCH_IF_TRUE) {
                    c.leave_block(block, n)?;
                } else {
                    add_operation(&mut c, n, functions)?;
                }
                op_node = c.ir.next_node(n);
            }
        }
        let gotos = std::mem::take(&mut c.gotos);
        goto_elimination::eliminate(&mut c.ast, gotos)?;
        let main_block = c.func().main_block;
        let f = c.current_function.as_mut().unwrap();
        optimizer::optimize(&mut c.ast, main_block, &mut f.locals)?;
        c.leave_function();
    }
    Ok(StructuredProgramInfo {
        ast: c.ast,
        functions: c.info_functions,
        io_definitions: c.io_definitions,
        helper_functions_mask: c.helper_functions_mask,
    })
}

/// Phis become copies at the end of each predecessor into a temporary, copied to the phi's destination.
fn remove_phis(ir: &mut Ir, blocks: &[BlockId]) {
    for &block in blocks {
        let mut node = ir.first_node(block);
        while let Some(n) = node {
            let mut next_node = ir.next_node(n);
            if !ir.is_phi(n) {
                node = next_node;
                continue;
            }
            let temp = ir.local();
            for i in 0..ir.sources_count(n) {
                let src = ir.src(n, i);
                let src_block = ir.phi_block(n, i);
                let copy = ir.operation(I::COPY, Some(temp), &[src]);
                ir.append(src_block, copy);
            }
            let dest = ir.dest(n);
            let copy2 = ir.operation(I::COPY, dest, &[temp]);
            let added = ir.add_after(n, copy2);
            next_node = ir.next_node(added);
            ir.remove_node(n);
            node = next_node;
        }
    }
}

fn add_operation(c: &mut Context, n: NodeId, functions: &[Function]) -> R<()> {
    let mut inst = c.ir.inst(n);
    let op = c.ir.op(n);
    let (storage_kind, force_precise, op_index) = (op.storage_kind, op.force_precise, op.index);
    let src_ops: Vec<Option<OperandId>> = op.sources.clone();
    let dests: Vec<Option<OperandId>> = op.dests.clone();
    let tex = op.tex;

    if inst == I::LOAD || inst == I::STORE {
        if storage_kind.is_input_or_output() {
            let s0 = src_ops[0].ok_or(NRE)?;
            let io_variable = IoVariable::from_value(c.ir.value(s0));
            let is_output = storage_kind.is_output();
            let (mut location, mut component) = (0, 0);
            if c.definitions.has_per_location_input_or_output(io_variable, is_output) {
                location = c.ir.value(src_ops.get(1).copied().flatten().ok_or(NRE)?);
                if src_ops.len() > 2 {
                    let s2 = src_ops[2].ok_or(NRE)?;
                    if c.ir.ty(s2) == OperandType::Constant
                        && c.definitions.has_per_location_input_or_output_component(io_variable, location, c.ir.value(s2), is_output)
                    {
                        component = c.ir.value(s2);
                    }
                }
            }
            c.io_definitions.insert(IoDefinition { storage_kind, io_variable, location, component });
        } else if storage_kind == StorageKind::ConstantBuffer {
            let s0 = src_ops[0].ok_or(NRE)?;
            if c.ir.ty(s0) == OperandType::Constant {
                c.rm.set_used_constant_buffer_binding(c.ir.value(s0));
            }
        }
    }

    let vector_dest = matches!(inst, I::IMAGE_LOAD | I::TEXTURE_SAMPLE);
    let sources_count = src_ops.len();
    let out_dests_count = if !dests.is_empty() && !vector_dest { dests.len() - 1 } else { 0 };
    let mut sources: Vec<Option<AstId>> = vec![None; sources_count + out_dests_count];

    if inst == I::CALL {
        // SPIR-V function parameters are copied to local variables before the call (as the Khronos compiler does).
        let func_index = c.ir.value(src_ops[0].ok_or(NRE)?);
        sources[0] = Some(c.ast.operand(OperandType::Constant, func_index));
        let in_args_count = functions.get(func_index as usize).ok_or("System.ArgumentOutOfRangeException")?.in_arguments_count;
        for index in 1..sources_count {
            let source = c.get_operand_or_cb_load(src_ops[index])?;
            if (index as i32) - 1 < in_args_count {
                let arg_temp = c.new_temp(FUNC_PARAMETER_TYPE);
                let a = c.ast.assign(arg_temp, source);
                c.add_node(a)?;
                sources[index] = Some(arg_temp);
            } else {
                sources[index] = source;
            }
        }
    } else {
        for index in 0..sources_count {
            sources[index] = c.get_operand_or_cb_load(src_ops[index])?;
        }
    }

    for index in 0..out_dests_count {
        let oper = c.get_operand(dests[1 + index]).ok_or(NRE)?;
        c.ast.opnd_mut(oper).var_type = src_var_type(inst, sources_count + index)?;
        sources[sources_count + index] = Some(oper);
    }

    let tex_op = |c: &mut Context, inst: I, sources: &[Option<AstId>]| -> AstId {
        let t = tex.unwrap();
        let at = AstTexture {
            ty: t.ty,
            format: t.format,
            flags: t.flags,
            set: t.set,
            binding: t.binding,
            sampler_set: t.sampler_set,
            sampler_binding: t.sampler_binding,
        };
        c.ast.operation_full(inst, StorageKind::None, false, op_index, sources.to_vec(), sources.len(), Some(at))
    };

    let components_count = (op_index as u32).count_ones();
    let dest0 = dests.first().copied().flatten();

    if vector_dest && components_count > 1 {
        let mut dest_type = dest_var_type(inst)?;
        let source = if tex.is_some() {
            if inst == I::IMAGE_LOAD {
                dest_type = tex.unwrap().format.component_type();
            }
            tex_op(c, inst, &sources)
        } else {
            c.ast.operation_full(inst, storage_kind, force_precise, op_index, sources.clone(), sources_count, None)
        };
        let dest_elem_type = dest_type;
        dest_type = match components_count {
            2 => dest_type | AggregateType::VECTOR2,
            3 => dest_type | AggregateType::VECTOR3,
            4 => dest_type | AggregateType::VECTOR4,
            _ => dest_type,
        };
        let dest_vec = c.new_temp(dest_type);
        let a = c.ast.assign(dest_vec, Some(source));
        c.add_node(a)?;
        for (i, &d) in dests.iter().enumerate() {
            let dest = c.get_operand(d).ok_or(NRE)?;
            let index = c.ast.operand(OperandType::Constant, i as i32);
            c.ast.opnd_mut(dest).var_type = dest_elem_type;
            let extract = c.ast.operation_full(I::VECTOR_EXTRACT, StorageKind::None, false, 0, vec![Some(dest_vec), Some(index)], 2, None);
            let a = c.ast.assign(dest, Some(extract));
            c.add_node(a)?;
        }
    } else if let Some(d) = dest0 {
        let dest = c.get_operand(Some(d)).unwrap();
        // All-bool sources: short-circuiting logical operations rather than bitwise ones on integers.
        if is_bitwise_inst(inst) && are_all_source_types_bool(&c.ast, &sources) {
            inst = logical_from_bitwise(inst);
        }
        let is_cond_sel = inst == I::CONDITIONAL_SELECT;
        let is_copy = inst == I::COPY;
        if is_cond_sel || is_copy {
            let ty = var_type_from_uses(c.ir, d)?;
            if is_cond_sel && ty == AggregateType::FP32 {
                inst = inst | I::FP32;
            }
            c.ast.opnd_mut(dest).var_type = ty;
        } else {
            c.ast.opnd_mut(dest).var_type = dest_var_type(inst)?;
        }
        let source = if tex.is_some() {
            if inst == I::IMAGE_LOAD {
                c.ast.opnd_mut(dest).var_type = tex.unwrap().format.component_type();
            }
            Some(tex_op(c, inst, &sources))
        } else if !is_copy {
            Some(c.ast.operation_full(inst, storage_kind, force_precise, op_index, sources.clone(), sources_count, None))
        } else {
            sources[0]
        };
        let a = c.ast.assign(dest, source);
        c.add_node(a)?;
    } else if inst == I::COMMENT {
        let text = c.ir.op(n).comment.clone().unwrap_or_default();
        let cm = c.ast.comment(text);
        c.add_node(cm)?;
    } else if tex.is_some() {
        let t = tex_op(c, inst, &sources);
        c.add_node(t)?;
    } else {
        let o = c.ast.operation_full(inst, storage_kind, force_precise, op_index, sources, sources_count, None);
        c.add_node(o)?;
    }

    // NVIDIA-specific operations the backend emulates with helper functions.
    match c.ir.inst(n) {
        I::MULTIPLY_HIGH_S32 => c.helper_functions_mask.0 |= HelperFunctionsMask::MULTIPLY_HIGH_S32,
        I::MULTIPLY_HIGH_U32 => c.helper_functions_mask.0 |= HelperFunctionsMask::MULTIPLY_HIGH_U32,
        I::SWIZZLE_ADD => c.helper_functions_mask.0 |= HelperFunctionsMask::SWIZZLE_ADD,
        I::FSI_BEGIN | I::FSI_END => c.helper_functions_mask.0 |= HelperFunctionsMask::FSI,
        _ => {}
    }
    Ok(())
}

/// The type a copy or select result is used as, following copies.
fn var_type_from_uses(ir: &Ir, dest: OperandId) -> R<AggregateType> {
    let mut visited = HashSet::new();
    let mut pending = VecDeque::new();
    visited.insert(dest);
    pending.push_back(dest);
    while let Some(operand) = pending.pop_front() {
        for use_node in ir.use_ops(operand).to_vec() {
            let Some(op) = ir.as_op(use_node) else { continue };
            if op.inst == I::COPY {
                let d = op.dests.first().copied().flatten().ok_or(NRE)?;
                if ir.ty(d) == OperandType::LocalVariable {
                    if visited.insert(d) {
                        pending.push_back(d);
                        break;
                    }
                } else {
                    return super::var_type_of(ir.ty(d));
                }
            } else {
                for (index, &s) in op.sources.iter().enumerate() {
                    if s == Some(operand) {
                        return src_var_type(op.inst, index);
                    }
                }
            }
        }
    }
    Ok(AggregateType::S32)
}

fn are_all_source_types_bool(ast: &Ast, sources: &[Option<AstId>]) -> bool {
    sources.iter().all(|s| s.and_then(|s| ast.as_operand(s)).is_some_and(|o| o.var_type == AggregateType::BOOL))
}

fn is_bitwise_inst(inst: I) -> bool {
    matches!(inst, I::BITWISE_AND | I::BITWISE_EXCLUSIVE_OR | I::BITWISE_NOT | I::BITWISE_OR)
}

fn logical_from_bitwise(inst: I) -> I {
    match inst {
        I::BITWISE_AND => I::LOGICAL_AND,
        I::BITWISE_EXCLUSIVE_OR => I::LOGICAL_EXCLUSIVE_OR,
        I::BITWISE_NOT => I::LOGICAL_NOT,
        _ => I::LOGICAL_OR,
    }
}
