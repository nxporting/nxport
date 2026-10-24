// SPIR-V from the structured program (CodeGen.Spirv: SpirvGenerator and CodeGenContext). Every instruction is built in
// the original's order, so ids, deduplication and therefore the binary are the same.

mod declarations;
mod instructions;

use std::collections::HashMap;

use crate::collections::NetMap;
use crate::ir::{ir_consts, IoVariable, OperandType, StorageKind};
use crate::spv::*;
use crate::structured::{AstBlockType, AstBlockVisitor, AstId, AstKind, IoDefinition, StructuredProgramInfo, VisitEvent};
use crate::translation::definitions::{AggregateType as A, ShaderDefinitions};
use crate::translation::optimizations::utils::NRE;
use crate::translation::properties::ShaderProperties;
use crate::types::{InputTopology, OutputTopology, SamplerType, SetBindingPair, ShaderStage, TessPatchType, TessSpacing};

type R<T> = Result<T, String>;

const SPIRV_VERSION_PACKED: u32 = (1 << 16) | (3 << 8);

/// What the host supports (HostCapabilities), as the engine's accessor answers.
pub struct HostCapabilities {
    pub reduced_precision: bool,
    pub supports_fragment_shader_interlock: bool,
    pub supports_geometry_shader_passthrough: bool,
    pub supports_shader_barrier_divergence: bool,
    pub supports_shader_float64: bool,
    pub supports_shader_non_uniform_indexing: bool,
    pub supports_image_gather_extended: bool,
    pub supports_draw_parameters: bool,
    pub cbuf_limits: bool,
    pub binding_base: i32,
}

#[derive(Clone)]
pub struct SamplerDeclaration {
    pub image_type: Inst,
    pub sampled_image_type: Inst,
    pub sampled_image_pointer_type: Inst,
    pub image: Inst,
    pub is_indexed: bool,
}

#[derive(Clone)]
pub struct ImageDeclaration {
    pub image_type: Inst,
    pub image_pointer_type: Inst,
    pub image: Inst,
    pub is_indexed: bool,
}

#[derive(Default)]
struct BlockState {
    entry_count: usize,
    labels: Vec<Inst>,
}

pub struct CodeGenContext<'a> {
    pub m: Module,
    pub info: &'a StructuredProgramInfo,
    pub definitions: &'a ShaderDefinitions,
    pub properties: &'a ShaderProperties,
    pub host: &'a HostCapabilities,

    pub constant_buffers: HashMap<i32, Inst>,
    pub storage_buffers: HashMap<i32, Inst>,
    pub local_memories: HashMap<i32, Inst>,
    pub shared_memories: HashMap<i32, Inst>,
    pub samplers_types: HashMap<SetBindingPair, SamplerType>,
    pub samplers: HashMap<SetBindingPair, SamplerDeclaration>,
    pub images: HashMap<SetBindingPair, ImageDeclaration>,
    pub inputs: NetMap<IoDefinition, Inst>,
    pub outputs: NetMap<IoDefinition, Inst>,
    pub inputs_per_patch: NetMap<IoDefinition, Inst>,
    pub outputs_per_patch: NetMap<IoDefinition, Inst>,

    pub current_function: usize,
    locals: HashMap<AstId, Inst>,
    func_args: HashMap<i32, Inst>,
    functions: HashMap<i32, (usize, Inst)>,
    labels: HashMap<AstId, BlockState>,
    pub loop_targets: HashMap<AstId, (Inst, Inst)>,
    pub current_block: AstId,
    pub is_main_function: bool,
    pub may_have_returned: bool,
    pub was_non_uniform_access_declared: bool,
}

impl CodeGenContext<'_> {
    pub fn start_function(&mut self, is_main_function: bool) {
        self.is_main_function = is_main_function;
        self.may_have_returned = false;
        self.locals.clear();
        self.func_args.clear();
    }

    pub fn enter_block(&mut self, block: AstId) {
        self.current_block = block;
        let l = self.next_label_auto_increment(block);
        self.m.add_label(&l);
    }

    fn label_at(&mut self, block: AstId, index: usize) -> Inst {
        while index >= self.labels.entry(block).or_default().labels.len() {
            let l = self.m.label();
            self.labels.get_mut(&block).unwrap().labels.push(l);
        }
        self.labels[&block].labels[index].clone()
    }

    fn next_label_auto_increment(&mut self, block: AstId) -> Inst {
        let i = self.labels.entry(block).or_default().entry_count;
        self.labels.get_mut(&block).unwrap().entry_count += 1;
        self.label_at(block, i)
    }

    pub fn first_label(&mut self, block: AstId) -> Inst {
        self.label_at(block, 0)
    }

    pub fn next_label(&mut self, block: AstId) -> Inst {
        let i = self.labels.entry(block).or_default().entry_count;
        self.label_at(block, i)
    }

    pub fn new_block(&mut self) -> Inst {
        let label = self.m.label();
        self.m.branch(&label);
        self.m.add_label(&label);
        label
    }

    pub fn main_interface(&self) -> Vec<Inst> {
        let mut v: Vec<Inst> = self.inputs.values().cloned().collect();
        v.extend(self.outputs.values().cloned());
        v.extend(self.inputs_per_patch.values().cloned());
        v.extend(self.outputs_per_patch.values().cloned());
        v
    }

    pub fn declare_local(&mut self, local: AstId, spv_local: Inst) -> R<()> {
        if self.locals.insert(local, spv_local).is_some() {
            return Err("System.ArgumentException: An item with the same key has already been added.".into());
        }
        Ok(())
    }

    pub fn declare_argument(&mut self, arg_index: i32, spv_local: Inst) {
        self.func_args.insert(arg_index, spv_local);
    }

    pub fn get_fp32(&mut self, node: Option<AstId>) -> R<Inst> {
        self.get(A::FP32, node)
    }

    pub fn get_fp64(&mut self, node: Option<AstId>) -> R<Inst> {
        self.get(A::FP64, node)
    }

    pub fn get_s32(&mut self, node: Option<AstId>) -> R<Inst> {
        self.get(A::S32, node)
    }

    pub fn get_u32(&mut self, node: Option<AstId>) -> R<Inst> {
        self.get(A::U32, node)
    }

    pub fn get(&mut self, ty: A, node: Option<AstId>) -> R<Inst> {
        let node = node.ok_or(NRE)?;
        let info = self.info;
        match info.ast.kind(node) {
            AstKind::Operation(_) => {
                let (rt, value) = instructions::generate(self, node)?;
                self.bitcast_if_needed(ty, rt, value.ok_or(NRE)?)
            }
            AstKind::Operand(o) => match o.ty {
                OperandType::Argument => self.get_argument(ty, node),
                OperandType::Constant => self.get_constant(ty, node),
                OperandType::LocalVariable => self.get_local(ty, node),
                OperandType::Undefined => Ok(self.get_undefined(ty)),
                t => Err(format!("System.ArgumentException: Invalid operand type \"{t:?}\".")),
            },
            _ => Err("System.NotImplementedException".into()),
        }
    }

    pub fn get_with_type(&mut self, node: Option<AstId>) -> R<(A, Inst)> {
        let node = node.ok_or(NRE)?;
        let info = self.info;
        match info.ast.kind(node) {
            AstKind::Operation(_) => {
                let (t, v) = instructions::generate(self, node)?;
                Ok((t, v.ok_or(NRE)?))
            }
            AstKind::Operand(o) if o.ty == OperandType::LocalVariable => {
                let t = o.var_type;
                Ok((t, self.get_local(t, node)?))
            }
            AstKind::Operand(o) => Err(format!("System.ArgumentException: Invalid operand type \"{:?}\".", o.ty)),
            _ => Err("System.NotImplementedException".into()),
        }
    }

    fn get_undefined(&mut self, ty: A) -> Inst {
        match ty {
            A::BOOL => {
                let b = self.type_bool();
                self.m.constant_false(&b)
            }
            A::FP32 => {
                let t = self.type_fp32();
                self.m.constant(&t, Lit::f32(0.0))
            }
            A::FP64 => {
                let t = self.type_fp64();
                self.m.constant(&t, Lit::f64(0.0))
            }
            _ => {
                // GetType throws for anything else; Undefined operands are only read as these types.
                let t = self.get_type(ty, 1).expect("undefined of an invalid type");
                self.m.constant(&t, Lit::i32(0))
            }
        }
    }

    pub fn get_constant(&mut self, ty: A, operand: AstId) -> R<Inst> {
        let value = self.info.ast.opnd(operand).value;
        Ok(match ty {
            A::BOOL => {
                let b = self.type_bool();
                if value != 0 { self.m.constant_true(&b) } else { self.m.constant_false(&b) }
            }
            A::FP32 => {
                let t = self.type_fp32();
                self.m.constant(&t, Lit::f32(f32::from_bits(value as u32)))
            }
            A::FP64 => {
                let t = self.type_fp64();
                self.m.constant(&t, Lit::f64(f32::from_bits(value as u32) as f64))
            }
            A::S32 => {
                let t = self.type_s32();
                self.m.constant(&t, Lit::i32(value))
            }
            A::U32 => {
                let t = self.type_u32();
                self.m.constant(&t, Lit::u32(value as u32))
            }
            t => return Err(format!("System.ArgumentException: Invalid type \"{}\".", t.0)),
        })
    }

    pub fn local_pointer(&self, local: AstId) -> R<Inst> {
        self.locals.get(&local).cloned().ok_or_else(|| "System.Collections.Generic.KeyNotFoundException".into())
    }

    pub fn argument_pointer(&self, func_arg: AstId) -> R<Inst> {
        let v = self.info.ast.opnd(func_arg).value;
        self.func_args.get(&v).cloned().ok_or_else(|| "System.Collections.Generic.KeyNotFoundException".into())
    }

    pub fn get_local(&mut self, dst: A, local: AstId) -> R<Inst> {
        let src = self.info.ast.opnd(local).var_type;
        let t = self.get_type(src, 1)?;
        let p = self.local_pointer(local)?;
        let v = self.m.load(&t, &p, None);
        self.bitcast_if_needed(dst, src, v)
    }

    pub fn get_argument(&mut self, dst: A, func_arg: AstId) -> R<Inst> {
        let src = self.info.ast.opnd(func_arg).var_type;
        let t = self.get_type(src, 1)?;
        let p = self.argument_pointer(func_arg)?;
        let v = self.m.load(&t, &p, None);
        self.bitcast_if_needed(dst, src, v)
    }

    pub fn function(&self, func_index: i32) -> R<(usize, Inst)> {
        self.functions.get(&func_index).cloned().ok_or_else(|| "System.Collections.Generic.KeyNotFoundException".into())
    }

    pub fn get_type(&mut self, ty: A, length: i32) -> R<Inst> {
        if ty.has(A::ARRAY) {
            let elem = self.get_type(ty & !A::ARRAY, 1)?;
            if length > 0 {
                let u = self.type_u32();
                let len = self.m.constant(&u, Lit::i32(length));
                return Ok(self.m.type_array(&elem, &len, false));
            }
            return Ok(self.m.type_runtime_array(&elem, false));
        } else if ty.0 & A::ELEMENT_COUNT_MASK.0 != 0 {
            let n = match ty & A::ELEMENT_COUNT_MASK {
                A::VECTOR2 => 2,
                A::VECTOR3 => 3,
                A::VECTOR4 => 4,
                _ => 1,
            };
            let elem = self.get_type(ty & !A::ELEMENT_COUNT_MASK, 1)?;
            return Ok(self.m.type_vector(&elem, Lit::i32(n), false));
        }
        Ok(match ty {
            A::VOID => self.m.type_void(false),
            A::BOOL => self.type_bool(),
            A::FP32 => self.type_fp32(),
            A::FP64 => self.type_fp64(),
            A::S32 => self.type_s32(),
            A::U32 => self.type_u32(),
            t => return Err(format!("System.ArgumentException: Invalid attribute type \"{}\".", t.0)),
        })
    }

    pub fn bitcast_if_needed(&mut self, dst: A, src: A, value: Inst) -> R<Inst> {
        if dst == src {
            return Ok(value);
        }
        if dst == A::BOOL {
            let b = self.type_bool();
            let v = self.bitcast_if_needed(A::S32, src, value)?;
            let s = self.type_s32();
            let z = self.m.constant(&s, Lit::i32(0));
            Ok(self.m.i_not_equal(&b, &v, &z))
        } else if src == A::BOOL {
            let s = self.type_s32();
            let int_true = self.m.constant(&s, Lit::i32(ir_consts::TRUE));
            let s = self.type_s32();
            let int_false = self.m.constant(&s, Lit::i32(ir_consts::FALSE));
            let s = self.type_s32();
            let sel = self.m.select(&s, &value, &int_true, &int_false);
            self.bitcast_if_needed(dst, A::S32, sel)
        } else {
            let t = self.get_type(dst, 1)?;
            Ok(self.m.bitcast(&t, &value))
        }
    }

    pub fn type_bool(&mut self) -> Inst {
        self.m.type_bool(false)
    }

    pub fn type_s32(&mut self) -> Inst {
        self.m.type_int(Lit::i32(32), Lit::bool(true), false)
    }

    pub fn type_u32(&mut self) -> Inst {
        self.m.type_int(Lit::i32(32), Lit::bool(false), false)
    }

    pub fn type_fp32(&mut self) -> Inst {
        self.m.type_float(Lit::i32(32), false)
    }

    pub fn type_fp64(&mut self) -> Inst {
        self.m.type_float(Lit::i32(64), false)
    }

    /// Constant(TypeU32(), value) with an int literal.
    pub fn const_u32_i(&mut self, value: i32) -> Inst {
        let t = self.type_u32();
        self.m.constant(&t, Lit::i32(value))
    }
}

fn execution_model(stage: ShaderStage) -> ExecutionModel {
    match stage {
        ShaderStage::Compute => ExecutionModel::GL_COMPUTE,
        ShaderStage::Vertex => ExecutionModel::VERTEX,
        ShaderStage::TessellationControl => ExecutionModel::TESSELLATION_CONTROL,
        ShaderStage::TessellationEvaluation => ExecutionModel::TESSELLATION_EVALUATION,
        ShaderStage::Geometry => ExecutionModel::GEOMETRY,
        ShaderStage::Fragment => ExecutionModel::FRAGMENT,
    }
}

fn io(storage_kind: StorageKind, io_variable: IoVariable) -> IoDefinition {
    IoDefinition { storage_kind, io_variable, location: 0, component: 0 }
}

/// Interface changes made before code generation: the lane id that SwizzleAdd reads, and no subgroup built-ins in
/// stages where subgroups are single lanes.
pub fn adjust_io_definitions(info: &mut StructuredProgramInfo, stage: ShaderStage) {
    if info.helper_functions_mask.0 & crate::structured::HelperFunctionsMask::SWIZZLE_ADD != 0 {
        info.io_definitions.insert(io(StorageKind::Input, IoVariable::SubgroupLaneId));
    }
    if instructions::single_lane_subgroups(stage) {
        // Subgroup built-ins are constants in these stages (see instructions::single_lane_subgroups).
        let remove: Vec<IoDefinition> = info
            .io_definitions
            .iter()
            .filter(|d| {
                d.storage_kind == StorageKind::Input
                    && matches!(
                        d.io_variable,
                        IoVariable::SubgroupLaneId
                            | IoVariable::SubgroupEqMask
                            | IoVariable::SubgroupGeMask
                            | IoVariable::SubgroupGtMask
                            | IoVariable::SubgroupLeMask
                            | IoVariable::SubgroupLtMask
                    )
            })
            .copied()
            .collect();
        for d in remove {
            info.io_definitions.remove(&d);
        }
    }

}

pub use instructions::single_lane_subgroups;

/// SpirvGenerator.Generate.
pub fn generate(
    info: &mut StructuredProgramInfo,
    definitions: &ShaderDefinitions,
    properties: &ShaderProperties,
    host: &HostCapabilities,
) -> R<Vec<u8>> {
    let mut m = Module::new(SPIRV_VERSION_PACKED);
    m.add_capability(Capability::SHADER);
    m.set_memory_model(AddressingModel::LOGICAL, MemoryModel::GLSL450);
    m.add_capability(Capability::GROUP_NON_UNIFORM_BALLOT);
    m.add_capability(Capability::GROUP_NON_UNIFORM_SHUFFLE);
    m.add_capability(Capability::GROUP_NON_UNIFORM_VOTE);
    m.add_capability(Capability::IMAGE_BUFFER);
    if host.supports_image_gather_extended {
        m.add_capability(Capability::IMAGE_GATHER_EXTENDED);
    }
    m.add_capability(Capability::IMAGE_QUERY);
    m.add_capability(Capability::SAMPLED_BUFFER);
    if host.supports_shader_float64 {
        m.add_capability(Capability::FLOAT64);
    }
    // No transform feedback.
    let stage = definitions.stage;
    let ios = &info.io_definitions;
    match stage {
        ShaderStage::Fragment => {
            if ios.contains(&io(StorageKind::Input, IoVariable::Layer)) || ios.contains(&io(StorageKind::Input, IoVariable::PrimitiveId)) {
                m.add_capability(Capability::GEOMETRY);
            }
            if host.supports_fragment_shader_interlock {
                m.add_capability(Capability::FRAGMENT_SHADER_PIXEL_INTERLOCK_EXT);
                m.add_extension("SPV_EXT_fragment_shader_interlock");
            }
        }
        ShaderStage::Geometry => {
            m.add_capability(Capability::GEOMETRY);
            if definitions.gp_passthrough && host.supports_geometry_shader_passthrough {
                m.add_extension("SPV_NV_geometry_shader_passthrough");
                m.add_capability(Capability::GEOMETRY_SHADER_PASSTHROUGH_NV);
            }
        }
        ShaderStage::TessellationControl | ShaderStage::TessellationEvaluation => m.add_capability(Capability::TESSELLATION),
        ShaderStage::Vertex if host.supports_draw_parameters => m.add_capability(Capability::DRAW_PARAMETERS),
        ShaderStage::Vertex => {}
        ShaderStage::Compute => {}
    }
    if !matches!(stage, ShaderStage::Fragment | ShaderStage::Geometry | ShaderStage::Compute)
        && (ios.contains(&io(StorageKind::Output, IoVariable::Layer)) || ios.contains(&io(StorageKind::Output, IoVariable::ViewportIndex)))
    {
        m.add_extension("SPV_EXT_shader_viewport_index_layer");
        m.add_capability(Capability::SHADER_VIEWPORT_INDEX_LAYER_EXT);
    }
    if ios.contains(&io(StorageKind::Output, IoVariable::ViewportMask)) {
        m.add_extension("SPV_NV_viewport_array2");
        m.add_capability(Capability::SHADER_VIEWPORT_MASK_NV);
    }
    adjust_io_definitions(info, stage);

    let info: &StructuredProgramInfo = info;
    let mut c = CodeGenContext {
        m,
        info,
        definitions,
        properties,
        host,
        constant_buffers: HashMap::new(),
        storage_buffers: HashMap::new(),
        local_memories: HashMap::new(),
        shared_memories: HashMap::new(),
        samplers_types: HashMap::new(),
        samplers: HashMap::new(),
        images: HashMap::new(),
        inputs: NetMap::new(),
        outputs: NetMap::new(),
        inputs_per_patch: NetMap::new(),
        outputs_per_patch: NetMap::new(),
        current_function: 0,
        locals: HashMap::new(),
        func_args: HashMap::new(),
        functions: HashMap::new(),
        labels: HashMap::new(),
        loop_targets: HashMap::new(),
        current_block: info.functions.first().map_or(AstId(0), |f| f.main_block),
        is_main_function: false,
        may_have_returned: false,
        was_non_uniform_access_declared: false,
    };

    declarations::declare_all(&mut c)?;

    for (func_index, function) in info.functions.iter().enumerate() {
        let ret_type = c.get_type(function.return_type, 1)?;
        let mut func_args = Vec::new();
        for arg_index in 0..function.in_arguments.len() + function.out_arguments.len() {
            let arg_type = c.get_type(function.argument_type(arg_index), 1)?;
            func_args.push(c.m.type_pointer(StorageClass::FUNCTION, &arg_type, false));
        }
        let func_type = c.m.type_function(&ret_type, false, &func_args);
        let spv_func = c.m.function(&ret_type, FunctionControlMask::MASK_NONE, &func_type);
        c.functions.insert(func_index as i32, (func_index, spv_func));
    }
    for func_index in 0..info.functions.len() {
        generate_function(&mut c, func_index)?;
    }
    Ok(c.m.generate())
}

fn generate_function(c: &mut CodeGenContext, func_index: usize) -> R<()> {
    let (_, spv_func) = c.function(func_index as i32)?;
    let info = c.info;
    let function = &info.functions[func_index];
    c.current_function = func_index;
    c.m.add_function(&spv_func);
    c.start_function(func_index == 0);
    declarations::declare_parameters(c, func_index)?;
    let main_block = function.main_block;
    c.enter_block(main_block);
    declarations::declare_locals(c, func_index)?;
    generate_block(c, main_block)?;
    // Functions always end with a return.
    let last = c.info.ast.last(main_block);
    let ends = last.and_then(|l| c.info.ast.as_operation(l)).is_some_and(|o| {
        o.inst == crate::ir::Instruction::RETURN || o.inst == crate::ir::Instruction::DISCARD
    });
    if !ends {
        c.m.return_();
    }
    c.m.function_end();

    if func_index == 0 {
        let interface = c.main_interface();
        c.m.add_entry_point(execution_model(c.definitions.stage), &spv_func, "main", &interface);
        let d = c.definitions;
        match d.stage {
            ShaderStage::TessellationControl => {
                c.m.add_execution_mode(&spv_func, ExecutionMode::OUTPUT_VERTICES, &[Lit::i32(d.threads_per_input_primitive).into()]);
            }
            ShaderStage::TessellationEvaluation => {
                let mode = match d.tess_patch_type() {
                    TessPatchType::Isolines => ExecutionMode::ISOLINES,
                    TessPatchType::Triangles => ExecutionMode::TRIANGLES,
                    TessPatchType::Quads => ExecutionMode::QUADS,
                };
                c.m.add_execution_mode(&spv_func, mode, &[]);
                let spacing = match d.tess_spacing() {
                    TessSpacing::EqualSpacing => ExecutionMode::SPACING_EQUAL,
                    TessSpacing::FractionalEventSpacing => ExecutionMode::SPACING_FRACTIONAL_EVEN,
                    TessSpacing::FractionalOddSpacing => ExecutionMode::SPACING_FRACTIONAL_ODD,
                };
                c.m.add_execution_mode(&spv_func, spacing, &[]);
                // The front face is inverted on Vulkan, so the vertex order is too.
                let tess_cw = !d.tess_cw();
                c.m.add_execution_mode(&spv_func, if tess_cw { ExecutionMode::VERTEX_ORDER_CW } else { ExecutionMode::VERTEX_ORDER_CCW }, &[]);
            }
            ShaderStage::Geometry => {
                let input = match d.input_topology() {
                    InputTopology::Points => ExecutionMode::INPUT_POINTS,
                    InputTopology::Lines => ExecutionMode::INPUT_LINES,
                    InputTopology::LinesAdjacency => ExecutionMode::INPUT_LINES_ADJACENCY,
                    InputTopology::Triangles => ExecutionMode::TRIANGLES,
                    InputTopology::TrianglesAdjacency => ExecutionMode::INPUT_TRIANGLES_ADJACENCY,
                };
                c.m.add_execution_mode(&spv_func, input, &[]);
                c.m.add_execution_mode(&spv_func, ExecutionMode::INVOCATIONS, &[Lit::i32(d.threads_per_input_primitive).into()]);
                let output = match d.output_topology {
                    OutputTopology::PointList => ExecutionMode::OUTPUT_POINTS,
                    OutputTopology::LineStrip => ExecutionMode::OUTPUT_LINE_STRIP,
                    OutputTopology::TriangleStrip => ExecutionMode::OUTPUT_TRIANGLE_STRIP,
                };
                c.m.add_execution_mode(&spv_func, output, &[]);
                c.m.add_execution_mode(&spv_func, ExecutionMode::OUTPUT_VERTICES, &[Lit::i32(d.max_output_vertices).into()]);
            }
            ShaderStage::Fragment => {
                c.m.add_execution_mode(&spv_func, if d.origin_upper_left() { ExecutionMode::ORIGIN_UPPER_LEFT } else { ExecutionMode::ORIGIN_LOWER_LEFT }, &[]);
                if c.info.io_definitions.contains(&io(StorageKind::Output, IoVariable::FragmentOutputDepth)) {
                    c.m.add_execution_mode(&spv_func, ExecutionMode::DEPTH_REPLACING, &[]);
                    // The depth a discarding stage writes for the host (Host::discard_writes_depth) is its own: a true
                    // promise that keeps early rejection for LESS tests.
                    if !d.omap_depth {
                        c.m.add_execution_mode(&spv_func, ExecutionMode::DEPTH_GREATER, &[]);
                    }
                }
                if d.early_z_force() {
                    c.m.add_execution_mode(&spv_func, ExecutionMode::EARLY_FRAGMENT_TESTS, &[]);
                }
                if c.info.helper_functions_mask.0 & crate::structured::HelperFunctionsMask::FSI != 0 && c.host.supports_fragment_shader_interlock {
                    c.m.add_execution_mode(&spv_func, ExecutionMode::PIXEL_INTERLOCK_ORDERED_EXT, &[]);
                }
            }
            ShaderStage::Compute => {
                let [x, y, z] = d.compute_local_size;
                c.m.add_execution_mode(&spv_func, ExecutionMode::LOCAL_SIZE, &[Lit::i32(x).into(), Lit::i32(y).into(), Lit::i32(z).into()]);
            }
            ShaderStage::Vertex => {}
        }
    }
    Ok(())
}

/// SpirvGenerator.Generate(context, block): the block's nodes, with structured control flow at block boundaries.
fn generate_block(c: &mut CodeGenContext, block: AstId) -> R<()> {
    let info = c.info;
    let ast = &info.ast;
    let mut visitor = AstBlockVisitor::new(block);
    c.loop_targets.clear();
    while let Some(event) = visitor.next_event(ast) {
        match event {
            VisitEvent::Enter(b) => {
                let merge_block = ast.parent(b).ok_or(NRE)?;
                match ast.block_type(b) {
                    AstBlockType::If => {
                        let if_true = b;
                        let if_false = match ast.next(b)?.filter(|&n| ast.as_block(n).is_some_and(|x| x.ty == AstBlockType::Else)) {
                            Some(e) => e,
                            None => merge_block,
                        };
                        let condition = c.get(A::BOOL, ast.condition(b))?;
                        let merge_label = c.next_label(merge_block);
                        c.m.selection_merge(&merge_label, SelectionControlMask::MASK_NONE);
                        let t = c.next_label(if_true);
                        let f = c.next_label(if_false);
                        c.m.branch_conditional(&condition, &t, &f, &[]);
                    }
                    AstBlockType::DoWhile => {
                        let continue_target = c.m.label();
                        let loop_target = c.new_block();
                        c.loop_targets.insert(b, (loop_target, continue_target.clone()));
                        let merge_label = c.next_label(merge_block);
                        c.m.loop_merge(&merge_label, &continue_target, LoopControlMask::MASK_NONE);
                        let first = c.first_label(b);
                        c.m.branch(&first);
                    }
                    _ => {}
                }
                c.enter_block(b);
            }
            VisitEvent::Left(b) => {
                if let Some(parent) = ast.parent(b) {
                    if ast.block_type(b) == AstBlockType::DoWhile {
                        // A loop: back to the header while the condition holds.
                        let (loop_target, continue_target) = c.loop_targets[&b].clone();
                        c.m.branch(&continue_target);
                        c.m.add_label(&continue_target);
                        let condition = c.get(A::BOOL, ast.condition(b))?;
                        let merge_label = c.next_label(parent);
                        c.m.branch_conditional(&condition, &loop_target, &merge_label, &[]);
                    } else {
                        // A branch unless the last instruction already left.
                        let last_is_cf = ast.last(b).and_then(|l| ast.as_operation(l)).is_some_and(|o| {
                            use crate::ir::Instruction as I;
                            matches!(o.inst, I::DISCARD | I::LOOP_BREAK | I::LOOP_CONTINUE | I::RETURN)
                        });
                        if !last_is_cf {
                            let l = c.next_label(parent);
                            c.m.branch(&l);
                        }
                    }
                    let has_else = ast
                        .next(b)?
                        .and_then(|n| ast.as_block(n))
                        .is_some_and(|x| x.ty == AstBlockType::Else || x.ty == AstBlockType::ElseIf);
                    // Back into the parent block.
                    if !has_else {
                        c.enter_block(parent);
                    }
                }
            }
            VisitEvent::Node(n) => match ast.kind(n) {
                AstKind::Assignment { destination, source } => {
                    let (dest, source) = (*destination, *source);
                    let d = ast.opnd(dest);
                    match d.ty {
                        OperandType::LocalVariable => {
                            let v = c.get(d.var_type, source)?;
                            let p = c.local_pointer(dest)?;
                            c.m.store(&p, &v, None);
                        }
                        OperandType::Argument => {
                            let v = c.get(d.var_type, source)?;
                            let p = c.argument_pointer(dest)?;
                            c.m.store(&p, &v, None);
                        }
                        t => return Err(format!("System.NotImplementedException: {t:?}")),
                    }
                }
                AstKind::Operation(_) => {
                    instructions::generate(c, n)?;
                }
                _ => {}
            },
        }
    }
    Ok(())
}
