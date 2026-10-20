use super::block::Block;
use super::inst_decoders::{InstAld, InstAst, InstBrx, InstConditional, InstImnmxI, InstIpa, InstLdc, InstShlI, Ccc};
use super::inst_name::InstName;
use super::inst_op::InstOp;
use super::inst_props::InstProps;
use super::inst_table::get_op;
use super::register::RegisterConsts;
use crate::gpu_accessor::MemoryGpuAccessor;
use bitflags::bitflags;
use rustc_hash::FxHashSet;
use std::collections::VecDeque;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct FeatureFlags: u32 {
        const NONE = 0;
        const BINDLESS = 1 << 0;
        const SHUFFLE = 1 << 1;
        const LOCAL_MEMORY = 1 << 2;
        const SHARED_MEMORY = 1 << 3;
        const STORE = 1 << 4;
        const FRAG_COORD_XY = 1 << 5;
        const INSTANCE_ID = 1 << 6;
        const DRAW_PARAMETERS = 1 << 7;
        const RT_LAYER = 1 << 8;
        const VTG_AS_COMPUTE = 1 << 9;
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct AttributeUsage {
    pub input_attributes: u32,
    pub output_attributes: u32,
}

impl AttributeUsage {
    pub fn set_input_user_attribute(&mut self, index: usize) {
        if index < 32 {
            self.input_attributes |= 1 << index;
        }
    }

    pub fn set_output_user_attribute(&mut self, index: usize) {
        if index < 32 {
            self.output_attributes |= 1 << index;
        }
    }

    pub fn set_all_input_user_attributes(&mut self) {
        self.input_attributes = u32::MAX;
    }

    pub fn set_all_output_user_attributes(&mut self) {
        self.output_attributes = u32::MAX;
    }
}

pub struct DecoderContext<'a> {
    pub accessor: &'a MemoryGpuAccessor,
    pub attribute_usage: AttributeUsage,
    pub used_features: FeatureFlags,
    pub clip_distances_written: u8,
    pub cb1_data_size: i32,
}

impl<'a> DecoderContext<'a> {
    pub fn new(accessor: &'a MemoryGpuAccessor) -> Self {
        Self {
            accessor,
            attribute_usage: AttributeUsage::default(),
            used_features: FeatureFlags::NONE,
            clip_distances_written: 0,
            cb1_data_size: 0,
        }
    }

    pub fn constant_buffer1_read(&mut self, offset: i32) -> u32 {
        if self.cb1_data_size < offset + 4 {
            self.cb1_data_size = offset + 4;
        }
        self.accessor.constant_buffer1_read(offset)
    }

    pub fn set_used_feature(&mut self, flags: FeatureFlags) {
        self.used_features |= flags;
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeType {
    Sync,
    Brk,
    Cont,
}

#[allow(dead_code)]
fn get_merge_type_from_push(name: InstName) -> MergeType {
    match name {
        InstName::Pbk => MergeType::Brk,
        InstName::Pcnt => MergeType::Cont,
        _ => MergeType::Sync,
    }
}

#[allow(dead_code)]
fn get_merge_type_from_pop(name: InstName) -> MergeType {
    match name {
        InstName::Brk => MergeType::Brk,
        InstName::Cont => MergeType::Cont,
        _ => MergeType::Sync,
    }
}

#[allow(dead_code)]
fn is_pop_branch(name: InstName) -> bool {
    matches!(name, InstName::Brk | InstName::Cont | InstName::Sync)
}

pub struct DecodedProgram {
    pub blocks: Vec<Block>,
    pub attribute_usage: AttributeUsage,
    pub used_features: FeatureFlags,
    pub clip_distances_written: u8,
    pub cb1_data_size: i32,
    pub max_end_address: u64,
}

pub fn decode(accessor: &MemoryGpuAccessor, start_address: u64, _is_compute: bool) -> DecodedProgram {
    let mut context = DecoderContext::new(accessor);
    let mut blocks: Vec<Block> = Vec::new();
    let mut work_queue: VecDeque<u64> = VecDeque::new();
    let mut visited: FxHashSet<u64> = FxHashSet::default();

    work_queue.push_back(0);
    visited.insert(0);

    loop {
        while let Some(curr_address) = work_queue.pop_front() {
            // Check if curr_address is inside an existing block
            if let Some(idx) = find_block_covering(&blocks, curr_address) {
                if blocks[idx].address == curr_address {
                    continue;
                }
                // Split existing block
                let right_block = split_block(&mut blocks[idx], curr_address);
                blocks.insert(idx + 1, right_block);
                continue;
            }

            let mut curr_block = Block::new(curr_address);
            let limit_address = find_limit_address(&blocks, curr_address);

            fill_block(accessor, &mut context, &mut curr_block, limit_address, start_address);

            if !curr_block.op_codes.is_empty() {
                for push_op in &curr_block.push_op_codes {
                    let target_addr = push_op.op.get_absolute_address();
                    if visited.insert(target_addr) {
                        work_queue.push_back(target_addr);
                    }
                }

                if let Some(last_op) = curr_block.get_last_op() {
                    if last_op.name == InstName::Bra {
                        let target_addr = last_op.get_absolute_address();
                        if visited.insert(target_addr) {
                            work_queue.push_back(target_addr);
                        }
                    }

                    if !is_unconditional(&last_op) {
                        let next_addr = curr_block.end_address;
                        if visited.insert(next_addr) {
                            work_queue.push_back(next_addr);
                        }
                    }
                }
            }

            // Insert into sorted block list
            let insert_idx = blocks
                .binary_search_by_key(&curr_block.address, |b| b.address)
                .unwrap_or_else(|idx| idx);
            blocks.insert(insert_idx, curr_block);
        }

        // Try to resolve BRX indirect branches
        let has_new_target = find_brx_targets(&mut context, &mut blocks, &mut work_queue, &mut visited);

        if !has_new_target && work_queue.is_empty() {
            break;
        }
    }

    let mut max_end_address = 0u64;
    for b in &blocks {
        if b.end_address > max_end_address {
            max_end_address = b.end_address;
        }
    }

    DecodedProgram {
        blocks,
        attribute_usage: context.attribute_usage,
        used_features: context.used_features,
        clip_distances_written: context.clip_distances_written,
        cb1_data_size: context.cb1_data_size,
        max_end_address,
    }
}

fn find_block_covering(blocks: &[Block], address: u64) -> Option<usize> {
    for (i, b) in blocks.iter().enumerate() {
        if address >= b.address && address < b.end_address {
            return Some(i);
        }
    }
    None
}

fn find_limit_address(blocks: &[Block], address: u64) -> u64 {
    for b in blocks {
        if b.address > address {
            return b.address;
        }
    }
    u64::MAX
}

fn split_block(block: &mut Block, right_address: u64) -> Block {
    let split_index = block
        .op_codes
        .binary_search_by_key(&right_address, |op| op.address)
        .unwrap_or_else(|idx| idx);

    let mut right_block = Block::new(right_address);
    right_block.end_address = block.end_address;
    block.end_address = right_address;

    right_block.op_codes = block.op_codes.split_off(split_index);

    // Split push ops
    let mut remaining_push = Vec::new();
    for p in block.push_op_codes.drain(..) {
        if p.op.address >= right_address {
            right_block.push_op_codes.push(p);
        } else {
            remaining_push.push(p);
        }
    }
    block.push_op_codes = remaining_push;

    right_block
}

fn fill_block(
    accessor: &MemoryGpuAccessor,
    context: &mut DecoderContext,
    block: &mut Block,
    limit_address: u64,
    start_address: u64,
) {
    let mut address = block.address;
    let mut buffer = accessor.get_code(start_address + address);
    let mut buffer_offset = 0;

    loop {
        if address + 7 >= limit_address {
            break;
        }

        // Skip 8-byte control code every 32 bytes
        if (address & 0x1f) == 0 {
            address += 8;
            buffer_offset += 1;
            continue;
        }

        if buffer_offset >= buffer.len() {
            buffer = accessor.get_code(start_address + address);
            buffer_offset = 0;
            if buffer.is_empty() {
                break;
            }
        }

        let op_code = buffer[buffer_offset];
        buffer_offset += 1;

        let op = get_op(address, op_code);

        if op.props.contains(InstProps::TEX_B) {
            context.set_used_feature(FeatureFlags::BINDLESS);
        }

        match op.name {
            InstName::Ald | InstName::Ast | InstName::Ipa => {
                set_user_attribute_uses(context, op.name, op_code);
            }
            InstName::Pbk | InstName::Pcnt | InstName::Ssy => {
                block.add_push_op(op);
            }
            InstName::Shfl => {
                context.set_used_feature(FeatureFlags::SHUFFLE);
            }
            InstName::Ldl | InstName::Stl => {
                context.set_used_feature(FeatureFlags::LOCAL_MEMORY);
            }
            InstName::Atoms | InstName::AtomsCas | InstName::Lds | InstName::Sts => {
                context.set_used_feature(FeatureFlags::SHARED_MEMORY);
            }
            InstName::Atom
            | InstName::AtomCas
            | InstName::Red
            | InstName::Stg
            | InstName::Suatom
            | InstName::SuatomB
            | InstName::SuatomB2
            | InstName::SuatomCas
            | InstName::SuatomCasB
            | InstName::Sured
            | InstName::SuredB
            | InstName::Sust
            | InstName::SustB
            | InstName::SustD
            | InstName::SustDB => {
                context.set_used_feature(FeatureFlags::STORE);
            }
            _ => {}
        }

        block.op_codes.push(op);
        address += 8;

        if op.props.contains(InstProps::BRA) {
            break;
        }
    }

    block.end_address = address;
}

fn set_user_attribute_uses(context: &mut DecoderContext, name: InstName, op_code: u64) {
    let (offset, count, is_store, indexed) = match name {
        InstName::Ast => {
            let op = InstAst::new(op_code);
            (op.imm11() as usize, op.al_size() as usize + 1, true, op.phys())
        }
        InstName::Ald => {
            let op = InstAld::new(op_code);
            (op.imm11() as usize, op.al_size() as usize + 1, op.o(), op.phys())
        }
        _ => {
            let op = InstIpa::new(op_code);
            (op.imm10() as usize, 1, false, op.idx())
        }
    };

    if indexed {
        if is_store {
            context.attribute_usage.set_all_output_user_attributes();
        } else {
            context.attribute_usage.set_all_input_user_attributes();
        }
    } else {
        for elem in 0..count {
            let attr = offset + elem * 4;
            // Standard UserAttributeBase = 0x80, UserAttributeEnd = 0x280 (up to 32 attributes of 16 bytes each)
            const USER_ATTR_BASE: usize = 0x80;
            const USER_ATTR_END: usize = 0x280;
            if attr >= USER_ATTR_BASE && attr < USER_ATTR_END {
                let user_attr = attr - USER_ATTR_BASE;
                let index = user_attr / 16;
                if is_store {
                    context.attribute_usage.set_output_user_attribute(index);
                } else {
                    context.attribute_usage.set_input_user_attribute(index);
                }
            }
        }
    }
}

fn is_unconditional(op: &InstOp) -> bool {
    let cond = InstConditional::new(op.raw_op_code);
    if (op.name == InstName::Bra || op.name == InstName::Exit) && cond.ccc() != Ccc::T {
        return false;
    }
    cond.pred() == RegisterConsts::PREDICATE_TRUE_INDEX as i32 && !cond.pred_inv()
}

fn find_brx_targets(
    context: &mut DecoderContext,
    blocks: &mut [Block],
    work_queue: &mut VecDeque<u64>,
    visited: &mut FxHashSet<u64>,
) -> bool {
    let mut has_new_target = false;
    for block in blocks.iter_mut() {
        if let Some(last_op) = block.get_last_op() {
            if last_op.name == InstName::Brx {
                let op_brx = InstBrx::new(last_op.raw_op_code);
                let base_offset = last_op.get_absolute_address();
                let (cb_base, cb_count) = find_brx_target_range(block, op_brx.src_a());
                for i in 0..cb_count {
                    let target_offset = context.constant_buffer1_read(cb_base + i * 4);
                    let target_address = base_offset + target_offset as u64;
                    if !block.brx_targets.contains(&target_address) {
                        block.brx_targets.push(target_address);
                    }
                    if visited.insert(target_address) {
                        work_queue.push_back(target_address);
                        has_new_target = true;
                    }
                }
            }
        }
    }
    has_new_target
}

fn find_brx_target_range(block: &Block, brx_reg: i32) -> (i32, i32) {
    // Matches the standard compiler jump table pattern:
    // IMNMX.U32 Rx, Rx, UpperBound, PT
    // SHL Rx, Rx, 0x2
    // LDC Rx, c[0x1][Rx+BaseOffset]
    let mut ldc_op: Option<InstLdc> = None;
    let mut shl_reg = -1;

    for op in block.op_codes.iter().rev() {
        if op.name == InstName::Ldc {
            let ldc = InstLdc::new(op.raw_op_code);
            if ldc.dest() == brx_reg && ldc.cbuf_slot() == 1 {
                shl_reg = ldc.src_a();
                ldc_op = Some(ldc);
                break;
            }
        }
    }

    let Some(ldc) = ldc_op else { return (0, 0) };

    let mut imnmx_reg = -1;
    for op in block.op_codes.iter().rev() {
        if op.name == InstName::Shl {
            let shl = InstShlI::new(op.raw_op_code);
            if shl.dest() == shl_reg && shl.imm20() == 2 {
                imnmx_reg = shl.src_a();
                break;
            }
        }
    }

    if imnmx_reg < 0 {
        return (0, 0);
    }

    for op in block.op_codes.iter().rev() {
        if op.name == InstName::Imnmx {
            let imnmx = InstImnmxI::new(op.raw_op_code);
            if imnmx.dest() == imnmx_reg {
                let count = (imnmx.imm20() + 1).max(1);
                return (ldc.cbuf_offset(), count);
            }
        }
    }

    (0, 0)
}
