pub mod emitter_context;

pub use emitter_context::EmitterContext;

use crate::decoders::decoder::DecodedProgram;
use crate::instructions::emit_op;
use crate::ir::operation::Operation;

pub fn emit_shader(program: &DecodedProgram) -> Vec<Operation> {
    let mut context = EmitterContext::new();

    for block in &program.blocks {
        let block_label = context.get_or_create_label(block.address);
        context.mark_label(block_label);

        for op in &block.op_codes {
            let cond = crate::decoders::inst_decoders::InstConditional::new(op.raw_op_code);
            let pred = cond.pred();
            let pred_inv = cond.pred_inv();

            if pred == crate::decoders::register::RegisterConsts::PREDICATE_TRUE_INDEX as i32 && pred_inv {
                // PT inverted: condition is always FALSE, instruction is never executed.
                continue;
            }

            let is_bra = op.name == crate::decoders::inst_name::InstName::Bra;
            let is_predicated = !is_bra && pred != crate::decoders::register::RegisterConsts::PREDICATE_TRUE_INDEX as i32;

            let skip_lbl = if is_predicated {
                let lbl = context.alloc_label();
                let pred_op = crate::ir::operand::Operand::register(pred as usize, crate::decoders::register::RegisterType::Predicate);
                if pred_inv {
                    context.branch_if_true(lbl, pred_op);
                } else {
                    context.branch_if_false(lbl, pred_op);
                }
                Some(lbl)
            } else {
                None
            };

            emit_op(&mut context, op, &block.brx_targets);

            if let Some(lbl) = skip_lbl {
                context.mark_label(lbl);
            }
        }
    }

    context.operations
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoders::decoder::decode;
    use crate::gpu_accessor::MemoryGpuAccessor;

    #[test]
    fn test_emit_real_shader() {
        let bytes = std::fs::read("../../shader_dump_missing/3461ed5d7d63fb45_s0.code").unwrap();
        let sass = &bytes[0x30..];
        let cb1 = MemoryGpuAccessor::find_const_bank(&bytes);
        let accessor = MemoryGpuAccessor::new(sass, false, Some(&bytes), cb1);
        let program = decode(&accessor, 0x50, false);
        let ops = emit_shader(&program);
        assert!(!ops.is_empty());
        println!("Successfully emitted {} IR operations from real shader!", ops.len());
        for (i, op) in ops.iter().take(15).enumerate() {
            println!("  Op {}: {:?} dest={:?} src={:?}", i, op.inst, op.dest, op.sources);
        }
    }
}
