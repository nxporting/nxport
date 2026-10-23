// Running recorded NVN command lists on Vulkan.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use ash::vk;
use sass2spv::accessor::GraphicsState;
use sass2spv::types::AttributeType;

use super::device::Dev;
use super::shader::{self, Feed, Stage};
use super::texture::{self, Textures};
use super::{format, state, Arena, Imports};
use crate::gpu::{GpuOnlyTexture, Texels};
use crate::nvn::api::NvnTexture;
use crate::nvn::cmd::{AttribState, Cmd, RtRef};

#[path = "copy.rs"]
mod copy;
#[path = "draw.rs"]
mod draw;

/// NVN state the commands set (it persists across submits, as the queue's does).
#[derive(Default)]
struct Nvn {
    colors: Vec<RtRef>,
    depth: Option<RtRef>,
    viewport: (i32, i32, i32, i32),
    scissor: Option<(i32, i32, i32, i32)>,
    depth_range: (f32, f32),
    blend: [u64; 8],
    color_state: u32,
    channel_mask: Option<u32>,
    depth_stencil: Option<u64>,
    polygon: u32,
    multisample: u64,
    /// Stencil reference, value (read) mask and write mask, [front, back].
    stencil: [[u32; 2]; 3],
    blend_color: [f32; 4],
    program: usize,
    compute_program: usize,
    vertex_buffers: [(u64, usize); 16],
    attribs: AttribState,
    streams: Vec<u64>,
    uniforms: HashMap<(i32, i32), (u64, usize)>,
    storage: HashMap<(i32, i32), (u64, usize)>,
    textures: HashMap<(i32, i32), u64>,
    images: HashMap<(i32, i32), u64>,
    render_disabled: bool,
    prim_restart: Option<u32>,
    depth_clamp: bool,
    polygon_offset: (f32, f32, f32),
}

/// The render pass being recorded.
struct Pass {
    /// The attached images (render-target feedback: a draw sampling one of them).
    images: Vec<vk::Image>,
    colors: Vec<Option<(vk::ImageView, vk::Format, i32)>>,
    depth: Option<(vk::ImageView, vk::Format)>,
    extent: (u32, u32),
    samples: vk::SampleCountFlags,
}

// The context is only used under the backend's lock; its raw pointers point into guest memory and mapped buffers.
unsafe impl Send for Ctx {}

pub struct Ctx {
    dev: Dev,
    pub programs: HashMap<usize, Vec<(u64, u64)>>,
    tex: Textures,
    imports: Imports,
    arena: Arena,
    stages: HashMap<(u64, u64), Stage>,
    failed_stages: HashSet<(u64, u64)>,
    pipelines: HashMap<u64, (vk::Pipeline, vk::PipelineLayout)>,
    layouts: HashMap<(vk::DescriptorSetLayout, vk::DescriptorSetLayout), vk::PipelineLayout>,
    empty_layout: vk::DescriptorSetLayout,
    desc_pools: Vec<vk::DescriptorPool>,
    st: Nvn,
    cb: vk::CommandBuffer,
    pass: Option<Pass>,
    /// Reports to write once the submit completed: (address, value).
    reports: Vec<(u64, u64)>,
    /// The device was lost: nothing more is submitted.
    lost: bool,
    /// Storage buffers bound as copies (the device could not use guest memory in place): (guest address, copy, length),
    /// copied back once the submit completed.
    writebacks: Vec<(usize, *const u8, usize)>,
    /// Guest memory GPU work of this submit writes (copy and clear destinations, storage buffers): [start, end). A uniform
    /// buffer there is bound in place, so it sees those writes in GPU order (a copy taken while recording would not).
    gpu_dirty: Vec<(usize, usize)>,
    /// Render-target feedback: a draw sampling an attachment of its own pass reads a copy of it taken at the first such
    /// draw after the last barrier that invalidates textures (NVN: it sees the writes from before the barrier; sampling
    /// an attachment inside a Vulkan render pass is undefined). Copies by texture description, with their epoch.
    fb_snaps: HashMap<texture::Desc, (u64, texture::Image)>,
    fb_epoch: u64,
    /// GPU texture compression: the game renders compressed blocks through an integer view of a block-compressed texture
    /// (one texel per block). Those passes render into an uncompressed proxy, copied into the texture's level when the
    /// pass ends. Proxies by (texture, level, view format); the ones the current pass renders.
    bc_proxies: HashMap<(texture::Desc, u32, vk::Format), texture::Image>,
    bc_pending: Vec<(texture::Desc, u32, vk::Format)>,
    default_sampler: vk::Sampler,
    warned: HashSet<String>,
}

unsafe fn dev_sampler(dev: &Dev) -> vk::Sampler {
    dev.device.create_sampler(&vk::SamplerCreateInfo::default(), None).unwrap_or(vk::Sampler::null())
}

fn hash<T: Hash>(v: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn attr_type_code(t: AttributeType) -> u8 {
    match t {
        AttributeType::Float => 0,
        AttributeType::Sint => 1,
        AttributeType::Uint => 2,
        AttributeType::Sscaled => 3,
        AttributeType::Uscaled => 4,
    }
}

impl Ctx {
    pub fn new(dev: Dev) -> Result<Ctx, String> {
        let default_sampler = unsafe { dev_sampler(&dev) };
        // Vertex formats the device cannot fetch (their attributes would read garbage).
        let missing: Vec<u32> = (1..=120)
            .filter(|&f| {
                format::vertex(f).is_some_and(|(vf, _)| unsafe {
                    !dev.instance.get_physical_device_format_properties(dev.pdev, vf).buffer_features.contains(vk::FormatFeatureFlags::VERTEX_BUFFER)
                })
            })
            .collect();
        if !missing.is_empty() {
            println!("[vulkan] vertex formats the device cannot fetch (NVN formats): {missing:?}");
        }
        let align = dev
            .limits
            .min_uniform_buffer_offset_alignment
            .max(dev.limits.min_storage_buffer_offset_alignment)
            .max(256);
        let empty_layout = unsafe {
            dev.device
                .create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default(), None)
                .map_err(|e| e.to_string())?
        };
        Ok(Ctx {
            dev,
            programs: HashMap::new(),
            tex: Textures::default(),
            imports: Imports::default(),
            arena: Arena { chunks: Vec::new(), used: 0, align },
            stages: HashMap::new(),
            failed_stages: HashSet::new(),
            pipelines: HashMap::new(),
            layouts: HashMap::new(),
            empty_layout,
            desc_pools: Vec::new(),
            st: Nvn {
                depth_range: (0.0, 1.0),
                stencil: [[0, 0], [0xFF, 0xFF], [0xFF, 0xFF]],
                ..Default::default()
            },
            cb: vk::CommandBuffer::null(),
            pass: None,
            reports: Vec::new(),
            lost: false,
            writebacks: Vec::new(),
            gpu_dirty: Vec::new(),
            fb_snaps: HashMap::new(),
            bc_proxies: HashMap::new(),
            bc_pending: Vec::new(),
            fb_epoch: 1,
            default_sampler,
            warned: HashSet::new(),
        })
    }

    fn warn(&mut self, what: impl Into<String>) {
        let w = what.into();
        if self.warned.insert(w.clone()) {
            println!("[vulkan] {w}");
        }
    }

    pub unsafe fn submit(&mut self, lists: &[u64]) {
        if self.lost {
            return;
        }
        let d = &self.dev.device;
        let Ok(cbs) = d.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default().command_pool(self.dev.cmd_pool).command_buffer_count(1),
        ) else {
            return;
        };
        self.cb = cbs[0];
        let _ = d.begin_command_buffer(self.cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT));
        for &h in lists {
            self.run_list(h, 0);
        }
        self.end_pass();
        let _ = self.dev.device.end_command_buffer(self.cb);
        if let Err(e) = self.dev.submit_wait(self.cb) {
            let frame = super::VULKAN.frames.load(std::sync::atomic::Ordering::Relaxed);
            println!("[vulkan] submit of {} lists at presented frame {frame} failed: {e}; no more GPU work", lists.len());
            self.lost = true;
        }
        self.dev.device.free_command_buffers(self.dev.cmd_pool, &[self.cb]);
        self.cb = vk::CommandBuffer::null();
        self.gpu_dirty.clear();
        for (addr, src, len) in std::mem::take(&mut self.writebacks) {
            std::ptr::copy_nonoverlapping(src, addr as *mut u8, len);
        }
        for (addr, value) in std::mem::take(&mut self.reports) {
            let a = crate::nvn::api::translate_virtual(addr as usize);
            std::ptr::write_unaligned(a as *mut u64, value);
            std::ptr::write_unaligned((a + 8) as *mut u64, 0);
        }
        for p in self.desc_pools.drain(..) {
            self.dev.device.destroy_descriptor_pool(p, None);
        }
        self.arena.reset(&self.dev);
        self.imports.release_retired(&self.dev);
    }

    unsafe fn run_list(&mut self, handle: u64, depth: u32) {
        let Some(list) = crate::nvn::api::command_list(handle, depth > 0) else { return };
        for op in &list.ops {
            match &op.typed {
                Some(Cmd::CallCommands(hs)) if depth < 16 => {
                    for &h in hs.iter() {
                        self.run_list(h, depth + 1);
                    }
                }
                Some(c) => self.exec(c),
                None => {
                    let name = op.name;
                    self.warn(format!("{name} is not implemented"));
                }
            }
        }
    }

    unsafe fn exec(&mut self, c: &Cmd) {
        match c {
            Cmd::SetRenderTargets { colors, depth } => {
                self.end_pass();
                self.st.colors = colors.clone();
                self.st.depth = *depth;
            }
            Cmd::ClearColor { index, rgba, mask } => self.clear_color(*index, *rgba, *mask),
            Cmd::ClearDepthStencil { depth, depth_mask, stencil, stencil_mask } => {
                self.clear_depth(*depth, *depth_mask, *stencil, *stencil_mask)
            }
            Cmd::SetViewport { x, y, w, h } => self.st.viewport = (*x, *y, *w, *h),
            Cmd::SetScissor { x, y, w, h } => self.st.scissor = Some((*x, *y, *w, *h)),
            Cmd::SetDepthRange { near, far } => self.st.depth_range = (*near, *far),
            Cmd::SetStencilValue { kind, faces, value } => {
                for f in 0..2 {
                    if faces & (1 << f) != 0 {
                        self.st.stencil[*kind as usize % 3][f] = *value as u32;
                    }
                }
            }
            Cmd::SetDepthClamp(e) => self.st.depth_clamp = *e,
            Cmd::SetPrimitiveRestart { enable, index } => self.st.prim_restart = enable.then_some(*index),
            Cmd::SetPolygonOffset { factor, units, clamp } => self.st.polygon_offset = (*factor, *units, *clamp),
            Cmd::SetBlendColor(c) => self.st.blend_color = *c,
            Cmd::BindProgram { program, stages } => {
                // Compute and graphics programs are bound independently.
                if stages & 0x20 != 0 {
                    self.st.compute_program = *program;
                }
                if stages & 0x1F != 0 || *stages == 0 {
                    self.st.program = *program;
                }
            }
            Cmd::DispatchCompute { x, y, z } => self.dispatch(*x, *y, *z),
            Cmd::CopyBufferToBuffer { .. }
            | Cmd::ClearBuffer { .. }
            | Cmd::CopyBufferToTexture { .. }
            | Cmd::CopyTextureToBuffer { .. }
            | Cmd::CopyTextureToTexture(_)
            | Cmd::ClearTexture(_) => self.exec_copy(c),
            Cmd::BindVertexBuffer { index, addr, size } => {
                if let Some(v) = self.st.vertex_buffers.get_mut(*index as usize) {
                    *v = (*addr, *size);
                }
            }
            Cmd::BindVertexAttribState(a) => self.st.attribs = *a,
            Cmd::BindVertexStreamState(s) => self.st.streams = s.clone(),
            Cmd::BindBlendState(b) => {
                let target = (b & 0xF) as usize;
                if target < 8 {
                    self.st.blend[target] = *b;
                }
            }
            Cmd::BindColorState(c) => self.st.color_state = *c,
            Cmd::BindChannelMaskState(m) => self.st.channel_mask = Some(*m),
            Cmd::BindDepthStencilState(d) => self.st.depth_stencil = Some(*d),
            Cmd::BindPolygonState(p) => self.st.polygon = *p,
            Cmd::BindMultisampleState(m) => self.st.multisample = *m,
            Cmd::Barrier(bits) => {
                self.end_pass();
                self.dev.full_barrier(self.cb);
                // INVALIDATE_TEXTURE: textures read after it see what was rendered before it.
                if bits & 0x10 != 0 {
                    self.fb_epoch += 1;
                }
            }
            Cmd::BindTexture { stage, index, handle } => {
                self.st.textures.insert((*stage, *index), *handle);
            }
            Cmd::BindImage { stage, index, handle } => {
                self.st.images.insert((*stage, *index), *handle);
            }
            Cmd::BindTexturesDeferred { stage, first, count, handles } => {
                for i in 0..(*count).clamp(0, 32) {
                    let h = if *handles > 0x10000 { std::ptr::read_unaligned((*handles as *const u64).add(i as usize)) } else { 0 };
                    self.st.textures.insert((*stage, *first + i), h);
                }
            }
            Cmd::SetRenderEnable(e) => self.st.render_disabled = !*e,
            Cmd::SetRenderEnableConditional { mode, counters } => {
                let a = crate::nvn::api::translate_virtual(*counters as usize);
                if a > 0x10000 {
                    let (x, y) = (std::ptr::read_unaligned(a as *const u64), std::ptr::read_unaligned((a + 16) as *const u64));
                    self.st.render_disabled = if *mode == 0 { x != y } else { x == y };
                }
            }
            Cmd::BindUniformBuffer { stage, index, addr, size } => {
                self.st.uniforms.insert((*stage, *index), (*addr, *size));
            }
            Cmd::BindStorageBuffer { stage, index, addr, size } => {
                self.st.storage.insert((*stage, *index), (*addr, *size));
            }
            Cmd::UpdateUniformBuffer { addr, offset, data } => {
                // In stream order: draws recorded before it took their copies already.
                let a = crate::nvn::api::translate_virtual((*addr as i64 + *offset as i64) as usize);
                if a > 0x10000 {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), a as *mut u8, data.len());
                }
            }
            Cmd::DrawArrays { mode, first, count, .. } => self.draw(*mode, Draw::Arrays { first: *first, count: *count }, 0, 1),
            Cmd::DrawArraysInstanced { mode, first, count, base_instance, instances, .. } => {
                self.draw(*mode, Draw::Arrays { first: *first, count: *count }, *base_instance, *instances)
            }
            Cmd::DrawElements { mode, index_type, count, index_addr, base_vertex, .. } => self.draw(
                *mode,
                Draw::Elements { index_type: *index_type, count: *count, addr: *index_addr, base_vertex: *base_vertex },
                0,
                1,
            ),
            Cmd::DrawElementsInstanced { mode, index_type, count, index_addr, base_vertex, base_instance, instances, .. } => {
                self.draw(
                    *mode,
                    Draw::Elements { index_type: *index_type, count: *count, addr: *index_addr, base_vertex: *base_vertex },
                    *base_instance,
                    *instances,
                )
            }
            Cmd::ReportValue { value, addr } => self.reports.push((*addr, *value as u64)),
            Cmd::ReportCounter { counter, addr } => {
                // Only the counters the guest's frame logic reads so far: timestamps (TOP reads as 0) and occlusion
                // results (every sample passes until queries are implemented).
                let v = match counter {
                    1 => 1,
                    _ => 0,
                };
                self.reports.push((*addr, v));
            }
            Cmd::SetPointSize(_) | Cmd::ResetCounter { .. } | Cmd::SetDepthBounds { .. } => {}
            other => {
                let name = format!("{other:?}");
                let name = name.split([' ', '(', '{']).next().unwrap_or("").to_string();
                self.warn(format!("command {name} is not implemented"));
            }
        }
    }

    /// The image and NVN texture a render target reference names now.
    unsafe fn target(&mut self, r: &RtRef) -> Option<(NvnTexture, vk::ImageView, vk::Format, (u32, u32), vk::SampleCountFlags, vk::Image)> {
        let addr = crate::nvn::api::resolve_texture_ref(r.texture, r.gen);
        let (_, t) = crate::nvn::api::texture_snapshot(addr)?;
        if r.view_format != 0 {
            if let Some(p) = self.bc_proxy(&t, r) {
                return Some(p);
            }
        }
        let img = self.tex.get(&self.dev, &t)?;
        img.gpu_written = true;
        let fmt = if r.view_format != 0 && format::view_compatible(t.format, r.view_format as i32) {
            texture::device_format(&self.dev, r.view_format as i32)
                .filter(|f| format::aspects(*f) == format::aspects(img.format))
                .unwrap_or(img.format)
        } else {
            img.format
        };
        // A 3D texture renders into one depth slice (the reference's layer) through a 2D view of its 2D_ARRAY_COMPATIBLE
        // image; the slice counts as the view's layer.
        let (layer, ty) = (r.layer as u32, vk::ImageViewType::TYPE_2D);
        let extent = ((img.extent.width >> r.level).max(1), (img.extent.height >> r.level).max(1));
        let samples = img.samples;
        let image = img.image;
        let view = Textures::view(&self.dev, img, ty, fmt, r.level as u32, 1, layer, 1, [2, 3, 4, 5], false)?;
        Some((t, view, fmt, extent, samples, image))
    }

    unsafe fn begin_pass(&mut self) -> bool {
        if self.pass.is_some() {
            return true;
        }
        let mut colors = Vec::new();
        let mut images = Vec::new();
        let mut extent = (u32::MAX, u32::MAX);
        let mut samples = vk::SampleCountFlags::TYPE_1;
        for r in self.st.colors.clone() {
            if r.texture == 0 {
                colors.push(None);
                continue;
            }
            match self.target(&r) {
                Some((t, view, fmt, e, s, image)) => {
                    extent = (extent.0.min(e.0), extent.1.min(e.1));
                    samples = s;
                    images.push(image);
                    colors.push(Some((view, fmt, t.format)));
                }
                None => colors.push(None),
            }
        }
        let depth = match self.st.depth {
            Some(r) if r.texture != 0 => self.target(&r).map(|(_, view, fmt, e, s, image)| {
                extent = (extent.0.min(e.0), extent.1.min(e.1));
                samples = s;
                images.push(image);
                (view, fmt)
            }),
            _ => None,
        };
        if extent.0 == u32::MAX {
            if self.st.colors.is_empty() && self.st.depth.is_none() {
                return false;
            }
            // Only targets that could not be made: the draws go nowhere.
            return false;
        }
        let ca: Vec<vk::RenderingAttachmentInfo> = colors
            .iter()
            .map(|c| match c {
                Some((v, _, _)) => vk::RenderingAttachmentInfo::default()
                    .image_view(*v)
                    .image_layout(vk::ImageLayout::GENERAL)
                    .load_op(vk::AttachmentLoadOp::LOAD)
                    .store_op(vk::AttachmentStoreOp::STORE),
                None => vk::RenderingAttachmentInfo::default(),
            })
            .collect();
        let da = depth.map(|(v, _)| {
            vk::RenderingAttachmentInfo::default()
                .image_view(v)
                .image_layout(vk::ImageLayout::GENERAL)
                .load_op(vk::AttachmentLoadOp::LOAD)
                .store_op(vk::AttachmentStoreOp::STORE)
        });
        let has_stencil = depth.is_some_and(|(_, f)| format::aspects(f).contains(vk::ImageAspectFlags::STENCIL));
        let has_depth = depth.is_some_and(|(_, f)| format::aspects(f).contains(vk::ImageAspectFlags::DEPTH));
        let mut info = vk::RenderingInfo::default()
            .render_area(vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: vk::Extent2D { width: extent.0, height: extent.1 } })
            .layer_count(1)
            .color_attachments(&ca);
        if let Some(d) = da.as_ref() {
            if has_depth {
                info = info.depth_attachment(d);
            }
            if has_stencil {
                info = info.stencil_attachment(d);
            }
        }
        self.dev.full_barrier(self.cb);
        self.dev.device.cmd_begin_rendering(self.cb, &info);
        self.pass = Some(Pass { images, colors, depth, extent, samples });
        true
    }

    unsafe fn end_pass(&mut self) {
        if self.pass.take().is_some() {
            self.dev.device.cmd_end_rendering(self.cb);
            self.dev.full_barrier(self.cb);
            self.resolve_bc_proxies();
        }
    }

    /// The proxy a render target that names a block-compressed texture through a view format of one block per texel
    /// renders into (None for other targets).
    unsafe fn bc_proxy(&mut self, t: &NvnTexture, r: &RtRef) -> Option<(NvnTexture, vk::ImageView, vk::Format, (u32, u32), vk::SampleCountFlags, vk::Image)> {
        let info = crate::nvn::layout::format_info(t.format)?;
        let view = crate::nvn::layout::format_info(r.view_format as i32)?;
        if info.block_w <= 1 || view.block_w != 1 || view.bytes != info.bytes {
            return None;
        }
        let fmt = texture::device_format(&self.dev, r.view_format as i32)?;
        let d = texture::Desc::of(t);
        let level = r.level as u32;
        let key = (d, level, fmt);
        if !self.bc_proxies.contains_key(&key) {
            let mut pd = d;
            pd.storage = 0;
            pd.params.width = ((t.width >> level).max(1) as u32).div_ceil(info.block_w) as i32;
            pd.params.height = ((t.height >> level).max(1) as u32).div_ceil(info.block_h) as i32;
            pd.params.depth = if t.target == crate::nvn::layout::NVN_TARGET_3D { (t.depth >> level).max(1) } else { t.depth };
            pd.params.levels = 1;
            pd.params.format = r.view_format as i32;
            pd.params.flags = 0;
            pd.params.stride = 0;
            let p = self.tex.create(&self.dev, &pd)?;
            println!(
                "[vulkan] rendering compressed format {} ({}x{}, level {level}) through a format {} proxy",
                t.format, t.width, t.height, r.view_format
            );
            self.bc_proxies.insert(key, p);
        }
        if !self.bc_pending.contains(&key) {
            self.bc_pending.push(key);
        }
        if let Some(img) = self.tex.get(&self.dev, t) {
            img.gpu_written = true;
        }
        let p = self.bc_proxies.get_mut(&key).unwrap();
        let (extent, samples, image) = ((p.extent.width, p.extent.height), p.samples, p.image);
        let v = Textures::view(&self.dev, p, vk::ImageViewType::TYPE_2D, fmt, 0, 1, r.layer as u32, 1, [2, 3, 4, 5], false)?;
        Some((*t, v, fmt, extent, samples, image))
    }

    /// Copies the proxies the pass that just ended rendered into their compressed textures: each proxy texel is one
    /// block, so the bytes go through a buffer unchanged.
    unsafe fn resolve_bc_proxies(&mut self) {
        for key in std::mem::take(&mut self.bc_pending) {
            let (d, level, _) = key;
            let Some(info) = crate::nvn::layout::format_info(d.params.format) else { continue };
            let Some(p) = self.bc_proxies.get(&key) else { continue };
            let Some(target) = self.tex.images.get(&d) else { continue };
            let (bw, bh, depth, layers) = (p.extent.width, p.extent.height, p.extent.depth, p.layers);
            let bytes = (bw * bh * depth * layers) as usize * info.bytes as usize;
            let (pimg, timg) = (p.image, target.image);
            let (tw, th) = ((target.extent.width >> level).max(1), (target.extent.height >> level).max(1));
            let td = if target.extent.depth > 1 { (target.extent.depth >> level).max(1) } else { 1 };
            let Some((buf, off, _)) = self.arena.alloc(&self.dev, bytes) else { continue };
            let sub = |level| vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).mip_level(level).layer_count(layers);
            let out = vk::BufferImageCopy::default()
                .buffer_offset(off)
                .image_subresource(sub(0))
                .image_extent(vk::Extent3D { width: bw, height: bh, depth });
            self.dev.device.cmd_copy_image_to_buffer(self.cb, pimg, vk::ImageLayout::GENERAL, buf, &[out]);
            self.dev.full_barrier(self.cb);
            let back = vk::BufferImageCopy::default()
                .buffer_offset(off)
                .buffer_row_length(bw * info.block_w)
                .buffer_image_height(bh * info.block_h)
                .image_subresource(sub(level))
                .image_extent(vk::Extent3D { width: tw, height: th, depth: td.min(depth) });
            self.dev.device.cmd_copy_buffer_to_image(self.cb, buf, timg, vk::ImageLayout::GENERAL, &[back]);
            self.dev.full_barrier(self.cb);
        }
    }

    unsafe fn clear_color(&mut self, index: i32, rgba: [f32; 4], mask: i32) {
        if self.st.render_disabled || !self.begin_pass() {
            return;
        }
        let p = self.pass.as_ref().unwrap();
        let Some(Some((_, _, nvn_fmt))) = p.colors.get(index as usize) else { return };
        let (nvn_fmt, (w, h)) = (*nvn_fmt, p.extent);
        if mask & 0xF != 0xF {
            self.warn(format!("partial color clears (mask {mask:#x}) are cleared whole"));
        }
        let value = match format::kind(nvn_fmt) {
            format::Kind::Float => vk::ClearColorValue { float32: rgba },
            format::Kind::Uint => vk::ClearColorValue { uint32: rgba.map(|v| v as u32) },
            format::Kind::Sint => vk::ClearColorValue { int32: rgba.map(|v| v as i32) },
        };
        let a = [vk::ClearAttachment::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .color_attachment(index as u32)
            .clear_value(vk::ClearValue { color: value })];
        let r = [vk::ClearRect {
            rect: vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: vk::Extent2D { width: w, height: h } },
            base_array_layer: 0,
            layer_count: 1,
        }];
        self.dev.device.cmd_clear_attachments(self.cb, &a, &r);
    }

    unsafe fn clear_depth(&mut self, depth: f32, depth_mask: bool, stencil: i32, stencil_mask: i32) {
        if self.st.render_disabled || !self.begin_pass() {
            return;
        }
        let p = self.pass.as_ref().unwrap();
        let Some((_, fmt)) = p.depth else { return };
        let mut aspect = vk::ImageAspectFlags::empty();
        let has = format::aspects(fmt);
        if depth_mask && has.contains(vk::ImageAspectFlags::DEPTH) {
            aspect |= vk::ImageAspectFlags::DEPTH;
        }
        if stencil_mask != 0 && has.contains(vk::ImageAspectFlags::STENCIL) {
            aspect |= vk::ImageAspectFlags::STENCIL;
        }
        if aspect.is_empty() {
            return;
        }
        let (w, h) = p.extent;
        let a = [vk::ClearAttachment::default().aspect_mask(aspect).clear_value(vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue { depth, stencil: stencil as u32 },
        })];
        let r = [vk::ClearRect {
            rect: vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: vk::Extent2D { width: w, height: h } },
            base_array_layer: 0,
            layer_count: 1,
        }];
        self.dev.device.cmd_clear_attachments(self.cb, &a, &r);
    }

    pub unsafe fn read_texture(&mut self, texture: usize, gen: u32, level: u32, layer: u32) -> Option<Texels> {
        let addr = crate::nvn::api::resolve_texture_ref(texture, gen);
        let (_, t) = crate::nvn::api::texture_snapshot(addr)?;
        let img = self.tex.get(&self.dev, &t)?;
        Textures::read(&self.dev, img, t.format, level, layer)
    }

    pub unsafe fn gpu_only_textures(&mut self, f: &mut dyn FnMut(GpuOnlyTexture)) -> usize {
        let mut skipped = 0;
        for (d, img) in &self.tex.images {
            if !img.gpu_written {
                continue;
            }
            let is3d = img.extent.depth > 1;
            for level in 0..img.levels {
                let n = if is3d { (img.extent.depth >> level).max(1) } else { img.layers };
                for layer in 0..n {
                    match Textures::read(&self.dev, img, d.params.format, level, layer) {
                        Some(texels) => f(GpuOnlyTexture { storage: d.storage, params: d.params, level, layer, texels, last_written: 0 }),
                        None => skipped += 1,
                    }
                }
            }
        }
        skipped
    }
}

#[derive(Clone, Copy)]
enum Draw {
    Arrays { first: i32, count: i32 },
    Elements { index_type: i32, count: i32, addr: u64, base_vertex: i32 },
}

