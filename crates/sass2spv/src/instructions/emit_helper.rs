use crate::decoders::register::RegisterType;
use crate::ir::operand::Operand;

pub fn get_zf() -> Operand {
    Operand::register(0, RegisterType::Flag)
}

pub fn get_nf() -> Operand {
    Operand::register(1, RegisterType::Flag)
}

pub fn get_cf() -> Operand {
    Operand::register(2, RegisterType::Flag)
}

pub fn get_vf() -> Operand {
    Operand::register(3, RegisterType::Flag)
}

pub fn get_dest(rd: i32) -> Operand {
    Operand::register(rd as usize, RegisterType::Gpr)
}

pub fn get_dest2(rd: i32) -> Operand {
    Operand::register((rd | 1) as usize, RegisterType::Gpr)
}

pub fn get_src_reg(reg: i32) -> Operand {
    Operand::register(reg as usize, RegisterType::Gpr)
}

pub fn get_src_cbuf(cbuf_slot: i32, cbuf_offset: i32) -> Operand {
    Operand::constant_buffer(cbuf_slot, cbuf_offset)
}

pub fn get_src_imm(imm: i32) -> Operand {
    Operand::constant(imm)
}

pub fn imm20_to_sint(imm20: i32) -> i32 {
    (imm20 << 12) >> 12
}

pub fn get_predicate(pred: i32, inv: bool) -> Operand {
    if pred == 7 {
        if inv {
            Operand::constant(0)
        } else {
            Operand::constant(1)
        }
    } else {
        Operand::register(pred as usize, RegisterType::Predicate)
    }
}

