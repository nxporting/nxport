// Register operands to SSA locals with phi nodes at dominance frontiers (Ssa).

use std::collections::{HashMap, VecDeque};

use crate::decoders::register::{Register, RegisterType};
use crate::ir::{BlockId, Ir, NodeId, OperandId, OperandType};

const GPRS_COUNT: i32 = 256;
const PREDS_COUNT: i32 = 8;
const FLAGS_COUNT: i32 = 4;
const TOTAL_COUNT: i32 = GPRS_COUNT + PREDS_COUNT + FLAGS_COUNT;
const GPRS_AND_PREDS_COUNT: i32 = GPRS_COUNT + PREDS_COUNT;

struct DefMap {
    map: HashMap<Register, OperandId>,
    phi_masks: [u64; ((TOTAL_COUNT + 63) / 64) as usize],
}

impl DefMap {
    fn new() -> DefMap {
        DefMap { map: HashMap::new(), phi_masks: [0; ((TOTAL_COUNT + 63) / 64) as usize] }
    }

    fn try_add_operand(&mut self, reg: Register, operand: OperandId) -> bool {
        if self.map.contains_key(&reg) {
            return false;
        }
        self.map.insert(reg, operand);
        true
    }

    fn add_phi(&mut self, reg: Register) -> bool {
        let key = key_from_register(reg);
        let (index, mask) = ((key / 64) as usize, 1u64 << (key & 63));
        if self.phi_masks[index] & mask != 0 {
            return false;
        }
        self.phi_masks[index] |= mask;
        true
    }

    fn has_phi(&self, reg: Register) -> bool {
        let key = key_from_register(reg);
        self.phi_masks[(key / 64) as usize] & (1u64 << (key & 63)) != 0
    }
}

struct LocalDefMap {
    map: Vec<Option<OperandId>>,
    uses: Vec<i32>,
    use_count: usize,
}

impl LocalDefMap {
    fn new() -> LocalDefMap {
        LocalDefMap { map: vec![None; TOTAL_COUNT as usize], uses: vec![0; TOTAL_COUNT as usize], use_count: 0 }
    }

    fn get(&self, key: i32) -> Option<OperandId> {
        self.map[key as usize]
    }

    fn add(&mut self, key: i32, operand: OperandId) {
        if self.map[key as usize].is_none() {
            self.uses[self.use_count] = key;
            self.use_count += 1;
        }
        self.map[key as usize] = Some(operand);
    }

    fn clear(&mut self) {
        for i in 0..self.use_count {
            self.map[self.uses[i] as usize] = None;
        }
        self.use_count = 0;
    }
}

fn is_register(ir: &Ir, o: Option<OperandId>) -> bool {
    o.is_some_and(|o| ir.ty(o) == OperandType::Register)
}

pub fn rename(ir: &mut Ir, blocks: &[BlockId]) {
    let mut global_defs: Vec<DefMap> = (0..blocks.len()).map(|_| DefMap::new()).collect();
    let mut local_defs = LocalDefMap::new();
    let mut df_phi_blocks: VecDeque<BlockId> = VecDeque::new();

    // First pass: every block's definitions and local uses.
    for &block in blocks {
        for node in ir.block_nodes(block) {
            if !ir.is_op(node) {
                continue;
            }
            for index in 0..ir.sources_count(node) {
                let src = ir.get_source_opt(node, index);
                let renamed = if is_register(ir, src) {
                    local_defs.get(key_from_register(ir.opnd(src.unwrap()).register())).or(src)
                } else {
                    src
                };
                ir.set_source(node, index, renamed);
            }
            for index in 0..ir.dests_count(node) {
                let dest = ir.get_dest(node, index);
                if is_register(ir, dest) {
                    let local = ir.local();
                    local_defs.add(key_from_register(ir.opnd(dest.unwrap()).register()), local);
                    ir.set_dest(node, index, Some(local));
                }
            }
        }
        for index in 0..local_defs.use_count {
            let key = local_defs.uses[index];
            let local = local_defs.map[key as usize].unwrap();
            let reg = register_from_key(key);
            global_defs[ir.block(block).index as usize].try_add_operand(reg, local);
            df_phi_blocks.push_back(block);
            while let Some(df_phi_block) = df_phi_blocks.pop_front() {
                let frontiers = ir.block(df_phi_block).dominance_frontiers.to_vec();
                for dom_frontier in frontiers {
                    if global_defs[ir.block(dom_frontier).index as usize].add_phi(reg) {
                        df_phi_blocks.push_back(dom_frontier);
                    }
                }
            }
        }
        local_defs.clear();
    }

    // Second pass: uses of definitions from other blocks.
    for (blk_index, &block) in blocks.iter().enumerate() {
        let mut node = ir.first_node(block);
        while let Some(n) = node {
            if ir.is_op(n) {
                for index in 0..ir.sources_count(n) {
                    let src = ir.get_source_opt(n, index);
                    let renamed = if is_register(ir, src) {
                        let reg = ir.opnd(src.unwrap()).register();
                        let key = key_from_register(reg);
                        match local_defs.get(key) {
                            Some(l) => Some(l),
                            None => {
                                let d = find_definition_for_curr(ir, &mut global_defs, block, reg);
                                local_defs.add(key, d);
                                Some(d)
                            }
                        }
                    } else {
                        src
                    };
                    ir.set_source(n, index, renamed);
                }
            }
            node = ir.next_node(n);
        }
        if blk_index < blocks.len() - 1 {
            local_defs.clear();
        }
    }
}

fn find_definition_for_curr(ir: &mut Ir, global_defs: &mut [DefMap], current: BlockId, reg: Register) -> OperandId {
    if global_defs[ir.block(current).index as usize].has_phi(reg) {
        return insert_phi(ir, global_defs, current, reg);
    }
    let idom = ir.block(current).immediate_dominator.unwrap();
    if current != idom {
        return find_definition(ir, global_defs, idom, reg).1;
    }
    ir.undef()
}

fn find_definition(ir: &mut Ir, global_defs: &mut [DefMap], current: BlockId, reg: Register) -> (BlockId, OperandId) {
    // The block and its immediate dominators, up to the entry.
    let mut block = current;
    loop {
        let index = ir.block(block).index as usize;
        if let Some(&last_def) = global_defs[index].map.get(&reg) {
            return (block, last_def);
        }
        if global_defs[index].has_phi(reg) {
            let l = insert_phi(ir, global_defs, block, reg);
            return (block, l);
        }
        let idom = ir.block(block).immediate_dominator.unwrap();
        if block == idom {
            break;
        }
        block = idom;
    }
    (current, ir.undef())
}

fn insert_phi(ir: &mut Ir, global_defs: &mut [DefMap], block: BlockId, reg: Register) -> OperandId {
    // Materialise the phi that defines a new version of the variable here, with every predecessor's definition.
    let local = ir.local();
    let phi = ir.phi(local);
    add_phi(ir, block, phi);
    global_defs[ir.block(block).index as usize].try_add_operand(reg, local);
    let predecessors = ir.block(block).predecessors.clone();
    for predecessor in predecessors {
        let (def_block, def_local) = find_definition(ir, global_defs, predecessor, reg);
        ir.phi_add_source(phi, def_block, def_local);
    }
    local
}

fn add_phi(ir: &mut Ir, block: BlockId, phi: NodeId) {
    let mut node = ir.first_node(block);
    if let Some(mut n) = node {
        while let Some(next) = ir.next_node(n).filter(|&x| ir.is_phi(x)) {
            n = next;
        }
        node = Some(n);
    }
    match node {
        Some(n) if ir.is_phi(n) => {
            ir.add_after(n, phi);
        }
        _ => ir.add_first(block, phi),
    }
}

fn key_from_register(reg: Register) -> i32 {
    match reg.ty {
        RegisterType::Gpr => reg.index,
        RegisterType::Predicate => GPRS_COUNT + reg.index,
        _ => GPRS_AND_PREDS_COUNT + reg.index,
    }
}

fn register_from_key(key: i32) -> Register {
    if key < GPRS_COUNT {
        Register::new(key, RegisterType::Gpr)
    } else if key < GPRS_AND_PREDS_COUNT {
        Register::new(key - GPRS_COUNT, RegisterType::Predicate)
    } else {
        Register::new(key - GPRS_AND_PREDS_COUNT, RegisterType::Flag)
    }
}
