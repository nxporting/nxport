use crate::ir::storage_kind::AggregateType;
use crate::options::{ShaderStage, TargetApi};
use crate::structured_ir::ast::{AstArena, AstNodeKind, NodeId};
use crate::structured_ir::program::IoDefinition;
use rspirv::dr::Builder;
use rspirv::spirv;
use rspirv::spirv::Word;
use rustc_hash::FxHashMap;

#[derive(Default)]
pub struct BlockState {
    pub entry_count: usize,
    pub labels: Vec<Word>,
}

impl BlockState {
    pub fn get_label(&mut self, builder: &mut Builder, index: usize) -> Word {
        while index >= self.labels.len() {
            self.labels.push(builder.id());
        }
        self.labels[index]
    }

    pub fn get_next_label(&mut self, builder: &mut Builder) -> Word {
        let idx = self.entry_count;
        self.get_label(builder, idx)
    }

    pub fn get_next_label_auto_increment(&mut self, builder: &mut Builder) -> Word {
        let idx = self.entry_count;
        self.entry_count += 1;
        self.get_label(builder, idx)
    }
}

pub struct CodeGenContext {
    pub builder: Builder,
    pub stage: ShaderStage,
    pub target_api: TargetApi,

    pub void_type: Word,
    pub bool_type: Word,
    pub u32_type: Word,
    pub s32_type: Word,
    pub f32_type: Word,
    pub f64_type: Word,

    pub v4f32_type: Word,
    pub v4u32_type: Word,
    pub v4s32_type: Word,

    pub glsl_ext_inst: Word,

    pub block_states: FxHashMap<NodeId, BlockState>,
    pub loop_targets: FxHashMap<NodeId, (Word, Word)>,
    pub locals: FxHashMap<NodeId, Word>,
    pub constant_buffers: FxHashMap<i32, Word>,
    pub storage_buffers: FxHashMap<i32, Word>,
    pub inputs: FxHashMap<IoDefinition, Word>,
    pub outputs: FxHashMap<IoDefinition, Word>,
    pub main_interface: Vec<Word>,

    int_constants: FxHashMap<(Word, u32), Word>,
    float_constants: FxHashMap<(Word, u32), Word>,
}

impl CodeGenContext {
    pub fn new(stage: ShaderStage, target_api: TargetApi) -> Self {
        let mut builder = Builder::new();
        builder.set_version(1, 3);
        builder.capability(spirv::Capability::Shader);
        builder.capability(spirv::Capability::GroupNonUniformBallot);
        builder.capability(spirv::Capability::GroupNonUniformShuffle);
        builder.capability(spirv::Capability::GroupNonUniformVote);
        builder.capability(spirv::Capability::ImageQuery);

        builder.memory_model(spirv::AddressingModel::Logical, spirv::MemoryModel::GLSL450);
        let glsl_ext_inst = builder.ext_inst_import("GLSL.std.450");

        let void_type = builder.type_void();
        let bool_type = builder.type_bool();
        let u32_type = builder.type_int(32, 0);
        let s32_type = builder.type_int(32, 1);
        let f32_type = builder.type_float(32);
        let f64_type = builder.type_float(64);

        let v4f32_type = builder.type_vector(f32_type, 4);
        let v4u32_type = builder.type_vector(u32_type, 4);
        let v4s32_type = builder.type_vector(s32_type, 4);

        Self {
            builder,
            stage,
            target_api,
            void_type,
            bool_type,
            u32_type,
            s32_type,
            f32_type,
            f64_type,
            v4f32_type,
            v4u32_type,
            v4s32_type,
            glsl_ext_inst,
            block_states: FxHashMap::default(),
            loop_targets: FxHashMap::default(),
            locals: FxHashMap::default(),
            constant_buffers: FxHashMap::default(),
            storage_buffers: FxHashMap::default(),
            inputs: FxHashMap::default(),
            outputs: FxHashMap::default(),
            main_interface: Vec::new(),
            int_constants: FxHashMap::default(),
            float_constants: FxHashMap::default(),
        }
    }

    pub fn get_type(&mut self, agg_type: AggregateType) -> Word {
        let elem_type = match agg_type.element_type() {
            AggregateType::BOOL => self.bool_type,
            AggregateType::FP64 => self.f64_type,
            AggregateType::S32 => self.s32_type,
            AggregateType::U32 => self.u32_type,
            AggregateType::VOID => self.void_type,
            _ => self.f32_type,
        };

        match agg_type.element_count() {
            2 => self.builder.type_vector(elem_type, 2),
            3 => self.builder.type_vector(elem_type, 3),
            4 => self.builder.type_vector(elem_type, 4),
            _ => elem_type,
        }
    }

    pub fn const_u32(&mut self, val: u32) -> Word {
        let key = (self.u32_type, val);
        if let Some(&id) = self.int_constants.get(&key) {
            id
        } else {
            let id = self.builder.constant_bit32(self.u32_type, val);
            self.int_constants.insert(key, id);
            id
        }
    }

    pub fn const_s32(&mut self, val: i32) -> Word {
        let key = (self.s32_type, val as u32);
        if let Some(&id) = self.int_constants.get(&key) {
            id
        } else {
            let id = self.builder.constant_bit32(self.s32_type, val as u32);
            self.int_constants.insert(key, id);
            id
        }
    }

    pub fn const_f32(&mut self, val: f32) -> Word {
        let key = (self.f32_type, val.to_bits());
        if let Some(&id) = self.float_constants.get(&key) {
            id
        } else {
            let id = self.builder.constant_bit32(self.f32_type, val.to_bits());
            self.float_constants.insert(key, id);
            id
        }
    }

    pub fn const_bool(&mut self, val: bool) -> Word {
        if val {
            self.builder.constant_true(self.bool_type)
        } else {
            self.builder.constant_false(self.bool_type)
        }
    }

    pub fn get_value(&mut self, arena: &AstArena, node_id: NodeId, desired_type: AggregateType) -> Word {
        match &arena.nodes[node_id.0].kind {
            AstNodeKind::Operand(oper) => match oper.op_type {
                crate::ir::operand::OperandType::Constant => match desired_type.element_type() {
                    AggregateType::BOOL => self.const_bool(oper.value != 0),
                    AggregateType::FP32 => self.const_f32(f32::from_bits(oper.value as u32)),
                    AggregateType::S32 => self.const_s32(oper.value),
                    _ => self.const_u32(oper.value as u32),
                },
                crate::ir::operand::OperandType::LocalVariable
                | crate::ir::operand::OperandType::Register => {
                    let ptr = self.locals.get(&node_id).copied().expect("Local variable must be declared");
                    let var_type = self.get_type(oper.var_type);
                    self.builder.load(var_type, None, ptr, None, vec![]).unwrap()
                }
                _ => self.const_u32(0),
            },
            _ => self.const_u32(0),
        }
    }

    pub fn enter_block(&mut self, block: NodeId) {
        let label = self.get_next_label_auto_increment(block);
        let _ = self.builder.begin_block(Some(label));
    }

    pub fn get_first_label(&mut self, block: NodeId) -> Word {
        let state = self.block_states.entry(block).or_default();
        state.get_label(&mut self.builder, 0)
    }

    pub fn get_next_label(&mut self, block: NodeId) -> Word {
        let state = self.block_states.entry(block).or_default();
        state.get_next_label(&mut self.builder)
    }

    pub fn get_next_label_auto_increment(&mut self, block: NodeId) -> Word {
        let state = self.block_states.entry(block).or_default();
        state.get_next_label_auto_increment(&mut self.builder)
    }

    pub fn new_block(&mut self) -> Word {
        let label = self.builder.id();
        let _ = self.builder.branch(label);
        let _ = self.builder.begin_block(Some(label));
        label
    }
}
