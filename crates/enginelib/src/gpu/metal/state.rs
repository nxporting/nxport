// The executor's state: per-frame State, the texture cache, encode context and caches shared with the encode worker,
// binding tables, pending clears, uniform copies.

use super::*;

#[derive(Clone)]
pub(crate) struct CachedPassRts {
    pub(super) color_texs: smallvec::SmallVec<[(usize, Tex, RtRef); 8]>,
    pub(super) depth_tex: Option<(Tex, RtRef)>,
    pub(super) color_pfs: [MTLPixelFormat; 8],
    pub(super) depth_pf: MTLPixelFormat,
    /// The targets' NVN formats (texture, view; pipeline lists) and sample count.
    pub(super) nvn_colors: [(u32, u32); 8],
    pub(super) nvn_depth: (u32, u32),
    pub(super) samples: u32,
    pub(super) enc_key: smallvec::SmallVec<[u64; 36]>,
    pub(super) target_size: (f64, f64),
    /// The targets' common scale (gpu::scale): game coordinates (viewports, scissors) times this are the targets'.
    pub(super) scale: u32,
    /// Block-compressed targets rendered through proxies: (proxy, target, level), see `bc_proxy`.
    pub(super) bc: smallvec::SmallVec<[(Tex, Tex, usize); 1]>,
}

pub(crate) struct State {
    pub(crate) colors: Vec<RtRef>,
    /// Per color slot: the earlier slot whose surface it repeats (the slot itself is left unattached), see
    /// SetRenderTargets.
    pub(crate) color_dups: Vec<Option<usize>>,
    pub(crate) depth: Option<RtRef>,
    pub(crate) program: Option<usize>,
    /// Bound compute program (NVN binds graphics and compute programs independently).
    pub(crate) compute_program: Option<usize>,
    pub(crate) compute_pipelines: HashMap<String, Retained<ProtocolObject<dyn MTLComputePipelineState>>>,
    pub(crate) viewport: Option<(i32, i32, i32, i32)>,
    pub(crate) scissor: Option<(i32, i32, i32, i32)>,
    pub(crate) vertex_buffers: HashMap<i32, (u64, usize)>,
    pub(crate) uniform_buffers: BindTable<(u64, usize)>,
    /// Bound storage buffers by [stage][index] (size 0 = unbound); read for every draw.
    pub(crate) storage_buffers: [[(u64, usize); 16]; 6],
    pub(crate) vertex_attribs: crate::nvn::cmd::AttribState,
    pub(crate) vertex_streams: Vec<u64>,
    pub(crate) textures_bound: BindTable<u64>,
    /// Render targets to copy back into guest memory, with the frame they were bound in: (bound this frame, bound last
    /// frame); see `writeback_pass`.
    pub(crate) writeback: (HashMap<usize, u64>, HashMap<usize, u64>),
    /// Write-back copies in flight (textures.rs writeback_pass).
    pub(crate) writeback_jobs: Vec<super::WritebackJob>,
    /// Storage -> last frame a render target over it was bound (picks the rarely rendered ones for write-back).
    pub(crate) rt_history: HashMap<usize, u64>,
    /// nvnCommandBufferBindImage: (stage, image unit) -> image handle (pool id).
    pub(crate) images_bound: BindTable<u64>,
    /// Queue render enable (nvnCommandBufferSetRenderEnable): when false, draws and clears are discarded.
    pub(crate) render_enabled: bool,
    /// nvnCommandBufferSetRasterizerDiscard: draws make no fragments, so they are skipped.
    pub(crate) rasterizer_discard: bool,
    /// SAMPLES_PASSED occlusion queries (nvnCommandBufferResetCounter / ReportCounter counter 1).
    pub(super) occl: Occlusion,
    /// TIMESTAMP reports recorded into the command buffer being built: (address, before any GPU work in it); see
    /// gpu_timestamps.
    pub(crate) ts_pending: Vec<(u64, bool)>,
    /// nvnCommandBufferSetPointSize (used when the vertex shader does not write gl_PointSize).
    pub(super) point_size: f32,
    /// NVNblendState per color target (bits 0..3 of the state select the target; 0 = not bound since reset).
    pub(crate) blend_states: [u64; 8],
    pub(crate) color_state: u32,
    pub(crate) channel_mask: u32,
    pub(crate) stats: Stats,
    // ---- translated draw path (metal/translated.rs) ----
    pub(crate) depth_stencil: u64,
    pub(crate) polygon: u32,
    pub(crate) depth_range: (f32, f32),
    /// Stencil reference, read (value) mask and write mask, [front, back] (defaults: ref 0, masks 0xff).
    pub(crate) stencil: [[u8; 2]; 3],
    /// Depth bounds test (enable, min, max): fragments whose stored depth is outside the range are discarded.
    pub(crate) depth_bounds: Option<(f32, f32)>,
    pub(crate) depth_clamp: bool,
    /// Primitive restart index, when enabled (nvnCommandBufferSetPrimitiveRestart).
    pub(super) prim_restart: Option<u32>,
    /// Bound NVNmultisampleState bits (alpha-to-coverage is bit 1).
    pub(super) multisample: u64,
    /// nvnCommandBufferSetPolygonOffsetClamp: (factor, units, clamp).
    pub(super) polygon_offset: (f32, f32, f32),
    pub(crate) blend_color: [f32; 4],
    /// Color targets drawn into since the last present, in first-use order, with draw counts (frame dumps).
    pub(crate) frame_rts: Vec<(u64, Tex, u32)>,
    /// Debug (--snapshot-enc): command buffer being recorded, the open encoder's label and color targets, and
    /// copies of targets taken when a listed encoder ended (written out with the frame dump).
    pub(crate) cur_cb: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    /// When encoding into `cur_cb` began (time-based commits, see maybe_split).
    pub(crate) cb_started: Option<std::time::Instant>,
    pub(crate) enc_name: String,
    pub(crate) enc_targets: Vec<(usize, Tex)>,
    pub(crate) snapshots: Vec<(String, Tex)>,
    /// Depth attachment of the current encoder (texture, slice, level), for --snapshot-depth.
    pub(super) enc_depth: Option<(Tex, usize, usize)>,
    /// Depth snapshots: (label, buffer, width, height, bytes per pixel).
    pub(crate) depth_snapshots: Vec<(
        String,
        Retained<ProtocolObject<dyn MTLBuffer>>,
        usize,
        usize,
        usize,
    )>,
    /// Uncompressed stand-ins for block-compressed render targets the game writes through a same-size integer view
    /// (GPU texture compression), keyed by (target texture, level, view format); see `bc_proxy`.
    pub(crate) bc_proxies: HashMap<(usize, usize, u64), Tex>,
    /// Partial / masked texture clears: pipelines by (pixel format, channel mask) (copy.rs clear_region).
    pub(crate) clear_pipes: HashMap<(usize, u8), Retained<ProtocolObject<dyn MTLRenderPipelineState>>>,
    /// Render targets bound through a texture view of another format (e.g. RGBA8 drawn through an RGBA8_SRGB view).
    pub(crate) rt_format_views: HashMap<(usize, u64), Tex>,
    /// Proxies rendered in the current encoder, copied into their compressed targets when it ends: (proxy, target, level).
    pub(crate) bc_pending: Vec<(Tex, Tex, usize)>,
    /// Clears not yet encoded (see `PendingClear`).
    pub(super) pending_clears: Vec<PendingClear>,
    /// Guest memory written by GPU work (buffer copies / clears, compute storage buffers) in the current submit. A
    /// uniform buffer in such a range is bound zero-copy so it sees those writes in GPU order (the encode-time copy
    /// would capture the contents from before them, e.g. a bone palette the GPU fills, leaving characters in T-pose).
    /// Merged, non-overlapping [start, end) ranges keyed by start (gpu_written: one lookup per uniform binding).
    pub(crate) gpu_dirty: std::collections::BTreeMap<u64, u64>,
    /// --early-fence: gpu_dirty ranges of earlier submits, with the last serial their work can be in. The game may
    /// encode its next frame before that work completed; uniform buffers in these ranges stay zero-copy until it has.
    pub(crate) carried_dirty: std::collections::VecDeque<(u64, Vec<(u64, u64)>)>,
    /// Render encoder kept open across consecutive draws to the same targets (see metal/translated.rs), with its target key.
    /// The open translated render encoder: its calls are queued to `issuer` (see metal/issuer.rs).
    pub(super) enc: Option<issuer::Issuer>,
    pub(super) issuer: issuer::Issuer,
    pub(crate) enc_key: smallvec::SmallVec<[u64; 36]>,
    pub(super) rts_dirty: bool,
    pub(super) cached_rts: Option<std::sync::Arc<CachedPassRts>>,
    /// Draws issued in the current shared encoder (matches Metal validation's per-encoder draw numbers).
    pub(crate) enc_draws: u32,
    /// Uniform data the plan copies because the submit itself rewrites it later (see translated::plan_uniforms), and
    /// the arena those copies live in.
    pub(super) plan_ubo: UboCopies,
    pub(super) plan_arena: translated::Arena,
    /// GUEST_WRITES value when plan_ubo was last validated.
    pub(super) ubo_epoch_plan: u64,
    /// plan_uniforms' decision per (stage, Metal binding), valid while the binding's (address, size, copy length) and
    /// ubo_plan_epoch are unchanged: ~21k uniform bindings per frame read ~450 binds' worth of buffers.
    pub(super) ubo_decisions: Box<[[Option<translated::UboDecision>; 32]; 2]>,
    /// Bumped by everything that can change a uniform decision: GPU-written ranges, executor writes (inline updates,
    /// reports), a new submit.
    pub(super) ubo_plan_epoch: u64,
    /// The previous draw's texture resolution and its key (translated::plan_draw).
    pub(super) tex_reuse: Option<(translated::TexReuseKey, translated::ResolvedTex)>,
    /// The draw packet reused by every translated draw (translated::draw_translated).
    pub(super) draw_packet: Option<Box<translated::DrawPacket>>,
    pub(super) replay: ReplayStats,
    /// Ranges UpdateUniformBuffer commands of the current submit write (uniform buffers there are copied per draw).
    pub(crate) ubo_updated: Vec<(u64, u64)>,
    /// Scratch for cur_cb_and_ranges.
    pub(crate) lists_stack: Vec<u64>,
    /// Submit-scoped cache of command lists reachable in the current submit (avoids COMMAND_LISTS mutex locks).
    pub(crate) submit_lists: HashMap<u64, std::sync::Arc<crate::nvn::api::RecordedCommandList>>,
    /// --log-draw-reuse: the list (Arc pointer) and op index being executed, whether the list is from an earlier frame.
    pub(crate) cur_list_op: Option<(usize, usize, bool)>,
    /// Frame traces: the game caller (main+ offset) of the draw being executed (--log-draw-callers).
    pub(super) cur_caller: u32,
    /// --log-draw-reuse: plan hash of each (list, op) draw's previous run.
    pub(crate) draw_reuse: rustc_hash::FxHashMap<(usize, usize, u64), [u64; 12]>,
    /// --log-draw-reuse: hash of the call path (parent list and op index at each level).
    pub(crate) call_ctx: u64,
    /// Commit at the next opportunity (a long render pass was cut, see split_draws).
    pub(crate) force_split: bool,
    /// Recorded NVN commands the executor has no handler for, counted per frame (--cmd-stats-at=N prints frame N).
    pub(crate) ignored_cmds: HashMap<&'static str, u64>,
    /// Render-target feedback (see translated::feedback_snapshots): bumped by every nvnCommandBufferBarrier that orders
    /// fragments and invalidates the texture cache. A draw that samples an attachment of its own pass reads a copy of it
    /// taken at the first such draw after the barrier: on the Switch it sees the writes from before the barrier, while
    /// sampling an attachment inside a Metal render pass reads whatever happens to be in memory.
    pub(crate) fb_epoch: u64,
    /// fb_epoch when the current render encoder began (in-place render-target feedback, translated::feedback_snapshots).
    pub(super) enc_fb_epoch: u64,
    /// The command being executed comes from a called list recorded in an earlier frame (--log-phases).
    pub(super) in_old_list: bool,
    /// The last two frames each render target (by storage id) began a pass in, the latest first, and the frame these
    /// were last pruned in: whether the current pass's targets are drawn every frame (`enc_recur`).
    pub(super) rt_frames: HashMap<u64, (u64, u64)>,
    pub(super) rt_frames_pruned: u64,
    /// One of the current render encoder's targets was drawn in the previous frame too: a draw skipped there while its
    /// pipeline compiles is drawn again next frame. Elsewhere (a target drawn now and then) its draw waits.
    pub(super) enc_recur: bool,
    /// Texture pointer -> root id, for the current frame (translated::feedback_snapshots; holds the textures).
    pub(crate) root_memo: HashMap<usize, (Tex, u64)>,
    /// Submitted command buffers the GPU may still be running, oldest first (at most `frames_in_flight()`).
    pub(super) inflight: std::collections::VecDeque<InFlight>,
    /// Set after the first GPU error: all further rendering is skipped (see `submit`).
    pub(crate) gpu_halted: bool,
    /// Consecutive command buffers discarded as innocent victims of a GPU recovery caused by something else.
    pub(crate) victims_in_a_row: u32,
    /// Last window-present command buffer, checked for errors on the next present (it is never waited on).
    pub(crate) present_cb: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    pub(crate) warned_prims: Vec<i32>,
    /// Texture resolution caches (Metal textures by object / description / alias, memos, CPU-write checks): one
    /// struct so it can get its own lock (parallel encoding).
    pub(super) tc: TexCache,
    /// Per-encoding-thread state: uniform arena, redundant-state filter, uniform copies, draw statistics (one per worker with parallel encoding).
    pub(super) cx: EncodeCtx,
    /// Caches the draw path shares: pipelines, samplers, depth-stencil states, constant banks, pool and repack buffers (rare inserts).
    pub(super) sh: std::sync::Arc<Mutex<SharedCaches>>,
    /// Labels and per-draw details of the encoders in the command buffer being encoded (error and hang reports).
    pub(super) lb: EncLabels,
}

/// See State::tc.
pub(crate) struct TexCache {
    pub(super) textures: HashMap<usize, Tex>,
    /// Snapshot generation each cached Metal texture was built from (see `tex_info`).
    pub(super) tex_gens: HashMap<usize, u64>,
    /// Texture object -> (generation, frame) of its last CPU-write check (see bound_texture).
    pub(super) refresh_memo: HashMap<usize, (Option<u64>, u64)>,
    pub(super) dummy_textures: HashMap<String, Tex>,
    /// Write generation (guest/dirty_track.rs) each write-tracked texture description was last hashed at.
    pub(super) tex_track_gen: HashMap<TexDesc, u64>,
    /// Gfx::texture's fast path: (horizon::hle::tex_table_epoch, horizon::hle::texture_dirty_gen, generation) when the object last resolved
    /// to its cached Metal texture with nothing to upload.
    pub(super) tex_memo: HashMap<usize, (u64, u64, u64)>,
    /// NVNtexture -> (root NVNtexture, root generation) for textures resolved as views of another texture's storage.
    pub(super) alias_of: HashMap<usize, (usize, u64)>,
    /// Metal textures by full description, so an NVNtexture re-initialized with the same description (the game
    /// rebuilds its render-target objects every frame) reuses its texture instead of re-creating and re-uploading
    /// it. Value: (texture, hash of the guest bytes last uploaded). Bounded per storage address.
    pub(super) tex_by_desc: HashMap<TexDesc, (Tex, u64)>,
    /// Frame each `tex_by_desc` entry was last resolved through (creation, alias view, by-description lookup), for
    /// `sweep_shared_storage`.
    pub(super) desc_used: HashMap<TexDesc, u64>,
    /// Depth twins of colour textures sampled with depth comparison (translated.rs depth_twin), by the colour texture's
    /// storage id: (twin, the colour object's generation it was filled at).
    pub(super) depth_twins: HashMap<u64, (Tex, u64)>,
    /// Frame each texture description was last checked for CPU-side changes (see `refresh_cpu_written`).
    pub(super) tex_checked: HashMap<TexDesc, u64>,
    /// Sparse content samples of compressed textures as of their last full check (see `refresh_cpu_written`).
    pub(super) tex_sparse: HashMap<TexDesc, u64>,
    /// Texture views made for bindings (see translated::texture_for_binding).
    pub(super) view_cache: HashMap<(usize, usize, usize, [usize; 4], [isize; 4]), (Tex, u64)>,
    /// Bytes hashed for change detection in the current frame (bounded per frame).
    pub(super) tex_check_budget_used: usize,
    /// Storage identities (see `storage_id`) the GPU has rendered or cleared into: their content lives in Metal, so
    /// they are never refreshed from guest memory.
    pub(super) gpu_written: rustc_hash::FxHashSet<u64>,
    /// Per storage identity: the serial of its last GPU write (mark_gpu_written), and the counter they come from.
    pub(super) gpu_write_seq: rustc_hash::FxHashMap<u64, u64>,
    pub(super) gpu_write_counter: u64,
    /// Depth twins of GPU-written colour textures (translated::depth_twin): the twin and the write serial it holds.
    pub(super) gpu_twins: rustc_hash::FxHashMap<u64, (Tex, u64)>,
    /// Twins a texture resolution found out of date: (source, twin, storage identity), copied before the draw.
    pub(super) twin_pending: Vec<(Tex, Tex, u64)>,
    /// Scaled textures (gpu::scale) demoted, by guest storage address, until the texture at the game's size is made.
    pub(super) demoted: HashMap<usize, Tex>,
    /// (scaled texture, its replacement at the game's size): contents to bring down before the next draw or copy.
    pub(super) downscale_pending: Vec<(Tex, Tex)>,
    /// Resolutions that returned such a twin: they are neither memoized nor reused (the twin may need a copy first).
    pub(super) twin_uses: u64,
    /// alias_view's views by (root, format, type, levels, slices).
    pub(super) alias_views:
        HashMap<(usize, usize, usize, usize, usize, usize, usize), (Tex, Tex, u64)>,
    /// alias_views' key by view object (the views are held there, so these pointers stay valid while listed).
    pub(super) alias_view_keys: HashMap<usize, (usize, usize, usize, usize, usize, usize, usize)>,
    /// Set by alias_view when the root it found was re-initialized before it could be resolved (search again).
    pub(super) alias_raced: bool,
}

impl Default for TexCache {
    fn default() -> Self {
        TexCache {
            textures: HashMap::default(),
            tex_gens: HashMap::default(),
            refresh_memo: HashMap::default(),
            dummy_textures: HashMap::default(),
            tex_track_gen: HashMap::default(),
            tex_memo: HashMap::default(),
            alias_views: HashMap::default(),
            alias_view_keys: HashMap::default(),
            alias_raced: false,
            alias_of: HashMap::default(),
            tex_by_desc: HashMap::default(),
            desc_used: HashMap::default(),
            depth_twins: HashMap::default(),
            tex_checked: HashMap::default(),
            tex_sparse: HashMap::default(),
            view_cache: HashMap::default(),
            tex_check_budget_used: 0,
            gpu_written: Default::default(),
            gpu_write_seq: Default::default(),
            gpu_write_counter: 0,
            gpu_twins: Default::default(),
            twin_pending: Vec::new(),
            demoted: HashMap::default(),
            downscale_pending: Vec::new(),
            twin_uses: 0,
        }
    }
}

impl TexCache {
    /// The GPU wrote the storage `id`: its contents live in Metal (gpu_written), newer than any depth twin of it.
    pub(super) fn mark_gpu_written(&mut self, id: u64) {
        self.gpu_written.insert(id);
        self.gpu_write_counter += 1;
        self.gpu_write_seq.insert(id, self.gpu_write_counter);
    }
}

/// See State::cx.
pub(crate) struct EncodeCtx {
    pub(super) arena: translated::Arena,
    /// Ryujinx support buffers by viewport size (its only input). Arena chunks are never written again after reset,
    /// so a cached allocation stays valid.
    pub(super) support_cache: HashMap<(u32, u32, u64), (Retained<ProtocolObject<dyn MTLBuffer>>, usize)>,
    pub(super) translated_draws: u64,
    /// (draws with instances > 1, largest instance count, sum of instance counts, largest vertex/index count)
    pub(super) instanced_stats: (u64, i64, i64, i64),
    /// Uniform buffer copies made in this submit, by (guest address, length); see metal/translated.rs (UBO binding).
    pub(super) ubo_copies: UboCopies,
    /// GUEST_WRITES value when `ubo_copies` was last validated.
    pub(super) ubo_epoch: u64,
    /// State bound in the open render encoder (translated::EncCache).
    pub(super) ec: translated::EncCache,
    /// (vertex entry, fragment entry, first color target format raw, depth format raw) -> draws
    /// Keyed by the stages' addresses (&'static translated::CachedStage, 0 = no fragment stage).
    pub(super) translated_hist: HashMap<(usize, usize, u64, u64), u64>,
    /// This thread's texture binding memos (validated by global epochs; a miss resolves through the texture cache).
    pub(super) bind_memo: HashMap<(i32, usize), BindMemo>,
    /// encode_draw's vertex layout + pipeline by their inputs (translated::LayoutMemo).
    pub(super) layout_memo: HashMap<Vec<u64>, std::sync::Arc<translated::LayoutMemo>>,
    /// The last draw's layout memo inputs and result (checked before the map).
    /// Index scans of this submit by (index address, count, index type, restart): translated::cached_max_vertex.
    pub(super) max_index_memo: HashMap<(u64, i32, i32, Option<u32>), Option<u64>>,
    pub(super) layout_last: Option<(
        smallvec::SmallVec<[u64; 64]>,
        std::sync::Arc<translated::LayoutMemo>,
    )>,
    /// The last driver cbuf (binding 17 / guest cbuf 0) per stage by its inputs: consecutive draws share one copy (a
    /// fresh one per draw costs a 4 KB fill and splits batched runs, metal/batch.rs). Same lifetime as support_cache.
    pub(super) driver_cb_memo: [Option<(
        translated::DriverCbKey,
        (Retained<ProtocolObject<dyn MTLBuffer>>, usize),
    )>; 2],
    /// The last index buffer copy (restart rewrite, u8 widening, misaligned range) by its inputs: (key, buffer, offset,
    /// index count). Same lifetime as support_cache.
    pub(super) index_memo: Option<(
        (i32, i32, u64, i32, Option<u32>, i32, u8),
        Retained<ProtocolObject<dyn MTLBuffer>>,
        usize,
        usize,
    )>,
    /// Texture and sampler descriptor heaps of this encoding thread (stages built with --texture-heap).
    pub(super) heaps: tex_heap::TexHeaps,
}

impl Default for EncodeCtx {
    fn default() -> Self {
        EncodeCtx {
            bind_memo: HashMap::default(),
            layout_memo: HashMap::default(),
            layout_last: None,
            max_index_memo: HashMap::default(),
            arena: translated::Arena::default(),
            support_cache: HashMap::default(),
            translated_draws: 0,
            instanced_stats: (0, 0, 0, 0),
            ec: translated::EncCache::default(),
            ubo_copies: UboCopies::default(),
            ubo_epoch: 0,
            translated_hist: HashMap::default(),
            driver_cb_memo: [None, None],
            index_memo: None,
            heaps: Default::default(),
        }
    }
}

/// See State::sh.
pub(crate) struct SharedCaches {
    /// Tiny-pass kernels by (function name, function constants): translated::emit_tiny.
    pub(super) tiny_pipes: HashMap<
        (usize, Vec<i64>),
        Retained<ProtocolObject<dyn objc2_metal::MTLComputePipelineState>>,
    >,
    pub(super) zero_buffer: Option<Retained<ProtocolObject<dyn MTLBuffer>>>,
    /// Repacked vertex streams by (source address, bound size, stride, layout) (see draw_translated).
    pub(super) repack_cache: HashMap<(u64, usize, u64, u64), RepackEntry>,
    pub(super) repack_bytes: usize,
    /// (stage data, cache entry: CachedStage pointer, 'static) -> the stage's cbuf 1.
    pub(super) const_banks: HashMap<(u64, usize), Retained<ProtocolObject<dyn MTLBuffer>>>,
    /// Every sampler state made, by descriptor: one Metal object per distinct sampler (samplers that can be used
    /// through argument buffers are a limited device resource, and pool slots repeat descriptors).
    pub(super) sampler_states: Vec<(crate::nvn::api::SamplerDesc, Retained<ProtocolObject<dyn MTLSamplerState>>)>,
    pub(super) samplers: HashMap<
        (i32, bool),
        (
            crate::nvn::api::SamplerDesc,
            Retained<ProtocolObject<dyn MTLSamplerState>>,
        ),
    >,
    pub(super) default_sampler: Option<Retained<ProtocolObject<dyn MTLSamplerState>>>,
    pub(super) ds_states: HashMap<u128, Retained<ProtocolObject<dyn MTLDepthStencilState>>>,
    pub(super) translated_pipelines:
        HashMap<Vec<u64>, Retained<ProtocolObject<dyn MTLRenderPipelineState>>>,
    /// Pipeline keys whose build failed (not retried).
    pub(super) translated_pipelines_bad: std::collections::HashSet<Vec<u64>>,
    pub(super) translated_pipelines_failed: u64,
}

// Metal objects may be used from any thread; the caches are only touched under their mutex.
unsafe impl Send for SharedCaches {}

impl Default for SharedCaches {
    fn default() -> Self {
        SharedCaches {
            tiny_pipes: HashMap::default(),
            zero_buffer: None,
            repack_cache: HashMap::default(),
            repack_bytes: 0,
            const_banks: HashMap::default(),
            samplers: HashMap::default(),
            sampler_states: Vec::new(),
            default_sampler: None,
            ds_states: HashMap::default(),
            translated_pipelines: HashMap::default(),
            translated_pipelines_bad: Default::default(),
            translated_pipelines_failed: 0,
        }
    }
}

/// See State::lb.
pub(crate) struct EncLabels {
    /// One entry per encoder created in the current command buffer, in order (what it was for): Metal reports failures
    /// by encoder index.
    pub(super) enc_labels: Vec<String>,
    /// (vertex, fragment) shader identities already named in the current encoder's label.
    pub(super) enc_label_pairs: Vec<(usize, usize)>,
    /// Per encoder: the CPU-side state of each draw in it (bindings, buffers, viewport...), printed for the encoder that
    /// stalled when the GPU reports an error.
    pub(super) enc_details: Vec<Vec<String>>,
}

impl Default for EncLabels {
    fn default() -> Self {
        EncLabels {
            enc_labels: Vec::new(),
            enc_label_pairs: Vec::new(),
            enc_details: Vec::new(),
        }
    }
}

/// Per-stage binding table keyed by (stage, slot): a fixed array for the usual range, a map beyond it. Looked up for
/// every binding of every draw (a hash map lookup each showed in profiles).
pub(crate) struct BindTable<V: Copy> {
    slots: Box<[Option<V>; BIND_STAGES * BIND_SLOTS]>,
    extra: HashMap<(i32, i32), V>,
}

pub(crate) const BIND_STAGES: usize = 8;

pub(crate) const BIND_SLOTS: usize = 96;

impl<V: Copy> Default for BindTable<V> {
    fn default() -> Self {
        BindTable {
            slots: Box::new([None; BIND_STAGES * BIND_SLOTS]),
            extra: HashMap::default(),
        }
    }
}

impl<V: Copy> BindTable<V> {
    fn index(k: &(i32, i32)) -> Option<usize> {
        let (s, i) = (k.0 as usize, k.1 as usize);
        (k.0 >= 0 && k.1 >= 0 && s < BIND_STAGES && i < BIND_SLOTS).then_some(s * BIND_SLOTS + i)
    }

    pub(super) fn get(&self, k: &(i32, i32)) -> Option<&V> {
        match Self::index(k) {
            Some(i) => self.slots[i].as_ref(),
            None => self.extra.get(k),
        }
    }

    pub(super) fn insert(&mut self, k: (i32, i32), v: V) -> Option<V> {
        match Self::index(&k) {
            Some(i) => self.slots[i].replace(v),
            None => self.extra.insert(k, v),
        }
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = ((i32, i32), &V)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                v.as_ref()
                    .map(|v| (((i / BIND_SLOTS) as i32, (i % BIND_SLOTS) as i32), v))
            })
            .chain(self.extra.iter().map(|(k, v)| (*k, v)))
    }
}

impl<V: Copy + std::fmt::Debug> std::fmt::Debug for BindTable<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            lb: EncLabels::default(),
            sh: std::sync::Arc::new(Mutex::new(SharedCaches::default())),
            cx: EncodeCtx::default(),
            tc: TexCache::default(),
            colors: Vec::new(),
            color_dups: Vec::new(),
            depth: None,
            program: None,
            compute_program: None,
            compute_pipelines: HashMap::default(),
            viewport: None,
            scissor: None,
            vertex_buffers: HashMap::default(),
            storage_buffers: [[(0, 0); 16]; 6],
            uniform_buffers: BindTable::default(),
            vertex_attribs: Default::default(),
            vertex_streams: Vec::new(),
            textures_bound: BindTable::default(),
            images_bound: BindTable::default(),
            writeback: Default::default(),
            writeback_jobs: Vec::new(),
            rt_history: HashMap::default(),
            render_enabled: true,
            rasterizer_discard: false,
            occl: Occlusion::default(),
            point_size: 1.0,
            blend_states: [0; 8],
            color_state: 1,
            channel_mask: 0xF,
            stats: Stats::default(),
            // NVNdepthStencilState / NVNpolygonState defaults: depth write on, LESS; cull none, CCW, fill.
            depth_stencil: (1 << 1) | (2 << 2),
            polygon: (1 << 2) | (2 << 3),
            depth_range: (0.0, 1.0),
            depth_bounds: None,
            stencil: [[0, 0], [0xff, 0xff], [0xff, 0xff]],
            depth_clamp: false,
            prim_restart: None,
            polygon_offset: (0.0, 0.0, 0.0),
            multisample: 0,
            blend_color: [0.0; 4],
            frame_rts: Vec::new(),
            cur_cb: None,
            cb_started: None,
            enc_name: String::new(),
            enc_targets: Vec::new(),
            snapshots: Vec::new(),
            enc_depth: None,
            depth_snapshots: Vec::new(),
            bc_proxies: HashMap::default(),
            clear_pipes: HashMap::default(),
            ts_pending: Vec::new(),
            rt_format_views: HashMap::default(),
            bc_pending: Vec::new(),
            pending_clears: Vec::new(),
            gpu_dirty: Default::default(),
            carried_dirty: Default::default(),
            enc_draws: 0,
            draw_packet: None,
            tex_reuse: None,
            plan_ubo: UboCopies::default(),
            plan_arena: Default::default(),
            ubo_epoch_plan: 0,
            ubo_decisions: Box::new(std::array::from_fn(|_| std::array::from_fn(|_| None))),
            ubo_plan_epoch: 0,
            replay: ReplayStats::default(),
            ubo_updated: Vec::new(),
            lists_stack: Vec::new(),
            submit_lists: HashMap::default(),
            cur_list_op: None,
            cur_caller: 0,
            draw_reuse: Default::default(),
            call_ctx: 0,
            force_split: false,
            ignored_cmds: HashMap::default(),
            fb_epoch: 0,
            enc_fb_epoch: 0,
            in_old_list: false,
            rt_frames: HashMap::default(),
            rt_frames_pruned: 0,
            enc_recur: false,
            root_memo: HashMap::default(),
            enc: None,
            issuer: issuer::Issuer::new(),
            enc_key: smallvec::SmallVec::new(),
            rts_dirty: true,
            cached_rts: None,
            inflight: std::collections::VecDeque::new(),
            gpu_halted: false,
            victims_in_a_row: 0,
            present_cb: None,
            warned_prims: Vec::new(),
        }
    }
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Stats {
    pub(crate) submits: u64,
    pub(crate) clears: u64,
    pub(crate) copies: u64,
    pub(crate) draws: u64,
    pub(crate) passes: u64,
}

// Debug: --enc-stats prints, every 600 presents, which call sites ended render encoders and how many translated
// passes reopened the targets of the pass just ended (merge candidates).
/// A clear waiting to be folded into the next pass on its target or encoded on its own (`Gfx::defer_clear`).
/// A cached repacked vertex stream: its buffer, vertices covered, hash of the source bytes, frame last validated.
pub(crate) struct RepackEntry {
    pub(super) buf: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub(super) cap: usize,
    pub(super) src_hash: u64,
    pub(super) checked: u64,
}

/// A texture binding's resolution, valid while the pool slot, the object's generation, the dirty-texture counter, the
/// texture-cache epoch and (for alias views) the root's generation are unchanged, within one submit.
pub(crate) struct BindMemo {
    pub(super) tex_addr: usize,
    pub(super) view: Option<[u8; 16]>,
    pub(super) gen: Option<u64>,
    pub(super) dirty_gen: u64,
    pub(super) epoch: u64,
    pub(super) alias_root: Option<(usize, u64)>,
    pub(super) result: Option<Tex>,
    /// horizon::hle::tex_table_epoch() before this resolution's lookups.
    pub(super) table_epoch: u64,
    /// Presented frame the texture's CPU-write check last ran for through this memo (memos persist across submits).
    pub(super) frame: u64,
    /// translated::root_id of `result` (render-target feedback compares it with the pass's attachments).
    pub(super) root: Option<u64>,
}

pub(crate) struct PendingClear {
    pub(super) tex: Tex,
    /// translated::root_id(&tex), computed once (each new clear is compared with every pending one).
    pub(super) root: u64,
    pub(super) level: usize,
    /// Array slice, or depth plane of a 3D texture.
    pub(super) layer: usize,
    pub(super) kind: ClearKind,
}

/// The clear depth Metal is given for `depth` on a target of format `pf`. On Apple GPUs a Depth16Unorm clear truncates
/// (0.5 stores 32767) while rasterized depth rounds (0.5 stores 32768), so a draw at the cleared depth failed an EQUAL test
/// against it. TotK clears two D16 targets to 0.5 each frame and then selects regions with EQUAL tests; with the clear
/// truncated, pixels left at the cleared depth fail them (blocks in Ultrahand / Fuse effects, totk-mobile PRs 5 and 9). Given the f32 the game passes,
/// rounded half up as the rasterizer does (a standalone Metal test on the M4: all 201 values 0..1 match a draw at that
/// depth, against 4 of 5 failing before), then nudged into the step so the truncation keeps it.
pub(crate) fn clear_depth_value(pf: MTLPixelFormat, depth: f32) -> f64 {
    if pf != MTLPixelFormat::Depth16Unorm {
        return depth as f64;
    }
    let k = (depth.clamp(0.0, 1.0) as f64 * 65535.0).round();
    ((k + 0.25) / 65535.0).min(1.0)
}

#[derive(Clone, Copy)]
pub(crate) enum ClearKind {
    Color([f32; 4]),
    /// None: that aspect is kept.
    DepthStencil {
        depth: Option<f32>,
        stencil: Option<u32>,
    },
}

impl PendingClear {
    pub(super) fn target_eq(&self, o: &PendingClear) -> bool {
        Retained::as_ptr(&self.tex) == Retained::as_ptr(&o.tex)
            && self.level == o.level
            && self.layer == o.layer
    }
    pub(super) fn is_target(&self, t: &Tex, level: usize, layer: usize) -> bool {
        Retained::as_ptr(&self.tex) == Retained::as_ptr(t)
            && self.level == level
            && self.layer == layer
    }
}

/// Uniform buffer copies by (guest address, length), with an address-ordered index so the executor's inline uniform
/// updates (~thousands per frame) drop the overlapping copies without scanning them all.
#[derive(Default)]
/// Keys are (guest address, copied length | zero-padding bytes << 32): a padded copy (translated::cbuf_zero_pad) is
/// valid only for bindings with the same bound size. Invalidation uses the low half (the guest bytes copied).
pub(crate) struct UboCopies {
    map: HashMap<(u64, usize), (Retained<ProtocolObject<dyn MTLBuffer>>, usize)>,
    by_addr: std::collections::BTreeSet<(u64, usize)>,
    max_len: usize,
}

impl UboCopies {
    pub(super) fn get(
        &self,
        key: &(u64, usize),
    ) -> Option<&(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        self.map.get(key)
    }

    /// `indexed`: whether the executor may write the copied range in this submit (see metal::ubo_updated); copies it
    /// cannot write are never invalidated, so they skip the index.
    pub(super) fn insert(
        &mut self,
        key: (u64, usize),
        v: (Retained<ProtocolObject<dyn MTLBuffer>>, usize),
        indexed: bool,
    ) {
        if indexed {
            self.max_len = self.max_len.max(key.1 & 0xFFFF_FFFF);
            self.by_addr.insert(key);
            UBO_INDEXED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.map.insert(key, v);
    }

    pub(super) fn clear(&mut self) {
        if !self.map.is_empty() {
            self.map.clear();
            self.by_addr.clear();
        }
        self.max_len = 0;
    }

    /// Drops the copies overlapping guest bytes [lo, hi).
    pub(super) fn invalidate(&mut self, lo: u64, hi: u64) {
        let from = lo.saturating_sub(self.max_len as u64);
        let hit: Vec<(u64, usize)> = self
            .by_addr
            .range((from, 0)..(hi, 0))
            .filter(|&&(a, l)| a + (l & 0xFFFF_FFFF) as u64 > lo)
            .copied()
            .collect();
        for k in hit {
            self.by_addr.remove(&k);
            self.map.remove(&k);
        }
    }
}

/// Everything that determines a texture's Metal object and its guest memory layout.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TexDesc {
    pub(crate) storage: usize,
    pub(crate) target: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) depth: i32,
    pub(crate) levels: i32,
    pub(crate) format: i32,
    pub(crate) flags: i32,
    pub(crate) samples: i32,
    pub(crate) stride: i64,
}

impl TexDesc {
    pub(crate) fn of(t: &NvnTexture) -> TexDesc {
        TexDesc {
            storage: t.storage,
            target: t.target,
            width: t.width,
            height: t.height,
            depth: t.depth,
            levels: t.levels,
            format: t.format,
            flags: t.flags,
            samples: t.samples,
            stride: t.stride,
        }
    }
}
