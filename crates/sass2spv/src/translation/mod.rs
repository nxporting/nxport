// Turns a decoded stage into IR and runs the passes up to code generation (Translator and TranslatorContext).

pub mod cfg;
pub mod definitions;
pub mod emitter;
pub mod function_match;
pub mod helper_functions;
pub mod optimizations;
pub mod properties;
pub mod register_usage;
pub mod resource_manager;
pub mod ssa;
pub mod transforms;

use crate::accessor::GpuAccessor;
use crate::decoders::inst_decoders::InstConditional;
use crate::decoders::register::PREDICATE_TRUE_INDEX;
use crate::decoders::{self, BlockIdx, DecodedProgram, FunctionIdx, InstName, InstProps};
use crate::ir::{IoVariable, Ir, NodeId, StorageKind};
use crate::types::ShaderStage;
use definitions::{FeatureFlags, ShaderDefinitions, ShaderHeader};
use emitter::{Emit, EmitterContext};
use resource_manager::ResourceManager;

const THREADS_PER_WARP: i32 = 32;
const HEADER_SIZE: u64 = 0x50;

/// What the optimisation and transformation passes work on (TransformContext).
pub struct TransformContext<'a> {
    pub hfm: &'a mut helper_functions::HelperFunctionManager,
    pub ir: &'a mut Ir,
    pub blocks: &'a [crate::ir::BlockId],
    pub definitions: &'a ShaderDefinitions,
    pub rm: &'a mut ResourceManager,
    pub gpu: &'a GpuAccessor,
    pub stage: ShaderStage,
    pub used_features: &'a mut FeatureFlags,
}

/// A function's operations, in order (Translator.FunctionCode).
pub struct FunctionCode {
    pub code: Vec<NodeId>,
}

/// What the translator knows about the stage (TranslatorContext).
pub struct TranslatorContext {
    pub address: u64,
    pub size: i32,
    pub local_memory_size: i32,
    pub definitions: ShaderDefinitions,
    pub gpu: GpuAccessor,
    pub is_compute: bool,
}

impl TranslatorContext {
    pub fn stage(&self) -> ShaderStage {
        self.definitions.stage
    }

    /// The depth register is two registers after the last color output.
    pub fn depth_register(&self) -> i32 {
        (self.definitions.omap_targets as u32).count_ones() as i32 + 1
    }
}

/// A decoded stage, ready to translate (Translator.CreateContext).
pub struct Decoded {
    pub tc: TranslatorContext,
    pub program: DecodedProgram,
    pub ir: Ir,
}

pub fn create_context(address: u64, gpu: GpuAccessor, is_compute: bool) -> Result<Decoded, String> {
    let mut ir = Ir::new();
    let (mut definitions, local_memory_size, start) = if is_compute {
        (ShaderDefinitions::compute(gpu.local_size), gpu.local_memory_size, address)
    } else {
        let header = ShaderHeader::new(&gpu, address)?;
        let size = header.shader_local_memory_low_size
            + header.shader_local_memory_high_size
            + header.shader_local_memory_crs_size / THREADS_PER_WARP;
        (ShaderDefinitions::graphics(&gpu, &header), size, address + HEADER_SIZE)
    };
    let program = decoders::decode(&mut definitions, &gpu, &mut ir, start)?;
    let mut max_end_address = 0;
    for f in program.function_indices() {
        for &b in &program.function(f).blocks {
            max_end_address = max_end_address.max(program.block(b).end_address);
        }
    }
    let size = max_end_address as i32 + if is_compute { 0 } else { HEADER_SIZE as i32 };
    let tc = TranslatorContext { address, size, local_memory_size, definitions, gpu, is_compute };
    Ok(Decoded { tc, program, ir })
}

impl Decoded {
    pub fn stage(&self) -> ShaderStage {
        self.tc.stage()
    }

    /// Emits every function's operations (Translator.EmitShader); the resource manager collects what they use.
    pub fn emit_shader(&mut self, rm: &mut ResourceManager) -> Result<Vec<FunctionCode>, String> {
        function_match::run_pass(&mut self.program);
        let mut user: Vec<FunctionIdx> =
            self.program.function_indices().filter(|&f| !self.program.function(f).is_compiler_generated()).collect();
        // OrderBy is a stable sort.
        user.sort_by_key(|&f| self.program.function(f).address);
        for f in user {
            self.program.add_function_and_set_id(f);
        }
        let count = self.program.functions_with_id_count();
        let mut functions = Vec::with_capacity(count);
        for index in 0..count {
            let mut c = EmitterContext::new(&mut self.ir, &self.tc, &self.program, rm, index != 0);
            if index == 0 {
                emit_outputs_initialization(&mut c);
            }
            let function = self.program.function_by_id(index as i32);
            for &block in &self.program.function(function).blocks {
                c.curr_block = block;
                c.enter_block(self.program.block(block).address);
                emit_ops(&mut c, block);
            }
            if let Some(e) = c.error.take() {
                return Err(e);
            }
            functions.push(FunctionCode { code: c.operations() });
        }
        Ok(functions)
    }
}

/// Outputs the next stage reads must hold a value (EmitOutputsInitialization). A single stage has no next stage, so
/// only the vertex stage's position and clip distances are initialised.
fn emit_outputs_initialization(c: &mut EmitterContext) {
    match c.stage() {
        ShaderStage::Compute | ShaderStage::Fragment => {}
        ShaderStage::Vertex => {
            for comp in 0..4 {
                let (ci, v) = (c.const_i(comp), c.const_f(if comp == 3 { 1.0 } else { 0.0 }));
                c.store_io(StorageKind::Output, IoVariable::Position, None, &[ci, v]);
            }
            if c.program.clip_distances_written != 0 {
                for i in 0..8 {
                    let (ci, v) = (c.const_i(i), c.const_f(0.0));
                    c.store_io(StorageKind::Output, IoVariable::ClipDistance, None, &[ci, v]);
                }
            }
        }
        _ => {}
    }
}

fn emit_ops(c: &mut EmitterContext, block: BlockIdx) {
    let program = c.program;
    let b = program.block(block);
    for (op_index, &op) in b.op_codes.iter().enumerate() {
        let cond = InstConditional(op.raw_op_code);
        let mut no_pred = op.props.has(InstProps::NO_PRED);
        if !no_pred && cond.pred() == PREDICATE_TRUE_INDEX && cond.pred_inv() {
            continue;
        }
        let mut pred_skip_lbl = None;
        if decoders::is_pop_branch(op.name) {
            // A SYNC or BRK with one possible target is a plain branch: the condition goes on the branch itself.
            no_pred = b.sync_targets.len() <= 1;
        } else if op.name == InstName::Bra {
            no_pred = true;
        }
        if !(cond.pred() == PREDICATE_TRUE_INDEX || no_pred) {
            let label = if op_index == b.op_codes.len() - 1 && b.has_next() {
                c.get_label(program.block(b.successors[0]).address)
            } else {
                let l = c.label();
                pred_skip_lbl = Some(l);
                l
            };
            let pred = c.pred(cond.pred());
            if cond.pred_inv() {
                c.branch_if_true(label, pred);
            } else {
                c.branch_if_false(label, pred);
            }
        }
        c.curr_op = op;
        if let Some(emit) = op.emitter {
            emit(c);
        }
        if let Some(l) = pred_skip_lbl {
            c.mark_label(l);
        }
    }
}

/// The rest of TranslatorContext.Translate: control flow graphs, SSA, optimisation, structuring and SPIR-V.
pub fn translate(d: Decoded) -> Result<crate::ShaderProgram, String> {
    translate_to(d, &crate::pipeline::Target::Spirv)
}

pub fn translate_to(mut d: Decoded, target: &crate::pipeline::Target) -> Result<crate::ShaderProgram, String> {
    let mut rm = ResourceManager::new(d.stage(), d.tc.gpu.set_for(0));
    let uses_local_memory = d.program.used_features.has(FeatureFlags::LOCAL_MEMORY);
    rm.set_current_local_memory(d.tc.local_memory_size, uses_local_memory);
    if d.stage() == ShaderStage::Compute {
        let uses_shared_memory = d.program.used_features.has(FeatureFlags::SHARED_MEMORY);
        rm.set_current_shared_memory(d.tc.gpu.shared_memory_size, uses_shared_memory);
    }
    let code = d.emit_shader(&mut rm)?;
    let used_features = d.program.used_features;
    let clip_distances_written = d.program.clip_distances_written;
    crate::pipeline::translate_functions(d, code, rm, used_features, clip_distances_written, target)
}
