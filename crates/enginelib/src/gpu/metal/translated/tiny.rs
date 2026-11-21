// Tiny render passes run as compute kernels (qualifying 3-6 vertex draws).

use super::*;

impl Gfx {
    /// A tiny-pass kernel (tkv_ / tkf_ of `entry`) with the function constants its key (tiny_keys) stands for.
    pub(crate) fn make_tiny(&self, lib: &ShaderLibs, entry: &str, key: &[i64]) -> Result<ComputePipe, String> {
        let cv = MTLFunctionConstantValues::new();
        // SAFETY: each value lives across its call and has the size of `ty`.
        let set = |id: usize, ty: MTLDataType, v: *const libc::c_void| unsafe {
            cv.setConstantValue_type_atIndex(std::ptr::NonNull::new(v as *mut libc::c_void).unwrap(), ty, id)
        };
        let set_int = |id: usize, v: i32| set(id, MTLDataType::Int, &v as *const i32 as _);
        let set_bool = |id: usize, v: bool| set(id, MTLDataType::Bool, &v as *const bool as _);
        let name = if key[0] == 0 {
            set_bool(40, key[1] != 0);
            set_bool(41, key[2] != 0);
            let point_size = f32::from_bits(key[3] as u32);
            set(42, MTLDataType::Float, &point_size as *const f32 as _);
            for kv in key[4..].chunks(2) {
                set_int(kv[0] as usize, kv[1] as i32);
            }
            format!("tkv_{entry}")
        } else {
            set_bool(43, false);
            for kv in key[1..].chunks(2) {
                if kv[0] < 100 {
                    set_int(32 + kv[0] as usize, kv[1] as i32);
                } else {
                    set_bool(48 + (kv[0] - 100) as usize, kv[1] != 0);
                }
            }
            format!("tkf_{entry}")
        };
        let f = lib.kernel(entry, &name, &cv)?;
        self.device.newComputePipelineStateWithFunction_error(&f).map_err(|e| e.localizedDescription().to_string())
    }

    /// Whether this draw runs as compute (tkv_ / tkf_ kernels the cache builder generated from the
    /// two stages) instead of in a render pass (~40 us of fixed GPU time per pass on Apple GPUs, ~1-2 us per dispatch).
    /// Only when the result cannot differ from rasterizing it: see the checks. On success the open render pass is ended
    /// (pending clears first) and the targets are marked GPU-written; the packet then carries the plan.
    /// --tiny-compute enables; --replay-ab-tiny renders on replay side B; traces log declines and reasons.
    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn tiny_plan(
        &self,
        st: &mut State,
        vs: &'static CachedStage,
        fs: Option<&'static CachedStage>,
        call: &DrawKind,
        mode: i32,
        color_texs: &[(usize, Tex, RtRef)],
        depth_tex: &Option<(Tex, RtRef)>,
        resolved: &ResolvedTex,
    ) -> Option<TinyPlan> {
        if !tiny_enabled() {
            return None;
        }
        // Kernels reading their samplers from the sampler heap gave varying results from one run of the same frame to
        // the next (TotK's atmosphere LUT kernel s_ec8a96bdd2f04e50: whole-frame tint flashes; the render versions are
        // stable, cause not found): only kernels that take their samplers as parameters run (index line `tks`).
        let heap_samplers = |c: &CachedStage| c.texture_heap && !c.textures.is_empty() && !c.tiny_samplers;
        if heap_samplers(vs) || fs.is_some_and(heap_samplers) {
            return None;
        }
        let r = self.tiny_plan_inner(st, vs, fs, call, mode, color_texs, depth_tex, resolved);
        if super::tracing_frame() {
            match &r {
                Some(t) => println!(
                    "[ftrace] TINY COMPUTE {} / {}: {} vertices, {} triangles, rect {:?}",
                    vs.entry,
                    fs.map_or("-", |f| &f.entry),
                    t.nverts,
                    t.ntri,
                    t.rect
                ),
                None => println!(
                    "[ftrace] TINY COMPUTE declined {} / {}: {}",
                    vs.entry,
                    fs.map_or("-", |f| &f.entry),
                    TINY_REASON.with(|x| x.take())
                ),
            }
        }
        r
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn tiny_plan_inner(
        &self,
        st: &mut State,
        vs: &'static CachedStage,
        fs: Option<&'static CachedStage>,
        call: &DrawKind,
        mode: i32,
        color_texs: &[(usize, Tex, RtRef)],
        depth_tex: &Option<(Tex, RtRef)>,
        resolved: &ResolvedTex,
    ) -> Option<TinyPlan> {
        // Reasons are formatted only while a frame is traced: this runs for every draw.
        macro_rules! decline {
            ($($t:tt)*) => {{ if super::tracing_frame() { tiny_reason(format!($($t)*)); } return None; }};
        }
        let Some(fs) = fs else {
            decline!("no fragment stage")
        };
        if !vs.tiny_kernel || !fs.tiny_kernel {
            decline!(
                "no kernel (vertex {}, fragment {})",
                vs.tiny_kernel,
                fs.tiny_kernel
            );
        }
        // Geometry first (cheap; most draws stop here): a few triangles, one instance.
        let (count, instances, indexed) = match *call {
            DrawKind::Arrays {
                count, instances, ..
            } => (count, instances, false),
            DrawKind::Elements {
                count, instances, ..
            } => (count, instances, true),
        };
        if instances != 1 || !(3..=6).contains(&count) {
            decline!("{} vertices x {} instances", count, instances);
        }
        if denylist()
            .lock()
            .unwrap()
            .contains(&format!("tiny:{}:{}", vs.entry, fs.entry))
        {
            decline!("pair denied after a kernel failure");
        }
        let (tris, ntri): ([[u32; 4]; 2], u32) = match (mode, count) {
            (4, 3) => ([[0, 1, 2, 0], [0; 4]], 1),
            (4, 6) => ([[0, 1, 2, 0], [3, 4, 5, 0]], 2),
            (5, 3) => ([[0, 1, 2, 0], [0; 4]], 1),
            (5, 4) => ([[0, 1, 2, 0], [1, 3, 2, 0]], 2),
            _ => decline!("primitive mode {} with {} vertices", mode, count),
        };
        if indexed && st.prim_restart.is_some() {
            decline!("primitive restart");
        }
        // Rasterizer state the kernel does not apply: culling, non-fill modes, depth / stencil.
        // Culling is applied by the kernel (facing from the triangle's window-space winding); culling both faces draws
        // nothing and non-fill modes rasterize lines / points.
        if st.polygon & 3 == 3 || (st.polygon >> 3) & 3 != 2 {
            decline!(
                "polygon state {:#x} (front and back culled, or not fill)",
                st.polygon
            );
        }
        // A depth / stencil target the draw neither tests nor writes (depth test, depth write and stencil test off:
        // NVN depth-stencil bits 0, 1, 6) is left untouched by it, as by a kernel.
        if depth_tex.is_some() && st.depth_stencil & 0x43 != 0 {
            decline!("depth target in use (depth-stencil {:x})", st.depth_stencil);
        }
        // Targets: 2D, level 0, single sample, writable by compute, not sRGB.
        if color_texs.is_empty() {
            decline!("no color target");
        }
        let (mut w, mut h) = (u32::MAX, u32::MAX);
        let mut outputs: smallvec::SmallVec<[(usize, Tex); 8]> = smallvec::SmallVec::new();
        // (root, level, slice) written by this draw: a sampled view of the same texture is fine if it covers neither.
        let mut out_subs: smallvec::SmallVec<[(u64, usize, usize); 8]> = smallvec::SmallVec::new();
        for (i, t, rt) in color_texs {
            let pf = t.pixelFormat();
            let ty = t.textureType();
            if !matches!(
                ty,
                MTLTextureType::Type2D
                    | MTLTextureType::Type2DArray
                    | MTLTextureType::TypeCube
                    | MTLTextureType::TypeCubeArray
            ) || t.sampleCount() != 1
                || !t
                    .usage()
                    .contains(objc2_metal::MTLTextureUsage::ShaderWrite)
                || matches!(
                    pf,
                    MTLPixelFormat::RGBA8Unorm_sRGB | MTLPixelFormat::BGRA8Unorm_sRGB
                )
            {
                decline!(
                    "target c{}: {:?} {:?} level {} layer {} usage {:?}",
                    i,
                    pf,
                    ty,
                    rt.level,
                    rt.layer,
                    t.usage()
                );
            }
            if (st.color_state >> i) & 1 != 0 {
                let b = if st.blend_states[*i] == 0 {
                    DEFAULT_BLEND_STATE
                } else {
                    st.blend_states[*i]
                };
                for sh in [4, 10, 16, 22] {
                    if (16..=19).contains(&crate::nvn::state::blend_func_decode((b >> sh) & 0x3F)) {
                        decline!("dual-source blending on c{}", i);
                    }
                }
            }
            // The level and slice the render pass would draw into, as a 2D view (cached like binding views).
            let (level, slice) = (rt.level as usize, rt.layer as usize);
            if level >= t.mipmapLevelCount()
                || slice
                    >= t.arrayLength()
                        * if matches!(ty, MTLTextureType::TypeCube | MTLTextureType::TypeCubeArray)
                        {
                            6
                        } else {
                            1
                        }
            {
                decline!(
                    "target c{}: level {} / slice {} outside the texture",
                    i,
                    level,
                    slice
                );
            }
            let tv = if ty == MTLTextureType::Type2D && level == 0 && t.mipmapLevelCount() == 1 {
                t.clone()
            } else {
                match sub_view_2d(&mut st.tc.view_cache, t, level, slice) {
                    Some(v) => v,
                    None => decline!(
                        "target c{}: no 2D view of level {} slice {}",
                        i,
                        level,
                        slice
                    ),
                }
            };
            if super::tex_scale(t) > 1 {
                decline!("target c{}: scaled (gpu::scale)", i);
            }
            w = w.min(tv.width() as u32);
            h = h.min(tv.height() as u32);
            out_subs.push((
                root_id(t),
                t.parentRelativeLevel() + level,
                t.parentRelativeSlice() + slice,
            ));
            outputs.push((*i, tv));
        }
        for &loc in &fs.colors {
            if !outputs.iter().any(|(i, _)| *i == loc as usize) {
                decline!("fragment output {} has no target", loc);
            }
        }
        // Sampled textures: one level (implicit LOD becomes level 0), none of them covering a level / slice this draw writes.
        for (stage, binding, t, samp, root) in resolved {
            if let Some(t) = t {
                // Implicit-LOD samples become level 0 in the kernel: exact for one-level textures. Explicit levels and
                // gradients work on any texture; vertex-stage samples are explicit (level 0) already.
                let implicit = *stage == STAGE_FRAGMENT
                    && fs
                        .tiny_implicit
                        .as_ref()
                        .map_or(true, |l| l.contains(binding));
                // Several levels are still exact when the sampler fixes the level: no mipmapping (min filter NEAREST /
                // LINEAR reads the base level) or a pinned LOD range (the clamp decides, whatever the derivatives).
                let sd = samp.and_then(crate::nvn::api::lookup_pool_sampler);
                let fixed_level =
                    sd.is_some_and(|d| matches!(d.min_filter, 0 | 1) || d.lod_min == d.lod_max);
                // Levels (relative to the view) the samples can read: only the fixed one(s) for implicit LOD with such a
                // sampler (linear mip filtering of a fractional pinned LOD reads two), else the whole view.
                let nl = t.mipmapLevelCount();
                let (rl0, rl1) = match sd {
                    Some(d) if implicit && matches!(d.min_filter, 0 | 1) => (0, 1),
                    Some(d) if implicit && d.lod_min == d.lod_max => {
                        let l = d.lod_min.max(0.0);
                        (
                            (l.floor() as usize).min(nl - 1),
                            (l.ceil() as usize).min(nl - 1) + 1,
                        )
                    }
                    _ => (0, nl),
                };
                if implicit && t.mipmapLevelCount() != 1 && !fixed_level {
                    decline!(
                        "stage {} texture {} has {} levels",
                        stage,
                        binding,
                        t.mipmapLevelCount()
                    );
                }
                // level(0) also fixes the filter choice: automatic LOD picks the min filter when the pass shrinks its
                // source and the mag filter otherwise. Exact only if they agree (mip part ignored: one level) and no
                // anisotropy (its taps follow the derivatives).
                if let Some(d) = samp
                    .filter(|_| implicit)
                    .and_then(crate::nvn::api::lookup_pool_sampler)
                {
                    let base_min = match d.min_filter {
                        0 | 2 | 4 => 0,
                        _ => 1,
                    };
                    if base_min != d.mag_filter || d.max_anisotropy > 1.0 {
                        decline!(
                            "stage {} texture {}: sampler min {} mag {} anisotropy {}",
                            stage,
                            binding,
                            d.min_filter,
                            d.mag_filter,
                            d.max_anisotropy
                        );
                    }
                }
                let r = root.unwrap_or_else(|| root_id(t));
                let (l0, s0) = (t.parentRelativeLevel(), t.parentRelativeSlice());
                let slices = t.arrayLength()
                    * if matches!(
                        t.textureType(),
                        MTLTextureType::TypeCube | MTLTextureType::TypeCubeArray
                    ) {
                        6
                    } else {
                        1
                    };
                if out_subs.iter().any(|&(or, ol, os)| {
                    or == r && (l0 + rl0..l0 + rl1).contains(&ol) && (s0..s0 + slices).contains(&os)
                }) {
                    decline!("stage {} texture {} is a target", stage, binding);
                }
            }
        }
        if !vs.images.is_empty() || !fs.images.is_empty() {
            decline!("storage images");
        }
        if vs.textures.len() > 16 {
            decline!("vertex stage sampler table (buffer 29 would collide with stream 1)");
        }
        // Vertex streams 0 and 1 only (kernel buffers 25 / 26; the index buffer takes 26), no instance divisors.
        for &loc in &vs.attrs {
            let Some(&bits) = st.vertex_attribs.get(loc as usize) else {
                continue;
            };
            let a = decode_attr(bits);
            if vertex_format(a.format).is_none() {
                continue; // read as the default attribute
            }
            let div = st.vertex_streams.get(a.stream).map_or(0, |b| b >> 32);
            if a.stream > 1 || (indexed && a.stream != 0) || div != 0 {
                decline!("attribute {} in stream {} (divisor {})", loc, a.stream, div);
            }
        }
        // Rectangle: viewport and scissor within the targets.
        let vp = st.viewport.unwrap_or((0, 0, w as i32, h as i32));
        let sc = st.scissor.unwrap_or((0, 0, w as i32, h as i32));
        let x0 = vp.0.max(sc.0).max(0) as u32;
        let y0 = vp.1.max(sc.1).max(0) as u32;
        let x1 = ((vp.0 + vp.2).min(sc.0 + sc.2).max(0) as u32).min(w);
        let y1 = ((vp.1 + vp.3).min(sc.1 + sc.3).max(0) as u32).min(h);
        if x1 <= x0 || y1 <= y0 {
            decline!("empty rectangle");
        }
        // Large passes are slower as kernels than as tile-based fragment shading (full-resolution post passes as compute
        // take about twice the GPU time); the gain is the per-pass overhead of small ones.
        // --tiny-max-px: largest rectangle in pixels (default 640x360).
        static MAX_PX: OnceLock<u64> = OnceLock::new();
        let max_px = *MAX_PX.get_or_init(|| crate::settings::get().graphics.tiny_max_px);
        if ((x1 - x0) as u64) * ((y1 - y0) as u64) > max_px {
            decline!("rectangle {}x{} above --tiny-max-px", x1 - x0, y1 - y0);
        }
        let cb = st.cur_cb.clone()?;
        // In order with the passes around it. Only clears of textures this draw writes or samples go first: the others
        // stay pending, so runs of clears separated by tiny draws can still share passes (flush_clears_matching).
        self.end_encoder_keep_clears(st);
        // A pending clear of an output the rectangle covers entirely is folded into the kernel (uncovered pixels get the
        // clear colour, blending and write masks see it as the destination): the clear needs no pass of its own.
        // Float outputs, kernels built with folded clears (index `tkc`).
        let mut clears: smallvec::SmallVec<[(usize, [f32; 4]); 8]> = smallvec::SmallVec::new();
        if fs.tiny_clears {
            for (i, t, rt) in color_texs {
                let (level, layer) = (rt.level as usize, rt.layer as usize);
                let (vw, vh) = (
                    ((t.width() >> level).max(1)) as u32,
                    ((t.height() >> level).max(1)) as u32,
                );
                if (x0, y0, x1, y1) != (0, 0, vw, vh)
                    || pf_kind(t.pixelFormat()) != 0
                    || !fs.colors.contains(&(*i as u32))
                {
                    continue;
                }
                if let Some(k) = st.pending_clears.iter().position(|c| {
                    c.is_target(t, level, layer) && matches!(c.kind, super::ClearKind::Color(_))
                }) {
                    if let super::ClearKind::Color(rgba) = st.pending_clears.remove(k).kind {
                        clears.push((*i, rgba));
                        super::CLEARS_FOLDED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        }
        let mut touched: smallvec::SmallVec<[u64; 16]> =
            out_subs.iter().map(|&(r, _, _)| r).collect();
        touched.extend(
            resolved
                .iter()
                .filter_map(|(_, _, t, _, r)| r.or_else(|| t.as_ref().map(root_id))),
        );
        self.flush_clears_matching(st, |c| touched.contains(&c.root));
        let cb = st.cur_cb.clone().unwrap_or(cb);
        for (_, t) in &outputs {
            st.tc.mark_gpu_written(super::storage_id(t));
        }
        super::TINY_COMPUTE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(TinyPlan {
            cb,
            outputs,
            tris,
            ntri,
            nverts: count as u32,
            rect: (x0, y0, x1 - x0, y1 - y0),
            clears,
        })
    }

    /// The compute version of a draw encode_draw has prepared (bindings recorded in `rec`): the
    /// vertex kernel over the draw's vertices into vertex records, then the fragment kernel over the rectangle, in one
    /// compute encoder (serial dispatch: the second starts after the first completes).
    #[allow(clippy::too_many_arguments)]
    pub(crate) unsafe fn emit_tiny(
        &self,
        env: &mut EncodeEnv,
        p: &DrawPacket,
        tp: &TinyPlan,
        rec: TinyRec,
        vs: &CachedStage,
        fs: Option<&CachedStage>,
        attr_descs: &[(u32, MTLVertexFormat, usize, usize, i32)],
        streams_eff: &[u64],
        depth_gl: bool,
        point_size: f32,
    ) -> bool {
        let Some(fs) = fs else { return false };
        let Some(lib) = &self.shader_lib else {
            return false;
        };
        let enc = env.enc.clone();
        let fail = |what: String| {
            denylist()
                .lock()
                .unwrap()
                .insert(format!("tiny:{}:{}", vs.entry, fs.entry));
            warn_once(
                format!("tiny-fail-{}-{}", vs.entry, fs.entry),
                format!(
                    "tiny pass {} / {}: {} (pair renders from now on; this draw is lost)",
                    vs.entry, fs.entry, what
                ),
            );
            false
        };
        // Pipelines: the kernels with the function constants the render pipeline would get (keys: tiny_keys).
        let (vkey, fkey) = tiny_keys(vs, fs, attr_descs, depth_gl, point_size, &p.color_pfs);
        // A kernel this thread has not made yet: list the draw's pipeline as one that runs as a tiny pass, so the next
        // session (and other players) make its kernels before the draw (queue_tiny).
        let vk = (vs as *const CachedStage as usize, vkey.clone());
        let fk = (fs as *const CachedStage as usize, fkey.clone());
        if !env.sh.tiny_pipes.contains_key(&vk) || !env.sh.tiny_pipes.contains_key(&fk) {
            let blends = p.blend_states.map(|b| if b == 0 { DEFAULT_BLEND_STATE } else { b });
            record_pipeline(vs, Some(fs), p, point_size, depth_gl, &blends, true);
        }
        let pipe = |sh: &mut SharedCaches, stage: &CachedStage, key: Vec<i64>| -> Result<ComputePipe, String> {
            let k = (stage as *const CachedStage as usize, key);
            if let Some(p) = sh.tiny_pipes.get(&k) {
                return Ok(p.clone());
            }
            let p = tiny_cell(&stage.entry, &k.1).0.get_or_init(|| self.make_tiny(lib, &stage.entry, &k.1)).clone()?;
            sh.tiny_pipes.insert(k, p.clone());
            Ok(p)
        };
        let kv = match pipe(&mut *env.sh, vs, vkey) {
            Ok(p) => p,
            Err(e) => return fail(e),
        };
        let kf = match pipe(&mut *env.sh, fs, fkey) {
            Ok(p) => p,
            Err(e) => return fail(e),
        };
        // Vertex kernel parameters (tk_VParams).
        let mut vparams = [0u32; 8 + 64];
        vparams[0] = tp.nverts;
        let (index_type, index_addr) = match p.call {
            DrawKind::Arrays {
                first,
                base_instance,
                ..
            } => {
                vparams[1] = first.max(0) as u32;
                vparams[4] = base_instance.max(0) as u32;
                (0u32, 0u64)
            }
            DrawKind::Elements {
                index_type,
                index_addr,
                base_vertex,
                base_instance,
                ..
            } => {
                vparams[3] = base_vertex as u32;
                vparams[4] = base_instance.max(0) as u32;
                (
                    match index_type {
                        0 => 1,
                        1 => 2,
                        _ => 4,
                    },
                    index_addr,
                )
            }
        };
        vparams[2] = index_type;
        for (loc, fmt, buf, off, _) in attr_descs {
            let o = 8 + 4 * (*loc as usize).min(15);
            if *buf == DEFAULT_ATTR_BUFFER || *buf > VB_BASE || VB_BASE - *buf > 1 {
                vparams[o + 1] = 0; // the default attribute (0, 0, 0, 1)
                continue;
            }
            let stream = VB_BASE - *buf;
            vparams[o] = stream as u32;
            vparams[o + 1] = fmt.0 as u32;
            vparams[o + 2] = *off as u32;
            vparams[o + 3] = (streams_eff.get(stream).copied().unwrap_or(0) & 0xFFFF_FFFF) as u32;
        }
        let as_bytes =
            |v: &[u32]| std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4).to_vec();
        let Some(vp_buf) = env.cx.arena.alloc(&self.device, &as_bytes(&vparams)) else {
            return fail("arena".into());
        };
        let rec_len = tp.nverts as usize * 33 * 16;
        let Some(records) = env
            .cx
            .arena
            .alloc_with(&self.device, rec_len, |b| b.fill(0))
        else {
            return fail("arena".into());
        };
        let ib = if index_type != 0 {
            let bytes = index_type as usize;
            match self.guest_buffer_ro(
                &mut *env.sh,
                &mut *env.cx,
                index_addr,
                tp.nverts as usize * bytes,
            ) {
                Some(x) => Some(x),
                None => return fail("index buffer".into()),
            }
        } else {
            None
        };
        // Fragment kernel parameters (tk_FParams).
        let (tw, th) = p.target_size;
        let (vx, vy, vw, vh) = p
            .viewport
            .map(|(x, y, w, h)| (x as f32, y as f32, w as f32, h as f32))
            .unwrap_or((0.0, 0.0, tw as f32, th as f32));
        // tk_FParams in u32 words (MSL alignment: uint4 members start on 16 bytes): viewport 0..4, depth_range 4..6,
        // origin 6..8, size 8..10, ntri 10, masks 11, tri 12..20, blend 20..52, blend_color 52..56, raster 56..60,
        // clear_mask 60..64, clear_color 64..96.
        let mut fparams = [0u32; 96];
        for &(c, rgba) in &tp.clears {
            fparams[60] |= 1 << c;
            for (k, v) in rgba.iter().enumerate() {
                fparams[64 + 4 * c + k] = v.to_bits();
            }
        }
        // Raster state (tk_FParams.raster): cull mode and Metal's front-face winding as encode_draw sets it.
        fparams[56] = match p.polygon & 3 {
            1 => 1,
            2 => 2,
            _ => 0,
        };
        fparams[57] = (((p.polygon >> 2) & 1 == 1) == y_negate()) as u32;
        for (i, v) in [vx, vy, vw, vh, p.depth_range.0, p.depth_range.1]
            .iter()
            .enumerate()
        {
            fparams[i] = v.to_bits();
        }
        let (rx, ry, rw, rh) = tp.rect;
        fparams[6..10].copy_from_slice(&[rx, ry, rw, rh]);
        fparams[10] = tp.ntri;
        let mut masks = 0u32;
        for i in 0..8 {
            if fs.colors.contains(&(i as u32)) {
                masks |= ((p.channel_mask >> (i * 4)) & 0xF) << (i * 4);
            }
        }
        fparams[11] = masks;
        for t in 0..2 {
            fparams[12 + 4 * t..16 + 4 * t].copy_from_slice(&tp.tris[t]);
        }
        for i in 0..8 {
            if (p.color_state >> i) & 1 == 0
                || p.color_pfs[i] == MTLPixelFormat::Invalid
                || pf_kind(p.color_pfs[i]) != 0
            {
                continue;
            }
            let b = if p.blend_states[i] == 0 {
                DEFAULT_BLEND_STATE
            } else {
                p.blend_states[i]
            };
            let f = |sh: u64| super::nvn_blend_factor(((b >> sh) & 0x3F) as u32).0 as u32;
            let o = |sh: u64| super::nvn_blend_op(((b >> sh) & 0xF) as u32).0 as u32;
            let unorm = !matches!(
                p.color_pfs[i],
                MTLPixelFormat::RGBA16Float
                    | MTLPixelFormat::RG16Float
                    | MTLPixelFormat::R16Float
                    | MTLPixelFormat::RGBA32Float
                    | MTLPixelFormat::RG32Float
                    | MTLPixelFormat::R32Float
                    | MTLPixelFormat::RG11B10Float
                    | MTLPixelFormat::RGB9E5Float
            );
            fparams[20 + 4 * i..24 + 4 * i].copy_from_slice(&[
                1,
                f(4) | f(10) << 8 | f(16) << 16 | f(22) << 24,
                o(28) | o(32) << 8,
                unorm as u32,
            ]);
        }
        for (i, v) in p.blend_color.iter().enumerate() {
            fparams[52 + i] = v.to_bits();
        }
        let Some(fp_buf) = env.cx.arena.alloc(&self.device, &as_bytes(&fparams)) else {
            return fail("arena".into());
        };
        // One compute encoder: vertex kernel, then fragment kernel.
        enc.push(Op::CJoin(
            tp.cb.clone(),
            format!("tiny {} / {}", vs.entry, fs.entry),
            !rec.uses.is_empty(),
        ));
        enc.push(Op::CPipeline(kv));
        if crate::settings::get().log.log_tiny_bufs {
            use objc2_metal::MTLResource as _;
            for (b, off, i) in rec.vbufs.iter().chain(rec.fbufs.iter()) {
                println!("[tiny-bufs] {} / {} index {} len {:#x} off {:#x} gpu {:#x} cpu {:p} storage {:?}", vs.entry, fs.entry, i,
                    b.length(), off, b.gpuAddress(), b.contents().as_ptr(), b.storageMode());
            }
        }
        for (b, off, i) in rec.vbufs {
            let i = if (VB_BASE - 1..=VB_BASE).contains(&i) {
                25 + (VB_BASE - i)
            } else {
                i
            };
            if i == 26 && ib.is_some() {
                continue; // stream 1 unused when indexed (tiny_plan); slot 26 is the index buffer
            }
            enc.push(Op::CBuf(b, off, i));
        }
        if let Some((b, off)) = &ib {
            enc.push(Op::CBuf(b.clone(), *off, 26));
        }
        for x in &rec.uses {
            enc.push(Op::CUse(x.clone(), objc2_metal::MTLResourceUsage::Read));
        }
        for (t, i) in rec.vtex {
            enc.push(Op::CTex(t, i));
        }
        for (smp, i) in rec.vsamp {
            enc.push(Op::CSamp(smp, i));
        }
        enc.push(Op::CBuf(vp_buf.0, vp_buf.1, 27));
        enc.push(Op::CBuf(records.0.clone(), records.1, 28));
        enc.push(Op::CDispatch(
            MTLSize {
                width: 1,
                height: 1,
                depth: 1,
            },
            MTLSize {
                width: tp.nverts as usize,
                height: 1,
                depth: 1,
            },
        ));
        enc.push(Op::CPipeline(kf));
        for (b, off, i) in rec.fbufs {
            enc.push(Op::CBuf(b, off, i));
        }
        for (t, i) in rec.ftex {
            enc.push(Op::CTex(t, i));
        }
        for (smp, i) in rec.fsamp {
            enc.push(Op::CSamp(smp, i));
        }
        for (c, t) in &tp.outputs {
            enc.push(Op::CTex(t.clone(), 24 + c));
            enc.push(Op::CTex(t.clone(), 32 + c));
        }
        enc.push(Op::CBuf(fp_buf.0, fp_buf.1, 27));
        enc.push(Op::CBuf(records.0, records.1, 28));
        enc.push(Op::CDispatch(
            MTLSize {
                width: (rw as usize + 7) / 8,
                height: (rh as usize + 7) / 8,
                depth: 1,
            },
            MTLSize {
                width: 8,
                height: 8,
                depth: 1,
            },
        ));
        enc.push(Op::CLinger);
        true
    }
}
pub(crate) type TinyCell = std::sync::Arc<OnceLock<Result<ComputePipe, String>>>;

/// A tiny-pass kernel, shared by draws and the precompile like compute_cell; true when the key is new.
pub(crate) fn tiny_cell(entry: &str, key: &[i64]) -> (TinyCell, bool) {
    static M: OnceLock<Mutex<HashMap<(String, Vec<i64>), TinyCell>>> = OnceLock::new();
    let mut m = M.get_or_init(Default::default).lock().unwrap();
    if let Some(c) = m.get(&(entry.to_string(), key.to_vec())) {
        return (c.clone(), false);
    }
    let c = TinyCell::default();
    m.insert((entry.to_string(), key.to_vec()), c.clone());
    (c, true)
}

/// The keys of a draw's two tiny-pass kernels: [0, depth_gl, y_negate, point size bits, (location, attribute kind)..]
/// for the vertex kernel, [1, (location, colour output kind).., (100 + location, the vertex stage writes it)..] for the
/// fragment kernel (Gfx::make_tiny turns them into function constants).
pub(crate) fn tiny_keys(
    vs: &CachedStage,
    fs: &CachedStage,
    attr_descs: &[(u32, MTLVertexFormat, usize, usize, i32)],
    depth_gl: bool,
    point_size: f32,
    color_pfs: &[MTLPixelFormat],
) -> (Vec<i64>, Vec<i64>) {
    let mut vkey: Vec<i64> = vec![0, depth_gl as i64, y_negate() as i64, point_size.to_bits() as i64];
    for (loc, _, _, _, kind) in attr_descs {
        vkey.extend([*loc as i64, *kind as i64]);
    }
    let mut fkey: Vec<i64> = vec![1];
    for &loc in &fs.colors {
        fkey.extend([loc as i64, pf_kind(color_pfs.get(loc as usize).copied().unwrap_or(MTLPixelFormat::Invalid)) as i64]);
    }
    for &loc in &fs.varyings {
        fkey.extend([100 + loc as i64, vs.varyings.contains(&loc) as i64]);
    }
    (vkey, fkey)
}

/// Queues the tiny-pass kernels a listed pipeline's draws would run (when its stages have them), so a first launch
/// does not make them at the draw.
pub(crate) fn queue_tiny(
    vs: &'static CachedStage,
    fs: Option<&'static CachedStage>,
    attr_descs: &[(u32, MTLVertexFormat, usize, usize, i32)],
    depth_gl: bool,
    point_size: f32,
    color_pfs: &[MTLPixelFormat],
) {
    let Some(fs) = fs else { return };
    let heap_samplers = |c: &CachedStage| c.texture_heap && !c.textures.is_empty() && !c.tiny_samplers;
    if !tiny_enabled() || !vs.tiny_kernel || !fs.tiny_kernel || heap_samplers(vs) || heap_samplers(fs) {
        return;
    }
    let (vkey, fkey) = tiny_keys(vs, fs, attr_descs, depth_gl, point_size, color_pfs);
    let jobs: Vec<_> = [(vs, vkey), (fs, fkey)]
        .into_iter()
        .filter_map(|(st, k)| tiny_cell(&st.entry, &k).1.then_some((st.entry.as_str(), k)))
        .collect();
    if jobs.is_empty() {
        return;
    }
    let (q, cv) = pipe_queue();
    let mut g = q.lock().unwrap();
    g.compute_n += jobs.len();
    crate::gpu::pipeline_list::listed_queued(jobs.len());
    g.tiny.extend(jobs);
    drop(g);
    cv.notify_all();
}

/// Makes one queued tiny-pass kernel (or finds it made by a draw).
pub(crate) fn build_tiny(entry: &str, key: &[i64]) {
    let t0 = std::time::Instant::now();
    if let Some(g) = super::GFX.get() {
        if let Some(lib) = g.shader_lib.as_ref() {
            let _ = tiny_cell(entry, key).0.get_or_init(|| g.make_tiny(lib, entry, key));
        }
    }
    kernel_done(t0);
}

/// A draw the tiny-pass path runs as compute: the command buffer, its color outputs (color index, target texture) and
/// the triangles over its vertex records.
pub(crate) struct TinyPlan {
    pub(crate) cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    pub(crate) outputs: smallvec::SmallVec<[(usize, Tex); 8]>,
    pub(crate) tris: [[u32; 4]; 2],
    pub(crate) ntri: u32,
    pub(crate) nverts: u32,
    /// Dispatch rectangle: x, y, width, height (viewport and scissor within the targets).
    pub(crate) rect: (u32, u32, u32, u32),
    /// Pending clears folded into the kernel: (colour output, clear colour).
    pub(crate) clears: smallvec::SmallVec<[(usize, [f32; 4]); 8]>,
}

/// Tiny passes as compute: on by default; --tiny-compute=0 renders them. Without a cache built with kernels (index lines `tk`) no draw qualifies.
pub(crate) fn tiny_compute_on() -> bool {
    static F: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *F.get_or_init(|| crate::settings::get().graphics.tiny_compute)
}

pub(crate) fn tiny_enabled() -> bool {
    static AB: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    tiny_compute_on() && !(*AB.get_or_init(|| crate::settings::get().replay.replay_ab_tiny) && super::super::replay_b())
}

pub(crate) fn tiny_reason(r: String) {
    if super::tracing_frame() {
        TINY_REASON.with(|x| *x.borrow_mut() = r);
    }
}
