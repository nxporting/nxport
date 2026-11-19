// Textures: resolving NVN texture objects to Metal textures, alias views, uploads of guest data and refreshes of
// CPU-written textures.

use super::*;

impl Gfx {
    /// Metal texture backing an NVNtexture, created on first use and re-uploaded from guest storage when the
    /// guest wrote texels into it since (nvnTextureWriteTexels / FlushTexels).
    pub(super) unsafe fn texture(&self, tc: &mut TexCache, nvn_tex: usize) -> Option<Tex> {
        if nvn_tex == 0 {
            return None;
        }
        // Unchanged tables: the object's (and an alias root's) generation is what it was; unchanged dirty counter: no
        // upload pending. Render targets resolve here for every draw (tex_info takes a global lock).
        let table_epoch = crate::nvn::api::tex_table_epoch();
        let dirty_gen = crate::nvn::api::texture_dirty_gen();
        {
            if let Some(&(e, d, g)) = tc.tex_memo.get(&nvn_tex) {
                if e == table_epoch && d == dirty_gen && tc.tex_gens.get(&nvn_tex) == Some(&g) {
                    if let Some(t) = tc.textures.get(&nvn_tex) {
                        TEX_MEMO_STATS[0].fetch_add(1, Ordering::Relaxed);
                        return Some(t.clone());
                    }
                }
                TEX_MEMO_STATS[1 + (e != table_epoch) as usize].fetch_add(1, Ordering::Relaxed);
            }
        }
        let (gen, snap) = tex_info(nvn_tex)?;
        // A view of another texture's storage is only valid while that root is still the same texture.
        if let Some(&(root, root_gen)) = tc.alias_of.get(&nvn_tex) {
            if tex_info(root).map(|(g, _)| g) != Some(root_gen) {
                tc.textures.remove(&nvn_tex);
                tc.alias_of.remove(&nvn_tex);
                // Only this object's resolution changed, and memos check the object's and its alias root's generations
                // themselves: a global bump here invalidated every binding memo several times per frame (transient
                // roots are re-initialized every frame).
                EPOCH_BUMPS[0].fetch_add(1, Ordering::Relaxed);
            }
        }
        if let Some(t) = tc.textures.get(&nvn_tex) {
            if tc.tex_gens.get(&nvn_tex) == Some(&gen) {
                let t = t.clone();
                if crate::nvn::api::take_texture_dirty(nvn_tex) {
                    self.upload_texture(&t, &snap);
                }
                {
                    if tc.tex_memo.len() > 1 << 16 {
                        tc.tex_memo.clear();
                    }
                    tc.tex_memo.insert(nvn_tex, (table_epoch, dirty_gen, gen));
                }
                return Some(t);
            }
            // The object was re-initialized as a different texture: drop the stale Metal texture.
            tc.textures.remove(&nvn_tex);
        }
        let watched = watch_storage() == Some(snap.storage);
        let _slow = SlowTimer::start();
        let mut alias = None;
        for _ in 0..4 {
            tc.alias_raced = false;
            alias = self.alias_view(tc, nvn_tex, &snap);
            if alias.is_some() || !tc.alias_raced {
                break;
            }
        }
        if let Some(v) = alias {
            if watched {
                println!("[watch] f{} texture 0x{:x} ({}x{} fmt {} flags {:x}) -> alias view, storage id {:x}", presented_frames() + 1, nvn_tex, snap.width, snap.height, snap.format, snap.flags, storage_id(&v));
            }
            tc.textures.insert(nvn_tex, v.clone());
            tc.tex_gens.insert(nvn_tex, gen);
            // The same memory with the same layout must resolve to this view later too: TotK re-initializes texture
            // objects for transient memory within a frame, so by the time a texture is sampled the root this view was
            // made from may describe other data and the alias search finds nothing. Falling back to a separate Metal
            // texture for the same memory then samples what was never rendered (e.g. empty shadow cascades).
            let desc_key = TexDesc::of(&snap);
            // Objects already resolved to an older Metal texture for this memory must follow: otherwise the render
            // target writes into the view while those objects keep sampling the old texture (e.g. the half-res
            // linear depth read back as zeros, which threw every shadow lookup off the cascade).
            if let Some((old, _)) = tc.tex_by_desc.get(&desc_key) {
                let old_id = storage_id(old);
                if old_id != storage_id(&v) {
                    let stale: Vec<usize> = tc
                        .textures
                        .iter()
                        .filter(|(_, t)| storage_id(t) == old_id)
                        .map(|(&a, _)| a)
                        .collect();
                    for a in stale {
                        tc.textures.remove(&a);
                        tc.tex_gens.remove(&a);
                    }
                    bump_tex_epoch();
                    EPOCH_BUMPS[1].fetch_add(1, Ordering::Relaxed);
                    tc.textures.insert(nvn_tex, v.clone());
                }
            }
            // No hash here (views are re-resolved many times per frame as objects are re-initialized): a view of a
            // GPU-rendered root is never compared, and for others the first refresh check records the baseline.
            // Re-resolving the same view keeps the entry's hash (overwriting it with HASH_UNKNOWN forced a full re-hash
            // on every re-resolve, ~19 per frame).
            let same = tc
                .tex_by_desc
                .get(&desc_key)
                .is_some_and(|(t, _)| storage_id(t) == storage_id(&v));
            tc.desc_used.insert(desc_key, presented_frames());
            if !same {
                let h = if tc.gpu_written.contains(&storage_id(&v)) {
                    0
                } else {
                    HASH_UNKNOWN
                };
                tc.tex_by_desc.insert(desc_key, (v.clone(), h));
            }
            return Some(v);
        }
        let desc_key = TexDesc::of(&snap);
        if let Some((t, _)) = tc.tex_by_desc.get(&desc_key).cloned() {
            tc.desc_used.insert(desc_key, presented_frames());
            // Same memory and layout as a texture seen before (objects are re-initialized constantly). Its contents can
            // only have changed behind our back if the CPU can write that memory: check like any CPU-written texture
            // (once per frame; sparse sample for compressed data) instead of fully re-hashing on every lookup.
            if !tc.gpu_written.contains(&storage_id(&t))
                && crate::nvn::api::memory_pool_cpu_writable(snap.storage)
            {
                tc.textures.insert(nvn_tex, t.clone());
                tc.tex_gens.insert(nvn_tex, gen);
                self.refresh_cpu_written(tc, nvn_tex);
            }
            if watched {
                println!("[watch] f{} texture 0x{:x} ({}x{} fmt {} flags {:x}) -> by desc, storage id {:x}", presented_frames() + 1, nvn_tex, snap.width, snap.height, snap.format, snap.flags, storage_id(&t));
            }
            tc.textures.insert(nvn_tex, t.clone());
            tc.tex_gens.insert(nvn_tex, gen);
            return Some(t);
        }
        let t = &snap;
        // One line per Metal texture created (~30 per Hateno frame): only with --log-tex-init.
        static LOG: OnceLock<bool> = OnceLock::new();
        if *LOG.get_or_init(|| crate::settings::get().log.log_tex_init) {
            println!(
                "[TEXTURE] addr=0x{:x}, target={}, {}x{}x{}, levels={}, fmt={}, samples={}, storage=0x{:x}",
                nvn_tex, t.target, t.width, t.height, t.depth, t.levels, t.format, t.samples, t.storage
            );
        }
        if t.width <= 0 || t.height <= 0 || t.width > 16384 || t.height > 16384 {
            return None;
        }
        let (pf, _) = metal_format(t.format);
        let layout = layout_of(t);
        let layers = layout.layers.max(1) as usize;
        let (tt, array_len) = match t.target {
            crate::nvn::layout::NVN_TARGET_1D => (MTLTextureType::Type1D, 1),
            crate::nvn::layout::NVN_TARGET_2D | crate::nvn::layout::NVN_TARGET_RECTANGLE => {
                (MTLTextureType::Type2D, 1)
            }
            crate::nvn::layout::NVN_TARGET_3D => (MTLTextureType::Type3D, 1),
            crate::nvn::layout::NVN_TARGET_1D_ARRAY => (MTLTextureType::Type1DArray, layers),
            crate::nvn::layout::NVN_TARGET_2D_ARRAY => (MTLTextureType::Type2DArray, layers),
            crate::nvn::layout::NVN_TARGET_2D_MULTISAMPLE => (MTLTextureType::Type2DMultisample, 1),
            crate::nvn::layout::NVN_TARGET_2D_MULTISAMPLE_ARRAY => {
                (MTLTextureType::Type2DMultisampleArray, layers)
            }
            crate::nvn::layout::NVN_TARGET_CUBEMAP => (MTLTextureType::TypeCube, 1),
            crate::nvn::layout::NVN_TARGET_CUBEMAP_ARRAY => {
                (MTLTextureType::TypeCubeArray, (layers / 6).max(1))
            }
            _ => return None, // buffer textures are not expressible as sampled Metal textures here
        };
        let multisample = matches!(
            tt,
            MTLTextureType::Type2DMultisample | MTLTextureType::Type2DMultisampleArray
        );
        let max_dim = t.width.max(t.height).max(if tt == MTLTextureType::Type3D {
            t.depth
        } else {
            1
        }) as u32;
        let max_levels = (32 - max_dim.leading_zeros()) as usize;
        let levels = (t.levels.max(1) as usize).min(max_levels).max(1);
        let is_compressed = crate::nvn::layout::format_info(t.format)
            .map(|i| i.block_w > 1)
            .unwrap_or(false);
        // Render targets of the scene's size class are made larger (gpu::scale; 1 unless the setting asks for more).
        let scale = crate::gpu::scale::scale(&crate::gpu::scale::Shape {
            width: t.width as u32,
            height: t.height as u32,
            two_d: matches!(
                tt,
                MTLTextureType::Type2D
                    | MTLTextureType::Type2DArray
                    | MTLTextureType::Type2DMultisample
                    | MTLTextureType::Type2DMultisampleArray
            ),
            compressed: is_compressed,
            linear: t.flags & (crate::nvn::layout::NVN_TEXTURE_FLAGS_LINEAR_BIT | crate::nvn::layout::NVN_TEXTURE_FLAGS_LINEAR_RENDER_TARGET_BIT) != 0,
            image: t.flags & 0x4 != 0,
            storage: t.storage as usize,
        }) as usize;
        let desc = MTLTextureDescriptor::new();
        desc.setTextureType(tt);
        desc.setPixelFormat(pf);
        desc.setWidth(t.width as usize * scale);
        desc.setHeight(
            if matches!(tt, MTLTextureType::Type1D | MTLTextureType::Type1DArray) {
                1
            } else {
                t.height as usize * scale
            },
        );
        if tt == MTLTextureType::Type3D {
            desc.setDepth(t.depth.max(1) as usize);
        }
        if array_len > 1
            || matches!(
                tt,
                MTLTextureType::Type1DArray
                    | MTLTextureType::Type2DArray
                    | MTLTextureType::TypeCubeArray
                    | MTLTextureType::Type2DMultisampleArray
            )
        {
            desc.setArrayLength(array_len);
        }
        desc.setMipmapLevelCount(if multisample { 1 } else { levels });
        if multisample {
            desc.setSampleCount(t.samples.max(1) as usize);
        }
        let mut usage = MTLTextureUsage::ShaderRead;
        if wants_view_flag(pf) {
            usage |= MTLTextureUsage::PixelFormatView;
        }
        if !is_compressed {
            usage |= MTLTextureUsage::RenderTarget;
        }
        // NVN_TEXTURE_FLAGS_IMAGE_BIT: the texture may be bound as a storage image (shader writes).
        if t.flags & 0x4 != 0 && !is_compressed {
            usage |= MTLTextureUsage::ShaderWrite;
        }
        // Small render targets may be written by compute versions of their passes.
        if translated::tiny_compute_on()
            && !is_compressed
            && usage.contains(MTLTextureUsage::RenderTarget)
            && !translated::is_depth_or_srgb(pf)
            && t.samples <= 1
            && tiny_shader_write(pf, t.width as usize * t.height as usize)
        {
            usage |= MTLTextureUsage::ShaderWrite;
        }
        desc.setUsage(usage);
        // Shared (unified memory) so uploads and debug readback work directly; no cost on Apple Silicon.
        desc.setStorageMode(MTLStorageMode::Shared);
        if HITCH_TEX_N.fetch_add(1, Ordering::Relaxed) < 8 && hitch_enabled() {
            HITCH_TEX.lock().unwrap().push(format!(
                "{}x{}x{} fmt {} lv {} flags {:#x} storage {:#x}",
                t.width,
                t.height,
                t.depth.max(layers as i32),
                t.format,
                levels,
                t.flags,
                t.storage
            ));
        }
        let tex = self.device.newTextureWithDescriptor(&desc)?;
        if crate::gpu::scale::factor() > 1 {
            set_tex_scale(&tex, scale as u32);
            if let Some(old) = tc.demoted.remove(&(t.storage as usize)) {
                if old.pixelFormat() == pf && scale == 1 {
                    tc.downscale_pending.push((old, tex.clone()));
                }
            }
        }
        TEXSTAT[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        journal(|| {
            format!(
                "texture new {:x} for {:#x}: {}x{}x{} fmt {} lv {} flags {:#x} storage {:#x}",
                tex_id(&tex),
                nvn_tex,
                t.width,
                t.height,
                t.depth.max(layers as i32),
                t.format,
                levels,
                t.flags,
                t.storage
            )
        });
        if watch_texture(&t) {
            let len = layout_of(&t).total as usize;
            println!(
                "[watch-size] f{} new metal {:x} for {:#x}: {}x{} fmt {} lv {} flags {:#x} storage {:#x}+{:#x} held {}",
                presented_frames() + 1,
                tex_id(&tex),
                nvn_tex,
                t.width,
                t.height,
                t.format,
                levels,
                t.flags,
                t.storage,
                len,
                rt_guard::overlaps_held(t.storage, len)
            );
        }
        if watched {
            println!(
                "[watch] f{} texture 0x{:x} ({}x{} fmt {} flags {:x}) -> new, storage id {:x}",
                presented_frames() + 1,
                nvn_tex,
                t.width,
                t.height,
                t.format,
                t.flags,
                storage_id(&tex)
            );
        }
        // Uploaded even for GPU-only (CPU_NO_ACCESS) memory: skipping it broke the HUD (hearts, minimap), whose textures
        // live in such pools and depend on these contents (the pool's initial bytes). Except for a COMPRESSIBLE texture
        // (render targets) where a texture we rendered into had the same storage: NVN requires such textures to be
        // cleared before the GPU reads or writes them when they reuse memory, so their old bytes are never used, and the
        // CPU cannot have written them since (CPU_NO_ACCESS). Dynamic resolution (Kirby) re-creates ~22 full-screen
        // targets at their old storage every few seconds; uploading and hashing them cost ~35 ms per resize (a visible
        // stutter).
        let stale_rt = !is_compressed
            && t.flags & 0x8 != 0
            && !crate::nvn::api::memory_pool_cpu_writable(t.storage)
            && tc.tex_by_desc.iter().any(|(k, (old, _))| {
                k.storage == desc_key.storage && tc.gpu_written.contains(&storage_id(old))
            });
        let t0 = std::time::Instant::now();
        if !stale_rt {
            self.upload_texture(&tex, t);
        }
        HITCH_UPLOAD_NS.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
        // Debug: --poison-new-tex fills new GPU-only (CPU_NO_ACCESS pool) textures with all-ones bytes (NaN for
        // float formats) instead of their stale guest bytes, to expose rendering that depends on unwritten texels.
        // --poison-new-tex=2 also poisons textures in CPU-writable pools (render targets placed there).
        static POISON: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        let poison = *POISON.get_or_init(|| crate::settings::get().debug.poison_new_tex);
        if poison > 0
            && !is_compressed
            && (poison >= 2 || !crate::nvn::api::memory_pool_cpu_writable(t.storage))
            && matches!(
                tt,
                MTLTextureType::Type2D
                    | MTLTextureType::Type2DArray
                    | MTLTextureType::TypeCube
                    | MTLTextureType::TypeCubeArray
            )
        {
            if let Some(info) = crate::nvn::layout::format_info(t.format) {
                let slices = match tt {
                    MTLTextureType::TypeCube => 6,
                    MTLTextureType::TypeCubeArray => 6 * array_len,
                    _ => array_len,
                };
                for level in 0..levels {
                    let (lw, lh) = (
                        ((t.width as usize) >> level).max(1),
                        ((t.height as usize) >> level).max(1),
                    );
                    let bpr = lw * info.bytes as usize;
                    let mut buf = vec![0xFFu8; bpr * lh];
                    for slice in 0..slices {
                        tex.replaceRegion_mipmapLevel_slice_withBytes_bytesPerRow_bytesPerImage(
                            MTLRegion {
                                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                                size: MTLSize {
                                    width: lw,
                                    height: lh,
                                    depth: 1,
                                },
                            },
                            level,
                            slice,
                            std::ptr::NonNull::new(buf.as_mut_ptr() as *mut libc::c_void).unwrap(),
                            bpr,
                            0,
                        );
                    }
                }
            }
        }
        tc.textures.insert(nvn_tex, tex.clone());
        tc.tex_gens.insert(nvn_tex, gen);
        let t0 = std::time::Instant::now();
        let h = if stale_rt {
            // Treated like a GPU-rendered texture: its guest bytes are never compared.
            tc.mark_gpu_written(storage_id(&tex));
            0
        } else {
            guest_bytes_hash(t)
        };
        HITCH_HASH_NS.fetch_add(t0.elapsed().as_nanos() as u64, Ordering::Relaxed);
        tc.tex_by_desc.insert(desc_key, (tex.clone(), h));
        tc.desc_used.insert(desc_key, presented_frames());
        Some(tex)
    }

    /// Sampled textures whose memory the CPU can write may change without any NVN call (the game's glyph cache writes
    /// rasterized glyphs straight into its font atlas). Once per frame, re-hash such a texture and re-upload it when
    /// its guest bytes changed. Limited to uncompressed textures the GPU never rendered into, within a per-frame budget.
    pub(super) unsafe fn refresh_cpu_written(&self, tc: &mut TexCache, nvn_tex: usize) {
        self.refresh_cpu_written_with(tc, nvn_tex, None)
    }

    /// `known`: the object's snapshot when the caller already has it (saves a global lookup per resolved binding).
    pub(super) unsafe fn refresh_cpu_written_with(
        &self,
        tc: &mut TexCache,
        nvn_tex: usize,
        known: Option<NvnTexture>,
    ) {
        const BUDGET: usize = 64 << 20;
        // A view of another texture's storage shares that root's Metal texture: refresh the root.
        let (nvn_tex, known) = match tc.alias_of.get(&nvn_tex) {
            Some(&(root, _)) => (root, None),
            None => (nvn_tex, known),
        };
        let Some(snap) = known.or_else(|| tex_info(nvn_tex).map(|(_, s)| s)) else {
            return;
        };
        let key = TexDesc::of(&snap);
        let frame = presented_frames();
        if tc.tex_checked.get(&key) == Some(&frame) {
            return; // already checked this frame (checked before the pool lookups below, which are not free)
        }
        let debug = crate::settings::get()
            .debug
            .debug_refresh
            .is_some_and(|s| s.is(snap.width, snap.height));
        let why = |r: &str| {
            if debug {
                println!("[refresh] f{} 0x{:x}: {}", presented_frames(), nvn_tex, r)
            }
        };
        // Only CPU-writable pools: textures in CPU_NO_ACCESS pools are filled by the GPU (e.g. TotK's text rasters),
        // and that content lives only in the Metal texture, so re-uploading their stale guest memory would erase it.
        if snap.storage <= 0x10000 || !crate::nvn::api::memory_pool_cpu_writable(snap.storage) {
            why("pool not CPU-writable");
            return;
        }
        // Compressed textures are mostly streamed assets: TotK's streamer decodes texels into the storage after
        // nvnTextureInitialize, possibly after the first draw (and reuses storage that held another texture), so they
        // need re-checking too, but full hashes of every one each frame cost too much. They are checked by a sparse
        // sample of each mip level instead and fully re-hashed only when that changes.
        let compressed =
            crate::nvn::layout::format_info(snap.format).map_or(true, |i| i.block_w > 1);
        let Some((t, old_hash)) = tc.tex_by_desc.get(&key).cloned() else {
            why("no cached texture for this description");
            return;
        };
        if tc.gpu_written.contains(&storage_id(&t)) {
            why("GPU-written");
            return;
        }
        let layout = layout_of(&snap);
        let len = layout.total as usize;
        // --tex-backing holds its whole pages: no CPU access since then (tex_backing::holds).
        if tex_backing::holds(snap.storage as usize, len) {
            tc.tex_checked.insert(key, frame);
            return;
        }
        // Compressed textures are write-tracked too where their pages allow (--no-sparse-track: sample every frame):
        // no page written since the last sample means nothing to sample.
        let sparse_track = compressed
            && crate::guest::dirty_track::trackable(snap.storage as usize, len);
        if sparse_track
            && tc.tex_sparse.contains_key(&key)
            && crate::guest::dirty_track::generation(snap.storage as usize, len)
                .is_some_and(|g| tc.tex_track_gen.get(&key) == Some(&g))
        {
            static VERIFY: OnceLock<bool> = OnceLock::new();
            if *VERIFY.get_or_init(|| crate::settings::get().log.dt_verify) {
                let sp = sparse_bytes_hash(&snap, &layout);
                let still = crate::guest::dirty_track::generation(snap.storage as usize, len)
                    == tc.tex_track_gen.get(&key).copied();
                if tc.tex_sparse.get(&key) != Some(&sp) && still {
                    println!("[dirty-track] MISSED f{} compressed tex {:#x} storage {:#x} {}x{} fmt {}: skipped but the sample changed", frame, nvn_tex, snap.storage, snap.width, snap.height, snap.format);
                }
            }
            tc.tex_checked.insert(key, frame);
            return;
        }
        if sparse_track {
            // Before sampling: writes during the sample change the token.
            match crate::guest::dirty_track::protect(snap.storage as usize, len) {
                Some(g) => {
                    tc.tex_track_gen.insert(key, g);
                }
                None => {
                    tc.tex_track_gen.remove(&key);
                }
            }
        }
        let sparse = compressed.then(|| sparse_bytes_hash(&snap, &layout));
        if let Some(sp) = sparse {
            if tc.tex_sparse.get(&key) == Some(&sp) {
                tc.tex_checked.insert(key, frame);
                return;
            }
        }
        // Large textures whose pages are their own are write-tracked (guest/dirty_track.rs): skip the hash while untouched.
        let tracked =
            sparse.is_none() && crate::guest::dirty_track::trackable(snap.storage as usize, len);
        // Per description: several descriptions can share one storage range, and each must see the writes itself.
        if tracked
            && old_hash != HASH_UNKNOWN
            && crate::guest::dirty_track::generation(snap.storage as usize, len)
                .is_some_and(|g| tc.tex_track_gen.get(&key) == Some(&g))
        {
            // Debug: --dt-verify hashes anyway and reports changes the tracking missed.
            static VERIFY: OnceLock<bool> = OnceLock::new();
            if *VERIFY.get_or_init(|| crate::settings::get().log.dt_verify) {
                let h = guest_bytes_hash(&snap);
                // A write racing with this check has bumped the generation by now and is caught next frame: only an
                // unchanged generation means the tracking missed it.
                let still = crate::guest::dirty_track::generation(snap.storage as usize, len)
                    == tc.tex_track_gen.get(&key).copied();
                if h != old_hash && !still {
                    println!("[dirty-track] raced f{} tex {:#x}: written during the check (refreshed next frame)", frame, nvn_tex);
                }
                if h != old_hash && still {
                    println!("[dirty-track] MISSED f{} tex {:#x} storage {:#x}: skipped but contents changed (gen {:?})", frame, nvn_tex, snap.storage, crate::guest::dirty_track::generation(snap.storage as usize, len));
                }
            }
            tc.tex_checked.insert(key, frame);
            return;
        }
        // GPU output not written back yet: the CPU has not written it since (that would have ended the hold).
        if rt_guard::overlaps_held(snap.storage as usize, len) {
            journal(|| format!("  refresh skipped: tex {:#x} storage {:#x}+{:#x} under a held render target", nvn_tex, snap.storage, len));
            why("held render target");
            return;
        }
        if tc.tex_check_budget_used + len > BUDGET {
            why("over budget");
            return; // try again next frame
        }
        tc.tex_check_budget_used += len;
        tc.tex_checked.insert(key, frame);
        if let Some(sp) = sparse {
            tc.tex_sparse.insert(key, sp);
        }
        if tracked {
            // Before hashing: writes during the hash change the generation.
            match crate::guest::dirty_track::protect(snap.storage as usize, len) {
                Some(g) => {
                    tc.tex_track_gen.insert(key, g);
                }
                None => {
                    tc.tex_track_gen.remove(&key);
                }
            }
        }
        let h = guest_bytes_hash(&snap);
        REFRESH_HASHED.fetch_add(len as u64, std::sync::atomic::Ordering::Relaxed);
        // Debug: --log-refresh-stats prints, every 300 frames, the texture shapes that took the most hashing.
        static STATS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *STATS.get_or_init(|| crate::settings::get().log.log_refresh_stats) {
            static AGG: std::sync::Mutex<
                Option<(u64, HashMap<(i32, i32, i32, bool, bool, bool), (u64, u64)>)>,
            > = std::sync::Mutex::new(None);
            let mut agg = AGG.lock().unwrap();
            let a = agg.get_or_insert_with(|| (frame, HashMap::default()));
            let e =
                a.1.entry((
                    snap.width,
                    snap.height,
                    snap.format,
                    compressed,
                    tracked,
                    old_hash == h,
                ))
                .or_insert((0, 0));
            e.0 += 1;
            e.1 += len as u64;
            if frame >= a.0 + 300 {
                let mut v: Vec<_> = a.1.iter().collect();
                v.sort_by_key(|(_, &(_, b))| std::cmp::Reverse(b));
                println!("[refresh-stats] frames {}..{}: (w, h, fmt, compressed, tracked, unchanged) -> (hashes, MB)", a.0, frame);
                for (k, (n, b)) in v.iter().take(15) {
                    println!(
                        "[refresh-stats]   {:?} -> ({}, {:.1})",
                        k,
                        n,
                        *b as f64 / 1048576.0
                    );
                }
                *agg = None;
            }
        }
        // Debug: --log-refresh-hash names the textures fully hashed (once per texture shape).
        static LOG_HASH: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *LOG_HASH.get_or_init(|| crate::settings::get().log.log_refresh_hash) {
            static SEEN: std::sync::Mutex<Vec<(i32, i32, i32, bool)>> =
                std::sync::Mutex::new(Vec::new());
            let k = (snap.width, snap.height, snap.format, compressed);
            let mut seen = SEEN.lock().unwrap();
            if !seen.contains(&k) || len > (4 << 20) {
                seen.push(k);
                println!("[refresh-hash] f{} {}x{} fmt {} compressed={} {} bytes storage {:#x} layers {} levels {} (old hash unknown: {})", frame, snap.width, snap.height, snap.format, compressed, len, snap.storage, layout.layers, snap.levels, old_hash == HASH_UNKNOWN);
            }
        }
        why(&format!("checked: hash {:x} -> {:x}", old_hash, h));
        if old_hash == HASH_UNKNOWN {
            tc.tex_by_desc.insert(key, (t, h)); // first check of an alias view: baseline only
            return;
        }
        if h != old_hash && len > (4 << 20) && crate::settings::get().log.log_dirty_track {
            println!("[dirty-track] f{} large texture {:#x} storage {:#x} hash changed (tracked {}, gen {:?})", frame, nvn_tex, snap.storage, tracked, crate::guest::dirty_track::generation(snap.storage as usize, len));
        }
        if h != old_hash {
            static LOGGED: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
            let mut l = LOGGED.lock().unwrap();
            if !l.contains(&nvn_tex) && l.len() < 64 {
                l.push(nvn_tex);
                println!(
                    "[+] Gfx: texture 0x{:x} ({}x{} fmt {}) changed in guest memory; re-uploaded",
                    nvn_tex, snap.width, snap.height, snap.format
                );
            }
            drop(l);
            self.upload_texture(&t, &snap);
            tc.tex_by_desc.insert(key, (t, h));
        }
    }

    /// If another live texture's storage contains this one's at a mip level / layer boundary with the same shape
    /// there (and a view-compatible format), returns a Metal view of that root texture; rendering through either
    /// object is then visible through the other, as on the GPU. --no-texture-alias disables this.
    pub(super) unsafe fn alias_view(
        &self,
        tc: &mut TexCache,
        nvn_tex: usize,
        snap: &NvnTexture,
    ) -> Option<Tex> {
        if snap.storage <= 0x10000 || snap.samples > 1 || snap.width <= 0 || snap.height <= 0 {
            return None;
        }
        let my_pf = metal_format(snap.format).0;
        let my_layout = layout_of(snap);
        let my_layers = my_layout.layers.max(1) as usize;
        // Size of a texture's memory, measured the same way for this texture and every candidate root. (The candidate
        // used its declared size and this one its layout size: two same-shape textures on one allocation whose declared
        // size exceeds the layout then each chose the other as root, and texture() / alias_view() recursed until the
        // Presentation Thread's stack overflowed -- the Hateno / Great Sky Island crashes.)
        let mem_size = |t: &NvnTexture, layout: &crate::nvn::layout::TextureLayout| {
            if t.size > 0 {
                (t.size as u64).max(layout.total)
            } else {
                layout.total
            }
        };
        let my_size = mem_size(snap, &my_layout);
        let layered = |target: i32| {
            use crate::nvn::layout::*;
            matches!(
                target,
                NVN_TARGET_2D
                    | NVN_TARGET_2D_ARRAY
                    | NVN_TARGET_CUBEMAP
                    | NVN_TARGET_CUBEMAP_ARRAY
                    | NVN_TARGET_RECTANGLE
            )
        };
        // A container starts at most its own size below this texture: no need to look further than the largest one.
        // --alias-check also searches 1 GiB and reports a different result.
        static CHECK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let check = *CHECK.get_or_init(|| crate::settings::get().log.alias_check);
        let mut results: Vec<Option<(usize, usize, usize)>> = Vec::new();
        let mut best: Option<(usize, u64, NvnTexture, usize, usize, u64)> = None;
        for span in [crate::nvn::api::max_texture_span(), 1 << 30]
            .into_iter()
            .take(if check { 2 } else { 1 })
        {
            best = None;
            for (addr, g, c) in crate::nvn::api::textures_near_storage(snap.storage, span) {
                if addr == nvn_tex || c.samples > 1 || c.width <= 0 || c.height <= 0 {
                    continue;
                }
                let layout = layout_of(&c);
                let size = mem_size(&c, &layout);
                // The root is the largest container; among equal ones the lowest object address, so every alias
                // of the same memory resolves to the same root whatever order they are first used in. The order is strict,
                // so resolution always ends at a texture with no root.
                if size < my_size
                    || (size == my_size && addr > nvn_tex)
                    || snap.storage as u64 >= c.storage as u64 + size
                {
                    continue;
                }
                if !is_view_compatible(my_pf, metal_format(c.format).0) {
                    continue;
                }
                if !(c.target == snap.target || (layered(c.target) && layered(snap.target))) {
                    continue;
                }
                let rel = (snap.storage - c.storage) as u64;
                let (slice, in_layer) = if layout.layer_stride > 0 {
                    (
                        (rel / layout.layer_stride) as usize,
                        rel % layout.layer_stride,
                    )
                } else {
                    (0, rel)
                };
                let Some(level) = layout.level_offsets.iter().position(|&o| o == in_layer) else {
                    continue;
                };
                let (lw, lh) = ((c.width >> level).max(1), (c.height >> level).max(1));
                if (lw, lh) != (snap.width, snap.height) {
                    continue;
                }
                if level + (snap.levels.max(1) as usize) > c.levels.max(1) as usize
                    || slice + my_layers > layout.layers.max(1) as usize
                {
                    continue;
                }
                if best
                    .as_ref()
                    .map_or(true, |b| size > b.5 || (size == b.5 && addr < b.0))
                {
                    best = Some((addr, g, c, level, slice, size));
                }
            }
            results.push(best.as_ref().map(|b| (b.0, b.3, b.4)));
        }
        if check && results.len() == 2 && results[0] != results[1] {
            println!("[alias-check] MISMATCH tex {:#x} storage {:#x}: bounded {:x?} vs 1 GiB {:x?} (span {:#x})", nvn_tex, snap.storage, results[0], results[1], crate::nvn::api::max_texture_span());
            if let Some((r, _, _)) = results[1] {
                if let Some((_, c)) = crate::nvn::api::texture_snapshot(r) {
                    println!("[alias-check]   root storage {:#x} (dist {:#x}) size {:#x} layout {:#x} {}x{}x{} fmt {} levels {} target {}", c.storage, snap.storage - c.storage, c.size,
                        crate::nvn::layout::compute_layout(&texture_params(&c)).total, c.width, c.height, c.depth, c.format, c.levels, c.target);
                }
            }
        }
        let (root_addr, root_gen, _root, level, slice, _) = best?;
        let root_tex = self.texture(tc, root_addr)?;
        // Game threads re-initialize transient texture objects while the executor resolves them: the root may describe
        // another texture by now (generations only grow, so an unchanged one means root_tex is the root the search
        // found). A view of the new texture, cached under this texture's description, made every later lookup of the
        // memory return a texture of the wrong size (Mario Kart 8 Deluxe: its 1920x1080 scene color as the 960x540
        // texture that replaced the root, so the race rendered black or in blocks in some runs). Search again.
        if tex_info(root_addr).map(|(g, _)| g) != Some(root_gen) {
            tc.alias_raced = true;
            return None;
        }
        let tt = match snap.target {
            crate::nvn::layout::NVN_TARGET_2D_ARRAY => MTLTextureType::Type2DArray,
            crate::nvn::layout::NVN_TARGET_CUBEMAP => MTLTextureType::TypeCube,
            crate::nvn::layout::NVN_TARGET_CUBEMAP_ARRAY => MTLTextureType::TypeCubeArray,
            crate::nvn::layout::NVN_TARGET_3D => MTLTextureType::Type3D,
            _ => MTLTextureType::Type2D,
        };
        let levels = (snap.levels.max(1) as usize)
            .min(root_tex.mipmapLevelCount().saturating_sub(level))
            .max(1);
        let slices = if tt == MTLTextureType::Type3D {
            1
        } else {
            my_layers
        };
        let pf = if is_view_compatible(my_pf, root_tex.pixelFormat()) {
            my_pf
        } else {
            root_tex.pixelFormat()
        };
        // A root that is itself one of our alias views (a view of a view): make the view on the texture behind it, at
        // the summed level / slice offsets. The intermediate view objects are re-made whenever their NVN objects are
        // re-initialized (every frame for some), so keying by them missed the cache ~14 times per frame in Hateno:
        // alias_views refilled to its cap again and again, and the live views slowed Metal's per-view queries
        // (parentTexture, hazard tracking) as a session went on.
        let (root_tex, level, slice) = {
                let (mut base, mut level, mut slice) = (root_tex, level, slice);
                while let Some(k) = tc
                    .alias_view_keys
                    .get(&(Retained::as_ptr(&base) as *const () as usize))
                    .copied()
                {
                    let Some((r, _, _)) = tc.alias_views.get(&k) else {
                        break;
                    };
                    if !is_view_compatible(pf, r.pixelFormat()) {
                        break;
                    }
                    level += k.3;
                    slice += if k.2 == MTLTextureType::Type3D.0 as usize {
                        0
                    } else {
                        k.5
                    };
                    base = r.clone();
                }
                (base, level, slice)
            };
        // The same view of the same root is asked for every frame (TotK re-initializes its transient texture objects):
        // reuse it. The cache holds the root, so its pointer cannot be reused for another texture while cached.
        // A root without room for the view (its level / slice offsets plus this texture's levels and layers) does not
        // contain this texture: no alias.
        let root_slices = match root_tex.textureType() {
            MTLTextureType::Type3D => 1,
            MTLTextureType::TypeCube | MTLTextureType::TypeCubeArray => 6 * root_tex.arrayLength().max(1),
            _ => root_tex.arrayLength().max(1),
        };
        if level + levels > root_tex.mipmapLevelCount() || (tt != MTLTextureType::Type3D && slice + slices > root_slices) {
            return None;
        }
        let vkey = (
            Retained::as_ptr(&root_tex) as *const () as usize,
            pf.0 as usize,
            tt.0 as usize,
            level,
            levels,
            slice,
            slices,
        );
        let root_id = translated::root_id(&root_tex);
        let view = match tc.alias_views.get(&vkey) {
            Some((_, v, _)) => v.clone(),
            None => {
                if !view_format_ok(&root_tex, pf) {
                    return None;
                }
                let v = root_tex.newTextureViewWithPixelFormat_textureType_levels_slices(
                    pf,
                    tt,
                    objc2_foundation::NSRange::new(level, levels),
                    objc2_foundation::NSRange::new(
                        if tt == MTLTextureType::Type3D {
                            0
                        } else {
                            slice
                        },
                        slices,
                    ),
                )?;
                ALIAS_NEW.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if crate::settings::get().log.log_alias_new && presented_frames() % 300 < 2 {
                    let same_shape: Vec<String> = tc
                        .alias_views
                        .iter()
                        .filter(|(k, _)| {
                            k.1 == vkey.1
                                && k.2 == vkey.2
                                && k.3 == vkey.3
                                && k.4 == vkey.4
                                && k.5 == vkey.5
                                && k.6 == vkey.6
                        })
                        .take(4)
                        .map(|(k, (_, _, r))| format!("root {:#x} (root id {:#x})", k.0, r))
                        .collect();
                    println!("[alias-new] f{} tex {:#x} {}x{} fmt {} -> root obj {:#x} @{:#x} {}x{} root id {:#x} parent {} | key {:?} | same shape cached: {} {:?}",
                        presented_frames(), nvn_tex, snap.width, snap.height, snap.format, root_addr, vkey.0, root_tex.width(), root_tex.height(), root_id,
                        root_tex.parentTexture().is_some(), (vkey.1, vkey.2, vkey.3, vkey.4, vkey.5, vkey.6), same_shape.len(), same_shape);
                }
                if tc.alias_views.len() > 256 {
                    tc.alias_views.clear();
                    tc.alias_view_keys.clear();
                }
                tc.alias_views
                    .insert(vkey, (root_tex.clone(), v.clone(), root_id));
                tc.alias_view_keys
                    .insert(Retained::as_ptr(&v) as *const () as usize, vkey);
                v
            }
        };
        TEXSTAT[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if alias_log_enabled() {
            println!(
                "[TEXTURE-ALIAS] 0x{:x} ({}x{} fmt {} lv{}) = view of 0x{:x} level {} slice {}",
                nvn_tex, snap.width, snap.height, snap.format, snap.levels, root_addr, level, slice
            );
        }
        tc.alias_of.insert(nvn_tex, (root_addr, root_gen));
        let _ = root_id;
        Some(view)
    }

    /// Fills every level / layer of `tex` from the guest's block-linear (or pitch-linear) storage.
    pub(super) unsafe fn upload_texture(&self, tex: &Tex, t: &NvnTexture) {
        TEXSTAT[2].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        journal(|| {
            format!(
                "upload {:x}: {}x{} fmt {} storage {:#x}",
                tex_id(tex),
                t.width,
                t.height,
                t.format,
                t.storage
            )
        });
        if t.storage <= 0x10000 {
            return;
        }
        if watch_texture(t) {
            let len = layout_of(t).total as usize;
            println!(
                "[watch-size] f{} upload metal {:x} from {:#x}+{:#x}: {} of {} bytes non-zero, held {}",
                presented_frames() + 1,
                tex_id(tex),
                t.storage,
                len,
                watch_nonzero(t.storage, len),
                len,
                rt_guard::overlaps_held(t.storage, len)
            );
        }
        if verify_tex_enabled() {
            UPLOADS
                .lock()
                .unwrap()
                .get_or_insert_with(HashMap::default)
                .insert(
                    storage_id(tex),
                    (t.storage, guest_bytes_hash(t), presented_frames() + 1),
                );
        }
        let tt = tex.textureType();
        if matches!(
            tt,
            MTLTextureType::Type2DMultisample | MTLTextureType::Type2DMultisampleArray
        ) {
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| println!("[!] Gfx: uploading multisample texture data is not implemented (first seen: target {})", t.target));
            return;
        }
        if tt == MTLTextureType::Type3D {
            // Volume textures (noise for fire / smoke, colour grading LUTs): whole levels, tiled in depth too.
            let Some(info) = crate::nvn::layout::format_info(t.format) else {
                return;
            };
            let p = texture_params(t);
            if (t.flags & 0x10 != 0) || (t.flags & 0x40 != 0) {
                return; // linear 3D: not seen yet
            }
            for level in 0..tex.mipmapLevelCount() {
                let (lw, lh, ld) = (
                    ((t.width as usize) >> level).max(1),
                    ((t.height as usize) >> level).max(1),
                    ((t.depth as usize) >> level).max(1),
                );
                let wb = (lw + info.block_w as usize - 1) / info.block_w as usize;
                let hb = (lh + info.block_h as usize - 1) / info.block_h as usize;
                let bpr = wb * info.bytes as usize;
                let mut buf = vec![0u8; bpr * hb * ld];
                if !crate::nvn::layout::read_texels_3d(
                    t.storage as *const u8,
                    &p,
                    level,
                    buf.as_mut_ptr(),
                ) {
                    continue;
                }
                tex.replaceRegion_mipmapLevel_slice_withBytes_bytesPerRow_bytesPerImage(
                    MTLRegion {
                        origin: MTLOrigin { x: 0, y: 0, z: 0 },
                        size: MTLSize {
                            width: lw,
                            height: lh,
                            depth: ld,
                        },
                    },
                    level,
                    0,
                    std::ptr::NonNull::new(buf.as_mut_ptr() as *mut libc::c_void).unwrap(),
                    bpr,
                    bpr * hb,
                );
            }
            return;
        }
        let Some(info) = crate::nvn::layout::format_info(t.format) else {
            return;
        };
        let p = texture_params(t);
        let layout = crate::nvn::layout::compute_layout(&p);
        let is_linear = (t.flags & 0x10 != 0) || (t.flags & 0x40 != 0);
        let levels = if is_linear { 1 } else { tex.mipmapLevelCount() };
        let layers = layout.layers.max(1) as usize;
        let mut scratch: Vec<u8> = Vec::new();
        for layer in 0..layers {
            let slice_off = layer * layout.layer_stride as usize;
            for level in 0..levels {
                let (lw, lh) = (
                    ((t.width as usize) >> level).max(1),
                    ((t.height as usize) >> level).max(1),
                );
                let (wb, hb) = (
                    (lw + info.block_w as usize - 1) / info.block_w as usize,
                    (lh + info.block_h as usize - 1) / info.block_h as usize,
                );
                let bpr = wb * info.bytes as usize;
                let (bytes_ptr, bytes_per_row): (*const u8, usize) = if is_linear {
                    let pitch = if t.stride > 0 { t.stride as usize } else { bpr };
                    ((t.storage + slice_off) as *const u8, pitch)
                } else {
                    scratch.resize(bpr * hb, 0);
                    if !crate::nvn::layout::read_texels_2d(
                        t.storage as *const u8,
                        slice_off,
                        &p,
                        level,
                        0,
                        0,
                        lw as i32,
                        lh as i32,
                        scratch.as_mut_ptr(),
                    ) {
                        continue;
                    }
                    (scratch.as_ptr(), bpr)
                };
                // Array / cube layers are Metal slices; 1D / 2D textures have a single slice 0.
                let slice = if matches!(tt, MTLTextureType::Type1D | MTLTextureType::Type2D) {
                    0
                } else {
                    layer
                };
                tex.replaceRegion_mipmapLevel_slice_withBytes_bytesPerRow_bytesPerImage(
                    MTLRegion {
                        origin: MTLOrigin { x: 0, y: 0, z: 0 },
                        size: MTLSize {
                            width: lw,
                            height: lh,
                            depth: 1,
                        },
                    },
                    level,
                    slice,
                    std::ptr::NonNull::new(bytes_ptr as *mut libc::c_void).unwrap(),
                    bytes_per_row,
                    0,
                );
            }
        }
    }
}
/// Frees Metal textures for memory that has since been reused for other textures: entries of `tex_by_desc` whose storage
/// address also backs another description, unused for SWEEP_AGE frames and not resolved by any live texture object.
/// Dynamic resolution (Kirby) re-creates its render targets at the same storage in new sizes every few seconds; without
/// this the old sizes stay allocated (memory on iOS). Not done at creation: TotK places several transient targets at one
/// storage address within every frame, and evicting the others there re-created ~36 textures per frame (some 1920x1080),
/// uploads and hashes included. On iOS (or with --tex-sweep-unshared; =0 turns it off there) textures alone at their
/// storage are freed too once unused for UNSHARED_AGE frames (areas, characters, cutscenes left behind), for memory.
/// Render targets bound at the present are never freed. --no-tex-sweep keeps everything.
/// A scaled texture (gpu::scale) whose memory a use needs at the game's size (a copy across sizes, a read into guest
/// memory): the memory stays at the game's size from now on, and its cached textures go so the next lookup makes it
/// again; its contents follow (downscale.rs).
pub(crate) fn demote_scaled(tc: &mut TexCache, tex: &Tex, guest_storage: usize, why: &str) {
    if tex_scale(tex) == 1 {
        return;
    }
    crate::gpu::scale::demote(guest_storage, || {
        format!("{}x{} {:?}, {why}", tex.width(), tex.height(), tex.pixelFormat())
    });
    let mut root = tex.clone();
    while let Some(p) = root.parentTexture() {
        root = p;
    }
    tc.demoted.insert(guest_storage, root);
    let id = storage_id(tex);
    tc.tex_by_desc.retain(|_, (t, _)| storage_id(t) != id);
    tc.textures.retain(|_, t| storage_id(t) != id);
    bump_tex_epoch();
}

pub(crate) fn sweep_shared_storage(tc: &mut TexCache, colors: &[RtRef], depth: Option<&RtRef>) {
    const SWEEP_EVERY: u64 = 300;
    const SWEEP_AGE: u64 = 300;
    const UNSHARED_AGE: u64 = 1200;
    let frame = presented_frames();
    if frame % SWEEP_EVERY != 0 {
        return;
    }
    let sweep_unshared = crate::settings::get().graphics.tex_sweep_unshared;

    // Active render targets in the current frame must never be evicted.
    let active_storages: rustc_hash::FxHashSet<usize> = colors
        .iter()
        .chain(depth)
        .filter_map(|r| unsafe { tex_info(r.texture) }.map(|(_, t)| t.storage))
        .collect();

    let mut per_storage: HashMap<usize, u32> = HashMap::default();
    for k in tc.tex_by_desc.keys() {
        *per_storage.entry(k.storage).or_default() += 1;
    }
    // Memory the game released (pool finalized, virtual range remapped, address region freed): copies of guest data
    // there are dead (the cache otherwise kept them until their storage was reused: walking around for ~9000 frames,
    // 1.5 GB of copies over 0.95 GB of resident guest memory, 2026-10-16). GPU-written textures stay: their content
    // exists only in Metal, and on the Switch it would still be in that memory.
    let mut released = crate::nvn::api::take_tex_released();
    released.sort_unstable();
    let in_released = |a: usize| {
        let i = released.partition_point(|&(s, _)| s <= a);
        i > 0 && a < released[i - 1].0 + released[i - 1].1
    };

    let old: Vec<TexDesc> = tc
        .tex_by_desc
        .iter()
        .filter(|(k, (tex, _))| {
            if active_storages.contains(&k.storage) {
                return false;
            }
            if !released.is_empty()
                && in_released(k.storage)
                && !tc.gpu_written.contains(&storage_id(tex))
            {
                return true;
            }
            let max_age = if per_storage.get(&k.storage).copied().unwrap_or(0) >= 2 {
                SWEEP_AGE
            } else if sweep_unshared {
                UNSHARED_AGE
            } else {
                return false;
            };
            tc.desc_used.get(k).map_or(true, |&f| f + max_age < frame)
        })
        .map(|(k, _)| *k)
        .collect();

    if old.is_empty() {
        return;
    }

    // Storage ids that a texture object still resolves to with its current contents.
    let live: std::collections::HashSet<u64> = tc
        .textures
        .iter()
        .filter(|(&a, _)| unsafe { tex_info(a) }.map(|(g, _)| g) == tc.tex_gens.get(&a).copied())
        .map(|(_, t)| storage_id(t))
        .collect();

    let mut freed = 0;
    for k in old {
        let (tex, _) = match tc.tex_by_desc.get(&k) {
            Some(entry) => entry,
            None => continue,
        };
        let id = storage_id(tex);
        if live.contains(&id) {
            continue;
        }
        if tc.tex_by_desc.remove(&k).is_none() {
            continue;
        }
        watch_log(k.storage, k.storage + 1, || {
            format!(
                "sweep evicted metal {:x} storage {:#x} ({}x{} fmt {}, gpu-written {})",
                id,
                k.storage,
                k.width,
                k.height,
                k.format,
                tc.gpu_written.contains(&id)
            )
        });
        tc.desc_used.remove(&k);
        tc.gpu_written.remove(&id);
        tc.tex_checked.remove(&k);
        tc.tex_sparse.remove(&k);
        tc.tex_track_gen.remove(&k);

        let to_remove: Vec<usize> = tc
            .textures
            .iter()
            .filter(|(_, t)| storage_id(t) == id)
            .map(|(&a, _)| a)
            .collect();
        for a in to_remove {
            tc.textures.remove(&a);
            tc.tex_gens.remove(&a);
            tc.tex_memo.remove(&a);
            tc.refresh_memo.remove(&a);
            tc.alias_of.remove(&a);
        }
        freed += 1;
    }
    if freed > 0 {
        bump_tex_epoch();
    }
    tc.desc_used.retain(|k, _| tc.tex_by_desc.contains_key(k));
}

/// Identity of a texture's storage: a view's parent, else the texture itself.
static TEX_SCALES: std::sync::Mutex<Option<rustc_hash::FxHashMap<u64, u32>>> = std::sync::Mutex::new(None);

/// Records a new texture's scale (an earlier texture's entry at the same address goes).
fn set_tex_scale(t: &Tex, scale: u32) {
    let mut m = TEX_SCALES.lock().unwrap();
    let m = m.get_or_insert_with(Default::default);
    if scale > 1 {
        m.insert(storage_id(t), scale);
    } else {
        m.remove(&storage_id(t));
    }
}

/// The scale a Metal texture (or a view of it) was made at (gpu::scale): its size over the game's.
pub(crate) fn tex_scale(t: &Tex) -> u32 {
    if crate::gpu::scale::factor() == 1 {
        return 1;
    }
    TEX_SCALES.lock().unwrap().as_ref().and_then(|m| m.get(&storage_id(t)).copied()).unwrap_or(1)
}

pub(crate) fn storage_id(t: &Tex) -> u64 {
    match t.parentTexture() {
        Some(p) => Retained::as_ptr(&p) as *const () as u64,
        None => Retained::as_ptr(t) as *const () as u64,
    }
}

/// GPU-rendered textures in guest memory. On the Switch the GPU writes render targets into memory the CPU shares, and
/// TotK's texture heap compaction moves textures with CPU copies (aglTextureDataMgr / tera::ImageResourceMgr): the
/// terrain normal maps it renders once at load were moved as zeros here, so the grass vertex shader saw flat-zero
/// normals and every grass fragment failed its alpha test (Hyrule Field nearly bare, issue 13). Exact emulation would
/// trap CPU reads of GPU-written pages (as Ryujinx does); this approximation copies a render target back into guest
/// memory one frame after it was drawn, but only targets not drawn in the 4 frames before (rendered once or rarely), so
/// per-frame and alternating targets cost nothing after their first use. 2D, single-sample, uncompressed color targets only.
/// --no-rt-writeback disables.
pub(crate) fn rt_writeback_enabled() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *E.get_or_init(|| !crate::settings::get().graphics.no_rt_writeback)
}

/// A write-back's GPU copy of a target into buffers, written into guest memory once the copy has run.
pub(crate) struct WritebackJob {
    t: NvnTexture,
    cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    /// (level, the level's texels in rows of width x bytes per texel)
    levels: Vec<(usize, Retained<ProtocolObject<dyn MTLBuffer>>, usize)>,
}

/// Copies last frame's write-back candidates into buffers on the GPU, after the frame's rendering (present synced the
/// issuer, so their command buffers are committed ahead), and writes the copies an earlier present made into guest
/// memory once they ran. Reading the textures back on the CPU instead (getBytes) first waited for the frame's GPU work
/// and then decompressed Apple's lossless framebuffer compression on the CPU, under the game's render-thread lock in
/// Breath of the Wild (~20% of its Presentation Thread).
/// A scaled render target's write-back: averaged down into a game-size texture and copied into buffers, in a command
/// buffer of its own (queued after the frame's). False: not possible for its format.
unsafe fn writeback_scaled(g: &Gfx, st: &mut State, real: &Tex, t: &NvnTexture, bpp: usize) -> bool {
    use objc2_metal::MTLBlitCommandEncoder;
    let levels = (t.levels.max(1) as usize).min(real.mipmapLevelCount());
    let desc = MTLTextureDescriptor::new();
    desc.setPixelFormat(real.pixelFormat());
    desc.setWidth(t.width as usize);
    desc.setHeight(t.height as usize);
    desc.setMipmapLevelCount(levels);
    desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    desc.setStorageMode(MTLStorageMode::Private);
    let Some(small) = g.device.newTextureWithDescriptor(&desc) else { return false };
    let Some(cb) = g.queue.commandBuffer() else { return false };
    if !g.encode_downscale(&cb, real, &small) {
        return false;
    }
    let Some(b) = cb.blitCommandEncoder() else { return false };
    objc2_metal::MTLCommandEncoder::setLabel(&*b, Some(&NSString::from_str("render-target write-back (scaled)")));
    let mut out = Vec::new();
    for level in 0..levels {
        let (w, h) = (((t.width as usize) >> level).max(1), ((t.height as usize) >> level).max(1));
        let len = w * h * bpp;
        let Some(buf) = g.device.newBufferWithLength_options(len, MTLResourceOptions::StorageModeShared) else {
            continue;
        };
        b.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
            &small,
            0,
            level,
            MTLOrigin { x: 0, y: 0, z: 0 },
            MTLSize { width: w, height: h, depth: 1 },
            &buf,
            0,
            w * bpp,
            len,
        );
        out.push((level, buf, len));
    }
    b.endEncoding();
    cb.commit();
    st.writeback_jobs.push(WritebackJob { t: *t, cb, levels: out });
    true
}

pub(super) unsafe fn writeback_pass(g: &Gfx, st: &mut State) {
    for job in std::mem::take(&mut st.writeback_jobs) {
        if job.cb.status() != objc2_metal::MTLCommandBufferStatus::Completed {
            st.writeback_jobs.push(job);
            continue;
        }
        // Still inside a live memory pool (see below): the game may have freed it since the copy was made.
        if crate::nvn::api::containing_pool(job.t.storage, layout_of(&job.t).total as usize).is_none() {
            continue;
        }
        for (level, buf, len) in &job.levels {
            let data = std::slice::from_raw_parts(buf.contents().as_ptr() as *const u8, *len);
            crate::nvn::api::guest_write_level(&job.t, *level, data);
        }
    }
    let done = std::mem::replace(&mut st.writeback.1, std::mem::take(&mut st.writeback.0));
    let mut blit = None;
    let mut cb_new = None;
    let mut n = 0;
    for (addr, _frame) in done {
        let Some((_, t)) = tex_info(addr) else {
            continue;
        };
        // Drawn again since (the game rendered it in the frame just submitted too): present waited for that rendering
        // to finish, so the write-back copies the newest contents now. Postponing it until the game stopped drawing the
        // target (dc6c9bf) wrote textures back long after the game had moved them (tera::ImageResourceMgr compacts its
        // terrain textures with CPU copies) into memory it had reused: SIGSEGV at TotK 1.1.2 main+0xbecbf8 on a list node full of
        // texels, reliably after unequipping a weapon in the inventory (2026-10-14). Skipping it (b743568) lost targets
        // rendered over a few consecutive frames and then kept.
        let Some(info) = crate::nvn::layout::format_info(t.format) else {
            continue;
        };
        if info.block_w != 1 || t.target != 1 || t.samples > 1 || t.storage < 0x10000 {
            continue;
        }
        // The texture may have been freed since it was drawn (the descriptor is the one recorded at initialization) and
        // its memory returned to the game's heap: writing there corrupted live game data (2026-10-14: a list node of
        // tera::ImageResourceMgr overwritten with texels, SIGSEGV at TotK 1.1.2 main+0xbecbf8 after ~7 minutes of play). On the
        // Switch the GPU wrote this memory while it was still a texture; only memory still inside a live memory pool
        // is written now.
        if crate::nvn::api::containing_pool(t.storage, layout_of(&t).total as usize).is_none() {
            if crate::settings::get().log.log_rt_writeback {
                println!(
                    "[writeback]   skipped {}x{} fmt {} storage {:#x}: not in a live memory pool",
                    t.width, t.height, t.format, t.storage
                );
            }
            continue;
        }
        let Some(tex) = g.texture(&mut st.tc, addr) else {
            continue;
        };
        let mut real = tex.clone();
        while let Some(p) = real.parentTexture() {
            real = p;
        }
        // Guest memory holds the game's size: a scaled target (gpu::scale) is averaged down into a copy at that size,
        // which is what goes back; the target keeps its scale (Ryujinx flushes scaled textures through a scaled-down
        // copy). Demoting it instead brought most of BotW's frame back to 1x within seconds.
        if tex_scale(&tex) > 1 {
            if real.sampleCount() > 1
                || crate::gpu::metal::copy::is_depth_stencil(real.pixelFormat())
                || real.textureType() != MTLTextureType::Type2D
                || !writeback_scaled(g, st, &real, &t, info.bytes as usize)
            {
                demote_scaled(&mut st.tc, &tex, t.storage as usize, "written back to guest memory");
            } else {
                n += 1;
            }
            continue;
        }
        if objc2_metal::MTLResource::storageMode(&*real) == MTLStorageMode::Private
            || real.sampleCount() > 1
            || crate::gpu::metal::copy::is_depth_stencil(real.pixelFormat())
            || real.textureType() != MTLTextureType::Type2D
        {
            continue;
        }
        if cb_new.is_none() {
            let Some(cb) = g.queue.commandBuffer() else { break };
            let Some(b) = cb.blitCommandEncoder() else { break };
            objc2_metal::MTLCommandEncoder::setLabel(&*b, Some(&NSString::from_str("render-target write-back")));
            blit = Some(b);
            cb_new = Some(cb);
        }
        use objc2_metal::MTLBlitCommandEncoder;
        let (Some(b), Some(cb)) = (blit.as_ref(), cb_new.as_ref()) else { break };
        let mut levels = Vec::new();
        for level in 0..(t.levels.max(1) as usize).min(real.mipmapLevelCount()) {
            let (w, h) = (
                ((t.width as usize) >> level).max(1),
                ((t.height as usize) >> level).max(1),
            );
            let bpp = info.bytes as usize;
            let len = w * h * bpp;
            let Some(buf) = g.device.newBufferWithLength_options(len, MTLResourceOptions::StorageModeShared) else {
                continue;
            };
            b.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                &real,
                0,
                level,
                MTLOrigin { x: 0, y: 0, z: 0 },
                MTLSize { width: w, height: h, depth: 1 },
                &buf,
                0,
                w * bpp,
                len,
            );
            levels.push((level, buf, len));
        }
        st.writeback_jobs.push(WritebackJob { t, cb: cb.clone(), levels });
        if crate::settings::get().log.log_rt_writeback {
            println!(
                "[writeback]   {}x{} fmt {} levels {} storage {:#x}",
                t.width, t.height, t.format, t.levels, t.storage
            );
        }
        n += 1;
    }
    if let (Some(b), Some(cb)) = (blit, cb_new) {
        b.endEncoding();
        cb.commit();
    }
    if n > 0 && crate::settings::get().log.log_rt_writeback {
        println!(
            "[writeback] f{} wrote {} render target(s) back to guest memory",
            presented_frames(),
            n
        );
    }
}

pub(crate) static UPLOADS: std::sync::Mutex<Option<HashMap<u64, (usize, u64, u64)>>> =
    std::sync::Mutex::new(None);

pub(crate) fn upload_record(id: u64) -> Option<(usize, u64, u64)> {
    UPLOADS.lock().unwrap().as_ref()?.get(&id).copied()
}

pub(crate) unsafe fn current_guest_hash(nvn_tex: usize) -> Option<(usize, u64)> {
    tex_info(nvn_tex).map(|(_, t)| (t.storage, guest_bytes_hash(&t)))
}

/// `tex_by_desc` hash of an entry whose guest bytes were not hashed yet (see the alias path in `texture`).
pub(crate) const HASH_UNKNOWN: u64 = u64::MAX;

/// Bytes fully hashed by `refresh_cpu_written` (printed per frame with the fps line).
pub(crate) static REFRESH_HASHED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Fast hash of a texture's guest bytes (detects new data streamed into reused memory).
pub(crate) unsafe fn guest_bytes_hash(t: &NvnTexture) -> u64 {
    if t.storage <= 0x10000 {
        return 0;
    }
    let len = layout_of(t).total as usize;
    let words = std::slice::from_raw_parts(t.storage as *const u64, len / 8);
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15 ^ len as u64;
    for (i, w) in words.iter().enumerate() {
        h = (h ^ w.wrapping_add(i as u64))
            .rotate_left(27)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    h
}

/// Cheap content fingerprint: a few words from each mip level of each layer (up to 8 per level, spread over its pages).
/// Streamed texels replace whole levels, so any new data changes it.
pub(crate) unsafe fn sparse_bytes_hash(t: &NvnTexture, layout: &crate::nvn::layout::TextureLayout) -> u64 {
    if t.storage <= 0x10000 {
        return 0;
    }
    let base = t.storage as *const u8;
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15 ^ layout.total;
    for layer in 0..layout.layers.max(1) as u64 {
        for (&off, &size) in layout.level_offsets.iter().zip(&layout.level_sizes) {
            let n = (size / 4096).clamp(1, 8);
            let step = size / n;
            for k in 0..n {
                let o = layer * layout.layer_stride + off + (k * step + step / 2) / 8 * 8;
                if o + 8 > layout.total {
                    continue;
                }
                let w = std::ptr::read_unaligned(base.add(o as usize) as *const u64);
                h = (h ^ w.wrapping_add(o))
                    .rotate_left(27)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15);
            }
        }
    }
    h
}
