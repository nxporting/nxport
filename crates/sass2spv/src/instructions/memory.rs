use super::emit_helper::*;
use crate::ir::instruction::Instruction;
use crate::ir::operand::Operand;
use crate::translation::emitter_context::EmitterContext;

pub fn emit_ldc(context: &mut EmitterContext, cbuf_slot: i32, cbuf_offset: i32, rd: i32) {
    let cbuf = get_src_cbuf(cbuf_slot, cbuf_offset);
    context.copy(get_dest(rd), cbuf);
}

pub fn emit_ald(context: &mut EmitterContext, offset: i32, count: usize, rd: i32) {
    // ALD loads vertex attributes into registers rd, rd+1, ...
    for i in 0..count {
        let attr_offset = offset + (i as i32) * 4;
        let attr_op = Operand::constant(attr_offset);
        let val = context.alloc_var();
        context.add(Instruction::Load, Some(val), vec![attr_op]);
        context.copy(get_dest(rd + i as i32), val);
    }
}

pub fn emit_ast(context: &mut EmitterContext, offset: i32, count: usize, src: i32) {
    // AST stores values from registers src, src+1 into output attributes
    for i in 0..count {
        let attr_offset = Operand::constant(offset + (i as i32) * 4);
        let val = get_src_reg(src + i as i32);
        context.add(Instruction::Store, None, vec![attr_offset, val]);
    }
}

pub fn emit_ipa(context: &mut EmitterContext, imm10: i32, rd: i32) {
    let attr_offset = Operand::constant(imm10);
    let val = context.alloc_var();
    context.add(Instruction::Load, Some(val), vec![attr_offset]);
    context.copy(get_dest(rd), val);
}

pub fn emit_ldg(context: &mut EmitterContext, src_addr: Operand, rd: i32) {
    let val = context.alloc_var();
    context.add(Instruction::Load, Some(val), vec![src_addr]);
    context.copy(get_dest(rd), val);
}

