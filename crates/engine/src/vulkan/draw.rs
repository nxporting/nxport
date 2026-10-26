// Draws: the pipeline for the bound state, the resources each stage's bindings name, vertex and index buffers.

use super::*;

const DRIVER_CBUF_SIZE: usize = 0x1000;

/// One draw's or dispatch's descriptors: (binding, type, buffer) and (binding, type, images).
#[derive(Default)]
pub(super) struct DescWrites {
    buffers: smallvec::SmallVec<[(u32, vk::DescriptorType, vk::DescriptorBufferInfo); 24]>,
    images: smallvec::SmallVec<[(u32, vk::DescriptorType, smallvec::SmallVec<[vk::DescriptorImageInfo; 1]>); 24]>,
}
/// The support buffer with the constant buffer bound sizes (sass2spv::accessor::Host::cbuf_limits).
const SUPPORT_SIZE: usize = sass2spv::translation::properties::SUPPORT_SAMPLER_SLOTS_OFFSET
    + 16 * sass2spv::translation::properties::SUPPORT_SAMPLER_SLOTS_COUNT as usize;

/// The support buffer's constant part: render scales of 1 for every texture (no resolution scaling).
fn support_template() -> &'static [u8; SUPPORT_SIZE] {
    static T: std::sync::OnceLock<[u8; SUPPORT_SIZE]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut s = [0u8; SUPPORT_SIZE];
        for i in 0..73 {
            s[192 + i * 16..196 + i * 16].copy_from_slice(&1.0f32.to_le_bytes());
        }
        s
    })
}

/// Largest vertex stream kept repacked across submits.
const REPACK_CACHE_MAX: usize = 8 << 20;

/// A vertex stream repacked for all its elements, with the hash of its guest bytes then and the last frame a draw
/// used it.
pub(super) struct RepackedStream {
    pub(super) buf: crate::vulkan::device::HostBuffer,
    elems: usize,
    token: u64,
    pub(super) used: u64,
}

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
pub(in crate::vulkan) struct VStream {
    stream: u32,
    stride: u32,
    src_stride: u32,
    divisor: u32,
    per_instance: bool,
    /// Copied into an aligned layout (`stride`, the attributes' `offset`) instead of bound in place.
    repack: bool,
    attrs: Vec<VAttr>,
}
/// The constant buffer NVN keeps bound texture and image handles in: handles a shader reads from it are the bound
/// tables (the Metal backend reads every handle from them).
const TEXTURE_TABLE_CBUF: i32 = 2;

/// The vertex binding that feeds unbound attributes (above NVN's 16 streams).

impl Ctx {
    /// The guest program's vertex and fragment stages: (code, control section, content hash).
    fn graphics_stages(&self) -> Option<((u64, u64, u64), Option<(u64, u64, u64)>)> {
        let stages = self.programs.get(&self.st.program)?;
        let mut vs = None;
        let mut fs = None;
        for &(data, control, content) in stages {
            if data < 0x10000 {
                continue;
            }
            let sph = unsafe { std::ptr::read_unaligned((data + 0x30) as *const u32) };
            match (sph >> 10) & 0xF {
                5 => fs = Some((data, control, content)),
                0 | 1 => vs = Some((data, control, content)),
                _ => {}
            }
        }
        Some((vs?, fs))
    }

    /// The vertex stage's state: attribute types from the bound attribute formats, NVN's clip conventions.
    fn vertex_state(&self) -> (GraphicsState, u64) {
        let s = vertex_graphics_state(self.st.attribs.as_slice().iter().copied().enumerate(), crate::nvn::api::depth_mode() == 0);
        let codes: Vec<u8> = s.attribute_types.iter().map(|&t| attr_type_code(t)).collect();
        let key = hash(&(codes, s.depth_mode));
        (s, key)
    }

    /// The fragment stage's state: output types from the bound color targets' formats.
    fn fragment_state(&self) -> (GraphicsState, u64) {
        // Unbound slots keep the engine default (Uint): a code of their own, or a stage translated for a pass with an
        // unbound slot is reused for one with a float target there.
        let rendered: Vec<(usize, i32)> =
            self.pass.iter().flat_map(|p| p.colors.iter().enumerate().take(8).filter_map(|(i, c)| c.map(|c| (i, c.2)))).collect();
        let s = fragment_graphics_state(rendered.iter().copied());
        let mut codes = [0xFFu8; 8];
        for &(i, _) in &rendered {
            codes[i] = attr_type_code(s.fragment_output_types[i]);
        }
        let depth = self.discard_depth();
        (s, hash(&(codes, depth)))
    }

    /// Whether this pass's discarding fragment stages write their depth (Dev::discard_writes_depth, with a depth target).
    fn discard_depth(&self) -> bool {
        self.dev.discard_writes_depth && self.pass.as_ref().is_some_and(|p| p.depth.is_some_and(|(_, f)| format::aspects(f).contains(vk::ImageAspectFlags::DEPTH)))
    }

    /// `fragment_inputs`: for a fragment stage, the varyings its vertex stage writes (part of `key`).
    /// `vertex`: a graphics vertex stage (its translation depends only on the attributes it reads).
    #[allow(clippy::too_many_arguments)]
    unsafe fn stage(&mut self, data: u64, control: u64, index: usize, state: GraphicsState, key: u64, set: u32, fragment_inputs: Option<u32>, vertex: bool) -> bool {
        let discard_depth = set == 1 && self.discard_depth();
        let k = (data, key);
        if self.stages.contains_key(&k) {
            return true;
        }
        if self.failed_stages.contains(&k) {
            return false;
        }
        // Graphics stages share set 0, the fragment stage's bindings above the vertex stage's.
        let base = if set == 1 { shader::FRAGMENT_BINDING_BASE } else { 0 };
        let code = shader::guest_code(data);
        let prep = shader::prep(code, shader::guest_ctrl(control), index);
        let known = crate::vulkan::stage_cache::identify(code, &prep);
        let state_for = |inputs: Option<&[u32]>| {
            let mut s = state.clone();
            if let (true, Some(inputs)) = (vertex, inputs) {
                for (i, t) in s.attribute_types.iter_mut().enumerate() {
                    if !inputs.contains(&(i as u32)) {
                        *t = AttributeType::Float;
                    }
                }
            }
            Some(s)
        };
        match shader::get(&self.dev, code, prep, known, &state_for, 0, base, fragment_inputs, discard_depth) {
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
        // Pipelines listed by earlier sessions, made on worker threads from the first draw.
        if self.pipeline_cache.is_none() {
            let cache = self.pipeline_cache();
            crate::vulkan::listed::start(self.dev.clone(), cache);
        }
        if self.st.render_disabled || instances <= 0 || !self.begin_pass() {
            return;
        }
        let Some(topology) = state::topology(mode) else {
            self.warn(format!("draw mode {mode} is not implemented"));
            return;
        };
        let Some(((vd, vc, vh), fs)) = self.graphics_stages() else {
            self.warn(format!("program {:#x} has no vertex stage", self.st.program));
            return;
        };
        let (vstate, vkey) = self.vertex_state();
        let vkey = hash(&(vkey, vh));
        if !self.stage(vd, vc, 0, vstate, vkey, 0, None, true) {
            return;
        }
        // Metal links a fragment function's inputs to the vertex function's outputs (MoltenVK).
        let varyings = self.dev.link_varyings.then(|| self.stages[&(vd, vkey)].outputs);
        let fkey = match fs {
            Some((fd, fc, fh)) => {
                let (fstate, fkey) = self.fragment_state();
                let fkey = hash(&(fkey, fh, varyings));
                if !self.stage(fd, fc, 1, fstate, fkey, 1, varyings, false) {
                    return;
                }
                Some((fd, fkey))
            }
            None => None,
        };
        // Rebuilt only after the attribute or stream state or the vertex stage's inputs changed.
        let vfull = self.stages[&(vd, vkey)].known.clone();
        let (vl, vl_hash) = match &self.vl_cache {
            Some((vl, h, f)) if *f == vfull.full => (vl.clone(), *h),
            _ => {
                let vl: std::sync::Arc<[VStream]> = self.vertex_layout(&vfull.inputs).into();
                let h = hash(&vl);
                self.vl_cache = Some((vl.clone(), h, vfull.full));
                (vl, h)
            }
        };
        let Some((pipe, layout)) = self.pipeline(mode, topology, (vd, vkey), fkey, &vl, vl_hash) else { return };
        let Some((set_layout, _, push)) = self.graphics_layout((vd, vkey), fkey) else { return };
        let mut w = DescWrites::default();
        self.stage_writes((vd, vkey), vd, vc, 0, &call, base_instance, &mut w);
        if let (Some(k), Some((fd, fc, _))) = (fkey, fs) {
            self.stage_writes(k, fd, fc, 1, &call, base_instance, &mut w);
        }
        // A feedback copy ended the pass: it resumes (loading what was rendered).
        if self.pass.is_none() && !self.begin_pass() {
            return;
        }
        self.ensure_begun();
        let d = &self.dev.device;
        d.cmd_bind_pipeline(self.cb, vk::PipelineBindPoint::GRAPHICS, pipe);
        self.bind_writes(&w, vk::PipelineBindPoint::GRAPHICS, layout, set_layout, push);
        self.bind_vertex_buffers(&vl, &call, base_instance, instances);
        self.dynamic_state();
        if let Some(p) = self.pass.as_mut() {
            p.draws += 1;
            if crate::gpu::history::journal_on() {
                let pair = (self.stages[&(vd, vkey)].known.full, fkey.map_or(0, |k| self.stages[&k].known.full));
                match p.pairs.iter_mut().find(|e| (e.0, e.1) == pair) {
                    Some(e) => e.2 += 1,
                    None => p.pairs.push((pair.0, pair.1, 1)),
                }
            }
        }
        match call {
            Draw::Arrays { first, count } => {
                self.dev.device.cmd_draw(self.cb, count.max(0) as u32, instances as u32, first.max(0) as u32, base_instance.max(0) as u32);
            }
            Draw::Elements { index_type, count, addr, base_vertex } => {
                // Restart indices present: Metal-based hosts restart only strips and compare after adding the base
                // vertex, so such ranges become 32-bit indices with the base vertex folded in (gpu::restart).
                if let Some(r) = self.st.prim_restart.filter(|_| self.dev.metal_restart) {
                    let a = crate::nvn::api::translate_virtual(addr as usize);
                    let key = (mode, index_type, a, count, r, base_vertex);
                    let rewritten = match self.restart_memo.get(&key) {
                        Some(&x) => x,
                        None => {
                            let x = crate::gpu::restart::restart_rewrite(mode, index_type, a as u64, count, r, base_vertex).and_then(|v| {
                                let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4);
                                self.arena.copy(&self.dev, bytes).map(|(b, o)| (b, o, v.len() as u32))
                            });
                            self.restart_memo.insert(key, x);
                            x
                        }
                    };
                    if let Some((buf, off, n)) = rewritten {
                        let d = &self.dev.device;
                        d.cmd_bind_index_buffer(self.cb, buf, off, vk::IndexType::UINT32);
                        d.cmd_draw_indexed(self.cb, n, instances as u32, 0, 0, base_instance.max(0) as u32);
                        return;
                    }
                }
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
        let Some(&(data, control, content)) = self.programs.get(&self.st.compute_program).and_then(|s| s.first()) else {
            self.warn(format!("compute program {:#x} has no stage", self.st.compute_program));
            return;
        };
        if !self.stage(data, control, 0, GraphicsState::zeroed(), content, 0, None, false) {
            return;
        }
        if crate::gpu::history::journal_on() {
            let full = self.stages[&(data, content)].known.full;
            crate::gpu::history::journal(|| format!("dispatch s_{full:016x} {x}x{y}x{z}"));
        }
        let key = hash(&("compute", data, content));
        let (pipe, layout) = match self.pipelines.get(&key) {
            Some(&p) => p,
            None => {
                let s = &self.stages[&(data, content)];
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
                let cache = self.pipeline_cache();
                crate::vulkan::PIPELINES_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let pipe = match self.dev.device.create_compute_pipelines(cache, &[info], None) {
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
        let mut w = DescWrites::default();
        self.stage_writes((data, content), data, control, 5, &Draw::Arrays { first: 0, count: 0 }, 0, &mut w);
        let set_layout = self.stages[&(data, content)].set_layout;
        self.dev.device.cmd_bind_pipeline(self.cb, vk::PipelineBindPoint::COMPUTE, pipe);
        self.bind_writes(&w, vk::PipelineBindPoint::COMPUTE, layout, set_layout, false);
        let d = &self.dev.device;
        d.cmd_dispatch(self.cb, x as u32, y as u32, z as u32);
        self.dev.full_barrier(self.cb);
    }

    /// The descriptor set layout (both stages' bindings in set 0) and pipeline layout of a vertex / fragment stage pair,
    /// and whether its descriptors are pushed: when each stage stays within the push limits.
    unsafe fn graphics_layout(&mut self, vs: (u64, u64), fs: Option<(u64, u64)>) -> Option<(vk::DescriptorSetLayout, vk::PipelineLayout, bool)> {
        if let Some(&l) = self.graphics_layouts.get(&(vs, fs)) {
            return Some(l);
        }
        let vstage = self.stages.get(&vs)?;
        let fstage = fs.and_then(|k| self.stages.get(&k));
        // The one the pipeline workers made for this pair, if any (their pipelines were made with it).
        let l = match pipelines::made_layout(vstage.id, fstage.map(|f| f.id)) {
            Some(l) => l,
            None => pipelines::layout(&self.dev, vstage, fstage)?,
        };
        self.graphics_layouts.insert((vs, fs), l);
        Some(l)
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn pipeline(
        &mut self,
        mode: i32,
        topology: vk::PrimitiveTopology,
        vs: (u64, u64),
        fs: Option<(u64, u64)>,
        vl: &[VStream],
        vl_hash: u64,
    ) -> Option<(vk::Pipeline, vk::PipelineLayout)> {
        let (_, layout, _) = self.graphics_layout(vs, fs)?;
        let cache = self.pipeline_cache();
        let pass = self.pass.as_ref()?;
        let upper_left = crate::nvn::api::window_origin_mode() == 1;
        let gl_depth = crate::nvn::api::depth_mode() == 0;
        let color_fmts: Vec<vk::Format> = pass.colors.iter().map(|c| c.map_or(vk::Format::UNDEFINED, |c| c.1)).collect();
        let depth_fmt = pass.depth.map_or(vk::Format::UNDEFINED, |d| d.1);
        let restart = self.st.prim_restart.is_some()
            && matches!(topology, vk::PrimitiveTopology::LINE_STRIP | vk::PrimitiveTopology::TRIANGLE_STRIP | vk::PrimitiveTopology::TRIANGLE_FAN);
        // Only the state the pipeline is made from (pipelines::normalize): the game leaves other bits set at random.
        let (blend, color_state, channel_mask, depth_stencil) = pipelines::normalize(
            &self.st.blend,
            self.st.color_state,
            self.st.channel_mask,
            self.st.depth_stencil,
            &color_fmts,
            depth_fmt,
        );
        let key = hash(&(
            (vs, fs, vl_hash, &blend),
            (color_state, channel_mask, depth_stencil, self.st.polygon, self.st.multisample),
            (color_fmts.iter().map(|f| f.as_raw()).collect::<Vec<_>>(), depth_fmt.as_raw(), pass.samples.as_raw()),
            (topology.as_raw(), restart, self.st.depth_clamp, upper_left, gl_depth),
        ));
        if let Some(&p) = self.pipelines.get(&key) {
            return Some(p);
        }
        let vstage = self.stages.get(&vs)?;
        let fstage = fs.and_then(|k| self.stages.get(&k));
        let desc = pipelines::Desc {
            vs: vstage.id,
            fs: fstage.map(|f| f.id),
            topology: topology.as_raw(),
            restart,
            streams: desc_streams(vl),
            blend,
            color_state,
            channel_mask,
            depth_stencil,
            polygon: self.st.polygon,
            multisample: self.st.multisample,
            depth_clamp: self.st.depth_clamp,
            colors: color_fmts.iter().map(|f| f.as_raw()).collect(),
            depth: depth_fmt.as_raw(),
            samples: pass.samples.as_raw(),
            upper_left,
            gl_depth,
        };
        let made = pipelines::take(hash(&desc)).filter(|&(_, l)| l == layout);
        let pipe = match made {
            Some((p, _)) => p,
            None => {
                crate::vulkan::PIPELINES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                crate::vulkan::PIPELINES_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                match pipelines::create(&self.dev, &desc, vstage, fstage, layout, cache) {
                    Ok(p) => {
                        self.record_pipeline(mode, vstage, fstage, &desc);
                        p
                    }
                    Err(e) => {
                        let targets: Vec<String> = pass.colors.iter().map(|c| c.map_or("-".into(), |c| format!("{:?}/nvn {}", c.1, c.2))).collect();
                        self.warn(format!("pipeline for {:#x}/{:?} (targets {targets:?}): {e}", vs.0, fs.map(|f| f.0)));
                        return None;
                    }
                }
            }
        };
        self.pipelines.insert(key, (pipe, layout));
        Some((pipe, layout))
    }

    /// The driver's constant buffer 0 for NVN stage `stage`: draw parameters and the bound storage buffers' descriptors.
    /// Fills `c` (DRIVER_CBUF_SIZE bytes) with the driver constant buffer of `stage` for this draw.
    fn driver_cbuf(&self, stage: i32, call: &Draw, base_instance: i32, c: &mut Vec<u8>) {
        c.clear();
        c.resize(DRIVER_CBUF_SIZE, 0);
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
    }

    /// Guest constant buffer `slot` of NVN stage `stage` as the shader sees it (None: nothing bound).
    /// A constant buffer's bytes where they are (the driver buffer being built, the shader's constant bank, guest
    /// memory): (start, length).
    unsafe fn cbuf_bytes(&self, stage: i32, slot: i32, driver: &[u8], code: (u64, u64)) -> Option<(*const u8, usize)> {
        match slot {
            0 => Some((driver.as_ptr(), driver.len())),
            1 => {
                let (data, control) = code;
                if control < 0x1_0000_0000 || *(control as *const u32) != 0x9876_1234 || *((control + 0x20) as *const u32) < 0x708 {
                    return None;
                }
                let off = *((control + 0x700) as *const u32) as u64;
                let size = (*((control + 0x6fc) as *const u32) as usize).min(0x10000);
                (size > 0).then(|| ((data + off) as *const u8, size))
            }
            n if n >= 3 => {
                let &(addr, size) = self.st.uniforms.get(&(stage, n - 3))?;
                let a = crate::nvn::api::translate_virtual(addr as usize);
                (a > 0x10000 && size > 0).then(|| (a as *const u8, size.min(0x10000)))
            }
            _ => None,
        }
    }

    /// The descriptors of stage `key`'s bindings for this draw, added to `out`.
    unsafe fn stage_writes(&mut self, key: (u64, u64), data: u64, control: u64, stage: i32, call: &Draw, base_instance: i32, out: &mut DescWrites) {
        let Some((bindings, base)) = self.stages.get(&key).map(|s| (s.bindings.clone(), s.base)) else { return };
        if bindings.is_empty() {
            return;
        }
        // Only for stages that read it (most do not).
        let mut driver = std::mem::take(&mut self.driver_scratch);
        if bindings.iter().any(|b| {
            matches!(b.feed, Feed::Cbuf(0) | Feed::Sbuf { cb_slot: 0, .. } | Feed::Texture { cbuf_slot: 0, .. } | Feed::Image { cbuf_slot: 0, .. })
        }) {
            self.driver_cbuf(stage, call, base_instance, &mut driver);
        } else {
            driver.clear();
        }
        let (vw, vh) = (self.st.viewport.2.max(1) as f32, self.st.viewport.3.max(1) as f32);
        let DescWrites { buffers, images } = out;
        let zero = Some((self.zeros.buffer, 0u64));
        // Constant buffer contents by slot, read once per draw (storage descriptors and bindless handles read them).
        let mut cbufs: smallvec::SmallVec<[(i32, Option<(*const u8, usize)>); 8]> = Default::default();
        let mut cbuf = |me: &Self, slot: i32| -> Option<&'static [u8]> {
            // Only used within this call, while `driver` and the guest memory are alive.
            let as_slice = |c: Option<(*const u8, usize)>| c.map(|(p, n)| unsafe { std::slice::from_raw_parts(p, n) });
            if let Some((_, c)) = cbufs.iter().find(|(s, _)| *s == slot) {
                return as_slice(*c);
            }
            let c = me.cbuf_bytes(stage, slot, &driver, (data, control));
            cbufs.push((slot, c));
            as_slice(c)
        };
        // Constant buffers first: the support buffer records their bound sizes.
        let mut limits: smallvec::SmallVec<[(u32, usize); 16]> = Default::default();
        // Then the sampler table (filled by the textures), then the support buffer (holding the bound sizes and the
        // textures' table indices).
        let mut ordered: smallvec::SmallVec<[&shader::Binding; 32]> =
            bindings.iter().filter(|b| !matches!(b.feed, Feed::Support | Feed::SamplerTable)).collect();
        ordered.extend(bindings.iter().filter(|b| matches!(b.feed, Feed::SamplerTable)));
        ordered.extend(bindings.iter().filter(|b| matches!(b.feed, Feed::Support)));
        // Sampler table: the distinct samplers, and each texture slot's index in it.
        let mut table: smallvec::SmallVec<[vk::Sampler; 16]> = Default::default();
        let mut slots: smallvec::SmallVec<[(u32, u32); 32]> = Default::default();
        let table_size = bindings.iter().find(|b| matches!(b.feed, Feed::SamplerTable)).map_or(0, |b| b.count as usize);
        for b in ordered {
            match &b.feed {
                Feed::Support => {
                    // Its contents depend on the viewport and the bound sizes only: one copy per submit each.
                    let key = super::hash_of(&(vw.to_bits(), vh.to_bits(), &limits[..], &slots[..]));
                    if let Some(&(buf, off)) = self.supports.get(&key) {
                        buffers.push((b.binding, b.ty, vk::DescriptorBufferInfo { buffer: buf, offset: off, range: SUPPORT_SIZE as u64 }));
                        continue;
                    }
                    let mut sup = *support_template();
                    let put = |s: &mut [u8; SUPPORT_SIZE], off: usize, v: [f32; 4]| {
                        for (i, x) in v.iter().enumerate() {
                            s[off + i * 4..off + i * 4 + 4].copy_from_slice(&x.to_le_bytes());
                        }
                    };
                    put(&mut sup, 144, [2.0 / vw, 2.0 / vh, 1.0, 0.0]);
                    put(&mut sup, 160, [vw, vh, 1.0, 0.0]);
                    for &(slot, index) in &slots {
                        let o = sass2spv::translation::properties::SUPPORT_SAMPLER_SLOTS_OFFSET + 16 * slot as usize;
                        if o + 4 <= sup.len() {
                            sup[o..o + 4].copy_from_slice(&index.to_le_bytes());
                        }
                    }
                    // Bound sizes of the constant buffers (vec4 units), for the shader's checks.
                    for &(binding, len) in &limits {
                        let o = sass2spv::translation::properties::SUPPORT_CBUF_LIMITS_OFFSET + 16 * binding as usize;
                        if o + 4 <= sup.len() {
                            sup[o..o + 4].copy_from_slice(&(len.div_ceil(16) as u32).to_le_bytes());
                        }
                    }
                    if let Some((buf, off)) = self.arena.copy(&self.dev, &sup) {
                        self.supports.insert(key, (buf, off));
                        buffers.push((b.binding, b.ty, vk::DescriptorBufferInfo { buffer: buf, offset: off, range: sup.len() as u64 }));
                    }
                }
                Feed::Cbuf(slot) => {
                    // Written by GPU work earlier in this submit or by earlier submits still running: in place (the
                    // GPU reads it after those writes, in queue order; a copy now could catch them half done).
                    if *slot >= 3 {
                        if let Some(&(addr, size)) = self.st.uniforms.get(&(stage, slot - 3)) {
                            let a = crate::nvn::api::translate_virtual(addr as usize);
                            let len = size.min(0x10000);
                            if a > 0x10000 && self.gpu_writes(a, a + len) {
                                if let Some((buf, off)) = self.imports.find(&self.dev, a, len) {
                                    if off % self.dev.limits.min_uniform_buffer_offset_alignment.max(1) == 0 {
                                        limits.push((b.binding - base, len));
                                        buffers.push((b.binding, b.ty, vk::DescriptorBufferInfo { buffer: buf, offset: off, range: len as u64 }));
                                        continue;
                                    }
                                }
                                self.warn("a uniform buffer the GPU writes in the same submit could not be bound in place");
                            }
                        }
                    }
                    // In place unless this submit's commands write that memory: the write guard does not hold uniform
                    // pages (avoiding mprotect signal storms on CPU-updated matrices/UI constants), and the shader checks the
                    // bound size. TotK finishes some of a frame's constants (HUD and dialogue text) after it submits the frame;
                    // copies taken while recording, or dropped into copies by hot-page tracking, draw other glyphs (the "L" key
                    // icon as a digit, dialogue text flickering). Always bind in place when aligned and non-overlapping.
                    if *slot >= 3 {
                        if let Some(&(addr, size)) = self.st.uniforms.get(&(stage, slot - 3)) {
                            let a = crate::nvn::api::translate_virtual(addr as usize);
                            let len = size.min(0x10000);
                            if a > 0x10000 && len > 0 && !self.cmd_writes.overlaps(a, a + len) {
                                if let Some((buf, off)) = self.imports.find(&self.dev, a, len) {
                                    if off % self.dev.limits.min_uniform_buffer_offset_alignment.max(1) == 0 {
                                        limits.push((b.binding - base, len));
                                        buffers.push((b.binding, b.ty, vk::DescriptorBufferInfo { buffer: buf, offset: off, range: len as u64 }));
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    let bytes = cbuf(self, *slot);
                    let x = match &bytes {
                        // The driver buffer changes per draw; others are copied once per contents (the same memory
                        // with no write by this submit's commands in between).
                        // Only what the stage reads at literal offsets (all of it when it indexes at run time); the bound
                        // size still goes to the shader's check.
                        Some(v) if *slot != 0 => {
                            let n = if b.extent >= 0 { (b.extent as usize).max(16).min(v.len()) } else { v.len() };
                            let key = (v.as_ptr() as usize, n, self.ubo_epoch);
                            match self.ubo_copies.get(&key) {
                                Some(&(b, o)) => Some((b, o, n as u64, v.len())),
                                None => {
                                    let c = self.arena.copy(&self.dev, &v[..n]);
                                    if let Some(c) = c {
                                        self.ubo_copies.insert(key, c);
                                    }
                                    c.map(|(b, o)| (b, o, n as u64, v.len()))
                                }
                            }
                        }
                        Some(v) => self.arena.copy(&self.dev, v).map(|(b, o)| (b, o, v.len() as u64, v.len())),
                        None => zero.map(|(b, o)| (b, o, 256, 256)),
                    };
                    if let Some((buf, off, len, bound)) = x {
                        limits.push((b.binding - base, bound));
                        buffers.push((b.binding, b.ty, vk::DescriptorBufferInfo { buffer: buf, offset: off, range: len.max(1) }));
                    }
                }
                Feed::Sbuf { cb_slot, cb_offset, write } => {
                    let desc = cbuf(self, *cb_slot).and_then(|c| {
                        // In 32-bit words.
                        let o = *cb_offset as usize * 4;
                        (o + 12 <= c.len()).then(|| {
                            (u64::from_le_bytes(c[o..o + 8].try_into().unwrap()), u32::from_le_bytes(c[o + 8..o + 12].try_into().unwrap()) as usize)
                        })
                    });
                    // Only the driver's descriptors (constant buffer 0) carry a size; in the game's own buffers the
                    // word after the address is whatever the game keeps there (TotK's Depths walls: a texture handle,
                    // read as 258 MB). Those are sized by the memory mapped from the address, at most 1 MiB, as
                    // Ryujinx does (StateUpdater: GetMappedSize, MaxUnknownStorageSize).
                    let desc = desc.and_then(|(addr, size)| {
                        if *cb_slot == 0 {
                            return Some((addr, size));
                        }
                        let a = crate::nvn::api::translate_virtual(addr as usize);
                        let (base, len) = crate::nvn::api::find_memory_pool(a)?;
                        Some((addr, (base + len - a).min(1 << 20)))
                    });
                    let info = match desc {
                        Some((addr, size)) if addr > 0x10000 && size > 0 => self.storage_buffer(addr as usize, size, *write),
                        _ => None,
                    };
                    match info.or_else(|| zero.map(|(b, o)| vk::DescriptorBufferInfo { buffer: b, offset: o, range: 256 })) {
                        Some(i) => buffers.push((b.binding, b.ty, i)),
                        None => {}
                    }
                }
                Feed::Texture { cbuf_slot, handle_index, view, count, buffer, slot } => {
                    let mut infos: smallvec::SmallVec<[vk::DescriptorImageInfo; 1]> = Default::default();
                    for j in 0..*count {
                        let handle = if *cbuf_slot < 0 || *cbuf_slot == TEXTURE_TABLE_CBUF {
                            let k = (handle_index - 8) / 2 + j as i32;
                            self.st.textures.get(&(stage, k)).copied()
                        } else {
                            cbuf(self, *cbuf_slot).and_then(|c| {
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
                        let s = match slot {
                            Some(first) => {
                                let index = match table.iter().position(|&t| t == s) {
                                    Some(i) => i,
                                    None if table.len() < table_size => {
                                        table.push(s);
                                        table.len() - 1
                                    }
                                    None => {
                                        self.warn("a draw uses more distinct samplers than its sampler table holds");
                                        table_size.saturating_sub(1)
                                    }
                                };
                                slots.push((first + j, index as u32));
                                vk::Sampler::null()
                            }
                            None => s,
                        };
                        infos.push(vk::DescriptorImageInfo { sampler: s, image_view: v, image_layout: vk::ImageLayout::GENERAL });
                    }
                    images.push((b.binding, b.ty, infos));
                }
                Feed::SamplerTable => {
                    let infos = (0..b.count)
                        .map(|i| vk::DescriptorImageInfo {
                            sampler: table.get(i as usize).copied().unwrap_or(self.default_sampler),
                            image_view: vk::ImageView::null(),
                            image_layout: vk::ImageLayout::GENERAL,
                        })
                        .collect();
                    images.push((b.binding, b.ty, infos));
                }
                Feed::Image { cbuf_slot, handle_index, view, buffer } => {
                    let handle = if *cbuf_slot < 0 || *cbuf_slot == TEXTURE_TABLE_CBUF {
                        let k = (handle_index - 8) / 2;
                        self.st.images.get(&(stage, k)).copied()
                    } else {
                        None
                    };
                    let v = if *buffer { vk::ImageView::null() } else { self.image_binding(handle, *view) };
                    images.push((b.binding, b.ty, smallvec::smallvec![vk::DescriptorImageInfo { sampler: vk::Sampler::null(), image_view: v, image_layout: vk::ImageLayout::GENERAL }]));
                }
            }
        }
        self.driver_scratch = driver;
    }

    /// Binds `w` for the next draw or dispatch: pushed into the command buffer, or written into a new set of `set_layout`.
    unsafe fn bind_writes(&mut self, w: &DescWrites, bind_point: vk::PipelineBindPoint, layout: vk::PipelineLayout, set_layout: vk::DescriptorSetLayout, push: bool) {
        if w.buffers.is_empty() && w.images.is_empty() {
            return;
        }
        // A set with the same contents written earlier in this submit is bound again (draws of one mesh or material).
        let key = (!push).then(|| {
            use std::hash::Hasher;
            let mut h = rustc_hash::FxHasher::default();
            h.write_u64(vk::Handle::as_raw(set_layout));
            for (binding, ty, info) in &w.buffers {
                h.write_u32(*binding);
                h.write_i32(ty.as_raw());
                h.write_u64(vk::Handle::as_raw(info.buffer));
                if *ty != vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC {
                    h.write_u64(info.offset);
                }
                h.write_u64(info.range);
            }
            for (binding, ty, infos) in &w.images {
                h.write_u32(*binding);
                h.write_i32(ty.as_raw());
                for i in infos {
                    h.write_u64(vk::Handle::as_raw(i.sampler));
                    h.write_u64(vk::Handle::as_raw(i.image_view));
                }
            }
            h.finish()
        });
        // Dynamic offsets in binding order.
        let mut dynamic: smallvec::SmallVec<[(u32, u32); 32]> = w
            .buffers
            .iter()
            .filter(|b| b.1 == vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC)
            .map(|b| (b.0, b.2.offset as u32))
            .collect();
        dynamic.sort_unstable_by_key(|d| d.0);
        let offsets: smallvec::SmallVec<[u32; 32]> = dynamic.iter().map(|d| d.1).collect();
        if let Some(&s) = key.and_then(|k| self.set_memo.get(&k)) {
            self.dev.device.cmd_bind_descriptor_sets(self.cb, bind_point, layout, 0, &[s], &offsets);
            return;
        }
        let set = if push {
            vk::DescriptorSet::null()
        } else {
            match self.alloc_set(set_layout) {
                Some(s) => s,
                None => return,
            }
        };
        if let Some(k) = key {
            self.set_memo.insert(k, set);
        }
        let bi: smallvec::SmallVec<[[vk::DescriptorBufferInfo; 1]; 32]> = w
            .buffers
            .iter()
            .map(|b| if b.1 == vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC { [vk::DescriptorBufferInfo { offset: 0, ..b.2 }] } else { [b.2] })
            .collect();
        let mut writes: smallvec::SmallVec<[vk::WriteDescriptorSet; 64]> = w
            .buffers
            .iter()
            .zip(&bi)
            .map(|((binding, ty, _), info)| vk::WriteDescriptorSet::default().dst_set(set).dst_binding(*binding).descriptor_type(*ty).buffer_info(info))
            .collect();
        for (binding, ty, infos) in &w.images {
            if *ty == vk::DescriptorType::UNIFORM_TEXEL_BUFFER || *ty == vk::DescriptorType::STORAGE_TEXEL_BUFFER {
                continue;
            }
            writes.push(vk::WriteDescriptorSet::default().dst_set(set).dst_binding(*binding).descriptor_type(*ty).image_info(infos));
        }
        if push {
            if let Some(p) = &self.dev.push {
                p.ext.cmd_push_descriptor_set(self.cb, bind_point, layout, 0, &writes);
            }
        } else {
            self.dev.device.update_descriptor_sets(&writes, &[]);
            self.dev.device.cmd_bind_descriptor_sets(self.cb, bind_point, layout, 0, &[set], &offsets);
        }
    }

    unsafe fn alloc_set(&mut self, layout: vk::DescriptorSetLayout) -> Option<vk::DescriptorSet> {
        let layouts = [layout];
        if let Some(&p) = self.desc_pools.last() {
            if let Ok(s) = self.dev.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(p).set_layouts(&layouts)) {
                return Some(s[0]);
            }
        }
        if let Some(p) = self.free_desc_pools.pop() {
            self.desc_pools.push(p);
            if let Ok(s) = self.dev.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(p).set_layouts(&layouts)) {
                return Some(s[0]);
            }
        }
        let sizes = [
            vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER, descriptor_count: 1024 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, descriptor_count: 16384 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: 8192 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: 16384 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: 2048 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_TEXEL_BUFFER, descriptor_count: 1024 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_TEXEL_BUFFER, descriptor_count: 1024 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: 4096 },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: 4096 },
        ];
        let p = self.dev.device.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(2048).pool_sizes(&sizes), None).ok()?;
        self.desc_pools.push(p);
        self.dev.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(p).set_layouts(&layouts)).ok().map(|s| s[0])
    }

    /// A storage buffer over guest memory: in place when its pool is imported and the offset aligned, else a copy that is
    /// written back after the submit.
    /// `write`: the stage writes it; read-only ones leave the write tracking and the write guard as they are (bindings
    /// from the game's own buffers are sized up to 1 MiB past their address and covered unrelated textures).
    unsafe fn storage_buffer(&mut self, addr: usize, size: usize, write: bool) -> Option<vk::DescriptorBufferInfo> {
        let a = crate::nvn::api::translate_virtual(addr);
        if write {
            self.gpu_dirty.add(a, a + size);
            // GPU writes cannot fault: pages the write tracking protected would lose them.
            crate::guest::dirty_track::release_for_gpu(a, size);
            crate::vulkan::guard::note_write(a, size);
        }
        // Read-only ones are not held by the write guard either: their bound sizes overstate what the GPU reads (up to
        // 1 MiB for bindings from the game's own buffers), and holding all of it made the game fault on every
        // neighbouring write (walking in Hateno: 49 -> 29 fps).
        let align = self.dev.limits.min_storage_buffer_offset_alignment.max(1);
        if let Some((buf, off)) = self.imports.find(&self.dev, a, size) {
            if off % align == 0 {
                return Some(vk::DescriptorBufferInfo { buffer: buf, offset: off, range: size as u64 });
            }
        }
        let (buf, off, p) = self.arena.alloc(&self.dev, size)?;
        std::ptr::copy_nonoverlapping(a as *const u8, p, size);
        crate::vulkan::guard::patch(a, p, size);
        if write {
            self.writebacks.push((a, p, size));
        }
        Some(vk::DescriptorBufferInfo { buffer: buf, offset: off, range: size as u64 })
    }

    unsafe fn texture_binding(&mut self, handle: Option<u64>, declared: vk::ImageViewType) -> (vk::ImageView, vk::Sampler) {
        let Some(h) = handle else { return (vk::ImageView::null(), self.default_sampler) };
        // Pools change between submits only (registration is CPU work): a handle resolves the same way within one,
        // except where its image is an attachment of the current pass (render-target feedback).
        let key = (h, declared);
        if let Some(&(img, v, s)) = self.bindings.get(&key) {
            if !self.pass.as_ref().is_some_and(|p| p.images.contains(&img)) {
                return (v, s);
            }
        }
        let (tex_id, sampler_id) = ((h & 0xFFFFF) as i32, ((h >> 20) & 0xFFF) as i32);
        let mut image = vk::Image::null();
        let mut feedback = false;
        let view = crate::nvn::api::lookup_pool_texture(tex_id).and_then(|(obj, view)| {
            let (_, t) = crate::nvn::api::texture_snapshot(obj)?;
            image = self.tex.get(&self.dev, &t)?.0.image;
            // Every barrier ends the pass, so the current pass began after the last one.
            if self.pass.as_ref().is_some_and(|p| p.images.contains(&image)) {
                feedback = true;
                if !self.dev.feedback_in_place {
                    return self.feedback_view(&t, view.as_ref(), declared);
                }
            }
            self.tex.sampled_view(&self.dev, &t, view.as_ref(), declared)
        });
        let sampler = crate::nvn::api::lookup_pool_sampler(sampler_id).and_then(|d| self.tex.sampler(&self.dev, &d));
        let r = (view.unwrap_or(vk::ImageView::null()), sampler.unwrap_or(self.default_sampler));
        if !feedback {
            self.bindings.insert(key, (image, r.0, r.1));
        }
        r
    }

    /// A view of the feedback copy of texture `t` (an attachment of the current pass), taken now if this barrier epoch
    /// has none: the pass ends for the copy and resumes at the next draw.
    unsafe fn feedback_view(&mut self, t: &NvnTexture, view: Option<&crate::nvn::api::NvnTextureView>, declared: vk::ImageViewType) -> Option<vk::ImageView> {
        let (d, level, layer) = self.tex.locate(t);
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
        Textures::sampled_view_of(&self.dev, snap, (level, layer), t, view, declared)
    }

    unsafe fn image_binding(&mut self, handle: Option<u64>, declared: vk::ImageViewType) -> vk::ImageView {
        let Some(h) = handle else { return vk::ImageView::null() };
        let Some((obj, view)) = crate::nvn::api::lookup_pool_texture((h & 0xFFFFF) as i32) else { return vk::ImageView::null() };
        let Some((_, t)) = crate::nvn::api::texture_snapshot(obj) else { return vk::ImageView::null() };
        let Some((img, at_level, at_layer)) = self.tex.get(&self.dev, &t) else { return vk::ImageView::null() };
        img.mark_gpu_write();
        let (level, layer) = view.map_or((0, 0), |v| (v.min_level as u32, v.min_layer as u32));
        let (level, layer) = (level + at_level, layer + at_layer);
        let fmt = img.format;
        let layers = if declared == vk::ImageViewType::TYPE_2D || declared == vk::ImageViewType::TYPE_3D { 1 } else { img.layers - layer };
        Textures::view(&self.dev, img, declared, fmt, level, 1, layer, layers, [2, 3, 4, 5], false).unwrap_or(vk::ImageView::null())
    }

    /// The bound vertex streams as the pipeline reads them: in place, or repacked where the device needs aligned strides
    /// and offsets (or cannot read past a stride), and expanded where a stream steps per several instances.
    /// The vertex streams and attributes for the attribute locations `inputs` (the vertex stage's).
    fn vertex_layout(&self, inputs: &[u32]) -> Vec<VStream> {
        let attribs = self.st.attribs.as_slice().iter().enumerate().filter(|(i, _)| inputs.contains(&(*i as u32))).map(|(i, &b)| (i as u32, b));
        vertex_streams(&self.dev, attribs, &self.st.streams)
    }
}

/// A pipeline description's vertex streams.
pub(in crate::vulkan) fn desc_streams(vl: &[VStream]) -> Vec<pipelines::Stream> {
    vl.iter().map(|v| (v.stream, v.stride, v.per_instance, v.attrs.iter().map(|a| (a.location, a.format, a.offset)).collect())).collect()
}

/// A vertex stage's translation state: attribute types from NVN attribute bits by location (unbound attributes read
/// float zeros from ZERO_STREAM), and the depth mode.
pub(in crate::vulkan) fn vertex_graphics_state(attribs: impl Iterator<Item = (usize, u32)>, depth_gl: bool) -> GraphicsState {
    let mut s = GraphicsState::engine();
    s.attribute_types = [AttributeType::Float; 32];
    for (i, bits) in attribs {
        if let (true, Some((_, t))) = (i < 32, format::vertex((bits >> 5) & 0x1FF)) {
            s.attribute_types[i] = t;
        }
    }
    s.origin_upper_left = true;
    s.depth_mode = depth_gl;
    s
}

/// A fragment stage's translation state: output types from the NVN formats its targets render in, by slot.
pub(in crate::vulkan) fn fragment_graphics_state(rendered: impl Iterator<Item = (usize, i32)>) -> GraphicsState {
    let mut s = GraphicsState::engine();
    for (i, nvn) in rendered {
        if i < 8 {
            s.fragment_output_types[i] = match format::kind(nvn) {
                format::Kind::Float => AttributeType::Float,
                format::Kind::Uint => AttributeType::Uint,
                format::Kind::Sint => AttributeType::Sint,
            };
        }
    }
    s.origin_upper_left = true;
    s
}

/// The vertex streams for NVN attribute bits by location (the vertex stage's inputs) and stream bits.
pub(in crate::vulkan) fn vertex_streams(dev: &Dev, attribs: impl Iterator<Item = (u32, u32)>, streams: &[u64]) -> Vec<VStream> {
    {
        let mut out: Vec<VStream> = Vec::new();
        for (i, b) in attribs {
            let Some((f, _)) = format::vertex((b >> 5) & 0x1FF) else { continue };
            let stream = b & 31;
            let bits = streams.get(stream as usize).copied().unwrap_or(0);
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
            v.attrs.push(VAttr { location: i, format: f.as_raw(), offset: b >> 14, src_offset: b >> 14, size });
        }
        let align = dev.vertex_align.max(1);
        for v in out.iter_mut() {
            let misaligned = v.stride % align != 0 || v.attrs.iter().any(|a| a.offset % align != 0);
            let beyond = !dev.beyond_stride && v.attrs.iter().any(|a| a.offset + a.size > v.stride);
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
}

impl Ctx {
    /// Adds a new pipeline to the pipeline list, and checks that the entry gives back this pipeline (listed.rs).
    unsafe fn record_pipeline(&self, mode: i32, vstage: &Stage, fstage: Option<&Stage>, desc: &pipelines::Desc) {
        let Some(pass) = self.pass.as_ref() else { return };
        let attribs = self.st.attribs.as_slice();
        let e = nxport_pipelines::Entry {
            vs: crate::vulkan::listed::name(vstage.known.full),
            fs: fstage.map(|f| crate::vulkan::listed::name(f.known.full)),
            point_size: self.st.point_size.to_bits(),
            depth_gl: desc.gl_depth,
            y_negate: !desc.upper_left,
            color_state: self.st.color_state,
            channel_mask: self.st.channel_mask.unwrap_or(u32::MAX),
            depth_pf: 0,
            color_pfs: [0; 8],
            blends: self.st.blend,
            attr_bits: vstage.known.inputs.iter().map(|&l| attribs.get(l as usize).copied()).collect(),
            streams: self.st.streams.clone(),
            nvn: Some(nxport_pipelines::Nvn {
                colors: pass.nvn_colors,
                depth: pass.nvn_depth,
                samples: pass.samples.as_raw(),
                mode: mode as u32,
                restart: self.st.prim_restart.is_some(),
                polygon: self.st.polygon,
                depth_stencil: self.st.depth_stencil.unwrap_or_else(state::default_depth_stencil),
                multisample: self.st.multisample,
                depth_clamp: self.st.depth_clamp,
            }),
        }
        .normalized();
        let back = crate::vulkan::listed::desc(&self.dev, &e, vstage, fstage);
        let keys = crate::vulkan::listed::stage_keys(&self.dev, &e, vstage, fstage);
        if back.as_ref() != Some(desc) || keys != Some((vstage.id.key, fstage.map(|f| f.id.key))) {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 8 {
                println!("[vulkan] pipeline list entry does not give back its pipeline: {}\n  drawn  {desc:?}\n  listed {back:?}\n  stage keys {keys:?}", e.line());
            }
        }
        crate::gpu::pipeline_list::record(e);
    }

    unsafe fn bind_vertex_buffers(&mut self, vl: &[VStream], call: &Draw, base_instance: i32, instances: i32) {
        self.dev.device.cmd_bind_vertex_buffers(self.cb, pipelines::ZERO_STREAM, &[self.zeros.buffer], &[0]);
        for v in vl {
            let (addr, size) = self.st.vertex_buffers.get(v.stream as usize).copied().unwrap_or((0, 0));
            let a = crate::nvn::api::translate_virtual(addr as usize);
            let bound = if a <= 0x10000 || size == 0 {
                Some((self.zeros.buffer, 0))
            } else if !v.repack {
                if let Some(&b) = self.vb_memo.get(&(a, size)) {
                    self.dev.device.cmd_bind_vertex_buffers(self.cb, v.stream, &[b.0], &[b.1]);
                    continue;
                }
                let b = match self.imports.find(&self.dev, a, size).filter(|_| !crate::vulkan::guard::hot(a, size)) {
                    Some(x) => {
                        crate::vulkan::guard::note_read(a, size);
                        Some(x)
                    }
                    // A copy, once per submit.
                    None => {
                        let key = super::hash_of(&(a, size, u32::MAX));
                        match self.repacked.get(&key) {
                            Some(&b) => Some(b),
                            None => {
                                let b = self.arena.copy(&self.dev, std::slice::from_raw_parts(a as *const u8, size));
                                if let Some(b) = b {
                                    self.repacked.insert(key, b);
                                }
                                b
                            }
                        }
                    }
                };
                if let Some(b) = b {
                    self.vb_memo.insert((a, size), b);
                }
                b
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
                // Per-vertex streams: repacked once for all their elements and kept across submits while the write
                // tracking shows their guest bytes unchanged (most are static meshes).
                if !v.per_instance && size <= REPACK_CACHE_MAX {
                    // Checked once per submit (guest memory does not change during one).
                    let skey = super::hash_of(&(a, size, v, n, u32::MAX - 1));
                    if let Some(&b) = self.repacked.get(&skey) {
                        self.dev.device.cmd_bind_vertex_buffers(self.cb, v.stream, &[b.0], &[b.1]);
                        continue;
                    }
                    if let Some(b) = self.cached_repack(a, size, v, n) {
                        self.repacked.insert(skey, (b, 0));
                        self.dev.device.cmd_bind_vertex_buffers(self.cb, v.stream, &[b], &[0]);
                        continue;
                    }
                }
                // The same stream repacked the same way earlier in this submit (guest memory does not change during
                // one).
                let key = super::hash_of(&(a, size, v, n, base_instance));
                if let Some(&b) = self.repacked.get(&key) {
                    self.dev.device.cmd_bind_vertex_buffers(self.cb, v.stream, &[b.0], &[b.1]);
                    continue;
                }
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
                let b = self.arena.copy(&self.dev, &data);
                if let Some(b) = b {
                    self.repacked.insert(key, b);
                }
                b
            };
            if let Some((buf, off)) = bound {
                self.dev.device.cmd_bind_vertex_buffers(self.cb, v.stream, &[buf], &[off]);
            }
        }
    }

    /// Stream `v` at host `a` (`size` bytes) repacked for all its elements (at least `n`), from the cache when its
    /// guest bytes still hash the same, else made now.
    unsafe fn cached_repack(&mut self, a: usize, size: usize, v: &VStream, n: usize) -> Option<vk::Buffer> {
        let key = super::hash_of(&(a, size, v));
        let frame = crate::vulkan::VULKAN.frames.load(std::sync::atomic::Ordering::Relaxed);
        let token = super::texture::hash_bytes(std::slice::from_raw_parts(a as *const u8, size));
        if let Some(e) = self.repack_cache.get_mut(&key) {
            if e.elems >= n && e.token == token {
                e.used = frame;
                return Some(e.buf.buffer);
            }
        }
        let elems = n.max(size.div_ceil(v.src_stride.max(1) as usize));
        let stride = v.stride as usize;
        let buf = self.dev.host_buffer((elems * stride).max(16) as u64, vk::BufferUsageFlags::VERTEX_BUFFER).ok()?;
        let dst = std::slice::from_raw_parts_mut(buf.ptr, elems * stride);
        dst.fill(0);
        let src = std::slice::from_raw_parts(a as *const u8, size);
        for e in 0..elems {
            for at in &v.attrs {
                let s0 = e * v.src_stride as usize + at.src_offset as usize;
                let d0 = e * stride + at.offset as usize;
                // Past the bound size reads zero (NVN bounds-checks vertex fetches).
                if s0 >= src.len() {
                    continue;
                }
                let len = (at.size as usize).min(src.len() - s0);
                dst[d0..d0 + len].copy_from_slice(&src[s0..s0 + len]);
            }
        }
        crate::vulkan::note_copied(elems * stride);
        let b = buf.buffer;
        if let Some(old) = self.repack_cache.insert(key, RepackedStream { buf, elems, token, used: frame }) {
            // Submits in flight may still read it.
            self.staging.push(old.buf);
        }
        Some(b)
    }

    /// The largest index an indexed draw uses (restart indices excluded).
    /// (Guest memory does not change during a submit: each range is scanned once per submit.)
    unsafe fn max_index(&mut self, index_type: i32, count: i32, addr: u64) -> u32 {
        let a = crate::nvn::api::translate_virtual(addr as usize);
        if a < 0x10000 || count <= 0 {
            return 0;
        }
        let n = count as usize;
        let r = self.st.prim_restart;
        let key = (a, count, index_type, r);
        if let Some(&m) = self.max_indices.get(&key) {
            return m;
        }
        let keep = |i: u32| r != Some(i);
        let m = match index_type {
            0 => std::slice::from_raw_parts(a as *const u8, n).iter().map(|&i| i as u32).filter(|&i| keep(i)).max().unwrap_or(0),
            1 => std::slice::from_raw_parts(a as *const u16, n).iter().map(|&i| i as u32).filter(|&i| keep(i)).max().unwrap_or(0),
            _ => std::slice::from_raw_parts(a as *const u32, n).iter().copied().filter(|&i| keep(i)).max().unwrap_or(0),
        };
        self.max_indices.insert(key, m);
        m
    }

    unsafe fn index_buffer(&mut self, index_type: i32, count: i32, addr: u64) -> Option<(vk::Buffer, u64, vk::IndexType)> {
        let a = crate::nvn::api::translate_virtual(addr as usize);
        if a < 0x10000 || count <= 0 {
            return None;
        }
        let n = count as usize;
        let restart = self.st.prim_restart;
        match index_type {
            // 8-bit indices: widened to 16 bits (vectorized fast expansion).
            0 => {
                let src = std::slice::from_raw_parts(a as *const u8, n);
                let mut wide = vec![0u16; n];
                if let Some(r) = restart {
                    let r = r as u8;
                    for (dst, &i) in wide.iter_mut().zip(src) {
                        *dst = if i == r { 0xFFFF } else { i as u16 };
                    }
                } else {
                    for (dst, &i) in wide.iter_mut().zip(src) {
                        *dst = i as u16;
                    }
                }
                let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, n * 2);
                self.arena.copy(&self.dev, bytes).map(|(b, o)| (b, o, vk::IndexType::UINT16))
            }
            1 if restart.is_some_and(|r| r != 0xFFFF) => {
                let r = restart.unwrap();
                let src = std::slice::from_raw_parts(a as *const u16, n);
                let v: Vec<u8> = src.iter().flat_map(|&i| if i as u32 == r { 0xFFFF } else { i }.to_le_bytes()).collect();
                self.arena.copy(&self.dev, &v).map(|(b, o)| (b, o, vk::IndexType::UINT16))
            }
            1 => {
                // In place only at 4-byte aligned offsets: Metal (under MoltenVK) needs them for 16-bit indices too, and
                // passed 2-mod-4 offsets through (Super Mario Odyssey's Cap Kingdom ground: draws with garbage indices).
                let x = self.imports.find(&self.dev, a, n * 2).filter(|(_, o)| o % 4 == 0 && !crate::vulkan::guard::hot(a, n * 2));
                if x.is_some() {
                    crate::vulkan::guard::note_read(a, n * 2);
                }
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
                let x = self.imports.find(&self.dev, a, n * 4).filter(|(_, o)| o % 4 == 0 && !crate::vulkan::guard::hot(a, n * 4));
                if x.is_some() {
                    crate::vulkan::guard::note_read(a, n * 4);
                }
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
        // Unchanged since the previous draw of this pass: nothing to set.
        let key = {
            use std::hash::Hasher;
            let mut hs = rustc_hash::FxHasher::default();
            for f in [vp.x, vp.y, vp.width, vp.height, vp.min_depth, vp.max_depth] {
                hs.write_u32(f.to_bits());
            }
            hs.write_i32(sc.offset.x);
            hs.write_i32(sc.offset.y);
            hs.write_u32(sc.extent.width);
            hs.write_u32(sc.extent.height);
            for v in self.st.stencil.iter().flatten() {
                hs.write_u32(*v);
            }
            for f in self.st.blend_color {
                hs.write_u32(f.to_bits());
            }
            let (factor, units, clamp) = self.st.polygon_offset;
            for f in [factor, units, clamp] {
                hs.write_u32(f.to_bits());
            }
            hs.finish()
        };
        if self.dynamic_memo == Some(key) {
            return;
        }
        self.dynamic_memo = Some(key);
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

