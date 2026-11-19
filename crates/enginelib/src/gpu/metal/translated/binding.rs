// Textures as draws bind them: handles resolved to Metal textures and views, depth twins for shadow compares,
// dummy textures, render-target feedback, tile maps, render-target format views and BC proxies.

use super::*;

impl Gfx {
    /// The depth twin of colour texture `t` for a binding that samples `v` (a view of `t`) with depth comparison: its
    /// memory read as R32F -> Depth32Float, R16 -> Depth16Unorm (layout::depth_twin), what the console compares. Filled
    /// from guest memory (shared storage, as a CPU-written texture is), again when the object's generation changes. None
    /// for other formats, or a colour texture the GPU wrote (guest memory does not hold its contents: the caller binds
    /// a dummy, as before).
    /// Copies GPU-written colour textures into their depth twins (depth_twin) that newer writes left out of date, between
    /// passes: R16Unorm / R32Float hold the bytes of Depth16Unorm / Depth32Float, and Metal blits colour into depth only
    /// through a buffer.
    pub(crate) unsafe fn refresh_depth_twins(&self, st: &mut State) {
        let pending = std::mem::take(&mut st.tc.twin_pending);
        let Some(cb) = st.cur_cb.clone() else { return };
        if st.enc.is_some() {
            self.end_encoder_keep_clears(st);
        }
        // Queued on the issuer thread after the encoder work before it (as copies are, copy.rs): waiting for that work
        // here (issuer.sync) stalled BotW's Presentation Thread, ~6% of it, inside the game's render lock.
        let mut copies = Vec::new();
        for (src, twin, key) in pending {
            let bpp = match src.pixelFormat() {
                MTLPixelFormat::R16Unorm => 2,
                MTLPixelFormat::R32Float => 4,
                _ => continue,
            };
            if (src.width(), src.height(), src.arrayLength(), src.mipmapLevelCount(), src.textureType())
                != (twin.width(), twin.height(), twin.arrayLength(), twin.mipmapLevelCount(), twin.textureType())
            {
                continue;
            }
            let slices = src.arrayLength().max(1)
                * if matches!(src.textureType(), MTLTextureType::TypeCube | MTLTextureType::TypeCubeArray) { 6 } else { 1 };
            let levels = src.mipmapLevelCount().max(1);
            let dims = |l: usize| ((src.width() >> l).max(1), (src.height() >> l).max(1));
            let total: usize = (0..levels).map(|l| dims(l).0 * dims(l).1 * bpp * slices).sum();
            let Some(buf) = self.device.newBufferWithLength_options(total, MTLResourceOptions::StorageModePrivate) else {
                continue;
            };
            let seq = st.tc.gpu_write_seq.get(&key).copied().unwrap_or(0);
            if let Some(e) = st.tc.gpu_twins.get_mut(&key) {
                e.1 = seq;
            }
            copies.push((src, twin, buf, bpp, slices, levels));
        }
        if copies.is_empty() {
            return;
        }
        st.issuer.push(super::issuer::Op::Blit {
            cb: cb.retain(),
            label: "depth twin copy",
            f: Box::new(move |blit| {
                use objc2_metal::MTLBlitCommandEncoder;
                for (src, twin, buf, bpp, slices, levels) in &copies {
                    let dims = |l: usize| ((src.width() >> l).max(1), (src.height() >> l).max(1));
                    let mut off = 0;
                    for level in 0..*levels {
                        let (w, h) = dims(level);
                        let (row, image) = (w * bpp, w * h * bpp);
                        for slice in 0..*slices {
                            let size = MTLSize { width: w, height: h, depth: 1 };
                            let origin = MTLOrigin { x: 0, y: 0, z: 0 };
                            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                                src, slice, level, origin, size, buf, off, row, image,
                            );
                            blit.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                                buf, off, row, image, size, twin, slice, level, origin,
                            );
                            off += image;
                        }
                    }
                }
            }),
        });
        self.note_encoder(st, "depth twin copy (blit)");
    }

    pub(crate) unsafe fn depth_twin(&self, tc: &mut TexCache, t: &Tex, v: &Tex, gen: u64, snap: &crate::nvn::api::NvnTexture) -> Option<Tex> {
        let pf = match crate::nvn::layout::depth_twin(snap.format) {
            Some(52) => MTLPixelFormat::Depth32Float,
            Some(50) => MTLPixelFormat::Depth16Unorm,
            _ => return None,
        };
        let key = super::storage_id(t);
        let twin = if tc.gpu_written.contains(&key) {
            // GPU-written: guest memory does not hold it, so the twin is a GPU copy of it (refresh_depth_twins), made
            // again before a draw that samples it after newer writes.
            let seq = tc.gpu_write_seq.get(&key).copied().unwrap_or(0);
            let tw = match tc.gpu_twins.get(&key) {
                Some((tw, _)) => tw.clone(),
                None => {
                    let desc = MTLTextureDescriptor::new();
                    desc.setTextureType(t.textureType());
                    desc.setPixelFormat(pf);
                    desc.setWidth(t.width());
                    desc.setHeight(t.height());
                    desc.setDepth(t.depth());
                    desc.setArrayLength(t.arrayLength());
                    desc.setMipmapLevelCount(t.mipmapLevelCount());
                    desc.setUsage(MTLTextureUsage::ShaderRead);
                    desc.setStorageMode(MTLStorageMode::Private);
                    let tw = self.device.newTextureWithDescriptor(&desc)?;
                    tc.gpu_twins.insert(key, (tw.clone(), u64::MAX));
                    tw
                }
            };
            if tc.gpu_twins.get(&key).is_some_and(|(_, s)| *s != seq) && !tc.twin_pending.iter().any(|p| p.2 == key) {
                tc.twin_pending.push((t.clone(), tw.clone(), key));
            }
            tc.twin_uses += 1;
            tw
        } else {
            match tc.depth_twins.get(&key) {
            Some((tw, g)) if *g == gen => tw.clone(),
            _ => {
                let desc = MTLTextureDescriptor::new();
                desc.setTextureType(t.textureType());
                desc.setPixelFormat(pf);
                desc.setWidth(t.width());
                desc.setHeight(t.height());
                desc.setDepth(t.depth());
                desc.setArrayLength(t.arrayLength());
                desc.setMipmapLevelCount(t.mipmapLevelCount());
                desc.setUsage(MTLTextureUsage::ShaderRead);
                desc.setStorageMode(MTLStorageMode::Shared);
                let tw = self.device.newTextureWithDescriptor(&desc)?;
                self.upload_texture(&tw, snap);
                tc.depth_twins.insert(key, (tw.clone(), gen));
                tw
            }
            }
        };
        // The binding's levels and layers of the twin.
        if v.mipmapLevelCount() == twin.mipmapLevelCount() && v.arrayLength() == twin.arrayLength() && v.textureType() == twin.textureType() {
            return Some(twin);
        }
        let (level, slice) = if v.parentTexture().is_some() { (v.parentRelativeLevel(), v.parentRelativeSlice()) } else { (0, 0) };
        let slices = if matches!(v.textureType(), MTLTextureType::TypeCube | MTLTextureType::TypeCubeArray) { v.arrayLength() * 6 } else { v.arrayLength() };
        twin.newTextureViewWithPixelFormat_textureType_levels_slices(
            pf,
            v.textureType(),
            objc2_foundation::NSRange::new(level, v.mipmapLevelCount()),
            objc2_foundation::NSRange::new(slice, slices),
        )
    }

    /// A 1x1 texture of `ty` to bind when the game's texture can't be expressed (yet).
    pub(crate) unsafe fn dummy_texture(&self, tc: &mut TexCache, ty: &str) -> Option<Tex> {
        if let Some(t) = tc.dummy_textures.get(ty) {
            return Some(t.clone());
        }
        let base = ty.split('+').next().unwrap_or("Texture2D");
        let array = ty.contains("+Array");
        let shadow = ty.contains("+Shadow");
        let (tt, depth) = match (base, array) {
            ("Texture1D", false) => (MTLTextureType::Type1D, 1),
            ("Texture1D", true) => (MTLTextureType::Type1DArray, 1),
            ("Texture3D", _) => (MTLTextureType::Type3D, 1),
            ("TextureCube", false) => (MTLTextureType::TypeCube, 1),
            ("TextureCube", true) => (MTLTextureType::TypeCubeArray, 1),
            (_, true) => (MTLTextureType::Type2DArray, 1),
            _ => (MTLTextureType::Type2D, 1),
        };
        let pf = if shadow {
            MTLPixelFormat::Depth32Float
        } else {
            MTLPixelFormat::RGBA8Unorm
        };
        let desc = MTLTextureDescriptor::new();
        desc.setTextureType(tt);
        desc.setPixelFormat(pf);
        desc.setWidth(1);
        desc.setHeight(
            if matches!(tt, MTLTextureType::Type1D | MTLTextureType::Type1DArray) {
                1
            } else {
                1
            },
        );
        if tt == MTLTextureType::Type3D {
            desc.setDepth(depth);
        }
        if matches!(
            tt,
            MTLTextureType::Type2DArray
                | MTLTextureType::Type1DArray
                | MTLTextureType::TypeCubeArray
        ) {
            desc.setArrayLength(if tt == MTLTextureType::TypeCubeArray {
                1
            } else {
                1
            });
        }
        desc.setUsage(MTLTextureUsage::ShaderRead);
        desc.setStorageMode(if shadow {
            MTLStorageMode::Private
        } else {
            MTLStorageMode::Shared
        });
        let t = self.device.newTextureWithDescriptor(&desc)?;
        if !shadow {
            let px = [255u8; 4];
            let faces = if matches!(tt, MTLTextureType::TypeCube | MTLTextureType::TypeCubeArray) {
                6
            } else {
                1
            };
            for slice in 0..faces {
                unsafe {
                    t.replaceRegion_mipmapLevel_slice_withBytes_bytesPerRow_bytesPerImage(
                        MTLRegion {
                            origin: MTLOrigin { x: 0, y: 0, z: 0 },
                            size: MTLSize {
                                width: 1,
                                height: 1,
                                depth: 1,
                            },
                        },
                        0,
                        slice,
                        std::ptr::NonNull::new(px.as_ptr() as *mut libc::c_void).unwrap(),
                        4,
                        0,
                    );
                }
            }
        }
        tc.dummy_textures.insert(ty.to_string(), t.clone());
        Some(t)
    }

    /// The Metal texture a stage's texture binding should read, following the handle the game bound.
    /// A memo hit in a new frame still owes the texture its once-per-frame CPU-write check (refresh_cpu_written).
    pub(crate) unsafe fn bind_memo_frame_check(
        &self,
        memo: &mut BindMemos,
        tca: &mut impl TcAccess,
        key: (i32, usize),
    ) {
        let frame = super::presented_frames();
        let Some(m) = memo.get_mut(&key) else { return };
        if m.frame == frame {
            return;
        }
        m.frame = frame;
        let tex_addr = m.tex_addr;
        let info = super::tex_info(tex_addr);
        let rm = (info.map(|(g, _)| g), frame);
        tca.with(|tc| {
            if tc.refresh_memo.get(&tex_addr) != Some(&rm) {
                self.refresh_cpu_written_with(tc, tex_addr, info.map(|(_, s)| s));
                tc.refresh_memo.insert(tex_addr, rm);
            }
        });
    }

    pub(crate) unsafe fn bound_texture(
        &self,
        st: &mut State,
        stage: i32,
        tb: &TexBinding,
    ) -> (Option<Tex>, Option<i32>) {
        let (t, s, _) = self.bound_texture_root(st, stage, tb);
        (t, s)
    }

    /// bound_texture plus the texture's root id (from the binding memo when it hits).
    pub(crate) unsafe fn bound_texture_root(
        &self,
        st: &mut State,
        stage: i32,
        tb: &TexBinding,
    ) -> (Option<Tex>, Option<i32>, Option<u64>) {
        // Handle table entry k lives at byte 0x20 + 8k of the stage's cbuf 2 (see module docs).
        let k = tb.handle_index.wrapping_sub(8) / 2;
        let handle = st.textures_bound.get(&(stage, k as i32)).copied();
        let r = self.bound_texture_handle(&mut st.cx.bind_memo, &mut &mut st.tc, handle, tb);
        if super::journal_on() {
            if let (Some(h), Some(t)) = (handle, &r.0) {
                journal_stale_sample(h, t);
            }
        }
        r
    }

    /// bound_texture_root for a handle the caller looked up (a DrawPacket's copy of the binding).
    pub(crate) unsafe fn bound_texture_handle(
        &self,
        memo: &mut BindMemos,
        tca: &mut impl TcAccess,
        handle: Option<u64>,
        tb: &TexBinding,
    ) -> (Option<Tex>, Option<i32>, Option<u64>) {
        let Some(handle) = handle else {
            return (tca.with(|tc| self.dummy_texture(tc, &tb.ty)), None, None);
        };
        let tex_id = (handle & 0xFFFFF) as i32;
        let sampler_id = (handle >> 20) as i32;
        let want = MTLTextureType(tb.declared as _);
        // Bindings repeat thousands of times per submit: reuse this submit's resolution of the same pool texture while
        // nothing it depends on changed (see BindMemo). --no-bind-memo disables.
        let memo_key = (tex_id, tb.declared);
        // No texture object or pool slot changed since the memo was made: its lookups would find the same objects.
        let table_epoch = crate::nvn::api::tex_table_epoch();
        {
            if let Some(m) = memo.get(&memo_key) {
                if m.table_epoch == table_epoch
                    && m.epoch == super::tex_epoch()
                    && m.dirty_gen == crate::nvn::api::texture_dirty_gen()
                {
                    if let Some(t) = m.result.clone() {
                        let root = m.root;
                        super::BIND_STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        self.bind_memo_frame_check(memo, tca, memo_key);
                        return (Some(t), Some(sampler_id), root);
                    }
                }
            }
        }
        let pool = crate::nvn::api::lookup_pool_texture(tex_id);
        {
            if let (Some((tex_addr, view)), Some(m)) = (pool.as_ref(), memo.get(&memo_key)) {
                let vb = view
                    .map(|v| std::mem::transmute::<crate::nvn::api::NvnTextureView, [u8; 16]>(v));
                // Another texture's texel writes move the global counter: only an upload pending for this one (or its
                // alias root) invalidates the memo.
                let dirty_now = crate::nvn::api::texture_dirty_gen();
                let dirty_ok = m.dirty_gen == dirty_now
                    || (!crate::nvn::api::texture_dirty_pending(*tex_addr)
                        && m.alias_root
                            .map_or(true, |(r, _)| !crate::nvn::api::texture_dirty_pending(r)));
                if m.tex_addr == *tex_addr
                    && m.view == vb
                    && m.epoch == super::tex_epoch()
                    && dirty_ok
                    && m.gen == super::tex_info(*tex_addr).map(|(g, _)| g)
                    && m.alias_root.map_or(true, |(r, g)| {
                        super::tex_info(r).map(|(rg, _)| rg) == Some(g)
                    })
                {
                    if let Some(t) = m.result.clone() {
                        let root = m.root;
                        super::BIND_STATS[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        // Still valid under the current tables.
                        if let Some(m) = memo.get_mut(&memo_key) {
                            m.table_epoch = table_epoch;
                            m.dirty_gen = dirty_now;
                        }
                        self.bind_memo_frame_check(memo, tca, memo_key);
                        return (Some(t), Some(sampler_id), root);
                    }
                }
            }
        }
        super::BIND_STATS[2].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if crate::settings::get().log.log_bind_miss {
            // Why the memo failed: [no memo, no pool slot, other object, other view, cache epoch, dirty, generation, alias root]
            static WHY: [std::sync::atomic::AtomicU64; 8] =
                [const { std::sync::atomic::AtomicU64::new(0) }; 8];
            let i = match (pool.as_ref(), memo.get(&memo_key)) {
                (_, None) => 0,
                (None, _) => 1,
                (Some((a, v)), Some(m)) => {
                    let vb = v.map(|v| {
                        std::mem::transmute::<crate::nvn::api::NvnTextureView, [u8; 16]>(v)
                    });
                    if m.tex_addr != *a {
                        2
                    } else if m.view != vb {
                        3
                    } else if m.epoch != super::tex_epoch() {
                        4
                    } else if m.dirty_gen != crate::nvn::api::texture_dirty_gen() {
                        5
                    } else if m.gen != super::tex_info(*a).map(|(g, _)| g) {
                        6
                    } else {
                        7
                    }
                }
            };
            WHY[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 200_000 == 199_999 {
                println!("[bind-miss] none {} no-slot {} object {} view {} epoch {} dirty {} gen {} alias {}", WHY[0].load(std::sync::atomic::Ordering::Relaxed), WHY[1].load(std::sync::atomic::Ordering::Relaxed), WHY[2].load(std::sync::atomic::Ordering::Relaxed), WHY[3].load(std::sync::atomic::Ordering::Relaxed), WHY[4].load(std::sync::atomic::Ordering::Relaxed), WHY[5].load(std::sync::atomic::Ordering::Relaxed), WHY[6].load(std::sync::atomic::Ordering::Relaxed), WHY[7].load(std::sync::atomic::Ordering::Relaxed));
            }
        }
        let dirty_before = crate::nvn::api::texture_dirty_gen();
        let epoch_before = super::tex_epoch();
        let twins_before = tca.with(|tc| tc.twin_uses);
        let (resolved, alias_root) = tca.with(|tc| {
            let resolved = pool.and_then(|(tex_addr, view)| {
                let t = self.texture(tc, tex_addr)?;
                let info = super::tex_info(tex_addr);
                // The CPU-write check runs once per frame per texture object (generation): skip the call itself after that
                // (bindings repeat thousands of times per frame and the call's own early exit was not free).
                let memo = (info.map(|(g, _)| g), super::presented_frames());
                if tc.refresh_memo.get(&tex_addr) != Some(&memo) {
                    self.refresh_cpu_written_with(tc, tex_addr, info.map(|(_, s)| s));
                    if tc.refresh_memo.len() > 1 << 16 {
                        tc.refresh_memo.clear();
                    }
                    tc.refresh_memo.insert(tex_addr, memo);
                }
                let tex_swizzle = info.map_or([2, 3, 4, 5], |(_, snap)| snap.swizzle);
                let v = texture_for_binding(&mut tc.view_cache, &t, view.as_ref(), want, tex_swizzle);
                // Depth comparison of a colour texture: its memory read as depth (layout::depth_twin).
                match (tb.shadow, v, info) {
                    (true, Some(v), Some((gen, snap))) => self.depth_twin(tc, &t, &v, gen, &snap).or(Some(v)),
                    (_, v, _) => v,
                }
            });
            let alias_root = pool.and_then(|(a, _)| tc.alias_of.get(&a).copied());
            (resolved, alias_root)
        });
        let ok = resolved.as_ref().map_or(false, |t| {
            // A shadow (depth-compare) binding must be a depth texture; anything else is a type error Metal rejects.
            !tb.shadow
                || matches!(
                    t.pixelFormat(),
                    MTLPixelFormat::Depth32Float
                        | MTLPixelFormat::Depth16Unorm
                        | MTLPixelFormat::Depth32Float_Stencil8
                        | MTLPixelFormat::Depth24Unorm_Stencil8
                )
        });
        if ok {
            if let Some((tex_addr, view)) = pool {
                // Only a resolution made without side effects on the cache can be replayed.
                if super::tex_epoch() == epoch_before
                    && crate::nvn::api::texture_dirty_gen() == dirty_before
                    && tca.with(|tc| tc.twin_uses) == twins_before
                {
                    let entry = super::BindMemo {
                        tex_addr,
                        view: view.map(|v| {
                            std::mem::transmute::<crate::nvn::api::NvnTextureView, [u8; 16]>(v)
                        }),
                        gen: super::tex_info(tex_addr).map(|(g, _)| g),
                        dirty_gen: dirty_before,
                        epoch: super::tex_epoch(),
                        alias_root,
                        result: resolved.clone(),
                        table_epoch,
                        frame: super::presented_frames(),
                        root: resolved.as_ref().map(root_id),
                    };
                    memo.insert(memo_key, entry);
                }
            }
            let root = resolved.as_ref().map(root_id);
            return (resolved, Some(sampler_id), root);
        }
        if let Some(t) = &resolved {
            let key = format!(
                "textype-{}-{:?}-{:?}",
                tb.ty,
                t.textureType(),
                t.pixelFormat()
            );
            warn_once(key, format!(
                "texture handle {:#x} is a {:?} {:?} but the shader declares {}: bound a dummy instead",
                handle, t.textureType(), t.pixelFormat(), tb.ty
            ));
        }
        (
            tca.with(|tc| self.dummy_texture(tc, &tb.ty)),
            Some(sampler_id),
            None,
        )
    }

    /// Render-target feedback. TotK's terrain decals (and ~20 other shaders, ~50 draws per frame) sample G-buffer targets
    /// of the pass they draw into, at their own pixel, after an nvnCommandBufferBarrier(ORDER_FRAGMENTS |
    /// INVALIDATE_TEXTURE): on the Switch they read what was rendered before the barrier. A Metal render pass keeps its
    /// attachments in tile memory, so sampling one inside the pass reads stale memory; the decals' fades then came out as
    /// solid dark patches that changed from frame to frame. For each such binding: once per barrier epoch, end the pass,
    /// copy the attachment's root texture, and bind the copy (a matching view of it); the next pass loads the targets.
    /// Draws after the first one in an epoch read the same copy, as the texture cache on the Switch would until the next
    /// barrier. --no-fb-snapshot disables.
    /// The draw's fragment texture bindings that sample its own colour targets, as (Metal texture binding, colour
    /// index), when every texture it samples from its pass is such a binding that can be read from the tile instead
    /// (tile_feedback.rs: the same level, layer, format and channels, the viewport over the whole target). None when
    /// it samples none, or any other way (a depth target, a vertex stage, another view).
    pub(crate) unsafe fn tile_map(
        &self,
        st: &mut State,
        colors: &[(usize, Tex, RtRef)],
        depth: Option<&Tex>,
        resolved: &ResolvedTex,
    ) -> Option<Vec<(u32, u32)>> {
        let mut memo_root = |t: &Tex| -> u64 {
            let k = Retained::as_ptr(t) as *const () as usize;
            if let Some(&(_, r)) = st.root_memo.get(&k) {
                return r;
            }
            let r = root_id(t);
            st.root_memo.insert(k, (t.clone(), r));
            r
        };
        let color_roots: smallvec::SmallVec<[u64; 8]> = colors.iter().map(|(_, t, _)| memo_root(t)).collect();
        let depth_root = depth.map(|t| memo_root(t));
        let mut map = Vec::new();
        for (stage, binding, t, _, root) in resolved.iter() {
            let Some(t) = t else { continue };
            let root = root.unwrap_or_else(|| memo_root(t));
            if depth_root == Some(root) {
                return None;
            }
            let Some(i) = color_roots.iter().position(|&r| r == root) else { continue };
            let (k, a, rt) = &colors[i];
            if *stage != STAGE_FRAGMENT || !super::tile_feedback::same_pixels(t, a, rt.level as usize, rt.layer as usize) {
                return None;
            }
            let (w, h) = ((a.width() >> rt.level) as i32, (a.height() >> rt.level) as i32);
            if st.viewport.is_some_and(|v| v != (0, 0, w, h)) {
                return None;
            }
            map.push((*binding, *k as u32));
        }
        (!map.is_empty()).then_some(map)
    }

    pub(crate) unsafe fn feedback_snapshots(
        &self,
        st: &mut State,
        colors: &[(usize, Tex, RtRef)],
        depth: Option<&Tex>,
        resolved: &mut ResolvedTex,
    ) {
        if resolved.iter().all(|r| r.2.is_none()) {
            return;
        }
        // Every bound texture of every draw is checked: root ids come from a per-frame memo (a plain hash lookup; the
        // associated-object lookup in root_id was ~2% of the Presentation Thread here).
        let mut memo_root = |t: &Tex| -> u64 {
            let k = Retained::as_ptr(t) as *const () as usize;
            if let Some(&(_, r)) = st.root_memo.get(&k) {
                return r;
            }
            let r = root_id(t);
            st.root_memo.insert(k, (t.clone(), r)); // holds the texture: its address cannot be reused this frame
            r
        };
        let atts: smallvec::SmallVec<[u64; 9]> =
            colors.iter().map(|(_, t, _)| t).chain(depth).map(|t| memo_root(t)).collect();
        // Roots come with the binding (computed once per binding memo); memo_root only for those without.
        let feedback = (0..resolved.len()).any(|i| {
            let r = &resolved[i];
            r.2.is_some()
                && r.4.or_else(|| r.2.as_ref().map(|t| memo_root(t))).is_some_and(|root| atts.contains(&root))
        });
        // The draw samples an attachment of its pass: sample it in place, in a pass begun after the barrier. Apple GPUs
        // keep a pass's attachment writes in tile memory until each tile is stored, so the texture's memory holds what it
        // held when the pass began, exactly for the pixel being shaded (the decals and deferred passes here read their own
        // pixel), while a tile already stored can show newer texels to reads of other pixels. Metal API validation
        // accepts the binding.
        if feedback && st.enc.is_some() && st.enc_fb_epoch != st.fb_epoch {
            self.end_encoder_keep_clears(st);
            super::FB_INPLACE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// For a render target that is a block-compressed texture written through an uncompressed view whose texel is one
    /// block (e.g. RGBA16UI over BC4), an uncompressed texture with one texel per block to render into instead.
    /// The target drawn through a view of another format (TotK renders the RGBA8 swapchain through an RGBA8_SRGB view,
    /// so the hardware encodes linear output to sRGB). Views are cached so consecutive draws keep sharing an encoder.
    pub(crate) fn rt_format_view(&self, st: &mut State, t: &Tex, rt: &RtRef) -> Option<Tex> {
        if rt.view_format == 0 {
            return None;
        }
        let pf = metal_format(rt.view_format as i32).0;
        if pf == t.pixelFormat()
            || pf == MTLPixelFormat::Invalid
            || !super::is_view_compatible(t.pixelFormat(), pf)
        {
            return None;
        }
        let key = (Retained::as_ptr(t) as usize, pf.0 as u64);
        if let Some(v) = st.rt_format_views.get(&key) {
            return Some(v.clone());
        }
        if st.rt_format_views.len() > 256 {
            st.rt_format_views.clear();
        }
        if !super::view_format_ok(&t, pf) {
            return None;
        }
        let v = t.newTextureViewWithPixelFormat(pf)?;
        st.rt_format_views.insert(key, v.clone());
        Some(v)
    }

    pub(crate) fn bc_proxy(&self, st: &mut State, t: &Tex, rt: &RtRef) -> Option<Tex> {
        let info =
            crate::nvn::layout::format_info(unsafe { super::tex_info(rt.texture) }?.1.format)?;
        let view = crate::nvn::layout::format_info(rt.view_format as i32)?;
        if info.block_w <= 1 || view.block_w != 1 || view.bytes != info.bytes {
            return None;
        }
        let pf = metal_format(rt.view_format as i32).0;
        let key = (Retained::as_ptr(t) as usize, rt.level as usize, pf.0 as u64);
        if let Some(p) = st.bc_proxies.get(&key) {
            return Some(p.clone());
        }
        let level = rt.level as usize;
        let (w, h) = ((t.width() >> level).max(1), (t.height() >> level).max(1));
        // One proxy slice per render-target layer: cube faces count (a cube has arrayLength 1 and six faces; a cube
        // array, six per cube). resolve_bc_proxies copies slice s to the target's slice s, which for cubes is the face.
        let slices = match t.textureType() {
            MTLTextureType::TypeCube => 6,
            MTLTextureType::TypeCubeArray => 6 * t.arrayLength(),
            _ => t.arrayLength().max(1),
        };
        let desc = MTLTextureDescriptor::new();
        desc.setTextureType(if t.textureType() == MTLTextureType::Type3D {
            MTLTextureType::Type3D
        } else if slices > 1 {
            MTLTextureType::Type2DArray
        } else {
            MTLTextureType::Type2D
        });
        desc.setPixelFormat(pf);
        unsafe {
            desc.setWidth(w.div_ceil(info.block_w as usize));
            desc.setHeight(h.div_ceil(info.block_h as usize));
            if t.textureType() == MTLTextureType::Type3D {
                desc.setDepth((t.depth() >> level).max(1));
            } else if slices > 1 {
                desc.setArrayLength(slices);
            }
        }
        desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        desc.setStorageMode(MTLStorageMode::Private);
        let p = self.device.newTextureWithDescriptor(&desc)?;
        println!(
            "[+] Gfx: rendering compressed {:?} target ({}x{}, level {}) through a {:?} proxy",
            t.pixelFormat(),
            w,
            h,
            level,
            pf
        );
        st.bc_proxies.insert(key, p.clone());
        Some(p)
    }
}
/// The texture a shader binding should see: `t` restricted by the NVN texture view (format, swizzle, mip range,
/// layer range) and re-typed to `want` when Metal allows that view; None when the types cannot be reconciled.
pub(crate) unsafe fn texture_for_binding(
    cache: &mut HashMap<(usize, usize, usize, [usize; 4], [isize; 4]), (Tex, u64)>,
    t: &Tex,
    view: Option<&crate::nvn::api::NvnTextureView>,
    want: MTLTextureType,
    tex_swizzle: [u8; 4],
) -> Option<Tex> {
    let have = t.textureType();
    let levels_total = t.mipmapLevelCount();
    let slices_total = match have {
        MTLTextureType::TypeCube => 6,
        MTLTextureType::TypeCubeArray => t.arrayLength() * 6,
        MTLTextureType::Type3D => 1,
        _ => t.arrayLength().max(1),
    };
    let (mut level0, mut nlevels, mut slice0, mut nslices) =
        (0usize, levels_total, 0usize, slices_total);
    let mut pf = t.pixelFormat();
    if let Some(v) = view {
        if v.num_levels > 0 {
            level0 = (v.min_level as usize).min(levels_total - 1);
            nlevels = (v.num_levels as usize).min(levels_total - level0).max(1);
        }
        if v.num_layers > 0 && have != MTLTextureType::Type3D {
            slice0 = (v.min_layer as usize).min(slices_total - 1);
            nslices = (v.num_layers as usize).min(slices_total - slice0).max(1);
        }
        if v.format != 0 {
            let vpf = metal_format(v.format as i32).0;
            if is_view_compatible(vpf, pf) {
                pf = vpf;
            }
        }
    }
    // The texture's own swizzle (nvnTextureBuilderSetSwizzle, e.g. ONE, ONE, ONE, R for an R8 mask); a view's swizzle
    // replaces it (NVN: view swizzles override the builder's). Composing them read Mario Kart 8 Deluxe's shadow mask
    // (texture and view both G, B, A, R) as B, A, R, G: its race track came out black.
    let mut eff = if tex_swizzle == [0; 4] {
        [2, 3, 4, 5]
    } else {
        tex_swizzle
    };
    if let Some(v) = view {
        if v.swizzle != [0; 4] {
            eff = v.swizzle;
        }
    }
    if let Some(m) = packed_channels(t.pixelFormat()) {
        eff = eff.map(|c| if (2..=5).contains(&c) { m[(c - 2) as usize] } else { c });
    }
    let swz = {
        let sw = |x: u8| match x {
            0 => MTLTextureSwizzle::Zero,
            1 => MTLTextureSwizzle::One,
            2 => MTLTextureSwizzle::Red,
            3 => MTLTextureSwizzle::Green,
            4 => MTLTextureSwizzle::Blue,
            _ => MTLTextureSwizzle::Alpha,
        };
        MTLTextureSwizzleChannels {
            red: sw(eff[0]),
            green: sw(eff[1]),
            blue: sw(eff[2]),
            alpha: sw(eff[3]),
        }
    };
    // Metal view type rules: 2D <-> 2D array (one slice for 2D), cube <-> 2D array (6 faces), cube array <-> 2D array.
    let layered = |t: MTLTextureType| {
        matches!(
            t,
            MTLTextureType::Type2D
                | MTLTextureType::Type2DArray
                | MTLTextureType::TypeCube
                | MTLTextureType::TypeCubeArray
        )
    };
    let (tt, nslices) = if want == have {
        (want, nslices)
    } else if layered(want) && layered(have) {
        match want {
            MTLTextureType::Type2D => (want, 1),
            MTLTextureType::TypeCube if nslices >= 6 => (want, 6),
            MTLTextureType::TypeCubeArray if nslices >= 6 => (want, nslices / 6 * 6),
            MTLTextureType::Type2DArray => (want, nslices),
            _ => return None,
        }
    } else {
        return None;
    };
    let identity = tt == have
        && pf == t.pixelFormat()
        && level0 == 0
        && nlevels == levels_total
        && slice0 == 0
        && nslices == slices_total
        && swz.red == MTLTextureSwizzle::Red
        && swz.green == MTLTextureSwizzle::Green
        && swz.blue == MTLTextureSwizzle::Blue
        && swz.alpha == MTLTextureSwizzle::Alpha;
    if identity {
        return Some(t.clone());
    }
    // Views are cached: creating one per draw was a measurable share of the executor's time. A cached view retains its
    // texture, so the texture's address cannot be reused while the entry exists. Entries remember the frame they were
    // last used in; the frame end drops those unused for two frames (see trim_view_cache).
    let key = (
        Retained::as_ptr(t) as *const () as usize,
        pf.0 as usize,
        tt.0 as usize,
        [level0, nlevels, slice0, nslices],
        [
            swz.red.0 as isize,
            swz.green.0 as isize,
            swz.blue.0 as isize,
            swz.alpha.0 as isize,
        ],
    );
    let frame = super::presented_frames();
    if let Some((v, used)) = cache.get_mut(&key) {
        *used = frame;
        return Some(v.clone());
    }
    BINDING_VIEW_STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if cache.len() >= view_cache_cap() {
        BINDING_VIEW_STATS[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        cache.clear();
    }
    if !super::view_format_ok(t, pf) {
        return None;
    }
    let v = t.newTextureViewWithPixelFormat_textureType_levels_slices_swizzle(
        pf,
        tt,
        objc2_foundation::NSRange::new(level0, nlevels),
        objc2_foundation::NSRange::new(slice0, nslices),
        swz,
    )?;
    cache.insert(key, (v.clone(), frame));
    Some(v)
}

/// A 2D view of one level and slice of `t` (tiny compute passes write render-target levels / cube faces through it),
/// kept in the binding-view cache.
pub(crate) fn sub_view_2d(
    cache: &mut HashMap<(usize, usize, usize, [usize; 4], [isize; 4]), (Tex, u64)>,
    t: &Tex,
    level: usize,
    slice: usize,
) -> Option<Tex> {
    let key = (
        Retained::as_ptr(t) as *const () as usize,
        t.pixelFormat().0 as usize,
        MTLTextureType::Type2D.0 as usize,
        [level, 1, slice, 1],
        [-1; 4],
    );
    let frame = super::presented_frames();
    if let Some((v, used)) = cache.get_mut(&key) {
        *used = frame;
        return Some(v.clone());
    }
    if cache.len() >= view_cache_cap() {
        cache.clear();
    }
    let v = unsafe {
        t.newTextureViewWithPixelFormat_textureType_levels_slices(
            t.pixelFormat(),
            MTLTextureType::Type2D,
            objc2_foundation::NSRange::new(level, 1),
            objc2_foundation::NSRange::new(slice, 1),
        )
    }?;
    BINDING_VIEW_STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    cache.insert(key, (v.clone(), frame));
    Some(v)
}

/// Entries the binding-view cache may hold (--view-cache-cap): a cached view retains its texture.
pub(crate) fn view_cache_cap() -> usize {
    static CAP: OnceLock<usize> = OnceLock::new();
    *CAP.get_or_init(|| crate::settings::get().graphics.view_cache_cap)
}

/// Frame end: drops binding views not used in this or the previous frame. TotK's textures are mostly stable from frame to
/// frame (a bound texture needs the same swizzle / type view each frame: ~450 views per Hateno frame, re-created every
/// frame when the cache was cleared at 512 entries, ~1.3 ms of encode), while transient ones disappear; keeping views of
/// those alive longer would keep their textures alive (a 4096-entry cache without aging doubled memory to ~9 GB).
pub(crate) fn trim_view_cache(
    cache: &mut HashMap<(usize, usize, usize, [usize; 4], [isize; 4]), (Tex, u64)>,
) {
    let frame = super::presented_frames();
    cache.retain(|_, (_, used)| *used + 1 >= frame);
    if cache.len() > view_cache_cap() {
        cache.clear();
    }
}

pub(crate) fn root_id(t: &Tex) -> u64 {
    static KEY: u8 = 0;
    let obj = Retained::as_ptr(t) as *const objc2::runtime::AnyObject;
    let key = &KEY as *const u8 as *const std::ffi::c_void;
    let v = unsafe { objc2::ffi::objc_getAssociatedObject(obj, key) } as u64;
    if v != 0 {
        ROOT_STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return v;
    }
    ROOT_STATS[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Views can be made of views (a swizzled binding view of a format view): follow the chain to the real texture.
    let mut r = t.clone();
    while let Some(p) = r.parentTexture() {
        r = p;
    }
    let id = Retained::as_ptr(&r) as *const () as u64;
    unsafe {
        objc2::ffi::objc_setAssociatedObject(
            obj as *mut _,
            key,
            id as *mut _,
            objc2::ffi::OBJC_ASSOCIATION_ASSIGN,
        )
    };
    id
}

/// --tiny-compute runs qualifying small draws as compute; off on replay side B with
/// --replay-ab-tiny.
/// Formats compute kernels cannot write as color targets (depth / stencil, sRGB).
pub(crate) fn is_depth_or_srgb(pf: MTLPixelFormat) -> bool {
    use MTLPixelFormat as P;
    matches!(
        pf,
        P::Depth16Unorm
            | P::Depth32Float
            | P::Depth24Unorm_Stencil8
            | P::Depth32Float_Stencil8
            | P::Stencil8
            | P::X32_Stencil8
            | P::X24_Stencil8
            | P::RGBA8Unorm_sRGB
            | P::BGRA8Unorm_sRGB
            | P::R8Unorm_sRGB
            | P::RG8Unorm_sRGB
    ) || super::copy::is_depth_stencil(pf)
}

/// plan_draw's texture-resolution reuse key (State::tex_reuse).
#[derive(PartialEq)]
pub(crate) struct TexReuseKey {
    pub(crate) vs: usize,
    pub(crate) fs: usize,
    pub(crate) handles: smallvec::SmallVec<[Option<u64>; 16]>,
    pub(crate) epochs: [u64; 4],
}

/// Draws whose textures were [reused from the previous draw, resolved].
pub(crate) static TEX_REUSE_STATS: [std::sync::atomic::AtomicU64; 2] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 2];

/// A thread's texture binding memos (EncodeCtx::bind_memo).
pub(crate) type BindMemos = HashMap<(i32, usize), super::BindMemo>;

/// Textures a draw binds: (stage, binding, texture, sampler id, root id).
pub(crate) type ResolvedTex =
    smallvec::SmallVec<[(i32, u32, Option<Tex>, Option<i32>, Option<u64>); 16]>;

/// Whether [addr, addr + size) overlaps guest memory written by GPU work earlier in this submit (`State::gpu_dirty`).
pub(crate) fn gpu_written(gpu_dirty: &std::collections::BTreeMap<u64, u64>, addr: u64, size: usize) -> bool {
    // The ranges are merged and disjoint: only the last one starting below `end` can overlap.
    let end = addr + size as u64;
    gpu_dirty
        .range(..end)
        .next_back()
        .is_some_and(|(_, &e)| e > addr)
}
