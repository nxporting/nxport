// Branches, calls, returns and the SSY/PBK/PCNT stack (InstEmitFlowControl).

use super::moves::get_condition;
use crate::decoders::inst_decoders::*;
use crate::decoders::register::PREDICATE_TRUE_INDEX;
use crate::decoders::{BlockIdx, FunctionType};
use crate::ir::{ir_consts, OperandType};
use crate::translation::emitter::{Emit, EmitterContext as Ctx};

pub fn bra(c: &mut Ctx) {
    let target = *c.program.block(c.curr_block).successors.last().expect("branch without a target");
    let address = c.program.block(target).address;
    emit_branch(c, address);
}

pub fn brk(c: &mut Ctx) {
    emit_brk_cont_sync(c);
}

pub fn brx(c: &mut Ctx) {
    let op = InstBrx(c.raw_op());
    let curr_op = c.curr_op;
    let program = c.program;
    let block = program.block(c.curr_block);
    let start_index = block.has_next() as usize;
    if block.successors.len() <= start_index {
        c.log(&format!("Failed to find targets for BRX instruction at 0x{:X}.", curr_op.address));
        return;
    }

    let offset = curr_op.absolute_address() as i32;
    let reg = c.gpr(op.src_a());
    let k = c.const_i(offset);
    let address = c.iadd(reg, k);

    let targets: Vec<BlockIdx> = block.successors[start_index..].to_vec();
    let total = targets.len();
    let addr = |b: BlockIdx| program.block(b).address;

    let mut ascending = targets.clone();
    ascending.sort_by_key(|&b| addr(b));

    let mut all_targets_single_pred = true;
    for (i, &target) in ascending.iter().enumerate() {
        if i + 1 < total && (program.block(target).predecessors.len() > 1 || addr(target) <= block.address) {
            all_targets_single_pred = false;
            break;
        }
    }

    if all_targets_single_pred {
        // Chain the targets: each checks whether the address is its own and otherwise jumps to the next, the last
        // one taken unconditionally. Only possible when the BRX block is every target's only predecessor and the
        // targets come after it (a label cannot be inserted into code already emitted).
        let mut current: Option<BlockIdx> = None;
        let mut first_target_address = 0;
        for &next in &ascending {
            match current {
                Some(cur) => {
                    if addr(cur) != addr(next) {
                        c.set_brx_target(addr(cur), address, addr(cur) as i32, addr(next));
                    }
                }
                None => first_target_address = addr(next),
            }
            current = Some(next);
        }
        let l = c.get_label(first_target_address);
        c.branch(l);
    } else {
        // Sequential branches: worse code, works in every case.
        let mut descending = targets;
        descending.sort_by_key(|&b| std::cmp::Reverse(addr(b)));
        let mut last_target_address = u64::MAX;
        for (i, &target) in descending.iter().enumerate() {
            let label = c.get_label(addr(target));
            if i + 1 < total {
                if addr(target) != last_target_address {
                    let k = c.const_i(addr(target) as i32);
                    let eq = c.icompare_equal(address, k);
                    c.branch_if_true(label, eq);
                }
                last_target_address = addr(target);
            } else {
                c.branch(label);
            }
        }
    }
}

pub fn cal(c: &mut Ctx) {
    let address = c.curr_op.absolute_address();
    let f = c.program.function_by_address(address).expect("call target function");
    let function = c.program.function(f);
    if function.is_compiler_generated() {
        match function.ty {
            FunctionType::BuiltInFsiBegin => c.fsi_begin(),
            FunctionType::BuiltInFsiEnd => c.fsi_end(),
            _ => {}
        }
    } else {
        let id = function.id;
        c.call(id, false, &[]);
    }
}

pub fn cont(c: &mut Ctx) {
    emit_brk_cont_sync(c);
}

pub fn exit(c: &mut Ctx) {
    let op = InstExit(c.raw_op());
    if c.is_non_main {
        c.log("Invalid exit on non-main function.");
        return;
    }
    if op.ccc() == Ccc::T {
        if c.prepare_for_return() {
            c.ret();
        }
    } else {
        let cond = get_condition(c, op.ccc(), ir_consts::FALSE);
        // Nothing to do when the condition is always false.
        if c.ir.ty(cond) != OperandType::Constant || c.ir.value(cond) != ir_consts::FALSE {
            let skip = c.label();
            c.branch_if_false(skip, cond);
            if c.prepare_for_return() {
                c.ret();
            }
            c.mark_label(skip);
        }
    }
}

pub fn kil(c: &mut Ctx) {
    c.discard();
}

pub fn pbk(c: &mut Ctx) {
    emit_pbk_pcnt_ssy(c);
}

pub fn pcnt(c: &mut Ctx) {
    emit_pbk_pcnt_ssy(c);
}

pub fn ret(c: &mut Ctx) {
    if c.is_non_main {
        c.ret();
    } else {
        c.log("Invalid return on main function.");
    }
}

pub fn ssy(c: &mut Ctx) {
    emit_pbk_pcnt_ssy(c);
}

pub fn sync(c: &mut Ctx) {
    emit_brk_cont_sync(c);
}

fn emit_pbk_pcnt_ssy(c: &mut Ctx) {
    let program = c.program;
    let address = c.curr_op.address;
    let block = program.block(c.curr_block);
    let push = block.push_op_codes.iter().copied().find(|&p| program.push_op(p).op.address == address).expect("push op");
    for (&consumer, &local) in program.push_op(push).consumers.iter() {
        let id = program.block(consumer).sync_targets.get(&address).expect("sync target").push_op_id;
        let k = c.const_i(id);
        c.copy_to(local, k);
    }
}

fn emit_brk_cont_sync(c: &mut Ctx) {
    let program = c.program;
    let block = program.block(c.curr_block);
    let targets = &block.sync_targets;
    if targets.len() == 1 {
        // A single target: the push was a plain branch.
        let t = *targets.values().next().unwrap();
        emit_branch(c, program.push_op(t.push_op_info).op.absolute_address());
    } else {
        // TODO in the original: the condition code here as well.
        for t in targets.values() {
            let info = program.push_op(t.push_op_info);
            let label = c.get_label(info.op.absolute_address());
            let local = *info.consumers.get(&c.curr_block).expect("push consumer");
            let k = c.const_i(t.push_op_id);
            let eq = c.icompare_equal(local, k);
            c.branch_if_true(label, eq);
        }
    }
}

fn emit_branch(c: &mut Ctx, address: u64) {
    let op = c.curr_op;
    let cond = InstConditional(op.raw_op_code);
    // A branch to the next instruction does nothing.
    if address == op.address + 8 {
        return;
    }
    let label = c.get_label(address);
    let mut pred = c.pred(cond.pred());
    if cond.ccc() != Ccc::T {
        let cc = get_condition(c, cond.ccc(), ir_consts::TRUE);
        if cond.pred() == PREDICATE_TRUE_INDEX {
            pred = cc;
        } else if cond.pred_inv() {
            let np = c.bitwise_not(pred);
            pred = c.bitwise_and(np, cc);
        } else {
            pred = c.bitwise_and(pred, cc);
        }
        c.branch_if_true(label, pred);
    } else if cond.pred() == PREDICATE_TRUE_INDEX {
        c.branch(label);
    } else if cond.pred_inv() {
        c.branch_if_false(label, pred);
    } else {
        c.branch_if_true(label, pred);
    }
}
