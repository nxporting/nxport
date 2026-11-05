// The performance overlay (host/overlay.rs) on Vulkan: Dear ImGui's draw data drawn over the swapchain image, in the
// command buffer that blits the frame to it. Nothing here runs until the overlay is turned on.

use std::cell::RefCell;
use std::collections::HashMap;

use ash::vk;
use dear_imgui_rs as imgui;
use imgui::render::{DrawCmd, DrawData, PendingFrame, SnapshotTextureId, SynchronousRendererConsumer, TextureOp};
use imgui::{BackendFlags, TextureFormat, TextureId};

use super::device::{Dev, HostBuffer};

static VERTEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/overlay_vert.spv"));
static FRAGMENT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/overlay_frag.spv"));

const COLOR_FORMAT: vk::Format = vk::Format::B8G8R8A8_UNORM;
/// Frames before a staging buffer's upload has certainly run: more than the window's slots.
const STAGING_FRAMES: usize = 8;

/// As overlay.vert takes them; positions already in clip space, so the shaders need no constants.
#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: u32,
}

/// What a slot's frame still uses: replaced when that slot comes round again, its fence then being waited on.
#[derive(Default)]
struct SlotData {
    vertices: Option<HostBuffer>,
    indices: Option<HostBuffer>,
    view: vk::ImageView,
}

struct Gpu {
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    /// Dear ImGui's textures by its snapshot id, with the id draw commands name them by.
    textures: HashMap<SnapshotTextureId, (TextureId, vk::Image)>,
    sets: HashMap<u64, vk::DescriptorSet>,
    next_id: u64,
    slots: Vec<SlotData>,
    staging: Vec<(usize, HostBuffer)>,
}

struct Overlay {
    ctx: imgui::Context,
    consumer: SynchronousRendererConsumer,
    gpu: Gpu,
}

thread_local! {
    static STATE: RefCell<Option<Overlay>> = const { RefCell::new(None) };
    /// Making it failed once: not tried at every present.
    static FAILED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn words(spirv: &[u8]) -> Vec<u32> {
    spirv.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect()
}

impl Gpu {
    unsafe fn new(dev: &Dev, slots: usize) -> Result<Gpu, String> {
        let d = &dev.device;
        let err = |e: vk::Result| e.to_string();
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        ];
        let set_layout = d
            .create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings), None)
            .map_err(err)?;
        let set_layouts = [set_layout];
        let layout = d
            .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts), None)
            .map_err(err)?;
        let sizes = [
            vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: 8 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: 8 },
        ];
        let pool = d
            .create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(8).pool_sizes(&sizes), None)
            .map_err(err)?;
        let sampler = d
            .create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                None,
            )
            .map_err(err)?;

        let (vert_code, frag_code) = (words(VERTEX), words(FRAGMENT));
        let vert = d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&vert_code), None).map_err(err)?;
        let frag = d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&frag_code), None).map_err(err)?;
        let stages = [
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(vert).name(c"main"),
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(frag).name(c"main"),
        ];
        let vb = [vk::VertexInputBindingDescription::default().stride(std::mem::size_of::<Vertex>() as u32)];
        let attrs = [
            vk::VertexInputAttributeDescription::default().location(0).format(vk::Format::R32G32_SFLOAT).offset(0),
            vk::VertexInputAttributeDescription::default().location(1).format(vk::Format::R32G32_SFLOAT).offset(8),
            vk::VertexInputAttributeDescription::default().location(2).format(vk::Format::R8G8B8A8_UNORM).offset(16),
        ];
        let vertex_input =
            vk::PipelineVertexInputStateCreateInfo::default().vertex_binding_descriptions(&vb).vertex_attribute_descriptions(&attrs);
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.0);
        let multisample =
            vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let blend = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend_state = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let formats = [COLOR_FORMAT];
        let mut rendering = vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&blend_state)
            .dynamic_state(&dynamic)
            .layout(layout)
            .push_next(&mut rendering);
        let made = d.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None);
        d.destroy_shader_module(vert, None);
        d.destroy_shader_module(frag, None);
        let pipeline = made.map_err(|(_, e)| e.to_string())?[0];
        Ok(Gpu {
            pipeline,
            layout,
            set_layout,
            pool,
            sampler,
            textures: HashMap::new(),
            sets: HashMap::new(),
            next_id: 1,
            slots: (0..slots).map(|_| SlotData::default()).collect(),
            staging: Vec::new(),
        })
    }

    /// Records a copy of `w` x `h` pixels into `image` at (x, y), which leaves it ready to sample. `old` is its layout.
    #[allow(clippy::too_many_arguments)]
    unsafe fn upload(
        &mut self,
        dev: &Dev,
        cb: vk::CommandBuffer,
        image: vk::Image,
        old: vk::ImageLayout,
        format: TextureFormat,
        (x, y, w, h): (usize, usize, usize, usize),
        row_pitch: usize,
        data: &[u8],
    ) -> Result<(), String> {
        if w == 0 || h == 0 {
            return Ok(());
        }
        let rgba: Vec<u8> = match format {
            TextureFormat::RGBA32 => (0..h).flat_map(|r| data[r * row_pitch..r * row_pitch + w * 4].iter().copied()).collect(),
            TextureFormat::Alpha8 => (0..h)
                .flat_map(|r| data[r * row_pitch..r * row_pitch + w].iter().flat_map(|&a| [255, 255, 255, a]))
                .collect(),
        };
        let staging = dev.host_buffer(rgba.len() as u64, vk::BufferUsageFlags::TRANSFER_SRC)?;
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), staging.ptr, rgba.len());
        let d = &dev.device;
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let to_copy = [vk::ImageMemoryBarrier::default()
            .image(image)
            .old_layout(old)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .subresource_range(range)];
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &to_copy);
        let region = vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1))
            .image_offset(vk::Offset3D { x: x as i32, y: y as i32, z: 0 })
            .image_extent(vk::Extent3D { width: w as u32, height: h as u32, depth: 1 });
        d.cmd_copy_buffer_to_image(cb, staging.buffer, image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region]);
        let to_read = [vk::ImageMemoryBarrier::default()
            .image(image)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .subresource_range(range)];
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &to_read);
        self.staging.push((STAGING_FRAMES, staging));
        Ok(())
    }

    unsafe fn create_texture(&mut self, dev: &Dev, width: u32, height: u32) -> Result<(TextureId, vk::Image), String> {
        let d = &dev.device;
        let err = |e: vk::Result| e.to_string();
        let image = d
            .create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .extent(vk::Extent3D { width, height, depth: 1 })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST),
                None,
            )
            .map_err(err)?;
        let memory = dev.alloc(d.get_image_memory_requirements(image), vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
        d.bind_image_memory(image, memory, 0).map_err(err)?;
        let view = d
            .create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .subresource_range(vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1)),
                None,
            )
            .map_err(err)?;
        let layouts = [self.set_layout];
        let set = d
            .allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(self.pool).set_layouts(&layouts))
            .map_err(err)?[0];
        let image_info = [vk::DescriptorImageInfo::default().image_view(view).image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let sampler_info = [vk::DescriptorImageInfo::default().sampler(self.sampler)];
        let writes = [
            vk::WriteDescriptorSet::default().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::SAMPLED_IMAGE).image_info(&image_info),
            vk::WriteDescriptorSet::default().dst_set(set).dst_binding(1).descriptor_type(vk::DescriptorType::SAMPLER).image_info(&sampler_info),
        ];
        d.update_descriptor_sets(&writes, &[]);
        let id = TextureId::new(self.next_id);
        self.next_id += 1;
        self.sets.insert(id.id(), set);
        Ok((id, image))
    }

    /// Applies the frame's texture requests in `cb`, before its rendering begins. A destroyed texture's objects stay: the
    /// overlay has one atlas in a run, and one that grows replaces it rarely.
    unsafe fn textures<'a>(&mut self, dev: &Dev, cb: vk::CommandBuffer, frame: PendingFrame<'a>) -> Result<imgui::render::ReconciledFrame<'a>, String> {
        let mut feedback = Vec::with_capacity(frame.texture_requests().len());
        for request in frame.texture_requests() {
            let fb = match request.operation() {
                TextureOp::Create { format, width, height, row_pitch, pixels } => {
                    let (id, image) = self.create_texture(dev, *width, *height)?;
                    let rect = (0, 0, *width as usize, *height as usize);
                    self.upload(dev, cb, image, vk::ImageLayout::UNDEFINED, *format, rect, *row_pitch, pixels)?;
                    self.textures.insert(request.texture(), (id, image));
                    request.uploaded(id).map_err(|e| e.to_string())?
                }
                TextureOp::Update { format, rects, .. } => match self.textures.get(&request.texture()).copied() {
                    Some((id, image)) => {
                        for r in rects {
                            let c = r.rect;
                            let rect = (c.x as usize, c.y as usize, c.w as usize, c.h as usize);
                            self.upload(dev, cb, image, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, *format, rect, r.row_pitch, &r.data)?;
                        }
                        request.uploaded(id).map_err(|e| e.to_string())?
                    }
                    None => request.retry(),
                },
                TextureOp::Destroy => {
                    if let Some((id, _)) = self.textures.remove(&request.texture()) {
                        self.sets.remove(&id.id());
                    }
                    request.destroyed().map_err(|e| e.to_string())?
                }
            };
            feedback.push(fb);
        }
        frame.reconcile_texture_feedback(feedback).map_err(|e| e.to_string())
    }

    /// A slot's buffer with room for `size` bytes; the one it had is not in use, its fence having been waited on.
    unsafe fn room(dev: &Dev, buffer: &mut Option<HostBuffer>, size: u64, usage: vk::BufferUsageFlags) -> Result<*mut u8, String> {
        if buffer.as_ref().is_none_or(|b| b.size < size) {
            if let Some(old) = buffer.take() {
                dev.free_host_buffer(&old);
            }
            *buffer = Some(dev.host_buffer(size.max(64 * 1024).next_power_of_two(), usage)?);
        }
        Ok(buffer.as_ref().unwrap().ptr)
    }

    unsafe fn draw(&mut self, dev: &Dev, cb: vk::CommandBuffer, image: vk::Image, extent: vk::Extent2D, slot: usize, draw: &DrawData) -> Result<(), String> {
        let d = &dev.device;
        let [ox, oy] = draw.display_pos();
        let (w, h) = (extent.width as f32, extent.height as f32);
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u16> = Vec::new();
        for list in draw.draw_lists() {
            vertices.extend(list.vtx_buffer().iter().map(|v| Vertex {
                pos: [(v.pos[0] - ox) / w * 2.0 - 1.0, (v.pos[1] - oy) / h * 2.0 - 1.0],
                uv: v.uv,
                color: v.col,
            }));
            indices.extend_from_slice(list.idx_buffer());
        }
        let data = &mut self.slots[slot];
        if data.view != vk::ImageView::null() {
            d.destroy_image_view(data.view, None);
        }
        data.view = d
            .create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(COLOR_FORMAT)
                    .subresource_range(vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1)),
                None,
            )
            .map_err(|e| e.to_string())?;
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let to_color = [vk::ImageMemoryBarrier::default()
            .image(image)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .subresource_range(range)];
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT, vk::DependencyFlags::empty(), &[], &[], &to_color);
        if vertices.is_empty() || indices.is_empty() {
            return Ok(());
        }
        let vp = Self::room(dev, &mut data.vertices, std::mem::size_of_val(&vertices[..]) as u64, vk::BufferUsageFlags::VERTEX_BUFFER)?;
        std::ptr::copy_nonoverlapping(vertices.as_ptr() as *const u8, vp, std::mem::size_of_val(&vertices[..]));
        let ip = Self::room(dev, &mut data.indices, std::mem::size_of_val(&indices[..]) as u64, vk::BufferUsageFlags::INDEX_BUFFER)?;
        std::ptr::copy_nonoverlapping(indices.as_ptr() as *const u8, ip, std::mem::size_of_val(&indices[..]));
        let attachments = [vk::RenderingAttachmentInfo::default()
            .image_view(data.view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE)];
        let area = vk::Rect2D { offset: vk::Offset2D::default(), extent };
        let info = vk::RenderingInfo::default().render_area(area).layer_count(1).color_attachments(&attachments);
        d.cmd_begin_rendering(cb, &info);
        d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
        d.cmd_set_viewport(cb, 0, &[vk::Viewport { x: 0.0, y: 0.0, width: w, height: h, min_depth: 0.0, max_depth: 1.0 }]);
        d.cmd_bind_vertex_buffers(cb, 0, &[data.vertices.as_ref().unwrap().buffer], &[0]);
        d.cmd_bind_index_buffer(cb, data.indices.as_ref().unwrap().buffer, 0, vk::IndexType::UINT16);
        let (mut base_vertex, mut base_index) = (0usize, 0usize);
        for list in draw.draw_lists() {
            for cmd in list.commands() {
                let DrawCmd::Elements { count, cmd_params: p } = cmd else { continue };
                let Some(&set) = self.sets.get(&p.texture_id.id()) else { continue };
                let [x0, y0, x1, y1] = p.clip_rect;
                let (x0, y0) = ((x0 - ox).max(0.0), (y0 - oy).max(0.0));
                let (x1, y1) = ((x1 - ox).min(w), (y1 - oy).min(h));
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                d.cmd_set_scissor(cb, 0, &[vk::Rect2D {
                    offset: vk::Offset2D { x: x0 as i32, y: y0 as i32 },
                    extent: vk::Extent2D { width: (x1 - x0) as u32, height: (y1 - y0) as u32 },
                }]);
                d.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::GRAPHICS, self.layout, 0, &[set], &[]);
                d.cmd_draw_indexed(cb, count as u32, 1, (base_index + p.idx_offset) as u32, (base_vertex + p.vtx_offset) as i32, 0);
            }
            base_vertex += list.vtx_buffer().len();
            base_index += list.idx_buffer().len();
        }
        d.cmd_end_rendering(cb);
        Ok(())
    }

    unsafe fn age_staging(&mut self, dev: &Dev) {
        self.staging.retain_mut(|(left, buffer)| {
            *left -= 1;
            if *left == 0 {
                dev.free_host_buffer(buffer);
            }
            *left > 0
        });
    }
}

unsafe fn make(dev: &Dev, slots: usize) -> Result<Overlay, String> {
    let mut ctx = imgui::Context::create();
    let _ = ctx.set_ini_filename(None::<String>);
    let source = ["C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\arial.ttf"]
        .iter()
        .find_map(|p| imgui::StbTrueTypeFontData::from_file(p).ok())
        .map(imgui::FontSource::stb_truetype)
        .unwrap_or_else(imgui::FontSource::default_font);
    ctx.font_atlas().add_font(&[source]);
    ctx.style_mut().set_font_size_base(14.0);
    ctx.set_renderer_name(Some("nxport-vulkan-overlay")).map_err(|e| e.to_string())?;
    let mut flags = ctx.io().backend_flags();
    flags.insert(BackendFlags::RENDERER_HAS_TEXTURES);
    flags.insert(BackendFlags::RENDERER_HAS_VTX_OFFSET);
    ctx.io_mut().set_backend_flags(flags);
    let consumer = ctx.create_synchronous_renderer_consumer().map_err(|e| e.to_string())?;
    Ok(Overlay { ctx, consumer, gpu: Gpu::new(dev, slots)? })
}

/// Draws the overlay over `image`, a swapchain image in TRANSFER_DST_OPTIMAL with its frame's picture, if the overlay is
/// on. True: it is now in COLOR_ATTACHMENT_OPTIMAL. `slot` is the window's slot, whose fence has been waited on.
pub(super) unsafe fn record(dev: &Dev, cb: vk::CommandBuffer, image: vk::Image, extent: vk::Extent2D, slot: usize, slots: usize) -> bool {
    if !crate::host::overlay::is_overlay_enabled() || FAILED.with(|f| f.get()) {
        return false;
    }
    STATE.with(|st| {
        let mut st = st.borrow_mut();
        if st.is_none() {
            match make(dev, slots) {
                Ok(o) => *st = Some(o),
                Err(e) => {
                    eprintln!("[!] Performance overlay: {e}");
                    FAILED.with(|f| f.set(true));
                    return false;
                }
            }
        }
        let o = st.as_mut().unwrap();
        o.gpu.age_staging(dev);
        let (stats, times, count) = crate::host::overlay::snapshot();
        let (w, h) = (extent.width as f32, extent.height as f32);
        let io = o.ctx.io_mut();
        io.set_display_size([w, h]);
        io.set_display_framebuffer_scale([1.0, 1.0]);
        io.set_delta_time(stats.frame_time_ms.max(1.0) / 1000.0);
        let frame = o.ctx.begin_frame();
        crate::host::overlay::draw_window(frame.ui(), w, &stats, &times[..count]);
        let pending = frame.render(&o.consumer);
        let result = o.gpu.textures(dev, cb, pending).and_then(|rendered| o.gpu.draw(dev, cb, image, extent, slot, rendered.draw_data()));
        if let Err(e) = result {
            eprintln!("[!] Performance overlay: {e}");
            FAILED.with(|f| f.set(true));
            return false;
        }
        true
    })
}

