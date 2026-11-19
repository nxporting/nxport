// The executor's encoders: opening, splitting and ending passes, deferred and folded clears, BC proxies resolved at
// pass ends, encoder target snapshots (--snapshot-enc).

use super::*;

impl Gfx {
    /// Ends the render encoder kept open for consecutive draws, if any. Everything that needs the command buffer for
    /// something else (clears, copies, other passes, commit) calls this first.
    /// Records what the next encoder is for. Call once per encoder creation, in creation order.
    pub(super) fn note_encoder(&self, st: &mut State, label: impl Into<String>) {
        st.lb.enc_labels.push(label.into());
        st.lb.enc_label_pairs.clear();
        st.lb.enc_details.push(Vec::new());
    }

    pub(super) fn note_detail(&self, lb: &mut EncLabels, detail: String) {
        if let Some(d) = lb.enc_details.last_mut() {
            if d.len() < 16 {
                d.push(detail);
            }
        }
    }

    /// Adds a draw's shader pair to the current encoder's label (a shared encoder covers many draws).
    pub(super) fn note_draw(&self, lb: &mut EncLabels, what: &str) {
        if let Some(l) = lb.enc_labels.last_mut() {
            if l.len() < 400 && !l.contains(what) {
                l.push_str(" | ");
                l.push_str(what);
            }
        }
    }

    /// Commits the command buffer being encoded once it holds `split_encoders()` encoders and continues in a new one, so
    /// the GPU starts on the first passes of a frame while the rest is still being encoded. TotK waits for each frame's
    /// GPU work right after submitting it; encoding everything before committing kept the GPU idle during encoding and
    /// the CPU idle during execution (frame time = encode + GPU).
    /// Runs before every command. With a render encoder open only force_split can split: the time, report and count
    /// rules wait for a pass boundary.
    #[inline(always)]
    pub(super) fn maybe_split(&self, st: &mut State) -> bool {
        if st.enc.is_some() && !st.force_split {
            return false;
        }
        self.maybe_split_slow(st)
    }

    #[inline(never)]
    pub(super) fn maybe_split_slow(&self, st: &mut State) -> bool {
        let k = split_encoders();
        // Also by time, between passes: encoders differ widely in encode time (a run of shadow-map passes took 3.2 ms to
        // encode without reaching 24 encoders while the GPU idled; from there the GPU was the bottleneck to the frame's
        // end), so the GPU gets work every split_ms of encoding without cutting a pass.
        let ms = split_ms();
        let by_time = ms > 0.0
            && st.enc.is_none()
            && !st.lb.enc_labels.is_empty()
            && st.pending_clears.is_empty()
            && st
                .cb_started
                .is_some_and(|t| t.elapsed().as_secs_f64() * 1e3 >= ms);
        // The encoder-count rule also waits for a pass boundary: cutting an open pass to commit stored and reloaded all its
        // attachments (~6 times per Hateno frame). --split-cuts-passes: cut as before (A/B).
        let by_count = k > 0 && st.lb.enc_labels.len() >= k && st.enc.is_none();
        // --report-guard: game threads wait for the occlusion reports of this submit (metal/report_guard.rs), so the command
        // buffer holding them is committed at the first pass boundary after them (on the Switch the GPU writes each
        // report as it reaches it; ours land when the command buffer completes).
        let by_reports = report_guard::mode() == crate::settings::ReportGuard::On
            && !st.occl.pending.is_empty()
            && st.enc.is_none()
            && st.pending_clears.is_empty();
        if !st.force_split && !by_time && !by_count && !by_reports || gfx_mode() == GfxMode::Dry {
            return false;
        }
        st.force_split = false;
        self.end_encoder(st);
        let Some(old) = st.cur_cb.take() else {
            return false;
        };
        tail_log_label(st, &old);
        let serial = push_inflight(st, old.clone());
        st.issuer.commit_serial(old, Some(serial)); // after the queued encoder ops, on the issuer thread (metal/issuer.rs)
        let desc = MTLCommandBufferDescriptor::new();
        desc.setErrorOptions(MTLCommandBufferErrorOption::EncoderExecutionStatus);
        st.cur_cb = self.queue.commandBufferWithDescriptor(&desc);
        st.cb_started = Some(std::time::Instant::now());
        true
    }

    /// Ends the current render encoder and encodes pending clears.
    #[track_caller]
    pub(super) fn end_encoder(&self, st: &mut State) {
        self.end_encoder_keep_clears(st);
        self.flush_clears(st);
    }

    /// Encodes the pending clears as their own passes (in order), before whatever the caller encodes next.
    #[track_caller]
    pub(super) fn flush_clears(&self, st: &mut State) {
        self.flush_clears_matching(st, |_| true);
    }

    /// Encodes pending clears that match `pred` as their own passes (in order), leaving unmatched clears deferred.
    #[track_caller]
    pub(super) fn flush_clears_matching(
        &self,
        st: &mut State,
        mut pred: impl FnMut(&PendingClear) -> bool,
    ) {
        if st.pending_clears.is_empty() {
            return;
        }
        let Some(cb) = st.cur_cb.clone() else {
            st.pending_clears.retain(|c| !pred(c));
            return;
        };
        // The matching clears, in order. Pending clears have distinct storage roots (defer_clear flushes before a second
        // view of the same storage), so their order does not matter: clears of equal size share one pass (up to 8 colour
        // attachments and one depth / stencil target; each render pass costs ~30 us of GPU time on Apple GPUs).
        // --no-clear-merge: one pass per clear (--replay-ab-clear: on replay side B).
        let mut taken: Vec<PendingClear> = Vec::new();
        let mut i = 0;
        while i < st.pending_clears.len() {
            if pred(&st.pending_clears[i]) {
                taken.push(st.pending_clears.remove(i));
            } else {
                i += 1;
            }
        }
        if taken.is_empty() {
            return;
        }
        if tracing_frame() {
            let l = std::panic::Location::caller();
            println!(
                "[ftrace] CLEAR FLUSH of {} at {}:{}",
                taken.len(),
                l.file(),
                l.line()
            );
        }
        self.end_encoder_keep_clears(st);
        let dims = |c: &PendingClear| {
            (
                (c.tex.width() >> c.level).max(1),
                (c.tex.height() >> c.level).max(1),
                c.tex.sampleCount(),
            )
        };
        let mut groups: Vec<(usize, usize, usize, Vec<PendingClear>)> = Vec::new();
        for c in taken {
            let (w, h, n) = dims(&c);
            let is_color = matches!(c.kind, ClearKind::Color(_));
            let slot = {
                groups
                    .iter()
                    .rposition(|g| (g.0, g.1, g.2) == (w, h, n))
                    .filter(|&k| {
                        let g = &groups[k].3;
                        if is_color {
                            g.iter()
                                .filter(|x| matches!(x.kind, ClearKind::Color(_)))
                                .count()
                                < 8
                        } else {
                            !g.iter().any(|x| !matches!(x.kind, ClearKind::Color(_)))
                        }
                    })
            };
            match slot {
                Some(k) => groups[k].3.push(c),
                None => groups.push((w, h, n, vec![c])),
            }
        }
        for (_, _, _, group) in groups {
            let pass = MTLRenderPassDescriptor::renderPassDescriptor();
            let mut ncolor = 0;
            let mut has_color = false;
            for c in &group {
                journal(|| {
                    format!(
                        "clear {:x} level {} layer {} {}",
                        tex_id(&c.tex),
                        c.level,
                        c.layer,
                        if matches!(c.kind, ClearKind::Color(_)) {
                            "color"
                        } else {
                            "depth"
                        }
                    )
                });
                if tracing_frame() {
                    println!(
                        "[ftrace] CLEAR {:x} {}x{} {:?} level {} layer {} {} (pass of {})",
                        tex_id(&c.tex),
                        c.tex.width() >> c.level,
                        c.tex.height() >> c.level,
                        c.tex.pixelFormat(),
                        c.level,
                        c.layer,
                        if matches!(c.kind, ClearKind::Color(_)) {
                            "color"
                        } else {
                            "depth"
                        },
                        group.len()
                    );
                }
                match c.kind {
                    ClearKind::Color(rgba) => {
                        has_color = true;
                        let att =
                            unsafe { pass.colorAttachments().objectAtIndexedSubscript(ncolor) };
                        ncolor += 1;
                        att.setTexture(Some(&c.tex));
                        att.setLevel(c.level);
                        if c.tex.textureType() == MTLTextureType::Type3D {
                            att.setDepthPlane(c.layer);
                        } else {
                            att.setSlice(c.layer);
                        }
                        att.setLoadAction(MTLLoadAction::Clear);
                        att.setStoreAction(MTLStoreAction::Store);
                        att.setClearColor(MTLClearColor {
                            red: rgba[0] as f64,
                            green: rgba[1] as f64,
                            blue: rgba[2] as f64,
                            alpha: rgba[3] as f64,
                        });
                    }
                    ClearKind::DepthStencil { depth, stencil } => {
                        let pf = c.tex.pixelFormat();
                        if !matches!(pf, MTLPixelFormat::Stencil8) {
                            let d = pass.depthAttachment();
                            d.setTexture(Some(&c.tex));
                            d.setLevel(c.level);
                            d.setSlice(c.layer);
                            d.setLoadAction(if depth.is_some() {
                                MTLLoadAction::Clear
                            } else {
                                MTLLoadAction::Load
                            });
                            d.setStoreAction(MTLStoreAction::Store);
                            d.setClearDepth(clear_depth_value(pf, depth.unwrap_or(0.0)));
                        }
                        if matches!(
                            pf,
                            MTLPixelFormat::Depth32Float_Stencil8 | MTLPixelFormat::Stencil8
                        ) {
                            let s = pass.stencilAttachment();
                            s.setTexture(Some(&c.tex));
                            s.setLevel(c.level);
                            s.setSlice(c.layer);
                            s.setLoadAction(if stencil.is_some() {
                                MTLLoadAction::Clear
                            } else {
                                MTLLoadAction::Load
                            });
                            s.setStoreAction(MTLStoreAction::Store);
                            s.setClearStencil(stencil.unwrap_or(0));
                        }
                    }
                }
                CLEARS_FLUSHED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            CLEAR_PASSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Queued like any pass (an issuer sync here was ~18 waits per Hateno frame).
            let label = if has_color { "clear" } else { "clear_depth" };
            st.issuer.push(issuer::Op::Begin {
                cb: cb.clone(),
                desc: pass,
                label: label.to_string(),
            });
            st.issuer.push(issuer::Op::End);
            self.note_encoder(st, label);
            st.stats.passes += 1;
        }
    }

    /// Records a clear to be folded into the next render pass on that exact target (load action Clear), or encoded as
    /// its own pass by `flush_clears` before anything else. Saves a pass and a full load/store of the target.
    pub(super) fn defer_clear(&self, st: &mut State, c: PendingClear) {
        let same = |p: &PendingClear| {
            p.target_eq(&c)
                && matches!(p.kind, ClearKind::Color(_)) == matches!(c.kind, ClearKind::Color(_))
        };
        if let Some(i) = st.pending_clears.iter().position(same) {
            // A later clear of the same target replaces the earlier one (depth and stencil merge).
            let old = st.pending_clears.remove(i);
            let kind = match (old.kind, c.kind) {
                (
                    ClearKind::DepthStencil {
                        depth: d0,
                        stencil: s0,
                    },
                    ClearKind::DepthStencil { depth, stencil },
                ) => ClearKind::DepthStencil {
                    depth: depth.or(d0),
                    stencil: stencil.or(s0),
                },
                (_, k) => k,
            };
            st.pending_clears.push(PendingClear { kind, ..c });
        } else {
            // Another view of the same storage: keep the two clears in order.
            if st.pending_clears.iter().any(|p| p.root == c.root) {
                self.flush_clears(st);
            }
            st.pending_clears.push(c);
        }
    }

    #[track_caller]
    pub(super) fn end_encoder_keep_clears(&self, st: &mut State) {
        if st.enc.is_some() && enc_stats_enabled() {
            let loc = std::panic::Location::caller();
            *ENC_END_SITES
                .lock()
                .unwrap()
                .entry((loc.file(), loc.line()))
                .or_insert(0) += 1;
        }
        if st.enc.is_some() && pass_graph::on() {
            let loc = std::panic::Location::caller();
            println!("[pg]   end by {}:{} after {} draws", loc.file().rsplit('/').next().unwrap_or(""), loc.line(), st.enc_draws);
        }
        if let Some(e) = st.enc.take() {
            rt_guard::pass_end();
            st.rts_dirty = true;
            st.cached_rts = None;
            journal(|| format!("  end {} draws {}", st.enc_name, st.enc_draws));
            // The next encoder counts into a fresh slot: a pass rewrites the visibility slots it used when it ends.
            st.occl.cur = None;
            e.push(issuer::Op::End);
            self.snapshot_encoder_targets(st);
            self.resolve_bc_proxies(st);
        }
    }

    /// Copies proxies rendered in the encoder that just ended into their block-compressed targets. Each proxy texel is
    /// one compressed block, so the bytes go through a buffer unchanged (Metal cannot view BCn data as integers).
    pub(super) fn resolve_bc_proxies(&self, st: &mut State) {
        use objc2_metal::MTLBlitCommandEncoder;
        if st.bc_pending.is_empty() {
            return;
        }
        let Some(cb) = st.cur_cb.clone() else {
            st.bc_pending.clear();
            return;
        };
        st.issuer.sync(); // other command-buffer use: queued encoder ops first (metal/issuer.rs)
        let Some(blit) = cb.blitCommandEncoder() else {
            st.bc_pending.clear();
            return;
        };
        objc2_metal::MTLCommandEncoder::setLabel(
            &*blit,
            Some(&objc2_foundation::NSString::from_str("bc proxy resolve")),
        );
        for (proxy, target, level) in std::mem::take(&mut st.bc_pending) {
            if pass_graph::on() {
                pass_graph::blit("bc proxy", Some(&proxy), Some(&target));
            }
            let (bw, bh) = (proxy.width(), proxy.height());
            let bpb = match proxy.pixelFormat() {
                MTLPixelFormat::RGBA32Uint | MTLPixelFormat::RGBA32Sint => 16,
                MTLPixelFormat::R32Uint
                | MTLPixelFormat::R32Sint
                | MTLPixelFormat::RG16Uint
                | MTLPixelFormat::RGBA8Uint => 4,
                _ => 8,
            };
            let is3d = proxy.textureType() == MTLTextureType::Type3D;
            let (depth, slices) = if is3d {
                (proxy.depth(), 1)
            } else {
                (1, proxy.arrayLength().max(1))
            };
            let row = bw * bpb;
            let image = row * bh;
            let Some(buf) = self.device.newBufferWithLength_options(
                image * depth * slices,
                MTLResourceOptions::StorageModePrivate,
            ) else {
                continue;
            };
            let (tw, th) = (
                (target.width() >> level).max(1),
                (target.height() >> level).max(1),
            );
            unsafe {
                for s in 0..slices {
                    let off = s * image * depth;
                    blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                        &proxy, s, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: bw, height: bh, depth }, &buf, off, row, image,
                    );
                    blit.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                        &buf, off, row, image, MTLSize { width: tw, height: th, depth }, &target, s, level, MTLOrigin { x: 0, y: 0, z: 0 },
                    );
                }
            }
            st.tc.mark_gpu_written(storage_id(&target));
        }
        blit.endEncoding();
    }

    /// --snapshot-enc=f900e23,f900e99: copy the color targets of those encoders as they stand when the encoder
    /// ends, so one frame's intermediate states can be inspected (dumped with --dump-frame of that frame).
    pub(super) fn snapshot_encoder_targets(&self, st: &mut State) {
        use objc2_metal::MTLBlitCommandEncoder;
        static LIST: OnceLock<Vec<String>> = OnceLock::new();
        let list = LIST.get_or_init(|| crate::settings::get().dump.snapshot_enc.clone());
        // Entries are labels or label prefixes ending in '*'; --snapshot-size=WxH[,WxH...] limits which targets are copied.
        // "trigger": the frames from the --trace-on-shader trigger on (--snapshot-trigger-frames).
        let trig = trace_triggered_frame();
        let cur = presented_frames() + 1;
        let window = crate::settings::get().dump.snapshot_trigger_frames;
        let by_trigger =
            list.iter().any(|e| e == "trigger") && trig != 0 && cur >= trig && cur < trig + window;
        // F9 capture: every encoder of the captured frame, G-buffer albedo only (1280x720 RGBA8 sRGB) unless
        // --snapshot-size / --snapshot-fmt say otherwise, to find the pass that draws a glitch.
        let by_capture = CAPTURE_REQUESTED.load(std::sync::atomic::Ordering::Relaxed)
            && trig != 0
            && cur == trig;
        if !by_trigger
            && !by_capture
            && !list.iter().any(|e| {
                e.strip_suffix('*')
                    .map_or(*e == st.enc_name, |p| st.enc_name.starts_with(p))
            })
        {
            return;
        }
        let size_filter = crate::settings::get()
            .dump
            .snapshot_size
            .or(by_capture.then_some(crate::settings::Size { w: 1280, h: 720 }));
        let fmt_filter = crate::settings::get()
            .dump
            .snapshot_fmt
            .or(by_capture.then_some(MTLPixelFormat::RGBA8Unorm_sRGB.0 as usize));
        let Some(cb) = st.cur_cb.clone() else { return };
        st.issuer.sync(); // other command-buffer use: queued encoder ops first (metal/issuer.rs)
        let Some(blit) = cb.blitCommandEncoder() else {
            return;
        };
        objc2_metal::MTLCommandEncoder::setLabel(
            &*blit,
            Some(&objc2_foundation::NSString::from_str("debug snapshot")),
        );
        // --snapshot-depth: also copy the depth attachment's slice (depth textures are private: through a buffer).
        if crate::settings::get().dump.snapshot_depth {
            if let Some((t, slice, level)) = st.enc_depth.clone() {
                let bpp = if t.pixelFormat() == MTLPixelFormat::Depth16Unorm {
                    2
                } else {
                    4
                };
                let (w, h) = ((t.width() >> level).max(1), (t.height() >> level).max(1));
                if matches!(
                    t.pixelFormat(),
                    MTLPixelFormat::Depth16Unorm | MTLPixelFormat::Depth32Float
                ) {
                    if let Some(buf) = self.device.newBufferWithLength_options(
                        w * h * bpp,
                        MTLResourceOptions::StorageModeShared,
                    ) {
                        unsafe {
                            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                                &t, slice, level, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: w, height: h, depth: 1 }, &buf, 0, w * bpp, w * h * bpp,
                            );
                        }
                        st.depth_snapshots.push((
                            format!("{}_depth_s{}", st.enc_name, slice),
                            buf,
                            w,
                            h,
                            bpp,
                        ));
                        println!("[snapshot] {} depth target: type {:?} {}x{} slices {} levels {} (drawing slice {} level {}), is view: {}",
                            st.enc_name, t.textureType(), t.width(), t.height(), t.arrayLength(), t.mipmapLevelCount(), slice, level, t.parentTexture().is_some());
                    }
                }
            }
        }
        for (i, t) in st.enc_targets.clone() {
            if size_filter.is_some_and(|f| !f.is(t.width(), t.height())) {
                continue;
            }
            // --snapshot-fmt=<MTLPixelFormat raw value> limits snapshots to that format.
            if fmt_filter.is_some_and(|f| f != t.pixelFormat().0 as usize) {
                continue;
            }
            let desc = MTLTextureDescriptor::new();
            desc.setPixelFormat(t.pixelFormat());
            unsafe {
                desc.setWidth(t.width());
                desc.setHeight(t.height());
            }
            desc.setUsage(MTLTextureUsage::ShaderRead);
            desc.setStorageMode(MTLStorageMode::Shared);
            let Some(copy) = self.device.newTextureWithDescriptor(&desc) else {
                continue;
            };
            let size = MTLSize {
                width: t.width(),
                height: t.height(),
                depth: 1,
            };
            unsafe {
                blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                    &t, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 }, size, &copy, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 },
                );
            }
            // Sequence number first: encoder indices restart after each mid-frame commit, so names repeat in a frame.
            let seq = st.snapshots.len();
            st.snapshots
                .push((format!("{:03}_{}_c{}", seq, st.enc_name, i), copy));
        }
        blit.endEncoding();
    }

    pub(super) unsafe fn clear(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        index: i32,
        rgba: [f32; 4],
        mask: i32,
    ) {
        if index < 0 || index as usize >= st.colors.len() {
            return;
        }
        let rt = st.colors[index as usize];
        let Some(tex) = self.texture(&mut st.tc, rt.texture) else {
            return;
        };
        if tracing_frame() {
            let idx = st
                .frame_rts
                .iter()
                .position(|(_, t, _)| translated::root_id(t) == translated::root_id(&tex));
            println!(
                "[ftrace] CLEAR c{} rt{:?} {}x{} {:?} rgba={:?}",
                index,
                idx,
                tex.width(),
                tex.height(),
                tex.pixelFormat(),
                rgba
            );
        }
        st.tc.mark_gpu_written(storage_id(&tex));
        if !is_color_renderable(&tex) {
            return; // e.g. a compressed or depth texture named as a color target; Metal would abort
        }
        // Clears are scissored (TotK clears its Depths lit-area mask in 50x50 tiles, each followed by that tile's
        // drawing) and channel-masked: those are drawn over the scissor rectangle, in order with everything else.
        // Whole-level clears of all channels stay deferred, to be folded into the next pass.
        let level = rt.level as usize;
        let (lw, lh) = ((tex.width() >> level).max(1), (tex.height() >> level).max(1));
        // The scissor is in game pixels; a scaled target (gpu::scale) has k times as many.
        let k = tex_scale(&tex) as i32;
        let (sx, sy, sw, sh) = st
            .scissor
            .map(|(x, y, w, h)| (x * k, y * k, w * k, h * k))
            .unwrap_or((0, 0, lw as i32, lh as i32));
        let scissored = sx > 0 || sy > 0 || (sx + sw) < lw as i32 || (sy + sh) < lh as i32;
        if scissored || mask & 0xF != 0xF {
            if mask & 0xF == 0 || sw <= 0 || sh <= 0 || sx >= lw as i32 || sy >= lh as i32 {
                return;
            }
            self.end_encoder(st);
            let (x0, y0) = (sx.max(0), sy.max(0));
            let region = crate::nvn::cmd::Region {
                x: x0,
                y: y0,
                z: rt.layer as i32,
                w: (sx + sw).min(lw as i32) - x0,
                h: (sy + sh).min(lh as i32) - y0,
                d: 1,
            };
            let dst = crate::nvn::cmd::TexRef { texture: rt.texture, gen: rt.gen, level: rt.level, layer: rt.layer };
            self.clear_region(st, cb, &tex, &dst, &region, &rgba.map(f32::to_bits), mask, level, (lw, lh));
            return;
        }
        let root = cached_root_id(st, &tex);
        if st
            .enc_targets
                .iter()
                .any(|(_, t)| translated::root_id(t) == root)
        {
            self.end_encoder_keep_clears(st);
        }
        self.defer_clear(
            st,
            PendingClear {
                tex,
                root,
                level: rt.level as usize,
                layer: rt.layer as usize,
                kind: ClearKind::Color(rgba),
            },
        );
    }

    pub(super) unsafe fn clear_depth(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        depth: f32,
        depth_mask: bool,
        stencil: i32,
        stencil_mask: i32,
    ) {
        let Some(rt) = st.depth else { return };
        let Some(tex) = self.texture(&mut st.tc, rt.texture) else {
            return;
        };
        st.tc.mark_gpu_written(storage_id(&tex));
        if !tex.usage().contains(MTLTextureUsage::RenderTarget) {
            return;
        }
        let Some((_, snap)) = tex_info(rt.texture) else {
            return;
        };
        let (pf, _) = metal_format(snap.format);
        let has_stencil = matches!(
            pf,
            MTLPixelFormat::Depth32Float_Stencil8 | MTLPixelFormat::Stencil8
        );
        let has_depth = !matches!(pf, MTLPixelFormat::Stencil8);
        // The view's mip level and array layer: shadow cascades live in layers and levels of one depth array, and a
        // clear of layer 0 / level 0 only left the others at their initial 0.0, so their LESS-tested casters never drew.
        let (level, slice) = (rt.level as usize, rt.layer as usize);
        if tracing_frame() {
            println!("[ftrace] CLEAR DEPTH nvn {:#x} -> mtl {:?} {:?} {}x{} arr{} lvls{} parent={} | level {} slice {} | depth {} mask {} stencil {} smask {}",
                rt.texture, tex.textureType(), tex.pixelFormat(), tex.width(), tex.height(), tex.arrayLength(), tex.mipmapLevelCount(), tex.parentTexture().is_some(),
                level, slice, depth, depth_mask, stencil, stencil_mask);
        }
        let depth = (has_depth && depth_mask).then_some(depth);
        let stencil = (has_stencil && stencil_mask != 0).then_some(stencil as u32);
        if depth.is_none() && stencil.is_none() {
            return;
        }
        let _ = cb;
        let root = cached_root_id(st, &tex);
        if st
            .enc_depth
                .as_ref()
                .map_or(false, |(t, _, _)| translated::root_id(t) == root)
        {
            self.end_encoder_keep_clears(st);
        }
        self.defer_clear(
            st,
            PendingClear {
                tex,
                root,
                level,
                layer: slice,
                kind: ClearKind::DepthStencil { depth, stencil },
            },
        );
    }
}
