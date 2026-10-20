// Maxwell code decoding into functions of basic blocks (Ryujinx.Graphics.Shader.Decoders).

mod decoder;
pub mod inst_decoders;
mod inst_name;
mod inst_table_data;
pub mod register;

pub use decoder::{decode, is_pop_branch, is_unconditional_branch};
pub use inst_name::InstName;

use crate::collections::{NetMap, NetSet};
use crate::instructions::InstEmitter;
use crate::ir::OperandId;
use crate::translation::definitions::{AttributeUsage, FeatureFlags, IoUsage};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct InstProps(pub u16);

impl InstProps {
    pub const NONE: Self = Self(0);
    pub const RD: Self = Self(1 << 0);
    pub const RD2: Self = Self(1 << 1);
    pub const RA: Self = Self(1 << 2);
    pub const RB: Self = Self(1 << 3);
    pub const RB2: Self = Self(1 << 4);
    pub const IB: Self = Self(1 << 5);
    pub const RC: Self = Self(1 << 6);
    pub const PD: Self = Self(1 << 7);
    pub const L_PD: Self = Self(2 << 7);
    pub const S_PD: Self = Self(3 << 7);
    pub const T_PD: Self = Self(4 << 7);
    pub const V_PD: Self = Self(5 << 7);
    pub const PD_MASK: Self = Self(7 << 7);
    pub const PDN: Self = Self(1 << 10);
    pub const PS: Self = Self(1 << 11);
    pub const TEX: Self = Self(1 << 12);
    pub const TEX_B: Self = Self(1 << 13);
    pub const BRA: Self = Self(1 << 14);
    pub const NO_PRED: Self = Self(1 << 15);

    /// Enum.HasFlag: every bit of `f` is set.
    pub fn has(self, f: Self) -> bool {
        self.0 & f.0 == f.0
    }
}

impl std::ops::BitOr for InstProps {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

#[derive(Clone, Copy)]
pub struct InstOp {
    pub address: u64,
    pub raw_op_code: u64,
    pub emitter: Option<InstEmitter>,
    pub props: InstProps,
    pub name: InstName,
}

impl InstOp {
    pub fn absolute_address(&self) -> u64 {
        (self.address as i64 + ((((self.raw_op_code >> 20) as i32) << 8) >> 8) as i64 + 8) as u64
    }
}

impl Default for InstOp {
    fn default() -> Self {
        InstOp { address: 0, raw_op_code: 0, emitter: None, props: InstProps::NONE, name: InstName::Invalid }
    }
}

const ENCODING_BITS: u32 = 14;

#[derive(Clone, Copy)]
struct TableEntry {
    name: InstName,
    emitter: Option<InstEmitter>,
    props: InstProps,
    x_bits: u32,
}

fn table() -> &'static [TableEntry] {
    static TABLE: OnceLock<Vec<TableEntry>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let empty = TableEntry { name: InstName::Invalid, emitter: None, props: InstProps::NONE, x_bits: 0 };
        let mut t = vec![empty; 1 << ENCODING_BITS];
        for &(encoding, name, emitter, props) in inst_table_data::ENTRIES {
            let mut value = 0usize;
            let mut x_mask = 0usize;
            let mut x_pos = Vec::new();
            for (index, c) in encoding.chars().enumerate() {
                let bit = ENCODING_BITS as usize - 1 - index;
                match c {
                    '1' => value |= 1 << bit,
                    'x' => {
                        x_mask |= 1 << bit;
                        x_pos.push(bit);
                    }
                    _ => {}
                }
            }
            let x_bits = x_pos.len() as u32;
            let entry = TableEntry { name, emitter: Some(emitter), props, x_bits };
            for index in 0..1usize << x_bits {
                value &= !x_mask;
                for (x, &pos) in x_pos.iter().enumerate() {
                    value |= ((index >> x) & 1) << pos;
                }
                if t[value].emitter.is_none() || t[value].x_bits > x_bits {
                    t[value] = entry;
                }
            }
        }
        t
    })
}

pub fn get_op(address: u64, op_code: u64) -> InstOp {
    let e = table()[(op_code >> (64 - ENCODING_BITS)) as usize];
    match e.emitter {
        Some(_) => InstOp { address, raw_op_code: op_code, emitter: e.emitter, props: e.props, name: e.name },
        None => InstOp { address, raw_op_code: op_code, ..InstOp::default() },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockIdx(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PushOpIdx(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FunctionIdx(pub u32);

pub struct PushOpInfo {
    pub op: InstOp,
    pub consumers: NetMap<BlockIdx, OperandId>,
}

#[derive(Clone, Copy)]
pub struct SyncTarget {
    pub push_op_info: PushOpIdx,
    pub push_op_id: i32,
}

pub struct Block {
    pub address: u64,
    pub end_address: u64,
    pub predecessors: Vec<BlockIdx>,
    pub successors: Vec<BlockIdx>,
    pub op_codes: Vec<InstOp>,
    pub push_op_codes: Vec<PushOpIdx>,
    pub sync_targets: NetMap<u64, SyncTarget>,
}

impl Block {
    fn new(address: u64) -> Block {
        Block {
            address,
            end_address: 0,
            predecessors: Vec::new(),
            successors: Vec::new(),
            op_codes: Vec::new(),
            push_op_codes: Vec::new(),
            sync_targets: NetMap::new(),
        }
    }

    pub fn last_op(&self) -> InstOp {
        self.op_codes.last().copied().unwrap_or_default()
    }

    pub fn has_next(&self) -> bool {
        !self.op_codes.is_empty() && !is_unconditional_branch(&self.last_op())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionType {
    User,
    Unused,
    BuiltInFsiBegin,
    BuiltInFsiEnd,
}

pub struct DecodedFunction {
    callers: NetSet<FunctionIdx>,
    pub ty: FunctionType,
    pub id: i32,
    pub address: u64,
    pub blocks: Vec<BlockIdx>,
}

impl DecodedFunction {
    pub fn is_compiler_generated(&self) -> bool {
        self.ty != FunctionType::User
    }
}

pub struct DecodedProgram {
    pub blocks: Vec<Block>,
    pub push_ops: Vec<PushOpInfo>,
    pub functions: Vec<DecodedFunction>,
    pub main_function: FunctionIdx,
    functions_with_id: Vec<FunctionIdx>,
    pub attribute_usage: AttributeUsage,
    pub used_features: FeatureFlags,
    pub clip_distances_written: u8,
    pub cb1_data_size: i32,
}

impl DecodedProgram {
    pub fn block(&self, b: BlockIdx) -> &Block {
        &self.blocks[b.0 as usize]
    }

    pub fn block_mut(&mut self, b: BlockIdx) -> &mut Block {
        &mut self.blocks[b.0 as usize]
    }

    pub fn push_op(&self, p: PushOpIdx) -> &PushOpInfo {
        &self.push_ops[p.0 as usize]
    }

    pub fn function(&self, f: FunctionIdx) -> &DecodedFunction {
        &self.functions[f.0 as usize]
    }

    pub fn function_mut(&mut self, f: FunctionIdx) -> &mut DecodedFunction {
        &mut self.functions[f.0 as usize]
    }

    /// Functions in the order they were found (the original enumerates its address dictionary).
    pub fn function_indices(&self) -> impl Iterator<Item = FunctionIdx> {
        (0..self.functions.len() as u32).map(FunctionIdx)
    }

    pub fn function_by_address(&self, address: u64) -> Option<FunctionIdx> {
        self.function_indices().find(|&f| self.function(f).address == address)
    }

    pub fn functions_with_id_count(&self) -> usize {
        self.functions_with_id.len()
    }

    pub fn function_by_id(&self, id: i32) -> FunctionIdx {
        self.functions_with_id[id as usize]
    }

    pub fn add_function_and_set_id(&mut self, f: FunctionIdx) {
        self.function_mut(f).id = self.functions_with_id.len() as i32;
        self.functions_with_id.push(f);
    }

    pub fn add_caller(&mut self, f: FunctionIdx, caller: FunctionIdx) {
        self.function_mut(f).callers.insert(caller);
    }

    pub fn remove_caller(&mut self, f: FunctionIdx, caller: FunctionIdx) {
        let func = self.function_mut(f);
        if func.callers.remove(&caller) && func.callers.is_empty() {
            func.ty = FunctionType::Unused;
        }
    }

    pub fn io_usage(&self) -> IoUsage {
        IoUsage::new(self.used_features, self.clip_distances_written, self.attribute_usage.used_output_attributes)
    }
}
