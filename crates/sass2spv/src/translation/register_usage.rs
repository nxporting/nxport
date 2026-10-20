// Which registers a function reads from its caller and writes back: they become its arguments (RegisterUsage).

use crate::decoders::register::{Register, RegisterType};
use crate::ir::{BlockId, Instruction, Ir, NodeId, OperandId, OperandType};
use crate::translation::cfg::ControlFlowGraph;

const GPR_MASKS: usize = 4;
const TOTAL_MASKS: usize = GPR_MASKS + 2;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct RegisterMask([i64; TOTAL_MASKS]);

impl std::ops::BitAnd for RegisterMask {
    type Output = Self;
    fn bitand(self, o: Self) -> Self {
        RegisterMask(std::array::from_fn(|i| self.0[i] & o.0[i]))
    }
}

impl std::ops::BitOr for RegisterMask {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        RegisterMask(std::array::from_fn(|i| self.0[i] | o.0[i]))
    }
}

impl std::ops::Not for RegisterMask {
    type Output = Self;
    fn not(self) -> Self {
        RegisterMask(self.0.map(|m| !m))
    }
}

impl std::ops::BitOrAssign for RegisterMask {
    fn bitor_assign(&mut self, o: Self) {
        *self = *self | o;
    }
}

impl std::ops::BitAndAssign for RegisterMask {
    fn bitand_assign(&mut self, o: Self) {
        *self = *self & o;
    }
}

#[derive(Clone, Default)]
pub struct FunctionRegisterUsage {
    pub in_arguments: Vec<Register>,
    pub out_arguments: Vec<Register>,
}

fn mask_of(register: Register) -> RegisterMask {
    let mut m = RegisterMask::default();
    match register.ty {
        RegisterType::Gpr => m.0[(register.index >> 6) as usize] = 1i64 << (register.index & 0x3f),
        RegisterType::Predicate => m.0[4] = 1i64 << register.index,
        _ => m.0[5] = 1i64 << register.index,
    }
    m
}

fn exchange(masks: &mut [RegisterMask], index: usize, value: RegisterMask) -> bool {
    let old = masks[index];
    masks[index] = value;
    old != value
}

fn ends_with_return(ir: &Ir, block: BlockId) -> bool {
    ir.last_node(block).and_then(|n| ir.inst_of(n)) == Some(Instruction::RETURN)
}

fn is_register(ir: &Ir, o: Option<OperandId>) -> bool {
    o.is_some_and(|o| ir.ty(o) == OperandType::Register)
}

pub fn run_pass(ir: &mut Ir, cfg: &ControlFlowGraph) -> FunctionRegisterUsage {
    let n = cfg.blocks.len();
    let idx = |ir: &Ir, b: BlockId| ir.block(b).index as usize;
    let mut in_arguments = Vec::new();
    let mut out_arguments = Vec::new();

    // Registers each block reads before writing, and writes.
    let mut local_inputs = vec![RegisterMask::default(); n];
    let mut local_outputs = vec![RegisterMask::default(); n];
    for &block in &cfg.blocks {
        let bi = idx(ir, block);
        for node in ir.block_nodes(block) {
            for i in 0..ir.sources_count(node) {
                let s = ir.get_source_opt(node, i);
                if !is_register(ir, s) {
                    continue;
                }
                let m = mask_of(ir.opnd(s.unwrap()).register());
                local_inputs[bi] |= m & !local_outputs[bi];
            }
            for i in 0..ir.dests_count(node) {
                let d = ir.get_dest(node, i);
                if is_register(ir, d) {
                    local_outputs[bi] |= mask_of(ir.opnd(d.unwrap()).register());
                }
            }
        }
    }

    // Across blocks.
    let mut global_cmn_outputs = vec![RegisterMask::default(); n];
    let mut global_inputs = vec![RegisterMask::default(); n];
    let mut global_outputs = vec![RegisterMask::default(); n];
    let mut all_outputs = RegisterMask::default();
    let mut all_cmn_outputs = RegisterMask([-1; TOTAL_MASKS]);
    let mut first_pass = true;
    loop {
        let mut modified = false;

        // Outputs.
        for &block in cfg.post_order_blocks.iter().rev() {
            let bi = idx(ir, block);
            let preds = ir.block(block).predecessors.clone();
            if let Some(&first) = preds.first() {
                let pi = idx(ir, first);
                let mut cmn_outputs = local_outputs[pi] | global_cmn_outputs[pi];
                let mut outputs = global_outputs[pi];
                for &p in &preds[1..] {
                    let pi = idx(ir, p);
                    cmn_outputs &= local_outputs[pi] | global_cmn_outputs[pi];
                    outputs |= global_outputs[pi];
                }
                global_inputs[bi] |= outputs & !cmn_outputs;
                if !first_pass {
                    cmn_outputs &= global_cmn_outputs[bi];
                }
                if ends_with_return(ir, block) {
                    all_cmn_outputs &= cmn_outputs | local_outputs[bi];
                }
                if exchange(&mut global_cmn_outputs, bi, cmn_outputs) {
                    modified = true;
                }
                outputs |= local_outputs[bi];
                let v = global_outputs[bi] | outputs;
                if exchange(&mut global_outputs, bi, v) {
                    all_outputs |= outputs;
                    modified = true;
                }
            } else if exchange(&mut global_outputs, bi, local_outputs[bi]) {
                all_outputs |= local_outputs[bi];
                modified = true;
            }
        }

        // Inputs.
        for &block in &cfg.post_order_blocks {
            let bi = idx(ir, block);
            let mut inputs = local_inputs[bi];
            if let Some(next) = ir.next(block) {
                inputs |= global_inputs[idx(ir, next)];
            }
            if let Some(branch) = ir.branch(block) {
                inputs |= global_inputs[idx(ir, branch)];
            }
            inputs &= !global_cmn_outputs[bi];
            let v = global_inputs[bi] | inputs;
            if exchange(&mut global_inputs, bi, v) {
                modified = true;
            }
        }

        first_pass = false;
        if !modified {
            break;
        }
    }

    // Context loads and stores.
    for &block in &cfg.blocks {
        // Only the entry block has no predecessors: it loads the context first.
        if ir.block(block).predecessors.is_empty() {
            let inputs = global_inputs[idx(ir, block)] | (all_outputs & !all_cmn_outputs);
            load_locals(ir, block, inputs, &mut in_arguments);
        }
        if ends_with_return(ir, block) {
            let in_count = in_arguments.len() as i32;
            store_locals(ir, block, all_outputs, in_count, &mut out_arguments);
        }
    }
    FunctionRegisterUsage { in_arguments, out_arguments }
}

pub fn fixup_calls(ir: &mut Ir, blocks: &[BlockId], frus: &[FunctionRegisterUsage]) {
    for &block in blocks {
        for node in ir.block_nodes(block) {
            if ir.inst_of(node) != Some(Instruction::CALL) {
                continue;
            }
            let func_id = ir.src(node, 0);
            let fru = &frus[ir.value(func_id) as usize];
            let in_regs: Vec<OperandId> = fru.in_arguments.iter().map(|&r| ir.register(r)).collect();
            ir.append_sources(node, &in_regs);
            let mut out_regs: Vec<Option<OperandId>> = vec![None];
            out_regs.extend(fru.out_arguments.iter().map(|&r| Some(ir.register(r))));
            ir.append_dests(node, &out_regs);
        }
    }
}

fn reg_type_and_base_index(i: usize) -> (RegisterType, i32) {
    if i < GPR_MASKS {
        (RegisterType::Gpr, (i * 64) as i32)
    } else if i == GPR_MASKS {
        (RegisterType::Predicate, 0)
    } else {
        (RegisterType::Flag, 0)
    }
}

fn load_locals(ir: &mut Ir, block: BlockId, masks: RegisterMask, in_arguments: &mut Vec<Register>) {
    let fill = in_arguments.is_empty();
    let mut node: Option<NodeId> = None;
    let mut arg_index = 0;
    for i in 0..TOTAL_MASKS {
        let (ty, base) = reg_type_and_base_index(i);
        let mut mask = masks.0[i];
        while mask != 0 {
            let bit = mask.trailing_zeros() as i32;
            mask &= !(1i64 << bit);
            let register = Register::new(base + bit, ty);
            if fill {
                in_arguments.push(register);
            }
            let (d, a) = (ir.register(register), ir.argument(arg_index));
            arg_index += 1;
            let copy = ir.operation(Instruction::COPY, Some(d), &[a]);
            node = Some(match node {
                None => {
                    ir.add_first(block, copy);
                    copy
                }
                Some(n) => ir.add_after(n, copy),
            });
        }
    }
}

fn store_locals(ir: &mut Ir, block: BlockId, masks: RegisterMask, in_arguments_count: i32, out_arguments: &mut Vec<Register>) {
    let mut node: Option<NodeId> = None;
    let mut arg_index = in_arguments_count;
    let fill = out_arguments.is_empty();
    for i in 0..TOTAL_MASKS {
        let (ty, base) = reg_type_and_base_index(i);
        let mut mask = masks.0[i];
        while mask != 0 {
            let bit = mask.trailing_zeros() as i32;
            mask &= !(1i64 << bit);
            let register = Register::new(base + bit, ty);
            if fill {
                out_arguments.push(register);
            }
            let (d, s) = (ir.argument(arg_index), ir.register(register));
            arg_index += 1;
            let copy = ir.operation(Instruction::COPY, Some(d), &[s]);
            node = Some(match node {
                None => {
                    let last = ir.last_node(block).unwrap();
                    ir.add_before(last, copy)
                }
                Some(n) => ir.add_after(n, copy),
            });
        }
    }
}
