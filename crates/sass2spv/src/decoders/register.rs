pub struct RegisterConsts;

impl RegisterConsts {
    pub const GPRS_COUNT: usize = 255;
    pub const PREDS_COUNT: usize = 7;
    pub const FLAGS_COUNT: usize = 4;
    pub const TOTAL_COUNT: usize = Self::GPRS_COUNT + Self::PREDS_COUNT + Self::FLAGS_COUNT;

    pub const REGISTER_ZERO_INDEX: usize = Self::GPRS_COUNT;
    pub const PREDICATE_TRUE_INDEX: usize = Self::PREDS_COUNT;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RegisterType {
    Flag,
    Gpr,
    Predicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Register {
    pub index: usize,
    pub reg_type: RegisterType,
}

impl Register {
    pub const fn new(index: usize, reg_type: RegisterType) -> Self {
        Self { index, reg_type }
    }

    pub fn is_rz(&self) -> bool {
        self.reg_type == RegisterType::Gpr && self.index == RegisterConsts::REGISTER_ZERO_INDEX
    }

    pub fn is_pt(&self) -> bool {
        self.reg_type == RegisterType::Predicate && self.index == RegisterConsts::PREDICATE_TRUE_INDEX
    }
}
