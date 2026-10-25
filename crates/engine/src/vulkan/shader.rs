// Guest shader stages as Vulkan shader modules: translated by sass2spv to SPIR-V for one pipeline's state (input and
// output types, clip conventions) and this device (sass2spv::accessor::Host), every resource of a stage in one
// descriptor set (vertex 0, fragment 1; compute 0).

use std::collections::HashMap;

use ash::vk;

use super::device::Dev;

/// What feeds a descriptor.
#[derive(Clone, Debug)]
pub enum Feed {
    Support,
    /// Guest constant buffer slot (0 driver, 1 the shader's constant bank, n >= 3 the uniform buffer bound at n - 3).
    Cbuf(i32),
    /// Storage buffer: its descriptor (address, size) is read from guest constant buffer `cb_slot` at word `cb_offset`.
    Sbuf { cb_slot: i32, cb_offset: i32, write: bool },
    /// Texture: handle at word `handle_index` of the bound texture table (cbuf_slot -1) or of constant buffer
    /// `cbuf_slot`; declared view type.
    /// `slot`: its first texture slot when sampled through the sampler table (a separate image).
    Texture { cbuf_slot: i32, handle_index: i32, view: vk::ImageViewType, count: u32, buffer: bool, slot: Option<u32> },
    /// The sampler table (sass2spv properties::sampler_table_plan): the textures' samplers, deduplicated.
    SamplerTable,
    Image { cbuf_slot: i32, handle_index: i32, view: vk::ImageViewType, buffer: bool },
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub binding: u32,
    pub ty: vk::DescriptorType,
    pub count: u32,
    pub feed: Feed,
    /// Constant buffers: bytes the stage reads at literal offsets (-1: it indexes the buffer at run time).
    pub extent: i32,
}

/// A stage as translated, the same in every session: its shader cache name (stage_cache.rs), the key of its translation
/// (stage and translation inputs) and its binding base.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct StageId {
    pub full: u64,
    pub key: u64,
    pub base: u32,
}

/// What translation takes from a stage's code and control section besides the code: the stage kind hint, the constant
/// bank offset, where the code the translator may read ends (code and constant bank), compute dimensions.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Prep {
    pub hint: i32,
    pub cb1: i32,
    pub code_len: usize,
    pub compute: bool,
    pub compute_fields: Option<[i32; 4]>,
}

/// The guest stage code at `data`: up to 64 KiB, within its memory pool.
pub unsafe fn guest_code<'a>(data: u64) -> &'a [u8] {
    let len = match crate::nvn::api::find_memory_pool(crate::nvn::api::translate_virtual(data as usize)) {
        Some((base, size)) => (base + size).saturating_sub(data as usize).min(0x10000),
        None => 0x10000,
    };
    std::slice::from_raw_parts(data as *const u8, len)
}

/// The GLSLC control section at `control`, if there is one.
pub unsafe fn guest_ctrl<'a>(control: u64) -> Option<&'a [u8]> {
    (control >= 0x1_0000_0000 && *(control as *const u32) == 0x9876_1234).then(|| std::slice::from_raw_parts(control as *const u8, 0x780))
}

/// The translation inputs of a stage (`index`: the program stage slot, 1 = fragment).
pub fn prep(code: &[u8], ctrl: Option<&[u8]>, index: usize) -> Prep {
    let word = |b: &[u8], off: usize| b.get(off..off + 4).map_or(0, |w| u32::from_le_bytes(w.try_into().unwrap()));
    let magic = word(code, 0);
    let c = |off: usize| ctrl.map_or(0, |c| word(c, off));
    let ctrl_len = c(0x20);
    let compute = magic == 0x12345679 || (magic != 0x12345678 && ctrl.is_some() && ctrl_len >= 0x74c && c(0x748) > 0);
    let hint = if compute {
        3
    } else if magic == 0x12345678 {
        match (word(code, 0x30) >> 10) & 0xF {
            5 => 1,
            _ => 2,
        }
    } else if index == 1 {
        1
    } else {
        2
    };
    let (cb1, code_len) = if ctrl.is_some() && ctrl_len >= 0x708 {
        let off = c(0x700) as i32;
        if off > 0 && off <= 0x10_0000 { (off, off as usize + (c(0x6fc) as usize).min(0x10000)) } else { (-1, 0) }
    } else {
        (-1, 0)
    };
    let compute_fields = (compute && ctrl.is_some() && ctrl_len >= 0x764).then(|| {
        let mem = (c(0x754).max(c(0x760)).max(c(0x710)) + 15) & !15;
        [c(0x748) as i32, c(0x74c) as i32, c(0x750) as i32, mem as i32]
    });
    Prep { hint, cb1, code_len, compute, compute_fields }
}

#[derive(Clone)]
pub struct Stage {
    pub id: StageId,
    /// Its shader cache name and input locations.
    pub known: std::sync::Arc<super::stage_cache::Known>,
    pub module: vk::ShaderModule,
    /// Its bindings, numbered from the stage's binding base (graphics stages share one set: vertex 0.., fragment
    /// FRAGMENT_BINDING_BASE..).
    pub bindings: std::sync::Arc<[Binding]>,
    /// A set of its bindings alone (compute).
    pub set_layout: vk::DescriptorSetLayout,
    pub stage_flags: vk::ShaderStageFlags,
    /// The binding base it was translated with.
    pub base: u32,
    /// Combined image samplers and buffer descriptors it binds (push descriptor limits).
    pub samplers: u32,
    pub buffers: u32,
    /// Input locations (a vertex stage's attributes).
    pub inputs: Vec<u32>,
    /// User output locations it declares (a vertex stage's varyings; bit N = location N).
    pub outputs: u32,
}

/// Fragment stages' first binding (vertex stages start at 0; sass2spv numbers a stage's bindings below 256).
pub const FRAGMENT_BINDING_BASE: u32 = 256;

fn view_type(ty: &str) -> (vk::ImageViewType, bool) {
    let array = ty.contains("Array");
    let t = if ty.contains("Cube") {
        if array { vk::ImageViewType::CUBE_ARRAY } else { vk::ImageViewType::CUBE }
    } else if ty.contains("3D") {
        vk::ImageViewType::TYPE_3D
    } else if ty.contains("1D") {
        if array { vk::ImageViewType::TYPE_1D_ARRAY } else { vk::ImageViewType::TYPE_1D }
    } else if array {
        vk::ImageViewType::TYPE_2D_ARRAY
    } else {
        vk::ImageViewType::TYPE_2D
    };
    (t, ty.contains("Buffer"))
}

/// Bytes of a stage its shader cache name covers: its header and the `size` bytes of code the translator decoded.
pub fn named_len(code: &[u8], size: i32) -> usize {
    let magic = code.get(..4).map_or(0, |w| u32::from_le_bytes(w.try_into().unwrap()));
    let size = size.max(0) as usize;
    match magic {
        0x12345679 => 0x100 + size,
        0x12345678 => 0x30 + size,
        _ => size,
    }
}

/// The input locations a SPIR-V module declares.
pub fn spirv_inputs(spirv: &[u8]) -> Vec<u32> {
    let words: Vec<u32> = spirv.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let mut locations: HashMap<u32, u32> = HashMap::new();
    let mut vars = Vec::new();
    let mut i = 5;
    while i < words.len() {
        let (n, op) = ((words[i] >> 16) as usize, words[i] & 0xFFFF);
        if n == 0 {
            break;
        }
        if op == 71 && n >= 4 && words[i + 2] == 30 {
            locations.insert(words[i + 1], words[i + 3]);
        }
        if op == 59 && n >= 4 && words[i + 3] == 1 {
            vars.push(words[i + 2]);
        }
        i += n;
    }
    let mut v: Vec<u32> = vars.iter().filter_map(|v| locations.get(v).copied()).collect();
    v.sort();
    v
}

pub fn key_for(host: &sass2spv::accessor::Host, prep: &Prep, full: u64, state: &Option<sass2spv::accessor::GraphicsState>) -> u64 {
    super::exec::hash_of(&(full, prep, format!("{state:?}"), format!("{host:?}")))
}

/// The key of a stage's translation (StageId::key) with these inputs (as get takes them).
#[allow(clippy::too_many_arguments)]
pub fn translation_key(
    dev: &Dev,
    prep: &Prep,
    full: u64,
    state: &Option<sass2spv::accessor::GraphicsState>,
    set: u32,
    binding_base: u32,
    fragment_inputs: Option<u32>,
    discard_writes_depth: bool,
) -> u64 {
    let host = sass2spv::accessor::Host { stage_set: Some(set as i32), binding_base: binding_base as i32, fragment_inputs, discard_writes_depth, ..dev.shader_host.clone() };
    key_for(&host, prep, full, state)
}

/// The stage `code` (`known`: as seen before) translated for the state `state_for` gives: None, the draw's whole state
/// (a stage seen for the first time is translated with it to learn its inputs); Some(inputs), the state for those
/// inputs only (what the translation is keyed by: a vertex stage's other attributes do not change it). From the disk
/// cache when it has the translation.
#[allow(clippy::too_many_arguments)]
pub unsafe fn get(
    dev: &Dev,
    code: &[u8],
    prep: Prep,
    known: Option<std::sync::Arc<super::stage_cache::Known>>,
    state_for: &dyn Fn(Option<&[u32]>) -> Option<sass2spv::accessor::GraphicsState>,
    set: u32,
    binding_base: u32,
    fragment_inputs: Option<u32>,
    discard_writes_depth: bool,
) -> Result<Stage, String> {
    use super::stage_cache as sc;
    let host = sass2spv::accessor::Host { stage_set: Some(set as i32), binding_base: binding_base as i32, fragment_inputs, discard_writes_depth, ..dev.shader_host.clone() };
    let translate = |state: Option<sass2spv::accessor::GraphicsState>| -> Result<sass2spv::ShaderProgram, String> {
        let p = sass2spv::translate_stage_for(code, prep.hint, prep.cb1, prep.compute_fields, &sass2spv::Target::Spirv, state, host.clone())?;
        if p.binary_code.is_empty() {
            return Err("empty SPIR-V".into());
        }
        Ok(p)
    };
    let key_of = |full: u64, state: &Option<sass2spv::accessor::GraphicsState>| key_for(&host, &prep, full, state);
    let mut first = None;
    let known = match known {
        Some(k) => k,
        None => {
            let whole = state_for(None);
            let p = translate(whole.clone())?;
            let k = sc::remember(code, named_len(code, p.size), prep, spirv_inputs(&p.binary_code));
            first = Some((format!("{whole:?}"), p));
            k
        }
    };
    let state = state_for(Some(&known.inputs));
    let key = key_of(known.full, &state);
    let id = StageId { full: known.full, key, base: binding_base };
    if let Some((_, p)) = first.filter(|(whole, _)| *whole == format!("{state:?}")) {
        let m = p.manifest();
        sc::store(key, &p.binary_code, &m);
        return build_stage(dev, &p.binary_code, &m, id, &prep, known);
    }
    if let Some((spirv, m)) = sc::load(key) {
        return build_stage(dev, &spirv, &m, id, &prep, known);
    }
    let p = translate(state)?;
    let m = p.manifest();
    sc::store(key, &p.binary_code, &m);
    if let Some(dir) = &crate::settings::get().debug.dump_spirv {
        let _ = std::fs::create_dir_all(dir);
        let name = format!("s_{:016x}_{binding_base}_{key:016x}", known.full);
        let _ = std::fs::write(dir.join(format!("{name}.spv")), &p.binary_code);
        let _ = std::fs::write(dir.join(format!("{name}.json")), sass2spv::manifest_json(&p));
    }
    build_stage(dev, &p.binary_code, &m, id, &prep, known)
}

/// A stage from its SPIR-V and manifest.
unsafe fn build_stage(
    dev: &Dev,
    spirv: &[u8],
    m: &sass2spv::manifest::ShaderManifest,
    id: StageId,
    prep: &Prep,
    known: std::sync::Arc<super::stage_cache::Known>,
) -> Result<Stage, String> {
    let (compute, hint, binding_base) = (prep.compute, prep.hint, id.base);
    let words: Vec<u32> = spirv.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    // The bindings the module declares (OpDecorate 71, Binding 33), each fed as the manifest says.
    let mut declared = Vec::new();
    // Inputs: OpVariable (59) of storage class Input (1) with a Location decoration (30).
    let mut locations: HashMap<u32, u32> = HashMap::new();
    let mut input_vars = Vec::new();
    let mut output_vars = Vec::new();
    let mut i = 5;
    while i < words.len() {
        let (n, op) = ((words[i] >> 16) as usize, words[i] & 0xFFFF);
        if n == 0 {
            break;
        }
        if op == 71 && n >= 4 && words[i + 2] == 33 {
            declared.push(words[i + 3]);
        }
        if op == 71 && n >= 4 && words[i + 2] == 30 {
            locations.insert(words[i + 1], words[i + 3]);
        }
        if op == 59 && n >= 4 && words[i + 3] == 1 {
            input_vars.push(words[i + 2]);
        }
        if op == 59 && n >= 4 && words[i + 3] == 3 {
            output_vars.push(words[i + 2]);
        }
        i += n;
    }
    let mut inputs: Vec<u32> = input_vars.iter().filter_map(|v| locations.get(v).copied()).collect();
    inputs.sort();
    let outputs = output_vars.iter().filter_map(|v| locations.get(v)).filter(|&&l| l < 32).fold(0u32, |m, &l| m | 1 << l);
    let mut out = Vec::new();
    for b in declared {
        let mb = b.wrapping_sub(binding_base);
        // Uniform buffers are dynamic: the offset is given when the set is bound, so draws whose copies differ only in
        // where they sit share a set.
        let feed = if mb == 0 {
            Some((vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, 1, Feed::Support))
        } else if let Some(c) = m.cbuffers.iter().find(|c| c.binding == mb) {
            Some((vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, 1, Feed::Cbuf(c.slot)))
        } else if let Some(s) = m.sbuffers.iter().find(|s| s.binding == mb) {
            Some((vk::DescriptorType::STORAGE_BUFFER, 1, Feed::Sbuf { cb_slot: s.cb_slot, cb_offset: s.cb_offset, write: s.write }))
        } else if mb == sass2spv::translation::properties::SAMPLER_TABLE_BINDING as u32 && m.sampler_table > 0 {
            Some((vk::DescriptorType::SAMPLER, m.sampler_table, Feed::SamplerTable))
        } else if let Some(t) = m.textures.iter().find(|t| t.binding == mb) {
            let (view, buffer) = view_type(&t.texture_type);
            let count = t.array_length.max(1) as u32;
            let slot = (t.sampler_slot >= 0).then_some(t.sampler_slot as u32);
            let ty = if buffer {
                vk::DescriptorType::UNIFORM_TEXEL_BUFFER
            } else if slot.is_some() {
                vk::DescriptorType::SAMPLED_IMAGE
            } else {
                vk::DescriptorType::COMBINED_IMAGE_SAMPLER
            };
            Some((ty, count, Feed::Texture { cbuf_slot: t.cbuf_slot, handle_index: t.handle_index, view, count, buffer, slot }))
        } else {
            m.images.iter().find(|t| t.binding == mb).map(|t| {
                let (view, buffer) = view_type(&t.texture_type);
                let ty = if buffer { vk::DescriptorType::STORAGE_TEXEL_BUFFER } else { vk::DescriptorType::STORAGE_IMAGE };
                (ty, 1, Feed::Image { cbuf_slot: t.cbuf_slot, handle_index: t.handle_index, view, buffer })
            })
        };
        match feed {
            Some((ty, count, feed)) => {
                let extent = m.cbuffers.iter().find(|c| c.binding == mb).map_or(-1, |c| c.extent);
                out.push(Binding { binding: b, ty, count, feed, extent })
            }
            None => return Err(format!("binding {b} is not in the manifest")),
        }
    }
    out.sort_by_key(|b| b.binding);
    let module = dev
        .device
        .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
        .map_err(|e| format!("vkCreateShaderModule: {e}"))?;
    let stage_flags = if compute {
        vk::ShaderStageFlags::COMPUTE
    } else if hint == 1 {
        vk::ShaderStageFlags::FRAGMENT
    } else {
        vk::ShaderStageFlags::VERTEX
    };
    let lb: Vec<vk::DescriptorSetLayoutBinding> = out
        .iter()
        .map(|b| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(b.binding)
                .descriptor_type(b.ty)
                .descriptor_count(b.count)
                .stage_flags(stage_flags)
        })
        .collect();
    let set_layout = dev
        .device
        .create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&lb), None)
        .map_err(|e| e.to_string())?;
    let samplers = out
        .iter()
        .filter(|b| matches!(b.ty, vk::DescriptorType::COMBINED_IMAGE_SAMPLER | vk::DescriptorType::SAMPLER))
        .map(|b| b.count)
        .sum();
    let buffers = out
        .iter()
        .filter(|b| {
            matches!(b.ty, vk::DescriptorType::UNIFORM_BUFFER | vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC | vk::DescriptorType::STORAGE_BUFFER)
        })
        .map(|b| b.count)
        .sum();
    Ok(Stage {
        id,
        known,
        module,
        bindings: out.into(),
        set_layout,
        stage_flags,
        base: binding_base,
        samplers,
        buffers,
        inputs,
        outputs,
    })
}
