use crate::decoders::register::{Register, RegisterType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperandType {
    Argument,
    Constant,
    ConstantBuffer,
    Label,
    LocalVariable,
    Register,
    Undefined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Operand {
    pub op_type: OperandType,
    pub value: i32,
}

const CBUF_SLOT_BITS: i32 = 5;
const CBUF_SLOT_LSB: i32 = 32 - CBUF_SLOT_BITS;
const CBUF_SLOT_MASK: i32 = (1 << CBUF_SLOT_BITS) - 1;

impl Operand {
    pub const fn new(op_type: OperandType, value: i32) -> Self {
        Self { op_type, value }
    }

    pub const fn undef() -> Self {
        Self {
            op_type: OperandType::Undefined,
            value: 0,
        }
    }

    pub const fn constant(val: i32) -> Self {
        Self {
            op_type: OperandType::Constant,
            value: val,
        }
    }

    pub fn constant_f32(val: f32) -> Self {
        Self {
            op_type: OperandType::Constant,
            value: val.to_bits() as i32,
        }
    }

    pub const fn constant_buffer(slot: i32, offset: i32) -> Self {
        let value = (slot << CBUF_SLOT_LSB) | offset;
        Self {
            op_type: OperandType::ConstantBuffer,
            value,
        }
    }

    pub const fn register(index: usize, reg_type: RegisterType) -> Self {
        let value = ((reg_type as i32) << 24) | (index as i32);
        Self {
            op_type: OperandType::Register,
            value,
        }
    }

    pub const fn local_var(id: u32) -> Self {
        Self {
            op_type: OperandType::LocalVariable,
            value: id as i32,
        }
    }

    pub const fn label(address: u64) -> Self {
        Self {
            op_type: OperandType::Label,
            value: address as i32,
        }
    }

    pub fn get_cbuf_slot(&self) -> i32 {
        (self.value >> CBUF_SLOT_LSB) & CBUF_SLOT_MASK
    }

    pub fn get_cbuf_offset(&self) -> i32 {
        self.value & !(CBUF_SLOT_MASK << CBUF_SLOT_LSB)
    }

    pub fn get_register(&self) -> Register {
        let index = (self.value & 0xffffff) as usize;
        let reg_type = match (self.value >> 24) as u8 {
            0 => RegisterType::Flag,
            1 => RegisterType::Gpr,
            _ => RegisterType::Predicate,
        };
        Register::new(index, reg_type)
    }

    pub fn as_float(&self) -> f32 {
        f32::from_bits(self.value as u32)
    }
}
