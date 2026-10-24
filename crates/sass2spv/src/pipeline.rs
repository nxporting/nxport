// From emitted functions to a SPIR-V program (the rest of TranslatorContext.Translate and Generate).

use crate::translation::properties::{BufferDescriptor, TextureDescriptor};
use crate::translation::resource_manager::ResourceManager;
use crate::translation::definitions::FeatureFlags;
use crate::accessor::GpuAccessor;
use crate::codegen::{self, HostCapabilities};
use crate::ir::Function;
use crate::structured;
use crate::translation::cfg::{self, ControlFlowGraph};
use crate::translation::helper_functions::HelperFunctionManager;
use crate::translation::register_usage::{self, FunctionRegisterUsage};
use crate::translation::{optimizations, ssa, transforms, Decoded, FunctionCode, TransformContext};
use crate::types::ShaderStage;

pub struct ShaderProgramInfo {
    pub cbuffers: Vec<BufferDescriptor>,
    pub sbuffers: Vec<BufferDescriptor>,
    pub textures: Vec<TextureDescriptor>,
    pub images: Vec<TextureDescriptor>,
    pub stage: ShaderStage,
    pub uses_instance_id: bool,
    pub uses_draw_parameters: bool,
    /// The sampler table (Host::sampler_slots; properties::sampler_table_plan): its size and the first texture slot of
    /// each texture binding sampled through it.
    pub sampler_table: Option<(u32, Vec<(i32, i32)>)>,
    /// SPIR-V: bytes of each constant buffer binding the stage reads at literal offsets, -1 when it indexes it at run
    /// time (bindings it never reads are absent).
    pub cbuf_extents: Vec<(i32, i32)>,
}

/// Entry point name for callers that name the entry after translating (the name can depend on the translated size).
pub const MSL_ENTRY_PLACEHOLDER: &str = "nx_entry_point";

/// What the translator produces.
pub enum Target {
    Spirv,
    /// Metal Shading Language with this entry point name.
    Msl(String),
}

pub struct ShaderProgram {
    pub info: ShaderProgramInfo,
    /// SPIR-V (Target::Spirv), else empty.
    pub binary_code: Vec<u8>,
    /// MSL (Target::Msl), else empty.
    pub msl: String,
    /// The stage's size in bytes (TranslatorContext.Size): where its code ends.
    pub size: i32,
}

pub fn translate_functions(
    mut d: Decoded,
    code: Vec<FunctionCode>,
    mut rm: ResourceManager,
    mut used_features: FeatureFlags,
    _clip_distances_written: u8,
    target: &Target,
) -> Result<ShaderProgram, String> {
    let ir = &mut d.ir;
    let cfgs: Vec<ControlFlowGraph> = code.iter().map(|f| ControlFlowGraph::create(ir, &f.code)).collect();
    let frus: Vec<FunctionRegisterUsage> = cfgs
        .iter()
        .enumerate()
        .map(|(i, cfg)| if i != 0 { register_usage::run_pass(ir, cfg) } else { FunctionRegisterUsage::default() })
        .collect();
    let stage = d.tc.stage();
    let mut hfm = HelperFunctionManager::new(code.len(), stage);
    for (i, cfg) in cfgs.iter().enumerate() {
        let (ins, outs) = if i != 0 { (frus[i].in_arguments.len() as i32, frus[i].out_arguments.len() as i32) } else { (0, 0) };
        if !cfg.blocks.is_empty() {
            register_usage::fixup_calls(ir, &cfg.blocks, &frus);
            cfg::find_dominators(ir, cfg);
            cfg::find_dominance_frontiers(ir, &cfg.blocks);
            ssa::rename(ir, &cfg.blocks);
            let mut ctx = TransformContext {
                hfm: &mut hfm,
                ir,
                blocks: &cfg.blocks,
                definitions: &d.tc.definitions,
                rm: &mut rm,
                gpu: &d.tc.gpu,
                stage,
                used_features: &mut used_features,
            };
            optimizations::run_pass(&mut ctx)?;
            transforms::run_pass(&mut ctx)?;
        }
        hfm.functions[i] = Some(Function {
            blocks: cfg.blocks.clone(),
            name: format!("fun{i}"),
            returns_value: false,
            in_arguments_count: ins,
            out_arguments_count: outs,
        });
    }
    let functions: Vec<Function> = hfm.functions.into_iter().map(|f| f.expect("function not translated")).collect();
    let mut s_info = structured::program::make_structured_program(&mut d.ir, &functions, &d.tc.definitions, &mut rm, &d.tc.gpu)?;
    let info = ShaderProgramInfo {
        cbuffers: rm.constant_buffer_descriptors(),
        sbuffers: rm.storage_buffer_descriptors(),
        textures: rm.texture_descriptors(),
        images: rm.image_descriptors(),
        stage: d.tc.definitions.stage,
        uses_instance_id: used_features.has(FeatureFlags::INSTANCE_ID),
        uses_draw_parameters: used_features.has(FeatureFlags::DRAW_PARAMETERS),
        sampler_table: None,
        cbuf_extents: Vec::new(),
    };
    let kinds: Vec<(i32, i32, crate::translation::properties::TextureKind)> = info
        .textures
        .iter()
        .map(|t| (t.binding, t.array_length, crate::translation::properties::TextureKind::of(t.ty, t.separate)))
        .collect();
    let mut info = ShaderProgramInfo { sampler_table: crate::translation::properties::sampler_table_plan(&kinds, d.tc.gpu.host.sampler_slots), ..info };
    let gpu = &d.tc.gpu;
    let host = HostCapabilities {
        reduced_precision: gpu.host_reduced_precision(),
        supports_fragment_shader_interlock: gpu.host_supports_fragment_shader_interlock(),
        supports_geometry_shader_passthrough: gpu.host_supports_geometry_shader_passthrough(),
        supports_shader_barrier_divergence: gpu.host_supports_shader_barrier_divergence(),
        supports_shader_float64: gpu.host_supports_shader_float64(),
        supports_shader_non_uniform_indexing: gpu.host_supports_shader_non_uniform_indexing(),
        supports_image_gather_extended: gpu.host.image_gather_extended,
        supports_draw_parameters: gpu.host.draw_parameters,
        cbuf_limits: gpu.host.cbuf_limits,
        binding_base: gpu.host.binding_base,
        fragment_inputs: gpu.host.fragment_inputs,
        sampler_slots: gpu.host.sampler_slots,
    };
    let (binary_code, msl) = match target {
        Target::Spirv => {
            let (code, extents) = codegen::generate(&mut s_info, &d.tc.definitions, &rm.properties, &host)?;
            info.cbuf_extents = extents.into_iter().collect();
            (code, String::new())
        }
        Target::Msl(entry) => (Vec::new(), crate::msl::generate(&mut s_info, &d.tc.definitions, &rm.properties, &host, entry)?),
    };
    Ok(ShaderProgram { info, binary_code, msl, size: d.tc.size })
}

/// The resource manifest the engine reads: which guest constant buffer or texture handle feeds each binding.
pub fn manifest_json(p: &ShaderProgram) -> String {
    let i = &p.info;
    let mut s = format!("{{\"stage\":\"{}\"", i.stage);
    s += &format!(",\"usesInstanceId\":{}", i.uses_instance_id);
    s += &format!(",\"usesDrawParameters\":{}", i.uses_draw_parameters);
    s += &format!(",\"codeEnd\":{}", p.size);
    let cb: Vec<String> = i.cbuffers.iter().map(|b| format!("{{\"binding\":{},\"slot\":{}}}", b.binding, b.slot)).collect();
    s += &format!(",\"cbuffers\":[{}]", cb.join(","));
    let sb: Vec<String> = i
        .sbuffers
        .iter()
        .map(|b| format!("{{\"binding\":{},\"slot\":{},\"cbSlot\":{},\"cbOffset\":{}}}", b.binding, b.slot, b.sb_cb_slot, b.sb_cb_offset))
        .collect();
    s += &format!(",\"sbuffers\":[{}]", sb.join(","));
    let tx: Vec<String> = i
        .textures
        .iter()
        .map(|t| {
            format!(
                "{{\"binding\":{},\"cbufSlot\":{},\"handleIndex\":{},\"type\":\"{}\",\"arrayLength\":{}}}",
                t.binding, t.cbuf_slot, t.handle_index, t.ty, t.array_length
            )
        })
        .collect();
    s += &format!(",\"textures\":[{}]", tx.join(","));
    let im: Vec<String> = i
        .images
        .iter()
        .map(|t| format!("{{\"binding\":{},\"cbufSlot\":{},\"handleIndex\":{},\"type\":\"{}\"}}", t.binding, t.cbuf_slot, t.handle_index, t.ty))
        .collect();
    s += &format!(",\"images\":[{}]}}", im.join(","));
    s
}

/// Translates one NVN shader stage (sass2spv_csharp_translate): `stage_hint` 3 marks a compute stage without a
/// preamble; `cb1_hint` is the constant bank offset from the control section (else found by shape); `compute_fields`
/// the workgroup size and shared memory bytes of a compute stage.
pub fn translate_stage(stage: &[u8], stage_hint: i32, cb1_hint: i32, compute_fields: Option<[i32; 4]>) -> Result<ShaderProgram, String> {
    translate_stage_to(stage, stage_hint, cb1_hint, compute_fields, &Target::Spirv)
}

/// translate_stage for either target.
pub fn translate_stage_to(
    stage: &[u8],
    stage_hint: i32,
    cb1_hint: i32,
    compute_fields: Option<[i32; 4]>,
    target: &Target,
) -> Result<ShaderProgram, String> {
    translate_stage_for(stage, stage_hint, cb1_hint, compute_fields, target, None, crate::accessor::Host::default())
}

/// translate_stage_to for a given draw-time state (inputs, outputs and clip conventions typed for one pipeline) and
/// host (crate::accessor::Host).
pub fn translate_stage_for(
    stage: &[u8],
    stage_hint: i32,
    cb1_hint: i32,
    compute_fields: Option<[i32; 4]>,
    target: &Target,
    state: Option<crate::accessor::GraphicsState>,
    host: crate::accessor::Host,
) -> Result<ShaderProgram, String> {
    let magic = if stage.len() >= 4 { u32::from_le_bytes(stage[..4].try_into().unwrap()) } else { 0 };
    let compute = (stage.len() > 0x100 && magic == 0x12345679) || (magic != 0x12345678 && stage_hint == 3);
    let sass = if compute && magic == 0x12345679 {
        &stage[0x100..]
    } else if stage.len() > 0x30 && magic == 0x12345678 {
        &stage[0x30..]
    } else {
        stage
    };
    let cb1 = if cb1_hint > 0 { cb1_hint } else { GpuAccessor::find_const_bank(stage) };
    let mut gpu = GpuAccessor::new(sass, compute, stage, cb1);
    gpu.state = state;
    gpu.host = host;
    if let Some(f) = compute_fields {
        gpu.local_size = [f[0].max(1), f[1].max(1), f[2].max(1)];
        gpu.shared_memory_size = f[3];
        gpu.local_memory_size = f[3];
    }
    let decoded = crate::translation::create_context(0, gpu, compute)?;
    crate::translation::translate_to(decoded, target)
}
