// Encoding a planned draw into its render encoder.

use super::*;

impl Gfx {
    /// Second half of a translated draw: everything that only needs the draw's packet and the caches (vertex buffers,
    /// pipeline, resources, the draw itself). Reads no NVN state from `State`: the packet holds what the draw uses.
    pub(crate) unsafe fn encode_draw(&self, env: &mut EncodeEnv, p: &DrawPacket) -> bool {
        let _ph_t0 = if super::phases_on() {
            Some(std::time::Instant::now())
        } else {
            None
        };
        let Some(lib) = &self.shader_lib else {
            return false;
        };
        let Some(vs) = p.vs else { return false };
        let (mode, prim, call, fs) = (p.mode, p.prim, p.call, p.fs);
        let vs_code = &p.vs_code;
        let fs_code = p.fs_code.as_ref();
        // Vertex layout and pipeline: a pure function of the shaders, attribute / stream state, target formats and blend
        // state, memoized per thread (decoding the attributes, the repack layout and the ~60-word pipeline key were about
        // a third of this function). Inputs are compared in full; --no-layout-memo derives them for every draw.
        let color_pfs = p.color_pfs;
        let depth_pf = p.depth_pf;
        let depth_gl = crate::nvn::api::depth_mode() == 0;
        let blends: [u64; 8] = p
            .blend_states
            .map(|b| if b == 0 { DEFAULT_BLEND_STATE } else { b });
        let blend = blends[0];
        // Points take the command buffer's point size (only point draws, so other draws share one pipeline).
        let point_size = if mode == 0 { p.point_size } else { 1.0 };
        // Argument-buffer draws: both stages bind their resources through one struct each (their ab_ variants).
        let use_ab = args_on()
            && p.tiny.is_none()
            && !super::batch::enabled()
            && vs.args.is_some()
            && fs.map_or(true, |f| f.args.is_some());
        let mut memo_key: smallvec::SmallVec<[u64; 64]> = smallvec::smallvec![
            point_size.to_bits() as u64,
            vs as *const CachedStage as u64,
            fs.map_or(0, |f| f as *const CachedStage as u64),
            depth_gl as u64,
            p.color_state as u64,
            p.channel_mask as u64,
            depth_pf.0 as u64,
        ];
        memo_key.extend(color_pfs.iter().map(|p| p.0 as u64));
        memo_key.extend(blends);
        memo_key.push(p.attr_bits.len() as u64);
        memo_key.extend(p.attr_bits.iter().map(|b| b.map_or(u64::MAX, |b| b as u64)));
        memo_key.extend(p.vertex_streams.iter().copied());
        memo_key.push(use_ab as u64);
        // Consecutive draws mostly repeat the previous draw's inputs: compare with those before hashing.
        let last_hit = env
            .cx
            .layout_last
            .as_ref()
            .filter(|(k, _)| k.as_slice() == memo_key.as_slice())
            .map(|(_, e)| e.clone());
        let layout: std::sync::Arc<LayoutMemo> = match last_hit.or_else(|| {
            env.cx.layout_memo.get(memo_key.as_slice()).cloned()
        }) {
            Some(e) => {
                if env
                    .cx
                    .layout_last
                    .as_ref()
                    .map_or(true, |(_, l)| !std::sync::Arc::ptr_eq(l, &e))
                {
                    env.cx.layout_last = Some((memo_key.clone(), e.clone()));
                }
                e
            }
            None => {
                crate::gpu::pipeline_list::precompile(compile_listed);
                let Layout {
                    attr_descs,
                    streams_used,
                    needs_default_attr,
                    streams_eff,
                    repack,
                } = derive_layout(vs, &p.attr_bits, &p.vertex_streams);

                // Debug: --log-shader-draws=s_a,s_b prints the vertex layout of the first draws using those vertex shaders.
                {
                    static LIST: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
                    let list =
                        LIST.get_or_init(|| crate::settings::get().log.log_shader_draws.clone());
                    if list.iter().any(|e| *e == vs.entry) {
                        static N: std::sync::Mutex<Vec<(String, u32)>> =
                            std::sync::Mutex::new(Vec::new());
                        let mut n = N.lock().unwrap();
                        let c = match n.iter_mut().find(|(e, _)| *e == vs.entry) {
                            Some(x) => {
                                x.1 += 1;
                                x.1
                            }
                            None => {
                                n.push((vs.entry.clone(), 1));
                                1
                            }
                        };
                        if c <= 4 {
                            let streams: Vec<String> = (0..MAX_STREAMS)
                                .filter(|&s| streams_used[s])
                                .map(|s| {
                                    format!(
                                        "s{} bits={:#x}->{:#x} vb={:x?}",
                                        s,
                                        p.vertex_streams.get(s).copied().unwrap_or(0),
                                        streams_eff.get(s).copied().unwrap_or(0),
                                        p.vertex_buffers[s]
                                    )
                                })
                                .collect();
                            let raw: Vec<String> = vs
                                .attrs
                                .iter()
                                .map(|&l| {
                                    format!(
                                        "a{}={:?}",
                                        l,
                                        vs.attrs
                                            .iter()
                                            .position(|x| *x == l)
                                            .and_then(|i| p.attr_bits[i])
                                            .map(decode_attr)
                                            .map(|a| (
                                                a.format,
                                                vertex_format(a.format).map(|f| f.0 .0),
                                                a.stream,
                                                a.offset
                                            ))
                                    )
                                })
                                .collect();
                            println!("[shader-draw] {} attrs {:?} raw {:?} streams {:?} repack {:?} call {:?}", vs.entry, attr_descs.iter().map(|a| (a.0, a.1.0, a.2, a.3)).collect::<Vec<_>>(), raw, streams, repack.iter().map(|r| (r.0, r.1)).collect::<Vec<_>>(), call);
                        }
                    }
                }
                let mut key = pipeline_key(
                    point_size,
                    vs,
                    fs,
                    depth_gl,
                    p.color_state,
                    p.channel_mask,
                    depth_pf,
                    &color_pfs,
                    &blends,
                    &attr_descs,
                    &streams_used,
                    &streams_eff,
                );
                if use_ab {
                    key.push(AB_KEY);
                }

                let pipe = if let Some(p) = env.sh.translated_pipelines.get(key.as_slice()) {
                    p.clone()
                } else if env.sh.translated_pipelines_bad.contains(key.as_slice()) {
                    // Building it failed before and would fail again (same shaders and state): don't pay for another compile.
                    env.sh.translated_pipelines_failed += 1;
                    {
                        if super::tracing_frame() {
                            println!("[ftrace] SKIP draw: pipeline failed to build earlier");
                        }
                        return false;
                    };
                } else if async_pipelines() {
                    // Compiled on the pipeline threads; the draw is skipped until it is ready (see async_pipeline).
                    let job = PipeJob {
                        vs: vs.clone(),
                        fs: fs.cloned(),
                        attrs: attr_descs.to_vec(),
                        streams_used,
                        streams: streams_eff.to_vec(),
                        color_pfs: color_pfs.to_vec(),
                        depth_pf,
                        depth_gl,
                        point_size,
                        blends,
                        color_state: p.color_state,
                        channel_mask: p.channel_mask,
                        blend_color: p.blend_color,
                        batched: false,
                        args: use_ab,
                    };
                    let mut r = async_pipeline(key.as_slice(), job);
                    // Still compiling: skipped only where the pass's targets are drawn every frame (the next frame draws
                    // it); a target drawn now and then (an icon, a cloud or fog layer made once) may never be drawn again,
                    // so its draw waits.
                    if matches!(r, AsyncPipe::Pending) && !p.targets_recur {
                        r = async_pipeline_wait(key.as_slice());
                    }
                    match r {
                        AsyncPipe::Ready(pipe) => {
                            env.sh
                                .translated_pipelines
                                .insert(key.to_vec(), pipe.clone());
                            record_pipeline(vs, fs, p, point_size, depth_gl, &blends, false);
                            register_batchable(
                                &pipe,
                                &key,
                                vs,
                                fs,
                                &attr_descs,
                                &streams_used,
                                &streams_eff,
                                &color_pfs,
                                depth_pf,
                                depth_gl,
                                point_size,
                                &blends,
                                p,
                            );
                            pipe
                        }
                        AsyncPipe::Pending => {
                            {
                                crate::gpu::pipeline_list::draw_skipped_compiling();
                                if super::tracing_frame() {
                                    println!("[ftrace] SKIP draw: pipeline still compiling");
                                }
                                return false;
                            };
                        }
                        AsyncPipe::Failed => {
                            env.sh.translated_pipelines_failed += 1;
                            env.sh.translated_pipelines_bad.insert(key.to_vec());
                            {
                                if super::tracing_frame() {
                                    println!("[ftrace] SKIP draw: pipeline failed to build");
                                }
                                return false;
                            };
                        }
                    }
                } else {
                    let built = build_now(key.as_slice(), || {
                        let t0 = std::time::Instant::now();
                        let built = self.build_translated_pipeline(
                            lib,
                            vs,
                            fs,
                            &attr_descs,
                            &streams_used,
                            &streams_eff,
                            &color_pfs,
                            depth_pf,
                            depth_gl,
                            point_size,
                            &blends,
                            p.color_state,
                            p.channel_mask,
                            p.blend_color,
                            false,
                            use_ab,
                        );
                        super::HITCH_PIPE_NS.fetch_add(
                            t0.elapsed().as_nanos() as u64,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        super::HITCH_PIPE_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        built
                    });
                    let Some(pl) = built else {
                        env.sh.translated_pipelines_failed += 1;
                        env.sh.translated_pipelines_bad.insert(key.to_vec());
                        {
                            if super::tracing_frame() {
                                println!("[ftrace] SKIP draw at translated.rs:1035");
                            }
                            return false;
                        };
                    };
                    env.sh.translated_pipelines.insert(key.to_vec(), pl.clone());
                    record_pipeline(vs, fs, p, point_size, depth_gl, &blends, false);
                    register_batchable(
                        &pl,
                        &key,
                        vs,
                        fs,
                        &attr_descs,
                        &streams_used,
                        &streams_eff,
                        &color_pfs,
                        depth_pf,
                        depth_gl,
                        point_size,
                        &blends,
                        p,
                    );
                    pl
                };
                let e = std::sync::Arc::new(LayoutMemo {
                    attr_descs,
                    streams_used,
                    needs_default_attr,
                    streams_eff,
                    repack,
                    pipe,
                });
                if env.cx.layout_memo.len() > 8192 {
                    env.cx.layout_memo.clear();
                }
                env.cx.layout_memo.insert(memo_key.to_vec(), e.clone());
                env.cx.layout_last = Some((memo_key.clone(), e.clone()));
                e
            }
        };
        let (attr_descs, streams_used, needs_default_attr, streams_eff, repack, pipe) = (
            &layout.attr_descs,
            layout.streams_used,
            layout.needs_default_attr,
            &layout.streams_eff,
            &layout.repack,
            &layout.pipe,
        );

        let _ph_t1 = if super::phases_on() {
            Some(std::time::Instant::now())
        } else {
            None
        };
        if let (Some(a), Some(b)) = (_ph_t0, _ph_t1) {
            super::PHASES2[4].fetch_add(
                (b - a).as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        // Textures: resolved by the plan.
        let Some(resolved_tex) = p.resolved_tex.as_ref() else {
            return false;
        };
        let enc = env.enc.clone();
        if p.tiny.is_some() {
            // Compute mode: record this draw's bindings for the kernels (state calls are ignored).
            env.cx.ec = EncCache::default();
            env.cx.ec.rec = Some(TinyRec::default());
        }
        env.cx.ec.pipeline(&enc, &pipe);
        if let Some(b) = debug_vpos_buffer() {
            // Through the cache, so it knows what index 26 holds (offset-only updates rely on that).
            env.cx.ec.vbuf(&enc, &b.0, 0, 26);
            env.cx.ec.fbuf(&enc, &b.0, 0, 26);
        }

        let (tw, th) = p.target_size;
        let (vx, vy, vw, vh) = p
            .viewport
            .map(|(x, y, w, h)| (x as f64, y as f64, w as f64, h as f64))
            .unwrap_or((0.0, 0.0, tw, th));
        env.cx.ec.viewport(
            &enc,
            MTLViewport {
                originX: vx,
                originY: vy,
                width: vw,
                height: vh,
                znear: p.depth_range.0 as f64,
                zfar: p.depth_range.1 as f64,
            },
        );
        {
            let (mw, mh) = (tw as usize, th as usize);
            let (mut sx, mut sy, mut sw, mut sh) =
                p.scissor.unwrap_or((0, 0, mw as i32, mh as i32));
            // --replay-ab-scissor=s_a,...|all: replay side B gives those draws a 1x1 scissor (vertex work and binning
            // stay, fragments go): the same-frame GPU delta is their fragment cost. Measurement only.
            static SC_B: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
            let sc_b = SC_B.get_or_init(|| crate::settings::get().replay.replay_ab_scissor.clone());
            if !sc_b.is_empty()
                && super::replay_b()
                && (sc_b.iter().any(|x| x == "all")
                    || sc_b.contains(&vs.entry)
                    || fs.is_some_and(|f| sc_b.contains(&f.entry)))
            {
                (sx, sy, sw, sh) = (0, 0, 1, 1);
            }
            let x = (sx.max(0) as usize).min(mw.saturating_sub(1));
            let y = (sy.max(0) as usize).min(mh.saturating_sub(1));
            env.cx.ec.scissor(
                &enc,
                MTLScissorRect {
                    x,
                    y,
                    width: (sw.max(1) as usize).min(mw - x),
                    height: (sh.max(1) as usize).min(mh - y),
                },
            );
        }

        // Polygon state: cull[0..2) NVNface, front face[2] (CW=0, CCW=1), mode[3..5) (POINT, LINE, FILL).
        let poly = p.polygon;
        let cull = match poly & 3 {
            1 => MTLCullMode::Front,
            2 => MTLCullMode::Back,
            _ => MTLCullMode::None,
        };
        if poly & 3 == 3 {
            {
                if super::tracing_frame() {
                    println!("[ftrace] SKIP draw at translated.rs:1132");
                }
                return true;
            }; // cull front and back: nothing is rasterized
        }
        // Debug: --no-cull disables face culling everywhere.
        env.cx.ec.cull(&enc, cull);
        // NVN depth clamp (shadow passes use it so casters in front of the cascade's near plane still occlude).
        let clamp = p.depth_clamp;
        env.cx.ec.clip(
            &enc,
            if clamp {
                objc2_metal::MTLDepthClipMode::Clamp
            } else {
                objc2_metal::MTLDepthClipMode::Clip
            },
        );
        // Metal's winding is the opposite of NVN's for the same clip-space triangle; negating y (LOWER_LEFT origin)
        // reverses it once more.
        let ccw_front = (poly >> 2) & 1 == 1;
        env.cx.ec.winding(
            &enc,
            if ccw_front == y_negate() {
                MTLWinding::Clockwise
            } else {
                MTLWinding::CounterClockwise
            },
        );
        env.cx.ec.fill(
            &enc,
            if (poly >> 3) & 3 == 1 {
                MTLTriangleFillMode::Lines
            } else {
                MTLTriangleFillMode::Fill
            },
        );
        // Polygon offset (GL semantics, as NVN: the polygon state enables it per polygon mode, POINT=1 LINE=2 FILL=4;
        // it never applies to point/line primitives). NVN's API-level units map directly to Metal's depthBias (Ryujinx
        // halves the hardware register, which holds 2x the API value). Shadow passes rely on it against acne.
        let is_tri = matches!(
            prim,
            MTLPrimitiveType::Triangle | MTLPrimitiveType::TriangleStrip
        );
        let mode_bit = match (poly >> 3) & 3 {
            0 => 1,
            1 => 2,
            _ => 4,
        };
        let (factor, units, oclamp) = p.polygon_offset;
        let pofs_on = is_tri && (poly >> 5) & mode_bit != 0;
        if pofs_on {
            env.cx.ec.bias(&enc, units, factor, oclamp);
        } else {
            env.cx.ec.bias(&enc, 0.0, 0.0, 0.0);
        }
        // Debug: --debug-no-depth-fs=<fragment entry>[,..] draws those shaders' draws without depth test or write.
        let ds_bits = match fs {
            Some(f)
                if crate::settings::get()
                    .debug
                    .debug_no_depth_fs
                    .iter()
                    .any(|x| *x == f.entry) =>
            {
                p.depth_stencil & !0x3
            }
            _ => p.depth_stencil,
        };
        let has_stencil = matches!(
            depth_pf,
            MTLPixelFormat::Depth32Float_Stencil8 | MTLPixelFormat::Stencil8
        );
        if has_stencil && (ds_bits >> 6) & 1 != 0 {
            env.cx.ec.stencil_ref(&enc, p.stencil[0]);
        }
        if let Some(ds) = self.depth_stencil_state(&mut *env.sh, p.stencil, ds_bits, has_stencil) {
            if depth_pf != MTLPixelFormat::Invalid {
                env.cx.ec.depth_stencil(&enc, &ds);
            }
        }
        let [br, bg, bb, ba] = p.blend_color;
        env.cx.ec.blend(&enc, [br, bg, bb, ba]);

        let _ph_t2 = if super::phases_on() {
            Some(std::time::Instant::now())
        } else {
            None
        };
        if let (Some(a), Some(b)) = (_ph_t1, _ph_t2) {
            super::PHASES2[0].fetch_add(
                (b - a).as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        // ---- vertex buffers ----
        // Highest vertex this draw fetches (index scan), computed at most once and only if some stream needs it.
        let mut exact_cache: Option<Option<u64>> = None;
        for s in 0..MAX_STREAMS {
            if !streams_used[s] {
                continue;
            }
            let Some((addr, size)) = p.vertex_buffers[s] else {
                println!("[!] Gfx: draw needs vertex stream {} but none is bound", s);
                {
                    if super::tracing_frame() {
                        println!("[ftrace] SKIP draw at translated.rs:1155");
                    }
                    return true;
                };
            };
            // NVN bounds-checks vertex fetches against the bound size (out of range reads as zero); Metal reads past
            // it. TotK draws with base vertices beyond the bound buffers (e.g. base vertex 2533 into 408-vertex
            // streams while meshes stream in): Metal fetched other meshes' data and huge triangles streaked across
            // the screen. Such streams get a copy padded with zeros up to the highest vertex the draw fetches.
            let bits = p.vertex_streams.get(s).copied().unwrap_or(0);
            let (stride, divisor) = ((bits & 0xFFFF_FFFF) as u64, bits >> 32);
            if let Some((_, new_stride, copies)) = repack.iter().find(|r| r.0 == s) {
                let last = exact_cache
                    .get_or_insert_with(|| {
                        cached_max_vertex(&mut env.cx.max_index_memo, &call, p.prim_restart)
                    })
                    .unwrap_or(0);
                let n = (last + 1) as usize;
                let ns = *new_stride as usize;
                if n * ns <= (64 << 20) {
                    // Repacked streams are cached by source range and layout (TotK's are mostly static meshes: 7 MB
                    // per frame was re-copied in Hateno). An entry covers the whole stream, so every draw from it
                    // shares one copy, and is validated by a hash of the source bytes once per frame. A changed
                    // stream gets a new buffer: one the GPU may still read is never rewritten.
                    let layout_sig = copies.iter().fold(ns as u64, |h, &(a, b, c)| {
                        (h ^ ((a as u64) << 32 | (b as u64) << 16 | c as u64))
                            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    });
                    let key = (addr as u64, size, stride, layout_sig);
                    let frame = super::presented_frames();
                    let src_hash = |st_size: usize| -> u64 {
                        if addr <= 0x10000 {
                            return 0;
                        }
                        let words =
                            std::slice::from_raw_parts(addr as usize as *const u64, st_size / 8);
                        // Four independent lanes (a single dependency chain ran at ~1 word per multiply latency).
                        let mut lanes = [0x9E37_79B9_7F4A_7C15u64 ^ st_size as u64, 1, 2, 3];
                        let mut chunks = words.chunks_exact(4);
                        for (i, c) in (&mut chunks).enumerate() {
                            for k in 0..4 {
                                lanes[k] = (lanes[k] ^ c[k].wrapping_add(i as u64))
                                    .rotate_left(27)
                                    .wrapping_mul(0x9E37_79B9_7F4A_7C15);
                            }
                        }
                        let mut h = lanes
                            .iter()
                            .fold(0u64, |a, &l| (a ^ l).wrapping_mul(0x100_0000_01B3));
                        for &w in chunks.remainder() {
                            h = (h ^ w).rotate_left(27).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                        }
                        for &b in std::slice::from_raw_parts(
                            (addr as usize + st_size / 8 * 8) as *const u8,
                            st_size % 8,
                        ) {
                            h = (h ^ b as u64).wrapping_mul(0x100_0000_01B3);
                        }
                        h
                    };
                    let mut hit: Option<Retained<ProtocolObject<dyn MTLBuffer>>> = None;
                    if let Some(e) = env.sh.repack_cache.get_mut(&key) {
                        if e.cap >= n && (e.checked == frame || e.src_hash == src_hash(size)) {
                            e.checked = frame;
                            hit = Some(e.buf.clone());
                        }
                    }
                    if hit.is_none() {
                        let cap = n.max(if stride > 0 {
                            (size + stride as usize - 1) / stride as usize
                        } else {
                            0
                        });
                        let fill = |out: &mut [u8]| {
                            out.fill(0);
                            if addr <= 0x10000 {
                                return;
                            }
                            // Fixed-size unaligned moves: a memcpy call per attribute per vertex dominated this loop.
                            let base = addr as usize as *const u8;
                            let o = out.as_mut_ptr();
                            for &(src, dst, len) in copies {
                                let mut from = src;
                                let mut to = dst;
                                for _ in 0..cap {
                                    if from + len > size {
                                        break; // the rest of the stream is past the bound size: zeros, as NVN reads them
                                    }
                                    let (s, d) = (base.add(from), o.add(to));
                                    match len {
                                        4 => (d as *mut u32)
                                            .write_unaligned((s as *const u32).read_unaligned()),
                                        8 => (d as *mut u64)
                                            .write_unaligned((s as *const u64).read_unaligned()),
                                        2 => (d as *mut u16)
                                            .write_unaligned((s as *const u16).read_unaligned()),
                                        16 => (d as *mut u128)
                                            .write_unaligned((s as *const u128).read_unaligned()),
                                        _ => std::ptr::copy_nonoverlapping(s, d, len),
                                    }
                                    from += stride as usize;
                                    to += ns;
                                }
                            }
                        };
                        let bytes = (cap * ns).max(16);
                        if let Some(buf) = self.device.newBufferWithLength_options(
                            bytes + CBUF_DECL_SIZE.min(4096),
                            MTLResourceOptions::empty(),
                        ) {
                            fill(std::slice::from_raw_parts_mut(
                                buf.contents().as_ptr() as *mut u8,
                                cap * ns,
                            ));
                            if env.sh.repack_bytes + bytes > (256 << 20) {
                                env.sh.repack_cache.clear();
                                env.sh.repack_bytes = 0;
                            }
                            if let Some(old) = env.sh.repack_cache.insert(
                                key,
                                super::RepackEntry {
                                    buf: buf.clone(),
                                    cap,
                                    src_hash: src_hash(size),
                                    checked: frame,
                                },
                            ) {
                                env.sh.repack_bytes =
                                    env.sh.repack_bytes.saturating_sub(old.buf.length());
                            }
                            env.sh.repack_bytes += buf.length();
                            hit = Some(buf);
                        }
                    }
                    if let Some(buf) = hit {
                        if super::tracing_frame() {
                            println!(
                                "[ftrace]   vb{} repacked: stride {} -> {}, {} vertices",
                                s, stride, new_stride, n
                            );
                        }
                        env.cx.ec.vbuf(&enc, &buf, 0, VB_BASE - s);
                        continue;
                    }
                }
            }
            if stride > 0 && divisor == 0 {
                let mv = max_fetched_vertex(&call, stride, size as u64, || {
                    *exact_cache.get_or_insert_with(|| {
                        cached_max_vertex(&mut env.cx.max_index_memo, &call, p.prim_restart)
                    })
                });
                if let Some(mv) = mv {
                    let need = (mv + 1) * stride;
                    if need > size as u64 && need <= (64 << 20) {
                        let mut bytes = vec![0u8; need as usize];
                        if addr > 0x10000 {
                            std::ptr::copy_nonoverlapping(
                                addr as *const u8,
                                bytes.as_mut_ptr(),
                                size.min(need as usize),
                            );
                        }
                        if let Some((buf, off)) = env.cx.arena.alloc(&self.device, &bytes) {
                            if super::tracing_frame() {
                                println!("[ftrace]   vb{} padded: fetches up to vertex {} ({:#x} bytes) of a {:#x}-byte buffer", s, mv, need, size);
                            }
                            env.cx.ec.vbuf(&enc, &buf, off, VB_BASE - s);
                            continue;
                        }
                    }
                }
            }
            let Some((buf, off)) = self.guest_buffer_ro(&mut *env.sh, &mut *env.cx, addr, size)
            else {
                let t = crate::nvn::api::translate_virtual(addr as usize);
                println!("[!] Gfx: vertex stream {} at 0x{:x} (+{}) is not in a known memory pool (translated 0x{:x}, pool {:x?}, covering {:x?})",
                    s, addr, size, t, crate::nvn::api::find_memory_pool(t), crate::nvn::api::containing_pool(t, size));
                {
                    if super::tracing_frame() {
                        println!("[ftrace] SKIP draw at translated.rs:1159");
                    }
                    return true;
                };
            };
            if super::tracing_frame() {
                println!(
                    "[ftrace]   vb{} mtl len={:#x} off={:#x} (guest {:#x}+{:#x})",
                    s,
                    buf.length(),
                    off,
                    addr,
                    size
                );
            }
            env.cx.ec.vbuf(&enc, &buf, off, VB_BASE - s);
        }
        if needs_default_attr {
            let one = [0.0f32, 0.0, 0.0, 1.0];
            let bytes = std::slice::from_raw_parts(one.as_ptr() as *const u8, 16);
            if let Some((buf, off)) = env.cx.arena.alloc(&self.device, bytes) {
                env.cx.ec.vbuf(&enc, &buf, off, DEFAULT_ATTR_BUFFER);
            }
        }

        let _ph_t3 = if super::phases_on() {
            Some(std::time::Instant::now())
        } else {
            None
        };
        if let (Some(a), Some(b)) = (_ph_t2, _ph_t3) {
            super::PHASES2[1].fetch_add(
                (b - a).as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        // ---- per-stage resources ----
        if use_ab {
            env.cx.ec.bind_rec = Some(TinyRec::default());
        }
        let mut notes: Vec<String> = Vec::new();
        // Per-draw detail text is only for traces and GPU-error reports (first 16 draws of an encoder); formatting it for
        // every draw cost ~20% of the executor.
        let want_notes = super::tracing_frame()
            || (draw_details() && env.lb.enc_details.last().map_or(false, |d| d.len() < 16));
        if want_notes {
            let rts: Vec<String> = color_pfs
                .iter()
                .enumerate()
                .filter(|(_, p)| **p != MTLPixelFormat::Invalid)
                .map(|(i, p)| format!("rt{}[{:?}]", i, p))
                .collect();
            let vb: Vec<String> = (0..MAX_STREAMS)
                .filter(|s| streams_used[*s])
                .map(|s| {
                    format!(
                        "vb{}={:x?} stride/div={:x}",
                        s,
                        p.vertex_buffers[s],
                        p.vertex_streams.get(s).copied().unwrap_or(0)
                    )
                })
                .collect();
            let (cnt, ib) = match call {
                DrawKind::Arrays {
                    first,
                    count,
                    base_instance,
                    instances,
                } => (
                    count,
                    format!(
                        "arrays first={} base_instance={} instances={}",
                        first, base_instance, instances
                    ),
                ),
                DrawKind::Elements {
                    index_type,
                    count,
                    index_addr,
                    base_vertex,
                    base_instance,
                    instances,
                } => (
                    count,
                    format!(
                        "elements type={} addr=0x{:x} base_vertex={} base_instance={} instances={}",
                        index_type, index_addr, base_vertex, base_instance, instances
                    ),
                ),
            };
            if want_notes {
                notes.push(format!(
                "mode={} count={} {} | {} | {} | viewport={:?} scissor={:?} depth_range={:?} dclamp={} pofs={:?} blend={:x} color={:x} mask={:x} poly={:x} ds={:x} ms={:x}",
                mode, cnt, ib, rts.join(" "), vb.join(" "), p.viewport, p.scissor, p.depth_range, p.depth_clamp, p.polygon_offset, blend, p.color_state, p.channel_mask, p.polygon, p.depth_stencil, p.multisample
            ));
            }
        }
        let base_vertex = if let DrawKind::Elements { base_vertex, .. } = call {
            base_vertex
        } else if let DrawKind::Arrays { first, .. } = call {
            first
        } else {
            0
        };
        let base_instance_value = match call {
            DrawKind::Arrays { base_instance, .. } | DrawKind::Elements { base_instance, .. } => {
                base_instance
            }
        };
        for (nvn_stage, code, cached) in [
            (STAGE_VERTEX, Some(vs_code), Some(vs)),
            (STAGE_FRAGMENT, fs_code, fs),
        ] {
            let (Some(code), Some(cached)) = (code, cached) else {
                continue;
            };
            let is_vs = nvn_stage == STAGE_VERTEX;
            // Storage buffers. The NVN driver keeps a 0x100-byte block of {u64 address, u32 size, pad} descriptors per
            // hardware stage in cbuf 0, starting at 0x110 (vertex = block 0, fragment = block 4; inferred from TotK's
            // shaders against the game's BindStorageBuffer calls). A shader may instead read a descriptor the game wrote
            // into one of its own uniform buffers.
            let sb_block = 0x110 + 0x100 * if is_vs { 0 } else { 4 };
            let mut sb_descs: Vec<(usize, u64, u64)> = Vec::new(); // (cbuf0 byte offset, address, size)
                                                                   // p.sbufs holds the non-empty bindings of both stages in index order (DrawPacket::fill).
            for &(s, i, addr, size) in &p.sbufs {
                if s == nvn_stage && (0..16).contains(&i) {
                    sb_descs.push((sb_block + 0x10 * i as usize, addr, size as u64));
                }
            }
            for &(binding, _slot, cb_slot, cb_off) in &cached.sbuffers {
                let byte = cb_off as usize * 4;
                let desc = if cb_slot == 0 {
                    sb_descs.iter().find(|d| d.0 == byte).map(|d| (d.1, d.2))
                } else if cb_slot >= 3 {
                    p.sb_desc(nvn_stage, binding)
                } else {
                    None
                };
                let bound = {
                    desc.and_then(|(a, sz)| {
                        if a > 0x10000 && sz > 0 {
                            // Storage buffers the shader never writes are GPU reads (write tracking and the early-fence guard keep them).
                            if cached.sb_readonly.contains(&binding) {
                                self.guest_buffer_ro(&mut *env.sh, &mut *env.cx, a, sz as usize)
                            } else {
                                super::note_gpu_guest_write(
                                    super::GuestWrite::Storage,
                                    a,
                                    sz as usize,
                                );
                                self.guest_buffer(&mut *env.sh, &mut *env.cx, a, sz as usize)
                            }
                        } else {
                            None
                        }
                    })
                };
                let (buf, off) = match bound {
                    Some(b) => {
                        if want_notes {
                            let words = desc
                                .map(|(a, sz)| {
                                    let n = (sz as usize / 4).min(32);
                                    format!(
                                        "{:08x?}",
                                        std::slice::from_raw_parts(a as *const u32, n)
                                    )
                                })
                                .unwrap_or_default();
                            notes.push(format!(
                                "s{} sb b{} desc cb{}+{:#x} -> {:x?} words {}",
                                nvn_stage, binding, cb_slot, byte, desc, words
                            ));
                        }
                        b
                    }
                    None => {
                        if want_notes {
                            notes.push(format!(
                                "s{} sb b{} desc cb{}+{:#x} UNRESOLVED ({:x?}) -> zero buffer",
                                nvn_stage, binding, cb_slot, byte, desc
                            ));
                        }
                        match self.zero_buffer(&mut *env.sh) {
                            Some(z) => (z, 0),
                            None => continue,
                        }
                    }
                };
                if is_vs {
                    env.cx.ec.vbuf(&enc, &buf, off, binding as usize);
                } else {
                    env.cx.ec.fbuf(&enc, &buf, off, binding as usize);
                }
            }
            // Ryujinx's support buffer (buffer 0; the guest's cbuf 0 is translated to binding 17): render scales for
            // texture-size queries, viewport size/inverse. Shaders that use it read it unconditionally.
            {
                let (vw, vh) = p
                    .viewport
                    .map(|(_, _, w, h)| (w.max(1) as f32, h.max(1) as f32))
                    .unwrap_or((1.0, 1.0));
                let key = (vw.to_bits(), vh.to_bits());
                if !env.cx.support_cache.contains_key(&key) {
                    let mut sup = vec![0u8; SUPPORT_BUFFER_SIZE];
                    let put = |b: &mut Vec<u8>, off: usize, v: [f32; 4]| {
                        for (i, x) in v.iter().enumerate() {
                            b[off + i * 4..off + i * 4 + 4].copy_from_slice(&x.to_le_bytes());
                        }
                    };
                    put(&mut sup, 144, [2.0 / vw, 2.0 / vh, 1.0, 0.0]); // ViewportInverse
                    put(&mut sup, 160, [vw, vh, 1.0, 0.0]); // ViewportSize
                    for i in 0..73 {
                        put(&mut sup, 192 + i * 16, [1.0, 0.0, 0.0, 0.0]); // RenderScale: native resolution
                    }
                    if env.cx.support_cache.len() > 64 {
                        env.cx.support_cache.clear();
                    }
                    if let Some(x) = env.cx.arena.alloc(&self.device, &sup) {
                        env.cx.support_cache.insert(key, x);
                    }
                }
                if let Some((buf, off)) = env.cx.support_cache.get(&key).cloned() {
                    if is_vs {
                        env.cx.ec.vbuf(&enc, &buf, off, 0);
                    } else {
                        env.cx.ec.fbuf(&enc, &buf, off, 0);
                    }
                }
            }
            // Descriptor heaps (metal/tex_heap.rs): each sampled texture's and sampler's slot, the handles and heap
            // addresses for the driver cbuf, residency once per encoder.
            let heap_cb = if cached.texture_heap {
                let frame = super::presented_frames();
                env.cx.heaps.sweep(frame);
                let mut hc = HeapCb::default();
                let mut used = 1u32 << super::tex_heap::SAMPLERS;
                for tb in &cached.textures {
                    let Some(r) = resolved_tex
                        .iter()
                        .find(|r| r.0 == nvn_stage && r.1 == tb.binding)
                    else {
                        continue;
                    };
                    let Some(tex) = r.2.clone() else { continue };
                    let plain = p.color_state & 0xff != 0 || tex.mipmapLevelCount() <= 1;
                    let sampler = r
                        .3
                        .and_then(|id| self.sampler_state(&mut *env.sh, id, plain))
                        .or_else(|| self.default_sampler(&mut *env.sh));
                    // Tiny kernels take the sampler as a parameter at the texture's binding (index line `tks`).
                    if let (Some(r), Some(s)) = (env.cx.ec.rec.as_mut(), sampler.as_ref()) {
                        let smp = (Some(s.clone()), tb.binding as usize);
                        if is_vs {
                            r.vsamp.push(smp);
                        } else {
                            r.fsamp.push(smp);
                        }
                    }
                    let Some(ss) =
                        sampler.and_then(|s| env.cx.heaps.sampler(&self.device, &s, frame))
                    else {
                        continue;
                    };
                    let Some((ts, root)) =
                        env.cx.heaps.texture(&self.device, tb.declared, &tex, frame)
                    else {
                        continue;
                    };
                    hc.handles.push((tb.handle_index as u16, ts | ss << 20));
                    used |= 1 << tb.declared.min(super::tex_heap::SAMPLERS - 1);
                    for t in [&tex, &root] {
                        env.cx.ec.heap_use(
                            &enc,
                            ProtocolObject::<dyn objc2_metal::MTLResource>::from_ref(&**t),
                        );
                    }
                }
                for h in 0..super::tex_heap::HEAPS {
                    if used >> h & 1 == 0 {
                        continue;
                    }
                    hc.addresses[h] = env.cx.heaps.address(h);
                    if let Some(b) = env.cx.heaps.buffer(h).cloned() {
                        env.cx.ec.heap_use(
                            &enc,
                            ProtocolObject::<dyn objc2_metal::MTLResource>::from_ref(&*b),
                        );
                    }
                }
                Some(hc)
            } else {
                None
            };
            // Bound size of each constant buffer the stage reads (see CBUF_LIMITS_OFFSET).
            let mut limits = [4096u32; 18];
            for &(binding, slot) in &cached.cbuffers {
                if let Some(l) = limits.get_mut(binding as usize) {
                    *l = match slot {
                        0 => cbuf_limit(0x1000),
                        n if n >= 3 => p.ubo(nvn_stage, n as i32 - 3).map_or(0, |(a, sz)| {
                            if a > 0x10000 {
                                cbuf_limit(sz)
                            } else {
                                0
                            }
                        }),
                        _ => 4096,
                    };
                }
            }
            // Driver cbuf: draw parameters at the offsets Ryujinx documents for NVN titles, the storage buffer descriptors
            // of this stage and the constant buffer sizes.
            let fill = |c0: &mut [u8]| {
                c0.fill(0);
                c0[0x640..0x644].copy_from_slice(&(base_vertex as i32).to_le_bytes());
                c0[0x644..0x648].copy_from_slice(&base_instance_value.to_le_bytes());
                for &(o, a, sz) in &sb_descs {
                    c0[o..o + 8].copy_from_slice(&a.to_le_bytes());
                    c0[o + 8..o + 12].copy_from_slice(&(sz as u32).to_le_bytes());
                }
                for (b, l) in limits.iter().enumerate() {
                    c0[CBUF_LIMITS_OFFSET + b * 4..CBUF_LIMITS_OFFSET + b * 4 + 4]
                        .copy_from_slice(&l.to_le_bytes());
                }
                if let Some(hc) = &heap_cb {
                    for (h, a) in hc.addresses.iter().enumerate() {
                        let o = super::tex_heap::ADDRESSES + h * 8;
                        c0[o..o + 8].copy_from_slice(&a.to_le_bytes());
                    }
                    for &(i, v) in &hc.handles {
                        let o = super::tex_heap::HANDLES + i as usize * 4;
                        c0[o..o + 4].copy_from_slice(&v.to_le_bytes());
                    }
                }
            };
            // One copy per distinct content: consecutive draws mostly share it (--no-driver-cb-memo: a copy per draw).
            let dkey = DriverCbKey {
                base_vertex: base_vertex as i32,
                base_instance: base_instance_value,
                sb_descs: sb_descs.clone(),
                limits,
                heap: heap_cb.clone(),
            };
            let driver_cb =
                |env: &mut EncodeEnv| -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
                    let si = if is_vs { 0 } else { 1 };
                    if let Some((k, x)) = &env.cx.driver_cb_memo[si] {
                        if *k == dkey {
                            return Some(x.clone());
                        }
                    }
                    let x = env.cx.arena.alloc_with(&self.device, 0x1000, fill)?;
                    env.cx.driver_cb_memo[si] = Some((dkey.clone(), x.clone()));
                    Some(x)
                };
            if (cached.cbuf_limits && !cached.cbuffers.iter().any(|&(_, slot)| slot == 0))
                || (cached.texture_heap && !cached.cbuffers.iter().any(|&(b, _)| b == 17))
            {
                if let Some((buf, off)) = driver_cb(env) {
                    if is_vs {
                        env.cx.ec.vbuf(&enc, &buf, off, 17);
                    } else {
                        env.cx.ec.fbuf(&enc, &buf, off, 17);
                    }
                }
            }
            for &(binding, slot) in &cached.cbuffers {
                let (buf, off) = match slot {
                    0 => match driver_cb(env) {
                        Some(x) => x,
                        None => continue,
                    },
                    1 => match self.const_bank(&mut *env.sh, code.data, code.control, cached) {
                        Some(b) => {
                            if super::tracing_frame() {
                                let f = std::slice::from_raw_parts(
                                    b.contents().as_ptr() as *const f32,
                                    8,
                                );
                                if want_notes {
                                    notes.push(format!(
                                        "s{} cbuf1 (const bank {:x?}) f32={:?}",
                                        nvn_stage,
                                        control_const_bank(code.control),
                                        f
                                    ));
                                }
                            }
                            (b, 0)
                        }
                        None => {
                            if want_notes {
                                notes.push(format!(
                                    "s{} cbuf1 (const bank) MISSING -> mtl{} left unbound",
                                    nvn_stage, binding
                                ));
                            }
                            continue;
                        }
                    },
                    // Texture handle table: textures are bound directly, but a shader that declares it must not read null.
                    2 => match self.zero_buffer(&mut *env.sh) {
                        Some(z) => (z, 0),
                        None => continue,
                    },
                    n => {
                        let ubo = (nvn_stage, n as i32 - 3);
                        if let Some(UboPre::Snapshot(b, o)) = p.ubo_pre(nvn_stage, binding) {
                            if want_notes {
                                notes.push(format!("s{} ubo{} (cbuf{} -> mtl{}) copied in command order (written later in this submit)", nvn_stage, n as i32 - 3, n, binding));
                            }
                            (b.clone(), *o)
                        } else {
                            match p.ubo(ubo.0, ubo.1) {
                                // Copied at encode time, not bound zero-copy: UpdateUniformBuffer commands rewrite guest memory in
                                // stream order on the CPU, while the GPU reads it later, so a shared pool binding would show every
                                // draw in the submit the final value.
                                // Zero-copy, as the GPU reads it: memory GPU work of this submit writes, and with
                                // --ubo-zero-copy (experiment) memory no UpdateUniformBuffer of this submit writes.
                                // Inline updates are applied to guest memory at encode time, so buffers they touch must be
                                // copied per draw. Measured no faster than copying (8000 uniform binds per frame in Hateno),
                                // and a zero-copy binding near a pool's end lacks the slack copies have for the shaders'
                                // 64 KB declarations, so it stays off.
                                Some((addr, size))
                                    if addr > 0x10000
                                        && size > 0
                                        && matches!(
                                            p.ubo_pre(nvn_stage, binding),
                                            Some(UboPre::ZeroCopy)
                                        ) =>
                                {
                                    if want_notes {
                                        // Shared memory: what the CPU sees now is the previous submit's GPU result.
                                        let prev: Vec<f32> = if super::tracing_frame() {
                                            let k = crate::settings::get()
                                                .trace
                                                .trace_ubo_floats
                                                .min(size / 4);
                                            std::slice::from_raw_parts(addr as *const f32, k)
                                                .to_vec()
                                        } else {
                                            Vec::new()
                                        };
                                        notes.push(format!("s{} ubo{} (cbuf{} -> mtl{}) {}B written by GPU work this submit: bound zero-copy; previous={:?}", nvn_stage, n as i32 - 3, n, binding, size, prev));
                                    }
                                    match self.guest_buffer_ro(
                                        &mut *env.sh,
                                        &mut *env.cx,
                                        addr,
                                        size.min(CBUF_DECL_SIZE),
                                    ) {
                                        Some(x) => x,
                                        None => match self.zero_buffer(&mut *env.sh) {
                                            Some(z) => (z, 0),
                                            None => continue,
                                        },
                                    }
                                }
                                Some((addr, size)) if addr > 0x10000 && size > 0 => {
                                    // Only what the stage can read (its literal-offset extent). The arena leaves
                                    // CBUF_DECL_SIZE of slack after every copy for the shaders' 64 KB declarations.
                                    let len = cached.cbuf_read_len(binding, CBUF_DECL_SIZE);
                                    let bytes = std::slice::from_raw_parts(
                                        addr as *const u8,
                                        size.min(len).max(size.min(16)),
                                    );
                                    let pad = cbuf_zero_pad(len, size);
                                    if want_notes {
                                        let first: Vec<f32> = bytes
                                            .chunks(4)
                                            .take(if super::tracing_frame() {
                                                crate::settings::get().trace.trace_ubo_floats
                                            } else {
                                                6
                                            })
                                            .map(|c| {
                                                if c.len() == 4 {
                                                    f32::from_le_bytes([c[0], c[1], c[2], c[3]])
                                                } else {
                                                    0.0
                                                }
                                            })
                                            .collect();
                                        notes.push(format!(
                                            "s{} ubo{} (cbuf{} -> mtl{}) {}B first={:?}",
                                            nvn_stage,
                                            n as i32 - 3,
                                            n,
                                            binding,
                                            size,
                                            first
                                        ));
                                    }
                                    // Within a submit the bytes change only through the executor's own writes (which drop
                                    // affected copies) or GPU work (bound zero-copy above): reuse the copy made for an
                                    // earlier draw.
                                    let epoch = super::GUEST_WRITES
                                        .load(std::sync::atomic::Ordering::Relaxed);
                                    if env.cx.ubo_epoch != epoch {
                                        UBO_STATS[3]
                                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                        env.cx.ubo_copies.clear();
                                        env.cx.ubo_epoch = epoch;
                                    }
                                    let key = (addr, bytes.len() | pad << 32);
                                    let cached = env.cx.ubo_copies.get(&key).cloned();
                                    UBO_STATS[if cached.is_some() { 1 } else { 0 }]
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    if cached.is_none() {
                                        UBO_STATS[2].fetch_add(
                                            bytes.len() as u64,
                                            std::sync::atomic::Ordering::Relaxed,
                                        );
                                    }
                                    if cached.is_none() {
                                        super::note_ubo_pages(addr as u64, bytes.len());
                                    }
                                    let fresh = if cached.is_some() {
                                        None
                                    } else if pad > 0 {
                                        env.cx.arena.alloc_with(
                                            &self.device,
                                            bytes.len() + pad,
                                            |b| {
                                                b[..bytes.len()].copy_from_slice(bytes);
                                                b[bytes.len()..].fill(0);
                                            },
                                        )
                                    } else {
                                        env.cx.arena.alloc(&self.device, bytes)
                                    };
                                    if let Some(f) = &fresh {
                                        env.cx.ubo_copies.insert(key, f.clone(), true);
                                    }
                                    match cached.or(fresh) {
                                        Some(x) => x,
                                        None => {
                                            if want_notes {
                                                notes.push(format!("s{} cbuf{} arena allocation FAILED -> mtl{} zero buffer", nvn_stage, n, binding));
                                            }
                                            match self.zero_buffer(&mut *env.sh) {
                                                Some(z) => (z, 0),
                                                None => continue,
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    if want_notes {
                                        notes.push(format!("s{} ubo{} (cbuf{} -> mtl{}) NOT BOUND by the game: zero-filled", nvn_stage, n as i32 - 3, n, binding));
                                    }
                                    match self.zero_buffer(&mut *env.sh) {
                                        Some(z) => (z, 0),
                                        None => continue,
                                    }
                                }
                            }
                        }
                    }
                };
                if is_vs {
                    env.cx.ec.vbuf(&enc, &buf, off, binding as usize);
                } else {
                    env.cx.ec.fbuf(&enc, &buf, off, binding as usize);
                }
            }
            let mut extra_samplers: Vec<(usize, Retained<ProtocolObject<dyn MTLSamplerState>>)> =
                Vec::new();
            for tb in &cached.textures {
                let (tex, sampler_id) = match resolved_tex
                    .iter()
                    .find(|r| r.0 == nvn_stage && r.1 == tb.binding)
                {
                    Some(r) => (r.2.clone(), r.3),
                    None => (None, None),
                };
                // The plan resolved every binding (dummies included): None means no dummy could be made either.
                let Some(tex) = tex else {
                    if want_notes {
                        notes.push(format!(
                            "s{} tex b{} {} NO TEXTURE and no dummy -> left unbound",
                            nvn_stage, tb.binding, tb.ty
                        ));
                    }
                    continue;
                };
                let k = tb.handle_index.wrapping_sub(8) / 2;
                if super::tracing_frame()
                    && tex.width() * tex.height() <= 64
                    && tex.pixelFormat() == MTLPixelFormat::RGBA8Unorm
                {
                    let n = tex.width() * tex.height();
                    let mut px = vec![0u8; n * 4];
                    let region = MTLRegion {
                        origin: MTLOrigin { x: 0, y: 0, z: 0 },
                        size: MTLSize {
                            width: tex.width(),
                            height: tex.height(),
                            depth: 1,
                        },
                    };
                    tex.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                        std::ptr::NonNull::new(px.as_mut_ptr().cast()).unwrap(),
                        tex.width() * 4,
                        region,
                        0,
                    );
                    if want_notes {
                        notes.push(format!(
                            "s{} tex b{} texels(rgba8)={:?}",
                            nvn_stage,
                            tb.binding,
                            &px[..px.len().min(32)]
                        ));
                    }
                }
                if want_notes {
                    let tex_root = root_id(&tex);
                    let src = env
                        .frame_rts
                        .iter()
                        .position(|(id, ..)| *id == tex_root)
                        .map_or("not drawn this frame".to_string(), |i| {
                            format!("=rt{:02}", i)
                        });
                    let nvn = p.handle(nvn_stage, k as i32).and_then(|h| crate::nvn::api::lookup_pool_texture((h & 0xFFFFF) as i32)).and_then(|(a, v)| {
                    // The recorded state: the object itself may have been freed and reused by the game.
                    let (_, t) = super::tex_info(a)?;
                    let t = &t;
                    let verify = if super::verify_tex_enabled() {
                        let upl = super::upload_record(super::storage_id(&tex));
                        let cur = super::current_guest_hash(a);
                        let nz = if t.storage > 0x10000 && crate::guest::exec::crash_handler::readable(t.storage as usize, 0x4000) { std::slice::from_raw_parts(t.storage as *const u8, 0x4000).iter().filter(|&&x| x != 0).count() } else { 0 };
                        format!(" verify: guest_nonzero16k={} snap_storage={:x?} upload={:x?} {}", nz, cur.map(|c| c.0), upl,
                            match (cur, upl) { (Some(c), Some(u)) if c.0 == u.0 && c.1 == u.1 => "OK", (Some(c), Some(u)) if c.0 != u.0 => "FOREIGN", (Some(_), Some(_)) => "STALE", _ => "?" })
                    } else { String::new() };
                    Some(format!("nvn tex {:#x} {}x{} fmt={} storage={:#x} view={:?} mtl_swizzle={:?}{}", a, t.width, t.height, t.format, t.storage, v, {
                        let sw = tex.swizzle();
                        (sw.red.0, sw.green.0, sw.blue.0, sw.alpha.0)
                    }, verify))
                });
                    notes.push(format!(
                    "s{} tex b{} want={} got={:?} {}x{} {:?} lvls={} handle={:x?} sampler={:?} src {} [{}]",
                    nvn_stage, tb.binding, tb.ty, tex.textureType(), tex.width(), tex.height(), tex.pixelFormat(), tex.mipmapLevelCount(),
                    p.handle(nvn_stage, k as i32), sampler_id, src, nvn.unwrap_or_default()
                ));
                }
                if cached.texture_heap {
                    continue;
                }
                let plain = p.color_state & 0xff != 0 || tex.mipmapLevelCount() <= 1;
                let sampler = sampler_id
                    .and_then(|id| self.sampler_state(&mut *env.sh, id, plain))
                    .or_else(|| self.default_sampler(&mut *env.sh));
                if is_vs {
                    env.cx.ec.vtex(&enc, &tex, tb.binding as usize);
                } else {
                    env.cx.ec.ftex(&enc, &tex, tb.binding as usize);
                }
                if (tb.binding as usize) < EXTRA_SAMPLER_BASE {
                    if is_vs {
                        env.cx
                            .ec
                            .vsamp(&enc, sampler.as_deref(), tb.binding as usize);
                    } else {
                        env.cx
                            .ec
                            .fsamp(&enc, sampler.as_deref(), tb.binding as usize);
                    }
                } else if let Some(s) = sampler {
                    extra_samplers.push((tb.binding as usize - EXTRA_SAMPLER_BASE, s));
                }
            }
            if let Some((buf, off)) = self.extra_sampler_buffer(&mut *env.cx, &extra_samplers) {
                if is_vs {
                    env.cx.ec.vbuf(&enc, &buf, off, EXTRA_SAMPLER_BUFFER);
                } else {
                    env.cx.ec.fbuf(&enc, &buf, off, EXTRA_SAMPLER_BUFFER);
                }
            }
        }

        {
            let (cnt, inst) = match call {
                DrawKind::Arrays {
                    count, instances, ..
                }
                | DrawKind::Elements {
                    count, instances, ..
                } => (count as i64, instances as i64),
            };
            if inst > 1 {
                env.cx.instanced_stats.0 += 1;
                env.cx.instanced_stats.1 = env.cx.instanced_stats.1.max(inst);
                env.cx.instanced_stats.2 += inst;
            }
            env.cx.instanced_stats.3 = env.cx.instanced_stats.3.max(cnt);
        }
        // By stage pointer (stages live for the whole run; names are looked up when printed): cloning both entry names
        // here allocated twice per draw.
        let hist_key = (
            vs as *const CachedStage as usize,
            fs.map_or(0, |f| f as *const CachedStage as usize),
            color_pfs[0].0 as u64,
            depth_pf.0 as u64,
        );
        if env.cx.translated_hist.get(&hist_key).copied().unwrap_or(0) < trace_limit() {
            let mut tex_info = Vec::new();
            for (nvn_stage, cached) in [(STAGE_VERTEX, Some(vs)), (STAGE_FRAGMENT, fs)] {
                let Some(c) = cached else { continue };
                for tb in &c.textures {
                    let k = tb.handle_index.wrapping_sub(8) / 2;
                    let h = p.handle(nvn_stage, k as i32).copied();
                    tex_info.push(format!(
                        "s{}:b{}:{}:k{}:h{:x?}",
                        nvn_stage, tb.binding, tb.ty, k, h
                    ));
                }
            }
            let (count, indexed) = match call {
                DrawKind::Arrays { count, .. } => (count, false),
                DrawKind::Elements { count, .. } => (count, true),
            };
            // Inputs: vertex stream 0 (as half4 and float4), index buffer, user cbuf contents, texture provenance.
            if let Some((addr, size)) = p.vertex_buffers[0] {
                if addr > 0x10000 && size >= 16 {
                    let h = std::slice::from_raw_parts(addr as *const u16, 16.min(size / 2));
                    let f = std::slice::from_raw_parts(addr as *const f32, 8.min(size / 4));
                    println!("[trace]   vb0 size={} half={:04x?} f32={:?}", size, h, f);
                }
            }
            if let DrawKind::Elements {
                index_type,
                count,
                index_addr,
                ..
            } = call
            {
                if index_addr > 0x10000 && index_type == 1 {
                    println!(
                        "[trace]   ib16 {:?}",
                        std::slice::from_raw_parts(
                            index_addr as *const u16,
                            (count as usize).min(12)
                        )
                    );
                }
            }
            for (nvn_stage, cached) in [(STAGE_VERTEX, Some(vs)), (STAGE_FRAGMENT, fs)] {
                let Some(c) = cached else { continue };
                for &(binding, slot) in &c.cbuffers {
                    if slot >= 3 {
                        if let Some((addr, size)) = p.ubo(nvn_stage, slot as i32 - 3) {
                            if addr > 0x10000 && size >= 16 {
                                let f = std::slice::from_raw_parts(
                                    addr as *const f32,
                                    (size / 4).min(24),
                                );
                                println!(
                                    "[trace]   s{} cbuf{} (ubo{}, mtl {}) size={} f32={:?}",
                                    nvn_stage,
                                    slot,
                                    slot - 3,
                                    binding,
                                    size,
                                    f
                                );
                            }
                        } else {
                            println!(
                                "[trace]   s{} cbuf{} (ubo{}) NOT BOUND",
                                nvn_stage,
                                slot,
                                slot - 3
                            );
                        }
                    }
                }
            }
            for (nvn_stage, cached) in [(STAGE_VERTEX, Some(vs)), (STAGE_FRAGMENT, fs)] {
                let Some(c) = cached else { continue };
                for tb in &c.textures {
                    let k = tb.handle_index.wrapping_sub(8) / 2;
                    if let Some(&h) = p.handle(nvn_stage, k as i32) {
                        let id = (h & 0xFFFFF) as i32;
                        println!(
                            "[trace]   tex s{} b{} id={} pool={:?} sampler={:?}",
                            nvn_stage,
                            tb.binding,
                            id,
                            crate::nvn::api::lookup_pool_texture(id),
                            crate::nvn::api::lookup_pool_sampler((h >> 20) as i32)
                        );
                    }
                }
            }
            println!(
                "[trace] draw#{} mode={} count={} indexed={} rt_fmts={:?} depth={:?} viewport={:?} scissor={:?} vs={} fs={} attrs={:?} blend={:x} color_state={:x} mask={:x} poly={:x} ds={:x} tex=[{}]",
                env.cx.translated_draws, mode, count, indexed,
                color_pfs.iter().filter(|p| **p != MTLPixelFormat::Invalid).map(|p| p.0).collect::<Vec<_>>(),
                depth_pf,
                p.viewport, p.scissor, vs.entry, fs.map_or("-", |f| &f.entry),
                attr_descs.iter().map(|(l, f, b, o, k)| (*l, f.0, *b, *o, *k)).collect::<Vec<_>>(),
                blend, p.color_state, p.channel_mask, poly, p.depth_stencil, tex_info.join(" "),
            );
        }

        let _ph_t4 = if super::phases_on() {
            Some(std::time::Instant::now())
        } else {
            None
        };
        if let (Some(a), Some(b)) = (_ph_t3, _ph_t4) {
            super::PHASES2[2].fetch_add(
                (b - a).as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        // ---- draw ----
        // The encoder label collects the shader pairs drawn in it (hang/error reports name the encoder). Per-draw
        // strings cost ~10% of the executor, so the per-draw debug group (named by the failing command buffer on a
        // hang) and per-draw binding details are built only with --draw-details or while tracing.
        let details = draw_details() || super::tracing_frame();
        let pair = if details {
            format!(
                "{} / {} ({} idx/verts)",
                vs.entry,
                fs.map_or("-", |f| &f.entry),
                match call {
                    DrawKind::Arrays { count, .. } | DrawKind::Elements { count, .. } => count,
                }
            )
        } else {
            String::new()
        };
        if super::tracing_frame() {
            let rts: Vec<String> = color_pfs
                .iter()
                .enumerate()
                .filter(|(_, p)| **p != MTLPixelFormat::Invalid)
                .map(|(i, p)| format!("c{}={:?}", i, p))
                .collect();
            let attrs: Vec<String> = attr_descs
                .iter()
                .map(|(loc, fmt, buf, off, _)| format!("a{}:{:?}@b{}+{}", loc, fmt, buf, off))
                .collect();
            let vb0 = p.vertex_buffers[0]
                .map(|(addr, size)| {
                    if addr > 0x10000 && size >= 16 {
                        format!(
                            "{:?}",
                            std::slice::from_raw_parts(addr as *const f32, (size / 4).min(16))
                        )
                    } else {
                        String::new()
                    }
                })
                .unwrap_or_default();
            let mut sbs: Vec<String> = Vec::new();
            for (nvn_stage, cached) in [(STAGE_VERTEX, Some(vs)), (STAGE_FRAGMENT, fs)] {
                let Some(c) = cached else { continue };
                for &(b, slot, cbs, off) in &c.sbuffers {
                    sbs.push(format!(
                        "s{} sb b{} slot{} desc@cb{}+{:#x}",
                        nvn_stage,
                        b,
                        slot,
                        cbs,
                        off * 4
                    ));
                }
                let mut bound: Vec<_> = (0..16)
                    .filter_map(|i| p.sbuf(nvn_stage, i).map(|(a, sz)| (i, a, sz)))
                    .collect();
                bound.sort();
                if !bound.is_empty() {
                    sbs.push(format!("s{} bound ssbo {:x?}", nvn_stage, bound));
                }
            }
            println!(
                "[ftrace] DRAW enc=f{}e{} draw={} [{}] {} | attrs {} | vb0 f32 {} | {} | {}",
                super::presented_frames() + 1,
                env.lb.enc_labels.len().saturating_sub(1),
                env.enc_draws,
                rts.join(", "),
                pair,
                attrs.join(" "),
                vb0,
                sbs.join(" ; "),
                notes.join(" | ")
            );
        }
        if let Some(tp) = &p.tiny {
            let rec = env.cx.ec.rec.take().unwrap_or_default();
            env.cx.ec = EncCache::default();
            let ok = self.emit_tiny(
                env,
                p,
                tp,
                rec,
                vs,
                fs,
                attr_descs,
                streams_eff,
                depth_gl,
                point_size,
            );
            env.drew = ok;
            env.cx.translated_draws += 1;
            super::TRANSLATED_DRAWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return true;
        }
        if use_ab {
            let rec = env.cx.ec.bind_rec.take().unwrap_or_default();
            // The structs take buffer 0 of each stage: the plain path's binding there is gone.
            env.cx.ec.vbuf[0] = (0, usize::MAX);
            env.cx.ec.fbuf[0] = (0, usize::MAX);
            let zero = self.zero_buffer(&mut *env.sh);
            let mut res = Vec::new();
            let v = compose_args(vs, &rec.vbufs, &rec.vtex, &rec.vsamp, zero.as_deref(), &mut res);
            let f = fs.map_or_else(Vec::new, |f| compose_args(f, &rec.fbufs, &rec.ftex, &rec.fsamp, zero.as_deref(), &mut res));
            res.extend(rec.uses.into_iter().map(|x| (x, objc2_metal::MTLResourceUsage::Read)));
            enc.push(Op::ArgStructs { v: v.into(), f: f.into(), res });
        }
        if details {
            enc.push(Op::PushGroup(pair.clone()));
        }
        match call {
            DrawKind::Arrays {
                first,
                count,
                base_instance,
                instances,
            } => {
                if count > 0 && instances > 0 {
                    enc.push(Op::Draw {
                        prim,
                        start: first.max(0) as usize,
                        count: count as usize,
                        instances: instances as usize,
                        base_instance: base_instance.max(0) as usize,
                    });
                }
            }
            DrawKind::Elements {
                index_type,
                count,
                index_addr,
                base_vertex,
                base_instance,
                instances,
            } => {
                // Primitive restart: rewritten to 32-bit indices with the base vertex folded in (see restart_rewrite).
                // Index copies (restart rewrite, u8 widening, misaligned ranges) are reused by the next draw with the same
                // inputs (one memo entry, this submit: guest index data does not change between its draws): consecutive
                // draws of a mesh share one copy, which also lets their draws batch (metal/batch.rs).
                let mkey = |kind: u8| {
                    (
                        mode,
                        index_type,
                        index_addr,
                        count,
                        p.prim_restart,
                        base_vertex,
                        kind,
                    )
                };
                let memo_hit = |env: &EncodeEnv,
                                kind: u8|
                 -> Option<(
                    Retained<ProtocolObject<dyn MTLBuffer>>,
                    usize,
                    usize,
                )> {
                    match &env.cx.index_memo {
                        Some((k, b, o, n)) if *k == mkey(kind) => {
                            Some((b.clone(), *o, *n))
                        }
                        _ => None,
                    }
                };
                let restart_hit = if p.prim_restart.is_some() {
                    memo_hit(env, 0)
                } else {
                    None
                };
                let rewritten = if restart_hit.is_some() {
                    None
                } else {
                    p.prim_restart.and_then(|r| {
                        crate::gpu::restart::restart_rewrite(mode, index_type, index_addr, count, r, base_vertex)
                    })
                };
                if super::tracing_frame() {
                    println!(
                        "[ftrace]   ^ restart {:x?} mode {} rewritten {:?}",
                        p.prim_restart,
                        mode,
                        rewritten.as_ref().map(|v| v.len())
                    );
                }
                if let Some((buf, off, n)) = restart_hit {
                    if n > 0 && instances > 0 {
                        enc.push(Op::DrawIndexed {
                            prim,
                            count: n,
                            ty: MTLIndexType::UInt32,
                            buf,
                            off,
                            instances: instances as usize,
                            base_vertex: 0,
                            base_instance: base_instance.max(0) as usize,
                        });
                    }
                } else if let Some(v) = rewritten {
                    if !v.is_empty() && instances > 0 {
                        let bytes: &[u8] =
                            std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4);
                        if let Some((buf, off)) = env.cx.arena.alloc(&self.device, bytes) {
                            env.cx.index_memo = Some((mkey(0), buf.clone(), off, v.len()));
                            enc.push(Op::DrawIndexed {
                                prim,
                                count: v.len(),
                                ty: MTLIndexType::UInt32,
                                buf,
                                off,
                                instances: instances as usize,
                                base_vertex: 0,
                                base_instance: base_instance.max(0) as usize,
                            });
                        }
                    }
                } else if count > 0 && instances > 0 {
                    let (ty, bytes) = match index_type {
                        1 => (MTLIndexType::UInt16, 2usize),
                        2 => (MTLIndexType::UInt32, 4usize),
                        _ => (MTLIndexType::UInt16, 2usize), // u8 indices are widened below
                    };
                    let ib = if let Some((b, o, _)) = memo_hit(env, 1) {
                        Some((b, o))
                    } else if index_type == 0 {
                        let held = super::rt_guard::guest_copy::<u8>(index_addr as usize, count as usize);
                        let u8s = held.as_deref().unwrap_or_else(|| {
                            std::slice::from_raw_parts(index_addr as *const u8, count as usize)
                        });
                        let wide: Vec<u8> =
                            u8s.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect();
                        let x = env.cx.arena.alloc(&self.device, &wide);
                        if let Some((b, o)) = &x {
                            env.cx.index_memo = Some((mkey(1), b.clone(), *o, count as usize));
                        }
                        x
                    } else {
                        // Metal needs a 4-byte aligned index buffer offset; NVN allows any 2-byte aligned address for
                        // 16-bit indices (TotK draws sub-ranges starting mid-buffer). Misaligned offsets fetched garbage
                        // indices: huge triangles across the screen. Copy those into the (aligned) arena instead.
                        match self.guest_buffer_ro(
                            &mut *env.sh,
                            &mut *env.cx,
                            index_addr,
                            count as usize * bytes,
                        ) {
                            Some((_, off)) if off % 4 != 0 => {
                                let x = env.cx.arena.alloc(
                                    &self.device,
                                    std::slice::from_raw_parts(
                                        index_addr as *const u8,
                                        count as usize * bytes,
                                    ),
                                );
                                if let Some((b, o)) = &x {
                                    env.cx.index_memo =
                                        Some((mkey(1), b.clone(), *o, count as usize));
                                }
                                x
                            }
                            other => other,
                        }
                    };
                    if super::tracing_frame() && index_addr > 0x10000 {
                        let (lo, hi) = match index_type {
                            2 => {
                                let v = std::slice::from_raw_parts(
                                    index_addr as *const u32,
                                    count as usize,
                                );
                                (
                                    v.iter().min().copied().unwrap_or(0) as i64,
                                    v.iter().max().copied().unwrap_or(0) as i64,
                                )
                            }
                            1 => {
                                let v = std::slice::from_raw_parts(
                                    index_addr as *const u16,
                                    count as usize,
                                );
                                (
                                    v.iter().min().copied().unwrap_or(0) as i64,
                                    v.iter().max().copied().unwrap_or(0) as i64,
                                )
                            }
                            _ => {
                                let v = std::slice::from_raw_parts(
                                    index_addr as *const u8,
                                    count as usize,
                                );
                                (
                                    v.iter().min().copied().unwrap_or(0) as i64,
                                    v.iter().max().copied().unwrap_or(0) as i64,
                                )
                            }
                        };
                        println!(
                            "[ftrace]   ^ index range {}..={} (+base_vertex {}) ib={:?}",
                            lo,
                            hi,
                            base_vertex,
                            ib.as_ref().map(|(b, o)| (b.length(), *o))
                        );
                    }
                    if let Some((buf, off)) = ib {
                        enc.push(Op::DrawIndexed {
                            prim,
                            count: count as usize,
                            ty,
                            buf: buf.clone(),
                            off,
                            instances: instances as usize,
                            base_vertex: base_vertex as isize,
                            base_instance: base_instance.max(0) as usize,
                        });
                    } else {
                        println!(
                            "[!] Gfx: index buffer 0x{:x} is not in a known memory pool",
                            index_addr
                        );
                    }
                }
            }
        }
        if details {
            enc.push(Op::PopGroup);
        }
        if want_notes {
            self.note_detail(&mut *env.lb, notes.join(" || "));
        }
        env.drew = true;
        env.cx.translated_draws += 1;
        super::TRANSLATED_DRAWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *env.cx.translated_hist.entry(hist_key).or_insert(0) += 1;
        true
    }
}
