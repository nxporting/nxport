// Maxwell registers: general purpose, predicates and condition code flags.

pub const GPRS_COUNT: i32 = 255;
pub const PREDS_COUNT: i32 = 7;
pub const FLAGS_COUNT: i32 = 4;
pub const TOTAL_COUNT: i32 = GPRS_COUNT + PREDS_COUNT + FLAGS_COUNT;
pub const REGISTER_ZERO_INDEX: i32 = GPRS_COUNT;
pub const PREDICATE_TRUE_INDEX: i32 = PREDS_COUNT;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegisterType {
    Flag,
    Gpr,
    Predicate,
}

impl RegisterType {
    pub fn from_i32(v: i32) -> RegisterType {
        match v {
            0 => RegisterType::Flag,
            1 => RegisterType::Gpr,
            2 => RegisterType::Predicate,
            _ => panic!("invalid register type {v}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Register {
    pub index: i32,
    pub ty: RegisterType,
}

impl Register {
    pub fn new(index: i32, ty: RegisterType) -> Register {
        Register { index, ty }
    }

    pub fn is_rz(&self) -> bool {
        self.ty == RegisterType::Gpr && self.index == REGISTER_ZERO_INDEX
    }

    pub fn is_pt(&self) -> bool {
        self.ty == RegisterType::Predicate && self.index == PREDICATE_TRUE_INDEX
    }
}
