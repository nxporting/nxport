// Finds a program's functions and basic blocks: follows branches, calls, the SSY/PBK/PCNT push targets their
// SYNC/BRK/CONT pops reach, and indirect branch (BRX) jump tables in constant buffer 1.

use std::collections::VecDeque;

use super::inst_decoders::*;
use super::register::PREDICATE_TRUE_INDEX;
use super::*;
use crate::accessor::GpuAccessor;
use crate::ir::Ir;
use crate::translation::definitions::{attribute_consts as ac, ShaderDefinitions};
use crate::types::ShaderStage;

struct Context {
    attribute_usage: AttributeUsage,
    used_features: FeatureFlags,
    clip_distances_written: u8,
    cb1_data_size: i32,
}

impl Context {
    fn constant_buffer1_read(&mut self, gpu: &GpuAccessor, offset: i32) -> u32 {
        if self.cb1_data_size < offset + 4 {
            self.cb1_data_size = offset + 4;
        }
        gpu.constant_buffer1_read(offset)
    }
}

struct State<'a> {
    blocks: Vec<Block>,
    push_ops: Vec<PushOpInfo>,
    ir: &'a mut Ir,
}

impl State<'_> {
    fn b(&self, b: BlockIdx) -> &Block {
        &self.blocks[b.0 as usize]
    }

    fn bm(&mut self, b: BlockIdx) -> &mut Block {
        &mut self.blocks[b.0 as usize]
    }

    fn new_block(&mut self, address: u64) -> BlockIdx {
        self.blocks.push(Block::new(address));
        BlockIdx(self.blocks.len() as u32 - 1)
    }

    /// Block.Split: moves the ops from `right`'s address on into `right`.
    fn split(&mut self, left: BlockIdx, right: BlockIdx) -> Result<(), String> {
        let right_address = self.b(right).address;
        let ops = &self.b(left).op_codes;
        let mut split_index = op_binary_search(ops, right_address);
        if ops[split_index].address < right_address {
            split_index += 1;
        }
        let split_count = ops.len() as i64 - split_index as i64;
        if split_count <= 0 {
            return Err("Can't split at right block address.".into());
        }
        let end = self.b(left).end_address;
        let successors = self.b(left).successors.clone();
        {
            let r = self.bm(right);
            r.end_address = end;
            r.successors.extend(successors);
            r.predecessors.push(left);
        }
        let moved: Vec<InstOp> = self.bm(left).op_codes.drain(split_index..).collect();
        {
            let l = self.bm(left);
            l.end_address = right_address;
            l.successors.clear();
            l.successors.push(right);
        }
        self.bm(right).op_codes.extend(moved);

        let targets: Vec<(u64, SyncTarget)> = self.b(left).sync_targets.iter().map(|(&k, &v)| (k, v)).collect();
        for &(_, t) in &targets {
            let consumers = &mut self.push_ops[t.push_op_info.0 as usize].consumers;
            let local = *consumers.get(&left).unwrap();
            consumers.remove(&left);
            assert!(consumers.try_add(right, local), "duplicate push consumer");
        }
        for (k, v) in targets {
            assert!(self.bm(right).sync_targets.try_add(k, v), "duplicate sync target");
        }
        self.bm(left).sync_targets.clear();

        let pos = self.b(left).push_op_codes.iter().position(|&p| self.push_ops[p.0 as usize].op.address >= right_address);
        if let Some(i) = pos {
            let moved: Vec<PushOpIdx> = self.bm(left).push_op_codes.drain(i..).collect();
            self.bm(right).push_op_codes.extend(moved);
        }
        Ok(())
    }
}

fn op_binary_search(ops: &[InstOp], address: u64) -> usize {
    let (mut left, mut middle, mut right) = (0i64, 0i64, ops.len() as i64 - 1);
    while left <= right {
        middle = left + ((right - left) >> 1);
        let a = ops[middle as usize].address;
        if address == a {
            break;
        }
        if address < a {
            right = middle - 1;
        } else {
            left = middle + 1;
        }
    }
    middle as usize
}

/// Decoder.Decode.
pub fn decode(definitions: &mut ShaderDefinitions, gpu: &GpuAccessor, ir: &mut Ir, start_address: u64) -> Result<DecodedProgram, String> {
    let mut context = Context {
        attribute_usage: AttributeUsage::default(),
        used_features: FeatureFlags::NONE,
        clip_distances_written: 0,
        cb1_data_size: 0,
    };
    let mut st = State { blocks: Vec::new(), push_ops: Vec::new(), ir };
    let mut functions: Vec<DecodedFunction> = Vec::new();
    let mut functions_queue: VecDeque<FunctionIdx> = VecDeque::new();

    let mut enqueue_function = |functions: &mut Vec<DecodedFunction>, queue: &mut VecDeque<FunctionIdx>, address: u64| -> FunctionIdx {
        if let Some(i) = functions.iter().position(|f| f.address == address) {
            return FunctionIdx(i as u32);
        }
        functions.push(DecodedFunction {
            callers: NetSet::new(),
            ty: FunctionType::User,
            id: -1,
            address,
            blocks: Vec::new(),
        });
        let f = FunctionIdx(functions.len() as u32 - 1);
        queue.push_back(f);
        f
    };

    let main_function = enqueue_function(&mut functions, &mut functions_queue, 0);

    while let Some(current_function) = functions_queue.pop_front() {
        let mut blocks: Vec<BlockIdx> = Vec::new();
        let mut work_queue: VecDeque<BlockIdx> = VecDeque::new();
        let mut visited: NetMap<u64, BlockIdx> = NetMap::new();

        let get_block = |st: &mut State, visited: &mut NetMap<u64, BlockIdx>, wq: &mut VecDeque<BlockIdx>, address: u64| -> BlockIdx {
            if let Some(&b) = visited.get(&address) {
                return b;
            }
            let b = st.new_block(address);
            wq.push_back(b);
            visited.try_add(address, b);
            b
        };

        let fa = functions[current_function.0 as usize].address;
        get_block(&mut st, &mut visited, &mut work_queue, fa);

        loop {
            while let Some(curr) = work_queue.pop_front() {
                // Inside another block?
                let (found, n_blk_index) = block_binary_search(&st, &blocks, st.b(curr).address);
                if found {
                    let n_block = blocks[n_blk_index];
                    if st.b(n_block).address == st.b(curr).address {
                        return Err("Found duplicate block address on the list.".into());
                    }
                    st.split(n_block, curr)?;
                    blocks.insert(n_blk_index + 1, curr);
                    continue;
                }

                // A block after this one limits it.
                let mut limit_address = u64::MAX;
                if n_blk_index != blocks.len() {
                    let n_addr = st.b(blocks[n_blk_index]).address;
                    let next_index = n_blk_index + 1;
                    let curr_addr = st.b(curr).address;
                    if n_addr < curr_addr && next_index < blocks.len() {
                        limit_address = st.b(blocks[next_index]).address;
                    } else if n_addr > curr_addr {
                        limit_address = n_addr;
                    }
                }

                fill_block(definitions, gpu, &mut context, &mut st, curr, limit_address, start_address)?;

                if !st.b(curr).op_codes.is_empty() {
                    // Blocks for every branch target, including SSY/PBK/PCNT targets.
                    let push_targets: Vec<u64> =
                        st.b(curr).push_op_codes.iter().map(|&p| st.push_ops[p.0 as usize].op.absolute_address()).collect();
                    for a in push_targets {
                        get_block(&mut st, &mut visited, &mut work_queue, a);
                    }

                    // Branch: the block a taken branch goes to; next: the block at the next address.
                    let last_op = st.b(curr).last_op();
                    if last_op.name == InstName::Cal {
                        let f = enqueue_function(&mut functions, &mut functions_queue, last_op.absolute_address());
                        functions[f.0 as usize].callers.insert(current_function);
                    } else if last_op.name == InstName::Bra {
                        let succ = get_block(&mut st, &mut visited, &mut work_queue, last_op.absolute_address());
                        st.bm(curr).successors.push(succ);
                        st.bm(succ).predecessors.push(curr);
                    }

                    if !is_unconditional_branch(&last_op) {
                        let end = st.b(curr).end_address;
                        let succ = get_block(&mut st, &mut visited, &mut work_queue, end);
                        st.bm(curr).successors.insert(0, succ);
                        st.bm(succ).predecessors.push(curr);
                    }
                }

                // Insert the new block into the list sorted by address.
                if !blocks.is_empty() {
                    let n_block = blocks[n_blk_index];
                    let after = st.b(n_block).address < st.b(curr).address;
                    blocks.insert(n_blk_index + after as usize, curr);
                } else {
                    blocks.push(curr);
                }
            }

            // Propagate SSY/PBK addresses into their uses (SYNC/BRK).
            let with_push: Vec<BlockIdx> = blocks.iter().copied().filter(|&b| !st.b(b).push_op_codes.is_empty()).collect();
            for block in with_push {
                let mut push_op_index = 0;
                while push_op_index < st.b(block).push_op_codes.len() {
                    propagate_push_op(&mut st, &visited, block, push_op_index);
                    push_op_index += 1;
                }
            }

            // Targets of indirect branches: new targets need another round of decoding.
            let has_new_target = find_brx_targets(gpu, &mut context, &mut st, &blocks, &mut visited, &mut work_queue, &get_block);
            if !has_new_target {
                break;
            }
        }

        functions[current_function.0 as usize].blocks = blocks;
    }

    Ok(DecodedProgram {
        blocks: st.blocks,
        push_ops: st.push_ops,
        functions,
        main_function,
        functions_with_id: Vec::new(),
        attribute_usage: context.attribute_usage,
        used_features: context.used_features,
        clip_distances_written: context.clip_distances_written,
        cb1_data_size: context.cb1_data_size,
    })
}

fn block_binary_search(st: &State, blocks: &[BlockIdx], address: u64) -> (bool, usize) {
    let mut index = 0;
    let (mut left, mut right) = (0i64, blocks.len() as i64 - 1);
    while left <= right {
        let middle = left + ((right - left) >> 1);
        let b = st.b(blocks[middle as usize]);
        index = middle as usize;
        if address >= b.address && address < b.end_address {
            return (true, index);
        }
        if address < b.address {
            right = middle - 1;
        } else {
            left = middle + 1;
        }
    }
    (false, index)
}

fn fill_block(
    definitions: &mut ShaderDefinitions,
    gpu: &GpuAccessor,
    context: &mut Context,
    st: &mut State,
    block: BlockIdx,
    limit_address: u64,
    start_address: u64,
) -> Result<(), String> {
    let mut address = st.b(block).address;
    let mut buffer: &[u64] = &[];
    let mut buffer_offset = 0usize;
    let mut op = InstOp::default();

    loop {
        if address + 7 >= limit_address {
            break;
        }
        // Scheduling instructions, every 32 bytes.
        if address & 0x1f == 0 {
            address += 8;
            buffer_offset += 1;
            continue;
        }
        if buffer_offset >= buffer.len() {
            buffer = gpu.code(start_address + address);
            buffer_offset = 0;
        }
        let Some(&op_code) = buffer.get(buffer_offset) else {
            return Err(format!("code ends inside a block (0x{address:x})"));
        };
        buffer_offset += 1;

        op = get_op(address, op_code);

        if op.props.has(InstProps::TEX_B) {
            context.used_features |= FeatureFlags::BINDLESS;
        }

        match op.name {
            InstName::Ald | InstName::Ast | InstName::Ipa => set_user_attribute_uses(definitions, context, op.name, op_code),
            InstName::Pbk | InstName::Pcnt | InstName::Ssy => {
                st.push_ops.push(PushOpInfo { op, consumers: NetMap::new() });
                let p = PushOpIdx(st.push_ops.len() as u32 - 1);
                st.bm(block).push_op_codes.push(p);
            }
            InstName::Shfl => context.used_features |= FeatureFlags::SHUFFLE,
            InstName::Ldl | InstName::Stl => context.used_features |= FeatureFlags::LOCAL_MEMORY,
            InstName::Atoms | InstName::AtomsCas | InstName::Lds | InstName::Sts => context.used_features |= FeatureFlags::SHARED_MEMORY,
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
            | InstName::SustDB => context.used_features |= FeatureFlags::STORE,
            _ => {}
        }

        st.bm(block).op_codes.push(op);
        address += 8;

        if op.props.has(InstProps::BRA) {
            break;
        }
    }

    st.bm(block).end_address = address;
    Ok(())
}

fn set_user_attribute_uses(definitions: &mut ShaderDefinitions, context: &mut Context, name: InstName, op_code: u64) {
    let (offset, count, indexed, per_patch, is_store) = match name {
        InstName::Ast => {
            let op = InstAst(op_code);
            (op.imm11(), op.al_size().0 as i32 + 1, op.phys(), op.p(), true)
        }
        InstName::Ald => {
            let op = InstAld(op_code);
            (op.imm11(), op.al_size().0 as i32 + 1, op.phys(), op.p(), op.o())
        }
        _ => {
            let op = InstIpa(op_code);
            (op.imm10(), 1, op.idx(), false, false)
        }
    };

    if indexed {
        if is_store {
            context.attribute_usage.set_all_output_user_attributes();
            definitions.enable_output_indexing();
        } else {
            context.attribute_usage.set_all_input_user_attributes();
            definitions.enable_input_indexing();
        }
        return;
    }

    for elem_index in 0..count {
        let attr = offset + elem_index * 4;

        if per_patch {
            if (ac::USER_ATTRIBUTE_PER_PATCH_BASE..ac::USER_ATTRIBUTE_PER_PATCH_END).contains(&attr) {
                let index = (attr - ac::USER_ATTRIBUTE_PER_PATCH_BASE) / 16;
                if is_store {
                    context.attribute_usage.set_output_user_attribute_per_patch(index);
                } else {
                    context.attribute_usage.set_input_user_attribute_per_patch(index);
                }
            }
        } else if (ac::USER_ATTRIBUTE_BASE..ac::USER_ATTRIBUTE_END).contains(&attr) {
            let user_attr = attr - ac::USER_ATTRIBUTE_BASE;
            let index = user_attr / 16;
            if is_store {
                context.attribute_usage.set_output_user_attribute(index);
            } else {
                context.attribute_usage.set_input_user_attribute(index, (user_attr >> 2) & 3);
            }
        }

        let fixed_func = attr == ac::FOG_COORD
            || (ac::FRONT_COLOR_DIFFUSE_R..ac::CLIP_DISTANCE0).contains(&attr)
            || (ac::TEX_COORD_BASE..ac::TEX_COORD_END).contains(&attr);
        if !is_store && fixed_func {
            context.used_features |= FeatureFlags::FIXED_FUNC_ATTR;
        } else if is_store {
            match attr {
                ac::LAYER => {
                    if definitions.stage != ShaderStage::Compute && definitions.stage != ShaderStage::Fragment {
                        context.used_features |= FeatureFlags::RT_LAYER;
                    }
                }
                ac::VIEWPORT_INDEX => {
                    if definitions.stage != ShaderStage::Fragment {
                        context.used_features |= FeatureFlags::VIEWPORT_INDEX;
                    }
                }
                ac::CLIP_DISTANCE0..=ac::CLIP_DISTANCE7 if (attr - ac::CLIP_DISTANCE0) % 4 == 0 => {
                    if definitions.stage.is_vtg() {
                        context.clip_distances_written |= 1 << ((attr - ac::CLIP_DISTANCE0) / 4);
                    }
                }
                ac::VIEWPORT_MASK => {
                    if definitions.stage != ShaderStage::Fragment {
                        context.used_features |= FeatureFlags::VIEWPORT_MASK;
                    }
                }
                _ => {}
            }
        } else {
            match attr {
                ac::POSITION_X | ac::POSITION_Y => {
                    if definitions.stage == ShaderStage::Fragment {
                        context.used_features |= FeatureFlags::FRAG_COORD_XY;
                    }
                }
                ac::INSTANCE_ID => {
                    if definitions.stage == ShaderStage::Vertex {
                        context.used_features |= FeatureFlags::INSTANCE_ID;
                    }
                }
                _ => {}
            }
        }
    }
}

pub fn is_unconditional_branch(op: &InstOp) -> bool {
    is_unconditional(op) && op.props.has(InstProps::BRA)
}

fn is_unconditional(op: &InstOp) -> bool {
    let cond = InstConditional(op.raw_op_code);
    if (op.name == InstName::Bra || op.name == InstName::Exit) && cond.ccc() != Ccc::T {
        return false;
    }
    cond.pred() == PREDICATE_TRUE_INDEX && !cond.pred_inv()
}

type GetBlock<'f> = dyn Fn(&mut State, &mut NetMap<u64, BlockIdx>, &mut VecDeque<BlockIdx>, u64) -> BlockIdx + 'f;

fn find_brx_targets(
    gpu: &GpuAccessor,
    context: &mut Context,
    st: &mut State,
    blocks: &[BlockIdx],
    visited_blocks: &mut NetMap<u64, BlockIdx>,
    work_queue: &mut VecDeque<BlockIdx>,
    get_block: &GetBlock,
) -> bool {
    let mut has_new_target = false;
    for &block in blocks {
        let last_op = st.b(block).last_op();
        let has_next = st.b(block).has_next();
        if last_op.name == InstName::Brx && st.b(block).successors.len() == has_next as usize {
            let mut visited: NetSet<u64> = NetSet::new();
            let op_brx = InstBrx(last_op.raw_op_code);
            let base_offset = last_op.absolute_address();

            // An indirect branch could go anywhere: try the target offsets in the constant buffer.
            let (cb_base_offset, cb_offsets_count) = find_brx_target_range(st, block, op_brx.src_a());
            if cb_offsets_count != 0 {
                has_new_target = true;
            }
            for i in 0..cb_offsets_count {
                let target_offset = context.constant_buffer1_read(gpu, cb_base_offset + i * 4);
                let target_address = base_offset.wrapping_add(target_offset as u64);
                if visited.insert(target_address) {
                    let target = get_block(st, visited_blocks, work_queue, target_address);
                    st.bm(target).predecessors.push(block);
                    st.bm(block).successors.push(target);
                }
            }
        }
    }
    has_new_target
}

/// The jump table of a BRX: matches the pattern compilers emit before it,
///   IMNMX.U32 Rx, Rx, UpperBound, PT
///   SHL Rx, Rx, 0x2
///   LDC Rx, c[0x1][Rx+BaseOffset]
/// giving (BaseOffset, UpperBound + 1), or (0, 0).
fn find_brx_target_range(st: &State, block: BlockIdx, brx_reg: i32) -> (i32, i32) {
    let mut visited: NetSet<BlockIdx> = NetSet::new();

    let ldc = find_first_reg_write(st, &mut visited, (block, st.b(block).op_codes.len() as i64 - 1), brx_reg);
    let Some(ldc) = ldc.filter(|&(b, i)| st.b(b).op_codes[i as usize].name == InstName::Ldc) else { return (0, 0) };
    let op_ldc = InstLdc(st.b(ldc.0).op_codes[ldc.1 as usize].raw_op_code);
    if op_ldc.cbuf_slot() != 1 || op_ldc.address_mode().0 != 0 {
        return (0, 0);
    }

    let shl = find_first_reg_write(st, &mut visited, ldc, op_ldc.src_a());
    let Some(shl) = shl.filter(|&l| is_imm_inst(st, l, InstName::Shl)) else { return (0, 0) };
    let op_shl = InstShlI(st.b(shl.0).op_codes[shl.1 as usize].raw_op_code);
    if op_shl.imm20() != 2 {
        return (0, 0);
    }

    let imnmx = find_first_reg_write(st, &mut visited, shl, op_shl.src_a());
    let Some(imnmx) = imnmx.filter(|&l| is_imm_inst(st, l, InstName::Imnmx)) else { return (0, 0) };
    let op_imnmx = InstImnmxI(st.b(imnmx.0).op_codes[imnmx.1 as usize].raw_op_code);
    if op_imnmx.signed() || op_imnmx.src_pred() != PREDICATE_TRUE_INDEX || op_imnmx.src_pred_inv() {
        return (0, 0);
    }

    (op_ldc.cbuf_offset(), op_imnmx.imm20() + 1)
}

fn is_imm_inst(st: &State, (b, i): (BlockIdx, i64), name: InstName) -> bool {
    let op = st.b(b).op_codes[i as usize];
    op.name == name && op.props.has(InstProps::IB)
}

fn find_first_reg_write(st: &State, visited: &mut NetSet<BlockIdx>, location: (BlockIdx, i64), reg_index: i32) -> Option<(BlockIdx, i64)> {
    let mut to_visit = VecDeque::new();
    to_visit.push_back(location);
    visited.insert(location.0);

    while let Some((block, index)) = to_visit.pop_front() {
        let b = st.b(block);
        let mut i = index - 1;
        while i >= 0 {
            if writes_to_register(&b.op_codes[i as usize], reg_index) {
                return Some((block, i));
            }
            i -= 1;
        }
        for &pred in &b.predecessors {
            if visited.insert(pred) {
                to_visit.push_back((pred, st.b(pred).op_codes.len() as i64));
            }
        }
    }
    None
}

fn writes_to_register(op: &InstOp, reg_index: i32) -> bool {
    // Predicate instructions only write predicates.
    if op.props.0 & (InstProps::RD.0 | InstProps::RD2.0) == 0 {
        return false;
    }
    if op.props.has(InstProps::RD2) && (op.raw_op_code >> 28) as u8 as i32 == reg_index {
        return true;
    }
    op.raw_op_code as u8 as i32 == reg_index
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MergeType {
    Brk,
    Cont,
    Sync,
}

enum PathBlockState {
    Visit(BlockIdx),
    /// Pops what the block pushed when returning from it.
    PopPushOp(usize),
    /// Pushes back what the block popped.
    PushBranchOp(u64, MergeType),
}

fn propagate_push_op(st: &mut State, blocks: &NetMap<u64, BlockIdx>, curr_block: BlockIdx, mut push_op_index: usize) {
    let push_op_info = st.b(curr_block).push_op_codes[push_op_index];
    let push_op = st.push_ops[push_op_info.0 as usize].op;
    let target = *blocks.get(&push_op.absolute_address()).expect("push target block");

    let mut work_queue: Vec<PathBlockState> = Vec::new();
    let mut visited: NetSet<BlockIdx> = NetSet::new();
    let mut branch_stack: Vec<(u64, MergeType)> = Vec::new();

    // Visits of already visited blocks are dropped (restore operations never are).
    let push = |wq: &mut Vec<PathBlockState>, visited: &NetSet<BlockIdx>, pbs: PathBlockState| {
        if let PathBlockState::Visit(b) = pbs {
            if visited.contains(&b) {
                return;
            }
        }
        wq.push(pbs);
    };

    push(&mut work_queue, &visited, PathBlockState::Visit(curr_block));

    while let Some(pbs) = work_queue.pop() {
        let current = match pbs {
            PathBlockState::PopPushOp(size) => {
                branch_stack.truncate(size.min(branch_stack.len()));
                continue;
            }
            PathBlockState::PushBranchOp(a, m) => {
                branch_stack.push((a, m));
                continue;
            }
            PathBlockState::Visit(b) => b,
        };

        // A block processed already would push the same children again, going around in circles.
        if !visited.insert(current) {
            continue;
        }

        let push_ops_count = st.b(current).push_op_codes.len();
        if push_ops_count != 0 {
            push(&mut work_queue, &visited, PathBlockState::PopPushOp(branch_stack.len()));
            for index in push_op_index..push_ops_count {
                let p = st.b(current).push_op_codes[index];
                let op = st.push_ops[p.0 as usize].op;
                branch_stack.push((op.absolute_address(), merge_type_from_push(op.name)));
            }
        }

        push_op_index = 0;

        let has_next = st.b(current).has_next();
        if has_next {
            push(&mut work_queue, &visited, PathBlockState::Visit(st.b(current).successors[0]));
        }

        let last_op = st.b(current).last_op();
        if is_pop_branch(last_op.name) {
            let pop_merge_type = merge_type_from_pop(last_op.name);
            let mut found = true;
            let mut target_address = 0u64;
            loop {
                let Some((a, m)) = branch_stack.pop() else {
                    found = false;
                    break;
                };
                target_address = a;
                // Pushed back when returning from that block.
                push(&mut work_queue, &visited, PathBlockState::PushBranchOp(a, m));
                if m == pop_merge_type {
                    break;
                }
            }

            // BRK consumes only PBK addresses, CONT only PCNT ones, SYNC only SSY ones.
            if found {
                if branch_stack.is_empty() {
                    // The whole stack consumed: this pop consumed our push.
                    let id = st.b(current).sync_targets.len() as i32;
                    if st.bm(current).sync_targets.try_add(push_op.address, SyncTarget { push_op_info, push_op_id: id }) {
                        let local = st.ir.local();
                        st.push_ops[push_op_info.0 as usize].consumers.try_add(current, local);
                        st.bm(target).predecessors.push(current);
                        st.bm(current).successors.push(target);
                    }
                } else {
                    let b = *blocks.get(&target_address).expect("pop target block");
                    push(&mut work_queue, &visited, PathBlockState::Visit(b));
                }
            }
        } else {
            // Descending address order: the LIFO processes them in ascending order.
            let mut successors: Vec<(usize, BlockIdx)> = st.b(current).successors.iter().copied().enumerate().collect();
            successors.sort_by(|a, b| st.b(b.1).address.cmp(&st.b(a.1).address).then(a.0.cmp(&b.0)));
            let first = st.b(current).successors.first().copied();
            for (_, possible) in successors {
                if !has_next || Some(possible) != first {
                    push(&mut work_queue, &visited, PathBlockState::Visit(possible));
                }
            }
        }
    }
}

pub fn is_pop_branch(name: InstName) -> bool {
    matches!(name, InstName::Brk | InstName::Cont | InstName::Sync)
}

fn merge_type_from_push(name: InstName) -> MergeType {
    match name {
        InstName::Pbk => MergeType::Brk,
        InstName::Pcnt => MergeType::Cont,
        _ => MergeType::Sync,
    }
}

fn merge_type_from_pop(name: InstName) -> MergeType {
    match name {
        InstName::Brk => MergeType::Brk,
        InstName::Cont => MergeType::Cont,
        _ => MergeType::Sync,
    }
}
