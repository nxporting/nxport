// Global memory accesses become storage buffer accesses: the address is traced back to the constant buffer words that
// hold a storage buffer's base address (GlobalToStorage).

use std::collections::{HashMap, HashSet, VecDeque};

use super::utils::{self, find_last_operation};
use crate::ir::{BlockId, Function, Instruction as I, Ir, NodeId, OperandId, OperandType, StorageKind};
use crate::translation::cfg::ControlFlowGraph;
use crate::translation::emitter::{Builder, Emit};
use crate::translation::helper_functions::{bit_offset, HelperFunctionManager};
use crate::translation::resource_manager::ResourceManager;
use crate::translation::TransformContext;

const DRIVER_RESERVED_CB: i32 = 0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum LsMemoryType {
    Local,
    Shared,
}

#[derive(Clone, Copy, Debug)]
struct SearchResult {
    sb_cb_slot: i32,
    sb_cb_offset: i32,
    offset: Option<OperandId>,
    const_offset: i32,
}

impl SearchResult {
    const NOT_FOUND: SearchResult = SearchResult { sb_cb_slot: -1, sb_cb_offset: 0, offset: None, const_offset: 0 };

    fn new(sb_cb_slot: i32, sb_cb_offset: i32) -> Self {
        SearchResult { sb_cb_slot, sb_cb_offset, offset: None, const_offset: 0 }
    }

    fn with_offset(sb_cb_slot: i32, sb_cb_offset: i32, offset: Option<OperandId>, const_offset: i32) -> Self {
        SearchResult { sb_cb_slot, sb_cb_offset, offset, const_offset }
    }

    fn found(&self) -> bool {
        self.sb_cb_slot != -1
    }
}

struct Entry {
    function_id: i32,
    inst: I,
    storage_kind: StorageKind,
    is_multi_target: bool,
    target_cbs: Vec<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct LsKey {
    base_offset: Option<OperandId>,
    const_offset: i32,
    ty: LsMemoryType,
}

#[derive(Default)]
struct GtsContext {
    entries: Vec<Entry>,
    shared_entries: HashMap<LsKey, Vec<(u32, SearchResult)>>,
}

impl GtsContext {
    fn add_function(&mut self, hfm: &mut HelperFunctionManager, inst: I, sk: StorageKind, is_multi_target: bool, target_cbs: Vec<u32>, f: Function) -> i32 {
        let function_id = hfm.add_function(f);
        self.entries.push(Entry { function_id, inst, storage_kind: sk, is_multi_target, target_cbs });
        function_id
    }

    fn try_get_function_id(&self, inst: I, sk: StorageKind, is_multi_target: bool, target_cbs: &[u32]) -> Option<i32> {
        self.entries
            .iter()
            .find(|e| e.inst == inst && e.storage_kind == sk && e.is_multi_target == is_multi_target && e.target_cbs == target_cbs)
            .map(|e| e.function_id)
    }

    fn add_memory_target_cb(&mut self, ty: LsMemoryType, base_offset: Option<OperandId>, const_offset: i32, target_cb: u32, result: SearchResult) {
        let key = LsKey { base_offset, const_offset, ty };
        match self.shared_entries.get_mut(&key) {
            None => {
                self.shared_entries.insert(key, vec![(target_cb, result)]);
            }
            Some(target_cbs) => match target_cbs.iter_mut().find(|(k, _)| *k == target_cb) {
                // Different offsets for the same region: no single offset (multi-target accesses do not need one).
                Some((_, existing)) => {
                    if existing.offset.is_some() && (existing.offset != result.offset || existing.const_offset != result.const_offset) {
                        *existing = SearchResult::new(result.sb_cb_slot, result.sb_cb_offset);
                    }
                }
                None => target_cbs.push((target_cb, result)),
            },
        }
    }

    fn try_get_memory_target_cb(&self, ty: LsMemoryType, base_offset: Option<OperandId>, const_offset: i32) -> Option<SearchResult> {
        let key = LsKey { base_offset, const_offset, ty };
        let target_cbs = self.shared_entries.get(&key)?;
        if target_cbs.len() == 1 && target_cbs[0].1.found() {
            return Some(target_cbs[0].1);
        }
        None
    }
}

pub fn run_pass(ctx: &mut TransformContext) -> Result<(), String> {
    let mut gts = GtsContext::default();
    for &block in ctx.blocks {
        let mut node = ctx.ir.first_node(block);
        while let Some(n) = node {
            if !ctx.ir.is_op(n) {
                node = ctx.ir.next_node(n);
                continue;
            }
            let sk = ctx.ir.op(n).storage_kind;
            if is_global_memory(sk) {
                match replace_global_memory_with_storage(&mut gts, ctx, block, n)? {
                    // Replacement failed: loads read zero and stores do nothing.
                    None => {
                        ctx.gpu.log_msg(&format!(
                            "Failed to reserve storage buffer for global memory operation \"{}\".",
                            ctx.ir.inst(n).name()
                        ));
                        if ctx.ir.dest(n).is_some() {
                            let z = ctx.ir.const_i(0);
                            ctx.ir.turn_into_copy(n, z);
                            node = ctx.ir.next_node(n);
                        } else {
                            // The node leaves the list: continue from its successor at the time.
                            let next = ctx.ir.next_node(n);
                            utils::delete_node(ctx.ir, n);
                            node = next;
                        }
                        continue;
                    }
                    Some(next) => {
                        node = ctx.ir.next_node(next);
                        continue;
                    }
                }
            } else if ctx.ir.inst(n) == I::STORE && matches!(sk, StorageKind::SharedMemory | StorageKind::LocalMemory) {
                // The compiler sometimes keeps a storage buffer's base address and size in shared or local memory.
                if let Some((ty, base_offset, const_offset)) = try_get_memory_offsets(ctx.ir, n) {
                    let value = ctx.ir.src(n, ctx.ir.sources_count(n) - 1);
                    let result = find_unique_base_address_cb(&gts, ctx.ir, block, value, false);
                    if result.found() {
                        let target_cb = pack_cb_slot_and_offset(result.sb_cb_slot, result.sb_cb_offset);
                        gts.add_memory_target_cb(ty, base_offset, const_offset, target_cb, result);
                    }
                }
            }
            node = ctx.ir.next_node(n);
        }
    }
    Ok(())
}

fn is_global_memory(sk: StorageKind) -> bool {
    matches!(
        sk,
        StorageKind::GlobalMemory | StorageKind::GlobalMemoryS8 | StorageKind::GlobalMemoryS16 | StorageKind::GlobalMemoryU8 | StorageKind::GlobalMemoryU16
    )
}

fn is_small_int(sk: StorageKind) -> bool {
    matches!(sk, StorageKind::GlobalMemoryS8 | StorageKind::GlobalMemoryS16 | StorageKind::GlobalMemoryU8 | StorageKind::GlobalMemoryU16)
}

/// The node to continue after, or None when no storage buffer could be used.
fn replace_global_memory_with_storage(gts: &mut GtsContext, ctx: &mut TransformContext, block: BlockId, n: NodeId) -> Result<Option<NodeId>, String> {
    let global_address = ctx.ir.src(n, 0);
    let result = find_unique_base_address_cb(gts, ctx.ir, block, global_address, true);
    if result.found() {
        // The storage buffer is known: simple accesses are made inline, others call a helper function.
        let mut offset = result.offset;
        if ctx.gpu.has_unaligned_storage_buffer() {
            let base_address = ctx.ir.cbuf(result.sb_cb_slot, result.sb_cb_offset);
            let base_address_masked = ctx.ir.local();
            let host_offset = ctx.ir.local();
            let alignment = ctx.gpu.host_storage_buffer_offset_alignment();
            let k = ctx.ir.const_i(-alignment);
            let mask_op = ctx.ir.operation(I::BITWISE_AND, Some(base_address_masked), &[base_address, k]);
            let sub_op = ctx.ir.operation(I::SUBTRACT, Some(host_offset), &[global_address, base_address_masked]);
            ctx.ir.add_before(n, mask_op);
            ctx.ir.add_before(n, sub_op);
            offset = Some(host_offset);
        } else if result.const_offset != 0 {
            let new_offset = ctx.ir.local();
            let k = ctx.ir.const_i(result.const_offset);
            let add_op = ctx.ir.operation(I::ADD, Some(new_offset), &[offset.ok_or(utils::NRE)?, k]);
            ctx.ir.add_before(n, add_op);
            offset = Some(new_offset);
        }
        if ctx.ir.op(n).storage_kind == StorageKind::GlobalMemory {
            // Inline (CanUseInlineStorageOp: always for SPIR-V once the access is not a small integer).
            return Ok(generate_inline_storage_op(ctx, n, offset.ok_or(utils::NRE)?, result));
        }
        let Some(function_id) = try_generate_single_target_storage_op(gts, ctx, n, result)? else { return Ok(None) };
        return Ok(Some(generate_call_storage_op(ctx.ir, n, offset, function_id)));
    }
    // Not found directly: the base address may come from several constant buffers through phis; a helper function
    // checks each possible storage buffer.
    let Some(function_id) = try_generate_multi_target_storage_op(gts, ctx, block, n)? else { return Ok(None) };
    Ok(Some(generate_call_storage_op(ctx.ir, n, None, function_id)))
}

fn generate_inline_storage_op(ctx: &mut TransformContext, n: NodeId, offset: OperandId, result: SearchResult) -> Option<NodeId> {
    let inst = ctx.ir.inst(n);
    let is_store = inst == I::STORE || inst.is_atomic();
    let binding = ctx.rm.try_get_storage_buffer_binding(ctx.gpu, result.sb_cb_slot, result.sb_cb_offset, is_store)?;
    let ir = &mut *ctx.ir;
    let word_offset = ir.local();
    let count = ir.sources_count(n);
    let (b, z) = (ir.const_i(binding), ir.const_i(0));
    let sources: Vec<OperandId> = if inst == I::ATOMIC_COMPARE_AND_SWAP {
        vec![b, z, word_offset, ir.src(n, count - 2), ir.src(n, count - 1)]
    } else if is_store {
        vec![b, z, word_offset, ir.src(n, count - 1)]
    } else {
        vec![b, z, word_offset]
    };
    let k2 = ir.const_i(2);
    let shift_op = ir.operation(I::SHIFT_RIGHT_U32, Some(word_offset), &[offset, k2]);
    let dest = ir.dest(n);
    let storage_op = ir.operation_sk(inst, StorageKind::StorageBuffer, dest, &sources);
    ir.add_before(n, shift_op);
    let new_node = ir.add_before(n, storage_op);
    utils::delete_node(ir, n);
    Some(new_node)
}

fn generate_call_storage_op(ir: &mut Ir, n: NodeId, offset: Option<OperandId>, function_id: i32) -> NodeId {
    let count = ir.sources_count(n);
    let mut sources = vec![ir.const_i(function_id)];
    match offset {
        // With an offset, it replaces the global address.
        Some(o) => {
            sources.push(o);
            for i in 2..count {
                sources.push(ir.src(n, i));
            }
        }
        // Without one, the 64-bit address goes as two 32-bit arguments.
        None => {
            for i in 0..count {
                sources.push(ir.src(n, i));
            }
        }
    }
    let returns_value = ir.dest(n).is_some();
    let return_value = if returns_value { Some(ir.local()) } else { None };
    let call_op = ir.operation(I::CALL, return_value, &sources);
    let new_node = ir.add_before(n, call_op);
    if let Some(rv) = return_value {
        ir.turn_into_copy(n, rv);
        n
    } else {
        utils::delete_node(ir, n);
        new_node
    }
}

fn in_arguments_count(inst: I, base: i32) -> i32 {
    if inst == I::ATOMIC_COMPARE_AND_SWAP {
        base + 2
    } else if inst == I::STORE || inst.is_atomic() {
        base + 1
    } else {
        base
    }
}

fn try_generate_single_target_storage_op(gts: &mut GtsContext, ctx: &mut TransformContext, n: NodeId, result: SearchResult) -> Result<Option<i32>, String> {
    let (inst, sk) = (ctx.ir.inst(n), ctx.ir.op(n).storage_kind);
    let target_cbs = vec![pack_cb_slot_and_offset(result.sb_cb_slot, result.sb_cb_offset)];
    if let Some(id) = gts.try_get_function_id(inst, sk, false, &target_cbs) {
        return Ok(Some(id));
    }
    let in_args = in_arguments_count(inst, 1);
    let mut c = Builder::new(ctx.ir);
    let offset = c.ir.argument(0);
    let (compare, value) = match in_args {
        3 => (Some(c.ir.argument(1)), Some(c.ir.argument(2))),
        2 => (None, Some(c.ir.argument(1))),
        _ => (None, None),
    };
    let Some(result_value) = try_generate_storage_op(ctx.rm, ctx.gpu, &mut c, inst, sk, offset, compare, value, result)? else {
        return Ok(None);
    };
    let returns_value = result_value.is_some();
    match result_value {
        Some(v) => c.ret_value(v),
        None => c.ret(),
    }
    let ops = c.operations();
    let name = function_name(inst, sk, false, &target_cbs);
    let cfg = ControlFlowGraph::create(ctx.ir, &ops);
    let f = Function { blocks: cfg.blocks, name, returns_value, in_arguments_count: in_args, out_arguments_count: 0 };
    Ok(Some(gts.add_function(ctx.hfm, inst, sk, false, target_cbs, f)))
}

fn try_generate_multi_target_storage_op(gts: &mut GtsContext, ctx: &mut TransformContext, block: BlockId, n: NodeId) -> Result<Option<i32>, String> {
    let ir = &mut *ctx.ir;
    let mut phis: VecDeque<NodeId> = VecDeque::new();
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut target_cbs: Vec<u32> = Vec::new();

    let mut global_address = ir.src(n, 0);
    if let Some(add_op) = ir.asg_op(global_address).filter(|&a| ir.inst_of(a) == Some(I::ADD)) {
        let (s1, s2) = (ir.src(add_op, 0), ir.src(add_op, 1));
        if ir.ty(s1) == OperandType::Constant && ir.ty(s2) == OperandType::LocalVariable {
            global_address = s2;
        } else if ir.ty(s1) == OperandType::LocalVariable && ir.ty(s2) == OperandType::Constant {
            global_address = s1;
        }
    }
    match ir.asg_op(global_address).filter(|&a| ir.is_phi(a)) {
        Some(phi) if visited.insert(phi) => phis.push_back(phi),
        _ => {
            let result = find_unique_base_address_cb(gts, ir, block, ir.src(n, 0), false);
            if result.found() {
                target_cbs.push(pack_cb_slot_and_offset(result.sb_cb_slot, result.sb_cb_offset));
            }
        }
    }
    while let Some(phi) = phis.pop_front() {
        for i in 0..ir.sources_count(phi) {
            let phi_block = ir.phi_block(phi, i);
            let phi_source = ir.src(phi, i);
            let result = find_unique_base_address_cb(gts, ir, phi_block, phi_source, false);
            if result.found() {
                let target_cb = pack_cb_slot_and_offset(result.sb_cb_slot, result.sb_cb_offset);
                if !target_cbs.contains(&target_cb) {
                    target_cbs.push(target_cb);
                }
            } else if let Some(phi2) = ir.asg_op(phi_source).filter(|&a| ir.is_phi(a)) {
                if visited.insert(phi2) {
                    phis.push_back(phi2);
                }
            }
        }
    }
    target_cbs.sort_unstable();

    let (inst, sk) = (ir.inst(n), ir.op(n).storage_kind);
    if target_cbs.is_empty() {
        ctx.gpu.log_msg(&format!("Failed to find storage buffer for global memory operation \"{}\".", inst.name()));
    }
    if let Some(id) = gts.try_get_function_id(inst, sk, true, &target_cbs) {
        return Ok(Some(id));
    }
    let returns_value = ir.dest(n).is_some();
    let in_args = in_arguments_count(inst, 2);

    let mut c = Builder::new(ctx.ir);
    let global_address_low = c.ir.argument(0);
    let global_address_high = c.ir.argument(1);
    for &target_cb in &target_cbs {
        let (sb_cb_slot, sb_cb_offset) = unpack_cb_slot_and_offset(target_cb);
        let base_addr_low = c.cbuf(sb_cb_slot, sb_cb_offset);
        let base_addr_high = c.cbuf(sb_cb_slot, sb_cb_offset + 1);
        let size = c.cbuf(sb_cb_slot, sb_cb_offset + 2);
        let offset = c.isubtract(global_address_low, base_addr_low);
        let borrow = c.icompare_less_unsigned(global_address_low, base_addr_low);
        let in_range_low = c.icompare_less_unsigned(offset, size);
        let addr_high_borrowed = c.iadd(global_address_high, borrow);
        let in_range_high = c.icompare_equal(addr_high_borrowed, base_addr_high);
        let in_range = c.bitwise_and(in_range_low, in_range_high);
        let lbl_skip = c.label();
        c.branch_if_false(lbl_skip, in_range);
        let (compare, value) = match in_args {
            4 => (Some(c.ir.argument(2)), Some(c.ir.argument(3))),
            3 => (None, Some(c.ir.argument(2))),
            _ => (None, None),
        };
        let result = SearchResult::new(sb_cb_slot, sb_cb_offset);
        let alignment = ctx.gpu.host_storage_buffer_offset_alignment();
        let k = c.const_i(-alignment);
        let base_address_masked = c.bitwise_and(base_addr_low, k);
        let host_offset = c.isubtract(global_address_low, base_address_masked);
        let Some(result_value) = try_generate_storage_op(ctx.rm, ctx.gpu, &mut c, inst, sk, host_offset, compare, value, result)? else {
            return Ok(None);
        };
        match result_value {
            Some(v) => c.ret_value(v),
            None => c.ret(),
        }
        c.mark_label(lbl_skip);
    }
    if returns_value {
        let z = c.const_i(0);
        c.ret_value(z);
    } else {
        c.ret();
    }
    let ops = c.operations();
    let name = function_name(inst, sk, true, &target_cbs);
    let cfg = ControlFlowGraph::create(ctx.ir, &ops);
    let f = Function { blocks: cfg.blocks, name, returns_value, in_arguments_count: in_args, out_arguments_count: 0 };
    Ok(Some(gts.add_function(ctx.hfm, inst, sk, true, target_cbs, f)))
}

fn pack_cb_slot_and_offset(cb_slot: i32, cb_offset: i32) -> u32 {
    (cb_slot as u16 as u32) | ((cb_offset as u16 as u32) << 16)
}

fn unpack_cb_slot_and_offset(packed: u32) -> (i32, i32) {
    (packed as u16 as i32, (packed >> 16) as u16 as i32)
}

fn function_name(inst: I, sk: StorageKind, is_multi_target: bool, target_cbs: &[u32]) -> String {
    let mut name = inst.name();
    name += match sk {
        StorageKind::GlobalMemoryS8 => "S8",
        StorageKind::GlobalMemoryS16 => "S16",
        StorageKind::GlobalMemoryU8 => "U8",
        StorageKind::GlobalMemoryU16 => "U16",
        _ => "",
    };
    if is_multi_target {
        name += "Multi";
    }
    for &target_cb in target_cbs {
        let (slot, offset) = unpack_cb_slot_and_offset(target_cb);
        name += &format!("_c{slot}o{offset}");
    }
    name
}

/// Ok(None): no storage buffer binding; Ok(Some(value)): the access's result, if any.
#[allow(clippy::too_many_arguments)]
fn try_generate_storage_op(
    rm: &mut ResourceManager,
    gpu: &crate::accessor::GpuAccessor,
    c: &mut Builder,
    inst: I,
    sk: StorageKind,
    offset: OperandId,
    compare: Option<OperandId>,
    value: Option<OperandId>,
    result: SearchResult,
) -> Result<Option<Option<OperandId>>, String> {
    let is_store = inst.is_atomic() || inst == I::STORE;
    let Some(binding) = rm.try_get_storage_buffer_binding(gpu, result.sb_cb_slot, result.sb_cb_offset, is_store) else {
        return Ok(None);
    };
    let k2 = c.const_i(2);
    let word_offset = c.shift_right_u32(offset, k2);
    let sb = StorageKind::StorageBuffer;
    let result_value;
    if inst.is_atomic() {
        if is_small_int(sk) {
            return Err("System.NotImplementedException: The method or operation is not implemented.".into());
        }
        let z = c.const_i(0);
        let v = value.ok_or(utils::NRE)?;
        result_value = Some(if inst == I::ATOMIC_COMPARE_AND_SWAP {
            c.atomic_binding(inst, sb, binding, &[z, word_offset, compare.ok_or(utils::NRE)?, v])
        } else {
            // SPIR-V has signed atomic minimum and maximum: no compare-and-swap loop.
            c.atomic_binding(inst, sb, binding, &[z, word_offset, v])
        });
    } else if inst == I::STORE {
        let bit_size = match sk {
            StorageKind::GlobalMemoryS8 | StorageKind::GlobalMemoryU8 => 8,
            StorageKind::GlobalMemoryS16 | StorageKind::GlobalMemoryU16 => 16,
            _ => 32,
        };
        let v = value.ok_or(utils::NRE)?;
        if bit_size < 32 {
            let bo = bit_offset(c, offset);
            atomic_cas_loop(c, word_offset, binding, |c, mem| {
                let size = c.const_i(bit_size);
                c.bitfield_insert(mem, v, bo, size)
            });
        } else {
            let z = c.const_i(0);
            c.store_b(sb, binding, &[z, word_offset, v]);
        }
        result_value = None;
    } else {
        let z = c.const_i(0);
        let mut v = c.load_b(sb, binding, &[z, word_offset]);
        if is_small_int(sk) {
            let bo = bit_offset(c, offset);
            match sk {
                StorageKind::GlobalMemoryS8 | StorageKind::GlobalMemoryS16 => {
                    v = c.shift_right_s32(v, bo);
                    let (z, len) = (c.const_i(0), c.const_i(if sk == StorageKind::GlobalMemoryS8 { 8 } else { 16 }));
                    v = c.bitfield_extract_s32(v, z, len);
                }
                _ => {
                    v = c.shift_right_u32(v, bo);
                    let m = c.const_i(if sk == StorageKind::GlobalMemoryU8 { u8::MAX as i32 } else { u16::MAX as i32 });
                    v = c.bitwise_and(v, m);
                }
            }
        }
        result_value = Some(v);
    }
    Ok(Some(result_value))
}

fn atomic_cas_loop(c: &mut Builder, word_offset: OperandId, binding: i32, op: impl FnOnce(&mut Builder, OperandId) -> OperandId) -> OperandId {
    let lbl_loop_head = c.label();
    c.mark_label(lbl_loop_head);
    let z = c.const_i(0);
    let old_value = c.load_b(StorageKind::StorageBuffer, binding, &[z, word_offset]);
    let new_value = op(c, old_value);
    let z = c.const_i(0);
    let cas_result = c.atomic_binding(I::ATOMIC_COMPARE_AND_SWAP, StorageKind::StorageBuffer, binding, &[z, word_offset, old_value, new_value]);
    let cas_fail = c.icompare_not_equal(cas_result, old_value);
    c.branch_if_true(lbl_loop_head, cas_fail);
    old_value
}

fn find_unique_base_address_cb(gts: &GtsContext, ir: &mut Ir, block: BlockId, global_address: OperandId, needs_offset: bool) -> SearchResult {
    let global_address = find_last_operation(ir, global_address, block, true);
    if ir.ty(global_address) == OperandType::ConstantBuffer {
        let zero = ir.const_i(0);
        return base_address_cb_with_offset(ir, global_address, Some(zero), 0);
    }
    let operation = ir.asg_op(global_address).filter(|&a| ir.is_op(a));
    let Some(mut operation) = operation.filter(|&o| ir.inst(o) == I::ADD) else {
        return find_base_address_cb_from_memory(gts, ir, operation, 0, needs_offset);
    };
    let (src1, src2) = (ir.src(operation, 0), ir.src(operation, 1));
    let mut const_offset = 0;
    let (t1, t2) = (ir.ty(src1), ir.ty(src2));
    if (t1 == OperandType::LocalVariable && t2 == OperandType::Constant) || (t2 == OperandType::LocalVariable && t1 == OperandType::Constant) {
        let (base_addr, offset) = if t1 == OperandType::LocalVariable {
            (find_last_operation(ir, src1, block, true), src2)
        } else {
            (find_last_operation(ir, src2, block, true), src1)
        };
        let result = base_address_cb_with_offset(ir, base_addr, Some(offset), 0);
        if result.found() {
            return result;
        }
        const_offset = ir.value(offset);
        let op = ir.asg_op(base_addr).filter(|&a| ir.is_op(a));
        match op.filter(|&o| ir.inst(o) == I::ADD) {
            Some(o) => operation = o,
            None => return find_base_address_cb_from_memory(gts, ir, op, const_offset, needs_offset),
        }
    }
    let (src1, src2) = (ir.src(operation, 0), ir.src(operation, 1));
    // With two candidates, the driver-reserved constant buffer (which holds the base addresses) wins; with two
    // constant buffers, the second operand, which is always the one from the last instruction.
    let (t1, t2) = (ir.ty(src1), ir.ty(src2));
    if t1 != OperandType::ConstantBuffer
        || (t1 == OperandType::ConstantBuffer && t2 == OperandType::ConstantBuffer)
        || (t2 == OperandType::ConstantBuffer && ir.opnd(src2).cbuf_slot() == DRIVER_RESERVED_CB)
    {
        return base_address_cb_with_offset(ir, src2, Some(src1), const_offset);
    }
    base_address_cb_with_offset(ir, src1, Some(src2), const_offset)
}

fn find_base_address_cb_from_memory(gts: &GtsContext, ir: &Ir, operation: Option<NodeId>, const_offset: i32, needs_offset: bool) -> SearchResult {
    if let Some(op) = operation {
        if let Some((ty, bo, co)) = try_get_memory_offsets(ir, op) {
            if let Some(result) = gts.try_get_memory_target_cb(ty, bo, co) {
                if result.offset.is_some() || !needs_offset {
                    if const_offset != 0 {
                        return SearchResult::with_offset(result.sb_cb_slot, result.sb_cb_offset, result.offset, result.const_offset + const_offset);
                    }
                    return result;
                }
            }
        }
    }
    SearchResult::NOT_FOUND
}

/// The base address must be 64-bit aligned (one register pair).
fn base_address_cb_with_offset(ir: &Ir, base_address: OperandId, offset: Option<OperandId>, const_offset: i32) -> SearchResult {
    if ir.ty(base_address) == OperandType::ConstantBuffer {
        let (slot, cb_offset) = (ir.opnd(base_address).cbuf_slot(), ir.opnd(base_address).cbuf_offset());
        if cb_offset & 1 == 0 {
            return SearchResult::with_offset(slot, cb_offset, offset, const_offset);
        }
    }
    SearchResult::NOT_FOUND
}

fn try_get_memory_offsets(ir: &Ir, op: NodeId) -> Option<(LsMemoryType, Option<OperandId>, i32)> {
    let o = ir.as_op(op)?;
    if o.inst != I::LOAD && o.inst != I::STORE {
        return None;
    }
    match o.storage_kind {
        StorageKind::SharedMemory => {
            let (base_offset, const_offset) = try_get_shared_memory_offsets(ir, op)?;
            Some((LsMemoryType::Shared, Some(base_offset), const_offset))
        }
        StorageKind::LocalMemory => {
            let offset = ir.src(op, 1);
            if ir.ty(offset) == OperandType::Constant {
                Some((LsMemoryType::Local, None, ir.value(offset)))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn try_get_shared_memory_offsets(ir: &Ir, op: NodeId) -> Option<(OperandId, i32)> {
    // The byte offset is shifted right by 2 for the word offset; the byte offset is what matches between accesses.
    let mut base_offset = None;
    let mut const_offset = 0;
    if let Some(shift_right_op) = ir.asg_op(ir.src(op, 1)).filter(|&a| ir.inst_of(a) == Some(I::SHIFT_RIGHT_U32)) {
        let s1 = ir.src(shift_right_op, 1);
        if ir.ty(s1) == OperandType::Constant && ir.value(s1) == 2 {
            base_offset = Some(ir.src(shift_right_op, 0));
        }
    }
    // A constant added to the base offset.
    if let Some(add_op) = base_offset.and_then(|b| ir.asg_op(b)).filter(|&a| ir.inst_of(a) == Some(I::ADD)) {
        let (s1, s2) = (ir.src(add_op, 0), ir.src(add_op, 1));
        if ir.ty(s1) == OperandType::Constant && ir.ty(s2) == OperandType::LocalVariable {
            const_offset = ir.value(s1);
            base_offset = Some(s2);
        } else if ir.ty(s1) == OperandType::LocalVariable && ir.ty(s2) == OperandType::Constant {
            base_offset = Some(s1);
            const_offset = ir.value(s2);
        }
    }
    let b = base_offset?;
    (ir.ty(b) == OperandType::LocalVariable).then_some((b, const_offset))
}
