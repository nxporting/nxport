// Planning a translated draw: what it binds and which pipeline it needs, and its uniforms.

use super::*;

impl Gfx {
    pub(crate) unsafe fn draw_translated(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        mode: i32,
        call: DrawKind,
    ) -> bool {
        // --log-phases: planning time of draws from lists recorded in earlier frames vs this frame's.
        if crate::settings::get().log.log_phases {
            let t0 = std::time::Instant::now();
            let old = st.in_old_list;
            let r = self.draw_translated_inner(st, cb, mode, call);
            let k = if old { 0 } else { 1 };
            super::PHASES[k * 2].fetch_add(
                t0.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
            super::PHASES[k * 2 + 1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return r;
        }
        self.draw_translated_inner(st, cb, mode, call)
    }

    pub(crate) unsafe fn draw_translated_inner(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        mode: i32,
        call: DrawKind,
    ) -> bool {
        if encode_on_worker(st) {
            let mut pkt = get_packet();
            return match self.plan_draw(st, cb, mode, call, &mut pkt) {
                Ok(()) if pkt.tiny.is_some() => {
                    // Compute, outside any render pass: through the issuer queue, in order.
                    st.issuer.push(super::issuer::Op::DrawJob(pkt));
                    true
                }
                Ok(()) => {
                    if super::draw_reuse_log() {
                        note_draw_reuse(st, &pkt);
                    }
                    let Some(enc) = st.enc.clone() else {
                        return false;
                    };
                    enc.push(super::issuer::Op::DrawJob(pkt));
                    // Counted as drawn (the worker may still skip it): only the pass-split cadence depends on it.
                    st.enc_draws += 1;
                    st.stats.passes += 1;
                    let n = super::split_draws();
                    if n > 0 && st.enc_draws >= n {
                        self.end_encoder(st);
                        st.force_split = true;
                    }
                    true
                }
                Err(r) => {
                    pkt.release();
                    recycle_packet(pkt);
                    r
                }
            };
        }
        let mut pkt = st.draw_packet.take().unwrap_or_default();
        let r = match self.plan_draw(st, cb, mode, call, &mut pkt) {
            Ok(()) => {
                let enc = if pkt.tiny.is_some() {
                    Some(st.issuer.clone())
                } else {
                    st.enc.clone()
                };
                let Some(enc) = enc.map(super::issuer::Sink::Queue) else {
                    return false;
                };
                let shm = st.sh.clone();
                let mut shg = shm.lock().unwrap();
                let mut env = EncodeEnv {
                    cx: &mut st.cx,
                    sh: &mut shg,
                    lb: &mut st.lb,
                    enc,
                    frame_rts: &st.frame_rts,
                    enc_draws: st.enc_draws,
                    drew: false,
                };
                let r = self.encode_draw(&mut env, &pkt);
                let _ = env.drew;
                drop(env);
                drop(shg);
                // Every planned draw counts toward the pass split, as on the worker path (which cannot know whether
                // encode_draw will skip one): pass boundaries, which in-place feedback depends on, stay the same either way.
                {
                    // Long passes are cut so the GPU can start on them (split_draws): an ordering decision, kept here.
                    st.enc_draws += 1;
                    st.stats.passes += 1;
                    let n = super::split_draws();
                    if n > 0 && st.enc_draws >= n {
                        self.end_encoder(st);
                        st.force_split = true;
                    }
                }
                r
            }
            Err(r) => r,
        };
        pkt.release();
        st.draw_packet = Some(pkt);
        r
    }

    /// First half of a translated draw, in command order: shader lookup and skips, render targets, the feedback decision,
    /// the render pass. Err(consumed) ends the draw here (see draw_translated's return value).
    pub(crate) unsafe fn plan_draw(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        mode: i32,
        call: DrawKind,
        pkt: &mut DrawPacket,
    ) -> Result<(), bool> {
        let Some(_lib) = &self.shader_lib else {
            {
                if super::tracing_frame() {
                    println!("[ftrace] SKIP draw at translated.rs:873");
                }
                return Err(false);
            }
        };
        let Some(program) = st.program else {
            {
                if super::tracing_frame() {
                    println!("[ftrace] SKIP draw at translated.rs:874");
                }
                return Err(false);
            }
        };
        let Some(stages) = super::programs::stages(program) else {
            {
                if super::tracing_frame() {
                    println!("[ftrace] SKIP draw at translated.rs:875");
                }
                return Err(false);
            }
        };
        let vs_code = stages
            .iter()
            .find(|s| s.cached.map_or(false, |c| c.kind == StageKind::Vertex));
        let fs_code = stages
            .iter()
            .find(|s| s.cached.map_or(false, |c| c.kind == StageKind::Fragment));
        let Some(vs_code) = vs_code else {
            warn_once(format!("novs-{:x}", program), format!("program 0x{:x} has no translated vertex stage (shader cache miss?): its draws are skipped", program));
            return Err(false);
        };
        // Any stage we have no translation for (geometry, tessellation, or a failed one): the draw is skipped.
        if stages.iter().any(|s| s.cached.is_none()) {
            let missing: Vec<String> = stages
                .iter()
                .enumerate()
                .filter(|(_, s)| s.cached.is_none())
                .map(|(i, s)| {
                    // SPH word 0 sits at +0x30; bits 10..14 are the shader stage (Ryujinx ShaderHeader).
                    let (weak, kind) = if s.data >= 0x1_0000_0000 {
                        let b = std::slice::from_raw_parts(s.data as *const u8, 0x130);
                        let w0 = u32::from_le_bytes([b[0x30], b[0x31], b[0x32], b[0x33]]);
                        (crate::gpu::metal::shader_cache::fnv1a64(b), (w0 >> 10) & 0xF)
                    } else {
                        (0, 99)
                    };
                    format!(
                        "stage#{} data=0x{:x} weak={:016x} sph_stage={}",
                        i, s.data, weak, kind
                    )
                })
                .collect();
            if super::tracing_frame() {
                println!(
                    "[ftrace] FALLBACK program 0x{:x}: {}",
                    program,
                    missing.join("; ")
                );
            }
            super::journal(|| format!("fallback program {:#x}", program));
            warn_once(
                format!("fallback-{:x}", program),
                format!(
                    "program 0x{:x}: draws skipped ({} of {} stages untranslated): {}",
                    program,
                    missing.len(),
                    stages.len(),
                    missing.join("; ")
                ),
            );
            {
                if super::tracing_frame() {
                    println!("[ftrace] SKIP draw at translated.rs:904");
                }
                return Err(false);
            };
        }
        let vs = vs_code.cached.unwrap();
        let fs = fs_code.and_then(|s| s.cached);
        super::trace_trigger_check(&[vs.entry.as_str(), fs.map_or("", |f| f.entry.as_str())]);
        // Debug: --skip-shader=s_<hash>,... drops every draw whose vertex or fragment entry is listed.
        static SKIP: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        let skip = SKIP.get_or_init(|| crate::settings::get().debug.skip_shader.clone());
        if !skip.is_empty()
            && (skip.contains(&vs.entry) || fs.is_some_and(|f| skip.contains(&f.entry)))
        {
            return Err(true);
        }
        // --replay-ab-skip=s_a,...: replay side B drops those draws (same-frame GPU cost of a shader group).
        static SKIP_B: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        let skip_b = SKIP_B.get_or_init(|| crate::settings::get().replay.replay_ab_skip.clone());
        if !skip_b.is_empty()
            && super::replay_b()
            && (skip_b.iter().any(|x| x == "all")
                || skip_b.contains(&vs.entry)
                || fs.is_some_and(|f| skip_b.contains(&f.entry)))
        {
            return Err(true);
        }

        let prim = match mode {
            0 => MTLPrimitiveType::Point,
            1 => MTLPrimitiveType::Line,
            3 => MTLPrimitiveType::LineStrip,
            4 => MTLPrimitiveType::Triangle,
            5 => MTLPrimitiveType::TriangleStrip,
            _ => {
                if !st.warned_prims.contains(&mode) {
                    st.warned_prims.push(mode);
                    println!(
                        "[!] Gfx: primitive mode {} is not supported by the translated path",
                        mode
                    );
                }
                {
                    if super::tracing_frame() {
                        println!("[ftrace] SKIP draw at translated.rs:920");
                    }
                    return Err(true);
                }; // consumed: drawing it with the wrong topology would be worse than skipping
            }
        };

        // ---- render targets ----
        // The pass's resolved targets are reused while it stays open and SetRenderTargets did not run.
        // Shared (Arc), not copied: the targets, formats and pass key were ~600 bytes and a retain per texture per draw.
        let (rts, targetless): (std::sync::Arc<super::CachedPassRts>, Option<(usize, usize)>) =
            if !st.rts_dirty && st.enc.is_some() && st.cached_rts.is_some() {
                (st.cached_rts.clone().unwrap(), None)
            } else {
                // Inline storage for the per-draw temporaries: heap allocations were ~6% of the encoding thread.
                let mut color_texs: smallvec::SmallVec<[(usize, Tex, RtRef); 8]> =
                    smallvec::SmallVec::new();
                let mut bc: smallvec::SmallVec<[(Tex, Tex, usize); 1]> =
                    smallvec::SmallVec::new();
                let ncolors = st.colors.len().min(8);
                for i in 0..ncolors {
                    let rt = &st.colors[i].clone();
                    if rt.texture == 0 {
                        continue;
                    }
                    match self.texture(&mut st.tc, rt.texture) {
                        // GPU texture compression: the game renders compressed blocks through an integer view of a BCn texture.
                        Some(t) if rt.view_format != 0 && self.bc_proxy(st, &t, rt).is_some() => {
                            let p = self.bc_proxy(st, &t, rt).unwrap();
                            // Registered for resolve_bc_proxies when the pass opens: the previous pass ends after this
                            // point and would copy the proxy before anything was drawn into it.
                            bc.push((p.clone(), t.clone(), rt.level as usize));
                            // The proxy is that one level (single-mip texture; resolve_bc_proxies copies it into rt.level of the
                            // target): attach its level 0. Attaching rt.level was out of range (Metal validation abort; undefined
                            // without validation, so GPU-compressed mips above 0 were never rendered correctly).
                            let mut proxy_rt = *rt;
                            proxy_rt.level = 0;
                            color_texs.push((i, p, proxy_rt))
                        }
                        Some(t) => {
                            let t = self.rt_format_view(st, &t, rt).unwrap_or(t);
                            color_texs.push((i, t, *rt))
                        }
                        None => {
                            if super::tracing_frame() {
                                println!("[ftrace] SKIP draw at translated.rs:932");
                            }
                            return Err(false);
                        }
                    }
                }
                let depth_tex = match st.depth {
                    Some(d) if d.texture != 0 => {
                        self.texture(&mut st.tc, d.texture).map(|t| (t, d))
                    }
                    _ => None,
                };
                // No render target bound: TotK draws occlusion-query boxes this way (ResetCounter, draw, ReportCounter with
                // SetRenderTargets(0 colors, no depth)); every rasterized sample counts. Metal does this with an attachment-less
                // pass of explicit size; the viewport (or scissor) gives the size. Skipping these draws reported 0 samples, and
                // the game culled whatever the queries guarded.
                let targetless: Option<(usize, usize)> =
                    if color_texs.is_empty() && depth_tex.is_none() {
                        let size = st
                            .viewport
                            .map(|(x, y, w, h)| {
                                (
                                    (x.max(0) + w.max(0)) as usize,
                                    (y.max(0) + h.max(0)) as usize,
                                )
                            })
                            .or_else(|| {
                                st.scissor.map(|(x, y, w, h)| {
                                    (
                                        (x.max(0) + w.max(0)) as usize,
                                        (y.max(0) + h.max(0)) as usize,
                                    )
                                })
                            })
                            .filter(|&(w, h)| w > 0 && h > 0)
                            .map(|(w, h)| (w.min(16384), h.min(16384)));
                        if size.is_none() {
                            if super::tracing_frame() {
                                println!(
                                    "[ftrace] SKIP draw: no render target and no viewport {} / {}",
                                    vs.entry,
                                    fs.map_or("-", |f| &f.entry)
                                );
                            }
                            return Err(false);
                        }
                        size
                    } else {
                        None
                    };
                // Metal rejects (and aborts on) render targets that are compressed or lack the RenderTarget usage; skip such
                // draws loudly rather than crash. Seen when the game's target list names a data texture. A level or layer the
                // Metal texture does not have aborts the same way. Layers are render target slices: cube faces count (a cube
                // has arrayLength 1 and six slices; TotK renders its environment cubemaps face by face).
                for (i, t, rt) in color_texs
                    .iter()
                    .map(|(i, t, rt)| (*i, t, rt))
                    .chain(depth_tex.iter().map(|(t, d)| (8, t, d)))
                {
                    let layers = match t.textureType() {
                        MTLTextureType::Type3D => t.depth(),
                        MTLTextureType::TypeCube => 6,
                        MTLTextureType::TypeCubeArray => 6 * t.arrayLength(),
                        _ => t.arrayLength(),
                    };
                    let in_range =
                        (rt.level as usize) < t.mipmapLevelCount() && (rt.layer as usize) < layers;
                    let ok = in_range
                        && if i == 8 {
                            t.usage().contains(MTLTextureUsage::RenderTarget)
                                && matches!(
                                    t.pixelFormat(),
                                    MTLPixelFormat::Depth16Unorm
                                        | MTLPixelFormat::Depth32Float
                                        | MTLPixelFormat::Depth32Float_Stencil8
                                        | MTLPixelFormat::Stencil8
                                )
                        } else {
                            is_color_renderable(t)
                        };
                    if !ok {
                        warn_once(format!("rt-unrenderable-{:x}", rt.texture), format!(
                        "draw skipped: render target {} (texture 0x{:x}, format {:?}, type {:?}, level {}/{}, layer {}/{}, view format {} target {}) is not renderable in Metal",
                        i, rt.texture, t.pixelFormat(), t.textureType(), rt.level, t.mipmapLevelCount(), rt.layer, layers, rt.view_format, rt.view_target
                    ));
                        {
                            if super::tracing_frame() {
                                println!("[ftrace] SKIP draw at translated.rs:956");
                            }
                            return Err(true);
                        };
                    }
                }

                let mut color_pfs = [MTLPixelFormat::Invalid; 8];
                for (idx, t, _) in &color_texs {
                    if *idx < 8 {
                        color_pfs[*idx] = t.pixelFormat();
                    }
                }
                let depth_pf = depth_tex
                    .as_ref()
                    .map(|(t, _)| t.pixelFormat())
                    .unwrap_or(MTLPixelFormat::Invalid);
                let nvn_format = |r: &RtRef| unsafe { super::tex_info(r.texture) }.map_or(0, |(_, t)| t.format as u32);
                let mut nvn_colors = [(0u32, 0u32); 8];
                for (idx, _, rt) in &color_texs {
                    if *idx < 8 {
                        nvn_colors[*idx] = (nvn_format(rt), rt.view_format as u32);
                    }
                }
                let nvn_depth = depth_tex.as_ref().map_or((0, 0), |(_, d)| (nvn_format(d), d.view_format as u32));
                let samples = color_texs
                    .first()
                    .map(|(_, t, _)| t.sampleCount())
                    .or_else(|| depth_tex.as_ref().map(|(t, _)| t.sampleCount()))
                    .unwrap_or(1) as u32;

                let tex_id = |t: &Tex| Retained::as_ptr(t) as *const () as u64;
                let enc_key: smallvec::SmallVec<[u64; 36]> = color_texs
                    .iter()
                    .flat_map(|(i, t, rt)| [*i as u64, tex_id(t), rt.level as u64, rt.layer as u64])
                    .chain(
                        depth_tex
                            .iter()
                            .flat_map(|(t, d)| [999u64, tex_id(t), d.level as u64, d.layer as u64]),
                    )
                    .chain(
                        targetless
                            .iter()
                            .flat_map(|&(w, h)| [0x7A46_0000u64, w as u64, h as u64]),
                    )
                    .collect();
                let target_size = color_texs
                    .first()
                    .map(|(_, t, _)| (t.width() as f64, t.height() as f64))
                    .or_else(|| {
                        depth_tex
                            .as_ref()
                            .map(|(t, _)| (t.width() as f64, t.height() as f64))
                    })
                    .or_else(|| targetless.map(|(w, h)| (w as f64, h as f64)))
                    .unwrap_or((0.0, 0.0));
                let scales: smallvec::SmallVec<[u32; 9]> = color_texs
                    .iter()
                    .map(|(_, t, _)| super::tex_scale(t))
                    .chain(depth_tex.iter().map(|(t, _)| super::tex_scale(t)))
                    .collect();
                let scale = crate::gpu::scale::pass_scale(&scales);

                (
                    std::sync::Arc::new(super::CachedPassRts {
                        color_texs,
                        depth_tex,
                        color_pfs,
                        depth_pf,
                        nvn_colors,
                        nvn_depth,
                        samples,
                        enc_key,
                        target_size,
                        scale,
                        bc,
                    }),
                    targetless,
                )
            };
        let color_texs = &rts.color_texs;
        let depth_tex = &rts.depth_tex;
        let (color_pfs, depth_pf, target_size) = (rts.color_pfs, rts.depth_pf, rts.target_size);
        let target_scale = rts.scale;
        let nvn_targets = (rts.nvn_colors, rts.nvn_depth, rts.samples);
        let enc_key = &rts.enc_key;

        if is_denied(&vs.entry) || fs.map_or(false, |f| is_denied(&f.entry)) {
            if super::tracing_frame() {
                let rts: Vec<String> = color_texs
                    .iter()
                    .map(|(i, t, _)| {
                        format!("c{}={}x{} {:?}", i, t.width(), t.height(), t.pixelFormat())
                    })
                    .collect();
                println!(
                    "[ftrace] DENIED {} / {} [{}]",
                    vs.entry,
                    fs.map_or("-", |f| &f.entry),
                    rts.join(", ")
                );
            }
            return Err(true); // known to hang the GPU (see `denylist`)
        }
        // --gfx-draw-mask=N/M runs only the shader pairs whose stable hash % M == N (the rest are skipped), so a GPU
        // hang can be bisected over shader pairs by halving M / choosing N. Also --gfx-draw-mask=N/M,N2/M2 for a union.
        if let Some(masks) = draw_masks() {
            let h = crate::gpu::metal::shader_cache::fnv1a64(
                format!("{}/{}", vs.entry, fs.map_or("-", |f| &f.entry)).as_bytes(),
            );
            if !masks.iter().any(|m| h % m.every == m.first) {
                {
                    if super::tracing_frame() {
                        println!("[ftrace] SKIP draw at translated.rs:972");
                    }
                    return Err(true);
                };
            }
        }
        // Passes that render into slices of a 3D texture, a cube map face, or an array layer (shadow maps, noise volumes,
        // environment cube maps) were once suspected of GPU errors; validated runs are clean since the GPU safety work, and
        // without them every shadow test fails and the scene is unlit.

        // ---- render-target feedback ----
        // A draw that samples an attachment of a pass begun before the latest INVALIDATE_TEXTURE barrier ends that pass
        // (in place mode, see feedback_snapshots); only then (or in copy mode) are its textures resolved here, in
        // order. Otherwise encode_draw resolves them.
        // Textures are resolved here, in order, for every draw: the texture cache then stays on this thread (the encode half
        // may run on a worker), and memo hits make this cheap. The feedback check only runs for a pass begun before the
        // latest barrier (in place mode) or in copy mode.
        // Consecutive draws often bind the same textures (shadow casters, grass, one material's meshes): reuse the previous
        // draw's resolution while the shaders, every handle and the epochs that validate binding memos are unchanged (the
        // frame too: the first resolution in a frame runs the per-frame CPU-write checks). --no-tex-reuse: off.
        let mut key = TexReuseKey {
            vs: vs as *const CachedStage as usize,
            fs: fs.map_or(0, |f| f as *const CachedStage as usize),
            handles: smallvec::SmallVec::new(),
            epochs: [
                crate::nvn::api::tex_table_epoch(),
                super::tex_epoch(),
                crate::nvn::api::texture_dirty_gen(),
                super::presented_frames(),
            ],
        };
        for (nvn_stage, cached) in [(STAGE_VERTEX, Some(vs)), (STAGE_FRAGMENT, fs)] {
            let Some(cached) = cached else { continue };
            for tb in &cached.textures {
                let k = tb.handle_index.wrapping_sub(8) / 2;
                key.handles
                    .push(st.textures_bound.get(&(nvn_stage, k as i32)).copied());
            }
        }
        let reuse = st.tex_reuse.as_ref().is_some_and(|(k, _)| *k == key);
        let mut resolved: ResolvedTex = smallvec::SmallVec::new();
        if reuse {
            resolved = st.tex_reuse.as_ref().unwrap().1.clone();
            TEX_REUSE_STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            let twins_before = st.tc.twin_uses;
            for (nvn_stage, cached) in [(STAGE_VERTEX, Some(vs)), (STAGE_FRAGMENT, fs)] {
                let Some(cached) = cached else { continue };
                for tb in &cached.textures {
                    let (t, samp, root) = self.bound_texture_root(st, nvn_stage, tb);
                    resolved.push((nvn_stage, tb.binding, t, samp, root));
                }
            }
            TEX_REUSE_STATS[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Resolution may itself bump the epochs (it creates or re-points textures): key the cache by their values now.
            key.epochs = [
                crate::nvn::api::tex_table_epoch(),
                super::tex_epoch(),
                crate::nvn::api::texture_dirty_gen(),
                super::presented_frames(),
            ];
            // A depth twin of a GPU-written texture may need a copy before a later draw: resolve those every time.
            st.tex_reuse = (st.tc.twin_uses == twins_before).then(|| (key, resolved.clone()));
        }
        if !st.tc.twin_pending.is_empty() {
            self.refresh_depth_twins(st);
        }
        if !st.tc.downscale_pending.is_empty() {
            self.run_downscales(st);
        }
        // A draw whose samples of its pass's colour targets are all at its own pixel reads them from the tile with a
        // variant of its fragment stage (tile_feedback.rs): its pass stays open.
        let mut fs = fs;
        let mut tile = false;
        if super::tile_feedback::active() && st.enc.is_some() && st.enc_fb_epoch != st.fb_epoch && !y_negate() {
            if let (Some(f), Some(fc)) = (fs, fs_code) {
                let depth = depth_tex.as_ref().map(|(t, _)| t);
                if let Some(map) = self.tile_map(st, &color_texs, depth, &resolved) {
                    let v = super::tile_feedback::variant(vs, (vs_code.data, vs_code.control), f, (fc.data, fc.control), &map);
                    if let Some(v) = v {
                        fs = Some(v);
                        tile = true;
                        super::tile_feedback::STATS[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        }
        if !tile && st.enc.is_some() && st.enc_fb_epoch != st.fb_epoch {
            self.feedback_snapshots(
                st,
                &color_texs,
                depth_tex.as_ref().map(|(t, _)| t),
                &mut resolved,
            );
        }

        // Tiny passes as compute: no render pass for this draw.
        if let Some(tp) =
            self.tiny_plan(st, vs, fs, &call, mode, &color_texs, &depth_tex, &resolved)
        {
            pkt.fill(
                st,
                mode,
                prim,
                call,
                vs,
                fs,
                vs_code,
                fs_code,
                Some(resolved),
                target_size,
                target_scale,
                color_pfs,
                depth_pf,
                nvn_targets,
            );
            self.plan_uniforms(st, pkt);
            pkt.tiny = Some(tp);
            return Ok(());
        }

        // ---- encoder ----
        // Consecutive draws to the same targets share one render pass. A pass per draw loads and stores every attachment
        // each time; at hundreds of draws per 1080p frame that is gigabytes of memory traffic and a hot GPU.
        // Pending clears force a new pass: those on this pass's targets become its load actions, the rest are encoded
        // first (a draw may sample a cleared texture).
        let clear_needed = !st.pending_clears.is_empty()
            && (color_texs.iter().any(|(_, t, rt)| {
                    st.pending_clears
                        .iter()
                        .any(|c| c.is_target(t, rt.level as usize, rt.layer as usize))
                })
                || depth_tex.as_ref().map_or(false, |(t, d)| {
                    st.pending_clears
                        .iter()
                        .any(|c| c.is_target(t, d.level as usize, d.layer as usize))
                })
                || resolved.iter().any(|(_, _, _, _, r)| {
                    st.pending_clears.iter().any(|c| *r == Some(c.root as u64))
                }));
        if st.enc.is_none() || st.enc_key != *enc_key || clear_needed {
            if super::enc_stats_enabled() {
                super::ENC_OPENED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if st.enc.is_none() && st.enc_key == *enc_key {
                    super::ENC_REOPEN_SAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            self.end_encoder_keep_clears(st);
            // Whether this pass's targets were drawn in the previous frame too (State::enc_recur), from the last two
            // frames each began a pass in; pruned once a frame to what that test reads.
            let f = super::presented_frames() + 1;
            if st.rt_frames_pruned != f {
                st.rt_frames_pruned = f;
                st.rt_frames.retain(|_, v| v.0 + 1 >= f);
            }
            let mut recur = false;
            for t in color_texs.iter().map(|(_, t, _)| t).chain(depth_tex.as_ref().map(|(t, _)| t)) {
                let e = st.rt_frames.entry(super::storage_id(t)).or_insert((0, 0));
                if e.0 != f {
                    *e = (f, e.0);
                }
                recur |= e.1 != 0 && e.1 + 1 == f;
            }
            st.enc_recur = recur;
            let mut take_clear =
                |t: &Tex, level: usize, layer: usize, color: bool| -> Option<super::ClearKind> {
                    let i = st.pending_clears.iter().position(|c| {
                        c.is_target(t, level, layer)
                            && matches!(c.kind, super::ClearKind::Color(_)) == color
                    })?;
                    super::CLEARS_FOLDED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Some(st.pending_clears.remove(i).kind)
                };
            let color_clears: Vec<Option<super::ClearKind>> = color_texs
                .iter()
                .map(|(_, t, rt)| take_clear(t, rt.level as usize, rt.layer as usize, true))
                .collect();
            let depth_clear = depth_tex
                .as_ref()
                .and_then(|(t, d)| take_clear(t, d.level as usize, d.layer as usize, false));
            // Clears still pending on memory this pass renders into run first: the pass's attachment may be another
            // view of the cleared texture (is_target compares the exact object), and a clear flushed after the pass
            // erased what it drew (Mario Kart 8 Deluxe's shadow mask, cleared through one object and drawn through
            // another: the track sampled zeros and rendered black).
            let attached: smallvec::SmallVec<[u64; 9]> = color_texs
                .iter()
                .map(|(_, t, _)| root_id(t))
                .chain(depth_tex.iter().map(|(t, _)| root_id(t)))
                .collect();
            self.flush_clears_matching(st, |c| {
                attached.contains(&c.root)
                    || resolved
                        .iter()
                        .any(|(_, _, _, _, r)| *r == Some(c.root as u64))
            });
            let pass = MTLRenderPassDescriptor::renderPassDescriptor();
            pass.setVisibilityResultBuffer(st.occl.buffer(&self.device).as_deref());
            if let Some((w, h)) = targetless {
                pass.setRenderTargetWidth(w);
                pass.setRenderTargetHeight(h);
                pass.setDefaultRasterSampleCount(1);
            }
            for ((i, t, rt), clear) in color_texs.iter().zip(&color_clears) {
                let att = pass.colorAttachments().objectAtIndexedSubscript(*i);
                att.setTexture(Some(t));
                att.setLevel(rt.level as usize);
                if t.textureType() == MTLTextureType::Type3D {
                    att.setDepthPlane(rt.layer as usize);
                } else {
                    att.setSlice(rt.layer as usize);
                }
                if let Some(super::ClearKind::Color(c)) = clear {
                    att.setLoadAction(MTLLoadAction::Clear);
                    att.setClearColor(MTLClearColor {
                        red: c[0] as f64,
                        green: c[1] as f64,
                        blue: c[2] as f64,
                        alpha: c[3] as f64,
                    });
                } else {
                    att.setLoadAction(MTLLoadAction::Load);
                }
                att.setStoreAction(MTLStoreAction::Store);
            }
            let (clear_depth, clear_stencil) = match depth_clear {
                Some(super::ClearKind::DepthStencil { depth, stencil }) => (depth, stencil),
                _ => (None, None),
            };
            if let Some((t, d)) = &depth_tex {
                let att = pass.depthAttachment();
                att.setTexture(Some(t));
                att.setLevel(d.level as usize);
                att.setSlice(d.layer as usize);
                att.setLoadAction(if clear_depth.is_some() {
                    MTLLoadAction::Clear
                } else {
                    MTLLoadAction::Load
                });
                att.setClearDepth(super::clear_depth_value(t.pixelFormat(), clear_depth.unwrap_or(0.0)));
                att.setStoreAction(MTLStoreAction::Store);
                if matches!(
                    depth_pf,
                    MTLPixelFormat::Depth32Float_Stencil8 | MTLPixelFormat::Stencil8
                ) {
                    let satt = pass.stencilAttachment();
                    satt.setTexture(Some(t));
                    satt.setLevel(d.level as usize);
                    satt.setSlice(d.layer as usize);
                    satt.setClearStencil(clear_stencil.unwrap_or(0));
                    satt.setLoadAction(if clear_stencil.is_some() {
                        MTLLoadAction::Clear
                    } else {
                        MTLLoadAction::Load
                    });
                    satt.setStoreAction(MTLStoreAction::Store);
                }
            }
            let _ = cb;
            // Label f<frame>e<encoder index>: Metal's validation reports name the encoder by label.
            let label = format!(
                "f{}e{}",
                super::presented_frames() + 1,
                st.lb.enc_labels.len()
            );
            // The Metal label also names the pass shape (attachment index:size:pixel format) and its first shaders, so
            // GPU profiles can be grouped by pass. One string per encoder, also used for error reports.
            use std::fmt::Write as _;
            // The attachment shapes cost three Objective-C queries per attachment plus formatting for every encoder
            // (~3% of the Presentation Thread): only with --enc-labels (GPU profiles grouped by pass) or
            // --draw-details. The shader pair is always named (hang and validation reports).
            let mut shape_label = String::with_capacity(if full_labels() { 160 } else { 48 });
            if full_labels() {
                shape_label.push('[');
                for (i, t, _) in color_texs.iter() {
                    let _ = write!(
                        shape_label,
                        "c{}:{}x{}:{},",
                        i,
                        t.width(),
                        t.height(),
                        t.pixelFormat().0
                    );
                }
                if let Some((t, _)) = &depth_tex {
                    let _ = write!(
                        shape_label,
                        "d:{}x{}:{}",
                        t.width(),
                        t.height(),
                        t.pixelFormat().0
                    );
                }
                shape_label.push_str("] ");
            }
            let _ = write!(shape_label, "{} {}", vs.entry, fs.map_or("-", |f| &f.entry));
            // Created on the issuer thread (see metal/issuer.rs), in order with the draws.
            let Some(cur_cb) = st.cur_cb.clone() else {
                return Err(false);
            };
            st.issuer.push(Op::Begin {
                cb: cur_cb,
                desc: pass.clone(),
                label: format!("{} {}", label, shape_label),
            });
            if super::journal_on() {
                for (_, t, rt) in color_texs.iter() {
                    note_frame_rt(
                        unsafe { super::tex_info(rt.texture) }.map_or(0, |(_, x)| x.storage),
                        t,
                    );
                }
                if let Some((t, d)) = &depth_tex {
                    note_frame_rt(
                        unsafe { super::tex_info(d.texture) }.map_or(0, |(_, x)| x.storage),
                        t,
                    );
                }
                let mut l = format!(
                    "pass {} {} {}:",
                    label,
                    vs.entry,
                    fs.map_or("-", |f| &f.entry)
                );
                for ((i, t, rt), clear) in color_texs.iter().zip(&color_clears) {
                    let info = unsafe { super::tex_info(rt.texture) };
                    let storage = info.as_ref().map_or(0, |(_, x)| x.storage);
                    let _ = write!(
                        l,
                        " c{}={:x}@{:x}:{}x{}:l{}s{}:{}",
                        i,
                        super::tex_id(t),
                        storage,
                        t.width(),
                        t.height(),
                        rt.level,
                        rt.layer,
                        if clear.is_some() { "C" } else { "L" }
                    );
                    if let Some((gen, x)) = &info {
                        let _ = write!(
                            l,
                            "[nvn {:x} gen {} {}x{} fmt {} vfmt {} flags {:#x} lv {}]",
                            rt.texture, gen, x.width, x.height, x.format, rt.view_format, x.flags, x.levels
                        );
                    }
                }
                if let Some((t, d)) = &depth_tex {
                    let info = unsafe { super::tex_info(d.texture) };
                    let storage = info.as_ref().map_or(0, |(_, x)| x.storage);
                    let _ = write!(
                        l,
                        " d={:x}@{:x}:{}x{}:l{}s{}:{}",
                        super::tex_id(t),
                        storage,
                        t.width(),
                        t.height(),
                        d.level,
                        d.layer,
                        if clear_depth.is_some() { "C" } else { "L" }
                    );
                    if let Some((gen, x)) = &info {
                        let _ = write!(
                            l,
                            "[nvn {:x} gen {} {}x{} fmt {} flags {:#x} lv {}]",
                            d.texture, gen, x.width, x.height, x.format, x.flags, x.levels
                        );
                    }
                }
                if targetless.is_some() {
                    l.push_str(" targetless");
                }
                super::journal(|| l);
            }
            st.enc_name = label;
            st.enc_targets = color_texs.iter().map(|(i, t, _)| (*i, t.clone())).collect();
            st.enc_depth = depth_tex
                .as_ref()
                .map(|(t, d)| (t.clone(), d.layer as usize, d.level as usize));
            st.enc_draws = 0;
            st.enc = Some(st.issuer.clone());
            for (p, t, level) in &rts.bc {
                if !st
                    .bc_pending
                    .iter()
                    .any(|(q, _, _)| Retained::as_ptr(q) == Retained::as_ptr(p))
                {
                    st.bc_pending.push((p.clone(), t.clone(), *level));
                }
            }
            st.enc_fb_epoch = st.fb_epoch;
            st.cx.ec = EncCache::default();
            // --no-enc-cache: every draw sets all state (A/B).
            st.enc_key = enc_key.clone();
            self.note_encoder(st, format!("translated pass {}", shape_label));
            // Every draw of the pass has these targets (the pass key compares them): marked once per pass.
            for (_, t, _) in color_texs.iter() {
                st.tc.mark_gpu_written(super::storage_id(t));
            }
            if let Some((t, _)) = &depth_tex {
                st.tc.mark_gpu_written(super::storage_id(t));
            }
        }
        if st.cached_rts.is_none() && targetless.is_none() && st.enc.is_some() {
            st.cached_rts = Some(rts.clone());
            st.rts_dirty = false;
        }
        // Per-frame render target list: only for traces and frame dumps (root_id walks Objective-C parent chains).
        if super::want_frame_rts() {
            // Color and depth targets (depth too, so a later sample of a depth target names its producer in traces).
            let mut ids: smallvec::SmallVec<[(char, usize, usize); 9]> = smallvec::SmallVec::new();
            for (k, t) in color_texs
                .iter()
                .map(|(i, t, _)| (*i, t))
                .chain(depth_tex.as_ref().map(|(t, _)| (99, t)))
            {
                let id = root_id(t);
                let pos = match st.frame_rts.iter().position(|(i, ..)| *i == id) {
                    Some(p) => {
                        st.frame_rts[p].2 += 1;
                        p
                    }
                    None => {
                        st.frame_rts.push((id, t.clone(), 1));
                        st.frame_rts.len() - 1
                    }
                };
                ids.push(if k == 99 {
                    ('d', 0, pos)
                } else {
                    ('c', k, pos)
                });
            }
            if super::tracing_frame() {
                let l: Vec<String> = ids
                    .iter()
                    .map(|&(c, k, p)| {
                        if c == 'd' {
                            format!("d=rt{:02}", p)
                        } else {
                            format!("c{}=rt{:02}", k, p)
                        }
                    })
                    .collect();
                println!(
                    "[ftrace] TARGETS {} caller main+{:#x}",
                    l.join(" "),
                    st.cur_caller
                );
            }
        }
        if st.occl.counting {
            st.occl.draws_counted += 1;
        }
        self.occl_before_draw(st);

        // The encoder label collects the shader pairs drawn in it (hang / error reports name the encoder); pairs already
        // named are skipped without building the string.
        let pair_id = (
            vs.entry.as_ptr() as usize,
            fs.map_or(0, |f| f.entry.as_ptr() as usize),
        );
        let new_pair = st.lb.enc_labels.last().map_or(false, |l| l.len() < 400)
            && !st.lb.enc_label_pairs.contains(&pair_id);
        if new_pair || draw_details() || super::tracing_frame() {
            st.lb.enc_label_pairs.push(pair_id);
            let mut pair = String::with_capacity(vs.entry.len() + 24);
            pair.push_str(&vs.entry);
            pair.push_str(" / ");
            pair.push_str(fs.map_or("-", |f| &f.entry));
            // A pair new to this encoder is not in its label yet; only the debug paths can repeat one (note_draw checks).
            if new_pair {
                if let Some(l) = st.lb.enc_labels.last_mut() {
                    l.push_str(" | ");
                    l.push_str(&pair);
                }
            } else {
                self.note_draw(&mut st.lb, &pair);
            }
        }
        let resolved_tex = Some(resolved);
        pkt.fill(
            st,
            mode,
            prim,
            call,
            vs,
            fs,
            vs_code,
            fs_code,
            resolved_tex,
            target_size,
            target_scale,
            color_pfs,
            depth_pf,
            nvn_targets,
        );
        self.plan_uniforms(st, pkt);
        Ok(())
    }

    /// Uniform buffer decisions that depend on command order (the encode half may run later, after the plan has
    /// applied this submit's inline uniform updates and counter reports to guest memory, or marked more memory as
    /// GPU-written): zero-copy for GPU-written memory, a copy made now for memory an executor write of this submit
    /// overlaps (st.ubo_updated covers every such range), and the storage buffer descriptors shaders read from uniform
    /// buffers.
    pub(crate) unsafe fn plan_uniforms(&self, st: &mut State, pkt: &mut DrawPacket) {
        pkt.ubo_pre.clear();
        pkt.sb_desc.clear();
        let Some(vs) = pkt.vs else { return };
        if crate::settings::get().log.log_ubo_content {
            ubo_content_probe(pkt, vs);
        }
        let epoch = super::GUEST_WRITES.load(std::sync::atomic::Ordering::Relaxed);
        if st.ubo_epoch_plan != epoch {
            st.plan_ubo.clear();
            st.ubo_epoch_plan = epoch;
            st.ubo_plan_epoch += 1;
        }
        for (si, (stage, cached)) in [(STAGE_VERTEX, Some(vs)), (STAGE_FRAGMENT, pkt.fs)]
            .into_iter()
            .enumerate()
        {
            let Some(cached) = cached else { continue };
            for &(binding, slot) in &cached.cbuffers {
                if slot < 3 {
                    continue;
                }
                let pre = match pkt.ubo(stage, slot as i32 - 3) {
                    Some((addr, size)) if addr > 0x10000 && size > 0 => {
                        let len = cached.cbuf_read_len(binding, CBUF_DECL_SIZE);
                        let n = size.min(len).max(size.min(16));
                        let pad = cbuf_zero_pad(len, size);
                        let slot_i = binding as usize;
                        let cached_pre = {
                            st.ubo_decisions
                                .get(si)
                                .and_then(|a| a.get(slot_i))
                                .and_then(|d| d.as_ref())
                                .filter(|d| {
                                    d.addr == addr
                                        && d.size == size
                                        && d.len == n
                                        && d.epoch == st.ubo_plan_epoch
                                })
                                .map(|d| d.pre.clone())
                        };
                        match cached_pre {
                            Some(pre) => pre,
                            None => {
                                let zc = crate::settings::get().graphics.ubo_zero_copy;
                                // Zero-copy only where the stage reads nothing past the bound size (those reads must
                                // return 0: a padded copy).
                                let pre = if gpu_written(&st.gpu_dirty, addr, size)
                                    || (zc
                                        && pad == 0
                                        && !super::ubo_updated(
                                            &st.ubo_updated,
                                            addr as u64,
                                            size.min(CBUF_DECL_SIZE) as u64,
                                        )) {
                                    UboPre::ZeroCopy
                                } else if super::ubo_updated(&st.ubo_updated, addr as u64, n as u64)
                                {
                                    let key = (addr, n | pad << 32);
                                    let hit = st.plan_ubo.get(&key).cloned();
                                    let copy = hit.or_else(|| {
                                        let held = super::rt_guard::guest_copy::<u8>(addr as usize, n);
                                        let bytes = held.as_deref().unwrap_or_else(|| {
                                            std::slice::from_raw_parts(addr as *const u8, n)
                                        });
                                        let f = st.plan_arena.alloc_with(
                                            &self.device,
                                            n + pad,
                                            |b| {
                                                b[..n].copy_from_slice(bytes);
                                                b[n..].fill(0);
                                            },
                                        )?;
                                        st.plan_ubo.insert(key, f.clone(), true);
                                        Some(f)
                                    });
                                    match copy {
                                        Some((b, o)) => UboPre::Snapshot(b, o),
                                        None => UboPre::Copy,
                                    }
                                } else {
                                    UboPre::Copy
                                };
                                if let Some(e) =
                                    st.ubo_decisions.get_mut(si).and_then(|a| a.get_mut(slot_i))
                                {
                                    *e = Some(UboDecision {
                                        addr,
                                        size,
                                        len: n,
                                        epoch: st.ubo_plan_epoch,
                                        pre: pre.clone(),
                                    });
                                }
                                pre
                            }
                        }
                    }
                    _ => UboPre::Copy,
                };
                pkt.ubo_pre.push((stage, binding, pre));
            }
            for &(binding, _slot, cb_slot, cb_off) in &cached.sbuffers {
                if cb_slot < 3 {
                    continue;
                }
                let byte = cb_off as usize * 4;
                let desc = pkt.ubo(stage, cb_slot as i32 - 3).and_then(|(ua, usz)| {
                    (ua > 0x10000 && byte + 12 <= usz).then(|| {
                        let a = std::ptr::read_unaligned((ua as usize + byte) as *const u64);
                        (a, user_sb_size(a))
                    })
                });
                pkt.sb_desc.push((stage, binding, desc));
            }
        }
    }
}
