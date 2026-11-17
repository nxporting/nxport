// Compute dispatches of translated compute stages.

use super::*;

impl Gfx {
    /// Runs one draw with the translated shaders. Returns false when it can't (caller falls back).
    /// The compute pipeline of kernel `entry` (no function constants).
    pub(crate) fn make_compute(&self, lib: &ShaderLibs, entry: &str) -> Result<ComputePipe, String> {
        let f = lib.function(entry, &MTLFunctionConstantValues::new()).map_err(|e| format!("compute function {entry}: {e}"))?;
        self.device
            .newComputePipelineStateWithFunction_error(&f)
            .map_err(|e| format!("compute pipeline {entry}: {}", e.localizedDescription()))
    }

    /// nvnCommandBufferDispatchCompute: runs the bound compute program's translated kernel.
    ///
    /// The NVN driver cbuf (c0) of a compute stage holds {u64 address, u32 size} descriptors of the bound uniform
    /// buffers at 0x210 + 0x10 * index and of the storage buffers at 0x310 + 0x10 * index (inferred from TotK's
    /// kernels against the game's BindUniformBuffer / BindStorageBuffer calls: Maxwell compute has only 8 hardware
    /// cbufs, so uniform buffers 5+ are read through these pointers). Uniform buffer N is also cbuf N + 3.
    pub(crate) unsafe fn dispatch_compute(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let shm = st.sh.clone();
        const STAGE_COMPUTE: i32 = 5;
        let Some(lib) = &self.shader_lib else { return };
        let Some(program) = st.compute_program else {
            return;
        };
        let Some(stages) = super::programs::stages(program) else {
            return;
        };
        let Some(code) = stages
            .iter()
            .find(|s| s.cached.map_or(false, |c| c.kind == StageKind::Compute))
            .copied()
        else {
            warn_once(
                format!("compute-missing-{:x}", program),
                format!(
                    "compute program 0x{:x} has no translated kernel: dispatch skipped",
                    program
                ),
            );
            return;
        };
        let cached = code.cached.unwrap();
        super::journal(|| format!("dispatch {} {}x{}x{}", cached.entry, x, y, z));
        if x <= 0 || y <= 0 || z <= 0 {
            return;
        }
        if denylist().lock().unwrap().contains(&cached.entry) {
            return;
        }
        let pipe = match st.compute_pipelines.get(&cached.entry) {
            Some(p) => p.clone(),
            None => {
                // Made here, or by the precompile (a dispatch waits for one being made there).
                let made = compute_cell(&cached.entry).get_or_init(|| self.make_compute(lib, &cached.entry)).clone();
                match made {
                    Ok(p) => {
                        st.compute_pipelines.insert(cached.entry.clone(), p.clone());
                        p
                    }
                    Err(e) => {
                        warn_once(format!("kpipe-{}", cached.entry), e);
                        return;
                    }
                }
            }
        };
        let [lx, ly, lz] = cached.local_size;
        if (lx * ly * lz) as usize > pipe.maxTotalThreadsPerThreadgroup() {
            warn_once(
                format!("klocal-{}", cached.entry),
                format!(
                    "compute {} workgroup {}x{}x{} exceeds the device limit",
                    cached.entry, lx, ly, lz
                ),
            );
            return;
        }

        // Driver cbuf contents: the dispatch's thread group counts (TotK's depth-cell passes index cells by them), then the
        // uniform and storage buffers' descriptors.
        let mut c0 = vec![0u8; 0x1000];
        for (k, n) in [x, y, z].iter().enumerate() {
            c0[0x200 + 4 * k..0x204 + 4 * k].copy_from_slice(&n.to_le_bytes());
        }
        for i in 0..18 {
            if let Some(&(a, sz)) = st.uniform_buffers.get(&(STAGE_COMPUTE, i)) {
                let o = 0x210 + 0x10 * i as usize;
                c0[o..o + 8].copy_from_slice(&a.to_le_bytes());
                c0[o + 8..o + 12].copy_from_slice(&(sz as u32).to_le_bytes());
            }
        }
        for i in 0..16 {
            if let Some((a, sz)) = super::storage_buffer(st, STAGE_COMPUTE, i) {
                let o = 0x310 + 0x10 * i as usize;
                c0[o..o + 8].copy_from_slice(&a.to_le_bytes());
                c0[o + 8..o + 12].copy_from_slice(&(sz as u32).to_le_bytes());
            }
        }
        let mut notes: Vec<String> = Vec::new();
        // Per-draw detail text is only for traces and GPU-error reports (first 16 draws of an encoder); formatting it for
        // every draw cost ~20% of the executor.
        let want_notes = super::tracing_frame()
            || (draw_details() && st.lb.enc_details.last().map_or(false, |d| d.len() < 16));
        // Encoded on the issuer thread like render passes (no sync: a sync per dispatch cost ~2% of the thread).
        let _ = cb;
        let Some(cur_cb) = st.cur_cb.clone() else {
            return;
        };
        // Consecutive dispatches (and tiny compute draws) share one compute encoder: serial dispatch keeps their order
        // and makes each see the previous one's writes; any other op ends it (issuer CJoin / CLinger).
        st.issuer.push(super::issuer::Op::CJoin(cur_cb, cached.entry.clone(), cached.texture_heap));
        self.note_encoder(
            st,
            format!(
                "compute {} [{}x{}x{} groups of {}x{}x{}]",
                cached.entry, x, y, z, lx, ly, lz
            ),
        );
        st.issuer.push(super::issuer::Op::CPipeline(pipe.clone()));

        // Ryujinx support buffer at 0: render scales 1.0 for texture size queries.
        let mut sup = vec![0u8; SUPPORT_BUFFER_SIZE];
        for i in 0..73 {
            sup[192 + i * 16..196 + i * 16].copy_from_slice(&1.0f32.to_le_bytes());
        }
        if let Some((buf, off)) = st.cx.arena.alloc(&self.device, &sup) {
            st.issuer.push(super::issuer::Op::CBuf(buf, off, 0));
        }
        // Descriptor heaps (metal/tex_heap.rs): slots, handles and heap addresses in the driver cbuf, residency.
        if cached.texture_heap {
            let frame = super::presented_frames();
            st.cx.heaps.sweep(frame);
            let mut used = 1u32 << super::tex_heap::SAMPLERS;
            for tb in &cached.textures {
                let (tex, sampler_id) = self.bound_texture(st, STAGE_COMPUTE, tb);
                let Some(tex) = tex.or_else(|| self.dummy_texture(&mut st.tc, &tb.ty)) else {
                    continue;
                };
                let sampler = {
                    let mut g = shm.lock().unwrap();
                    sampler_id
                        .and_then(|id| self.sampler_state(&mut g, id))
                        .or_else(|| self.default_sampler(&mut g))
                };
                let Some(ss) = sampler.and_then(|s| st.cx.heaps.sampler(&self.device, &s, frame))
                else {
                    continue;
                };
                let Some((ts, root)) = st.cx.heaps.texture(&self.device, tb.declared, &tex, frame)
                else {
                    continue;
                };
                let o = super::tex_heap::HANDLES + tb.handle_index as usize * 4;
                c0[o..o + 4].copy_from_slice(&(ts | ss << 20).to_le_bytes());
                used |= 1 << tb.declared.min(super::tex_heap::SAMPLERS - 1);
                for t in [&tex, &root] {
                    st.issuer.push(super::issuer::Op::CUse(
                        ProtocolObject::<dyn objc2_metal::MTLResource>::from_retained(t.clone()),
                        objc2_metal::MTLResourceUsage::Read,
                    ));
                }
            }
            for h in 0..super::tex_heap::HEAPS {
                if used >> h & 1 == 0 {
                    continue;
                }
                let o = super::tex_heap::ADDRESSES + h * 8;
                c0[o..o + 8].copy_from_slice(&st.cx.heaps.address(h).to_le_bytes());
                if let Some(b) = st.cx.heaps.buffer(h).cloned() {
                    st.issuer.push(super::issuer::Op::CUse(
                        ProtocolObject::<dyn objc2_metal::MTLResource>::from_retained(b),
                        objc2_metal::MTLResourceUsage::Read,
                    ));
                }
            }
        }
        // Bound size of each constant buffer the kernel reads, in the driver cbuf (see CBUF_LIMITS_OFFSET).
        if c0.len() >= CBUF_LIMITS_OFFSET + 18 * 4 {
            for b in 0..18usize {
                let l = match cached
                    .cbuffers
                    .iter()
                    .find(|c| c.0 as usize == b)
                    .map(|c| c.1)
                {
                    Some(0) => cbuf_limit(c0.len()),
                    Some(n) if n >= 3 => st
                        .uniform_buffers
                        .get(&(STAGE_COMPUTE, n as i32 - 3))
                        .map_or(0, |&(a, sz)| if a > 0x10000 { cbuf_limit(sz) } else { 0 }),
                    _ => 4096,
                };
                c0[CBUF_LIMITS_OFFSET + b * 4..CBUF_LIMITS_OFFSET + b * 4 + 4]
                    .copy_from_slice(&l.to_le_bytes());
            }
        }
        // Heap stages read the driver cbuf at binding 17 (sass2spv: a parameter of its own when the stage binds it elsewhere).
        if (cached.cbuf_limits && !cached.cbuffers.iter().any(|&(_, slot)| slot == 0))
            || (cached.texture_heap && !cached.cbuffers.iter().any(|&(b, _)| b == 17))
        {
            if let Some((buf, off)) = st.cx.arena.alloc(&self.device, &c0) {
                st.issuer.push(super::issuer::Op::CBuf(buf, off, 17));
            }
        }
        for &(binding, slot) in &cached.cbuffers {
            let bound = match slot {
                0 => st.cx.arena.alloc(&self.device, &c0),
                1 => {
                    let mut g = shm.lock().unwrap();
                    self.const_bank(&mut g, code.data, code.control, cached)
                }
                .map(|b| (b, 0)),
                n if n >= 3 => match st
                    .uniform_buffers
                    .get(&(STAGE_COMPUTE, n as i32 - 3))
                    .copied()
                {
                    Some((addr, size))
                        if addr > 0x10000 && size > 0 && gpu_written(&st.gpu_dirty, addr, size) =>
                    {
                        let mut g = shm.lock().unwrap();
                        self.guest_buffer_ro(&mut g, &mut st.cx, addr, size.min(CBUF_DECL_SIZE))
                    }
                    Some((addr, size)) if addr > 0x10000 && size > 0 => st.cx.arena.alloc(
                        &self.device,
                        std::slice::from_raw_parts(addr as *const u8, size.min(CBUF_DECL_SIZE)),
                    ),
                    _ => None,
                },
                _ => None,
            };
            let (buf, off) = match bound {
                Some(b) => b,
                None => {
                    if want_notes {
                        notes.push(format!("cbuf{} -> mtl{} zero buffer", slot, binding));
                    }
                    match {
                        let mut g = shm.lock().unwrap();
                        self.zero_buffer(&mut g)
                    } {
                        Some(z) => (z, 0),
                        None => continue,
                    }
                }
            };
            st.issuer
                .push(super::issuer::Op::CBuf(buf, off, binding as usize));
        }
        for &(binding, _slot, cb_slot, cb_off) in &cached.sbuffers {
            let byte = cb_off as usize * 4;
            let desc = if cb_slot == 0 && byte + 12 <= c0.len() {
                let a = u64::from_le_bytes(c0[byte..byte + 8].try_into().unwrap());
                let sz = u32::from_le_bytes(c0[byte + 8..byte + 12].try_into().unwrap()) as u64;
                Some((a, sz))
            } else if cb_slot >= 3 {
                st.uniform_buffers
                    .get(&(STAGE_COMPUTE, cb_slot as i32 - 3))
                    .copied()
                    .and_then(|(ua, usz)| {
                        (ua > 0x10000 && byte + 12 <= usz).then(|| {
                            let a = std::ptr::read_unaligned((ua as usize + byte) as *const u64);
                            (a, user_sb_size(a))
                        })
                    })
            } else {
                None
            };
            // Storage buffers the shader never writes are GPU reads (write tracking and the early-fence guard keep them).
            let ro = cached.sb_readonly.contains(&binding);
            let bound = desc.and_then(|(a, sz)| {
                if a > 0x10000 && sz > 0 {
                    let mut g = shm.lock().unwrap();
                    if ro {
                        self.guest_buffer_ro(&mut g, &mut st.cx, a, sz as usize)
                    } else {
                        super::note_gpu_guest_write(super::GuestWrite::Storage, a, sz as usize);
                        self.guest_buffer(&mut g, &mut st.cx, a, sz as usize)
                    }
                } else {
                    None
                }
            });
            if let (Some((a, sz)), Some(_)) = (desc, &bound) {
                super::mark_gpu_dirty(st, a, a + sz);
            }
            if want_notes {
                notes.push(format!(
                    "sb mtl{} desc cb{}+{:#x} -> {:x?} {}",
                    binding,
                    cb_slot,
                    byte,
                    desc,
                    if bound.is_some() { "bound" } else { "zero" }
                ));
            }
            let (buf, off) = match bound {
                Some(b) => b,
                None => match {
                    let mut g = shm.lock().unwrap();
                    self.zero_buffer(&mut g)
                } {
                    Some(z) => (z, 0),
                    None => continue,
                },
            };
            st.issuer
                .push(super::issuer::Op::CBuf(buf, off, binding as usize));
        }
        let mut extra_samplers: Vec<(usize, Retained<ProtocolObject<dyn MTLSamplerState>>)> =
            Vec::new();
        for tb in &cached.textures {
            if cached.texture_heap {
                break;
            }
            let (tex, sampler_id) = self.bound_texture(st, STAGE_COMPUTE, tb);
            let Some(tex) = tex.or_else(|| self.dummy_texture(&mut st.tc, &tb.ty)) else {
                continue;
            };
            if want_notes {
                notes.push(format!(
                    "tex b{} {} -> {:?} {}x{} {:?}",
                    tb.binding,
                    tb.ty,
                    tex.textureType(),
                    tex.width(),
                    tex.height(),
                    tex.pixelFormat()
                ));
            }
            let sampler = {
                let mut g = shm.lock().unwrap();
                sampler_id
                    .and_then(|id| self.sampler_state(&mut g, id))
                    .or_else(|| self.default_sampler(&mut g))
            };
            st.issuer
                .push(super::issuer::Op::CTex(tex, tb.binding as usize));
            if (tb.binding as usize) < EXTRA_SAMPLER_BASE {
                st.issuer
                    .push(super::issuer::Op::CSamp(sampler, tb.binding as usize));
            } else if let Some(s) = sampler {
                extra_samplers.push((tb.binding as usize - EXTRA_SAMPLER_BASE, s));
            }
        }
        if let Some((buf, off)) = self.extra_sampler_buffer(&mut st.cx, &extra_samplers) {
            st.issuer
                .push(super::issuer::Op::CBuf(buf, off, EXTRA_SAMPLER_BUFFER));
        }
        // Storage images: the shader reads image unit i's handle from the same cbuf 2 table as textures, at entry 32 + i
        // (byte 0x120 + 8i, after the 32 texture entries), which BindImage fills.
        for tb in &cached.images {
            let unit = (tb.handle_index.wrapping_sub(8) / 2) as i32 - 32;
            let tex = st.images_bound.get(&(STAGE_COMPUTE, unit)).copied().and_then(|handle| {
                let (tex_addr, view) = crate::nvn::api::lookup_pool_texture((handle & 0xFFFFF) as i32)?;
                let t = self.texture(&mut st.tc, tex_addr)?;
                if !t.usage().contains(MTLTextureUsage::ShaderWrite) {
                    warn_once(format!("kimg-usage-{:x}", tex_addr), format!("image {:#x} bound for {} was not created with NVN_TEXTURE_FLAGS_IMAGE", tex_addr, cached.entry));
                    return None;
                }
                texture_for_binding(&mut st.tc.view_cache, &t, view.as_ref(), MTLTextureType(tb.declared as _), [2, 3, 4, 5])
            });
            let Some(tex) = tex else {
                warn_once(
                    format!("kimg-{}-{}", cached.entry, unit),
                    format!(
                        "compute {} image unit {} has no usable image bound",
                        cached.entry, unit
                    ),
                );
                continue;
            };
            st.tc.mark_gpu_written(super::storage_id(&tex)); // contents now differ from guest memory
            if want_notes {
                notes.push(format!(
                    "img b{} unit {} -> {:?} {}x{}x{} {:?}",
                    tb.binding,
                    unit,
                    tex.textureType(),
                    tex.width(),
                    tex.height(),
                    tex.depth(),
                    tex.pixelFormat()
                ));
            }
            st.issuer
                .push(super::issuer::Op::CTex(tex, tb.binding as usize));
        }
        if super::tracing_frame() {
            println!(
                "[ftrace] DISPATCH {} groups {}x{}x{} local {}x{}x{} | {}",
                cached.entry,
                x,
                y,
                z,
                lx,
                ly,
                lz,
                notes.join(" | ")
            );
        }
        st.issuer.push(super::issuer::Op::CDispatch(
            MTLSize {
                width: x as usize,
                height: y as usize,
                depth: z as usize,
            },
            MTLSize {
                width: lx as usize,
                height: ly as usize,
                depth: lz as usize,
            },
        ));
        st.issuer.push(super::issuer::Op::CLinger);
        st.stats.passes += 1;
    }
}
