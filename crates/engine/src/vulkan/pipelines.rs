// Graphics pipelines by what selects them (Desc), and those the pipeline list's workers made (listed.rs).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ash::vk;

use super::device::Dev;
use super::shader::{Stage, StageId};
use super::{format, state};

/// Attribute stream for attributes a vertex stage reads but the game leaves unbound.
pub const ZERO_STREAM: u32 = 16;

/// A vertex stream: binding, stride, per-instance, attributes (location, VkFormat, offset).
pub type Stream = (u32, u32, bool, Vec<(u32, i32, u32)>);

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Desc {
    pub vs: StageId,
    pub fs: Option<StageId>,
    pub topology: i32,
    pub restart: bool,
    pub streams: Vec<Stream>,
    pub blend: [u64; 8],
    pub color_state: u32,
    pub channel_mask: Option<u32>,
    pub depth_stencil: Option<u64>,
    pub polygon: u32,
    pub multisample: u64,
    pub depth_clamp: bool,
    pub colors: Vec<i32>,
    pub depth: i32,
    pub samples: u32,
    pub upper_left: bool,
    pub gl_depth: bool,
}

/// The parts of the blend state, blend enables, channel mask and depth-stencil state a pipeline for these targets
/// is made from (state.rs reads no other bits).
pub fn normalize(
    blend: &[u64; 8],
    color_state: u32,
    channel_mask: Option<u32>,
    depth_stencil: Option<u64>,
    colors: &[vk::Format],
    depth: vk::Format,
) -> ([u64; 8], u32, Option<u32>, Option<u64>) {
    let bound = |i: usize| colors.get(i).is_some_and(|&f| f != vk::Format::UNDEFINED);
    let mut b = [0u64; 8];
    let mut enables = 0;
    for i in 0..8 {
        if bound(i) && color_state >> i & 1 != 0 {
            enables |= 1 << i;
            // Zero means the default blend (state::attachment): a nonzero word stays nonzero.
            b[i] = if blend[i] == 0 { 0 } else { blend[i] & 0xF_FFFF_FFFF | 1 << 63 };
        }
    }
    let m = channel_mask.unwrap_or(u32::MAX);
    let mask = Some((0..8).filter(|&i| bound(i)).fold(0, |a, i| a | m & 0xF << (i * 4)));
    let aspects = if depth == vk::Format::UNDEFINED { vk::ImageAspectFlags::empty() } else { format::aspects(depth) };
    let ds = depth_stencil.unwrap_or_else(state::default_depth_stencil);
    let mut used = 0u64;
    if aspects.contains(vk::ImageAspectFlags::DEPTH) {
        used |= 0x3F;
    }
    if aspects.contains(vk::ImageAspectFlags::STENCIL) {
        used |= 0x7F_FFFF_FFC0;
    }
    (b, enables, mask, Some(ds & used))
}

/// A descriptor set layout for both stages and its pipeline layout; whether the set is pushed.
pub type Layout = (vk::DescriptorSetLayout, vk::PipelineLayout, bool);

/// The layout of a graphics stage pair (one set: vertex bindings, then fragment ones).
pub unsafe fn layout(dev: &Dev, vstage: &Stage, fstage: Option<&Stage>) -> Option<Layout> {
    let mut lb: Vec<vk::DescriptorSetLayoutBinding> = Vec::new();
    for st in std::iter::once(vstage).chain(fstage) {
        lb.extend(st.bindings.iter().map(|b| {
            vk::DescriptorSetLayoutBinding::default().binding(b.binding).descriptor_type(b.ty).descriptor_count(b.count).stage_flags(st.stage_flags)
        }));
    }
    let total: u32 = lb.iter().map(|b| b.descriptor_count).sum();
    // Vertex streams (up to 16, and the zero stream) share the vertex stage's buffer slots.
    let push = dev.push.as_ref().is_some_and(|p| {
        total <= p.max
            && vstage.samplers <= p.samplers
            && fstage.is_none_or(|f| f.samplers <= p.samplers && f.buffers <= p.buffers)
            && vstage.buffers + 17 <= p.buffers
    });
    let flags = if push { vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR } else { vk::DescriptorSetLayoutCreateFlags::empty() };
    let dsl = dev.device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().flags(flags).bindings(&lb), None).ok()?;
    let sl = [dsl];
    let layout = dev.device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&sl), None).ok()?;
    Some((dsl, layout, push))
}

/// The pipeline `d` describes.
pub unsafe fn create(
    dev: &Dev,
    d: &Desc,
    vstage: &Stage,
    fstage: Option<&Stage>,
    layout: vk::PipelineLayout,
    cache: vk::PipelineCache,
) -> Result<vk::Pipeline, vk::Result> {
    let entry = c"main";
    let mut stages = vec![vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(vstage.module).name(entry)];
    if let Some(f) = fstage {
        stages.push(vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(f.module).name(entry));
    }
    let mut vb: Vec<vk::VertexInputBindingDescription> = d
        .streams
        .iter()
        .map(|&(stream, stride, per_instance, _)| {
            vk::VertexInputBindingDescription::default()
                .binding(stream)
                .stride(stride)
                .input_rate(if per_instance { vk::VertexInputRate::INSTANCE } else { vk::VertexInputRate::VERTEX })
        })
        .collect();
    let mut va: Vec<vk::VertexInputAttributeDescription> = d
        .streams
        .iter()
        .flat_map(|(stream, _, _, attrs)| {
            attrs.iter().map(move |&(location, format, offset)| {
                vk::VertexInputAttributeDescription::default().location(location).binding(*stream).format(vk::Format::from_raw(format)).offset(offset)
            })
        })
        .collect();
    // Attributes the vertex stage reads but the game leaves unbound read zeros (NVN), from a buffer of zeros.
    let missing: Vec<u32> = vstage.inputs.iter().copied().filter(|l| !va.iter().any(|a| a.location == *l)).collect();
    if !missing.is_empty() {
        vb.push(vk::VertexInputBindingDescription::default().binding(ZERO_STREAM).stride(0).input_rate(vk::VertexInputRate::VERTEX));
        for l in missing {
            va.push(vk::VertexInputAttributeDescription::default().location(l).binding(ZERO_STREAM).format(vk::Format::R32G32B32A32_SFLOAT).offset(0));
        }
    }
    let vi = vk::PipelineVertexInputStateCreateInfo::default().vertex_binding_descriptions(&vb).vertex_attribute_descriptions(&va);
    let ia = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::from_raw(d.topology)).primitive_restart_enable(d.restart);
    let mut dcc = vk::PipelineViewportDepthClipControlCreateInfoEXT::default().negative_one_to_one(true);
    let mut vp = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
    if d.gl_depth && dev.depth_clip_control {
        vp = vp.push_next(&mut dcc);
    }
    let (cull, front, pm, bias) = state::polygon(d.polygon, d.upper_left);
    let rs = vk::PipelineRasterizationStateCreateInfo::default()
        .depth_clamp_enable(d.depth_clamp)
        .polygon_mode(pm)
        .cull_mode(cull)
        .front_face(front)
        .depth_bias_enable(bias)
        .line_width(1.0);
    let samples = vk::SampleCountFlags::from_raw(d.samples);
    let ms = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(samples).alpha_to_coverage_enable(d.multisample >> 1 & 1 != 0);
    let depth_fmt = vk::Format::from_raw(d.depth);
    let has_depth = depth_fmt != vk::Format::UNDEFINED && format::aspects(depth_fmt).contains(vk::ImageAspectFlags::DEPTH);
    let has_stencil = depth_fmt != vk::Format::UNDEFINED && format::aspects(depth_fmt).contains(vk::ImageAspectFlags::STENCIL);
    let ds = state::depth_stencil(d.depth_stencil.unwrap_or_else(state::default_depth_stencil), has_depth, has_stencil);
    let mask = d.channel_mask.unwrap_or(u32::MAX);
    let color_fmts: Vec<vk::Format> = d.colors.iter().map(|&f| vk::Format::from_raw(f)).collect();
    // Targets the fragment stage does not write keep their contents (Apple GPUs store zeros there: write mask 0).
    let written = fstage.map_or(u32::MAX, |f| f.outputs);
    let att: Vec<vk::PipelineColorBlendAttachmentState> = color_fmts
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let a = state::attachment(i, d.blend[i], d.color_state, mask);
            let blends = f != vk::Format::UNDEFINED
                && dev.instance.get_physical_device_format_properties(dev.pdev, f).optimal_tiling_features.contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT_BLEND);
            let a = if written >> i & 1 != 0 { a } else { a.color_write_mask(vk::ColorComponentFlags::empty()) };
            // Formats that cannot blend (integer targets) are written unblended, as the Switch GPU does.
            if blends { a } else { a.blend_enable(false) }
        })
        .collect();
    let cbs = vk::PipelineColorBlendStateCreateInfo::default().attachments(&att);
    let dyns = [
        vk::DynamicState::VIEWPORT,
        vk::DynamicState::SCISSOR,
        vk::DynamicState::STENCIL_REFERENCE,
        vk::DynamicState::STENCIL_COMPARE_MASK,
        vk::DynamicState::STENCIL_WRITE_MASK,
        vk::DynamicState::BLEND_CONSTANTS,
        vk::DynamicState::DEPTH_BIAS,
    ];
    let dy = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyns);
    let mut ri = vk::PipelineRenderingCreateInfo::default()
        .color_attachment_formats(&color_fmts)
        .depth_attachment_format(if has_depth { depth_fmt } else { vk::Format::UNDEFINED })
        .stencil_attachment_format(if has_stencil { depth_fmt } else { vk::Format::UNDEFINED });
    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vi)
        .input_assembly_state(&ia)
        .viewport_state(&vp)
        .rasterization_state(&rs)
        .multisample_state(&ms)
        .depth_stencil_state(&ds)
        .color_blend_state(&cbs)
        .dynamic_state(&dy)
        .layout(layout)
        .push_next(&mut ri);
    dev.device.create_graphics_pipelines(cache, &[info], None).map(|p| p[0]).map_err(|(_, e)| e)
}

/// Pipelines and layouts the workers made, for the context to take on a miss.
#[derive(Default)]
struct Made {
    pipelines: HashMap<u64, (vk::Pipeline, vk::PipelineLayout)>,
    layouts: HashMap<(StageId, Option<StageId>), Layout>,
}

fn made() -> &'static Mutex<Made> {
    static M: OnceLock<Mutex<Made>> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// A pipeline the workers made for `key` (the hash of its Desc).
pub fn take(key: u64) -> Option<(vk::Pipeline, vk::PipelineLayout)> {
    made().lock().unwrap().pipelines.get(&key).copied()
}

/// A layout the workers made for this stage pair.
pub fn made_layout(vs: StageId, fs: Option<StageId>) -> Option<Layout> {
    made().lock().unwrap().layouts.get(&(vs, fs)).copied()
}

/// Adds a pipeline the workers made.
pub fn put(key: u64, p: vk::Pipeline, layout: vk::PipelineLayout) {
    made().lock().unwrap().pipelines.insert(key, (p, layout));
}

/// Adds a layout the workers made for this stage pair (or returns the one another worker added first).
pub fn put_layout(vs: StageId, fs: Option<StageId>, l: Layout) -> Layout {
    *made().lock().unwrap().layouts.entry((vs, fs)).or_insert(l)
}
