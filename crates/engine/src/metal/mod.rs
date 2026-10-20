// Metal executor for recorded NVN command lists.
//
// `submit` runs on the guest thread that calls nvnQueueSubmitCommands: it walks the recorded
// (typed) commands and encodes them into one MTLCommandBuffer. `present` blits the presented
// swapchain texture to the window's CAMetalLayer drawable. Metal objects are created lazily from
// the NVNtexture descriptors the game gave us; GPU addresses are host pointers (see horizon/hle.rs).
//
// Phase A (this file): render targets, clears, swapchain presentation. Draw calls are decoded and
// counted but not yet executed -- they need shader translation (BNSH / NVN GPU code -> MSL).

/// Executor maps are keyed by addresses and small fixed keys and looked up per draw: Fx hashing instead of SipHash
/// (which was ~11% of the presentation thread).
use rustc_hash::FxHashMap as HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBlendFactor, MTLBlendOperation, MTLBuffer, MTLClearColor, MTLCommandBuffer,
    MTLCommandBufferDescriptor, MTLCommandBufferErrorOption, MTLCommandBufferStatus,
    MTLCommandEncoder, MTLCommandQueue, MTLComputePipelineState, MTLDepthStencilState, MTLDevice,
    MTLIndexType, MTLLibrary, MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType,
    MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLRenderPipelineDescriptor,
    MTLRenderPipelineState, MTLResourceOptions, MTLSamplerState, MTLScissorRect, MTLSize,
    MTLStorageMode, MTLStoreAction, MTLTexture, MTLTextureDescriptor, MTLTextureType,
    MTLTextureUsage, MTLViewport,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};

pub(crate) mod backend;
mod batch;
mod copy;
pub mod csharp_sass2spv;
mod fence_guard;
mod issuer;
pub mod msl_specialize;
mod pass_prof;
mod programs;
mod report_guard;
pub mod shader_cache;
pub mod spirv_cross;
mod tex_backing;
mod translated;

use crate::nvn::api::NvnTexture;
use crate::nvn::cmd::{Cmd, RtRef};

type Device = Retained<ProtocolObject<dyn MTLDevice>>;
type Queue = Retained<ProtocolObject<dyn MTLCommandQueue>>;
type Tex = Retained<ProtocolObject<dyn MTLTexture>>;

struct Gfx {
    device: Device,
    queue: Queue,
    /// The window's layer: set by init, or later by attach_layer (iOS creates its window after the guest starts).
    layer: OnceLock<Retained<CAMetalLayer>>,
    blit: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Ahead-of-time translated game shaders (see metal/translated.rs); None unless --translated-shaders.
    shader_lib: Option<translated::ShaderLibs>,
    state: Mutex<State>,
}
// SAFETY: Metal devices, queues, textures and CAMetalLayer drawable APIs are thread-safe; all
// mutable bookkeeping lives behind `state`.
unsafe impl Send for Gfx {}
unsafe impl Sync for Gfx {}

#[derive(Clone)]
pub(super) struct CachedPassRts {
    pub(super) color_texs: smallvec::SmallVec<[(usize, Tex, RtRef); 8]>,
    pub(super) depth_tex: Option<(Tex, RtRef)>,
    pub(super) color_pfs: [MTLPixelFormat; 8],
    pub(super) depth_pf: MTLPixelFormat,
    pub(super) enc_key: smallvec::SmallVec<[u64; 36]>,
    pub(super) target_size: (f64, f64),
}

pub(crate) struct State {
    colors: Vec<RtRef>,
    depth: Option<RtRef>,
    program: Option<usize>,
    /// Bound compute program (NVN binds graphics and compute programs independently).
    compute_program: Option<usize>,
    compute_pipelines: HashMap<String, Retained<ProtocolObject<dyn MTLComputePipelineState>>>,
    viewport: Option<(i32, i32, i32, i32)>,
    scissor: Option<(i32, i32, i32, i32)>,
    vertex_buffers: HashMap<i32, (u64, usize)>,
    uniform_buffers: BindTable<(u64, usize)>,
    /// Bound storage buffers by [stage][index] (size 0 = unbound); read for every draw.
    storage_buffers: [[(u64, usize); 16]; 6],
    vertex_attribs: crate::nvn::cmd::AttribState,
    vertex_streams: Vec<u64>,
    textures_bound: BindTable<u64>,
    /// Render targets to copy back into guest memory, with the frame they were bound in: (bound this frame, bound last
    /// frame); see `writeback_pass`.
    writeback: (HashMap<usize, u64>, HashMap<usize, u64>),
    /// Storage -> last frame a render target over it was bound (picks the rarely rendered ones for write-back).
    rt_history: HashMap<usize, u64>,
    /// nvnCommandBufferBindImage: (stage, image unit) -> image handle (pool id).
    images_bound: BindTable<u64>,
    /// Queue render enable (nvnCommandBufferSetRenderEnable): when false, draws and clears are discarded.
    render_enabled: bool,
    /// SAMPLES_PASSED occlusion queries (nvnCommandBufferResetCounter / ReportCounter counter 1).
    occl: Occlusion,
    /// TIMESTAMP reports recorded into the command buffer being built: (address, before any GPU work in it); see
    /// gpu_timestamps.
    ts_pending: Vec<(u64, bool)>,
    /// nvnCommandBufferSetPointSize (used when the vertex shader does not write gl_PointSize).
    pub(super) point_size: f32,
    /// NVNblendState per color target (bits 0..3 of the state select the target; 0 = not bound since reset).
    blend_states: [u64; 8],
    color_state: u32,
    channel_mask: u32,
    stats: Stats,
    // ---- translated draw path (metal/translated.rs) ----
    depth_stencil: u64,
    polygon: u32,
    depth_range: (f32, f32),
    /// Stencil reference, read (value) mask and write mask, [front, back] (defaults: ref 0, masks 0xff).
    stencil: [[u8; 2]; 3],
    /// Depth bounds test (enable, min, max): fragments whose stored depth is outside the range are discarded.
    depth_bounds: Option<(f32, f32)>,
    depth_clamp: bool,
    /// Primitive restart index, when enabled (nvnCommandBufferSetPrimitiveRestart).
    pub(super) prim_restart: Option<u32>,
    /// Bound NVNmultisampleState bits (alpha-to-coverage is bit 1).
    pub(super) multisample: u64,
    /// nvnCommandBufferSetPolygonOffsetClamp: (factor, units, clamp).
    pub(super) polygon_offset: (f32, f32, f32),
    blend_color: [f32; 4],
    /// Color targets drawn into since the last present, in first-use order, with draw counts (frame dumps).
    frame_rts: Vec<(u64, Tex, u32)>,
    /// Debug (--snapshot-enc): command buffer being recorded, the open encoder's label and color targets, and
    /// copies of targets taken when a listed encoder ended (written out with the frame dump).
    cur_cb: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    /// When encoding into `cur_cb` began (time-based commits, see maybe_split).
    cb_started: Option<std::time::Instant>,
    enc_name: String,
    enc_targets: Vec<(usize, Tex)>,
    snapshots: Vec<(String, Tex)>,
    /// Depth attachment of the current encoder (texture, slice, level), for --snapshot-depth.
    pub(super) enc_depth: Option<(Tex, usize, usize)>,
    /// Depth snapshots: (label, buffer, width, height, bytes per pixel).
    depth_snapshots: Vec<(
        String,
        Retained<ProtocolObject<dyn MTLBuffer>>,
        usize,
        usize,
        usize,
    )>,
    /// Uncompressed stand-ins for block-compressed render targets the game writes through a same-size integer view
    /// (GPU texture compression), keyed by (target texture, level, view format); see `bc_proxy`.
    bc_proxies: HashMap<(usize, usize, u64), Tex>,
    /// Render targets bound through a texture view of another format (e.g. RGBA8 drawn through an RGBA8_SRGB view).
    rt_format_views: HashMap<(usize, u64), Tex>,
    /// Proxies rendered in the current encoder, copied into their compressed targets when it ends: (proxy, target, level).
    bc_pending: Vec<(Tex, Tex, usize)>,
    /// Clears not yet encoded (see `PendingClear`).
    pub(super) pending_clears: Vec<PendingClear>,
    /// Guest memory written by GPU work (buffer copies / clears, compute storage buffers) in the current submit. A
    /// uniform buffer in such a range is bound zero-copy so it sees those writes in GPU order (the encode-time copy
    /// would capture the contents from before them, e.g. a bone palette the GPU fills, leaving characters in T-pose).
    /// Merged, non-overlapping [start, end) ranges keyed by start (gpu_written: one lookup per uniform binding).
    gpu_dirty: std::collections::BTreeMap<u64, u64>,
    /// --early-fence: gpu_dirty ranges of earlier submits, with the last serial their work can be in. The game may
    /// encode its next frame before that work completed; uniform buffers in these ranges stay zero-copy until it has.
    carried_dirty: std::collections::VecDeque<(u64, Vec<(u64, u64)>)>,
    /// Render encoder kept open across consecutive draws to the same targets (see metal/translated.rs), with its target key.
    /// The open translated render encoder: its calls are queued to `issuer` (see metal/issuer.rs).
    enc: Option<issuer::Issuer>,
    issuer: issuer::Issuer,
    enc_key: smallvec::SmallVec<[u64; 36]>,
    pub(super) rts_dirty: bool,
    pub(super) cached_rts: Option<std::sync::Arc<CachedPassRts>>,
    /// Draws issued in the current shared encoder (matches Metal validation's per-encoder draw numbers).
    enc_draws: u32,
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
    draw_packet: Option<Box<translated::DrawPacket>>,
    replay: ReplayStats,
    /// Ranges UpdateUniformBuffer commands of the current submit write (uniform buffers there are copied per draw).
    ubo_updated: Vec<(u64, u64)>,
    /// Scratch for cur_cb_and_ranges.
    lists_stack: Vec<u64>,
    /// Submit-scoped cache of command lists reachable in the current submit (avoids COMMAND_LISTS mutex locks).
    submit_lists: HashMap<u64, std::sync::Arc<crate::nvn::api::RecordedCommandList>>,
    /// --log-draw-reuse: the list (Arc pointer) and op index being executed, whether the list is from an earlier frame.
    cur_list_op: Option<(usize, usize, bool)>,
    /// Frame traces: the game caller (main+ offset) of the draw being executed (--log-draw-callers).
    pub(super) cur_caller: u32,
    /// --log-draw-reuse: plan hash of each (list, op) draw's previous run.
    draw_reuse: rustc_hash::FxHashMap<(usize, usize, u64), [u64; 12]>,
    /// --log-draw-reuse: hash of the call path (parent list and op index at each level).
    call_ctx: u64,
    /// Commit at the next opportunity (a long render pass was cut, see split_draws).
    force_split: bool,
    /// Recorded NVN commands the executor has no handler for, counted per frame (--cmd-stats-at=N prints frame N).
    ignored_cmds: HashMap<&'static str, u64>,
    /// Render-target feedback (see translated::feedback_snapshots): bumped by every nvnCommandBufferBarrier that orders
    /// fragments and invalidates the texture cache. A draw that samples an attachment of its own pass reads a copy of it
    /// taken at the first such draw after the barrier: on the Switch it sees the writes from before the barrier, while
    /// sampling an attachment inside a Metal render pass reads whatever happens to be in memory.
    fb_epoch: u64,
    /// fb_epoch when the current render encoder began (in-place render-target feedback, translated::feedback_snapshots).
    pub(super) enc_fb_epoch: u64,
    /// The command being executed comes from a called list recorded in an earlier frame (--log-phases).
    pub(super) in_old_list: bool,
    /// Texture pointer -> root id, for the current frame (translated::feedback_snapshots; holds the textures).
    root_memo: HashMap<usize, (Tex, u64)>,
    /// Submitted command buffers the GPU may still be running, oldest first (at most `frames_in_flight()`).
    inflight: std::collections::VecDeque<InFlight>,
    /// Set after the first GPU error: all further rendering is skipped (see `submit`).
    gpu_halted: bool,
    /// Consecutive command buffers discarded as innocent victims of a GPU recovery caused by something else.
    victims_in_a_row: u32,
    /// Last window-present command buffer, checked for errors on the next present (it is never waited on).
    present_cb: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
    warned_prims: Vec<i32>,
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
pub(super) struct TexCache {
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
    /// alias_view's views by (root, format, type, levels, slices).
    pub(super) alias_views:
        HashMap<(usize, usize, usize, usize, usize, usize, usize), (Tex, Tex, u64)>,
    /// alias_views' key by view object (the views are held there, so these pointers stay valid while listed).
    pub(super) alias_view_keys: HashMap<usize, (usize, usize, usize, usize, usize, usize, usize)>,
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
            alias_of: HashMap::default(),
            tex_by_desc: HashMap::default(),
            desc_used: HashMap::default(),
            tex_checked: HashMap::default(),
            tex_sparse: HashMap::default(),
            view_cache: HashMap::default(),
            tex_check_budget_used: 0,
            gpu_written: Default::default(),
        }
    }
}

/// See State::cx.
pub(super) struct EncodeCtx {
    pub(super) arena: translated::Arena,
    /// Ryujinx support buffers by viewport size (its only input). Arena chunks are never written again after reset,
    /// so a cached allocation stays valid.
    pub(super) support_cache: HashMap<(u32, u32), (Retained<ProtocolObject<dyn MTLBuffer>>, usize)>,
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
    /// fresh one per draw cost a 4 KB fill and broke batched runs, metal/batch.rs). Same lifetime as support_cache.
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
        }
    }
}

/// See State::sh.
pub(super) struct SharedCaches {
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
    pub(super) samplers: HashMap<
        i32,
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
            default_sampler: None,
            ds_states: HashMap::default(),
            translated_pipelines: HashMap::default(),
            translated_pipelines_bad: Default::default(),
            translated_pipelines_failed: 0,
        }
    }
}

/// See State::lb.
pub(super) struct EncLabels {
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
pub(super) struct BindTable<V: Copy> {
    slots: Box<[Option<V>; BIND_STAGES * BIND_SLOTS]>,
    extra: HashMap<(i32, i32), V>,
}

const BIND_STAGES: usize = 8;
const BIND_SLOTS: usize = 96;

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
            rt_history: HashMap::default(),
            render_enabled: true,
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
struct Stats {
    submits: u64,
    clears: u64,
    copies: u64,
    draws: u64,
    passes: u64,
}

static GFX: OnceLock<Gfx> = OnceLock::new();

use crate::settings::GfxMode;

fn gfx_mode() -> GfxMode {
    crate::settings::get().graphics.gfx_mode
}

const BLIT_MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct VOut { float4 pos [[position]]; float2 uv; };
vertex VOut blit_vs(uint vid [[vertex_id]]) {
    float2 p = float2((vid << 1) & 2, vid & 2);
    VOut o; o.pos = float4(p * 2.0 - 1.0, 0.0, 1.0); o.uv = float2(p.x, 1.0 - p.y); return o;
}
// crop_uv: [u_min, v_start, u_scale, v_delta]
fragment float4 blit_fs(VOut in [[stage_in]], texture2d<float> t [[texture(0)]], constant float4& crop_uv [[buffer(0)]]) {
    constexpr sampler s(filter::linear, address::clamp_to_edge);
    return t.sample(s, crop_uv.xy + in.uv * crop_uv.zw);
}
"#;

fn nvn_blend_op(val: u32) -> MTLBlendOperation {
    match val {
        2 => MTLBlendOperation::Subtract,
        3 => MTLBlendOperation::ReverseSubtract,
        4 => MTLBlendOperation::Min,
        5 => MTLBlendOperation::Max,
        _ => MTLBlendOperation::Add,
    }
}

/// `code` is a blend-state factor field (see nvn::state::blend_func_encode).
fn nvn_blend_factor(code: u32) -> MTLBlendFactor {
    let val = crate::nvn::state::blend_func_decode(code as u64);
    match val {
        1 => MTLBlendFactor::Zero,
        2 => MTLBlendFactor::One,
        3 => MTLBlendFactor::SourceColor,
        4 => MTLBlendFactor::OneMinusSourceColor,
        5 => MTLBlendFactor::SourceAlpha,
        6 => MTLBlendFactor::OneMinusSourceAlpha,
        7 => MTLBlendFactor::DestinationAlpha,
        8 => MTLBlendFactor::OneMinusDestinationAlpha,
        9 => MTLBlendFactor::DestinationColor,
        10 => MTLBlendFactor::OneMinusDestinationColor,
        11 => MTLBlendFactor::SourceAlphaSaturated,
        // NVNblendFunc dual-source and constant-color values
        16 => MTLBlendFactor::Source1Color,
        17 => MTLBlendFactor::OneMinusSource1Color,
        18 => MTLBlendFactor::Source1Alpha,
        19 => MTLBlendFactor::OneMinusSource1Alpha,
        97 => MTLBlendFactor::BlendColor,
        98 => MTLBlendFactor::OneMinusBlendColor,
        99 => MTLBlendFactor::BlendAlpha,
        100 => MTLBlendFactor::OneMinusBlendAlpha,
        _ => {
            static WARNED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                println!("[!] Gfx: unknown NVN blend factor {} (using ONE)", val);
            }
            MTLBlendFactor::One
        }
    }
}

/// Called once by host/display.rs after the window and layer exist.
pub fn init(device: Device, layer: Option<Retained<CAMetalLayer>>) {
    let queue = device.newCommandQueue().expect("newCommandQueue");
    let lib = device
        .newLibraryWithSource_options_error(&NSString::from_str(BLIT_MSL), None)
        .expect("compile blit shader");
    let vs = lib
        .newFunctionWithName(&NSString::from_str("blit_vs"))
        .expect("blit_vs");
    let fs = lib
        .newFunctionWithName(&NSString::from_str("blit_fs"))
        .expect("blit_fs");
    let desc = MTLRenderPipelineDescriptor::new();
    desc.setVertexFunction(Some(&vs));
    desc.setFragmentFunction(Some(&fs));
    unsafe {
        desc.colorAttachments()
            .objectAtIndexedSubscript(0)
            .setPixelFormat(MTLPixelFormat::BGRA8Unorm)
    };
    let blit = device
        .newRenderPipelineStateWithDescriptor_error(&desc)
        .expect("blit pipeline");

    let shader_lib = translated::load_shader_lib(&device);
    let gfx = Gfx {
        device,
        queue,
        layer: OnceLock::new(),
        blit,
        shader_lib,
        state: Mutex::new(State::default()),
    };
    if let Some(l) = layer {
        let _ = gfx.layer.set(l);
    }
    if GFX.set(gfx).is_err() {
        eprintln!("[!] metal::init called twice");
    }
    println!("[+] Gfx: Metal executor ready");
}

/// Gives an initialised executor its window layer. iOS initialises gfx before the guest starts (its submits must be
/// executed from the first one: GPU-written values the game reads back were missing when the window came up late) and
/// creates the window afterwards.
#[cfg(target_os = "ios")]
pub fn attach_layer(layer: Retained<CAMetalLayer>) {
    if let Some(g) = GFX.get() {
        if g.layer.set(layer).is_err() {
            eprintln!("[!] metal::attach_layer: a layer is already attached");
        }
    }
}

/// The executor's Metal device, once initialised (or system default if called beforehand).
pub fn device() -> Option<Device> {
    if let Some(g) = GFX.get() {
        return Some(g.device.clone());
    }
    objc2_metal::MTLCreateSystemDefaultDevice()
}

/// NVNformat -> Metal pixel format for render targets / sampled textures.
fn metal_format(fmt: i32) -> (MTLPixelFormat, Option<MTLPixelFormat>) {
    // (format, optional non-sRGB twin used to pass raw bytes through when presenting)
    match fmt {
        1 => (MTLPixelFormat::R8Unorm, None),
        2 => (MTLPixelFormat::R8Snorm, None),
        3 => (MTLPixelFormat::R8Uint, None),
        // Integer and signed-normalized formats (NVN format order). Unlisted formats fall back to RGBA8Unorm, which silently
        // reinterprets the data, so every format with a Metal equivalent must be here.
        4 => (MTLPixelFormat::R8Sint, None),
        7 => (MTLPixelFormat::R16Snorm, None),
        8 => (MTLPixelFormat::R16Uint, None),
        9 => (MTLPixelFormat::R16Sint, None),
        11 => (MTLPixelFormat::R32Uint, None),
        12 => (MTLPixelFormat::R32Sint, None),
        14 => (MTLPixelFormat::RG8Snorm, None),
        15 => (MTLPixelFormat::RG8Uint, None),
        16 => (MTLPixelFormat::RG8Sint, None),
        19 => (MTLPixelFormat::RG16Snorm, None),
        20 => (MTLPixelFormat::RG16Uint, None),
        21 => (MTLPixelFormat::RG16Sint, None),
        23 => (MTLPixelFormat::RG32Uint, None),
        24 => (MTLPixelFormat::RG32Sint, None),
        39 => (MTLPixelFormat::RGBA8Uint, None),
        40 => (MTLPixelFormat::RGBA8Sint, None),
        43 => (MTLPixelFormat::RGBA16Snorm, None),
        44 => (MTLPixelFormat::RGBA16Uint, None),
        45 => (MTLPixelFormat::RGBA16Sint, None),
        47 => (MTLPixelFormat::RGBA32Uint, None),
        48 => (MTLPixelFormat::RGBA32Sint, None),
        62 => (MTLPixelFormat::RGB10A2Uint, None),
        64 => (MTLPixelFormat::RGB9E5Float, None),
        5 => (MTLPixelFormat::R16Float, None),
        6 => (MTLPixelFormat::R16Unorm, None),
        10 => (MTLPixelFormat::R32Float, None),
        13 => (MTLPixelFormat::RG8Unorm, None),
        17 => (MTLPixelFormat::RG16Float, None),
        18 => (MTLPixelFormat::RG16Unorm, None),
        22 => (MTLPixelFormat::RG32Float, None),
        37 | 109 => (MTLPixelFormat::RGBA8Unorm, None),
        55 => (
            MTLPixelFormat::RGBA8Unorm_sRGB,
            Some(MTLPixelFormat::RGBA8Unorm),
        ), // RGBX8_SRGB
        38 => (MTLPixelFormat::RGBA8Snorm, None),
        56 => (
            MTLPixelFormat::RGBA8Unorm_sRGB,
            Some(MTLPixelFormat::RGBA8Unorm),
        ),
        41 => (MTLPixelFormat::RGBA16Float, None),
        42 => (MTLPixelFormat::RGBA16Unorm, None),
        46 => (MTLPixelFormat::RGBA32Float, None),
        49 => (MTLPixelFormat::Stencil8, None),
        50 => (MTLPixelFormat::Depth16Unorm, None),
        51 | 52 => (MTLPixelFormat::Depth32Float, None),
        53 | 54 => (MTLPixelFormat::Depth32Float_Stencil8, None), // Apple GPUs have no D24S8
        61 => (MTLPixelFormat::RGB10A2Unorm, None),
        63 => (MTLPixelFormat::RG11B10Float, None),
        65 | 66 => (MTLPixelFormat::BC1_RGBA, None),
        67 => (MTLPixelFormat::BC2_RGBA, None),
        68 => (MTLPixelFormat::BC3_RGBA, None),
        69 | 70 => (MTLPixelFormat::BC1_RGBA_sRGB, None),
        71 => (MTLPixelFormat::BC2_RGBA_sRGB, None),
        72 => (MTLPixelFormat::BC3_RGBA_sRGB, None),
        73 => (MTLPixelFormat::BC4_RUnorm, None),
        74 => (MTLPixelFormat::BC4_RSnorm, None),
        75 => (MTLPixelFormat::BC5_RGUnorm, None),
        76 => (MTLPixelFormat::BC5_RGSnorm, None),
        77 => (MTLPixelFormat::BC7_RGBAUnorm, None),
        78 => (MTLPixelFormat::BC7_RGBAUnorm_sRGB, None),
        79 => (MTLPixelFormat::BC6H_RGBFloat, None),
        80 => (MTLPixelFormat::BC6H_RGBUfloat, None),
        121 => (MTLPixelFormat::ASTC_4x4_LDR, None),
        122 => (MTLPixelFormat::ASTC_5x4_LDR, None),
        123 => (MTLPixelFormat::ASTC_5x5_LDR, None),
        124 => (MTLPixelFormat::ASTC_6x5_LDR, None),
        125 => (MTLPixelFormat::ASTC_6x6_LDR, None),
        126 => (MTLPixelFormat::ASTC_8x5_LDR, None),
        127 => (MTLPixelFormat::ASTC_8x6_LDR, None),
        128 => (MTLPixelFormat::ASTC_8x8_LDR, None),
        129 => (MTLPixelFormat::ASTC_10x5_LDR, None),
        130 => (MTLPixelFormat::ASTC_10x6_LDR, None),
        131 => (MTLPixelFormat::ASTC_10x8_LDR, None),
        132 => (MTLPixelFormat::ASTC_10x10_LDR, None),
        133 => (MTLPixelFormat::ASTC_12x10_LDR, None),
        134 => (MTLPixelFormat::ASTC_12x12_LDR, None),
        135 => (MTLPixelFormat::ASTC_4x4_sRGB, None),
        136 => (MTLPixelFormat::ASTC_5x4_sRGB, None),
        137 => (MTLPixelFormat::ASTC_5x5_sRGB, None),
        138 => (MTLPixelFormat::ASTC_6x5_sRGB, None),
        139 => (MTLPixelFormat::ASTC_6x6_sRGB, None),
        140 => (MTLPixelFormat::ASTC_8x5_sRGB, None),
        141 => (MTLPixelFormat::ASTC_8x6_sRGB, None),
        142 => (MTLPixelFormat::ASTC_8x8_sRGB, None),
        143 => (MTLPixelFormat::ASTC_10x5_sRGB, None),
        144 => (MTLPixelFormat::ASTC_10x6_sRGB, None),
        145 => (MTLPixelFormat::ASTC_10x8_sRGB, None),
        146 => (MTLPixelFormat::ASTC_10x10_sRGB, None),
        147 => (MTLPixelFormat::ASTC_12x10_sRGB, None),
        148 => (MTLPixelFormat::ASTC_12x12_sRGB, None),
        153 | 154 => (MTLPixelFormat::BGRA8Unorm, None),
        155 | 156 => (
            MTLPixelFormat::BGRA8Unorm_sRGB,
            Some(MTLPixelFormat::BGRA8Unorm),
        ),
        _ => (MTLPixelFormat::RGBA8Unorm, None),
    }
}

/// Formats Metal can use as a color attachment: not depth/stencil, not block-compressed.
fn is_color_renderable(tex: &ProtocolObject<dyn MTLTexture>) -> bool {
    let pf = tex.pixelFormat();
    let depth_stencil = matches!(
        pf,
        MTLPixelFormat::Depth16Unorm
            | MTLPixelFormat::Depth32Float
            | MTLPixelFormat::Depth32Float_Stencil8
            | MTLPixelFormat::Stencil8
    );
    tex.usage().contains(MTLTextureUsage::RenderTarget) && !depth_stencil
}

/// Snapshot of the NVNtexture at `addr` (see horizon/hle.rs `texture_snapshot`); falls back to the live struct for objects the
/// HLE never saw initialized.
unsafe fn tex_info(addr: usize) -> Option<(u64, NvnTexture)> {
    if addr == 0 {
        return None;
    }
    crate::nvn::api::texture_snapshot(addr)
        .or_else(|| Some((0, std::ptr::read(addr as *const NvnTexture))))
}

/// Time spent resolving texture objects past the memo / generation fast paths (alias search, by-description lookup,
/// creation), for the timeline: [count, ns]. Nested resolutions (an alias root) count separately.
/// Texture-cache epoch events per frame: [alias root re-initialized (no bump), objects re-pointed (bump)].
static EPOCH_BUMPS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
static SLOW_RES: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
struct SlowTimer(std::time::Instant);
impl SlowTimer {
    fn start() -> Self {
        SlowTimer(std::time::Instant::now())
    }
}
impl Drop for SlowTimer {
    fn drop(&mut self) {
        SLOW_RES[0].fetch_add(1, Ordering::Relaxed);
        SLOW_RES[1].fetch_add(self.0.elapsed().as_nanos() as u64, Ordering::Relaxed);
    }
}

/// A texture's guest layout, cached by shape (it allocates, and texture resolution needs it for a texture and every
/// alias candidate each time an object is resolved: TotK re-initializes ~1,400 bound objects per frame).
pub(crate) fn layout_of(t: &NvnTexture) -> std::sync::Arc<crate::nvn::layout::TextureLayout> {
    type Key = (i32, i32, i32, i32, i32, i32, i32, i32, i64);
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<Key, std::sync::Arc<crate::nvn::layout::TextureLayout>>> = Default::default();
    }
    let p = texture_params(t);
    let key: Key = (
        p.target, p.width, p.height, p.depth, p.levels, p.format, p.flags, p.samples, p.stride,
    );
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(l) = c.get(&key) {
            return l.clone();
        }
        if c.len() > 1 << 14 {
            c.clear();
        }
        let l = std::sync::Arc::new(crate::nvn::layout::compute_layout(&p));
        c.insert(key, l.clone());
        l
    })
}

pub(crate) fn texture_params(t: &NvnTexture) -> crate::nvn::layout::TextureParams {
    crate::nvn::layout::TextureParams {
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

fn is_view_compatible(a: MTLPixelFormat, b: MTLPixelFormat) -> bool {
    match (a, b) {
        (MTLPixelFormat::RGBA8Unorm, MTLPixelFormat::RGBA8Unorm_sRGB)
        | (MTLPixelFormat::RGBA8Unorm_sRGB, MTLPixelFormat::RGBA8Unorm)
        | (MTLPixelFormat::BGRA8Unorm, MTLPixelFormat::BGRA8Unorm_sRGB)
        | (MTLPixelFormat::BGRA8Unorm_sRGB, MTLPixelFormat::BGRA8Unorm)
        | (MTLPixelFormat::BC1_RGBA, MTLPixelFormat::BC1_RGBA_sRGB)
        | (MTLPixelFormat::BC1_RGBA_sRGB, MTLPixelFormat::BC1_RGBA)
        | (MTLPixelFormat::BC3_RGBA, MTLPixelFormat::BC3_RGBA_sRGB)
        | (MTLPixelFormat::BC3_RGBA_sRGB, MTLPixelFormat::BC3_RGBA)
        | (MTLPixelFormat::BC7_RGBAUnorm, MTLPixelFormat::BC7_RGBAUnorm_sRGB)
        | (MTLPixelFormat::BC7_RGBAUnorm_sRGB, MTLPixelFormat::BC7_RGBAUnorm) => true,
        _ => a == b,
    }
}

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
    fn maybe_split(&self, st: &mut State) -> bool {
        if st.enc.is_some() && !st.force_split {
            return false;
        }
        self.maybe_split_slow(st)
    }

    #[inline(never)]
    fn maybe_split_slow(&self, st: &mut State) -> bool {
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
                            d.setClearDepth(depth.unwrap_or(0.0) as f64);
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
    fn defer_clear(&self, st: &mut State, c: PendingClear) {
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
        if let Some(e) = st.enc.take() {
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
    fn resolve_bc_proxies(&self, st: &mut State) {
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
            st.tc.gpu_written.insert(storage_id(&target));
        }
        blit.endEncoding();
    }

    /// --snapshot-enc=f900e23,f900e99: copy the color targets of those encoders as they stand when the encoder
    /// ends, so one frame's intermediate states can be inspected (dumped with --dump-frame of that frame).
    fn snapshot_encoder_targets(&self, st: &mut State) {
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

    /// Metal texture backing an NVNtexture, created on first use and re-uploaded from guest storage when the
    /// guest wrote texels into it since (nvnTextureWriteTexels / FlushTexels).
    unsafe fn texture(&self, tc: &mut TexCache, nvn_tex: usize) -> Option<Tex> {
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
        if let Some(v) = self.alias_view(tc, nvn_tex, &snap) {
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
        let desc = MTLTextureDescriptor::new();
        desc.setTextureType(tt);
        desc.setPixelFormat(pf);
        desc.setWidth(t.width as usize);
        desc.setHeight(
            if matches!(tt, MTLTextureType::Type1D | MTLTextureType::Type1DArray) {
                1
            } else {
                t.height as usize
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
        let is_compressed = crate::nvn::layout::format_info(t.format)
            .map(|i| i.block_w > 1)
            .unwrap_or(false);
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
            tc.gpu_written.insert(storage_id(&tex));
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
    unsafe fn alias_view(
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
    unsafe fn upload_texture(&self, tex: &Tex, t: &NvnTexture) {
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

    unsafe fn run_list(
        &self,
        st: &mut State,
        _cb: &ProtocolObject<dyn MTLCommandBuffer>,
        handle: u64,
        depth: u32,
    ) {
        if depth > 8 {
            return;
        }
        let Some(list) = st
            .submit_lists
            .get(&handle)
            .cloned()
            .or_else(|| crate::nvn::api::command_list(handle, depth > 0))
        else {
            // Its state changes and draws would silently vanish: always worth knowing.
            static MISSING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = MISSING.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n <= 40 || n.is_power_of_two() {
                println!("[!] Gfx: command list handle {:#x} is not recorded ({}, class at eviction: {}, {}); its commands are lost ({} so far)", handle,
                    if crate::nvn::api::command_handle_issued(handle) { "evicted" } else { "never issued by EndRecording" },
                    crate::nvn::api::evicted_list_class(handle), if depth > 0 { "called" } else { "submitted" }, n);
            }
            return;
        };
        let replay_log = replay_log_enabled();
        let tracing = tracing_frame();
        let list_old = list.recorded_frame < presented_frames();
        let Some(mut cur) = st.cur_cb.clone() else {
            return;
        };
        // --log-cmd-time=N: executor time per NVN command kind over frames N..N+60 (CallCommands includes its list).
        let mut cmd_t: Option<(&'static str, std::time::Instant)> = None;
        let cmd_time = cmd_time_on();
        if cmd_time && depth > 0 {
            let draws = list
                .ops
                .iter()
                .filter(|o| {
                    matches!(
                        o.typed,
                        Some(
                            Cmd::DrawArrays { .. }
                                | Cmd::DrawArraysInstanced { .. }
                                | Cmd::DrawElements { .. }
                                | Cmd::DrawElementsInstanced { .. }
                        )
                    )
                })
                .count();
            let calls = list
                .ops
                .iter()
                .filter(|o| matches!(o.typed, Some(Cmd::CallCommands(_))))
                .count();
            let age = presented_frames().saturating_sub(list.recorded_frame);
            let kind = match (age, draws > 0, calls > 0) {
                (0, false, false) => "called list: new, state only",
                (0, true, _) => "called list: new, draws",
                (0, false, true) => "called list: new, calls",
                (_, false, false) => "called list: old, state only",
                (_, true, _) => "called list: old, draws",
                (_, false, true) => "called list: old, calls",
            };
            note_cmd_time(kind, list.ops.len() as u64 * 1000);
        }
        let reuse_log = draw_reuse_log();
        for (op_index, op) in list.ops.iter().enumerate() {
            st.in_old_list = list_old && depth > 0;
            if reuse_log {
                st.cur_list_op = Some((std::sync::Arc::as_ptr(&list) as usize, op_index, list_old));
            }
            if cmd_time {
                let now = std::time::Instant::now();
                if let Some((n, t)) = cmd_t.replace((op.name, now)) {
                    note_cmd_time(n, (now - t).as_nanos() as u64);
                }
            }
            if replay_log
                && matches!(
                    op.typed,
                    Some(
                        Cmd::DrawArrays { .. }
                            | Cmd::DrawArraysInstanced { .. }
                            | Cmd::DrawElements { .. }
                            | Cmd::DrawElementsInstanced { .. }
                    )
                )
            {
                replay_note(st, handle, op_index, list_old, op);
            }
            // Commands go into the current command buffer, which `maybe_split` may have committed and replaced, also
            // inside called lists: TotK issues its occlusion queries from called lists, and game threads wait for them
            // (metal/report_guard.rs), so the command buffer holding them is committed right after them.
            if self.maybe_split(st) {
                let Some(new_cb) = st.cur_cb.clone() else {
                    return;
                };
                cur = new_cb;
            }
            let cb: &ProtocolObject<dyn MTLCommandBuffer> = &cur;
            if tracing {
                match &op.typed {
                    Some(Cmd::BindVertexBuffer { index, addr, size }) => println!(
                        "[cmdtrace] BindVertexBuffer {} {:#x}+{:#x}",
                        index, addr, size
                    ),
                    Some(Cmd::BindProgram { program, .. }) => {
                        println!("[cmdtrace] BindProgram {:#x}", program)
                    }
                    Some(Cmd::DrawElements {
                        count, base_vertex, ..
                    }) => println!(
                        "[cmdtrace] DrawElements count={} base_vertex={}",
                        count, base_vertex
                    ),
                    Some(Cmd::DrawElementsInstanced {
                        count, base_vertex, ..
                    }) => println!(
                        "[cmdtrace] DrawElementsInstanced count={} base_vertex={}",
                        count, base_vertex
                    ),
                    Some(Cmd::DrawArrays { count, .. }) => {
                        println!("[cmdtrace] DrawArrays count={}", count)
                    }
                    Some(Cmd::CallCommands(h)) => println!("[cmdtrace] CallCommands {:x?}", h),
                    Some(Cmd::SetRenderEnable(e)) => println!("[cmdtrace] SetRenderEnable {}", e),
                    Some(Cmd::Barrier(bits)) => println!("[cmdtrace] Barrier {:#x}", bits),
                    Some(Cmd::ResetCounter { counter }) => {
                        println!("[cmdtrace] ResetCounter {}", counter)
                    }
                    Some(Cmd::ReportCounter { counter, addr }) => {
                        println!("[cmdtrace] ReportCounter {} {:#x}", counter, addr)
                    }
                    Some(Cmd::BindUniformBuffer {
                        stage,
                        index,
                        addr,
                        size,
                    }) => println!(
                        "[cmdtrace] BindUniformBuffer s{} i{} {:#x}+{:#x}",
                        stage, index, addr, size
                    ),
                    Some(Cmd::CopyBufferToBuffer { src, dst, size }) => println!(
                        "[cmdtrace] CopyBufferToBuffer {:#x} -> {:#x} ({:#x} bytes)",
                        src, dst, size
                    ),
                    Some(Cmd::ClearBuffer { dst, size, value }) => println!(
                        "[cmdtrace] ClearBuffer {:#x} ({:#x} bytes) = {:#x}",
                        dst, size, value
                    ),
                    Some(Cmd::BindStorageBuffer {
                        stage,
                        index,
                        addr,
                        size,
                    }) => println!(
                        "[cmdtrace] BindStorageBuffer s{} i{} {:#x}+{:#x}",
                        stage, index, addr, size
                    ),
                    Some(Cmd::BindTexture {
                        stage,
                        index,
                        handle,
                    }) => println!("[cmdtrace] BindTexture s{} i{} {:#x}", stage, index, handle),
                    Some(Cmd::ReportValue { value, addr }) => {
                        println!("[cmdtrace] ReportValue {} {:#x}", value, addr)
                    }
                    Some(Cmd::SetRenderEnableConditional { mode, counters }) => println!(
                        "[cmdtrace] SetRenderEnableConditional mode={} {:#x}",
                        mode, counters
                    ),
                    Some(Cmd::ClearColor { index, mask, .. }) => {
                        println!("[cmdtrace] ClearColor c{} mask={:x}", index, mask)
                    }
                    Some(Cmd::SetRenderTargets { colors, depth }) => println!(
                        "[cmdtrace] SetRenderTargets {:x?} depth={:x?}",
                        colors.iter().map(|c| c.texture).collect::<Vec<_>>(),
                        depth.map(|d| d.texture)
                    ),
                    Some(_) => println!("[cmdtrace] {}", op.name),
                    None => println!(
                        "[cmdtrace] (ignored) {} args={:x?}",
                        op.name,
                        op.args.as_deref().map_or(&[][..], |a| &a[..])
                    ),
                }
            }
            let Some(cmd) = &op.typed else {
                // A dropped draw or dispatch silently removes geometry (or the GPU work that feeds it): say so once.
                if (op.name.contains("Draw") || op.name.contains("Dispatch"))
                    && !st.ignored_cmds.contains_key(op.name)
                {
                    static WARNED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
                    let mut w = WARNED.lock().unwrap();
                    if !w.contains(&op.name) {
                        w.push(op.name);
                        println!(
                            "[!] Gfx: {} is not executed (no handler), args={:x?}",
                            op.name,
                            op.args.as_deref().map_or(&[][..], |a| &a[..])
                        );
                    }
                }
                *st.ignored_cmds.entry(op.name).or_insert(0) += 1;
                continue;
            };
            if st.stats.submits <= 3 {
                match cmd {
                    Cmd::SetRenderTargets { colors, .. } => {
                        for (i, c) in colors.iter().enumerate() {
                            println!("[DEBUG GFX] SetRT[{}] tex=0x{:x}", i, c.texture);
                        }
                    }
                    Cmd::ClearColor { index, rgba, .. } => {
                        let rt_addr = st
                            .colors
                            .get(*index as usize)
                            .map(|c| c.texture)
                            .unwrap_or(0);
                        println!("[DEBUG GFX] ClearColor rt=0x{:x} rgba={:?}", rt_addr, rgba);
                    }
                    Cmd::DrawElements {
                        mode,
                        count,
                        base_vertex,
                        ..
                    } => {
                        let rt_addr = st.colors.first().map(|c| c.texture).unwrap_or(0);
                        println!(
                            "[DEBUG GFX] DrawElements rt=0x{:x} count={} mode={} base_vtx={}",
                            rt_addr, count, mode, base_vertex
                        );
                    }
                    Cmd::DrawArrays {
                        mode, first, count, ..
                    } => {
                        let rt_addr = st.colors.first().map(|c| c.texture).unwrap_or(0);
                        println!(
                            "[DEBUG GFX] DrawArrays rt=0x{:x} first={} count={} mode={}",
                            rt_addr, first, count, mode
                        );
                    }
                    Cmd::BindVertexBuffer { index, addr, size } => {
                        println!(
                            "[DEBUG GFX] BindVB[{}] addr=0x{:x} size={}",
                            index, addr, size
                        );
                    }
                    Cmd::BindTexture {
                        stage,
                        index,
                        handle,
                    } => {
                        println!(
                            "[DEBUG GFX] BindTex stage={} index={} handle=0x{:x}",
                            stage, index, handle
                        );
                    }
                    _ => {}
                }
            }
            if !st.render_enabled
                && matches!(
                    cmd,
                    Cmd::ClearColor { .. }
                        | Cmd::ClearDepthStencil { .. }
                        | Cmd::DrawArrays { .. }
                        | Cmd::DrawArraysInstanced { .. }
                        | Cmd::DrawElements { .. }
                        | Cmd::DrawElementsInstanced { .. }
                )
            {
                if tracing_frame() {
                    let count = match cmd {
                        Cmd::DrawArrays { count, .. } | Cmd::DrawElements { count, .. } => {
                            *count as i64
                        }
                        Cmd::DrawArraysInstanced { count, .. }
                        | Cmd::DrawElementsInstanced { count, .. } => *count as i64,
                        _ => -1,
                    };
                    println!(
                        "[ftrace] COND-SKIP count={} program={:#x}",
                        count,
                        st.program.unwrap_or(0)
                    );
                }
                journal(|| format!("  cond-skip program {:#x}", st.program.unwrap_or(0)));
                continue;
            }
            match cmd {
                Cmd::SetRenderEnable(enable) => {
                    if journal_on() && st.render_enabled != *enable {
                        journal(|| format!("  render-enable {}", enable));
                    }
                    st.render_enabled = *enable;
                }
                Cmd::SetPointSize(sz) => {
                    if tracing_frame() {
                        println!("[cmdtrace] SetPointSize {}", sz);
                    }
                    st.point_size = *sz;
                }
                Cmd::SetRenderEnableConditional { mode, counters } => {
                    // Counter reports earlier in the stream were written when they executed, so this sees them in order,
                    // except SAMPLES_PASSED reports: those are written when their command buffer completes, so a
                    // condition on one from this submit or an uncompleted one is not known yet while encoding. The GPU
                    // would evaluate it after the report; Metal has no predication, so such draws render (Ryujinx's
                    // fallback when a counter is not available). Skipping them instead culled TotK's Hyrule Field grass
                    // patches (both reports still read 0).
                    // Reports of command buffers the fence watcher resolves sit in `occl_watched` until it writes them
                    // (they used to be missed here: conditions on them read stale memory and skipped the draw, e.g. a
                    // signpost's new LOD for one frame, 2026-10-14).
                    let unresolved = |a: u64| {
                        st.occl.pending.iter().any(|(_, r)| *r == a)
                            || st.inflight.iter().any(|f| {
                                f.occl.iter().any(|(_, r)| *r == a)
                                    || f.occl_watched.as_ref().is_some_and(|w| {
                                        w.lock()
                                            .unwrap()
                                            .as_ref()
                                            .is_some_and(|v| v.iter().any(|(_, r)| *r == a))
                                    })
                            })
                    };
                    let pending = *counters > 0x10000
                        && (unresolved(*counters) || unresolved(*counters + 16));
                    st.render_enabled = if pending {
                        true
                    } else if *counters > 0x10000 {
                        // Report pages held by --report-guard are read through their aliases (the engine never waits).
                        let p = report_guard::engine_ptr(*counters as usize) as *const u64;
                        let equal = p.read_unaligned()
                            == (report_guard::engine_ptr(*counters as usize + 16) as *const u64)
                                .read_unaligned();
                        equal == (*mode == 0)
                    } else {
                        true
                    };
                    let read2 = |c: u64| unsafe {
                        (
                            (report_guard::engine_ptr(c as usize) as *const u64).read_unaligned(),
                            (report_guard::engine_ptr(c as usize + 16) as *const u64)
                                .read_unaligned(),
                        )
                    };
                    if journal_on() && *counters > 0x10000 {
                        let (a, b) = read2(*counters);
                        let en = st.render_enabled;
                        journal(|| {
                            format!(
                                "  cond mode {} @{:#x} [{:#x}, {:#x}]{} -> {}",
                                mode,
                                counters,
                                a,
                                b,
                                if pending { " pending" } else { "" },
                                en
                            )
                        });
                    }
                    if tracing_frame() && *counters > 0x10000 {
                        let (a, b) = read2(*counters);
                        println!("[cmdtrace]   conditional mode={} @{:#x} [{:#x}, {:#x}]{} -> render_enabled={}", mode, counters, a, b, if pending { " (report pending)" } else { "" }, st.render_enabled);
                    }
                }
                Cmd::BindTexturesDeferred {
                    stage,
                    first,
                    count,
                    handles,
                } => {
                    for i in 0..(*count).clamp(0, 32) {
                        let h = if *handles > 0x10000 {
                            std::ptr::read_unaligned((*handles as *const u64).add(i as usize))
                        } else {
                            0
                        };
                        st.textures_bound.insert((*stage, *first + i), h);
                    }
                }
                Cmd::SetRenderTargets { colors, depth } => {
                    if let Some(w) = watch_storage() {
                        for (i, c) in colors.iter().enumerate() {
                            if tex_info(c.texture).is_some_and(|(_, t)| t.storage == w) {
                                println!("[watch] f{} render target c{} = texture 0x{:x} (storage {:#x})", presented_frames() + 1, i, c.texture, w);
                                // --watch-trace: trace the rest of this frame (and --trace-trigger-frames after it).
                                if crate::settings::get().trace.watch_trace
                                    && TRACE_TRIGGERED_FRAME
                                        .load(std::sync::atomic::Ordering::Relaxed)
                                        == 0
                                {
                                    TRACE_TRIGGERED_FRAME.store(
                                        presented_frames() + 1,
                                        std::sync::atomic::Ordering::Relaxed,
                                    );
                                }
                            }
                        }
                    }
                    // Debug: --log-rt-frames=a-b logs every render-target bind (storage, size, format) in frames a..b.
                    if let Some(crate::settings::Span {
                        first: fa,
                        last: fb,
                    }) = crate::settings::get().log.log_rt_frames
                    {
                        let f = presented_frames() + 1;
                        if (fa..=fb).contains(&f) {
                            for (i, c) in colors.iter().enumerate() {
                                if let Some((_, t)) = tex_info(c.texture) {
                                    println!("[rt-log] f{} c{} storage {:#x} {}x{}x{} fmt {} levels {} level {} layer {} view_format {}", f, i, t.storage, t.width, t.height, t.depth, t.format, t.levels, c.level, c.layer, c.view_format);
                                }
                            }
                        }
                    }
                    if rt_writeback_enabled() {
                        let f = presented_frames() + 1;
                        for c in colors.iter() {
                            let Some((_, t)) = tex_info(c.texture) else {
                                continue;
                            };
                            // Only targets not rendered in the previous 4 frames: per-frame targets, swapchain images and
                            // ping-pong pairs are skipped after their first use, textures rendered once (generated maps)
                            // are written back.
                            let prev = st.rt_history.insert(t.storage, f);
                            if prev.map_or(true, |p| p + 4 < f) {
                                st.writeback.0.insert(c.texture, f);
                            }
                        }
                        if st.rt_history.len() > 1 << 14 {
                            st.rt_history.retain(|_, &mut v| v + 600 > f);
                        }
                    }
                    if let Some((ww, wh)) = watch_size() {
                        for (i, c) in colors.iter().enumerate() {
                            if let Some((_, t)) = tex_info(c.texture)
                                .filter(|(_, t)| t.width as u32 == ww && t.height as u32 == wh)
                            {
                                println!("[watch-size] f{} render target c{} = texture {:#x} storage {:#x} fmt {} levels {} view {:?}",
                                    presented_frames() + 1, i, c.texture, t.storage, t.format, t.levels, c);
                            }
                        }
                    }
                    st.colors = colors.clone();
                    st.depth = *depth;
                    st.rts_dirty = true;
                    st.cached_rts = None;
                }
                Cmd::ClearColor { index, rgba, mask } => {
                    st.stats.clears += 1;
                    self.clear(st, cb, *index, *rgba, *mask);
                }
                Cmd::ClearDepthStencil {
                    depth,
                    depth_mask,
                    stencil,
                    stencil_mask,
                } => {
                    st.stats.clears += 1;
                    self.clear_depth(st, cb, *depth, *depth_mask, *stencil, *stencil_mask);
                }
                Cmd::BindProgram { program, stages } => {
                    // NVN_SHADER_STAGE_COMPUTE_BIT: compute and graphics programs are bound independently.
                    if *stages & 0x20 != 0 {
                        st.compute_program = Some(*program);
                    }
                    if *stages & 0x1F != 0 || *stages == 0 {
                        st.program = Some(*program);
                    }
                }
                Cmd::DispatchCompute { x, y, z } => {
                    if translated::enabled() {
                        self.end_encoder(st);
                        self.dispatch_compute(st, cb, *x, *y, *z);
                    }
                }
                Cmd::DrawArrays {
                    mode,
                    first,
                    count,
                    caller,
                } => {
                    st.cur_caller = *caller;
                    st.stats.draws += 1;
                    if st.occl.counting {
                        st.occl.draws_seen += 1;
                    }
                    if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                        let t = (*caller != 0).then(std::time::Instant::now);
                        self.draw_arrays(st, cb, *mode, *first, *count, 0, 1);
                        if let Some(t) = t {
                            crate::nvn::cmd::note_draw_time(*caller, t.elapsed().as_nanos() as u64)
                        }
                    }
                }
                Cmd::DrawArraysInstanced {
                    mode,
                    first,
                    count,
                    base_instance,
                    instances,
                    caller,
                } => {
                    st.cur_caller = *caller;
                    st.stats.draws += 1;
                    if st.occl.counting {
                        st.occl.draws_seen += 1;
                    }
                    if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                        let t = (*caller != 0).then(std::time::Instant::now);
                        self.draw_arrays(st, cb, *mode, *first, *count, *base_instance, *instances);
                        if let Some(t) = t {
                            crate::nvn::cmd::note_draw_time(*caller, t.elapsed().as_nanos() as u64)
                        }
                    }
                }
                Cmd::DrawElements {
                    mode,
                    index_type,
                    count,
                    index_addr,
                    base_vertex,
                    caller,
                } => {
                    st.cur_caller = *caller;
                    st.stats.draws += 1;
                    if st.occl.counting {
                        st.occl.draws_seen += 1;
                    }
                    if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                        let t = (*caller != 0).then(std::time::Instant::now);
                        self.draw_elements(
                            st,
                            cb,
                            *mode,
                            *index_type,
                            *count,
                            *index_addr,
                            *base_vertex,
                            0,
                            1,
                        );
                        if let Some(t) = t {
                            crate::nvn::cmd::note_draw_time(*caller, t.elapsed().as_nanos() as u64)
                        }
                    }
                }
                Cmd::DrawElementsInstanced {
                    mode,
                    index_type,
                    count,
                    index_addr,
                    base_vertex,
                    base_instance,
                    instances,
                    caller,
                } => {
                    st.cur_caller = *caller;
                    st.stats.draws += 1;
                    if st.occl.counting {
                        st.occl.draws_seen += 1;
                    }
                    if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                        let t = (*caller != 0).then(std::time::Instant::now);
                        self.draw_elements(
                            st,
                            cb,
                            *mode,
                            *index_type,
                            *count,
                            *index_addr,
                            *base_vertex,
                            *base_instance,
                            *instances,
                        );
                        if let Some(t) = t {
                            crate::nvn::cmd::note_draw_time(*caller, t.elapsed().as_nanos() as u64)
                        }
                    }
                }
                Cmd::ResetCounter { counter } => {
                    if *counter == 1 && self.real_samples() {
                        self.occl_reset(st);
                    }
                }
                Cmd::ReportCounter { counter, addr } => {
                    if *counter == 1 && self.real_samples() {
                        self.occl_report(st, *addr);
                    } else {
                        write_counter(*counter, *addr, None);
                        if *counter == 0 && gpu_timestamps() && *addr >= 0x10000 {
                            // Rewritten with the GPU's time once the command buffer completes (the CPU time stays as a
                            // provisional value).
                            st.ts_pending.push((*addr, st.lb.enc_labels.is_empty()));
                        }
                        if *counter == 1 {
                            report_guard::close(crate::nvn::api::translate_virtual(*addr as usize));
                        }
                    }
                    st.plan_ubo.invalidate(*addr, *addr + 16);
                    st.ubo_plan_epoch += 1;
                }
                Cmd::ReportValue { value, addr } => {
                    write_counter(0, *addr, Some(*value as u64));
                    st.plan_ubo.invalidate(*addr, *addr + 16);
                    st.ubo_plan_epoch += 1;
                }
                Cmd::BindBlendState(s) => {
                    st.blend_states[(*s & 7) as usize] = *s;
                }
                Cmd::BindColorState(s) => {
                    st.color_state = *s;
                }
                Cmd::BindChannelMaskState(s) => {
                    st.channel_mask = *s;
                }
                Cmd::SetViewport { x, y, w, h } => {
                    st.viewport = Some((*x, *y, *w, *h));
                    // The main 3D passes (3+ colour targets: the G-buffer) show the game's render resolution (its
                    // dynamic resolution scales the viewport): reported with the fps line.
                    if st.colors.len() >= 3 && *w > 0 && *h > 0 {
                        // Largest since the last report (area in the high bits).
                        let (w, h) = ((*w as u64).min(0xFFFFF), (*h as u64).min(0xFFFFF));
                        SCENE_VIEWPORT.fetch_max((w * h) << 40 | w << 20 | h, Ordering::Relaxed);
                    }
                }
                Cmd::SetScissor { x, y, w, h } => {
                    st.scissor = Some((*x, *y, *w, *h));
                }
                Cmd::BindVertexBuffer { index, addr, size } => {
                    st.vertex_buffers.insert(*index, (*addr, *size));
                }
                Cmd::BindUniformBuffer {
                    stage,
                    index,
                    addr,
                    size,
                } => {
                    st.uniform_buffers.insert((*stage, *index), (*addr, *size));
                }
                Cmd::BindStorageBuffer {
                    stage,
                    index,
                    addr,
                    size,
                } => {
                    if (0..6).contains(stage) && (0..16).contains(index) {
                        st.storage_buffers[*stage as usize][*index as usize] = (*addr, *size);
                    }
                }
                Cmd::UpdateUniformBuffer { addr, offset, data } => {
                    if *addr > 0x10000 && !data.is_empty() {
                        let dest = (*addr as isize + *offset) as *mut u8;
                        std::ptr::copy_nonoverlapping(data.as_ptr(), dest, data.len());
                        // Plan-side copies of uniform buffers overlapping the written range are stale now (only those
                        // overlap: the encode side copies only ranges no executor write of this submit touches).
                        let (lo, hi) = (dest as u64, dest as u64 + data.len() as u64);
                        st.plan_ubo.invalidate(lo, hi);
                        st.ubo_plan_epoch += 1;
                    }
                }
                Cmd::BindVertexAttribState(attribs) => {
                    st.vertex_attribs = *attribs;
                }
                Cmd::BindVertexStreamState(streams) => {
                    st.vertex_streams.clone_from(streams);
                }
                Cmd::BindImage {
                    stage,
                    index,
                    handle,
                } => {
                    st.images_bound.insert((*stage, *index), *handle);
                }
                Cmd::BindTexture {
                    stage,
                    index,
                    handle,
                } => {
                    st.textures_bound.insert((*stage, *index), *handle);
                }
                Cmd::CopyBufferToTexture { .. }
                | Cmd::CopyTextureToBuffer { .. }
                | Cmd::CopyTextureToTexture(..)
                | Cmd::CopyBufferToBuffer { .. }
                | Cmd::ClearBuffer { .. }
                | Cmd::ClearTexture(..) => {
                    if let Some(w) = watch_storage() {
                        let dst = match cmd {
                            Cmd::CopyBufferToTexture { dst, .. } => Some(dst.texture),
                            Cmd::CopyTextureToTexture(p) => Some(p.dst.texture),
                            Cmd::ClearTexture(p) => Some(p.dst.texture),
                            _ => None,
                        };
                        if dst
                            .and_then(|d| tex_info(d))
                            .is_some_and(|(_, t)| t.storage == w)
                        {
                            println!(
                                "[watch] f{} copy/clear into storage {:#x}: {:?}",
                                presented_frames() + 1,
                                w,
                                cmd
                            );
                        }
                    }
                    if let Some((ww, wh)) = watch_size() {
                        let dst = match cmd {
                            Cmd::CopyBufferToTexture { dst, .. } => Some(dst.texture),
                            Cmd::CopyTextureToTexture(p) => Some(p.dst.texture),
                            Cmd::ClearTexture(p) => Some(p.dst.texture),
                            _ => None,
                        };
                        if let Some((_, t)) = dst
                            .and_then(|d| tex_info(d))
                            .filter(|(_, t)| t.width as u32 == ww && t.height as u32 == wh)
                        {
                            println!("[watch-size] f{} copy/clear into storage {:#x} fmt {} levels {}: {:?}", presented_frames() + 1, t.storage, t.format, t.levels, cmd);
                        }
                        let src = match cmd {
                            Cmd::CopyTextureToBuffer { src, .. } => Some(src.texture),
                            Cmd::CopyTextureToTexture(p) => Some(p.src.texture),
                            _ => None,
                        };
                        if let Some((_, t)) = src
                            .and_then(|d| tex_info(d))
                            .filter(|(_, t)| t.width as u32 == ww && t.height as u32 == wh)
                        {
                            println!(
                                "[watch-size] f{} copy out of storage {:#x} fmt {} levels {}: {:?}",
                                presented_frames() + 1,
                                t.storage,
                                t.format,
                                t.levels,
                                cmd
                            );
                        }
                    }
                    if gfx_mode() >= GfxMode::All || gfx_mode() == GfxMode::Dry {
                        self.exec_copy(st, cb, cmd);
                    }
                }
                Cmd::BindDepthStencilState(s) => {
                    st.depth_stencil = *s;
                }
                Cmd::BindPolygonState(s) => {
                    st.polygon = *s;
                }
                Cmd::BindMultisampleState(s) => {
                    st.multisample = *s;
                }
                Cmd::Barrier(bits) => {
                    // INVALIDATE_TEXTURE: texture reads after this see render-target writes from before it. The ordering
                    // that makes those writes complete may come in the same call or an earlier one ("either by
                    // setting an ORDER bit in the barrier or by waiting on a NVNsync object"): Kirby issues ORDER_PRIMITIVES
                    // and INVALIDATE_TEXTURE as separate barriers, and requiring both bits in one call never started a new
                    // epoch there, so its composite pass read stale texels (black screen).
                    if bits & 0x10 != 0 {
                        st.fb_epoch += 1;
                    }
                }
                Cmd::SetDepthRange { near, far } => {
                    st.depth_range = (*near, *far);
                }
                Cmd::SetStencilValue { kind, faces, value } => {
                    // NVNface: FRONT 1, BACK 2, FRONT_AND_BACK 3. Stencil values are 8-bit here (D24S8 / D32FS8).
                    let k = (*kind as usize).min(2);
                    if faces & 1 != 0 {
                        st.stencil[k][0] = *value as u8;
                    }
                    if faces & 2 != 0 {
                        st.stencil[k][1] = *value as u8;
                    }
                }
                Cmd::SetDepthBounds { enable, min, max } => {
                    st.depth_bounds = enable.then_some((*min, *max));
                    // Recorded but not applied yet (Metal has no depth bounds test); say so once if a game enables it.
                    static WARNED: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(false);
                    if *enable && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        println!(
                            "[!] Gfx: depth bounds test enabled ({}..{}) but not emulated",
                            min, max
                        );
                    }
                    if tracing_frame() {
                        println!("[ftrace] DEPTH BOUNDS {:?}", st.depth_bounds);
                    }
                }
                Cmd::SetDepthClamp(on) => {
                    st.depth_clamp = *on;
                }
                Cmd::SetPrimitiveRestart { enable, index } => {
                    st.prim_restart = enable.then_some(*index);
                }
                Cmd::SetPolygonOffset {
                    factor,
                    units,
                    clamp,
                } => {
                    st.polygon_offset = (*factor, *units, *clamp);
                }
                Cmd::SetBlendColor(c) => {
                    st.blend_color = *c;
                }
                Cmd::CallCommands(handles) => {
                    // --log-draw-reuse: the call path through lists from earlier frames (per-frame lists change
                    // every frame and are left out).
                    let saved_ctx = st.call_ctx;
                    if reuse_log && list_old {
                        st.call_ctx = st.call_ctx.rotate_left(17)
                            ^ (std::sync::Arc::as_ptr(&list) as u64)
                                .wrapping_mul(0x9e3779b97f4a7c15)
                            ^ op_index as u64;
                    }
                    for h in handles {
                        self.run_list(st, cb, *h, depth + 1);
                    }
                    st.call_ctx = saved_ctx;
                    if let Some(new_cb) = &st.cur_cb {
                        if Retained::as_ptr(&cur) != Retained::as_ptr(new_cb) {
                            cur = new_cb.clone();
                        }
                    }
                }
            }
        }
        if let Some((n, t)) = cmd_t {
            note_cmd_time(n, t.elapsed().as_nanos() as u64);
        }
    }

    unsafe fn draw_elements(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        mode: i32,
        index_type: i32,
        count: i32,
        index_addr: u64,
        base_vertex: i32,
        base_instance: i32,
        instances: i32,
    ) {
        if translated::enabled()
            && self.draw_translated(
                st,
                cb,
                mode,
                translated::DrawKind::Elements {
                    index_type,
                    count,
                    index_addr,
                    base_vertex,
                    base_instance,
                    instances,
                },
            )
        {
            return;
        }
        // No translation: the draw is skipped (translated.rs warns once per program).
    }

    unsafe fn draw_arrays(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        mode: i32,
        first: i32,
        count: i32,
        base_instance: i32,
        instances: i32,
    ) {
        if translated::enabled()
            && self.draw_translated(
                st,
                cb,
                mode,
                translated::DrawKind::Arrays {
                    first,
                    count,
                    base_instance,
                    instances,
                },
            )
        {
            return;
        }
        // No translation: the draw is skipped (translated.rs warns once per program).
    }

    unsafe fn clear(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        index: i32,
        rgba: [f32; 4],
        mask: i32,
    ) {
        if mask != 0xF || index < 0 || index as usize >= st.colors.len() {
            static WARNED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if mask != 0xF && WARNED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 8 {
                println!(
                    "[!] Gfx: partial-channel clear dropped: c{} mask {:#x} rgba {:?}",
                    index, mask, rgba
                );
            }
            return; // partial-channel clears need a draw; not needed yet
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
        st.tc.gpu_written.insert(storage_id(&tex));
        if !is_color_renderable(&tex) {
            return; // e.g. a compressed or depth texture named as a color target; Metal would abort
        }
        let _ = cb;
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

    unsafe fn clear_depth(
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
        st.tc.gpu_written.insert(storage_id(&tex));
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
            println!("[ftrace] CLEAR DEPTH nvn {:#x} -> mtl {:?} {}x{} arr{} lvls{} parent={} | level {} slice {} | depth {} mask {} stencil {} smask {}",
                rt.texture, tex.textureType(), tex.width(), tex.height(), tex.arrayLength(), tex.mipmapLevelCount(), tex.parentTexture().is_some(),
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

/// --log-occl: SAMPLES_PASSED reports whose draws (since the counter reset) did not all reach a counting encoder.
pub(super) fn log_occl() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().log.log_occl)
}

/// SAMPLES_PASSED occlusion queries on Metal visibility results.
///
/// NVN keeps one running sample counter: ResetCounter zeroes it and every ReportCounter writes its current value
/// (NVNcounterData {u64 value, u64 timestamp}). Metal counts per visibility-buffer slot and, at the end of a render pass,
/// overwrites the slots that pass used (others keep their values). So each stretch of counted draws inside one encoder
/// gets its own slot, and a report is the sum of the slots used since the last reset. Reports are written to guest
/// memory once the command buffer has completed (`resolve`, when `submit` or a sync wait retires it), like a query read
/// a frame or two late on hardware.
pub(super) struct Occlusion {
    buf: Option<Retained<ProtocolObject<dyn MTLBuffer>>>,
    /// Next free slot in `buf` (a ring; one command buffer in flight uses a few hundred).
    next: usize,
    /// A ResetCounter has been seen: draws are counted from then on.
    counting: bool,
    /// Slots summed by the next report.
    slots: Vec<usize>,
    /// Slot the open encoder is counting into.
    pub(super) cur: Option<usize>,
    /// Reports recorded in the command buffer being built (moved to its `InFlight` entry on commit).
    pending: Vec<(Vec<usize>, u64)>,
    /// --log-occl: draw commands executed since the last reset, and how many of them reached a counting encoder.
    pub(super) draws_seen: u32,
    pub(super) draws_counted: u32,
    /// Reports recorded in the current frame (journal ordinal).
    pub(super) reports_in_frame: u32,
    pub(super) ord_frame: u64,
}

impl Default for Occlusion {
    fn default() -> Self {
        Occlusion {
            buf: None,
            next: 0,
            counting: false,
            slots: Vec::new(),
            cur: None,
            pending: Vec::new(),
            draws_seen: 0,
            draws_counted: 0,
            reports_in_frame: 0,
            ord_frame: 0,
        }
    }
}

impl Occlusion {
    /// Metal limits visibility result offsets to 256 KiB on Apple GPUs (larger ones are silently not written).
    const SLOTS: usize = 1 << 15;

    /// The visibility buffer every render pass is given (so counting can start inside any open encoder).
    pub(super) fn buffer(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
    ) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        if self.buf.is_none() {
            self.buf = device.newBufferWithLength_options(
                Self::SLOTS * 8,
                MTLResourceOptions::StorageModeShared,
            );
        }
        self.buf.clone()
    }

    fn resolve(&self, reports: Vec<(Vec<usize>, u64)>) {
        if let Some(buf) = &self.buf {
            resolve_occlusion(buf, &reports);
        }
    }
}

/// Writes SAMPLES_PASSED reports of a completed command buffer: each the sum of its visibility slots.
fn resolve_occlusion(buf: &ProtocolObject<dyn MTLBuffer>, reports: &[(Vec<usize>, u64)]) {
    let vals = buf.contents().as_ptr() as *const u64;
    for (slots, addr) in reports {
        let n: u64 = slots.iter().map(|&s| unsafe { vals.add(s).read() }).sum();
        if tracing_frame() {
            println!(
                "[cmdtrace] SAMPLES_PASSED {:#x} = {} ({} slots)",
                addr,
                n,
                slots.len()
            );
        }
        // The host address and the count it held before (journal; matches the occl-read lines).
        let host = crate::nvn::api::translate_virtual(*addr as usize);
        let before = if journal_on() && host >= 0x10000 {
            unsafe { *(report_guard::engine_ptr(host) as *const u64) as i64 }
        } else {
            0
        };
        write_counter(1, *addr, Some(n));
        report_guard::close(host);
        if journal_on() || log_occl() {
            if let Some((f, ord, pos, draws)) = OCCL_TAGS.lock().unwrap().remove(addr) {
                let line = format!(
                    "occl f{} #{} pos {:?} draws {} = {} @{:#x} was {} t {:.3}",
                    f,
                    ord,
                    pos,
                    draws,
                    n,
                    host,
                    before,
                    host_secs() * 1e3
                );
                if log_occl() && n == 0 {
                    println!("[occl] {}", line);
                }
                journal(|| line);
            }
        }
    }
}

/// --journal-occl-reads (TotK 1.1.2, profile hook at main+0x10c4d44): the game's occlusion query read, called
/// with a query object. The object keeps three report slots at +0x00 / +0x20 / +0x40 of its report buffer ([obj+8]:
/// GPU address at +0x48, memory pool at +0x50; the pool's CPU pointer at +0x100, its GPU base at [[+0x110]+0x18]), a
/// state byte per slot (+0x22..+0x24: 1 = issued, advanced to 3, or to 2 with flag bit 2; 3 = readable), the result at
/// +0x18 (reset to -1) and flags at +0x20 (bit 1 visible, bit 0 result read). The first slot in state 3 is read:
/// count 0 clears bit 1, else it stays set; no readable slot leaves "visible, no result". Journals the object's state
/// before the read: `occl-read obj <obj> fl <flags> st <s0>,<s1>,<s2> <host addr>=<count> ...` (match the
/// addresses with the `@` addresses of the `occl` lines).
pub(crate) unsafe extern "C" fn journal_occl_read(obj: usize) {
    if !journal_on() || obj < 0x10000 {
        return;
    }
    let rd = |a: usize| -> usize {
        if a >= 0x10000 {
            *(a as *const usize)
        } else {
            0
        }
    };
    let buf = rd(obj + 8);
    let pool = if buf != 0 { rd(buf + 0x50) } else { 0 };
    let cpu = if pool != 0 { rd(pool + 0x100) } else { 0 };
    let p110 = if pool != 0 { rd(pool + 0x110) } else { 0 };
    let off = if p110 != 0 {
        rd(buf + 0x48).wrapping_sub(rd(p110 + 0x18))
    } else {
        0
    };
    let b = |o: usize| *((obj + o) as *const u8);
    let slots: Vec<String> = (0..3)
        .map(|k| {
            let a = cpu.wrapping_add(off).wrapping_add(0x20 * k);
            if cpu != 0 {
                format!("{:#x}={}", a, *(a as *const u64) as i64)
            } else {
                "-".to_string()
            }
        })
        .collect();
    let thread = std::thread::current();
    let line = format!(
        "occl-read obj {:#x} fl {:#x} st {},{},{} {} t {:.3} {}",
        obj,
        b(0x20),
        b(0x22),
        b(0x23),
        b(0x24),
        slots.join(" "),
        host_secs() * 1e3,
        thread.name().unwrap_or("?")
    );
    journal(|| line);
}

/// Journal tags of recorded SAMPLES_PASSED reports: frame, ordinal in the frame, query box position, draws.
static OCCL_TAGS: Mutex<HashMap<u64, (u64, u32, Vec<f32>, u32)>> =
    Mutex::new(HashMap::with_hasher(rustc_hash::FxBuildHasher));

impl Gfx {
    /// Real occlusion queries need the translated-shader draw path (the only one that counts); otherwise SAMPLES_PASSED
    /// reports 0.
    fn real_samples(&self) -> bool {
        self.shader_lib.is_some()
    }

    fn occl_reset(&self, st: &mut State) {
        self.occl_stop(st);
        st.occl.counting = true;
        st.occl.slots.clear();
        st.occl.draws_seen = 0;
        st.occl.draws_counted = 0;
    }

    fn occl_report(&self, st: &mut State, addr: u64) {
        // Later draws must not land in slots this report already sums (they are only final when the pass ends).
        self.occl_stop(st);
        if st.occl.counting && addr >= 0x10000 {
            if log_occl() && st.occl.draws_seen != st.occl.draws_counted {
                static N: AtomicU64 = AtomicU64::new(0);
                if N.fetch_add(1, Ordering::Relaxed) < 2000 {
                    println!("[occl] f{} report {:#x}: {} draw commands since reset, {} counted ({} slots)", presented_frames() + 1, addr,
                        st.occl.draws_seen, st.occl.draws_counted, st.occl.slots.len());
                }
            }
            if journal_on() || log_occl() {
                // The query box's position (its draw's vertex uniform buffer 1 starts with it) names the object across frames.
                let pos = st
                    .uniform_buffers
                    .get(&(0, 1))
                    .filter(|(a, sz)| *a >= 0x10000 && *sz >= 16)
                    .map(|(a, _)| unsafe {
                        std::slice::from_raw_parts(
                            report_guard::engine_ptr(crate::nvn::api::translate_virtual(
                                *a as usize,
                            )) as *const f32,
                            3,
                        )
                        .to_vec()
                    })
                    .unwrap_or_default();
                if st.occl.ord_frame != presented_frames() {
                    st.occl.ord_frame = presented_frames();
                    st.occl.reports_in_frame = 0;
                }
                let ord = st.occl.reports_in_frame;
                OCCL_TAGS
                    .lock()
                    .unwrap()
                    .insert(addr, (presented_frames() + 1, ord, pos, st.occl.draws_seen));
            }
            st.occl.reports_in_frame += 1;
            let slots = st.occl.slots.clone();
            st.occl.pending.push((slots, addr));
            note_gpu_guest_write(GuestWrite::Occlusion, addr, 16);
        } else {
            // No count to report (no reset since the last report): nothing will be written there.
            report_guard::close(crate::nvn::api::translate_virtual(addr as usize));
        }
    }

    fn occl_stop(&self, st: &mut State) {
        if st.occl.cur.take().is_some() {
            if let Some(enc) = &st.enc {
                enc.push(issuer::Op::Visibility(
                    objc2_metal::MTLVisibilityResultMode::Disabled,
                    0,
                ));
            }
        }
    }

    /// Called before each draw in the open encoder: starts counting into a fresh slot if needed.
    pub(super) fn occl_before_draw(&self, st: &mut State) {
        if !st.occl.counting || st.occl.cur.is_some() {
            return;
        }
        let Some(enc) = st.enc.clone() else { return };
        let s = st.occl.next;
        st.occl.next = (s + 1) % Occlusion::SLOTS;
        enc.push(issuer::Op::Visibility(
            objc2_metal::MTLVisibilityResultMode::Counting,
            s * 8,
        ));
        st.occl.slots.push(s);
        st.occl.cur = Some(s);
    }
}

/// Executes nvnCommandBufferReportCounter / ReportValue: writes an NVNcounterData {u64 counter, u64 timestamp} to the
/// guest buffer. The GPU would write it in stream order; doing it when the command is executed is equivalent for the
/// game, which only reads it after the fence. Timestamps are real; SAMPLES_PASSED normally goes through `Occlusion`
/// (this only writes the --fake-samples value); the other statistics counters read as zero.
/// Counts CPU-side writes the executor makes to guest memory (counters, report values, uniform buffer updates):
/// uniform buffer copies cached within a submit are dropped when it changes.
pub(super) static GUEST_WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 16 KiB pages holding uniform data copied in this session (metal/translated.rs notes them): counter writes elsewhere cannot
/// change a cached copy, so only writes to these pages advance GUEST_WRITES. Counters and timestamps (~54 per Hateno
/// frame) used to drop every copy (~5000 copies per frame against ~16000 reuses). --ubo-epoch-all: every write
/// advances it (A/B).
pub(super) static UBO_PAGES: std::sync::Mutex<Option<rustc_hash::FxHashSet<u64>>> =
    std::sync::Mutex::new(None);

pub(super) fn note_ubo_pages(addr: u64, len: usize) {
    let mut g = UBO_PAGES.lock().unwrap();
    let set = g.get_or_insert_with(Default::default);
    for page in (addr >> 14)..=((addr + len.max(1) as u64 - 1) >> 14) {
        set.insert(page);
    }
}

fn write_counter(counter: i32, addr: u64, value: Option<u64>) {
    if addr < 0x10000 {
        return;
    }
    let touches_ubo = UBO_PAGES
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|s| s.contains(&(addr >> 14)) || s.contains(&((addr + 15) >> 14)));
    if touches_ubo {
        GUEST_WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    // Same clock as nvnDeviceGetTimestampInNanoseconds (horizon/hle.rs): mach ticks converted to nanoseconds.
    let now = crate::clock::now_ns();
    // TIMESTAMP (0) reports the time as the counter; TIMESTAMP_TOP (0x10) writes zero there (games use it to reset
    // markers for SetRenderEnableConditional); ZCULL_STATS (0xF) is four 32-bit words and no timestamp.
    let counter_value = value.unwrap_or(match counter {
        0 => now,
        _ => 0,
    });
    static LOG_TS: OnceLock<bool> = OnceLock::new();
    if *LOG_TS.get_or_init(|| crate::settings::get().log.log_timestamps)
        && value.is_none()
        && (counter == 0 || counter == 0x10)
    {
        println!(
            "[ts] f{} counter {:#x} addr {:#x} t {:.3} ms (submit {})",
            presented_frames(),
            counter,
            addr,
            now as f64 / 1e6,
            submitted_serial()
        );
    }
    unsafe {
        // --report-guard: a report page held from the game is written through its alias.
        // Written through a read-write alias of the page while the write guard runs (it, or the report guard, may hold
        // the page read-only; the report guard's own alias can be read-only too).
        let p = if fence_guard::enabled() {
            fence_guard::engine_write_ptr(addr as usize)
        } else {
            report_guard::engine_ptr(addr as usize)
        } as *mut u64;
        p.write_unaligned(counter_value);
        p.add(1)
            .write_unaligned(if counter == 0xF && value.is_none() {
                0
            } else {
                now
            });
    }
}

/// Prints everything Metal reports about a failed command buffer, including the per-encoder status that names the
/// debug groups (draw labels, see metal/translated.rs) that were running when the GPU faulted or timed out.
fn report_gpu_error(
    cb: &ProtocolObject<dyn MTLCommandBuffer>,
    labels: &[String],
    details: &[Vec<String>],
) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSString};
    let Some(err) = cb.error() else {
        println!("[!!] GPU: command buffer ended in an error state without an error object");
        return;
    };
    println!(
        "[!!] GPU: command buffer failed: code {} -- {}",
        err.code(),
        err.localizedDescription()
    );
    let mut first_bad: Option<usize> = None;
    unsafe {
        let info = err.userInfo();
        let key = objc2_metal::MTLCommandBufferEncoderInfoErrorKey;
        if let Some(infos) = info.objectForKey(key) {
            let infos: &NSArray<AnyObject> =
                &*(Retained::as_ptr(&infos) as *const NSArray<AnyObject>);
            for i in 0..infos.count() {
                let enc = infos.objectAtIndex(i);
                let label: Option<Retained<NSString>> = msg_send![&*enc, label];
                let state: isize = msg_send![&*enc, errorState];
                let signposts: Option<Retained<NSArray<NSString>>> =
                    msg_send![&*enc, debugSignposts];
                let marks: Vec<String> = signposts
                    .map(|a| {
                        (0..a.count())
                            .map(|j| a.objectAtIndex(j).to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                // MTLCommandEncoderErrorState: 0 unknown, 1 completed, 2 affected, 3 pending, 4 faulted
                if state != 1 && first_bad.is_none() {
                    first_bad = Some(i);
                }
                if state == 4 || (state != 1 && Some(i) == first_bad) {
                    println!(
                        "[!!] GPU:   encoder {} state={} label={:?} draws={:?}",
                        i,
                        state,
                        label.map(|l| l.to_string()),
                        marks
                    );
                }
            }
        }
    }
    println!(
        "[!!] GPU: {} encoders were recorded; the first one that did not complete is {:?}",
        labels.len(),
        first_bad
    );
    // The GPU runs independent render passes concurrently and out of order, so "first encoder not completed" does not
    // single out the faulting pass. List every translated pass in the buffer and one full state line per distinct pair.
    println!("[!!] GPU: translated passes in the failed command buffer:");
    let mut seen_pairs: Vec<String> = Vec::new();
    for (i, l) in labels.iter().enumerate() {
        if !l.starts_with("translated pass") {
            continue;
        }
        println!("[!!] GPU:   encoder {} = {}", i, l);
        if let Some(pair) = l
            .split(" | ")
            .nth(1)
            .map(|p| p.split(" (").next().unwrap_or("").to_string())
        {
            if !seen_pairs.contains(&pair) {
                seen_pairs.push(pair);
                if let Some(d) = details.get(i).and_then(|d| d.first()) {
                    println!("[!!] GPU:     state: {}", d);
                }
            }
        }
    }
}

/// Waits for `cb` like waitUntilCompleted, but a command buffer still running after GPU_HANG_TIMEOUT means the GPU is
/// hung (a shader that never finishes, a runaway draw): left alone, macOS stalls the display and can freeze the whole
/// machine. Print what the command buffer contained and exit, which releases the GPU immediately.
fn wait_or_abort(cb: &ProtocolObject<dyn MTLCommandBuffer>, labels: &[String]) {
    const GPU_HANG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
    let start = std::time::Instant::now();
    loop {
        match cb.status() {
            MTLCommandBufferStatus::Completed | MTLCommandBufferStatus::Error => return,
            _ => {}
        }
        if start.elapsed() > GPU_HANG_TIMEOUT {
            println!("[!!] GPU HANG: a command buffer has run for {:?}; exiting to release the GPU. Its encoders:", start.elapsed());
            for (i, l) in labels.iter().enumerate() {
                println!(
                    "[!!]   encoder {}: {}",
                    i,
                    l.chars().take(600).collect::<String>()
                );
            }
            std::process::exit(4);
        }
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
}

/// nvnQueueSubmitCommands: execute the given command handles in order.
///
/// Safety net: at most one command buffer is in flight. A GPU hang ("progress timeout") makes macOS reset the GPU and, if
/// work keeps arriving, freeze the whole machine, so each submit first waits for the previous one and checks it. On the
/// first error the GPU is treated as broken, the details are printed, and no further rendering is submitted.
/// Frame-time accounting printed with the present rate: GPU execution time of the game's command buffers, and how long
/// `submit` waited for the previous one to finish (CPU blocked on GPU).
static GPU_BUSY_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Draws executed as compute dispatches instead of render passes (translated::try_tiny_compute).
/// Render / compute encoders the issuer began (translated draws' passes and the tiny compute passes; replay statistics).
pub(super) static ISSUED_PASSES: [std::sync::atomic::AtomicU64; 2] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 2];
/// Render targets that get ShaderWrite usage for tiny compute passes: any format up to 640x360 (larger passes stay render
/// passes anyway). ShaderWrite costs some formats their fast layout (~10-15% for 4-byte float formats, ~2x for 8- and
/// 16-byte ones on an M4), which only pays off for small targets.
fn tiny_shader_write(_pf: MTLPixelFormat, pixels: usize) -> bool {
    pixels <= 640 * 360
}

/// --log-phases: [plan ns, count] for draws of old lists / this frame's, then [encode ns, count] the same way.
pub(super) static PHASES: [std::sync::atomic::AtomicU64; 8] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 8];
/// --log-phases: encode_draw sections (ns): textures, raster state, vertex buffers, per-stage resources, pipeline.
pub(super) static PHASES2: [std::sync::atomic::AtomicU64; 5] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 5];
pub(super) fn phases_on() -> bool {
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| crate::settings::get().log.log_phases)
}
pub(super) static TINY_COMPUTE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SUBMIT_WAIT_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A committed command buffer and what is needed to retire it: its encoders' labels/details for error reports and the
/// occlusion reports it will produce.
pub(super) struct InFlight {
    serial: u64,
    /// The nvnQueueSubmitCommands call it belongs to (a submit may be split into several command buffers).
    group: u64,
    cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    labels: Vec<String>,
    details: Vec<Vec<String>>,
    occl: Vec<(Vec<usize>, u64)>,
    /// Reports the fence watcher writes when the command buffer completes; whichever of it and `retire_oldest` comes
    /// first writes them (taken out of the Option), so the serial is never marked complete before they are in memory.
    occl_watched: Option<std::sync::Arc<std::sync::Mutex<Option<Vec<(Vec<usize>, u64)>>>>>,
    /// TIMESTAMP reports to rewrite with GPU times (see gpu_timestamps); taken by whichever of the watcher and
    /// retire_oldest comes first.
    ts: std::sync::Arc<std::sync::Mutex<Option<Vec<(u64, bool)>>>>,
}

/// --gpu-timestamps: TIMESTAMP counter reports carry the time the GPU ran them, as on the Switch: the GPU start
/// time of their command buffer if no GPU work precedes them in it, else its end time (written at completion; the CPU
/// time at encode stays until then). Encode-time CPU times made TotK's per-frame GPU span (a start / end pair per frame,
/// read two frames later) our ~6-10 ms encode span, whatever the GPU took; its dynamic resolution never saw a slow GPU
/// (the iPhone: GPU ~26 ms per frame, frames just over 33 ms).
fn gpu_timestamps() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().graphics.gpu_timestamps)
}

/// Writes TIMESTAMP reports of a completed command buffer with its GPU start / end time (nanoseconds, host clock).
fn resolve_timestamps(cb: &ProtocolObject<dyn MTLCommandBuffer>, reports: &[(u64, bool)]) {
    let (start, end) = (cb.GPUStartTime(), cb.GPUEndTime());
    if !(end > 0.0 && start > 0.0) {
        return; // no GPU timing (error): the provisional CPU times stay
    }
    static LOG: OnceLock<bool> = OnceLock::new();
    let log = *LOG.get_or_init(|| crate::settings::get().log.log_timestamps);
    for &(addr, at_start) in reports {
        let t = if at_start { start } else { end };
        let v = (t * 1e9) as u64;
        if log {
            println!(
                "[ts-gpu] f{} addr {:#x} {} t {:.3} ms",
                presented_frames(),
                addr,
                if at_start { "start" } else { "end  " },
                v as f64 / 1e6
            );
        }
        let host = crate::nvn::api::translate_virtual(addr as usize);
        let p = if fence_guard::enabled() {
            fence_guard::engine_write_ptr(host)
        } else {
            report_guard::engine_ptr(host)
        } as *mut u64;
        unsafe {
            p.write_unaligned(v);
            p.add(1).write_unaligned(v);
        }
    }
}

/// Serial of the last committed command buffer and of the last one known complete (Metal runs a queue's command buffers
/// in order, so completion of serial n implies all earlier ones). nvnQueueFenceSync records the submitted serial and
/// nvnSyncWait waits for it.
static SUBMITTED_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static COMPLETED_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn completed_serial() -> u64 {
    COMPLETED_SERIAL.load(std::sync::atomic::Ordering::Acquire)
}

pub fn submitted_serial() -> u64 {
    SUBMITTED_SERIAL.load(std::sync::atomic::Ordering::Acquire)
}

/// Command buffers allowed on the GPU at once (--frames-in-flight, default 2, 1..=4). With one, the CPU encodes the
/// next frame only after the GPU finished the previous: frame time is CPU + GPU instead of the larger of the two.
pub fn frames_in_flight() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| crate::settings::get().graphics.frames_in_flight.clamp(1, 4))
}

/// Retires the oldest in-flight command buffer: waits for it (unless `only_if_done` and it is still running), then
/// publishes its occlusion reports and GPU time and handles errors. Returns whether one was retired.
fn retire_oldest(g: &Gfx, only_if_done: bool) -> bool {
    let entry = {
        let mut st = g.state.lock().unwrap();
        match st.inflight.front() {
            None => return false,
            Some(e)
                if only_if_done
                    && !matches!(
                        e.cb.status(),
                        MTLCommandBufferStatus::Completed | MTLCommandBufferStatus::Error
                    ) =>
            {
                return false
            }
            _ => {}
        }
        st.inflight.pop_front().unwrap()
    };
    let prev = entry.cb.clone();
    wait_or_abort(&prev, &entry.labels);
    if let Some(list) = entry.ts.lock().unwrap().take() {
        resolve_timestamps(&prev, &list);
    }
    if journal_on() && (!entry.occl.is_empty() || entry.occl_watched.is_some()) {
        let (s0, e0) = (prev.GPUStartTime(), prev.GPUEndTime());
        journal(|| {
            format!(
                "occl-cb {} gpu {:.3}-{:.3} retired t {:.3}",
                entry.serial,
                s0 * 1e3,
                e0 * 1e3,
                host_secs() * 1e3
            )
        });
    }
    g.state.lock().unwrap().occl.resolve(entry.occl);
    // Reports of a watched command buffer: written by the fence watcher, unless it has not got to them yet (the game's
    // fence wait must not see this serial complete before they are in memory).
    if let Some(shared) = &entry.occl_watched {
        let mut w = shared.lock().unwrap();
        if let Some(reports) = w.take() {
            let buf = g.state.lock().unwrap().occl.buf.clone();
            if let Some(buf) = buf {
                resolve_occlusion(&buf, &reports);
            }
        }
    }
    {
        // GPU busy time = union of the command buffers' GPU intervals (a frame is split into several command
        // buffers; the gaps between them are idle time, not work).
        let (start, end) = (prev.GPUStartTime(), prev.GPUEndTime());
        if end > start && end - start < 1.0 {
            static LAST_END: std::sync::Mutex<f64> = std::sync::Mutex::new(0.0);
            let mut last = LAST_END.lock().unwrap();
            let busy = end - start.max(*last);
            if busy > 0.0 {
                GPU_BUSY_US.fetch_add((busy * 1e6) as u64, std::sync::atomic::Ordering::Relaxed);
            }
            *last = last.max(end);
        }
        if prev.status() == MTLCommandBufferStatus::Error {
            let victim = prev.error().map_or(false, |e| {
                e.localizedDescription().to_string().contains("victim")
            });
            let mut st = g.state.lock().unwrap();
            if victim {
                // Discarded because the GPU was recovering from an error caused by something else (another process, or
                // a previous run that was just killed). Transient: skip this frame's work and carry on, but stop if it
                // keeps happening.
                st.victims_in_a_row += 1;
                println!("[!] GPU: a command buffer was discarded as an innocent victim of a GPU recovery (#{} in a row); continuing", st.victims_in_a_row);
                if st.victims_in_a_row >= 8 {
                    st.gpu_halted = true;
                    println!("[!!] GPU: rendering halted: the GPU keeps discarding work");
                }
            } else {
                let (labels, details) = (entry.labels.clone(), entry.details.clone());
                drop(st);
                report_gpu_error(&prev, &labels, &details);
                g.state.lock().unwrap().gpu_halted = true;
                println!("[!!] GPU: rendering halted to protect the system; the game keeps running without drawing");
            }
        } else {
            g.state.lock().unwrap().victims_in_a_row = 0;
        }
    }
    COMPLETED_SERIAL.fetch_max(entry.serial, std::sync::atomic::Ordering::AcqRel);
    true
}

/// Encoders per Metal command buffer before `maybe_split` commits it (--split-encoders, default 24; 0 = one
/// command buffer per submit).
/// --split-draws=N (default 600, 0 = off): end a render encoder after N draws and commit, continuing in a new encoder
/// that loads the targets. The GPU cannot start an encoder before it ends and its command buffer is committed: TotK's
/// G-buffer pass (~1000 draws, ~10 ms to encode) ran entirely after it was encoded, in the GPU tail the game waits for.
/// Each cut stores and reloads every attachment (~50 MB for the G-buffer): 600 beat 250 in GPU-bound Hyrule Field
/// (42.6 / 38.2 vs 37.7 / 36.0 fps) and matched it in Hateno; 1500 left a ~13 ms GPU tail.
/// Re-measured 2026-10-09 in Hateno (feedback in place, GPU ~10.8 ms): 150 / 300 / 600 / 1200 gave tail 3.7 / 3.8 / 4.4 /
/// 4.9 ms, total GPU 12.4 / 11.6 / 10.8 / 10.6 ms, 57.9 / 58.4 / 57.4 / 56.9 fps; 0 = a 21 ms tail. 300 wins ~1 fps where
/// encode dominates but costs GPU time, which GPU-bound scenes pay: 600 kept.
pub(super) fn split_draws() -> u32 {
    let s = crate::settings::get();
    // Profiling: --split-draws-from=F gives every draw its own encoder from presented frame F on, so a GPU trace times
    // draws individually (each also pays a pass's load / store) without the slow-down before reaching the scene.
    if s.graphics
        .split_draws_from
        .is_some_and(|f| presented_frames() >= f)
    {
        return 1;
    }
    // --replay-ab-split=N: replay side B cuts every N draws instead (same-frame A/B of the cut's GPU cost).
    match s.replay.replay_ab_split {
        Some(b) if replay_b() => b,
        _ => s.graphics.split_draws,
    }
}

/// --split-ms=T (default 1.0, 0 = off): commit the command buffer between passes once T ms were spent encoding it.
fn split_ms() -> f64 {
    static T: OnceLock<f64> = OnceLock::new();
    *T.get_or_init(|| crate::settings::get().graphics.split_ms)
}

fn split_encoders() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| crate::settings::get().graphics.split_encoders)
}

/// Records a just-committed command buffer as in flight, with the encoder labels/details and occlusion reports
/// encoded into it.
fn push_inflight(st: &mut State, cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>) -> u64 {
    let serial = SUBMITTED_SERIAL.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1;
    // Occlusion reports are written as soon as the command buffer completes (on the fence watcher thread) instead of
    // when the executor later retires it: with --early-fence the game reads them before its next submit, and a
    // report still unwritten then read as stale memory (an object that just came into range or switched LOD was culled
    // for a frame: Hateno signposts, 2026-10-13). Retiring then skips them: it runs later, and rewriting a report the
    // game reuses next frame would put the older value back over the newer one.
    let mut occl = std::mem::take(&mut st.occl.pending);
    if journal_on() && !occl.is_empty() {
        journal(|| {
            format!(
                "occl-cb {} committed with {} reports t {:.3}",
                serial,
                occl.len(),
                host_secs() * 1e3
            )
        });
    }
    let mut occl_watched = None;
    let ts_list = std::mem::take(&mut st.ts_pending);
    let ts = std::sync::Arc::new(std::sync::Mutex::new(
        (!ts_list.is_empty()).then_some(ts_list),
    ));
    struct SendBuf(Retained<ProtocolObject<dyn MTLBuffer>>);
    unsafe impl Send for SendBuf {}
    struct SendCb(Retained<ProtocolObject<dyn MTLCommandBuffer>>);
    unsafe impl Send for SendCb {}
    let occl_part = match (&st.occl.buf, occl.is_empty()) {
        (Some(buf), false) => {
            let shared = std::sync::Arc::new(std::sync::Mutex::new(Some(occl.clone())));
            occl_watched = Some(shared.clone());
            Some((SendBuf(buf.clone()), shared))
        }
        _ => None,
    };
    let has_ts = ts.lock().unwrap().is_some();
    let on_done: Option<Box<dyn FnOnce()>> = if occl_part.is_some() || has_ts {
        let ts = ts.clone();
        let cbs = SendCb(cb.clone());
        Some(Box::new(move || {
            let (occl_part, cbs) = (occl_part, cbs);
            if let Some((buf, shared)) = occl_part {
                let mut g = shared.lock().unwrap();
                if let Some(reports) = g.take() {
                    resolve_occlusion(&buf.0, &reports);
                }
            }
            if let Some(list) = ts.lock().unwrap().take() {
                resolve_timestamps(&cbs.0, &list);
            }
        }))
    } else {
        None
    };
    if fence_guard::watch(serial, &cb, on_done) {
        occl.clear();
    } else {
        occl_watched = None;
    }
    let entry = InFlight {
        serial,
        group: st.stats.submits,
        cb,
        labels: std::mem::take(&mut st.lb.enc_labels),
        details: std::mem::take(&mut st.lb.enc_details),
        occl,
        occl_watched,
        ts,
    };
    st.inflight.push_back(entry);
    serial
}

/// Submits with command buffers still in flight.
fn groups_in_flight(st: &State) -> usize {
    match (st.inflight.front(), st.inflight.back()) {
        (Some(a), Some(b)) => (b.group - a.group + 1) as usize,
        _ => 0,
    }
}

/// Last in-flight serial the issuer committed: all work up to it is encoded and handed to the GPU.
static COMMITTED_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Highest command buffer serial (possibly not yet assigned) with GPU writes the game can read: storage buffers, copy
/// destinations in guest memory, occlusion reports (written when it completes). --early-fence waits fully while one
/// is pending. Encoding stages that lag the plan thread may record a later serial than the right one (a longer wait).
static GPU_GUEST_WRITE_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub(super) enum GuestWrite {
    Storage,
    Copy,
    Occlusion,
}

/// Records GPU work that writes guest memory (results the game can read after the frame's fence) into the command
/// buffer being encoded. --early-fence does not wait for it: TotK reads such results right after each frame fence
/// (occlusion report slots; small compute storage buffers, e.g. s_e7935105553f7e48's 0x160-byte outputs, read by
/// main+0x10c4xxx), so it gets them one frame late, as emulators' asynchronous GPU modes do. --early-fence-strict
/// falls back to a full wait while any is pending: correct, but in Hateno play every frame has some (one buffer copy,
/// ~77 occlusion reports, compute storage), so it gains nothing there. --early-fence-trap traps their pages.
pub(super) fn note_gpu_guest_write(kind: GuestWrite, addr: u64, len: usize) {
    if !early_fence() {
        return;
    }
    if early_fence_trap() && (!trap_occl_only() || matches!(kind, GuestWrite::Occlusion)) {
        let start = crate::nvn::api::translate_virtual(addr as usize);
        if start >= 0x10000 && len > 0 {
            TRAP_PENDING.lock().unwrap().push((
                start & !0x3fff,
                (start + len + 0x3fff) & !0x3fff,
                submitted_serial() + 1,
            ));
            TRAP_ORIGINS.lock().unwrap().push((
                start,
                start + len,
                addr,
                len,
                submitted_serial() + 1,
            ));
        }
    }
    static STRICT: OnceLock<bool> = OnceLock::new();
    if !*STRICT.get_or_init(|| crate::settings::get().graphics.early_fence_strict) {
        return;
    }
    GUEST_WRITE_KINDS[kind as usize].fetch_add(1, Ordering::Relaxed);
    GPU_GUEST_WRITE_SERIAL.fetch_max(submitted_serial() + 1, Ordering::AcqRel);
}

/// GPU writes the strict mode waits for, per kind (storage, copy, occlusion), for --log-early-fence.
static GUEST_WRITE_KINDS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

/// --early-fence-trap (diagnostic, with --early-fence): when a fence returns early, the pages of guest memory
/// that still-running command buffers write (storage buffers, copy destinations, occlusion reports) are made
/// inaccessible; the first access to each logs its PC (guest main+offset, or a host PC with callers), read / write,
/// whether the GPU work was still running ("RACE") and the bindings on the page (* = the access hit one), then the
/// page is unprotected without waiting. Showed which game code reads GPU results early (see note_gpu_guest_write).
/// Pages shared with write-tracked textures lose their tracking: diagnostic runs only.
fn early_fence_trap() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        crate::settings::get().debug.early_fence_trap != crate::settings::FenceTrap::Off
    })
}

/// --early-fence-trap=occl: trap only occlusion report pages.
fn trap_occl_only() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        crate::settings::get().debug.early_fence_trap == crate::settings::FenceTrap::Occl
    })
}

/// Storage-buffer ranges (page-aligned start, end, serial) bound since the last early fence.
static TRAP_PENDING: std::sync::Mutex<Vec<(usize, usize, u64)>> = std::sync::Mutex::new(Vec::new());
/// The bindings behind them: (host start, host end, guest address, length, serial), for the hit report.
static TRAP_ORIGINS: std::sync::Mutex<Vec<(usize, usize, u64, usize, u64)>> =
    std::sync::Mutex::new(Vec::new());
const TRAP_SLOTS: usize = 512;
/// Protected ranges, searched lock-free by the signal handler (start, end; 0 = free).
static TRAPPED: [(AtomicUsize, AtomicUsize); TRAP_SLOTS] =
    [const { (AtomicUsize::new(0), AtomicUsize::new(0)) }; TRAP_SLOTS];
/// Per slot: the last serial whose storage bindings it covers (a hit after it completed is no race).
static TRAPPED_SERIAL: [AtomicU64; TRAP_SLOTS] = [const { AtomicU64::new(0) }; TRAP_SLOTS];
static TRAP_RACE: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];
/// Per slot: already hit since it was protected (only the first access is logged).
static TRAPPED_HIT: [AtomicUsize; TRAP_SLOTS] = [const { AtomicUsize::new(0) }; TRAP_SLOTS];
/// The previous generation's ranges (unprotected again): a fault delivered late for one of them is retried.
static TRAPPED_OLD: [(AtomicUsize, AtomicUsize); TRAP_SLOTS] =
    [const { (AtomicUsize::new(0), AtomicUsize::new(0)) }; TRAP_SLOTS];
/// Faults seen: (guest pc, address, write).
static TRAP_HITS: [(AtomicUsize, AtomicUsize, AtomicUsize); 256] = [const {
    (
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    )
}; 256];
/// Per hit: lr and the next return addresses from the frame-pointer chain (host callers of a host faulting PC).
static TRAP_CALLERS: [[AtomicUsize; 4]; 256] = [const { [const { AtomicUsize::new(0) }; 4] }; 256];
static TRAP_HIT_N: AtomicUsize = AtomicUsize::new(0);

/// Signal-handler hook for --early-fence-trap: true if `addr` was in a trapped range (now accessible again).
pub(super) unsafe fn early_fence_fault(
    addr: usize,
    pc: usize,
    write: bool,
    lr: usize,
    fp: usize,
) -> bool {
    for (slot, (s, e)) in TRAPPED.iter().enumerate() {
        let start = s.load(Ordering::Acquire);
        if start == 0 || addr < start || addr >= e.load(Ordering::Acquire) {
            continue;
        }
        let end = e.load(Ordering::Acquire);
        let race =
            COMPLETED_SERIAL.load(Ordering::Acquire) < TRAPPED_SERIAL[slot].load(Ordering::Acquire);
        // The slot stays until the next retrap: other threads faulting on it before this mprotect still find it.
        libc::mprotect(
            start as *mut libc::c_void,
            end - start,
            libc::PROT_READ | libc::PROT_WRITE,
        );
        if TRAPPED_HIT[slot].swap(1, Ordering::AcqRel) != 0 {
            return true;
        }
        let i = TRAP_HIT_N.fetch_add(1, Ordering::AcqRel);
        if i < TRAP_HITS.len() {
            TRAP_HITS[i].0.store(pc, Ordering::Relaxed);
            TRAP_HITS[i].1.store(addr, Ordering::Relaxed);
            TRAP_RACE[i].store(race as usize, Ordering::Relaxed);
            TRAP_CALLERS[i][0].store(lr, Ordering::Relaxed);
            let mut f = fp;
            for k in 1..4 {
                if f < 0x10000 || f & 7 != 0 {
                    break;
                }
                TRAP_CALLERS[i][k].store(*((f + 8) as *const usize), Ordering::Relaxed);
                f = *(f as *const usize);
            }
            TRAP_HITS[i].2.store(write as usize, Ordering::Release);
        }
        return true;
    }
    // Unprotected by a retrap between the fault and this handler: retry the access.
    TRAPPED_OLD.iter().any(|(s, e)| {
        let start = s.load(Ordering::Acquire);
        start != 0 && addr >= start && addr < e.load(Ordering::Acquire)
    })
}

extern "C" {
    fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
}

/// At an early fence return: unprotect the previous traps (their work completed), print the hits, protect the
/// storage ranges of command buffers up to `serial` that are still running.
fn early_fence_retrap(serial: u64) {
    for ((s, e), (os, oe)) in TRAPPED.iter().zip(TRAPPED_OLD.iter()) {
        let start = s.load(Ordering::Acquire);
        os.store(0, Ordering::Release);
        if start != 0 {
            let end = e.load(Ordering::Acquire);
            oe.store(end, Ordering::Release);
            os.store(start, Ordering::Release);
            unsafe {
                libc::mprotect(
                    start as *mut libc::c_void,
                    end - start,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
            s.store(0, Ordering::Release);
        }
    }
    let n = TRAP_HIT_N.swap(0, Ordering::AcqRel).min(TRAP_HITS.len());
    let base = crate::cpu::crash_handler::get_guest_base();
    let slide = unsafe { _dyld_get_image_vmaddr_slide(0) } as usize;
    for ((h, c), race) in TRAP_HITS[..n]
        .iter()
        .zip(TRAP_CALLERS.iter())
        .zip(TRAP_RACE.iter())
    {
        let pc = h.0.load(Ordering::Acquire);
        let rel = if pc >= base && pc < base + (256 << 20) {
            format!("main+{:#x}", pc - base)
        } else {
            format!("{:#x}", pc)
        };
        let callers: Vec<String> = c
            .iter()
            .map(|a| format!("{:#x}", a.swap(0, Ordering::Relaxed)))
            .collect();
        let hit = h.1.load(Ordering::Relaxed);
        let origins: Vec<String> = TRAP_ORIGINS
            .lock()
            .unwrap()
            .iter()
            .filter(|o| hit >= o.0 & !0x3fff && hit < (o.1 + 0x3fff) & !0x3fff)
            .map(|o| {
                format!(
                    "{:#x}+{:#x}{}",
                    o.2,
                    o.3,
                    if hit >= o.0 && hit < o.1 { "*" } else { "" }
                )
            })
            .collect();
        println!("[ef-trap]   bindings on that page: {}", origins.join(" "));
        println!(
            "[ef-trap] f{} {} {} {} {:#x} callers {} slide {:#x}",
            presented_frames(),
            if race.load(Ordering::Relaxed) != 0 {
                "RACE"
            } else {
                "late"
            },
            if h.2.load(Ordering::Acquire) != 0 {
                "write"
            } else {
                "read "
            },
            rel,
            h.1.load(Ordering::Relaxed),
            callers.join(","),
            slide
        );
    }
    let completed = COMPLETED_SERIAL.load(Ordering::Acquire);
    TRAP_ORIGINS.lock().unwrap().retain(|o| o.4 > completed);
    let mut pending = TRAP_PENDING.lock().unwrap();
    pending.retain(|r| r.2 > completed);
    let mut now: Vec<(usize, usize, u64)> =
        pending.iter().filter(|r| r.2 <= serial).copied().collect();
    pending.retain(|r| r.2 > serial);
    drop(pending);
    now.sort_unstable();
    let mut merged: Vec<(usize, usize, u64)> = Vec::new();
    for (a, b, sr) in now {
        match merged.last_mut() {
            Some(m) if a <= m.1 => {
                m.1 = m.1.max(b);
                m.2 = m.2.max(sr);
            }
            _ => merged.push((a, b, sr)),
        }
    }
    for ((((a, b, sr), slot), ss), hit) in merged
        .iter()
        .zip(TRAPPED.iter())
        .zip(TRAPPED_SERIAL.iter())
        .zip(TRAPPED_HIT.iter())
    {
        ss.store(*sr, Ordering::Release);
        hit.store(0, Ordering::Release);
        slot.1.store(*b, Ordering::Release);
        slot.0.store(*a, Ordering::Release);
        unsafe { libc::mprotect(*a as *mut libc::c_void, b - a, libc::PROT_NONE) };
    }
}

/// Crash reports: what the page guards know about `addr`'s page and its actual protection.
pub(super) fn describe_fault_page(addr: usize) -> String {
    extern "C" {
        static mach_task_self_: u32;
        fn mach_vm_region(
            task: u32,
            address: *mut u64,
            size: *mut u64,
            flavor: i32,
            info: *mut i32,
            count: *mut u32,
            object: *mut u32,
        ) -> i32;
    }
    let (mut a, mut size, mut count, mut obj) = (addr as u64, 0u64, 9u32, 0u32);
    let mut info = [0i32; 9]; // vm_region_basic_info_64: protection, max_protection, ...
    let kr = unsafe {
        mach_vm_region(
            mach_task_self_,
            &mut a,
            &mut size,
            9, /* VM_REGION_BASIC_INFO_64 */
            info.as_mut_ptr(),
            &mut count,
            &mut obj,
        )
    };
    let prot = if kr == 0 && a <= addr as u64 {
        format!("prot {} max {}", info[0], info[1])
    } else {
        format!("region query {}", kr)
    };
    format!(
        "{}; report guard: {}; texture backing: {}; write guard holds {}",
        prot,
        report_guard::describe(addr),
        tex_backing::describe(addr),
        fence_guard::holds(addr & !0x3fff)
    )
}

/// The address range of guest code (--report-guard: only accesses made by guest code wait for reports).
pub(crate) fn set_guest_code_range(start: usize, end: usize) {
    report_guard::set_guest_code_range(start, end);
}

pub(super) fn note_committed(serial: u64) {
    COMMITTED_SERIAL.fetch_max(serial, std::sync::atomic::Ordering::AcqRel);
}

/// --early-fence (on by default since 2026-10-10, =0 disables; changes behaviour): guest-main's nvnSyncWait returns once the frame's
/// work is committed instead of when the GPU has finished it, so the game's next frame overlaps the GPU's work on this
/// one (TotK waits on each frame's fence before starting the next); at most one frame overlaps. The game then reads GPU
/// results a frame late (occlusion reports, compute outputs: see note_gpu_guest_write) and may write data the GPU is
/// still reading (seen in the opening). The engine's own readers wait or keep GPU order: render-target write-back
/// waits for its targets, uniform buffers in memory GPU work of earlier submits writes stay zero-copy until it
/// completed (State::carried_dirty).
/// Without it the GPU idles between frames and macOS lowers its clock: walking in Hateno 2026-10-14, GPU ~29 ms per frame
/// instead of ~13 and ~26 fps instead of ~65-70. Each GPU result the game reads right after its fence still needs a
/// special case until guest memory the GPU writes is kept coherent for CPU reads in general.
pub(crate) fn early_fence() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().graphics.early_fence)
}

fn on_guest_main() -> bool {
    thread_local! {
        static MAIN: bool = std::thread::current().name() == Some("guest-main");
    }
    MAIN.with(|m| *m)
}

/// The game's fence wait: GPU completion of `serial`, or with --early-fence its commit.
pub fn wait_fence(serial: u64, timeout_ns: u64) -> bool {
    if journal_on() && on_guest_main() {
        let t0 = host_secs() * 1e3;
        let ok = wait_fence_inner(serial, timeout_ns);
        journal(|| format!("fence-wait {} t {:.3}-{:.3}", serial, t0, host_secs() * 1e3));
        return ok;
    }
    wait_fence_inner(serial, timeout_ns)
}

fn wait_fence_inner(serial: u64, timeout_ns: u64) -> bool {
    // Only the frame loop's wait (guest-main): other threads wait for GPU work whose results they read back (loading
    // stalled when their fences returned early).
    if !early_fence() || !on_guest_main() {
        return wait_serial(serial, timeout_ns);
    }
    // The previous fence this game waited on must have completed on the GPU: at most one frame overlaps.
    static PREV: AtomicU64 = AtomicU64::new(0);
    let prev = PREV.load(Ordering::Acquire);
    if prev < serial && !wait_serial(prev, timeout_ns) {
        return false;
    }
    if serial > prev {
        PREV.store(serial, Ordering::Release);
    }
    // GPU results the game may read after this fence (storage buffers, copies, occlusion reports) are not there yet.
    static N: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
    let full =
        GPU_GUEST_WRITE_SERIAL.load(Ordering::Acquire) > COMPLETED_SERIAL.load(Ordering::Acquire);
    N[full as usize].fetch_add(1, Ordering::Relaxed);
    let (early, fulls) = (N[0].load(Ordering::Relaxed), N[1].load(Ordering::Relaxed));
    if (early + fulls) % 600 == 0 && crate::settings::get().log.log_early_fence {
        let k: Vec<u64> = GUEST_WRITE_KINDS
            .iter()
            .map(|k| k.load(Ordering::Relaxed))
            .collect();
        println!("[early-fence] waits returned early {} / full {} (f{}); forcing writes: storage {} copy {} occlusion {}", early, fulls,
            presented_frames(), k[0], k[1], k[2]);
    }
    if full {
        let ok = wait_serial(serial, timeout_ns);
        if early_fence_trap() {
            early_fence_retrap(serial);
        }
        fence_guard::release_all();
        return ok;
    }
    let deadline =
        std::time::Instant::now().checked_add(std::time::Duration::from_nanos(timeout_ns));
    let trap = early_fence_trap();
    while COMMITTED_SERIAL.load(std::sync::atomic::Ordering::Acquire) < serial
        && COMPLETED_SERIAL.load(std::sync::atomic::Ordering::Acquire) < serial
    {
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_micros(50));
    }
    if trap {
        early_fence_retrap(serial);
    }
    // Pages the still-running work reads become read-only: a game write to one waits for it (metal/fence_guard.rs).
    fence_guard::protect(serial);
    true
}

/// Blocks until command buffer `serial` has completed, or `timeout_ns` passed (false).
pub fn wait_serial(serial: u64, timeout_ns: u64) -> bool {
    let Some(g) = GFX.get() else { return true };
    let deadline =
        std::time::Instant::now().checked_add(std::time::Duration::from_nanos(timeout_ns));
    while COMPLETED_SERIAL.load(std::sync::atomic::Ordering::Acquire) < serial {
        if retire_oldest(g, true) {
            continue;
        }
        if g.state.lock().unwrap().inflight.is_empty() {
            return true; // nothing left on the GPU (e.g. rendering halted after an error)
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }
    true
}

/// Frame timeline (--log-timeline, printed with the fps line): encode time in submit(), guest time blocked in
/// nvnSyncWait, GPU tail after the frame's last submit, presentation-thread gap between submits.
pub(crate) static TL_ENCODE_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// CPU time of the encoding thread during those submits (the rest of TL_ENCODE_US is waiting or preemption).
static TL_ENCODE_CPU_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// This thread's CPU time.
pub(crate) fn thread_cpu_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
pub(crate) static TL_SYNC_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static TL_TAIL_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// --log-tail: every 50th frame, each committed command buffer's commit time and GPU start / end (one clock,
/// ms from the frame's first commit), to see whether the GPU tail is the last chunk's work or a backlog.
fn tail_log_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().log.log_tail)
}

/// Command buffers are thread-safe Metal objects; only these debug lists hold them across threads.
struct TailCbs(Vec<(Retained<ProtocolObject<dyn MTLCommandBuffer>>, f64)>);
unsafe impl Send for TailCbs {}
static TAIL_CBS: Mutex<TailCbs> = Mutex::new(TailCbs(Vec::new()));
static TAIL_PREV: Mutex<Option<(u64, TailCbs)>> = Mutex::new(None);

fn host_secs() -> f64 {
    crate::clock::now_secs()
}

/// Names a command buffer after the encoders it holds (--log-tail).
fn tail_log_label(st: &State, cb: &ProtocolObject<dyn MTLCommandBuffer>) {
    if !tail_log_enabled() {
        return;
    }
    let short = |l: &String| {
        l.split(" | ")
            .next()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect::<String>()
    };
    let label = match (st.lb.enc_labels.first(), st.lb.enc_labels.last()) {
        (Some(a), Some(b)) => format!(
            "{} enc: {} .. {}",
            st.lb.enc_labels.len(),
            short(a),
            short(b)
        ),
        _ => "empty".to_string(),
    };
    cb.setLabel(Some(&objc2_foundation::NSString::from_str(&label)));
}

pub(super) fn tail_log_commit(cb: &Retained<ProtocolObject<dyn MTLCommandBuffer>>) {
    if tail_log_enabled() {
        TAIL_CBS.lock().unwrap().0.push((cb.clone(), host_secs()));
    }
}

/// At a present: the previous logged frame's command buffers have completed by now (TotK waited for them).
fn tail_log_present(frame: u64) {
    if !tail_log_enabled() {
        return;
    }
    let cbs = std::mem::replace(&mut *TAIL_CBS.lock().unwrap(), TailCbs(Vec::new()));
    let prev = TAIL_PREV.lock().unwrap().replace((frame, cbs));
    let Some((f, TailCbs(cbs))) = prev else {
        return;
    };
    if f % 50 != 0 || cbs.is_empty() {
        return;
    }
    let t0 = cbs[0].1;
    let ms = |t: f64| (t - t0) * 1e3;
    let mut line = format!("[tail] f{} {} cbs:", f, cbs.len());
    let mut gpu_busy = 0.0;
    let mut last_end = t0;
    for (cb, c) in &cbs {
        let (gs, ge) = (cb.GPUStartTime(), cb.GPUEndTime());
        gpu_busy += (ge - gs).max(0.0);
        last_end = last_end.max(ge);
        line.push_str(&format!(
            "\n    c{:5.1} g{:5.1}-{:5.1} ({:4.2} ms) {}",
            ms(*c),
            ms(gs),
            ms(ge),
            (ge - gs) * 1e3,
            cb.label().map(|l| l.to_string()).unwrap_or_default()
        ));
    }
    let last_commit = cbs.last().map(|x| x.1).unwrap_or(t0);
    let backlog: f64 = cbs
        .iter()
        .map(|(cb, _)| {
            (cb.GPUEndTime().min(f64::MAX) - cb.GPUStartTime().max(last_commit)).max(0.0)
        })
        .sum();
    line.push_str(&format!(" | GPU busy {:.1} ms, last commit {:.1}, GPU done {:.1} (tail {:.1}, of which GPU work after last commit {:.1})",
        gpu_busy * 1e3, ms(last_commit), ms(last_end), (last_end - last_commit) * 1e3, backlog * 1e3));
    println!("{}", line);
}
pub(crate) static TL_GAP_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static TL_LAST_SUBMIT_END: Mutex<Option<std::time::Instant>> = Mutex::new(None);
pub(crate) fn timeline_enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| crate::settings::get().log.log_timeline)
}
/// Microseconds since the last submit ended (for the GPU tail measured by nvnSyncWait).
pub(crate) fn since_last_submit_us() -> Option<u64> {
    TL_LAST_SUBMIT_END
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_micros() as u64)
}

/// iOS refuses GPU work from an app that is not active (the command buffer fails, which halts rendering for good):
/// host/display_ios.rs sets this while the app is inactive, and submits and presents wait for it to clear, stalling the
/// guest's presentation thread like a paused console.
pub static SUSPENDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn wait_while_suspended() {
    while SUSPENDED.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// This frame's submits (their command list handles), for --replay.
static FRAME_SUBMITS: Mutex<Vec<Vec<u64>>> = Mutex::new(Vec::new());
/// Set while a replay runs: nvnSyncWait holds the game's main thread so the frame's memory stays as it was.
static REPLAY_HOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// --replay-ab: replay repetitions alternate this flag (A = false, B = true) and report both sides, so a change
/// guarded by `replay_b()` is measured on the very same frame. Always false outside such a replay.
static REPLAY_B: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[inline]
pub fn replay_b() -> bool {
    REPLAY_B.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn replay_hold() -> bool {
    REPLAY_HOLD.load(std::sync::atomic::Ordering::Acquire)
}

/// --replay=F:K[:M]: at presented frame F (and every M frames after it) the frame's submits are executed K more times
/// before the present, while the game's main thread waits in nvnSyncWait. Each repetition prints the encode time (CPU,
/// this thread) and the drain time (until the GPU has finished it): identical work, so their minimum / median compare
/// builds and runtime toggles without run-to-run scene noise. Side effects: the frame renders K extra times (counters
/// and occlusion results are written again).
fn replay_spec() -> Option<(u64, u32, u64)> {
    crate::settings::get()
        .replay
        .replay
        .map(|r| (r.frame, r.reps, r.every))
}

fn replay_frame(g: &Gfx, frame: u64, reps: u32, submits: &[Vec<u64>], presented: usize) {
    let ab = crate::settings::get().replay.replay_ab;
    let mut enc = [Vec::new(), Vec::new()];
    let mut drain = [Vec::new(), Vec::new()];
    let mut cpu = [Vec::new(), Vec::new()];
    let mut gpu = [Vec::new(), Vec::new()];
    let mut threads: [Vec<[u64; 3]>; 2] = [Vec::new(), Vec::new()];
    let mut passes: [Vec<[u64; 6]>; 2] = [Vec::new(), Vec::new()];
    // --replay-diff=N: compare each repetition's presented image with the previous one of the same side. The frame's
    // inputs are frozen, so differences beyond GPU-particle noise are engine races; repetitions with more than N
    // differing pixels (default 2000) are dumped with their predecessor.
    let diff_limit: Option<usize> = crate::settings::get()
        .replay
        .replay_diff
        .map(|n| n as usize);
    let mut prev_img: [Option<Vec<u8>>; 2] = [None, None];
    let mut diff_counts: Vec<usize> = Vec::new();
    for i in 0..reps * if ab { 2 } else { 1 } {
        let side = (ab && i % 2 == 1) as usize;
        REPLAY_B.store(side == 1, std::sync::atomic::Ordering::Relaxed);
        while retire_oldest(g, true) {}
        let busy0 = GPU_BUSY_US.load(std::sync::atomic::Ordering::Relaxed);
        let th0 = crate::host::thread_stats::engine_cpu_us();
        let cnt = || {
            [
                ISSUED_PASSES[0].load(std::sync::atomic::Ordering::Relaxed),
                ISSUED_PASSES[1].load(std::sync::atomic::Ordering::Relaxed),
                TINY_COMPUTE.load(std::sync::atomic::Ordering::Relaxed),
                CLEAR_PASSES.load(std::sync::atomic::Ordering::Relaxed),
                batch::STATS[0].load(std::sync::atomic::Ordering::Relaxed),
                batch::STATS[1].load(std::sync::atomic::Ordering::Relaxed),
            ]
        };
        let n0 = cnt();
        let ph0: [u64; 8] =
            std::array::from_fn(|k| PHASES[k].load(std::sync::atomic::Ordering::Relaxed));
        let pq0: [u64; 5] =
            std::array::from_fn(|k| PHASES2[k].load(std::sync::atomic::Ordering::Relaxed));
        let t0 = std::time::Instant::now();
        let c0 = thread_cpu_ns();
        for handles in submits {
            submit_inner(handles);
        }
        let t1 = std::time::Instant::now();
        cpu[side].push((thread_cpu_ns() - c0) as f64 / 1e6);
        // Drain: a marker command buffer committed after everything the issuer queued.
        let marker = {
            let st = g.state.lock().unwrap();
            let Some(m) = g.queue.commandBuffer() else {
                return;
            };
            st.issuer.commit(m.clone());
            st.issuer.sync();
            m
        };
        marker.waitUntilCompleted();
        // GPU busy time of this repetition: its command buffers are complete, retiring them adds their GPU intervals.
        while retire_oldest(g, true) {}
        gpu[side].push(
            GPU_BUSY_US
                .load(std::sync::atomic::Ordering::Relaxed)
                .saturating_sub(busy0) as f64
                / 1e3,
        );
        let th1 = crate::host::thread_stats::engine_cpu_us();
        threads[side].push([0, 1, 2].map(|k| th1[k].saturating_sub(th0[k])));
        let n1 = cnt();
        passes[side].push([0, 1, 2, 3, 4, 5].map(|k| n1[k] - n0[k]));
        if crate::settings::get().log.log_phases && i + 1 == reps * if ab { 2 } else { 1 } {
            let ph: [u64; 8] = std::array::from_fn(|k| {
                PHASES[k].load(std::sync::atomic::Ordering::Relaxed) - ph0[k]
            });
            let us = |a: u64, n: u64| a as f64 / n.max(1) as f64 / 1e3;
            println!("[phases] frame {}: plan: old-list draws {} x {:.2} us = {:.2} ms, this frame's {} x {:.2} us = {:.2} ms; encode: old {} x {:.2} us = {:.2} ms, this frame's {} x {:.2} us = {:.2} ms",
                frame, ph[1], us(ph[0], ph[1]), ph[0] as f64 / 1e6, ph[3], us(ph[2], ph[3]), ph[2] as f64 / 1e6,
                ph[5], us(ph[4], ph[5]), ph[4] as f64 / 1e6, ph[7], us(ph[6], ph[7]), ph[6] as f64 / 1e6);
            let pq: [u64; 5] = std::array::from_fn(|k| {
                PHASES2[k].load(std::sync::atomic::Ordering::Relaxed) - pq0[k]
            });
            println!("[phases] frame {}: encode sections per frame (ms): pipeline + layout {:.2}, textures + raster state {:.2}, vertex buffers {:.2}, per-stage resources {:.2} (then the draw)",
                frame, pq[4] as f64 / 1e6, pq[0] as f64 / 1e6, pq[1] as f64 / 1e6, pq[2] as f64 / 1e6);
        }
        if let Some(limit) = diff_limit {
            let mut st = g.state.lock().unwrap();
            if let Some(t) = unsafe { g.texture(&mut st.tc, presented) } {
                let (w, h) = (t.width(), t.height());
                let mut raw = vec![0u8; w * h * 4];
                let region = MTLRegion {
                    origin: MTLOrigin { x: 0, y: 0, z: 0 },
                    size: MTLSize {
                        width: w,
                        height: h,
                        depth: 1,
                    },
                };
                unsafe {
                    t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                        std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                        w * 4,
                        region,
                        0,
                    )
                };
                if let Some(prev) = &prev_img[side] {
                    let (mut n, mut x0, mut y0, mut x1, mut y1) = (0usize, w, h, 0, 0);
                    for (k, (a, b)) in raw.chunks_exact(4).zip(prev.chunks_exact(4)).enumerate() {
                        if (0..3).any(|c| a[c].abs_diff(b[c]) > 8) {
                            n += 1;
                            let (x, y) = (k % w, k / w);
                            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                        }
                    }
                    diff_counts.push(n);
                    if n > limit {
                        println!("[replay-diff] frame {} rep {} side {}: {} pixels differ from the previous repetition, bbox ({},{})-({},{})",
                            frame, i, ["A", "B"][side], n, x0, y0, x1, y1);
                        dump_ppm(&t, &format!("/tmp/nxport_replay_{}_r{}.ppm", frame, i));
                        let _ = std::fs::write(
                            format!("/tmp/nxport_replay_{}_r{}_prev.raw", frame, i),
                            prev,
                        );
                    }
                }
                prev_img[side] = Some(raw);
            }
        }
        // --replay-dump: the image each side rendered last (same frame and memory: a deterministic A/B of output).
        // --replay-dump=2: also side A's repetition before (_a2): the same frame's noise floor between two runs of one path.
        let total = reps * if ab { 2 } else { 1 };
        let last = i + 1 >= total - if ab { 1 } else { 0 };
        let dump = crate::settings::get().replay.replay_dump;
        let second_last_a = dump == 2 && ab && i + 4 == total && side == 0;
        if (last || second_last_a) && dump > 0 {
            let mut st = g.state.lock().unwrap();
            if let Some(t) = unsafe { g.texture(&mut st.tc, presented) } {
                let tag = if second_last_a && !last {
                    "a2"
                } else {
                    ["a", "b"][side]
                };
                let path = format!("/tmp/nxport_replay_{}_{}.ppm", frame, tag);
                dump_ppm(&t, &path);
                println!(
                    "[replay] frame {} side {} dumped to {}",
                    frame,
                    ["A", "B"][side],
                    path
                );
                // --replay-dump-tex=WxH[,WxH]: also the raw level-0 contents of every cached texture of those sizes
                // (uncompressed color formats of 4 or 8 bytes per pixel), per side, to locate which target differs.
                let want = &crate::settings::get().replay.replay_dump_tex;
                if !want.is_empty() {
                    let near: Vec<String> = st
                        .tc
                        .textures
                        .values()
                        .filter(|tx| want.iter().any(|w| w.w as usize == tx.width()))
                        .map(|tx| {
                            format!(
                                "{}x{} {:?} {:?} {:?}",
                                tx.width(),
                                tx.height(),
                                tx.pixelFormat(),
                                tx.textureType(),
                                objc2_metal::MTLResource::storageMode(&**tx)
                            )
                        })
                        .collect();
                    println!("[replay] --replay-dump-tex: {} cached textures, of the wanted widths: {:?}", st.tc.textures.len(), near);
                    for (&addr, tx) in &st.tc.textures {
                        if !want.iter().any(|w| w.is(tx.width(), tx.height()))
                            || objc2_metal::MTLResource::storageMode(&**tx)
                                == objc2_metal::MTLStorageMode::Private
                        {
                            continue;
                        }
                        let bpp = match tx.pixelFormat() {
                            MTLPixelFormat::RGBA16Float
                            | MTLPixelFormat::RGBA16Unorm
                            | MTLPixelFormat::RG32Float => 8,
                            MTLPixelFormat::RGBA8Unorm
                            | MTLPixelFormat::RGBA8Unorm_sRGB
                            | MTLPixelFormat::BGRA8Unorm
                            | MTLPixelFormat::RGB10A2Unorm
                            | MTLPixelFormat::RG11B10Float
                            | MTLPixelFormat::R32Float
                            | MTLPixelFormat::RG16Float
                            | MTLPixelFormat::RG16Unorm => 4,
                            _ => continue,
                        };
                        let (w, h) = (tx.width(), tx.height());
                        let mut raw = vec![0u8; w * h * bpp];
                        let region = MTLRegion {
                            origin: MTLOrigin { x: 0, y: 0, z: 0 },
                            size: MTLSize {
                                width: w,
                                height: h,
                                depth: 1,
                            },
                        };
                        unsafe {
                            tx.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                                std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                                w * bpp,
                                region,
                                0,
                            )
                        };
                        let _ = std::fs::write(
                            format!(
                                "/tmp/nxport_replay_tex_{}_{:x}_{}x{}_{:?}.raw",
                                tag,
                                addr,
                                w,
                                h,
                                tx.pixelFormat()
                            ),
                            &raw,
                        );
                    }
                }
            }
        }
        enc[side].push(t1.duration_since(t0).as_secs_f64() * 1e3);
        drain[side].push(t1.elapsed().as_secs_f64() * 1e3);
    }
    REPLAY_B.store(false, std::sync::atomic::Ordering::Relaxed);
    if !diff_counts.is_empty() {
        let mut d = diff_counts.clone();
        d.sort_unstable();
        println!("[replay-diff] frame {}: differing pixels per repetition: median {} p90 {} max {} ({} of {} above the limit)", frame,
            d[d.len() / 2], d[d.len() * 9 / 10], d[d.len() - 1], d.iter().filter(|&&n| n > diff_limit.unwrap_or(0)).count(), d.len());
    }
    let stat = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (v[0], v[v.len() / 2])
    };
    for side in 0..if ab { 2 } else { 1 } {
        let (emin, emed) = stat(&mut enc[side]);
        let (dmin, dmed) = stat(&mut drain[side]);
        let (_, cmed) = stat(&mut cpu[side]);
        let (gmin, gmed) = stat(&mut gpu[side]);
        // Median CPU per repetition of each engine thread.
        let tmed: Vec<String> = (0..3)
            .map(|k| {
                let mut v: Vec<u64> = threads[side].iter().map(|t| t[k]).collect();
                v.sort_unstable();
                format!(
                    "{:.2}",
                    v.get(v.len() / 2).copied().unwrap_or(0) as f64 / 1e3
                )
            })
            .collect();
        println!("[replay] frame {}{}: thread CPU median ms per repetition: Presentation {}, gfx-encoder {}, gfx-issuer {}",
            frame, if ab { ["  A", "  B"][side] } else { "" }, tmed[0], tmed[1], tmed[2]);
        let pmed: Vec<u64> = (0..6)
            .map(|k| {
                let mut v: Vec<u64> = passes[side].iter().map(|t| t[k]).collect();
                v.sort_unstable();
                v.get(v.len() / 2).copied().unwrap_or(0)
            })
            .collect();
        println!("[replay] frame {}{}: per repetition (median): issuer render passes {} (clear passes {}), compute passes {}, tiny compute draws {}, batched draws {} of {} draws",
            frame, if ab { ["  A", "  B"][side] } else { "" }, pmed[0], pmed[3], pmed[1], pmed[2], pmed[4], pmed[5]);
        println!("[replay] frame {}{} x{} ({} submits): encode min {:.2} median {:.2} ms (CPU median {:.2}), drain (GPU after encode) min {:.2} median {:.2} ms, GPU busy min {:.2} median {:.2} ms",
            frame, if ab { ["  A", "  B"][side] } else { "" }, reps, submits.len(), emin, emed, cmed, dmin, dmed, gmin, gmed);
    }
}

pub fn submit(handles: &[u64]) {
    let _engine = report_guard::engine_scope();
    wait_while_suspended();
    journal(|| format!("submit {} lists t {:.3}", handles.len(), host_secs() * 1e3));
    if replay_spec().is_some() {
        FRAME_SUBMITS.lock().unwrap().push(handles.to_vec());
    }
    if !timeline_enabled() {
        return submit_inner(handles);
    }
    let t0 = std::time::Instant::now();
    if let Some(prev) = *TL_LAST_SUBMIT_END.lock().unwrap() {
        TL_GAP_US.fetch_add(
            t0.duration_since(prev).as_micros() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    let c0 = thread_cpu_ns();
    submit_inner(handles);
    TL_ENCODE_US.fetch_add(
        t0.elapsed().as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    TL_ENCODE_CPU_US.fetch_add(
        (thread_cpu_ns() - c0) / 1000,
        std::sync::atomic::Ordering::Relaxed,
    );
    *TL_LAST_SUBMIT_END.lock().unwrap() = Some(std::time::Instant::now());
}

/// Starts a submit: records the uniform-buffer ranges any UpdateUniformBuffer in it writes (all lists reachable from
/// `handles`), returning the command buffer to encode into. See `ubo_updated`.
/// Bumped when texture() drops or re-points other objects' Metal textures: invalidates every binding memo (global, so
/// encoding threads check it without the texture cache's lock).
static TEX_EPOCH: AtomicU64 = AtomicU64::new(0);

pub(super) fn tex_epoch() -> u64 {
    TEX_EPOCH.load(Ordering::Acquire)
}

fn bump_tex_epoch() {
    TEX_EPOCH.fetch_add(1, Ordering::AcqRel);
}

/// Gfx::texture memo: [hits, misses with the table epoch unchanged, misses after table changes].
static TEX_MEMO_STATS: [std::sync::atomic::AtomicU64; 3] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 3];
/// Table-epoch changes seen per frame by the encoder (horizon::hle::tex_table_epoch deltas at submit starts).
pub(super) static UBO_INDEXED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LISTS_SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn cur_cb_and_ranges(
    st: &mut State,
    cb: &Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    handles: &[u64],
) -> Retained<ProtocolObject<dyn MTLCommandBuffer>> {
    st.ubo_updated.clear();
    st.submit_lists.clear();
    let mut stack = std::mem::take(&mut st.lists_stack);
    let mut ranges = std::mem::take(&mut st.ubo_updated);
    let mut submit_lists = std::mem::take(&mut st.submit_lists);
    let mut samples: Vec<usize> = Vec::new();
    let guard = report_guard::mode() != crate::settings::ReportGuard::Off;
    let n = crate::nvn::api::walk_command_lists(handles, &mut stack, &mut submit_lists, |l| {
        let s = l.summary();
        ranges.extend_from_slice(&s.0);
        if guard {
            samples.extend(
                s.2.iter()
                    .map(|&a| crate::nvn::api::translate_virtual(a as usize)),
            );
        }
    });
    report_guard::begin_submit(&samples);
    st.ubo_updated = ranges;
    st.lists_stack = stack;
    st.submit_lists = submit_lists;
    LISTS_SEEN.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
    // Sorted, merged: ubo_updated is a binary search.
    st.ubo_updated.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(st.ubo_updated.len());
    for &(a, b) in &st.ubo_updated {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    UBO_RANGES.fetch_add(merged.len() as u64, std::sync::atomic::Ordering::Relaxed);
    st.ubo_updated = merged;
    cb.clone()
}

/// Whether [addr, addr+len) overlaps memory an UpdateUniformBuffer of the current submit writes.
pub(super) fn ubo_updated(ranges: &[(u64, u64)], addr: u64, len: u64) -> bool {
    // First merged range ending after addr: overlaps iff it starts before addr + len.
    let i = ranges.partition_point(|&(_, b)| b <= addr);
    ranges.get(i).is_some_and(|&(a, _)| a < addr + len)
}

static UBO_RANGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn submit_inner(handles: &[u64]) {
    autoreleasepool(|_| {
        let Some(g) = GFX.get() else { return };
        while retire_oldest(g, true) {}
        while groups_in_flight(&g.state.lock().unwrap()) >= frames_in_flight() {
            let t0 = std::time::Instant::now();
            retire_oldest(g, false);
            SUBMIT_WAIT_US.fetch_add(
                t0.elapsed().as_micros() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        if g.state.lock().unwrap().gpu_halted {
            return;
        }
        let desc = MTLCommandBufferDescriptor::new();
        desc.setErrorOptions(MTLCommandBufferErrorOption::EncoderExecutionStatus);
        let Some(cb) = g.queue.commandBufferWithDescriptor(&desc) else {
            return;
        };
        {
            let mut st = g.state.lock().unwrap();
            st.stats.submits += 1;
            // The issuer thread's encode state (parallel encoding) resets its per-submit memory in stream order.
            st.issuer.push(issuer::Op::SubmitStart);
            st.cx.arena.reset();
            st.cx.ubo_copies.clear();
            st.cx.max_index_memo.clear();
            // Its buffers are arena memory, which is reused once the GPU is done with it (Arena::reset).
            st.cx.support_cache.clear();
            st.cx.driver_cb_memo = [None, None];
            st.cx.index_memo = None;
            st.plan_arena.reset();
            st.plan_ubo.clear();
            st.ubo_plan_epoch += 1;
            st.rts_dirty = true;
            st.cached_rts = None;
            // Memos validate everything they depend on and persist across submits; the first ~1,400 bindings of every
            // frame went through full resolution while the memo was cleared here.
            if st.cx.bind_memo.len() > 1 << 16 {
                st.cx.bind_memo.clear();
            }
            if early_fence() {
                let prev: Vec<(u64, u64)> = std::mem::take(&mut st.gpu_dirty).into_iter().collect();
                if !prev.is_empty() {
                    // The previous submit's tail may go into the next command buffer.
                    st.carried_dirty.push_back((submitted_serial() + 1, prev));
                }
                let done = completed_serial();
                st.carried_dirty.retain(|(s, _)| *s > done);
                let carried: Vec<(u64, u64)> = st
                    .carried_dirty
                    .iter()
                    .flat_map(|(_, r)| r.iter().copied())
                    .collect();
                for (a, b) in carried {
                    mark_gpu_dirty(&mut st, a, b);
                }
            } else {
                st.gpu_dirty.clear();
            }
            st.lb.enc_labels.clear();
            st.lb.enc_details.clear();
            st.cur_cb = Some(cur_cb_and_ranges(&mut st, &cb, handles));
            st.cb_started = Some(std::time::Instant::now());
            for h in handles {
                let Some(cur) = st.cur_cb.clone() else { break };
                unsafe { g.run_list(&mut st, &cur, *h, 0) };
            }
            g.end_encoder(&mut st);
            st.submit_lists.clear();
        }
        if gfx_mode() == GfxMode::Dry {
            // Nothing reaches the GPU: the encoded work is dropped (all encoders are already ended).
            g.state.lock().unwrap().occl.pending.clear();
            return;
        }
        let mut st = g.state.lock().unwrap();
        let Some(cb) = st.cur_cb.take() else { return };
        tail_log_label(&st, &cb);
        let serial = push_inflight(&mut st, cb.clone());
        st.issuer.commit_serial(cb, Some(serial)); // after the queued encoder ops, on the issuer thread (metal/issuer.rs)
    });
}

/// Presented frames per second over the last half second or so (shown in the window title), as f32 bits.
static DISPLAY_FPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// Debug: --enc-stats prints, every 600 presents, which call sites ended render encoders and how many translated
// passes reopened the targets of the pass just ended (merge candidates).
/// A clear waiting to be folded into the next pass on its target or encoded on its own (`Gfx::defer_clear`).
/// A cached repacked vertex stream: its buffer, vertices covered, hash of the source bytes, frame last validated.
pub(super) struct RepackEntry {
    pub(super) buf: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub(super) cap: usize,
    pub(super) src_hash: u64,
    pub(super) checked: u64,
}

/// translated::root_id (memoized on the texture itself).
fn cached_root_id(_st: &mut State, t: &Tex) -> u64 {
    translated::root_id(t)
}

/// A texture binding's resolution, valid while the pool slot, the object's generation, the dirty-texture counter, the
/// texture-cache epoch and (for alias views) the root's generation are unchanged, within one submit.
pub(super) struct BindMemo {
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

/// Texture bindings resolved by bound_texture: [memo hit without lookups, memo hit after lookups, resolved].
pub(super) static BIND_STATS: [std::sync::atomic::AtomicU64; 3] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 3];

/// Alias views created (not found in alias_views), for the timeline log.
static ALIAS_NEW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// --log-view-formats: prints each distinct (texture format, view format) pair that needs MTLTextureUsagePixelFormatView
/// (formats that differ by more than sRGB-ness), with the creating site.
#[track_caller]
pub(crate) fn log_view_format(from: MTLPixelFormat, to: MTLPixelFormat) {
    static SEEN: OnceLock<Mutex<std::collections::HashSet<(usize, usize)>>> = OnceLock::new();
    if from == to || !crate::settings::get().log.log_view_formats || srgb_pair(from, to) {
        return;
    }
    if SEEN
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert((from.0, to.0))
    {
        println!(
            "[view-fmt] {:?} -> {:?} at {}",
            from,
            to,
            std::panic::Location::caller()
        );
    }
}

/// Pixel formats (raw values) whose textures are created with MTLTextureUsagePixelFormatView. The usage makes Metal store
/// a texture without its lossless compression / optimized layout (a 1080p RGBA8 copy took 0.16 ms with it, 0.045 ms
/// without, on an M4), and TotK needs it for no view at all: same-format, sRGB <-> linear, slice, level and swizzle views
/// work without it. A format joins this set the first time a view needing the usage is requested from a texture of that
/// format (that one view is not made).
static VIEW_FLAG_FORMATS: OnceLock<Mutex<std::collections::HashSet<usize>>> = OnceLock::new();

pub(crate) fn wants_view_flag(pf: MTLPixelFormat) -> bool {
    VIEW_FLAG_FORMATS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .contains(&pf.0)
}

/// Whether `t` can have a view of format `pf`: false (with a warning, and later textures of its format get the usage)
/// when that needs MTLTextureUsagePixelFormatView and `t` was created without it.
#[track_caller]
pub(crate) fn view_format_ok(t: &ProtocolObject<dyn MTLTexture>, pf: MTLPixelFormat) -> bool {
    let from = t.pixelFormat();
    log_view_format(from, pf);
    if from == pf || srgb_pair(from, pf) || t.usage().contains(MTLTextureUsage::PixelFormatView) {
        return true;
    }
    if VIEW_FLAG_FORMATS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(from.0)
    {
        println!("[!] Gfx: a {:?} view of a {:?} texture needs PixelFormatView usage ({}): not made; new {:?} textures get the usage",
            pf, from, std::panic::Location::caller(), from);
    }
    false
}

/// Two formats that differ only in sRGB-ness (Metal creates views between them without PixelFormatView usage).
pub(crate) fn srgb_pair(a: MTLPixelFormat, b: MTLPixelFormat) -> bool {
    let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
    hi == lo + 1 && matches!(lo, 10 | 30 | 70 | 80 | 130 | 132 | 134 | 152)
}

/// Passes ended for in-place render-target feedback (translated::feedback_snapshots).
pub(super) static FB_INPLACE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Diagnostics (printed with the timeline): alias views made, new Metal textures, texture uploads.
static TEXSTAT: [std::sync::atomic::AtomicU64; 3] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// --log-alias: log alias views as they are made (~20 per frame in Hateno; stdout cost was measurable).
fn alias_log_enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| crate::settings::get().log.log_alias)
}

/// --log-replay: per 100 frames, how many draws come from command lists recorded in an earlier frame, and how
/// many have the same input state (bindings) and the same uniform contents as the draw at the same list position in
/// the previous frame: the upper bound for caching translated draws across frames.
fn replay_log_enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| crate::settings::get().log.log_replay)
}

#[derive(Default)]
pub(super) struct ReplayStats {
    prev: HashMap<(u64, usize), [u64; 8]>,
    prev2: HashMap<(u64, usize), [u64; 8]>,
    cur: HashMap<(u64, usize), [u64; 8]>,
    comp_same: [u64; 8],
    seen2: u64,
    frame: u64,
    draws: u64,
    seen_before: u64,
    start: u64,
}

fn replay_note(
    st: &mut State,
    handle: u64,
    op_index: usize,
    _list_old: bool,
    op: &crate::nvn::api::RecordedNvnCommand,
) {
    use std::hash::{Hash, Hasher};
    const N: usize = 8;
    const NAMES: [&str; N] = [
        "program",
        "vertex layout",
        "vertex buffers",
        "uniform slots+sizes",
        "uniform addresses",
        "texture handles",
        "targets",
        "fixed-function+draw args",
    ];
    let f = presented_frames();
    if st.replay.frame != f {
        let r = &mut st.replay;
        r.prev2 = std::mem::take(&mut r.prev);
        r.prev = std::mem::take(&mut r.cur);
        r.frame = f;
        if f >= r.start + 100 && r.draws > 0 {
            let pct = |x: u64| x as f64 * 100.0 / r.draws as f64;
            let comps: Vec<String> = (0..N)
                .map(|k| format!("{} {:.0}%", NAMES[k], pct(r.comp_same[k])))
                .collect();
            println!("[replay] frames {}..{}: {:.0} draws/frame; same list position as 1 frame ago {:.1}% / 2 ago {:.1}%; of all draws, same as 2 frames ago: {}",
                r.start, f, r.draws as f64 / (f - r.start) as f64, pct(r.seen_before), pct(r.seen2), comps.join(", "));
            *r = ReplayStats {
                prev: std::mem::take(&mut r.prev),
                prev2: std::mem::take(&mut r.prev2),
                frame: f,
                start: f,
                ..Default::default()
            };
        }
    }
    let hash = |x: &dyn Fn(&mut rustc_hash::FxHasher)| {
        let mut h = rustc_hash::FxHasher::default();
        x(&mut h);
        h.finish()
    };
    let mut vb: Vec<_> = st.vertex_buffers.iter().map(|(k, v)| (*k, *v)).collect();
    vb.sort_unstable();
    let mut ub: Vec<_> = st.uniform_buffers.iter().map(|(k, v)| (k, *v)).collect();
    ub.sort_unstable();
    let mut tb: Vec<_> = st.textures_bound.iter().map(|(k, v)| (k, *v)).collect();
    tb.sort_unstable();
    let comps: [u64; N] = [
        hash(&|h| st.program.hash(h)),
        hash(&|h| {
            st.vertex_attribs.as_slice().hash(h);
            st.vertex_streams.hash(h);
        }),
        hash(&|h| vb.hash(h)),
        hash(&|h| ub.iter().for_each(|(k, (_, sz))| (*k, *sz).hash(h))),
        hash(&|h| ub.iter().for_each(|(_, (a, _))| a.hash(h))),
        hash(&|h| tb.hash(h)),
        hash(&|h| {
            for c in &st.colors {
                c.texture.hash(h);
            }
            st.depth.map(|d| d.texture).hash(h);
        }),
        hash(&|h| {
            st.blend_states.hash(h);
            (
                st.depth_stencil,
                st.polygon,
                st.viewport,
                st.scissor,
                st.color_state,
                st.channel_mask,
                st.prim_restart,
            )
                .hash(h);
            format!("{:?}", op.typed).hash(h);
        }),
    ];
    let r = &mut st.replay;
    r.draws += 1;
    if let Some(p) = r.prev2.get(&(handle, op_index)) {
        r.seen2 += 1;
        for k in 0..N {
            if p[k] == comps[k] {
                r.comp_same[k] += 1;
            }
        }
    }
    if r.prev.contains_key(&(handle, op_index)) {
        r.seen_before += 1;
    }
    r.cur.insert((handle, op_index), comps);
}

/// Records guest memory [a, b) as written by GPU work of the current submit (once per range).
/// The storage buffer bound at (stage, index), if any.
pub(super) fn storage_buffer(st: &State, stage: i32, index: i32) -> Option<(u64, usize)> {
    let e = *st
        .storage_buffers
        .get(stage as usize)?
        .get(index as usize)?;
    (e.1 != 0 || e.0 != 0).then_some(e)
}

pub(super) fn mark_gpu_dirty(st: &mut State, a: u64, b: u64) {
    if a >= b {
        return;
    }
    st.ubo_plan_epoch += 1;
    let (mut lo, mut hi) = (a, b);
    if let Some((&s, &e)) = st.gpu_dirty.range(..=lo).next_back() {
        if e >= lo {
            if e >= hi {
                return; // already covered (every compute dispatch re-adds its storage buffers)
            }
            lo = s;
        }
    }
    while let Some((&s, &e)) = st.gpu_dirty.range(lo..=hi).next() {
        st.gpu_dirty.remove(&s);
        hi = hi.max(e);
    }
    st.gpu_dirty.insert(lo, hi);
}

pub(super) struct PendingClear {
    pub(super) tex: Tex,
    /// translated::root_id(&tex), computed once (each new clear is compared with every pending one).
    pub(super) root: u64,
    pub(super) level: usize,
    /// Array slice, or depth plane of a 3D texture.
    pub(super) layer: usize,
    pub(super) kind: ClearKind,
}

#[derive(Clone, Copy)]
pub(super) enum ClearKind {
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

pub(crate) fn enc_stats_enabled() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *E.get_or_init(|| crate::settings::get().log.enc_stats)
}
static ENC_END_SITES: std::sync::Mutex<std::collections::BTreeMap<(&'static str, u32), u64>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
pub(crate) static ENC_REOPEN_SAME: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static ENC_OPENED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static CLEARS_FOLDED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static CLEARS_FLUSHED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
/// Render passes those clears took (clears of equal size share one, flush_clears_matching).
pub(crate) static CLEAR_PASSES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn print_enc_stats(frames: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    let mut sites = ENC_END_SITES.lock().unwrap();
    let mut v: Vec<_> = sites.iter().map(|(k, n)| (*n, *k)).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    let f = frames.max(1) as f64;
    println!("[enc-stats] per frame: translated passes opened {:.1}, reopened same targets {:.1}, clears folded {:.1} / own pass {:.1}; ends by site: {}",
        ENC_OPENED.swap(0, Relaxed) as f64 / f, ENC_REOPEN_SAME.swap(0, Relaxed) as f64 / f,
        CLEARS_FOLDED.swap(0, Relaxed) as f64 / f, CLEARS_FLUSHED.swap(0, Relaxed) as f64 / f,
        v.iter().take(10).map(|(n, (file, line))| format!("{}:{}={:.1}", file.rsplit('/').next().unwrap_or(file), line, *n as f64 / f)).collect::<Vec<_>>().join(" "));
    sites.clear();
}

#[cfg(target_os = "macos")]
pub fn display_fps() -> f32 {
    f32::from_bits(DISPLAY_FPS.load(std::sync::atomic::Ordering::Relaxed))
}

fn count_display_frame() {
    if enc_stats_enabled() {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 600 == 599 {
            print_enc_stats(600);
        }
    }
    static WINDOW: std::sync::Mutex<Option<(std::time::Instant, u32)>> =
        std::sync::Mutex::new(None);
    let now = std::time::Instant::now();
    let mut w = WINDOW.lock().unwrap();
    let (start, n) = w.get_or_insert((now, 0));
    *n += 1;
    let dt = now.duration_since(*start).as_secs_f32();
    if dt >= 0.5 {
        DISPLAY_FPS.store(
            (*n as f32 / dt).to_bits(),
            std::sync::atomic::Ordering::Relaxed,
        );
        *w = Some((now, 0));
    }
}

/// nvnQueuePresentTexture: show `nvn_tex` (a window texture) in the window.
/// --log-hitch=<ms>: per-frame work counters, printed for frames whose present-to-present time exceeds the threshold.
pub(crate) static HITCH_PIPE_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static HITCH_PIPE_N: AtomicU64 = AtomicU64::new(0);
pub(crate) static HITCH_TEX_N: AtomicU64 = AtomicU64::new(0);
static HITCH_UPLOAD_NS: AtomicU64 = AtomicU64::new(0);
static HITCH_HASH_NS: AtomicU64 = AtomicU64::new(0);
static HITCH_TEX: Mutex<Vec<String>> = Mutex::new(Vec::new());
static HITCH_MS: OnceLock<Option<f64>> = OnceLock::new();
fn hitch_enabled() -> bool {
    HITCH_MS
        .get_or_init(|| crate::settings::get().log.log_hitch)
        .is_some()
}

fn log_hitch() {
    hitch_enabled();
    let Some(thr) = *HITCH_MS.get().unwrap() else {
        return;
    };
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let now = std::time::Instant::now();
    let prev = LAST.lock().unwrap().replace(now);
    let pipe_ns = HITCH_PIPE_NS.swap(0, Ordering::Relaxed);
    let pipe_n = HITCH_PIPE_N.swap(0, Ordering::Relaxed);
    let async_started = translated::ASYNC_STARTED.swap(0, Ordering::Relaxed);
    let async_skips = translated::ASYNC_SKIPS.swap(0, Ordering::Relaxed);
    let tex_n = HITCH_TEX_N.swap(0, Ordering::Relaxed);
    let texs = std::mem::take(&mut *HITCH_TEX.lock().unwrap());
    let up_ns = HITCH_UPLOAD_NS.swap(0, Ordering::Relaxed);
    let hash_ns = HITCH_HASH_NS.swap(0, Ordering::Relaxed);
    if let Some(p) = prev {
        let ms = (now - p).as_secs_f64() * 1e3;
        if ms > thr {
            println!("[hitch] frame {} {:.1} ms: {} pipelines built in {:.1} ms, {} textures created (upload {:.1} ms, hash {:.1} ms)", presented_frames(), ms,
                pipe_n, pipe_ns as f64 / 1e6, tex_n, up_ns as f64 / 1e6, hash_ns as f64 / 1e6);
            if async_started + async_skips > 0 {
                println!("[hitch]   async pipelines: {} compiles started, {} draws skipped while compiling", async_started, async_skips);
            }
            for t in &texs {
                println!("[hitch]   new texture {}", t);
            }
        }
    }
}

pub fn present(nvn_tex: usize, crop: [i32; 4]) {
    autoreleasepool(|_| {
        wait_while_suspended();
        let Some(g) = GFX.get() else { return };
        if let Some((first, reps, every)) = replay_spec() {
            let submits = std::mem::take(&mut *FRAME_SUBMITS.lock().unwrap());
            let n = presented_frames();
            if n == first || (every > 0 && n > first && (n - first) % every == 0) {
                REPLAY_HOLD.store(true, std::sync::atomic::Ordering::Release);
                replay_frame(g, n, reps, &submits, nvn_tex);
                REPLAY_HOLD.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        count_display_frame();
        tail_log_present(presented_frames());
        log_hitch();
        {
            static FROM: OnceLock<Option<u64>> = OnceLock::new();
            if let Some(n) = *FROM.get_or_init(|| crate::settings::get().trace.trace_from_frame) {
                if presented_frames() == n {
                    unsafe { crate::cpu::crash_handler::install_deferred_trace_points() };
                }
            }
        }
        dump_main_image();
        // With --early-fence the game's fence no longer guarantees that the targets written back below (drawn before the
        // previous present) are finished: wait for them here, outside the state lock.
        if early_fence() && rt_writeback_enabled() {
            wait_serial(WRITEBACK_SERIAL.load(Ordering::Acquire), u64::MAX);
        }
        // A candidate drawn again in the frame just submitted is written back with that newer rendering, so all of it
        // must have finished (see writeback_pass).
        if rt_writeback_enabled() {
            let redrawn = {
                let st = g.state.lock().unwrap();
                st.writeback.1.iter().any(|(addr, frame)| {
                    unsafe { tex_info(*addr) }.is_some_and(|(_, t)| {
                        st.rt_history
                            .get(&t.storage)
                            .is_some_and(|&last| last > *frame)
                    })
                })
            };
            if redrawn {
                g.state.lock().unwrap().issuer.sync();
                wait_serial(submitted_serial(), u64::MAX);
            }
        }
        let src = {
            let mut st = g.state.lock().unwrap();
            // The frame's last command buffers may still be queued on the issuer thread (commits go through it). Metal runs
            // command buffers in commit order, so the present buffer (committed directly below) could otherwise overtake
            // the rendering it shows: Kirby's UI lost its text and showed half-drawn tiles on some frames.
            st.issuer.sync();
            pass_prof::on_present(&g.device, presented_frames());
            report_cmd_time();
            if rt_writeback_enabled() {
                unsafe { writeback_pass(g, &mut st) };
            }
            let Some(t) = (unsafe { g.texture(&mut st.tc, nvn_tex) }) else {
                return;
            };
            let (pf, twin) = metal_format(
                unsafe { tex_info(nvn_tex) }
                    .map(|(_, t)| t.format)
                    .unwrap_or(0),
            );
            // sRGB window textures hold display-ready bytes: view them as plain UNORM so the blit
            // doesn't linearise them and the (non-sRGB) drawable then double-encode.
            match twin {
                Some(plain) if plain != pf && view_format_ok(&t, plain) => {
                    t.newTextureViewWithPixelFormat(plain).unwrap_or(t)
                }
                _ => t,
            }
        };
        if let Some(layer) = g.layer.get().filter(|_| gfx_mode() != GfxMode::Dry) {
            // The drawable (and on iOS the letterboxed layer) follows the shape the game presents: mods render other aspect
            // ratios (UltraCam at the iPhone's 19.5:9).
            {
                let (w, h) = if crop[2] > 0 && crop[3] > 0 {
                    (crop[2] as u64, crop[3] as u64)
                } else {
                    (src.width() as u64, src.height() as u64)
                };
                static CONTENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                #[cfg(target_os = "macos")]
                if crate::host::display::follow_window_shape(layer) {
                    CONTENT.store(0, Ordering::Relaxed);
                }
                if CONTENT.swap(w << 32 | h, Ordering::Relaxed) != w << 32 | h {
                    crate::host::display::content_size_changed(layer, w as f64, h as f64);
                }
            }
            let Some(drawable) = layer.nextDrawable() else {
                return;
            };
            let pass = MTLRenderPassDescriptor::renderPassDescriptor();
            unsafe {
                let att = pass.colorAttachments().objectAtIndexedSubscript(0);
                att.setTexture(Some(&drawable.texture()));
                att.setLoadAction(MTLLoadAction::DontCare);
                att.setStoreAction(MTLStoreAction::Store);
            }
            let Some(cb) = g.queue.commandBuffer() else {
                return;
            };
            if let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) {
                enc.setRenderPipelineState(&g.blit);
                unsafe { enc.setFragmentTexture_atIndex(Some(&src), 0) };
                let tw = src.width() as f32;
                let th = src.height() as f32;
                let (cx, cy, cw, ch) = if crop[2] > 0 && crop[3] > 0 {
                    (
                        crop[0] as f32,
                        crop[1] as f32,
                        crop[2] as f32,
                        crop[3] as f32,
                    )
                } else {
                    (0.0, 0.0, tw, th)
                };
                let is_lower_left = crate::nvn::api::window_origin_mode() == 0;
                let crop_uv: [f32; 4] = if is_lower_left {
                    // NVN LOWER_LEFT window origin (Kirby; TotK uses UPPER_LEFT): rendering keeps the Switch's memory row order
                    // (clip y is negated, see translated::y_negate), where the first row is the bottom of the screen.
                    // Display rectangle starts at y = cy (bottom) and ends at y = cy + ch (top).
                    // Screen top (in.uv.y = 0) maps to (cy + ch) / th; screen bottom (in.uv.y = 1) maps to cy / th.
                    [cx / tw, (cy + ch) / th, cw / tw, -ch / th]
                } else {
                    // UPPER_LEFT (TotK): row 0 is the top of the screen.
                    // Screen top (in.uv.y = 0) maps to cy / th; screen bottom (in.uv.y = 1) maps to (cy + ch) / th.
                    [cx / tw, cy / th, cw / tw, ch / th]
                };
                unsafe {
                    enc.setFragmentBytes_length_atIndex(
                        std::ptr::NonNull::from(&crop_uv).cast(),
                        16,
                        0,
                    )
                };
                unsafe {
                    enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3)
                };
                enc.endEncoding();
            }
            crate::host::overlay::render(&g.device, &cb, &drawable.texture());
            cb.presentDrawable(ProtocolObject::from_ref(&*drawable));
            cb.commit();
            // Remember it: it is never waited on, so check how it ended when the next frame is presented. A failure here
            // would make the next rendering buffer an "innocent victim" in `submit`'s report.
            let prev_present = {
                let mut st = g.state.lock().unwrap();
                st.present_cb.replace(cb.clone())
            };
            if let Some(p) = prev_present {
                if p.status() == MTLCommandBufferStatus::Error {
                    let victim = p.error().map_or(false, |e| {
                        e.localizedDescription().to_string().contains("victim")
                    });
                    if !victim {
                        println!("[!!] GPU: the window PRESENT command buffer failed (this, not the rendering buffer, is the culprit):");
                        report_gpu_error(&p, &[], &[]);
                    }
                }
            }
        }
        static P: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let pn = P.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        // AOT dump run with mods (cpu/aot.rs): write the image once mods finished patching code, then stop.
        if crate::cpu::aot::deferred_dump_frame() == Some(pn) {
            if let Err(e) = unsafe { crate::cpu::aot::dump_after_mods() } {
                eprintln!("[!!] AOT: {}", e);
                std::process::exit(1);
            }
            println!(
                "[+] AOT: image written at frame {} (mods applied); exiting (--aot-dump)",
                pn
            );
            std::process::exit(0);
        }
        PRESENTED.store(pn, std::sync::atomic::Ordering::Relaxed);
        // At most frames_in_flight() command buffers are in flight, so memory freed that many presents ago (plus margin)
        // is no longer read by the GPU.
        crate::nvn::api::flush_pending_unmaps(pn, frames_in_flight() as u64 + 2);
        if crate::settings::get().log.hle_history_at == Some(pn) {
            crate::horizon::hle::summarize_hle_call_history();
        }
        if pn == 1 || pn % 100 == 0 {
            println!("[DEBUG GFX] present #{} nvn_tex=0x{:x}", pn, nvn_tex);
            if translated::enabled() {
                let st = g.state.lock().unwrap();
                println!(
                "[gfx] draws={} translated={} fallbacks={} pipelines={} pipeline_failures={} copies={}",
                st.stats.draws,
                TRANSLATED_DRAWS.load(Ordering::Relaxed),
                st.stats.draws.saturating_sub(TRANSLATED_DRAWS.load(Ordering::Relaxed)),
                { let sh = st.sh.lock().unwrap(); sh.translated_pipelines.len() },
                st.sh.lock().unwrap().translated_pipelines_failed,
                st.stats.copies
            );
                println!(
                    "[gfx] instanced draws={} max_instances={} total_instances={} max_count={}",
                    st.cx.instanced_stats.0,
                    st.cx.instanced_stats.1,
                    st.cx.instanced_stats.2,
                    st.cx.instanced_stats.3
                );
                let mut hist: Vec<_> = st.cx.translated_hist.iter().collect();
                hist.sort_by(|a, b| b.1.cmp(a.1));
                for ((vs, fs, rt, depth), n) in hist.into_iter().take(10) {
                    let name = |p: usize| {
                        if p == 0 {
                            "-".to_string()
                        } else {
                            unsafe { &*(p as *const crate::metal::shader_cache::CachedStage) }
                                .entry
                                .clone()
                        }
                    };
                    println!(
                        "[gfx]   {:6} x vs={} fs={} rt0_fmt={} depth_fmt={}",
                        n,
                        name(*vs),
                        name(*fs),
                        rt,
                        depth
                    );
                }
            }
        }
        if crate::settings::get().dump.dump_first_nonzero {
            if pn % 10 == 0 {
                if let Some(cb) = g.queue.commandBuffer() {
                    cb.commit();
                    cb.waitUntilCompleted();
                }
                if has_non_zero_pixel(&src) {
                    dump_ppm(&src, &format!("/tmp/nxport_frame_FIRST_NONZERO_{}.ppm", pn));
                    println!("[*** SUCCESS ***] Found non-zero pixels at frame {}! Dumped to /tmp/nxport_frame_FIRST_NONZERO_{}.ppm", pn, pn);
                    std::process::exit(0);
                }
                let st = g.state.lock().unwrap();
                for (&addr, t) in &st.tc.textures {
                    if (addr == 0x303602020 || addr == 0x3036020c0)
                        && addr != nvn_tex
                        && has_non_zero_pixel(t)
                    {
                        dump_ppm(
                            t,
                            &format!("/tmp/nxport_frame_FIRST_NONZERO_{}_backbuffer.ppm", pn),
                        );
                        println!("[*** SUCCESS ***] Found non-zero pixels in swapchain backbuffer 0x{:x} at frame {}!", addr, pn);
                        std::process::exit(0);
                    }
                }
            }
            if pn >= 1500 {
                if let Some(cb) = g.queue.commandBuffer() {
                    cb.commit();
                    cb.waitUntilCompleted();
                }
                dump_ppm(&src, "/tmp/nxport_frame_1500.ppm");
                println!("[+] Reached frame 1500, dumped to /tmp/nxport_frame_1500.ppm");
                std::process::exit(0);
            }
        }
        translated::ASYNC_WAITED_US.store(0, Ordering::Relaxed);
        if let Some(b) = translated::debug_vpos_buffer() {
            let at: u64 = crate::settings::get().debug.debug_vpos_dump_at;
            // Records start fresh with frame `at` (append counters at the buffer start are reset once the GPU is idle).
            if pn + 1 == at {
                if let Some(cb) = g.queue.commandBuffer() {
                    cb.commit();
                    cb.waitUntilCompleted();
                }
                unsafe {
                    std::ptr::write_bytes(
                        b.0.contents().as_ptr() as *mut u8,
                        0,
                        translated::debug_vpos_len(),
                    )
                };
            }
            if pn == at {
                if let Some(cb) = g.queue.commandBuffer() {
                    cb.commit();
                    cb.waitUntilCompleted();
                }
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        b.0.contents().as_ptr() as *const u8,
                        translated::debug_vpos_len(),
                    )
                };
                let _ = std::fs::write("/tmp/nxport_vpos.bin", bytes);
                println!(
                    "[+] Debug: vertex debug buffer written to /tmp/nxport_vpos.bin (frame {})",
                    pn
                );
            }
        }
        glitch_scan(g, pn, &src);
        report_guard::frame_end(pn);
        tex_mem_report(g, pn);
        tex_backing_frame(g, pn);
        frame_history(g, pn, &src);
        // --dump-frame=N or N,M,... writes the presented frame as /tmp/nxport_frame_N.ppm (tools/ppm2png.py converts);
        // with --exit-on-dump the process exits after the last listed frame.
        let dump_frames: &[u64] = &crate::settings::get().dump.dump_frame;
        // --dump-on-trigger=N: dump N frames after the triggered (traced) frame (1 = that frame).
        let trig_off: u64 = crate::settings::get()
            .dump
            .dump_on_trigger
            .unwrap_or(1)
            .max(1)
            - 1;
        let captured = CAPTURE_REQUESTED.load(std::sync::atomic::Ordering::Relaxed)
            && trace_triggered_frame() == pn;
        let triggered_dump = captured
            || (crate::settings::get().dump.dump_on_trigger.is_some()
                && trace_triggered_frame() != 0
                && trace_triggered_frame() + trig_off == pn);
        if dump_frames.contains(&pn) || triggered_dump {
            if let Some(cb) = g.queue.commandBuffer() {
                cb.commit();
                cb.waitUntilCompleted();
            }
            dump_ppm(&src, &format!("/tmp/nxport_frame_{}.ppm", pn));
            {
                let st = g.state.lock().unwrap();
                for (&addr, t) in &st.tc.textures {
                    if (addr == 0x303602020 || addr == 0x3036020c0) && addr != nvn_tex {
                        dump_ppm(t, &format!("/tmp/nxport_frame_{}_other.ppm", pn));
                        println!(
                            "[+] Gfx: dumped other swapchain buffer 0x{:x} (non-zero={})",
                            addr,
                            has_non_zero_pixel(t)
                        );
                    }
                }
            }
            // --dump-mem=0xADDR:LEN[,..]: print guest memory as f32 once the frame's GPU work has completed.
            for &crate::settings::MemRange { addr, len } in &crate::settings::get().dump.dump_mem {
                {
                    let p = crate::nvn::api::translate_virtual(addr) as *const f32;
                    let v: Vec<f32> = (0..len / 4)
                        .map(|i| unsafe { p.add(i).read_unaligned() })
                        .collect();
                    for (row, c) in v.chunks(4).enumerate() {
                        println!("[mem] {:#x} [{}] {:?}", addr + row * 16, row, c);
                    }
                }
            }
            // --dump-mem-raw=0xADDR:LEN:path[,..]: write guest memory bytes to a file (LEN decimal), same moment as above.
            for d in &crate::settings::get().dump.dump_mem_raw {
                let crate::settings::MemRange { addr, len } = d.range;
                let p = crate::nvn::api::translate_virtual(addr) as *const u8;
                let bytes = unsafe { std::slice::from_raw_parts(p, len) };
                match std::fs::write(&d.path, bytes) {
                    Ok(()) => println!("[mem] {:#x}+{} written to {}", addr, len, d.path.display()),
                    Err(e) => println!(
                        "[mem] {:#x}+{}: cannot write {}: {}",
                        addr,
                        len,
                        d.path.display(),
                        e
                    ),
                }
            }
            let want = &crate::settings::get().dump.dump_tex_size;
            if !want.is_empty() {
                // Debug: dump every cached texture of the given sizes (e.g. 2048x4096,800x450), whatever its role.
                let st = g.state.lock().unwrap();
                for (&addr, t) in &st.tc.textures {
                    if want.iter().any(|w| w.is(t.width(), t.height())) {
                        // Block-compressed textures: write level 0 slice 0 as raw blocks (row-major, block rows tightly
                        // packed), for byte comparison with another emulator's copy.
                        let bc_bytes = match t.pixelFormat() {
                            MTLPixelFormat::BC1_RGBA
                            | MTLPixelFormat::BC1_RGBA_sRGB
                            | MTLPixelFormat::BC4_RUnorm
                            | MTLPixelFormat::BC4_RSnorm => Some(8),
                            MTLPixelFormat::BC2_RGBA
                            | MTLPixelFormat::BC2_RGBA_sRGB
                            | MTLPixelFormat::BC3_RGBA
                            | MTLPixelFormat::BC3_RGBA_sRGB
                            | MTLPixelFormat::BC5_RGUnorm
                            | MTLPixelFormat::BC5_RGSnorm
                            | MTLPixelFormat::BC7_RGBAUnorm
                            | MTLPixelFormat::BC7_RGBAUnorm_sRGB => Some(16),
                            _ => None,
                        };
                        if let Some(bb) = bc_bytes {
                            let (bw, bh) = ((t.width() + 3) / 4, (t.height() + 3) / 4);
                            let mut raw = vec![0u8; bw * bh * bb];
                            let region = MTLRegion {
                                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                                size: MTLSize {
                                    width: t.width(),
                                    height: t.height(),
                                    depth: 1,
                                },
                            };
                            unsafe {
                                t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                                    std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                                    bw * bb,
                                    region,
                                    0,
                                )
                            };
                            let path = format!(
                                "/tmp/nxport_frame_{}_tex_{:x}_{}x{}_{:?}.raw",
                                pn,
                                addr,
                                t.width(),
                                t.height(),
                                t.pixelFormat()
                            );
                            let _ = std::fs::write(&path, &raw);
                            println!("[+] Gfx: texture 0x{:x} raw blocks -> {}", addr, path);
                            continue;
                        }
                        if matches!(
                            t.pixelFormat(),
                            MTLPixelFormat::Depth16Unorm | MTLPixelFormat::Depth32Float
                        ) {
                            // Depth textures are private: copy each slice into a buffer on the GPU and dump it as greyscale.
                            use objc2_metal::MTLBlitCommandEncoder;
                            let bpp = if t.pixelFormat() == MTLPixelFormat::Depth16Unorm {
                                2
                            } else {
                                4
                            };
                            let (w, h) = (t.width(), t.height());
                            let slices = t.arrayLength().max(1);
                            let Some(buf) = g.device.newBufferWithLength_options(
                                w * h * bpp * slices,
                                MTLResourceOptions::StorageModeShared,
                            ) else {
                                continue;
                            };
                            if let Some(cb) = g.queue.commandBuffer() {
                                st.issuer.sync(); // other command-buffer use: queued encoder ops first (metal/issuer.rs)
                                if let Some(blit) = cb.blitCommandEncoder() {
                                    for sl in 0..slices {
                                        unsafe {
                                            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                                            t, sl, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: w, height: h, depth: 1 }, &buf, sl * w * h * bpp, w * bpp, w * h * bpp,
                                        );
                                        }
                                    }
                                    blit.endEncoding();
                                }
                                st.issuer.sync(); // other command-buffer use: queued encoder ops first (metal/issuer.rs)
                                cb.commit();
                                cb.waitUntilCompleted();
                            }
                            let data = unsafe {
                                std::slice::from_raw_parts(
                                    buf.contents().as_ptr() as *const u8,
                                    w * h * bpp * slices,
                                )
                            };
                            for sl in 0..slices {
                                let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
                                let (mut lo, mut hi, mut ones) = (f32::MAX, f32::MIN, 0usize);
                                for i in 0..w * h {
                                    let o = (sl * w * h + i) * bpp;
                                    let d = if bpp == 2 {
                                        u16::from_le_bytes([data[o], data[o + 1]]) as f32 / 65535.0
                                    } else {
                                        f32::from_le_bytes([
                                            data[o],
                                            data[o + 1],
                                            data[o + 2],
                                            data[o + 3],
                                        ])
                                    };
                                    lo = lo.min(d);
                                    hi = hi.max(d);
                                    if d >= 0.9999 {
                                        ones += 1;
                                    }
                                    let v = (d.clamp(0.0, 1.0) * 255.0) as u8;
                                    out.extend([v, v, v]);
                                }
                                let path = format!(
                                    "/tmp/nxport_frame_{}_tex_{:x}_slice{}_depth.ppm",
                                    pn, addr, sl
                                );
                                let _ = std::fs::write(&path, out);
                                println!("[+] Gfx: depth texture 0x{:x} slice {}: range {:.4}..{:.4}, {:.1}% at far plane", addr, sl, lo, hi, ones as f64 * 100.0 / (w * h) as f64);
                            }
                            continue;
                        }
                        // Cube and array textures: one file per face/slice and mip level through 2D views.
                        let slices = match t.textureType() {
                            MTLTextureType::TypeCube => 6,
                            MTLTextureType::Type2DArray => t.arrayLength(),
                            _ => 1,
                        };
                        if slices > 1 {
                            for lvl in 0..t.mipmapLevelCount() {
                                for sl in 0..slices {
                                    let Some(v) = (unsafe {
                                        t.newTextureViewWithPixelFormat_textureType_levels_slices(
                                            t.pixelFormat(),
                                            MTLTextureType::Type2D,
                                            objc2_foundation::NSRange::new(lvl, 1),
                                            objc2_foundation::NSRange::new(sl, 1),
                                        )
                                    }) else {
                                        continue;
                                    };
                                    let lv = if lvl == 0 {
                                        String::new()
                                    } else {
                                        format!("_l{}", lvl)
                                    };
                                    let path = format!(
                                        "/tmp/nxport_frame_{}_tex_{:x}_slice{}{}_{:?}.ppm",
                                        pn,
                                        addr,
                                        sl,
                                        lv,
                                        t.pixelFormat()
                                    );
                                    println!(
                                        "[+] Gfx: texture 0x{:x} slice {} level {} {:?}: {}",
                                        addr,
                                        sl,
                                        lvl,
                                        t.pixelFormat(),
                                        dump_any(&v, &path)
                                    );
                                }
                            }
                            continue;
                        }
                        let path = format!(
                            "/tmp/nxport_frame_{}_tex_{:x}_{:?}.ppm",
                            pn,
                            addr,
                            t.pixelFormat()
                        );
                        println!(
                            "[+] Gfx: texture 0x{:x} {:?} {:?}: {}",
                            addr,
                            t.textureType(),
                            t.pixelFormat(),
                            dump_any(t, &path)
                        );
                    }
                }
            }
            if !crate::settings::get().dump.dump_final_only {
                let st = g.state.lock().unwrap();
                println!(
                    "[+] Gfx: frame {} drew into {} color targets:",
                    pn,
                    st.frame_rts.len()
                );
                for (i, (_, t, n)) in st.frame_rts.iter().enumerate() {
                    let path = format!(
                        "/tmp/nxport_frame_{}_rt{:02}_{}x{}_{:?}.ppm",
                        pn,
                        i,
                        t.width(),
                        t.height(),
                        t.pixelFormat()
                    );
                    let summary = dump_any(t, &path);
                    println!(
                        "[+] Gfx:   rt{:02} {}x{} {:?} draws={} {}",
                        i,
                        t.width(),
                        t.height(),
                        t.pixelFormat(),
                        n,
                        summary
                    );
                }
            }
            {
                let mut st = g.state.lock().unwrap();
                for (name, buf, w, h, bpp) in std::mem::take(&mut st.depth_snapshots) {
                    let data = unsafe {
                        std::slice::from_raw_parts(
                            buf.contents().as_ptr() as *const u8,
                            w * h * bpp,
                        )
                    };
                    let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
                    let (mut lo, mut hi, mut far) = (f32::MAX, f32::MIN, 0usize);
                    for i in 0..w * h {
                        let o = i * bpp;
                        let d = if bpp == 2 {
                            u16::from_le_bytes([data[o], data[o + 1]]) as f32 / 65535.0
                        } else {
                            f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]])
                        };
                        lo = lo.min(d);
                        hi = hi.max(d);
                        if d >= 0.9999 {
                            far += 1;
                        }
                        let v = (d.clamp(0.0, 1.0) * 255.0) as u8;
                        out.extend([v, v, v]);
                    }
                    let _ =
                        std::fs::write(format!("/tmp/nxport_snap_{}_{}x{}.ppm", name, w, h), out);
                    println!("[+] Gfx:   depth snapshot {} {}x{}: range {:.4}..{:.4}, {:.1}% at far plane", name, w, h, lo, hi, far as f64 * 100.0 / (w * h) as f64);
                }
                for (name, t) in std::mem::take(&mut st.snapshots) {
                    let path = format!(
                        "/tmp/nxport_snap_{}_{}x{}_{:?}.ppm",
                        name,
                        t.width(),
                        t.height(),
                        t.pixelFormat()
                    );
                    println!("[+] Gfx:   snapshot {} {}", name, dump_any(&t, &path));
                }
            }
            if crate::settings::get().dump.exit_on_dump && Some(&pn) == dump_frames.iter().max() {
                println!("[+] Dump complete, exiting on request (--exit-on-dump)");
                std::process::exit(0);
            }
        }
        let mut st_guard = g.state.lock().unwrap();
        let st = &mut *st_guard;
        st.frame_rts.clear();
        st.tc.tex_check_budget_used = 0;
        translated::trim_view_cache(&mut st.tc.view_cache);
        sweep_shared_storage(&mut st.tc, &st.colors, st.depth.as_ref());
        st.root_memo.clear();
        if st.tc.alias_views.len() > 256 {
            st.tc.alias_views.clear();
            st.tc.alias_view_keys.clear();
        }
        if crate::settings::get().log.cmd_stats_at == Some(pn) {
            let mut v: Vec<_> = st.ignored_cmds.iter().map(|(k, n)| (*k, *n)).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            println!(
                "[cmd-stats] frame {}: {} kinds of recorded commands had no executor handler:",
                pn,
                v.len()
            );
            for (k, n) in v {
                println!("[cmd-stats]   {:6} x {}", n, k);
            }
        }
        st.ignored_cmds.clear();
        st.stats.passes += 1;
        {
            static PREV_GPU: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            static PREV_WAIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            static PREV_VSYNC: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let cur_gpu = GPU_BUSY_US.load(Ordering::Relaxed);
            let cur_wait = SUBMIT_WAIT_US.load(Ordering::Relaxed);
            let cur_vsync = crate::nvn::api::ACQUIRE_SLEEP_US.load(Ordering::Relaxed);
            let gpu_delta = cur_gpu.saturating_sub(PREV_GPU.swap(cur_gpu, Ordering::Relaxed));
            let wait_delta = cur_wait.saturating_sub(PREV_WAIT.swap(cur_wait, Ordering::Relaxed));
            let vsync_delta = cur_vsync.saturating_sub(PREV_VSYNC.swap(cur_vsync, Ordering::Relaxed));
            let (footprint, _) = process_memory_stats();
            crate::host::overlay::record_present(
                gpu_delta as f32 / 1000.0,
                wait_delta as f32 / 1000.0,
                vsync_delta as f32 / 1000.0,
                st.stats.submits,
                st.stats.draws,
                st.stats.passes,
                footprint,
            );
        }
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if n == 1 || n % 50 == 0 {
            let s = st.stats;
            // Frame rate over the last 50 presents (wall clock).
            static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
            let now = std::time::Instant::now();
            let fps = LAST
                .lock()
                .unwrap()
                .replace(now)
                .map_or(0.0, |t| 50.0 / now.duration_since(t).as_secs_f64());
            let gpu = GPU_BUSY_US.swap(0, std::sync::atomic::Ordering::Relaxed) as f64 / 50_000.0;
            let wait =
                SUBMIT_WAIT_US.swap(0, std::sync::atomic::Ordering::Relaxed) as f64 / 50_000.0;
            let acq = crate::nvn::api::ACQUIRE_CALLS.swap(0, std::sync::atomic::Ordering::Relaxed);
            let vsync = crate::nvn::api::ACQUIRE_SLEEP_US
                .swap(0, std::sync::atomic::Ordering::Relaxed) as f64
                / 50_000.0;
            let hashed = REFRESH_HASHED.swap(0, std::sync::atomic::Ordering::Relaxed) as f64
                / 50.0
                / 1048576.0;
            let (footprint, avail) = process_memory_stats();
            let mem_str = if avail > 0 {
                format!(
                    " mem {} MB (avail {} MB)",
                    footprint / 1048576,
                    avail / 1048576
                )
            } else if footprint > 0 {
                format!(" mem {} MB", footprint / 1048576)
            } else {
                String::new()
            };
            let vp = SCENE_VIEWPORT.swap(0, Ordering::Relaxed);
            println!("[+] Gfx: present #{} {:.1} fps gpu {:.1} ms/frame submit-wait {:.1} ms/frame vsync-wait {:.1} ms/frame ({} acquires) interval {} (submits {} clears {} draws-decoded {} passes {}) refresh-hashed {:.1} MB/frame{} scene {}x{}",
            n, fps, gpu, wait, vsync, acq, crate::nvn::api::present_interval(), s.submits, s.clears, s.draws, s.passes, hashed, mem_str, vp >> 20 & 0xFFFFF, vp & 0xFFFFF);
            // --log-sizes: sizes of the executor's caches every 50 frames (to find what grows over a session).
            static SIZES: OnceLock<bool> = OnceLock::new();
            if (n == 1 || n % 50 == 0)
                && *SIZES.get_or_init(|| crate::settings::get().log.log_sizes)
            {
                use objc2_metal::MTLResource;
                let mut tex_bytes = 0usize;
                let mut root_seen: rustc_hash::FxHashSet<usize> = Default::default();
                for t in st.tc.textures.values() {
                    let ptr = objc2::rc::Retained::as_ptr(t) as *const () as usize;
                    if root_seen.insert(ptr) {
                        tex_bytes += t.allocatedSize();
                    }
                }
                let mut alias_bytes = 0usize;
                for (r, v, _) in st.tc.alias_views.values() {
                    let ptr_r = objc2::rc::Retained::as_ptr(r) as *const () as usize;
                    if root_seen.insert(ptr_r) {
                        alias_bytes += r.allocatedSize();
                    }
                    let ptr_v = objc2::rc::Retained::as_ptr(v) as *const () as usize;
                    if root_seen.insert(ptr_v) {
                        alias_bytes += v.allocatedSize();
                    }
                }
                let shs = st.sh.clone();
                let shs = shs.lock().unwrap();
                println!("[sizes] #{} tex_bytes {} MB alias_bytes {} MB | textures {} tex_gens {} alias_of {} tex_by_desc {} tex_checked {} tex_sparse {} tex_track_gen {} view_cache {} gpu_written {} pool_buffers {} const_banks {} samplers {} repack {} ({} MB) refresh_memo {} alias_views {} rt_format_views {} bc_proxies {} pipelines {} {}",
                n, tex_bytes >> 20, alias_bytes >> 20,
                st.tc.textures.len(), st.tc.tex_gens.len(), st.tc.alias_of.len(), st.tc.tex_by_desc.len(), st.tc.tex_checked.len(), st.tc.tex_sparse.len(), st.tc.tex_track_gen.len(),
                st.tc.view_cache.len(), st.tc.gpu_written.len(), translated::pool_buffers_len(), shs.const_banks.len(), shs.samplers.len(), shs.repack_cache.len(), shs.repack_bytes >> 20, st.tc.refresh_memo.len(),
                st.tc.alias_views.len(), st.rt_format_views.len(), st.bc_proxies.len(), shs.translated_pipelines.len(), crate::nvn::api::debug_table_sizes());
            }
            if crate::host::thread_stats::enabled() {
                crate::host::thread_stats::report(n, 50);
            }
            if timeline_enabled() {
                use std::sync::atomic::Ordering::Relaxed;
                let ms = |a: &std::sync::atomic::AtomicU64| a.swap(0, Relaxed) as f64 / 50_000.0;
                println!("[timeline] #{} per frame: encode {:.1} ms (CPU {:.1}), gap between submits {:.1} ms, guest blocked in SyncWait {:.1} ms, GPU tail after last submit {:.1} ms; alias views {:.1}, new textures {:.1}, uploads {:.1}, root walks {:.1} (tag hits {:.1}), alias views created {:.1} (cached {}), binding views created {:.1} (cache clears {:.2}), ubo copies {:.0} reused {:.0} ({:.0} KB, {:.0} clears), feedback passes split {:.1}, issuer syncs {:.1} ({:.2} ms waiting), texture bindings fast {:.0} memo {:.0} resolved {:.0}, texture sets reused {:.0} resolved {:.0}, command lists {:.0}, uniform update ranges {:.0}, indexed copies {:.0}, texture memo hits {:.0} misses {:.0} / {:.0} after table changes, table changes {:.0}, slow resolutions {:.0} ({:.2} ms), alias roots re-initialized {:.1}, objects re-pointed {:.1}",
                n, ms(&TL_ENCODE_US), ms(&TL_ENCODE_CPU_US), ms(&TL_GAP_US), ms(&TL_SYNC_US), ms(&TL_TAIL_US),
                TEXSTAT[0].swap(0, Relaxed) as f64 / 50.0, TEXSTAT[1].swap(0, Relaxed) as f64 / 50.0, TEXSTAT[2].swap(0, Relaxed) as f64 / 50.0,
                translated::ROOT_STATS[1].swap(0, Relaxed) as f64 / 50.0, translated::ROOT_STATS[0].swap(0, Relaxed) as f64 / 50.0, ALIAS_NEW.swap(0, Relaxed) as f64 / 50.0, st.tc.alias_views.len(),
                translated::BINDING_VIEW_STATS[0].swap(0, Relaxed) as f64 / 50.0, translated::BINDING_VIEW_STATS[1].swap(0, Relaxed) as f64 / 50.0,
                translated::UBO_STATS[0].swap(0, Relaxed) as f64 / 50.0, translated::UBO_STATS[1].swap(0, Relaxed) as f64 / 50.0,
                translated::UBO_STATS[2].swap(0, Relaxed) as f64 / 50.0 / 1024.0, translated::UBO_STATS[3].swap(0, Relaxed) as f64 / 50.0, FB_INPLACE.swap(0, Relaxed) as f64 / 50.0,
                issuer::SYNC_STATS[0].swap(0, Relaxed) as f64 / 50.0, ms(&issuer::SYNC_STATS[1]),
                BIND_STATS[0].swap(0, Relaxed) as f64 / 50.0, BIND_STATS[1].swap(0, Relaxed) as f64 / 50.0, BIND_STATS[2].swap(0, Relaxed) as f64 / 50.0, translated::TEX_REUSE_STATS[0].swap(0, Relaxed) as f64 / 50.0, translated::TEX_REUSE_STATS[1].swap(0, Relaxed) as f64 / 50.0, LISTS_SEEN.swap(0, Relaxed) as f64 / 50.0, UBO_RANGES.swap(0, Relaxed) as f64 / 50.0, UBO_INDEXED.swap(0, Relaxed) as f64 / 50.0,
                TEX_MEMO_STATS[0].swap(0, Relaxed) as f64 / 50.0, TEX_MEMO_STATS[1].swap(0, Relaxed) as f64 / 50.0, TEX_MEMO_STATS[2].swap(0, Relaxed) as f64 / 50.0,
                {
                    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                    let e = crate::nvn::api::tex_table_epoch();
                    e.saturating_sub(LAST.swap(e, Relaxed)) as f64 / 50.0
                }, SLOW_RES[0].swap(0, Relaxed) as f64 / 50.0, SLOW_RES[1].swap(0, Relaxed) as f64 / 50.0 / 1e6,
                EPOCH_BUMPS[0].swap(0, Relaxed) as f64 / 50.0, EPOCH_BUMPS[1].swap(0, Relaxed) as f64 / 50.0);
            }
        }
    });
}

/// Uniform buffer copies by (guest address, length), with an address-ordered index so the executor's inline uniform
/// updates (~thousands per frame) drop the overlapping copies without scanning them all.
#[derive(Default)]
/// Keys are (guest address, copied length | zero-padding bytes << 32): a padded copy (translated::cbuf_zero_pad) is
/// valid only for bindings with the same bound size. Invalidation uses the low half (the guest bytes copied).
pub(super) struct UboCopies {
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

/// Frees Metal textures for memory that has since been reused for other textures: entries of `tex_by_desc` whose storage
/// address also backs another description, unused for SWEEP_AGE frames and not resolved by any live texture object.
/// Dynamic resolution (Kirby) re-creates its render targets at the same storage in new sizes every few seconds; without
/// this the old sizes stay allocated (memory on iOS). Not done at creation: TotK places several transient targets at one
/// storage address within every frame, and evicting the others there re-created ~36 textures per frame (some 1920x1080),
/// uploads and hashes included. On iOS (or with --tex-sweep-unshared; =0 turns it off there) textures alone at their
/// storage are freed too once unused for UNSHARED_AGE frames (areas, characters, cutscenes left behind), for memory.
/// Render targets bound at the present are never freed. --no-tex-sweep keeps everything.
fn sweep_shared_storage(tc: &mut TexCache, colors: &[RtRef], depth: Option<&RtRef>) {
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

/// Everything that determines a texture's Metal object and its guest memory layout.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TexDesc {
    storage: usize,
    target: i32,
    width: i32,
    height: i32,
    depth: i32,
    levels: i32,
    format: i32,
    flags: i32,
    samples: i32,
    stride: i64,
}

impl TexDesc {
    fn of(t: &NvnTexture) -> TexDesc {
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

/// --log-tex-mem: every 600 frames, where texture memory goes. Metal textures of the cache by kind (rendered or
/// cleared by the GPU vs copies of guest data), and how much guest memory under the copies is resident (what the guest
/// side of the duplication costs), plus Metal's total allocation.
fn tex_mem_report(g: &Gfx, pn: u64) {
    static ON: OnceLock<bool> = OnceLock::new();
    if pn % 600 != 0 || !*ON.get_or_init(|| crate::settings::get().log.log_tex_mem) {
        return;
    }
    let st = g.state.lock().unwrap();
    let mut seen = rustc_hash::FxHashSet::default();
    let (mut rt, mut rt_n, mut copy, mut copy_n, mut resident) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut vec = vec![0u8; 1 << 16];
    for (desc, (tex, _)) in st.tc.tex_by_desc.iter() {
        let id = Retained::as_ptr(tex) as *const () as usize;
        if !seen.insert(id) {
            continue;
        }
        let bytes = objc2_metal::MTLAllocation::allocatedSize(&**tex) as u64;
        if st.tc.gpu_written.contains(&storage_id(tex)) {
            rt += bytes;
            rt_n += 1;
            continue;
        }
        copy += bytes;
        copy_n += 1;
        // Resident 16 KiB pages of the guest storage under it (mincore).
        let start = desc.storage & !0x3fff;
        let end = (desc.storage + bytes as usize + 0x3fff) & !0x3fff;
        let pages = (end - start) / 0x4000;
        if desc.storage >= 0x10000 && pages > 0 && pages <= vec.len() {
            if unsafe {
                libc::mincore(
                    start as *mut libc::c_void,
                    end - start,
                    vec.as_mut_ptr() as *mut libc::c_char,
                )
            } == 0
            {
                resident += vec[..pages].iter().filter(|&&b| b & 1 != 0).count() as u64 * 0x4000;
            }
        }
    }
    let metal = g.device.currentAllocatedSize() as u64;
    println!("[tex-mem] f{}: Metal total {} MB; cached textures: GPU-written {} ({} MB), copies of guest data {} ({} MB) over {} MB of resident guest memory",
        pn, metal >> 20, rt_n, rt >> 20, copy_n, copy >> 20, resident >> 20);
}

/// --tex-backing (metal/tex_backing.rs): every 60 frames, the storage of textures whose content is a copy of guest data.
fn tex_backing_frame(g: &Gfx, pn: u64) {
    if tex_backing::mode() == crate::settings::TexBacking::Off {
        return;
    }
    tex_backing::frame_end(pn);
    if pn % 60 != 0 {
        return;
    }
    let texs: Vec<tex_backing::TexRec> = {
        let st = g.state.lock().unwrap();
        st.tc
            .tex_by_desc
            .iter()
            .filter(|(_, (tex, _))| !st.tc.gpu_written.contains(&storage_id(tex)))
            .filter_map(|(d, (tex, _))| {
                let p = crate::nvn::layout::TextureParams {
                    target: d.target,
                    width: d.width,
                    height: d.height,
                    depth: d.depth,
                    levels: d.levels,
                    format: d.format,
                    flags: d.flags,
                    samples: d.samples,
                    stride: d.stride,
                };
                // Only textures whose guest bytes can be rebuilt from Metal (others keep their pages out of the ranges).
                tex_backing::supported(tex, &p).then(|| tex_backing::TexRec {
                    storage: d.storage,
                    len: crate::nvn::layout::storage_size(&p),
                    params: p,
                    tex: tex.clone(),
                })
            })
            .collect()
    };
    tex_backing::rebuild(pn, texs);
}

/// Identity of a texture's storage: a view's parent, else the texture itself.
pub(crate) fn storage_id(t: &Tex) -> u64 {
    match t.parentTexture() {
        Some(p) => Retained::as_ptr(&p) as *const () as u64,
        None => Retained::as_ptr(t) as *const () as u64,
    }
}

/// --watch-storage=0x...: log render-target binds and copies into textures stored at that address.
/// Debug: --watch-size=WxH logs render-target binds, copies and clears into textures of that size (any storage):
/// finds the writer of a texture whose storage address changes from run to run.
fn watch_size() -> Option<(u32, u32)> {
    crate::settings::get().trace.watch_size.map(|s| (s.w, s.h))
}

/// GPU-rendered textures in guest memory. On the Switch the GPU writes render targets into memory the CPU shares, and
/// TotK's texture heap compaction moves textures with CPU copies (aglTextureDataMgr / tera::ImageResourceMgr): the
/// terrain normal maps it renders once at load were moved as zeros here, so the grass vertex shader saw flat-zero
/// normals and every grass fragment failed its alpha test (Hyrule Field nearly bare, issue 13). Exact emulation would
/// trap CPU reads of GPU-written pages (as Ryujinx does); this approximation copies a render target back into guest
/// memory one frame after it was drawn, but only targets not drawn in the 4 frames before (rendered once or rarely), so
/// per-frame and alternating targets cost nothing after their first use. 2D, single-sample, uncompressed color targets only.
/// --no-rt-writeback disables.
fn rt_writeback_enabled() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *E.get_or_init(|| !crate::settings::get().graphics.no_rt_writeback)
}

/// Writes last frame's write-back candidates into guest memory (their GPU work has completed: TotK waits for each
/// frame's fence before presenting the next).
/// Last command buffer holding draws into the render targets the next `writeback_pass` copies back.
static WRITEBACK_SERIAL: AtomicU64 = AtomicU64::new(0);

unsafe fn writeback_pass(g: &Gfx, st: &mut State) {
    let done = std::mem::replace(&mut st.writeback.1, std::mem::take(&mut st.writeback.0));
    // Everything the candidates now in writeback.1 were drawn in is committed (present synced the issuer).
    WRITEBACK_SERIAL.store(submitted_serial(), Ordering::Release);
    let mut n = 0;
    for (addr, _frame) in done {
        let Some((_, t)) = tex_info(addr) else {
            continue;
        };
        // Drawn again since (the game rendered it in the frame just submitted too): present waited for that rendering
        // to finish, so the write-back copies the newest contents now. Postponing it until the game stopped drawing the
        // target (dc6c9bf) wrote textures back long after the game had moved them (tera::ImageResourceMgr compacts its
        // terrain textures with CPU copies) into memory it had reused: SIGSEGV at main+0xbecbf8 on a list node full of
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
        // tera::ImageResourceMgr overwritten with texels, SIGSEGV at main+0xbecbf8 after ~7 minutes of play). On the
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
        if objc2_metal::MTLResource::storageMode(&*real) == MTLStorageMode::Private
            || real.sampleCount() > 1
            || crate::metal::copy::is_depth_stencil(real.pixelFormat())
            || real.textureType() != MTLTextureType::Type2D
        {
            continue;
        }
        for level in 0..(t.levels.max(1) as usize).min(real.mipmapLevelCount()) {
            let (w, h) = (
                ((t.width as usize) >> level).max(1),
                ((t.height as usize) >> level).max(1),
            );
            let bpp = info.bytes as usize;
            let mut buf = vec![0u8; w * h * bpp];
            let region = MTLRegion {
                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                size: MTLSize {
                    width: w,
                    height: h,
                    depth: 1,
                },
            };
            real.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                std::ptr::NonNull::new(buf.as_mut_ptr().cast()).unwrap(),
                w * bpp,
                region,
                level,
            );
            crate::nvn::api::guest_write_level(&t, level, &buf);
        }
        if crate::settings::get().log.log_rt_writeback {
            println!(
                "[writeback]   {}x{} fmt {} levels {} storage {:#x}",
                t.width, t.height, t.format, t.levels, t.storage
            );
        }
        n += 1;
    }
    if n > 0 && crate::settings::get().log.log_rt_writeback {
        println!(
            "[writeback] f{} wrote {} render target(s) back to guest memory",
            presented_frames(),
            n
        );
    }
}

fn watch_storage() -> Option<usize> {
    crate::settings::get().trace.watch_storage.map(|a| a.0)
}

/// Debug: --verify-tex records, per Metal texture, the guest storage, content hash and frame of its last upload;
/// the frame trace prints it next to each bound texture's current guest hash (stale or foreign uploads show up there).
pub(crate) fn verify_tex_enabled() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| crate::settings::get().log.verify_tex)
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
const HASH_UNKNOWN: u64 = u64::MAX;
/// Bytes fully hashed by `refresh_cpu_written` (printed per frame with the fps line).
static REFRESH_HASHED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Fast hash of a texture's guest bytes (detects new data streamed into reused memory).
unsafe fn guest_bytes_hash(t: &NvnTexture) -> u64 {
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
unsafe fn sparse_bytes_hash(t: &NvnTexture, layout: &crate::nvn::layout::TextureLayout) -> u64 {
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

/// Whether draws must record the frame's render targets (`frame_rts`): frame traces and full frame dumps use it.
pub(crate) fn want_frame_rts() -> bool {
    static DUMPS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    tracing_frame()
        || *DUMPS.get_or_init(|| {
            !crate::settings::get().dump.dump_frame.is_empty()
                && !crate::settings::get().dump.dump_final_only
        })
}

/// Largest viewport of a main 3D pass since the last fps line (see Cmd::SetViewport): area << 40 | w << 20 | h.
static SCENE_VIEWPORT: AtomicU64 = AtomicU64::new(0);

/// Draws encoded through translated shaders, on any encoding thread (each keeps its own EncodeCtx count).
pub(super) static TRANSLATED_DRAWS: AtomicU64 = AtomicU64::new(0);

/// Number of frames presented so far; draws recorded now belong to frame PRESENTED + 1.
static PRESENTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Frames presented so far (for deferring work until the GPU is done with a frame).
pub(super) fn draw_reuse_log() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().log.log_draw_reuse)
}

fn cmd_time_from() -> Option<u64> {
    static F: OnceLock<Option<u64>> = OnceLock::new();
    *F.get_or_init(|| crate::settings::get().log.log_cmd_time)
}

fn cmd_time_on() -> bool {
    let on = cmd_time_from().is_some_and(|f| (f..f + 60).contains(&presented_frames()));
    if on {
        let _ = CMD_TIME_GEN0.compare_exchange(
            0,
            crate::nvn::api::pools_gen(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        let _ = CMD_TIME_SGEN0.compare_exchange(
            0,
            crate::nvn::api::samplers_gen(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }
    on
}

/// horizon::hle::pools_gen() when the --log-cmd-time window began (how often pool-derived memos are invalidated).
static CMD_TIME_GEN0: AtomicU64 = AtomicU64::new(0);
static CMD_TIME_SGEN0: AtomicU64 = AtomicU64::new(0);

static CMD_TIME: Mutex<Option<std::collections::HashMap<&'static str, (u64, u64)>>> =
    Mutex::new(None);

fn note_cmd_time(name: &'static str, ns: u64) {
    let mut g = CMD_TIME.lock().unwrap();
    let e = g
        .get_or_insert_with(Default::default)
        .entry(name)
        .or_default();
    e.0 += ns;
    e.1 += 1;
}

/// Prints the --log-cmd-time table once its 60 frames are over (called at the present).
fn report_cmd_time() {
    let Some(f) = cmd_time_from() else { return };
    if presented_frames() != f + 60 {
        return;
    }
    let g = CMD_TIME.lock().unwrap();
    let mut rows: Vec<_> = g
        .as_ref()
        .map(|m| m.iter().map(|(k, v)| (*k, *v)).collect())
        .unwrap_or_default();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let total: u64 = rows
        .iter()
        .filter(|r| r.0 != "nvnCommandBufferCallCommands")
        .map(|r| r.1 .0)
        .sum();
    println!("[cmd-time] frames {}..{}: {:.2} ms per frame in commands (CallCommands excluded; it includes its list); pool generation changes {:.1}, sampler registrations {:.1} per frame",
        f, f + 60, total as f64 / 60.0 / 1e6, crate::nvn::api::pools_gen().saturating_sub(CMD_TIME_GEN0.load(Ordering::Relaxed)) as f64 / 60.0,
        crate::nvn::api::samplers_gen().saturating_sub(CMD_TIME_SGEN0.load(Ordering::Relaxed)) as f64 / 60.0);
    for (n, (ns, c)) in rows.iter().take(30) {
        println!(
            "[cmd-time] {:7.3} ms {:8.1} per frame ({:6.2} us each) {}",
            *ns as f64 / 60.0 / 1e6,
            *c as f64 / 60.0,
            *ns as f64 / (*c).max(1) as f64 / 1e3,
            n
        );
    }
}

pub fn presented_frames() -> u64 {
    PRESENTED.load(std::sync::atomic::Ordering::Relaxed)
}

/// --trace-frame=N: log every draw of frame N (render targets, shaders, textures, fallbacks).
pub(crate) fn tracing_frame() -> bool {
    // Fast path (called many times per draw): nothing configured and nothing triggered.
    static NONE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *NONE.get_or_init(|| crate::settings::get().trace.trace_frame.is_empty())
        && TRACE_TRIGGERED_FRAME.load(std::sync::atomic::Ordering::Relaxed) == 0
    {
        return false;
    }
    let frames = &crate::settings::get().trace.trace_frame;
    let cur = PRESENTED.load(std::sync::atomic::Ordering::Relaxed) + 1;
    let n = crate::settings::get().trace.trace_trigger_frames;
    let trig = TRACE_TRIGGERED_FRAME.load(std::sync::atomic::Ordering::Relaxed);
    (!frames.is_empty() && frames.contains(&cur)) || (trig != 0 && cur >= trig && cur < trig + n)
}

/// Query the current process memory footprint (phys_footprint) and available memory limit on Darwin/Apple Silicon.
pub fn process_memory_stats() -> (u64, u64) {
    #[cfg(target_vendor = "apple")]
    unsafe {
        #[repr(C)]
        struct TaskVMInfo {
            virtual_size: u64,
            region_count: i32,
            page_size: i32,
            resident_size: u64,
            resident_size_peak: u64,
            device: u64,
            device_peak: u64,
            internal: u64,
            internal_peak: u64,
            external: u64,
            external_peak: u64,
            reusable: u64,
            reusable_peak: u64,
            purgeable_volatile_pmap: u64,
            purgeable_volatile_resident: u64,
            purgeable_volatile_virtual: u64,
            compressed: u64,
            compressed_peak: u64,
            compressed_lifetime: u64,
            phys_footprint: u64,
        }
        extern "C" {
            fn mach_task_self() -> u32;
            fn task_info(
                task: u32,
                flavor: u32,
                task_info_out: *mut TaskVMInfo,
                task_info_outCnt: *mut u32,
            ) -> i32;
            fn os_proc_available_memory() -> usize;
        }
        let mut info: TaskVMInfo = std::mem::zeroed();
        let mut count = (std::mem::size_of::<TaskVMInfo>() / 4) as u32;
        let footprint = if task_info(mach_task_self(), 22, &mut info, &mut count) == 0 {
            info.phys_footprint
        } else {
            0
        };
        let avail = os_proc_available_memory() as u64;
        (footprint, avail)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        (0, 0)
    }
}

/// --trace-on-shader=<entry>: the frame after the first draw using that shader is traced (and dumped with
/// --dump-on-trigger), for content that appears at a different frame number in every run.
static TRACE_TRIGGERED_FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Set by request_capture: the triggered frame is also dumped (as with --dump-on-trigger).
static CAPTURE_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// F9 in the game window: trace the next frame's draws into the log and dump it to /tmp/nxport_frame_<n>.ppm.
/// Can be repeated; each press captures a new frame.
#[cfg(target_os = "macos")]
pub fn request_capture() {
    use std::sync::atomic::Ordering::Relaxed;
    let f = PRESENTED.load(Relaxed) + 2;
    CAPTURE_REQUESTED.store(true, Relaxed);
    TRACE_TRIGGERED_FRAME.store(f, Relaxed);
    HISTORY_DUMP.store(true, Relaxed);
    println!(
        "[+] Capture requested: frame {} (trace in the log, image /tmp/nxport_frame_{}.ppm)",
        f, f
    );
}
pub(crate) fn trace_trigger_check(entries: &[&str]) {
    let Some(want) = &crate::settings::get().trace.trace_on_shader else {
        return;
    };
    if TRACE_TRIGGERED_FRAME.load(std::sync::atomic::Ordering::Relaxed) == 0
        && entries.iter().any(|e| e == want)
    {
        let f = PRESENTED.load(std::sync::atomic::Ordering::Relaxed) + 2;
        TRACE_TRIGGERED_FRAME.store(f, std::sync::atomic::Ordering::Relaxed);
        println!("[+] Gfx: {} seen; tracing frame {}", want, f);
    }
}
pub(crate) fn trace_triggered_frame() -> u64 {
    TRACE_TRIGGERED_FRAME.load(std::sync::atomic::Ordering::Relaxed)
}

fn has_non_zero_pixel(tex: &Tex) -> bool {
    let (w, h) = (tex.width(), tex.height());
    let mut raw = vec![0u8; w * h * 4];
    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    };
    unsafe {
        tex.getBytes_bytesPerRow_fromRegion_mipmapLevel(
            std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
            w * 4,
            region,
            0,
        );
    }
    raw.chunks_exact(4)
        .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0)
}

/// Debug: writes any common color format as a PPM (floats clamped to 0..1) and returns a short value summary
/// ("empty" when every channel is zero). Private textures cannot be read back and are skipped.
fn dump_any(tex: &Tex, path: &str) -> String {
    if objc2_metal::MTLResource::storageMode(&**tex) == MTLStorageMode::Private {
        return "private (not dumped)".into();
    }
    let (w, h) = (tex.width(), tex.height());
    let f = tex.pixelFormat();
    let bpp = match f {
        MTLPixelFormat::RGBA16Float | MTLPixelFormat::RGBA16Unorm => 8,
        MTLPixelFormat::RGBA32Float => 16,
        MTLPixelFormat::R8Unorm => 1,
        MTLPixelFormat::RG8Unorm | MTLPixelFormat::R16Float => 2,
        MTLPixelFormat::RG16Float | MTLPixelFormat::R32Float => 4,
        _ => 4,
    };
    let mut raw = vec![0u8; w * h * bpp];
    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    };
    unsafe {
        // Slice 0 (array and cube textures need the slice form).
        tex.getBytes_bytesPerRow_bytesPerImage_fromRegion_mipmapLevel_slice(
            std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
            w * bpp,
            w * h * bpp,
            region,
            0,
            0,
        );
    }
    let half = |b: &[u8]| half_to_f32(u16::from_le_bytes([b[0], b[1]]));
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
    let (mut lo, mut hi, mut nonzero) = (f32::MAX, f32::MIN, 0usize);
    let mut nonfinite = 0usize;
    let dump_alpha = crate::settings::get().dump.dump_alpha;
    for px in raw.chunks_exact(bpp) {
        let rgb: [f32; 3] = match f {
            MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB => [
                px[2] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[0] as f32 / 255.0,
            ],
            MTLPixelFormat::RGBA16Float => [half(&px[0..]), half(&px[2..]), half(&px[4..])],
            MTLPixelFormat::RGBA16Unorm => [
                u16::from_le_bytes([px[0], px[1]]) as f32 / 65535.0,
                u16::from_le_bytes([px[2], px[3]]) as f32 / 65535.0,
                u16::from_le_bytes([px[4], px[5]]) as f32 / 65535.0,
            ],
            MTLPixelFormat::RGBA32Float => [f32le(&px[0..]), f32le(&px[4..]), f32le(&px[8..])],
            MTLPixelFormat::RG16Float => [half(&px[0..]), half(&px[2..]), 0.0],
            MTLPixelFormat::R16Float => {
                let v = half(px);
                [v, v, v]
            }
            MTLPixelFormat::R32Float => {
                let v = f32le(px);
                [v, v, v]
            }
            MTLPixelFormat::R8Unorm => {
                let v = px[0] as f32 / 255.0;
                [v, v, v]
            }
            MTLPixelFormat::RG8Unorm => [px[0] as f32 / 255.0, px[1] as f32 / 255.0, 0.0],
            MTLPixelFormat::RGB10A2Unorm => {
                let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
                [
                    (v & 1023) as f32 / 1023.0,
                    ((v >> 10) & 1023) as f32 / 1023.0,
                    ((v >> 20) & 1023) as f32 / 1023.0,
                ]
            }
            MTLPixelFormat::RG11B10Float => {
                let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
                [
                    uf11(v & 0x7ff),
                    uf11((v >> 11) & 0x7ff),
                    uf10((v >> 22) & 0x3ff),
                ]
            }
            // --dump-alpha: 8-bit RGBA targets are dumped as their alpha channel (greyscale).
            _ if dump_alpha && bpp == 4 => {
                let a = px[3] as f32 / 255.0;
                [a, a, a]
            }
            _ => [
                px[0] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[2] as f32 / 255.0,
            ],
        };
        for c in rgb {
            if c.is_finite() {
                lo = lo.min(c);
                hi = hi.max(c);
            }
        }
        if rgb.iter().any(|c| *c != 0.0) {
            nonzero += 1;
        }
        if rgb.iter().any(|c| !c.is_finite()) {
            nonfinite += 1;
        }
        out.extend(rgb.iter().map(|c| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8));
    }
    let _ = std::fs::write(path, out);
    let nf = if nonfinite > 0 {
        format!(", {} non-finite px", nonfinite)
    } else {
        String::new()
    };
    if nonzero == 0 {
        "empty".into()
    } else {
        format!(
            "{} nonzero px ({:.1}%), range {:.3}..{:.3}{}",
            nonzero,
            nonzero as f64 * 100.0 / (w * h) as f64,
            lo,
            hi,
            nf
        )
    }
}

fn half_to_f32(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    if e == 0 {
        s * m * 2f32.powi(-24)
    } else if e == 31 {
        if m == 0.0 {
            s * f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        s * (1.0 + m / 1024.0) * 2f32.powi(e - 15)
    }
}
fn uf11(v: u32) -> f32 {
    let (e, m) = ((v >> 6) as i32, (v & 0x3f) as f32);
    if e == 0 {
        m / 64.0 * 2f32.powi(-14)
    } else if e == 31 {
        f32::INFINITY
    } else {
        (1.0 + m / 64.0) * 2f32.powi(e - 15)
    }
}
fn uf10(v: u32) -> f32 {
    let (e, m) = ((v >> 5) as i32, (v & 0x1f) as f32);
    if e == 0 {
        m / 32.0 * 2f32.powi(-14)
    } else if e == 31 {
        f32::INFINITY
    } else {
        (1.0 + m / 32.0) * 2f32.powi(e - 15)
    }
}

/// Debug: write a texture as a binary PPM (BGRA/RGBA 8-bit formats only).
/// --glitch-scan=from:count[:min_pixels]: single-frame glitch detector for still-camera runs. For each presented
/// frame in range it waits for the GPU, reads the image back and compares the last five frames: pixels (on a
/// half-resolution grid) where the middle frame differs from both neighbours while the two frames on each side are
/// stable are outliers.
/// When more than min_pixels (default 200) are found, the three frames are written as
/// /tmp/nxport_glitch_<frame>_{prev,glitch,next}.ppm with the outlier bounding box logged ([glitch]). Frames are read
/// back asynchronously (no GPU wait). --exit-on-dump exits after the range.
fn glitch_scan(g: &Gfx, pn: u64, src: &Tex) {
    let Some(crate::settings::GlitchScan {
        from,
        count,
        min_px,
    }) = crate::settings::get().debug.glitch_scan
    else {
        return;
    };
    if pn < from || pn >= from + count {
        if pn == from + count && crate::settings::get().dump.exit_on_dump {
            println!(
                "[glitch] scan of frames {}..{} done, {} events, {} tint flashes, {} sky dips",
                from,
                from + count,
                GLITCH_EVENTS.load(Ordering::Relaxed),
                GLITCH_TINTS.load(Ordering::Relaxed),
                GLITCH_SKY.load(Ordering::Relaxed)
            );
            std::process::exit(0);
        }
        return;
    }
    // Asynchronous: the frame is blitted into a staging texture and read once that copy has completed (a later present),
    // so the scan adds no GPU wait and frame timing (including --early-fence overlap) stays as in normal runs.
    use objc2_metal::MTLBlitCommandEncoder;
    type Staged = (
        u64,
        Retained<ProtocolObject<dyn MTLTexture>>,
        Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    );
    struct Pending(
        std::collections::VecDeque<Staged>,
        Vec<Retained<ProtocolObject<dyn MTLTexture>>>,
    );
    unsafe impl Send for Pending {} // presenting thread only, under the mutex
    static PENDING: Mutex<Pending> =
        Mutex::new(Pending(std::collections::VecDeque::new(), Vec::new()));
    let mut pending = PENDING.lock().unwrap();
    let (w, h) = (src.width(), src.height());
    let tex = match pending.1.pop() {
        Some(t) if t.width() == w && t.height() == h && t.pixelFormat() == src.pixelFormat() => t,
        _ => {
            let desc = MTLTextureDescriptor::new();
            desc.setPixelFormat(src.pixelFormat());
            unsafe {
                desc.setWidth(w);
                desc.setHeight(h);
            }
            desc.setUsage(MTLTextureUsage::ShaderRead);
            desc.setStorageMode(MTLStorageMode::Shared);
            let Some(t) = g.device.newTextureWithDescriptor(&desc) else {
                return;
            };
            t
        }
    };
    let Some(cb) = g.queue.commandBuffer() else {
        return;
    };
    let Some(blit) = cb.blitCommandEncoder() else {
        return;
    };
    unsafe {
        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
            src, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: w, height: h, depth: 1 }, &tex, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 });
    }
    blit.endEncoding();
    cb.commit();
    pending.0.push_back((pn, tex, cb));
    while pending.0.front().is_some_and(|f| {
        matches!(
            f.2.status(),
            MTLCommandBufferStatus::Completed | MTLCommandBufferStatus::Error
        )
    }) {
        let (f, t, _) = pending.0.pop_front().unwrap();
        let mut raw = vec![0u8; w * h * 4];
        let region = MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: w,
                height: h,
                depth: 1,
            },
        };
        unsafe {
            t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                w * 4,
                region,
                0,
            )
        };
        pending.1.push(t);
        glitch_analyze(f, raw, w, h, src.pixelFormat(), min_px);
    }
}

fn glitch_analyze(pn: u64, raw: Vec<u8>, w: usize, h: usize, fmt: MTLPixelFormat, min_px: usize) {
    static RING: Mutex<Vec<(u64, Vec<u8>)>> = Mutex::new(Vec::new());
    let mut ring = RING.lock().unwrap();
    ring.push((pn, raw));
    if ring.len() > 5 {
        ring.remove(0);
    }
    if ring.len() < 5 || ring[0].0 + 4 != ring[4].0 {
        return;
    }
    // Middle frame f2 against two stable frames on each side: continuously animated pixels (particles, wind) fail the
    // stability test, a one-frame error on a still image passes.
    let (f0, f1, f2, f3, f4) = (&ring[0].1, &ring[1].1, &ring[2].1, &ring[3].1, &ring[4].1);
    let d =
        |a: &[u8], b: &[u8], i: usize| (0..3).map(|c| a[i + c].abs_diff(b[i + c])).max().unwrap();
    // Second test, tolerant of camera motion: pixels black in f2 while both neighbours are clearly brighter (the
    // black blobs and tiles seen in play).
    let mx = |a: &[u8], i: usize| a[i].max(a[i + 1]).max(a[i + 2]);
    let (mut n, mut nb, mut x0, mut y0, mut x1, mut y1) = (0usize, 0usize, w, h, 0, 0);
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let i = (y * w + x) * 4;
            let still = d(f1, f3, i) < 12
                && d(f0, f1, i) < 12
                && d(f3, f4, i) < 12
                && d(f2, f1, i) > 40
                && d(f2, f3, i) > 40;
            let black = mx(f2, i) < 6 && mx(f1, i) > 30 && mx(f3, i) > 30;
            if still || black {
                n += still as usize;
                nb += black as usize;
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    // Third test: whole-frame tint / exposure flashes (scene-wide parameters wrong for one frame). Mean colour of each
    // frame on a coarse grid; the middle one jumps away from both neighbours while they agree.
    let mean = |f: &[u8]| {
        let mut m = [0f64; 3];
        let mut k = 0f64;
        for y in (0..h).step_by(8) {
            for x in (0..w).step_by(8) {
                let i = (y * w + x) * 4;
                for c in 0..3 {
                    m[c] += f[i + c] as f64;
                }
                k += 1.0;
            }
        }
        m.map(|v| v / k)
    };
    let (m1, m2, m3) = (mean(f1), mean(f2), mean(f3));
    let tint = (0..3)
        .map(|c| {
            (
                (m2[c] - m1[c]).abs().min((m2[c] - m3[c]).abs()),
                (m1[c] - m3[c]).abs(),
            )
        })
        .filter(|&(jump, base)| jump > 4.0 && base < jump / 3.0)
        .map(|(jump, _)| jump)
        .fold(0.0, f64::max);
    if tint > 0.0 {
        let n = GLITCH_TINTS.fetch_add(1, Ordering::Relaxed);
        println!("[glitch] frame {}: tint flash, mean colour {:.1?} between {:.1?} and {:.1?} (jump {:.1})", ring[2].0, m2, m1, m3, tint);
        if n < 20 {
            let bgra = matches!(
                fmt,
                MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB
            );
            for (k, name) in ["prev", "glitch", "next"].iter().enumerate() {
                write_ppm_raw(
                    &ring[k + 1].1,
                    w,
                    h,
                    bgra,
                    &format!("/tmp/nxport_glitch_{}_{}.ppm", ring[2].0, name),
                );
            }
        }
    }
    // Fourth test: sky dips. The top third of the frame (the sky in most scenes) darkens or brightens for one frame while
    // its neighbours agree; TotK's env-cubemap update frames did this in 2026-10-13 sessions (~10 levels, one frame).
    // Logged and counted only ([glitch] sky dip).
    let sky = |f: &[u8]| {
        let (mut m, mut k) = (0f64, 0f64);
        for y in (0..h / 3).step_by(4) {
            for x in (0..w).step_by(4) {
                let i = (y * w + x) * 4;
                m += (f[i] as f64 + f[i + 1] as f64 + f[i + 2] as f64) / 3.0;
                k += 1.0;
            }
        }
        m / k.max(1.0)
    };
    let (s1, s2, s3) = (sky(f1), sky(f2), sky(f3));
    if (s2 - s1).abs() > 4.0
        && (s2 - s3).abs() > 4.0
        && (s1 - s3).abs() < 1.5
        && (s2 - s1).signum() == (s2 - s3).signum()
    {
        GLITCH_SKY.fetch_add(1, Ordering::Relaxed);
        println!(
            "[glitch] frame {}: sky dip {:.1} (top third {:.1}, neighbours {:.1} / {:.1})",
            ring[2].0,
            s2 - (s1 + s3) / 2.0,
            s2,
            s1,
            s3
        );
    }
    // Black flashes have their own dump budget (camera motion fills the outlier one).
    static BLACK_DUMPS: AtomicU64 = AtomicU64::new(0);
    let black_dump = nb > 200 && BLACK_DUMPS.fetch_add(1, Ordering::Relaxed) < 30;
    if n <= min_px && nb <= min_px && !black_dump {
        return;
    }
    let ev = if black_dump {
        0
    } else {
        GLITCH_EVENTS.fetch_add(1, Ordering::Relaxed)
    };
    // --glitch-dump-history: an event also dumps the frame history (with targets), at most every 120 frames.
    static LAST_DUMP: AtomicU64 = AtomicU64::new(0);
    if crate::settings::get().debug.glitch_dump_history
        && ring[2].0 > LAST_DUMP.load(Ordering::Relaxed) + 120
    {
        LAST_DUMP.store(ring[2].0, Ordering::Relaxed);
        HISTORY_DUMP.store(true, Ordering::Relaxed);
        println!("[glitch] frame {}: dumping the frame history", ring[2].0);
    }
    let mid = ring[2].0;
    println!("[glitch] frame {}: {} outlier / {} black-flash pixels (half-res grid), bbox ({},{})-({},{})", mid, n, nb, x0, y0, x1, y1);
    if ev < 30 {
        let bgra = matches!(
            fmt,
            MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB
        );
        for (k, name) in ["prev", "glitch", "next"].iter().enumerate() {
            write_ppm_raw(
                &ring[k + 1].1,
                w,
                h,
                bgra,
                &format!("/tmp/nxport_glitch_{}_{}.ppm", mid, name),
            );
        }
    }
}
static GLITCH_EVENTS: AtomicU64 = AtomicU64::new(0);
static GLITCH_TINTS: AtomicU64 = AtomicU64::new(0);
static GLITCH_SKY: AtomicU64 = AtomicU64::new(0);

/// --frame-history=N: every presented frame is copied on the GPU into a ring of N textures (a blit, no CPU wait).
/// F9 (request_capture) then also writes the ring, oldest first, as half-resolution images
/// /tmp/nxport_history_<press frame>/<frame>.ppm: single-frame glitches are gone before anyone can press a key.
/// The dump ranks the frames by single-frame outlier pixels (frame t differs from t-1 and t+1 while those agree) and
/// logs the top candidates ([history]); with a moving camera every frame scores, so compare scores, not absolute values.
/// --history-dump-at=<frame> dumps without a key.
fn frame_history(g: &Gfx, pn: u64, src: &Tex) {
    let n = history_len();
    if n == 0 {
        return;
    }
    // The presented frame's events (everything since the previous present).
    journal_rotate(pn);
    struct Ring(Vec<(u64, Retained<ProtocolObject<dyn MTLTexture>>)>);
    unsafe impl Send for Ring {} // only touched on the presenting thread, under the mutex
    static RING: Mutex<Ring> = Mutex::new(Ring(Vec::new()));
    let mut guard = RING.lock().unwrap();
    let ring = &mut guard.0;
    static AT: OnceLock<Option<u64>> = OnceLock::new();
    let at = *AT.get_or_init(|| crate::settings::get().dump.history_dump_at);
    if (HISTORY_DUMP.swap(false, Ordering::AcqRel) || at == Some(pn)) && !ring.is_empty() {
        if let Some(cb) = g.queue.commandBuffer() {
            cb.commit();
            cb.waitUntilCompleted();
        }
        let dir = format!("/tmp/nxport_history_{}", pn);
        let _ = std::fs::create_dir_all(&dir);
        let mut frames: Vec<&(u64, Retained<ProtocolObject<dyn MTLTexture>>)> =
            ring.iter().collect();
        frames.sort_by_key(|f| f.0);
        let mut halves: Vec<(u64, Vec<u8>)> = Vec::new();
        for (f, t) in &frames {
            let (w, h) = (t.width(), t.height());
            let mut raw = vec![0u8; w * h * 4];
            let region = MTLRegion {
                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                size: MTLSize {
                    width: w,
                    height: h,
                    depth: 1,
                },
            };
            unsafe {
                t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                    std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                    w * 4,
                    region,
                    0,
                )
            };
            let bgra = matches!(
                t.pixelFormat(),
                MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB
            );
            let flip = crate::nvn::api::window_origin_mode() == 0;
            // Ring frames are stored at half resolution already (see below).
            let (hw, hh) = (w, h);
            let mut out = format!("P6\n{} {}\n255\n", hw, hh).into_bytes();
            for y in 0..hh {
                let row = if flip { h - 1 - y } else { y };
                for x in 0..hw {
                    let px = &raw[(row * w + x) * 4..][..4];
                    if bgra {
                        out.extend_from_slice(&[px[2], px[1], px[0]]);
                    } else {
                        out.extend_from_slice(&px[..3]);
                    }
                }
            }
            let hdr = out.len() - hw * hh * 3;
            halves.push((*f, out[hdr..].to_vec()));
            let _ = std::fs::write(format!("{}/{:06}.ppm", dir, f), out);
        }
        let mut scores: Vec<(usize, u64)> = Vec::new();
        for k in 1..halves.len().saturating_sub(1) {
            let (a, b, c) = (&halves[k - 1].1, &halves[k].1, &halves[k + 1].1);
            let d = |x: &[u8], y: &[u8], i: usize| {
                (0..3).map(|j| x[i + j].abs_diff(y[i + j])).max().unwrap()
            };
            let n = (0..b.len() / 3)
                .filter(|p| {
                    let i = p * 3;
                    d(a, c, i) < 16 && d(b, a, i) > 40 && d(b, c, i) > 40
                })
                .count();
            scores.push((n, halves[k].0));
        }
        let mut sorted = scores.clone();
        sorted.sort_unstable();
        let median = sorted.get(sorted.len() / 2).map_or(0, |s| s.0);
        sorted.reverse();
        println!(
            "[history] single-frame outlier pixels: median {}, top: {}",
            median,
            sorted
                .iter()
                .take(8)
                .map(|(n, f)| format!("f{} {}", f, n))
                .collect::<Vec<_>>()
                .join(", ")
        );
        journal_write(&dir);
        history_targets_write(&dir);
        println!(
            "[+] Frame history: wrote {} frames ({}..{}) and their journal to {}",
            frames.len(),
            frames[0].0,
            frames[frames.len() - 1].0,
            dir
        );
    }
    // Frames are kept at half resolution, drawn with the present's blit pipeline (linear filtering: each pixel the average
    // of a 2x2 block, the bytes the window shows), in memory row order. Full-size copies made the 360-frame ring of play
    // sessions ~3 GB of Metal memory (2026-10-16); captures were written at half resolution anyway.
    let (hw, hh) = ((src.width() / 2).max(1), (src.height() / 2).max(1));
    let slot = if ring.len() >= n {
        let i = ring
            .iter()
            .enumerate()
            .min_by_key(|(_, f)| f.0)
            .map(|(i, _)| i)
            .unwrap();
        let (_, t) = ring.remove(i);
        (t.width() == hw && t.height() == hh).then_some(t)
    } else {
        None
    };
    let dst = match slot {
        Some(t) => t,
        None => {
            let desc = MTLTextureDescriptor::new();
            desc.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            unsafe {
                desc.setWidth(hw);
                desc.setHeight(hh);
            }
            desc.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
            desc.setStorageMode(MTLStorageMode::Shared);
            let Some(t) = g.device.newTextureWithDescriptor(&desc) else {
                return;
            };
            t
        }
    };
    let Some(cb) = g.queue.commandBuffer() else {
        return;
    };
    let pass = MTLRenderPassDescriptor::renderPassDescriptor();
    unsafe {
        let att = pass.colorAttachments().objectAtIndexedSubscript(0);
        att.setTexture(Some(&dst));
        att.setLoadAction(MTLLoadAction::DontCare);
        att.setStoreAction(MTLStoreAction::Store);
    }
    let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) else {
        return;
    };
    enc.setRenderPipelineState(&g.blit);
    let crop_uv: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    unsafe {
        enc.setFragmentTexture_atIndex(Some(src), 0);
        enc.setFragmentBytes_length_atIndex(
            std::ptr::NonNull::new(crop_uv.as_ptr() as *mut libc::c_void).unwrap(),
            16,
            0,
        );
        enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
        // blit_vs: one full-screen triangle
    }
    enc.endEncoding();
    cb.commit();
    ring.push((pn, dst));
    history_targets_capture(g, pn);
}

/// Target history (--history-targets=WxH with --frame-history): the render targets of that size drawn in each of
/// the last --history-targets-n frames (default 12), copied at the present; written as raw files with the frame
/// history (targets/<frame>_<storage>_<w>x<h>_fmt<metal format>.raw). Shows which intermediate a glitch first appears in.
type TargetRing =
    std::collections::VecDeque<(u64, Vec<(usize, Retained<ProtocolObject<dyn MTLTexture>>)>)>;
struct TargetHistory(TargetRing, Vec<Retained<ProtocolObject<dyn MTLTexture>>>);
unsafe impl Send for TargetHistory {}
static TARGET_HISTORY: Mutex<TargetHistory> =
    Mutex::new(TargetHistory(std::collections::VecDeque::new(), Vec::new()));

fn history_targets_capture(g: &Gfx, pn: u64) {
    use objc2_metal::MTLBlitCommandEncoder;
    if translated::history_target_size().is_none() {
        return;
    }
    let n: usize = crate::settings::get().dump.history_targets_n;
    let targets = std::mem::take(&mut translated::FRAME_TARGETS.lock().unwrap().0);
    let mut h = TARGET_HISTORY.lock().unwrap();
    while h.0.len() >= n {
        let (_, old) = h.0.pop_front().unwrap();
        h.1.extend(old.into_iter().map(|(_, t)| t));
    }
    let Some(cb) = g.queue.commandBuffer() else {
        return;
    };
    let Some(blit) = cb.blitCommandEncoder() else {
        return;
    };
    let mut copies = Vec::new();
    for (storage, src) in targets {
        let pos = h.1.iter().position(|t| {
            t.width() == src.width()
                && t.height() == src.height()
                && t.pixelFormat() == src.pixelFormat()
        });
        let dst = match pos {
            Some(i) => h.1.swap_remove(i),
            None => {
                let desc = MTLTextureDescriptor::new();
                desc.setPixelFormat(src.pixelFormat());
                unsafe {
                    desc.setWidth(src.width());
                    desc.setHeight(src.height());
                }
                desc.setUsage(MTLTextureUsage::ShaderRead);
                desc.setStorageMode(MTLStorageMode::Shared);
                let Some(t) = g.device.newTextureWithDescriptor(&desc) else {
                    continue;
                };
                t
            }
        };
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                &src, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: src.width(), height: src.height(), depth: 1 }, &dst, 0, 0,
                MTLOrigin { x: 0, y: 0, z: 0 });
        }
        copies.push((storage, dst));
    }
    blit.endEncoding();
    cb.commit();
    h.0.push_back((pn, copies));
}

fn history_targets_write(dir: &str) {
    let h = TARGET_HISTORY.lock().unwrap();
    if h.0.is_empty() {
        return;
    }
    let tdir = format!("{}/targets", dir);
    let _ = std::fs::create_dir_all(&tdir);
    for (f, list) in &h.0 {
        for (storage, t) in list {
            let bpp = match t.pixelFormat().0 {
                10 => 1,
                20 | 25 | 30 => 2,
                105 | 115 => 8,
                125 => 16,
                _ => 4,
            };
            let (w, hh) = (t.width(), t.height());
            let mut raw = vec![0u8; w * hh * bpp];
            let region = MTLRegion {
                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                size: MTLSize {
                    width: w,
                    height: hh,
                    depth: 1,
                },
            };
            unsafe {
                t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                    std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                    w * bpp,
                    region,
                    0,
                )
            };
            let _ = std::fs::write(
                format!(
                    "{}/{:06}_{:x}_{}x{}_fmt{}.raw",
                    tdir,
                    f,
                    storage,
                    w,
                    hh,
                    t.pixelFormat().0
                ),
                raw,
            );
        }
    }
}
static HISTORY_DUMP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn history_len() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| crate::settings::get().dump.frame_history)
}

/// Frame journal (with --frame-history): compact engine events per frame (render passes with their targets' Metal
/// identities, load / clear actions and draw counts, clears, texture creations and uploads, compute dispatches,
/// shader fallbacks), kept for the same frames as the image ring and written with it as journal.txt. The glitch frame's
/// journal against its neighbours' shows what the engine did differently in that frame.
static JOURNAL: Mutex<(Vec<String>, std::collections::VecDeque<(u64, Vec<String>)>)> =
    Mutex::new((Vec::new(), std::collections::VecDeque::new()));

#[inline]
pub(crate) fn journal_on() -> bool {
    history_len() > 0
}

pub(crate) fn journal(line: impl FnOnce() -> String) {
    if journal_on() {
        let l = line();
        JOURNAL.lock().unwrap().0.push(l);
    }
}

fn journal_rotate(pn: u64) {
    if let Some(g) = translated::FRAME_RT_ROOTS.lock().unwrap().as_mut() {
        *translated::PREV_RT_ROOTS.lock().unwrap() = Some(std::mem::take(&mut g.0));
        g.1.clear();
    }
    let mut j = JOURNAL.lock().unwrap();
    let cur = std::mem::take(&mut j.0);
    j.1.push_back((pn, cur));
    while j.1.len() > history_len() {
        j.1.pop_front();
    }
}

fn journal_write(dir: &str) {
    let j = JOURNAL.lock().unwrap();
    let mut out = String::new();
    for (f, lines) in &j.1 {
        out.push_str(&format!("== frame {} ({} events)\n", f, lines.len()));
        for l in lines {
            out.push_str(l);
            out.push('\n');
        }
    }
    let _ = std::fs::write(format!("{}/journal.txt", dir), out);
}

pub(crate) fn tex_id(t: &Tex) -> usize {
    Retained::as_ptr(t) as *const _ as *const u8 as usize
}

fn write_ppm_raw(raw: &[u8], w: usize, h: usize, bgra: bool, path: &str) {
    let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
    let flip = crate::nvn::api::window_origin_mode() == 0;
    for y in 0..h {
        let row = if flip { h - 1 - y } else { y };
        for px in raw[row * w * 4..(row + 1) * w * 4].chunks_exact(4) {
            if bgra {
                out.extend_from_slice(&[px[2], px[1], px[0]]);
            } else {
                out.extend_from_slice(&px[..3]);
            }
        }
    }
    let _ = std::fs::write(path, out);
}

fn dump_ppm(tex: &Tex, path: &str) {
    let (w, h) = (tex.width(), tex.height());
    let mut raw = vec![0u8; w * h * 4];
    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    };
    unsafe {
        tex.getBytes_bytesPerRow_fromRegion_mipmapLevel(
            std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
            w * 4,
            region,
            0,
        );
    }
    let bgra = matches!(
        tex.pixelFormat(),
        MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB
    );
    let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
    // Rows as the screen shows them: bottom-up in memory for NVN's LOWER_LEFT window origin (see present).
    let flip = crate::nvn::api::window_origin_mode() == 0;
    for y in 0..h {
        let row = if flip { h - 1 - y } else { y };
        for px in raw[row * w * 4..(row + 1) * w * 4].chunks_exact(4) {
            if bgra {
                out.extend_from_slice(&[px[2], px[1], px[0]]);
            } else {
                out.extend_from_slice(&px[..3]);
            }
        }
    }
    let _ = std::fs::write(path, out);
    println!("[+] Gfx: dumped {}x{} frame to {}", w, h, path);
}

/// --dump-main-image=path[:frame] (default frame 300): writes the main module's .text + .rodata as they are in memory
/// at that frame (plus .data and .bss), to diff against a run without mods and list what mods (exlaunch hooks) patched.
fn dump_main_image() {
    let Some(crate::settings::FrameFile { path, frame }) =
        &crate::settings::get().dump.dump_main_image
    else {
        return;
    };
    if presented_frames() != *frame {
        return;
    }
    let base = crate::cpu::crash_handler::get_guest_base();
    // .text at 0, .rodata at 0x2B18000, .data at 0x41A7000, .bss at 0x466D000 (guest/loader.rs).
    let bytes = unsafe { std::slice::from_raw_parts(base as *const u8, 0x0466_D000 + 761_888) };
    match std::fs::write(path, bytes) {
        Ok(()) => println!(
            "[+] main image (.text to .bss, {} bytes) written to {}",
            bytes.len(),
            path.display()
        ),
        Err(e) => println!("[!] main image dump to {} failed: {}", path.display(), e),
    }
}
