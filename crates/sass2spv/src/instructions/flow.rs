use crate::decoders::inst_op::InstOp;
use crate::translation::emitter_context::EmitterContext;

pub fn emit_bra(context: &mut EmitterContext, op: &InstOp) {
    let target_addr = op.get_absolute_address();
    let label = context.get_or_create_label(target_addr);
    let cond = crate::decoders::inst_decoders::InstConditional::new(op.raw_op_code);
    let pred = cond.pred();
    let pred_inv = cond.pred_inv();

    if pred == crate::decoders::register::RegisterConsts::PREDICATE_TRUE_INDEX as i32 {
        if !pred_inv {
            context.branch(label);
        }
    } else {
        let pred_op = crate::ir::operand::Operand::register(pred as usize, crate::decoders::register::RegisterType::Predicate);
        if pred_inv {
            context.branch_if_false(label, pred_op);
        } else {
            context.branch_if_true(label, pred_op);
        }
    }
}

pub fn emit_exit(context: &mut EmitterContext) {
    context.return_op();
}

pub fn emit_kil(context: &mut EmitterContext) {
    context.discard();
}

pub fn emit_brk(context: &mut EmitterContext) {
    // Loop break
    context.add(crate::ir::instruction::Instruction::LoopBreak, None, vec![]);
}

pub fn emit_sync(_context: &mut EmitterContext) {
    // Convergence point marker
}

pub fn emit_brx(context: &mut EmitterContext, op: &InstOp, targets: &[u64]) {
    let op_brx = crate::decoders::inst_decoders::InstBrx::new(op.raw_op_code);
    let offset = op.get_absolute_address() as i32;
    let src_a = crate::ir::operand::Operand::register(op_brx.src_a() as usize, crate::decoders::register::RegisterType::Gpr);
    let target_addr_var = context.i_add(src_a, crate::ir::operand::Operand::constant(offset));

    if targets.is_empty() {
        context.return_op();
        return;
    }

    for (i, &target) in targets.iter().enumerate() {
        let label = context.get_or_create_label(target);
        if i + 1 < targets.len() {
            let cmp = context.compare_equal(target_addr_var, crate::ir::operand::Operand::constant(target as i32));
            context.branch_if_true(label, cmp);
        } else {
            context.branch(label);
        }
    }
}

