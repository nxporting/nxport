use super::inst_emitter::InstEmitter;
use super::inst_name::InstName;
use super::inst_props::InstProps;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstOp {
    pub address: u64,
    pub raw_op_code: u64,
    pub name: InstName,
    pub emitter: Option<InstEmitter>,
    pub props: InstProps,
}

impl InstOp {
    pub const fn new(
        address: u64,
        raw_op_code: u64,
        name: InstName,
        emitter: Option<InstEmitter>,
        props: InstProps,
    ) -> Self {
        Self {
            address,
            raw_op_code,
            name,
            emitter,
            props,
        }
    }

    pub fn get_absolute_address(&self) -> u64 {
        let offset = ((self.raw_op_code >> 20) as i32) << 8 >> 8;
        ((self.address as i64) + (offset as i64) + 8) as u64
    }
}
