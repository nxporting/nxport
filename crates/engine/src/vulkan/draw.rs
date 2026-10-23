// Draws: the pipeline for the bound state, the resources each stage's bindings name, vertex and index buffers.

use super::*;

const DRIVER_CBUF_SIZE: usize = 0x1000;
/// The support buffer with the constant buffer bound sizes (sass2spv::accessor::Host::cbuf_limits).
const SUPPORT_SIZE: usize = sass2spv::translation::properties::SUPPORT_CBUF_LIMITS_OFFSET
    + 16 * sass2spv::translation::properties::SUPPORT_CBUF_LIMITS_COUNT as usize;

/// A vertex attribute as the pipeline reads it, and where its data is in the guest's stream.
#[derive(Clone, Copy, Hash)]
pub(super) struct VAttr {
    location: u32,
    format: i32,
    offset: u32,
    src_offset: u32,
    size: u32,
}

/// A vertex stream as the pipeline reads it.
#[derive(Clone, Hash)]
pub(super) struct VStream {
    stream: u32,
    stride: u32,
    src_stride: u32,
    divisor: u32,
    per_instance: bool,
    /// Copied into an aligned layout (`stride`, the attributes' `offset`) instead of bound in place.
    repack: bool,
    attrs: Vec<VAttr>,
}
/// The vertex binding that feeds unbound attributes (above NVN's 16 streams).
const ZERO_STREAM: u32 = 16;

impl Ctx {
    /// The guest program's vertex and fragment stages: (code, control section).
    fn graphics_stages(&self) -> Option<((u64, u64), Option<(u64, u64)>)> {
        let stages = self.programs.get(&self.st.program)?;
        let mut vs = None;
        let mut fs = None;
        for &(data, control) in stages {
            if data < 0x10000 {
                continue;
            }
            let sph = unsafe { std::ptr::read_unaligned((data + 0x30) as *const u32) };
            match (sph >> 10) & 0xF {
                5 => fs = Some((data, control)),
                0 | 1 => vs = Some((data, control)),
                _ => {}
            }
        }
        Some((vs?, fs))
    }

    /// The vertex stage's state: attribute types from the bound attribute formats, NVN's clip conventions.
    fn vertex_state(&self) -> (GraphicsState, u64) {
        let mut s = GraphicsState::engine();
        // Unbound attributes read float zeros (ZERO_STREAM).
        s.attribute_types = [AttributeType::Float; 32];
        let mut codes = [0u8; 32];
        for (i, &bits) in self.st.attribs.as_slice().iter().enumerate().take(32) {
            if let Some((_, t)) = format::vertex((bits >> 5) & 0x1FF) {
                s.attribute_types[i] = t;
                codes[i] = attr_type_code(t);
            }
        }
        s.origin_upper_left = true;
        s.depth_mode = crate::nvn::api::depth_mode() == 0;
        let key = hash(&(codes, s.depth_mode));
        (s, key)
    }

    /// The fragment stage's state: output types from the bound color targets' formats.
    fn fragment_state(&self) -> (GraphicsState, u64) {
        let mut s = GraphicsState::engine();
        let mut codes = [0u8; 8];
        if let Some(p) = &self.pass {
            for (i, c) in p.colors.iter().enumerate().take(8) {
                if let Some((_, _, nvn)) = c {
                    let t = match format::kind(*nvn) {
                        format::Kind::Float => AttributeType::Float,
                        format::Kind::Uint => AttributeType::Uint,
                        format::Kind::Sint => AttributeType::Sint,
                    };
                    s.fragment_output_types[i] = t;
                    codes[i] = attr_type_code(t);
                }
            }
        }
        s.origin_upper_left = true;
        let depth = self.discard_depth();
        (s, hash(&(codes, depth)))
    }

    /// Whether this pass's discarding fragment stages write their depth (Dev::discard_writes_depth, with a depth target).
    fn discard_depth(&self) -> bool {
        self.dev.discard_writes_depth && self.pass.as_ref().is_some_and(|p| p.depth.is_some_and(|(_, f)| format::aspects(f).contains(vk::ImageAspectFlags::DEPTH)))
    }

    unsafe fn stage(&mut self, data: u64, control: u64, index: usize, state: GraphicsState, key: u64, set: u32) -> bool {
        let discard_depth = set == 1 && self.discard_depth();
        let k = (data, key);
        if self.stages.contains_key(&k) {
            return true;
        }
        if self.failed_stages.contains(&k) {
            return false;
        }
        match shader::translate(&self.dev, data, control, index, Some(state), set, discard_depth) {
            Ok(s) => {
                self.stages.insert(k, s);
                true
            }
            Err(e) => {
                self.warn(format!("stage {data:#x}: {e}"));
                self.failed_stages.insert(k);
                false
            }
        }
    }

    pub(super) unsafe fn draw(&mut self, mode: i32, call: Draw, base_instance: i32, instances: i32) {
        if self.st.render_disabled || instances <= 0 || !self.begin_pass() {
            return;
        }
        let Some(topology) = state::topology(mode) else {
            self.warn(format!("draw mode {mode} is not implemented"));
            return;
        };
        let Some(((vd, vc), fs)) = self.graphics_stages() else {
            self.warn(format!("program {:#x} has no vertex stage", self.st.program));
            return;
        };
        let (vstate, vkey) = self.vertex_state();
        if !self.stage(vd, vc, 0, vstate, vkey, 0) {
            return;
        }
        let fkey = match fs {
            Some((fd, fc)) => {
                let (fstate, fkey) = self.fragment_state();
                if !self.stage(fd, fc, 1, fstate, fkey, 1) {
                    return;
                }
                Some((fd, fkey))
            }
            None => None,
        };
        let vl = self.vertex_layout();
        let Some((pipe, layout)) = self.pipeline(topology, (vd, vkey), fkey, &vl) else { return };
        let mut sets = vec![self.descriptors((vd, vkey), vd, vc, 0, &call, base_instance)];
        if let (Some(k), Some((fd, fc))) = (fkey, fs) {
            sets.push(self.descriptors(k, fd, fc, 1, &call, base_instance));
        }
        // A feedback copy ended the pass: it resumes (loading what was rendered).
        if self.pass.is_none() && !self.begin_pass() {
            return;
        }
        let d = &self.dev.device;
        d.cmd_bind_pipeline(self.cb, vk::PipelineBindPoint::GRAPHICS, pipe);
        for (i, s) in sets.iter().enumerate() {
            if let Some(s) = s {
                d.cmd_bind_descriptor_sets(self.cb, vk::PipelineBindPoint::GRAPHICS, layout, i as u32, &[*s], &[]);
            }
        }
        self.bind_vertex_buffers(&vl, &call, base_instance, instances);
        self.dynamic_state();
        match call {
            Draw::Arrays { first, count } => {
                self.dev.device.cmd_draw(self.cb, count.max(0) as u32, instances as u32, first.max(0) as u32, base_instance.max(0) as u32);
            }
            Draw::Elements { index_type, count, addr, base_vertex } => {
                let Some((buf, off, ty)) = self.index_buffer(index_type, count, addr) else { return };
                let d = &self.dev.device;
                d.cmd_bind_index_buffer(self.cb, buf, off, ty);
                d.cmd_draw_indexed(self.cb, count.max(0) as u32, instances as u32, 0, base_vertex, base_instance.max(0) as u32);
            }
        }
    }

    pub(super) unsafe fn dispatch(&mut self, x: i32, y: i32, z: i32) {
        if x <= 0 || y <= 0 || z <= 0 {
            return;
        }
        self.end_pass();
        let Some(&(data, control)) = self.programs.get(&self.st.compute_program).and_then(|s| s.first()) else {
            self.warn(format!("compute program {:#x} has no stage", self.st.compute_program));
            return;
        };
        if !self.stage(data, control, 0, GraphicsState::zeroed(), 0, 0) {
            return;
        }
        let key = hash(&("compute", data));
        let (pipe, layout) = match self.pipelines.get(&key) {
            Some(&p) => p,
            None => {
                let s = &self.stages[&(data, 0)];
                let lk = (s.set_layout, self.empty_layout);
                let layout = match self.layouts.get(&lk) {
                    Some(&l) => l,
                    None => {
                        let sl = [s.set_layout];
                        let Ok(l) = self.dev.device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&sl), None) else { return };
                        self.layouts.insert(lk, l);
                        l
                    }
                };
                let info = vk::ComputePipelineCreateInfo::default()
                    .stage(vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::COMPUTE).module(s.module).name(c"main"))
                    .layout(layout);
                let pipe = match self.dev.device.create_compute_pipelines(vk::PipelineCache::null(), &[info], None) {
                    Ok(p) => p[0],
                    Err((_, e)) => {
                        self.warn(format!("compute pipeline for {data:#x}: {e}"));
                        return;
                    }
                };
                self.pipelines.insert(key, (pipe, layout));
                (pipe, layout)
            }
        };
        let set = self.descriptors((data, 0), data, control, 5, &Draw::Arrays { first: 0, count: 0 }, 0);
        let d = &self.dev.device;
        d.cmd_bind_pipeline(self.cb, vk::PipelineBindPoint::COMPUTE, pipe);
        if let Some(s) = set {
            d.cmd_bind_descriptor_sets(self.cb, vk::PipelineBindPoint::COMPUTE, layout, 0, &[s], &[]);
        }
        d.cmd_dispatch(self.cb, x as u32, y as u32, z as u32);
        self.dev.full_barrier(self.cb);
    }

    unsafe fn pipeline(&mut self, topology: vk::PrimitiveTopology, vs: (u64, u64), fs: Option<(u64, u64)>, vl: &[VStream]) -> Option<(vk::Pipeline, vk::PipelineLayout)> {
        let pass = self.pass.as_ref()?;
        let upper_left = crate::nvn::api::window_origin_mode() == 1;
        let gl_depth = crate::nvn::api::depth_mode() == 0;
        let color_fmts: Vec<vk::Format> = pass.colors.iter().map(|c| c.map_or(vk::Format::UNDEFINED, |c| c.1)).collect();
        let depth_fmt = pass.depth.map_or(vk::Format::UNDEFINED, |d| d.1);
        let restart = self.st.prim_restart.is_some()
            && matches!(topology, vk::PrimitiveTopology::LINE_STRIP | vk::PrimitiveTopology::TRIANGLE_STRIP | vk::PrimitiveTopology::TRIANGLE_FAN);
        let key = hash(&(
            (vs, fs, vl, &self.st.blend),
            (self.st.color_state, self.st.channel_mask, self.st.depth_stencil, self.st.polygon, self.st.multisample),
            (color_fmts.iter().map(|f| f.as_raw()).collect::<Vec<_>>(), depth_fmt.as_raw(), pass.samples.as_raw()),
            (topology.as_raw(), restart, self.st.depth_clamp, upper_left, gl_depth),
        ));
        if let Some(&p) = self.pipelines.get(&key) {
            return Some(p);
        }
        let vstage = self.stages.get(&vs)?;
        let fstage = fs.and_then(|k| self.stages.get(&k));
        let lk = (vstage.set_layout, fstage.map_or(self.empty_layout, |s| s.set_layout));
        let layout = match self.layouts.get(&lk) {
            Some(&l) => l,
            None => {
                let sl = [lk.0, lk.1];
                let l = self.dev.device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&sl), None).ok()?;
                self.layouts.insert(lk, l);
                l
            }
        };
        let entry = c"main";
        let mut stages = vec![vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(vstage.module).name(entry)];
        if let Some(f) = fstage {
            stages.push(vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(f.module).name(entry));
        }
        let vb: Vec<vk::VertexInputBindingDescription> = vl
            .iter()
            .map(|v| {
                vk::VertexInputBindingDescription::default()
                    .binding(v.stream)
                    .stride(v.stride)
                    .input_rate(if v.per_instance { vk::VertexInputRate::INSTANCE } else { vk::VertexInputRate::VERTEX })
            })
            .collect();
        let va: Vec<vk::VertexInputAttributeDescription> = vl
            .iter()
            .flat_map(|v| {
                v.attrs.iter().map(move |a| {
                    vk::VertexInputAttributeDescription::default().location(a.location).binding(v.stream).format(vk::Format::from_raw(a.format)).offset(a.offset)
                })
            })
            .collect();
        // Attributes the vertex stage reads but the game leaves unbound read zeros (NVN), from a buffer of zeros.
        let mut va = va;
        let mut vb = vb;
        let missing: Vec<u32> = vstage.inputs.iter().copied().filter(|l| !va.iter().any(|a| a.location == *l)).collect();
        if !missing.is_empty() {
            let zb = ZERO_STREAM;
            vb.push(vk::VertexInputBindingDescription::default().binding(zb).stride(0).input_rate(vk::VertexInputRate::VERTEX));
            for l in missing {
                va.push(vk::VertexInputAttributeDescription::default().location(l).binding(zb).format(vk::Format::R32G32B32A32_SFLOAT).offset(0));
            }
        }
        let vi = vk::PipelineVertexInputStateCreateInfo::default().vertex_binding_descriptions(&vb).vertex_attribute_descriptions(&va);
        let ia = vk::PipelineInputAssemblyStateCreateInfo::default().topology(topology).primitive_restart_enable(restart);
        let mut dcc = vk::PipelineViewportDepthClipControlCreateInfoEXT::default().negative_one_to_one(true);
        let mut vp = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
        if gl_depth && self.dev.depth_clip_control {
            vp = vp.push_next(&mut dcc);
        }
        let (cull, front, pm, bias) = state::polygon(self.st.polygon, upper_left);
        let rs = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(self.st.depth_clamp)
            .polygon_mode(pm)
            .cull_mode(cull)
            .front_face(front)
            .depth_bias_enable(bias)
            .line_width(1.0);
        let ms = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(pass.samples)
            .alpha_to_coverage_enable(self.st.multisample >> 1 & 1 != 0);
        let has_depth = format::aspects(depth_fmt).contains(vk::ImageAspectFlags::DEPTH) && depth_fmt != vk::Format::UNDEFINED;
        let has_stencil = format::aspects(depth_fmt).contains(vk::ImageAspectFlags::STENCIL) && depth_fmt != vk::Format::UNDEFINED;
        let ds = state::depth_stencil(self.st.depth_stencil.unwrap_or_else(state::default_depth_stencil), has_depth, has_stencil);
        let mask = self.st.channel_mask.unwrap_or(u32::MAX);
        let att: Vec<vk::PipelineColorBlendAttachmentState> =
            (0..color_fmts.len()).map(|i| state::attachment(i, self.st.blend[i], self.st.color_state, mask)).collect();
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
        let pipe = match self.dev.device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) {
            Ok(p) => p[0],
            Err((_, e)) => {
                self.warn(format!("pipeline for {:#x}/{:?}: {e}", vs.0, fs.map(|f| f.0)));
                return None;
            }
        };
        self.pipelines.insert(key, (pipe, layout));
        Some((pipe, layout))
    }

    /// The driver's constant buffer 0 for NVN stage `stage`: draw parameters and the bound storage buffers' descriptors.
    fn driver_cbuf(&self, stage: i32, call: &Draw, base_instance: i32) -> Vec<u8> {
        let mut c = vec![0u8; DRIVER_CBUF_SIZE];
        let base_vertex = match call {
            Draw::Arrays { first, .. } => *first,
            Draw::Elements { base_vertex, .. } => *base_vertex,
        };
        c[0x640..0x644].copy_from_slice(&base_vertex.to_le_bytes());
        c[0x644..0x648].copy_from_slice(&base_instance.to_le_bytes());
        // Blocks in hardware stage order (vertex, tessellation control, tessellation evaluation, geometry, fragment),
        // not NVN's stage numbers (vertex 0, fragment 1, geometry 2, tessellation control 3 and evaluation 4).
        let base = match stage {
            5 => 0x310,
            0 => 0x110,
            1 => 0x510,
            2 => 0x410,
            3 => 0x210,
            _ => 0x310,
        };
        // Compute stages also find their uniform buffers' descriptors here.
        if stage == 5 {
            for (&(s, i), &(addr, size)) in &self.st.uniforms {
                if s == 5 && (0..14).contains(&i) {
                    let o = 0x210 + 0x10 * i as usize;
                    c[o..o + 8].copy_from_slice(&addr.to_le_bytes());
                    c[o + 8..o + 12].copy_from_slice(&(size as u32).to_le_bytes());
                }
            }
        }
        for (&(s, i), &(addr, size)) in &self.st.storage {
            if s == stage && (0..16).contains(&i) {
                let o = base + 0x10 * i as usize;
                c[o..o + 8].copy_from_slice(&addr.to_le_bytes());
                c[o + 8..o + 12].copy_from_slice(&(size as u32).to_le_bytes());
            }
        }
        c
    }

    /// Guest constant buffer `slot` of NVN stage `stage` as the shader sees it (None: nothing bound).
    unsafe fn cbuf_bytes(&self, stage: i32, slot: i32, driver: &[u8], code: (u64, u64)) -> Option<Vec<u8>> {
        match slot {
            0 => Some(driver.to_vec()),
            1 => {
                let (data, control) = code;
                if control < 0x1_0000_0000 || *(control as *const u32) != 0x9876_1234 || *((control + 0x20) as *const u32) < 0x708 {
                    return None;
                }
                let off = *((control + 0x700) as *const u32) as u64;
                let size = (*((control + 0x6fc) as *const u32) as usize).min(0x10000);
                (size > 0).then(|| std::slice::from_raw_parts((data + off) as *const u8, size).to_vec())
            }
            n if n >= 3 => {
                let &(addr, size) = self.st.uniforms.get(&(stage, n - 3))?;
                let a = crate::nvn::api::translate_virtual(addr as usize);
                (a > 0x10000 && size > 0).then(|| std::slice::from_raw_parts(a as *const u8, size.min(0x10000)).to_vec())
            }
            _ => None,
        }
    }

    unsafe fn descriptors(&mut self, key: (u64, u64), data: u64, control: u64, stage: i32, call: &Draw, base_instance: i32) -> Option<vk::DescriptorSet> {
        let (bindings, layout) = {
            let s = self.stages.get(&key)?;
            (s.bindings.clone(), s.set_layout)
        };
        if bindings.is_empty() {
            return None;
        }
        let set = self.alloc_set(layout)?;
        let driver = self.driver_cbuf(stage, call, base_instance);
        let (vw, vh) = (self.st.viewport.2.max(1) as f32, self.st.viewport.3.max(1) as f32);
        let mut buffers: Vec<(u32, vk::DescriptorType, vk::DescriptorBufferInfo)> = Vec::new();
        let mut images: Vec<(u32, vk::DescriptorType, Vec<vk::DescriptorImageInfo>)> = Vec::new();
        let zero = self.arena.copy(&self.dev, &[0u8; 256]);
        // Constant buffers first: the support buffer records their bound sizes.
        let mut limits: Vec<(u32, usize)> = Vec::new();
        let mut ordered: Vec<&shader::Binding> = bindings.iter().filter(|b| !matches!(b.feed, Feed::Support)).collect();
        ordered.extend(bindings.iter().filter(|b| matches!(b.feed, Feed::Support)));
        for b in ordered {
            match &b.feed {
                Feed::Support => {
                    let mut sup = vec![0u8; SUPPORT_SIZE];
                    let put = |s: &mut Vec<u8>, off: usize, v: [f32; 4]| {
                        for (i, x) in v.iter().enumerate() {
                            s[off + i * 4..off + i * 4 + 4].copy_from_slice(&x.to_le_bytes());
                        }
                    };
                    put(&mut sup, 144, [2.0 / vw, 2.0 / vh, 1.0, 0.0]);
                    put(&mut sup, 160, [vw, vh, 1.0, 0.0]);
                    for i in 0..73 {
                        put(&mut sup, 192 + i * 16, [1.0, 0.0, 0.0, 0.0]);
                    }
                    // Bound sizes of the constant buffers (vec4 units), for the shader's checks.
                    for &(binding, len) in &limits {
                        let o = sass2spv::translation::properties::SUPPORT_CBUF_LIMITS_OFFSET + 16 * binding as usize;
                        if o + 4 <= sup.len() {
                            sup[o..o + 4].copy_from_slice(&(len.div_ceil(16) as u32).to_le_bytes());
                        }
                    }
                    if let Some((buf, off)) = self.arena.copy(&self.dev, &sup) {
                        buffers.push((b.binding, b.ty, vk::DescriptorBufferInfo { buffer: buf, offset: off, range: sup.len() as u64 }));
                    }
                }
                Feed::Cbuf(slot) => {
                    // Written by GPU work earlier in this submit: in place.
                    if *slot >= 3 {
                        if let Some(&(addr, size)) = self.st.uniforms.get(&(stage, slot - 3)) {
                            let a = crate::nvn::api::translate_virtual(addr as usize);
                            let len = size.min(0x10000);
                            if a > 0x10000 && self.gpu_dirty.iter().any(|&(s, e)| a < e && s < a + len) {
                                if let Some((buf, off)) = self.imports.find(&self.dev, a, len) {
                                    if off % self.dev.limits.min_uniform_buffer_offset_alignment.max(1) == 0 {
                                        limits.push((b.binding, len));
                                        buffers.push((b.binding, b.ty, vk::DescriptorBufferInfo { buffer: buf, offset: off, range: len as u64 }));
                                        continue;
                                    }
                                }
                                self.warn("a uniform buffer the GPU writes in the same submit could not be bound in place");
                            }
                        }
                    }
                    let bytes = self.cbuf_bytes(stage, *slot, &driver, (data, control));
                    let x = match &bytes {
                        Some(v) => self.arena.copy(&self.dev, v).map(|(b, o)| (b, o, v.len() as u64)),
                        None => zero.map(|(b, o)| (b, o, 256)),
                    };
                    if let Some((buf, off, len)) = x {
                        limits.push((b.binding, len as usize));
                        buffers.push((b.binding, b.ty, vk::DescriptorBufferInfo { buffer: buf, offset: off, range: len }));
                    }
                }
                Feed::Sbuf { cb_slot, cb_offset } => {
                    let desc = self.cbuf_bytes(stage, *cb_slot, &driver, (data, control)).and_then(|c| {
                        // In 32-bit words.
                        let o = *cb_offset as usize * 4;
                        (o + 12 <= c.len()).then(|| {
                            (u64::from_le_bytes(c[o..o + 8].try_into().unwrap()), u32::from_le_bytes(c[o + 8..o + 12].try_into().unwrap()) as usize)
                        })
                    });
                    let info = match desc {
                        Some((addr, size)) if addr > 0x10000 && size > 0 => self.storage_buffer(addr as usize, size),
                        _ => None,
                    };
                    match info.or_else(|| zero.map(|(b, o)| vk::DescriptorBufferInfo { buffer: b, offset: o, range: 256 })) {
                        Some(i) => buffers.push((b.binding, b.ty, i)),
                        None => {}
                    }
                }
                Feed::Texture { cbuf_slot, handle_index, view, count, buffer } => {
                    let mut infos = Vec::new();
                    for j in 0..*count {
                        let handle = if *cbuf_slot < 0 {
                            let k = (handle_index - 8) / 2 + j as i32;
                            self.st.textures.get(&(stage, k)).copied()
                        } else {
                            self.cbuf_bytes(stage, *cbuf_slot, &driver, (data, control)).and_then(|c| {
                                let o = (*handle_index as usize + j as usize) * 4;
                                (o + 4 <= c.len()).then(|| u32::from_le_bytes(c[o..o + 4].try_into().unwrap()) as u64)
                            })
                        };
                        let (v, s) = if *buffer {
                            self.warn("buffer textures are not implemented");
                            (vk::ImageView::null(), self.default_sampler)
                        } else {
                            self.texture_binding(handle, *view)
                        };
                        infos.push(vk::DescriptorImageInfo { sampler: s, image_view: v, image_layout: vk::ImageLayout::GENERAL });
                    }
                    images.push((b.binding, b.ty, infos));
                }
                Feed::Image { cbuf_slot, handle_index, view, buffer } => {
                    let handle = if *cbuf_slot < 0 {
                        let k = (handle_index - 8) / 2;
                        self.st.images.get(&(stage, k)).copied()
                    } else {
                        None
                    };
                    let v = if *buffer { vk::ImageView::null() } else { self.image_binding(handle, *view) };
                    images.push((b.binding, b.ty, vec![vk::DescriptorImageInfo { sampler: vk::Sampler::null(), image_view: v, image_layout: vk::ImageLayout::GENERAL }]));
                }
            }
        }
        let bi: Vec<[vk::DescriptorBufferInfo; 1]> = buffers.iter().map(|b| [b.2]).collect();
        let mut writes: Vec<vk::WriteDescriptorSet> = buffers
            .iter()
            .zip(&bi)
            .map(|((binding, ty, _), info)| vk::WriteDescriptorSet::default().dst_set(set).dst_binding(*binding).descriptor_type(*ty).buffer_info(info))
            .collect();
        for (binding, ty, infos) in &images {
            if *ty == vk::DescriptorType::UNIFORM_TEXEL_BUFFER || *ty == vk::DescriptorType::STORAGE_TEXEL_BUFFER {
                continue;
            }
            writes.push(vk::WriteDescriptorSet::default().dst_set(set).dst_binding(*binding).descriptor_type(*ty).image_info(infos));
        }
        self.dev.device.update_descriptor_sets(&writes, &[]);
        Some(set)
    }

    unsafe fn alloc_set(&mut self, layout: vk::DescriptorSetLayout) -> Option<vk::DescriptorSet> {
        let layouts = [layout];
        if let Some(&p) = self.desc_pools.last() {
            if let Ok(s) = self.dev.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(p).set_layouts(&layouts)) {
                return Some(s[0]);
            }
        }
        let sizes = [
            vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER, descriptor_count: 16384 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: 8192 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: 16384 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: 2048 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_TEXEL_BUFFER, descriptor_count: 1024 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_TEXEL_BUFFER, descriptor_count: 1024 },
        ];
        let p = self.dev.device.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(2048).pool_sizes(&sizes), None).ok()?;
        self.desc_pools.push(p);
        self.dev.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(p).set_layouts(&layouts)).ok().map(|s| s[0])
    }

    /// A storage buffer over guest memory: in place when its pool is imported and the offset aligned, else a copy that is
    /// written back after the submit.
    unsafe fn storage_buffer(&mut self, addr: usize, size: usize) -> Option<vk::DescriptorBufferInfo> {
        let a = crate::nvn::api::translate_virtual(addr);
        self.gpu_dirty.push((a, a + size));
        // GPU writes cannot fault: pages the write tracking protected would lose them.
        crate::guest::dirty_track::release_for_gpu(a, size);
        let align = self.dev.limits.min_storage_buffer_offset_alignment.max(1);
        if let Some((buf, off)) = self.imports.find(&self.dev, a, size) {
            if off % align == 0 {
                return Some(vk::DescriptorBufferInfo { buffer: buf, offset: off, range: size as u64 });
            }
        }
        let (buf, off, p) = self.arena.alloc(&self.dev, size)?;
        std::ptr::copy_nonoverlapping(a as *const u8, p, size);
        self.writebacks.push((a, p, size));
        Some(vk::DescriptorBufferInfo { buffer: buf, offset: off, range: size as u64 })
    }

    unsafe fn texture_binding(&mut self, handle: Option<u64>, declared: vk::ImageViewType) -> (vk::ImageView, vk::Sampler) {
        let Some(h) = handle else { return (vk::ImageView::null(), self.default_sampler) };
        let (tex_id, sampler_id) = ((h & 0xFFFFF) as i32, ((h >> 20) & 0xFFF) as i32);
        let view = crate::nvn::api::lookup_pool_texture(tex_id).and_then(|(obj, view)| {
            let (_, t) = crate::nvn::api::texture_snapshot(obj)?;
            let img = self.tex.get(&self.dev, &t)?.image;
            if self.pass.as_ref().is_some_and(|p| p.images.contains(&img)) {
                return self.feedback_view(&t, view.as_ref(), declared);
            }
            self.tex.sampled_view(&self.dev, &t, view.as_ref(), declared)
        });
        let sampler = crate::nvn::api::lookup_pool_sampler(sampler_id).and_then(|d| self.tex.sampler(&self.dev, &d));
        (view.unwrap_or(vk::ImageView::null()), sampler.unwrap_or(self.default_sampler))
    }

    /// A view of the feedback copy of texture `t` (an attachment of the current pass), taken now if this barrier epoch
    /// has none: the pass ends for the copy and resumes at the next draw.
    unsafe fn feedback_view(&mut self, t: &NvnTexture, view: Option<&crate::nvn::api::NvnTextureView>, declared: vk::ImageViewType) -> Option<vk::ImageView> {
        let d = texture::Desc::of(t);
        let fresh = self.fb_snaps.get(&d).is_some_and(|(e, _)| *e == self.fb_epoch);
        if !fresh {
            if !self.fb_snaps.contains_key(&d) {
                let snap = self.tex.create(&self.dev, &d)?;
                self.fb_snaps.insert(d, (0, snap));
            }
            self.end_pass();
            let src = self.tex.images.get(&d)?;
            let (_, snap) = self.fb_snaps.get(&d)?;
            let aspect = format::aspects(src.format);
            let regions: Vec<vk::ImageCopy> = (0..src.levels)
                .map(|l| {
                    let sub = vk::ImageSubresourceLayers::default().aspect_mask(aspect).mip_level(l).layer_count(src.layers);
                    vk::ImageCopy::default().src_subresource(sub).dst_subresource(sub).extent(vk::Extent3D {
                        width: (src.extent.width >> l).max(1),
                        height: (src.extent.height >> l).max(1),
                        depth: (src.extent.depth >> l).max(1),
                    })
                })
                .collect();
            self.dev.device.cmd_copy_image(self.cb, src.image, vk::ImageLayout::GENERAL, snap.image, vk::ImageLayout::GENERAL, &regions);
            self.dev.full_barrier(self.cb);
            self.fb_snaps.get_mut(&d)?.0 = self.fb_epoch;
        }
        let (_, snap) = self.fb_snaps.get_mut(&d)?;
        Textures::sampled_view_of(&self.dev, snap, t, view, declared)
    }

    unsafe fn image_binding(&mut self, handle: Option<u64>, declared: vk::ImageViewType) -> vk::ImageView {
        let Some(h) = handle else { return vk::ImageView::null() };
        let Some((obj, view)) = crate::nvn::api::lookup_pool_texture((h & 0xFFFFF) as i32) else { return vk::ImageView::null() };
        let Some((_, t)) = crate::nvn::api::texture_snapshot(obj) else { return vk::ImageView::null() };
        let Some(img) = self.tex.get(&self.dev, &t) else { return vk::ImageView::null() };
        img.gpu_written = true;
        let (level, layer) = view.map_or((0, 0), |v| (v.min_level as u32, v.min_layer as u32));
        let fmt = img.format;
        let layers = if declared == vk::ImageViewType::TYPE_2D || declared == vk::ImageViewType::TYPE_3D { 1 } else { img.layers - layer };
        Textures::view(&self.dev, img, declared, fmt, level, 1, layer, layers, [2, 3, 4, 5], false).unwrap_or(vk::ImageView::null())
    }

    /// The bound vertex streams as the pipeline reads them: in place, or repacked where the device needs aligned strides
    /// and offsets (or cannot read past a stride), and expanded where a stream steps per several instances.
    fn vertex_layout(&self) -> Vec<VStream> {
        let mut out: Vec<VStream> = Vec::new();
        for (i, &b) in self.st.attribs.as_slice().iter().enumerate() {
            let Some((f, _)) = format::vertex((b >> 5) & 0x1FF) else { continue };
            let stream = b & 31;
            let bits = self.st.streams.get(stream as usize).copied().unwrap_or(0);
            let v = match out.iter_mut().position(|v| v.stream == stream) {
                Some(k) => &mut out[k],
                None => {
                    out.push(VStream {
                        stream,
                        stride: bits as u32,
                        src_stride: bits as u32,
                        divisor: (bits >> 32) as u32,
                        per_instance: bits >> 32 != 0,
                        repack: false,
                        attrs: Vec::new(),
                    });
                    out.last_mut().unwrap()
                }
            };
            let size = format::vertex_size(f);
            v.attrs.push(VAttr { location: i as u32, format: f.as_raw(), offset: b >> 14, src_offset: b >> 14, size });
        }
        let align = self.dev.vertex_align.max(1);
        for v in out.iter_mut() {
            let misaligned = v.stride % align != 0 || v.attrs.iter().any(|a| a.offset % align != 0);
            let beyond = !self.dev.beyond_stride && v.attrs.iter().any(|a| a.offset + a.size > v.stride);
            if misaligned || beyond || v.divisor > 1 {
                v.repack = true;
                let mut off = 0;
                for a in v.attrs.iter_mut() {
                    a.offset = off;
                    off += a.size.next_multiple_of(align.max(4));
                }
                v.stride = off.max(align);
            }
        }
        out
    }

    unsafe fn bind_vertex_buffers(&mut self, vl: &[VStream], call: &Draw, base_instance: i32, instances: i32) {
        if let Some((buf, off)) = self.arena.copy(&self.dev, &[0u8; 64]) {
            self.dev.device.cmd_bind_vertex_buffers(self.cb, ZERO_STREAM, &[buf], &[off]);
        }
        for v in vl {
            let (addr, size) = self.st.vertex_buffers.get(v.stream as usize).copied().unwrap_or((0, 0));
            let a = crate::nvn::api::translate_virtual(addr as usize);
            let bound = if a <= 0x10000 || size == 0 {
                self.arena.copy(&self.dev, &[0u8; 256])
            } else if !v.repack {
                self.imports.find(&self.dev, a, size).or_else(|| self.arena.copy(&self.dev, std::slice::from_raw_parts(a as *const u8, size)))
            } else {
                // Elements to write: every vertex the draw can fetch, or every instance it draws.
                let n = if v.per_instance {
                    (base_instance.max(0) + instances.max(0)) as usize
                } else {
                    match *call {
                        Draw::Arrays { first, count } => (first.max(0) + count.max(0)) as usize,
                        Draw::Elements { index_type, count, addr, base_vertex } => {
                            (self.max_index(index_type, count, addr) as i64 + base_vertex as i64 + 1).max(0) as usize
                        }
                    }
                };
                let src = std::slice::from_raw_parts(a as *const u8, size);
                let mut data = vec![0u8; n * v.stride as usize];
                for e in 0..n {
                    // A stream stepping per `divisor` instances: element base + i reads source element base + i / d.
                    let from = if v.per_instance && v.divisor > 1 {
                        let b = base_instance.max(0) as usize;
                        if e < b { e } else { b + (e - b) / v.divisor as usize }
                    } else {
                        e
                    };
                    for at in &v.attrs {
                        let s0 = from * v.src_stride as usize + at.src_offset as usize;
                        let d0 = e * v.stride as usize + at.offset as usize;
                        // Past the bound size reads zero (NVN bounds-checks vertex fetches).
                        let len = (at.size as usize).min(src.len().saturating_sub(s0));
                        data[d0..d0 + len].copy_from_slice(&src[s0..s0 + len]);
                    }
                }
                self.arena.copy(&self.dev, &data)
            };
            if let Some((buf, off)) = bound {
                self.dev.device.cmd_bind_vertex_buffers(self.cb, v.stream, &[buf], &[off]);
            }
        }
    }

    /// The largest index an indexed draw uses (restart indices excluded).
    unsafe fn max_index(&self, index_type: i32, count: i32, addr: u64) -> u32 {
        let a = crate::nvn::api::translate_virtual(addr as usize);
        if a < 0x10000 || count <= 0 {
            return 0;
        }
        let n = count as usize;
        let r = self.st.prim_restart;
        let keep = |i: u32| r != Some(i);
        match index_type {
            0 => std::slice::from_raw_parts(a as *const u8, n).iter().map(|&i| i as u32).filter(|&i| keep(i)).max().unwrap_or(0),
            1 => std::slice::from_raw_parts(a as *const u16, n).iter().map(|&i| i as u32).filter(|&i| keep(i)).max().unwrap_or(0),
            _ => std::slice::from_raw_parts(a as *const u32, n).iter().copied().filter(|&i| keep(i)).max().unwrap_or(0),
        }
    }

    unsafe fn index_buffer(&mut self, index_type: i32, count: i32, addr: u64) -> Option<(vk::Buffer, u64, vk::IndexType)> {
        let a = crate::nvn::api::translate_virtual(addr as usize);
        if a < 0x10000 || count <= 0 {
            return None;
        }
        let n = count as usize;
        let restart = self.st.prim_restart;
        match index_type {
            // 8-bit indices: widened to 16 bits.
            0 => {
                let src = std::slice::from_raw_parts(a as *const u8, n);
                let wide: Vec<u8> = src
                    .iter()
                    .flat_map(|&i| if restart == Some(i as u32) { 0xFFFFu16 } else { i as u16 }.to_le_bytes())
                    .collect();
                self.arena.copy(&self.dev, &wide).map(|(b, o)| (b, o, vk::IndexType::UINT16))
            }
            1 if restart.is_some_and(|r| r != 0xFFFF) => {
                let r = restart.unwrap();
                let src = std::slice::from_raw_parts(a as *const u16, n);
                let v: Vec<u8> = src.iter().flat_map(|&i| if i as u32 == r { 0xFFFF } else { i }.to_le_bytes()).collect();
                self.arena.copy(&self.dev, &v).map(|(b, o)| (b, o, vk::IndexType::UINT16))
            }
            1 => {
                let x = self.imports.find(&self.dev, a, n * 2).filter(|(_, o)| o % 2 == 0);
                x.or_else(|| self.arena.copy(&self.dev, std::slice::from_raw_parts(a as *const u8, n * 2)))
                    .map(|(b, o)| (b, o, vk::IndexType::UINT16))
            }
            _ if restart.is_some_and(|r| r != u32::MAX) => {
                let r = restart.unwrap();
                let src = std::slice::from_raw_parts(a as *const u32, n);
                let v: Vec<u8> = src.iter().flat_map(|&i| if i == r { u32::MAX } else { i }.to_le_bytes()).collect();
                self.arena.copy(&self.dev, &v).map(|(b, o)| (b, o, vk::IndexType::UINT32))
            }
            _ => {
                let x = self.imports.find(&self.dev, a, n * 4).filter(|(_, o)| o % 4 == 0);
                x.or_else(|| self.arena.copy(&self.dev, std::slice::from_raw_parts(a as *const u8, n * 4)))
                    .map(|(b, o)| (b, o, vk::IndexType::UINT32))
            }
        }
    }

    unsafe fn dynamic_state(&mut self) {
        let Some(p) = &self.pass else { return };
        let (pw, ph) = p.extent;
        let (x, y, mut w, mut h) = self.st.viewport;
        if w <= 0 || h <= 0 {
            (w, h) = (pw as i32, ph as i32);
        }
        let (near, far) = self.st.depth_range;
        // Window y = 0 is memory row 0 in both NVN origin modes; upper-left flips NDC y (Vulkan's points down).
        let vp = if crate::nvn::api::window_origin_mode() == 1 {
            vk::Viewport { x: x as f32, y: (y + h) as f32, width: w as f32, height: -(h as f32), min_depth: near, max_depth: far }
        } else {
            vk::Viewport { x: x as f32, y: y as f32, width: w as f32, height: h as f32, min_depth: near, max_depth: far }
        };
        let (sx, sy, sw, sh) = self.st.scissor.unwrap_or((0, 0, pw as i32, ph as i32));
        let (x0, y0) = (sx.clamp(0, pw as i32), sy.clamp(0, ph as i32));
        let (x1, y1) = ((sx + sw).clamp(0, pw as i32), (sy + sh).clamp(0, ph as i32));
        let sc = vk::Rect2D {
            offset: vk::Offset2D { x: x0, y: y0 },
            extent: vk::Extent2D { width: (x1 - x0).max(0) as u32, height: (y1 - y0).max(0) as u32 },
        };
        let d = &self.dev.device;
        d.cmd_set_viewport(self.cb, 0, &[vp]);
        d.cmd_set_scissor(self.cb, 0, &[sc]);
        let [r, vm, wm] = self.st.stencil;
        for (face, i) in [(vk::StencilFaceFlags::FRONT, 0), (vk::StencilFaceFlags::BACK, 1)] {
            d.cmd_set_stencil_reference(self.cb, face, r[i]);
            d.cmd_set_stencil_compare_mask(self.cb, face, vm[i]);
            d.cmd_set_stencil_write_mask(self.cb, face, wm[i]);
        }
        d.cmd_set_blend_constants(self.cb, &self.st.blend_color);
        let (factor, units, clamp) = self.st.polygon_offset;
        d.cmd_set_depth_bias(self.cb, units, clamp, factor);
    }
}

