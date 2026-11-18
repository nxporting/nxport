// Rebuilding an image from the other images over its guest memory, on the GPU (texture.rs, rebuild_on_gpu). Images that
// share memory with different shapes (TotK packs a downsample chain into one range) see each other's texels only
// through the guest layout: a texel of one is whatever bytes the others' writes left at its address. The CPU route reads
// the writers back, swizzles their rows into guest memory and uploads the image again, waiting for the GPU each time.
// This route does the same steps in a command buffer: the guest bytes go into a buffer, the writers' rows are copied
// out and swizzled into it in write order (relayout.comp), and the image's levels are deswizzled from it and copied in.

use ash::vk;

use super::device::Dev;

static SPIRV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/relayout.spv"));

/// relayout.comp's push constants.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Params {
    pub lin_word: u32,
    pub lin_pitch: u32,
    pub row_bytes: u32,
    pub rows: u32,
    pub y0: u32,
    pub g_word: u32,
    pub g_base: i32,
    pub g_len: u32,
    pub pitch: u32,
    pub bh: u32,
    pub to_rows: u32,
    pub x0: u32,
}

pub struct Relayout {
    module: vk::ShaderModule,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
}

impl Relayout {
    /// None without push descriptors (it binds its buffer with them).
    pub unsafe fn new(dev: &Dev) -> Option<Relayout> {
        dev.push_ext.as_ref()?;
        let words: Vec<u32> = SPIRV.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let module = dev.device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None).ok()?;
        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::COMPUTE)];
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
                println!("[vulkan] relayout shader: {e}");
                return None;
            }
        };
        Some(Relayout { module, set_layout, layout, pipeline })
    }

    /// Binds the pipeline and `buffer` (which holds both the rows and the guest bytes).
    pub unsafe fn bind(&self, dev: &Dev, cb: vk::CommandBuffer, buffer: vk::Buffer) {
        let Some(push) = &dev.push_ext else { return };
        let buffers = [vk::DescriptorBufferInfo::default().buffer(buffer).range(vk::WHOLE_SIZE)];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffers)];
        dev.device.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, self.pipeline);
        push.cmd_push_descriptor_set(cb, vk::PipelineBindPoint::COMPUTE, self.layout, 0, &writes);
    }

    /// Records one move (after `bind`).
    pub unsafe fn dispatch(&self, dev: &Dev, cb: vk::CommandBuffer, p: Params) {
        let bytes = std::slice::from_raw_parts(&p as *const Params as *const u8, std::mem::size_of::<Params>());
        dev.device.cmd_push_constants(cb, self.layout, vk::ShaderStageFlags::COMPUTE, 0, bytes);
        dev.device.cmd_dispatch(cb, p.row_bytes.div_ceil(16).div_ceil(64), p.rows, 1);
    }

    pub unsafe fn destroy(&self, dev: &Dev) {
        dev.device.destroy_pipeline(self.pipeline, None);
        dev.device.destroy_pipeline_layout(self.layout, None);
        dev.device.destroy_descriptor_set_layout(self.set_layout, None);
        dev.device.destroy_shader_module(self.module, None);
    }
}
