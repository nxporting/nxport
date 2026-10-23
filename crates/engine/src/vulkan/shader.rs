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
    /// Storage buffer: its descriptor (address, size) is read from guest constant buffer `cb_slot` at `cb_offset`.
    Sbuf { cb_slot: i32, cb_offset: i32 },
    /// Texture: handle at word `handle_index` of the bound texture table (cbuf_slot -1) or of constant buffer
    /// `cbuf_slot`; declared view type.
    Texture { cbuf_slot: i32, handle_index: i32, view: vk::ImageViewType, count: u32, buffer: bool },
    Image { cbuf_slot: i32, handle_index: i32, view: vk::ImageViewType, buffer: bool },
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub binding: u32,
    pub ty: vk::DescriptorType,
    pub count: u32,
    pub feed: Feed,
}

pub struct Stage {
    pub module: vk::ShaderModule,
    pub bindings: Vec<Binding>,
    pub set_layout: vk::DescriptorSetLayout,
    pub stage: sass2spv::ShaderStage,
}

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

/// The guest stage at `data` (code) with `control` (GLSLC control section, may be 0) as SPIR-V for `state`.
pub unsafe fn translate(
    dev: &Dev,
    data: u64,
    control: u64,
    index: usize,
    state: Option<sass2spv::accessor::GraphicsState>,
    set: u32,
) -> Result<Stage, String> {
    let len = match crate::nvn::api::find_memory_pool(crate::nvn::api::translate_virtual(data as usize)) {
        Some((base, size)) => (base + size).saturating_sub(data as usize).min(0x10000),
        None => 0x10000,
    };
    let code = std::slice::from_raw_parts(data as *const u8, len);
    let magic = u32::from_le_bytes(code[..4].try_into().unwrap());
    let ctrl_ok = control >= 0x1_0000_0000 && *(control as *const u32) == 0x9876_1234;
    let ctrl = |off: u64| *((control + off) as *const u32);
    let compute = magic == 0x12345679 || (magic != 0x12345678 && ctrl_ok && ctrl(0x20) >= 0x74c && ctrl(0x748) > 0);
    let hint = if compute {
        3
    } else if magic == 0x12345678 {
        match (u32::from_le_bytes(code[0x30..0x34].try_into().unwrap()) >> 10) & 0xF {
            5 => 1,
            _ => 2,
        }
    } else if index == 1 {
        1
    } else {
        2
    };
    let cb1 = if ctrl_ok && ctrl(0x20) >= 0x708 {
        let off = ctrl(0x700) as i32;
        if off > 0 && off <= 0x10_0000 { off } else { -1 }
    } else {
        -1
    };
    let compute_fields = (compute && ctrl_ok && ctrl(0x20) >= 0x764).then(|| {
        let mem = (ctrl(0x754).max(ctrl(0x760)).max(ctrl(0x710)) + 15) & !15;
        [ctrl(0x748) as i32, ctrl(0x74c) as i32, ctrl(0x750) as i32, mem as i32]
    });
    let host = sass2spv::accessor::Host { stage_set: Some(set as i32), ..dev.shader_host.clone() };
    let p = sass2spv::translate_stage_for(code, hint, cb1, compute_fields, &sass2spv::Target::Spirv, state, host)?;
    if p.binary_code.is_empty() {
        return Err("empty SPIR-V".into());
    }
    let m = p.manifest();
    let words: Vec<u32> = p.binary_code.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    // The bindings the module declares (OpDecorate 71, Binding 33), each fed as the manifest says.
    let mut declared = Vec::new();
    let mut i = 5;
    while i < words.len() {
        let (n, op) = ((words[i] >> 16) as usize, words[i] & 0xFFFF);
        if n == 0 {
            break;
        }
        if op == 71 && n >= 4 && words[i + 2] == 33 {
            declared.push(words[i + 3]);
        }
        i += n;
    }
    let mut out = Vec::new();
    for b in declared {
        let feed = if b == 0 {
            Some((vk::DescriptorType::UNIFORM_BUFFER, 1, Feed::Support))
        } else if let Some(c) = m.cbuffers.iter().find(|c| c.binding == b) {
            Some((vk::DescriptorType::UNIFORM_BUFFER, 1, Feed::Cbuf(c.slot)))
        } else if let Some(s) = m.sbuffers.iter().find(|s| s.binding == b) {
            Some((vk::DescriptorType::STORAGE_BUFFER, 1, Feed::Sbuf { cb_slot: s.cb_slot, cb_offset: s.cb_offset }))
        } else if let Some(t) = m.textures.iter().find(|t| t.binding == b) {
            let (view, buffer) = view_type(&t.texture_type);
            let count = t.array_length.max(1) as u32;
            let ty = if buffer { vk::DescriptorType::UNIFORM_TEXEL_BUFFER } else { vk::DescriptorType::COMBINED_IMAGE_SAMPLER };
            Some((ty, count, Feed::Texture { cbuf_slot: t.cbuf_slot, handle_index: t.handle_index, view, count, buffer }))
        } else {
            m.images.iter().find(|t| t.binding == b).map(|t| {
                let (view, buffer) = view_type(&t.texture_type);
                let ty = if buffer { vk::DescriptorType::STORAGE_TEXEL_BUFFER } else { vk::DescriptorType::STORAGE_IMAGE };
                (ty, 1, Feed::Image { cbuf_slot: t.cbuf_slot, handle_index: t.handle_index, view, buffer })
            })
        };
        match feed {
            Some((ty, count, feed)) => out.push(Binding { binding: b, ty, count, feed }),
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
    Ok(Stage {
        module,
        bindings: out,
        set_layout,
        stage: p.info.stage,
    })
}
