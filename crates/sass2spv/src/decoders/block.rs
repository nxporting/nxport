use super::inst_name::InstName;
use super::inst_op::InstOp;
use super::inst_props::InstProps;
use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub struct PushOpInfo {
    pub op: InstOp,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub address: u64,
    pub end_address: u64,
    pub predecessors: Vec<usize>,
    pub successors: Vec<usize>,
    pub op_codes: Vec<InstOp>,
    pub push_op_codes: Vec<PushOpInfo>,
    pub sync_targets: FxHashMap<u64, usize>,
    pub brx_targets: Vec<u64>,
}

impl Block {
    pub fn new(address: u64) -> Self {
        Self {
            address,
            end_address: 0,
            predecessors: Vec::new(),
            successors: Vec::new(),
            op_codes: Vec::new(),
            push_op_codes: Vec::new(),
            sync_targets: FxHashMap::default(),
            brx_targets: Vec::new(),
        }
    }

    pub fn add_push_op(&mut self, op: InstOp) {
        self.push_op_codes.push(PushOpInfo { op });
    }

    pub fn get_last_op(&self) -> Option<InstOp> {
        self.op_codes.last().copied()
    }

    pub fn is_unconditional_branch(op: &InstOp) -> bool {
        if !op.props.contains(InstProps::BRA) {
            return false;
        }
        let cond = crate::decoders::inst_decoders::InstConditional::new(op.raw_op_code);
        if (op.name == InstName::Bra || op.name == InstName::Exit) && cond.ccc() != crate::decoders::inst_decoders::Ccc::T {
            return false;
        }
        cond.pred() == crate::decoders::register::RegisterConsts::PREDICATE_TRUE_INDEX as i32 && !cond.pred_inv()
    }
}
