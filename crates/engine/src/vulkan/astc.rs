// ASTC textures on devices without ASTC (desktop GPUs; Apple GPUs and MoltenVK have it). Such a texture is an RGBA8
// image, and its uploads decode the guest's blocks with a compute shader (astc.comp) into the image through a UNORM
// storage view. sRGB formats are sampled through an sRGB view of it, so the sampler converts as it would for ASTC.

use ash::vk;

use super::device::Dev;

static SPIRV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/astc.spv"));

/// NVN's ASTC formats: 14 block sizes, then the same in sRGB.
pub fn is_astc(nvn_format: i32) -> bool {
    (121..=148).contains(&nvn_format)
}

/// The format a decoded ASTC texture is sampled in.
pub fn decoded_format(nvn_format: i32) -> vk::Format {
    if nvn_format >= 135 {
        vk::Format::R8G8B8A8_SRGB
    } else {
        vk::Format::R8G8B8A8_UNORM
    }
}

/// astc.comp's push constants.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Params {
    pub offset: u32,
    pub blocks_w: u32,
    pub width: u32,
    pub height: u32,
    pub bw: u32,
    pub bh: u32,
    pub srgb: u32,
}

pub struct Decoder {
    module: vk::ShaderModule,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
}

impl Decoder {
    /// None without push descriptors (the decoder binds its buffer and image with them).
    pub unsafe fn new(dev: &Dev) -> Option<Decoder> {
        dev.push.as_ref()?;
        let words: Vec<u32> = SPIRV.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let module = dev.device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None).ok()?;
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
        ];
        let set_layout = dev
            .device
            .create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default()
                    .flags(vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR)
                    .bindings(&bindings),
                None,
            )
            .ok()?;
        let ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .size(std::mem::size_of::<Params>() as u32)];
        let set_layouts = [set_layout];
        let layout = dev
            .device
            .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts).push_constant_ranges(&ranges), None)
            .ok()?;
        let info = vk::ComputePipelineCreateInfo::default()
            .stage(vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::COMPUTE).module(module).name(c"main"))
            .layout(layout);
        let pipeline = match dev.device.create_compute_pipelines(vk::PipelineCache::null(), &[info], None) {
            Ok(p) => p[0],
            Err((_, e)) => {
                println!("[vulkan] ASTC decoder: {e}");
                return None;
            }
        };
        Some(Decoder { module, set_layout, layout, pipeline })
    }

    /// Records decoding the blocks at `p.offset` words into `src` into `view` (one level and layer, GENERAL layout).
    pub unsafe fn record(&self, dev: &Dev, cb: vk::CommandBuffer, src: vk::Buffer, view: vk::ImageView, p: Params) {
        let Some(push) = &dev.push else { return };
        let buffers = [vk::DescriptorBufferInfo::default().buffer(src).range(vk::WHOLE_SIZE)];
        let images = [vk::DescriptorImageInfo::default().image_view(view).image_layout(vk::ImageLayout::GENERAL)];
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&buffers),
            vk::WriteDescriptorSet::default()
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .image_info(&images),
        ];
        dev.device.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, self.pipeline);
        push.ext.cmd_push_descriptor_set(cb, vk::PipelineBindPoint::COMPUTE, self.layout, 0, &writes);
        let bytes = std::slice::from_raw_parts(&p as *const Params as *const u8, std::mem::size_of::<Params>());
        dev.device.cmd_push_constants(cb, self.layout, vk::ShaderStageFlags::COMPUTE, 0, bytes);
        dev.device.cmd_dispatch(cb, p.width.div_ceil(8), p.height.div_ceil(8), 1);
    }

    pub unsafe fn destroy(&self, dev: &Dev) {
        dev.device.destroy_pipeline(self.pipeline, None);
        dev.device.destroy_pipeline_layout(self.layout, None);
        dev.device.destroy_descriptor_set_layout(self.set_layout, None);
        dev.device.destroy_shader_module(self.module, None);
    }
}
