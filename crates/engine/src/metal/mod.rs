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

mod debug;
pub(crate) use debug::*;
mod replay;
pub(crate) use replay::*;
mod present;
pub use present::*;
mod frame_stats;
pub use frame_stats::*;
mod fence;
pub use fence::*;
mod inflight;
pub use inflight::*;
mod occlusion;
pub(crate) use occlusion::*;
mod submit;
pub use submit::*;
mod commands;
mod encoders;
mod textures;
pub(crate) mod backend;
mod batch;
mod copy;
mod fence_guard;
mod issuer;
pub mod msl_specialize;
mod pass_graph;
mod pass_plan;
pub mod source_pack;
mod pass_prof;
pub(crate) mod pipeline_archive;
mod programs;
mod readback;
pub(crate) use crate::gpu::report_guard;
pub(crate) mod rt_guard;
pub mod shader_cache;
mod tex_backing;
mod tex_heap;
mod tile_feedback;
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
    /// The targets' NVN formats (texture, view; pipeline lists) and sample count.
    pub(super) nvn_colors: [(u32, u32); 8],
    pub(super) nvn_depth: (u32, u32),
    pub(super) samples: u32,
    pub(super) enc_key: smallvec::SmallVec<[u64; 36]>,
    pub(super) target_size: (f64, f64),
    /// Block-compressed targets rendered through proxies: (proxy, target, level), see `bc_proxy`.
    pub(super) bc: smallvec::SmallVec<[(Tex, Tex, usize); 1]>,
}

pub(crate) struct State {
    colors: Vec<RtRef>,
    /// Per color slot: the earlier slot whose surface it repeats (the slot itself is left unattached), see
    /// SetRenderTargets.
    color_dups: Vec<Option<usize>>,
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
    /// nvnCommandBufferSetRasterizerDiscard: draws make no fragments, so they are skipped.
    rasterizer_discard: bool,
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
    /// Partial / masked texture clears: pipelines by (pixel format, channel mask) (copy.rs clear_region).
    clear_pipes: HashMap<(usize, u8), Retained<ProtocolObject<dyn MTLRenderPipelineState>>>,
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
    /// The last two frames each render target (by storage id) began a pass in, the latest first, and the frame these
    /// were last pruned in: whether the current pass's targets are drawn every frame (`enc_recur`).
    pub(super) rt_frames: HashMap<u64, (u64, u64)>,
    pub(super) rt_frames_pruned: u64,
    /// One of the current render encoder's targets was drawn in the previous frame too: a draw skipped there while its
    /// pipeline compiles is drawn again next frame. Elsewhere (a target drawn now and then) its draw waits.
    pub(super) enc_recur: bool,
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
    /// Every sampler state made, by descriptor: one Metal object per distinct sampler (samplers that can be used
    /// through argument buffers are a limited device resource, and pool slots repeat descriptors).
    pub(super) sampler_states: Vec<(crate::nvn::api::SamplerDesc, Retained<ProtocolObject<dyn MTLSamplerState>>)>,
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
    crate::gpu::pipeline_list::fetch();
    pipeline_archive::preload(&device);
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
        // 16-bit packed formats. They fell through to RGBA8: uploads of their 2-byte texels were rejected and the
        // textures stayed empty (Mario Kart 8 Deluxe's cup emblems and countdown, RGB565, drew black). NVN names them
        // from the low bits (red lowest); Metal has no such layouts, so sampling reorders the channels
        // (translated::packed_channels).
        57 => (MTLPixelFormat::ABGR4Unorm, None),       // RGBA4
        58 | 59 => (MTLPixelFormat::BGR5A1Unorm, None), // RGB5, RGB5A1
        60 => (MTLPixelFormat::B5G6R5Unorm, None),      // RGB565
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
        let l = std::sync::Arc::new((*crate::nvn::layout::compute_layout(&p)).clone());
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
    let now = crate::platform::clock::now_ns();
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

/// The clear depth Metal is given for `depth` on a target of format `pf`. On Apple GPUs a Depth16Unorm clear truncates
/// (0.5 stores 32767) while rasterized depth rounds (0.5 stores 32768), so a draw at the cleared depth failed an EQUAL test
/// against it. TotK clears two D16 targets to 0.5 each frame and then selects regions with EQUAL tests; with the clear
/// truncated, pixels left at the cleared depth fail them (blocks in Ultrahand / Fuse effects, totk-mobile PRs 5 and 9). Given the f32 the game passes,
/// rounded half up as the rasterizer does (a standalone Metal test on the M4: all 201 values 0..1 match a draw at that
/// depth, against 4 of 5 failing before), then nudged into the step so the truncation keeps it.
pub(super) fn clear_depth_value(pf: MTLPixelFormat, depth: f32) -> f64 {
    if pf != MTLPixelFormat::Depth16Unorm {
        return depth as f64;
    }
    let k = (depth.clamp(0.0, 1.0) as f64 * 65535.0).round();
    ((k + 0.25) / 65535.0).min(1.0)
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

/// Guest memory of the textures --watch-size has seen; watch_log reports everything else that touches it (holds,
/// write-backs, ended holds, engine reads of held pages, cache evictions), so a texture's whole history is in the log.
static WATCH_MEM: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

/// Remembers `t`'s memory when it has the --watch-size size; true if it does.
pub(crate) fn watch_texture(t: &NvnTexture) -> bool {
    let Some((w, h)) = watch_size() else {
        return false;
    };
    if t.width as u32 != w || t.height as u32 != h || t.storage <= 0x10000 {
        return false;
    }
    let range = (t.storage, t.storage + layout_of(t).total as usize);
    let mut m = WATCH_MEM.lock().unwrap();
    if !m.contains(&range) {
        if m.len() >= 8192 {
            m.remove(0);
        }
        m.push(range);
    }
    true
}

pub(crate) fn watch_log(start: usize, end: usize, what: impl FnOnce() -> String) {
    if watch_size().is_none() || !WATCH_MEM.lock().unwrap().iter().any(|&(s, e)| s < end && e > start) {
        return;
    }
    println!("[watch-size] f{} {}", presented_frames() + 1, what());
}

/// Non-zero bytes of [addr, addr+len) read without disturbing holds (through their second mappings).
unsafe fn watch_nonzero(addr: usize, len: usize) -> usize {
    match rt_guard::guest_copy::<u8>(addr, len) {
        Some(v) => v.iter().filter(|&&b| b != 0).count(),
        None => std::slice::from_raw_parts(addr as *const u8, len).iter().filter(|&&b| b != 0).count(),
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
    crate::gpu::history::request();
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


fn apply_vertex_input(st: &mut State, v: &crate::nvn::cmd::VertexInput) {
    st.vertex_attribs = v.attribs;
    st.vertex_streams.clone_from(&v.streams);
    for &(i, a, n) in &v.buffers {
        st.vertex_buffers.insert(i, (a, n));
    }
}

fn apply_render_state(st: &mut State, r: &crate::nvn::cmd::RenderState) {
    for &b in &r.blend {
        st.blend_states[(b & 7) as usize] = b;
    }
    st.color_state = r.color;
    st.channel_mask = r.channel_mask;
    st.blend_color = r.blend_color;
    st.depth_stencil = r.depth_stencil;
    if let Some((mask, reference)) = r.stencil {
        st.stencil = [[reference; 2], [mask; 2], [0xff; 2]];
    }
    st.polygon = r.polygon;
    st.polygon_offset = r.polygon_offset;
    st.depth_clamp = r.depth_clamp;
}

fn apply_model_bind(st: &mut State, stage: i32, index: i32, kind: crate::nvn::cmd::ModelBindKind) {
    use crate::nvn::cmd::ModelBindKind;
    match kind {
        ModelBindKind::Uniform { addr, size } => {
            st.uniform_buffers.insert((stage, index), (addr, size));
        }
        ModelBindKind::Storage { addr, size } => {
            if (0..6).contains(&stage) && (0..16).contains(&index) {
                st.storage_buffers[stage as usize][index as usize] = (addr, size);
            }
        }
        ModelBindKind::Texture(handle) => {
            st.textures_bound.insert((stage, index), handle);
        }
        ModelBindKind::Vertex { addr, size } => {
            st.vertex_buffers.insert(index, (addr, size));
        }
    }
}

fn apply_program(st: &mut State, program: usize, stages: i32) {
    // NVN_SHADER_STAGE_COMPUTE_BIT: compute and graphics programs are bound independently.
    if stages & 0x20 != 0 {
        st.compute_program = Some(program);
    }
    if stages & 0x1F != 0 || stages == 0 {
        st.program = Some(program);
    }
}

unsafe fn apply_textures_deferred(st: &mut State, stage: i32, first: i32, count: i32, handles: u64) {
    for i in 0..count.clamp(0, 32) {
        let h = if handles > 0x10000 { std::ptr::read_unaligned((handles as *const u64).add(i as usize)) } else { 0 };
        st.textures_bound.insert((stage, first + i), h);
    }
}

fn set_viewport(st: &mut State, x: i32, y: i32, w: i32, h: i32) {
    st.viewport = Some((x, y, w, h));
    // The main 3D passes (3+ colour targets: the G-buffer) show the game's render resolution (its dynamic resolution
    // scales the viewport): reported with the fps line.
    if st.colors.len() >= 3 && w > 0 && h > 0 {
        // Largest since the last report (area in the high bits).
        let (w, h) = ((w as u64).min(0xFFFFF), (h as u64).min(0xFFFFF));
        SCENE_VIEWPORT.fetch_max((w * h) << 40 | w << 20 | h, Ordering::Relaxed);
    }
}

fn apply_fixed(st: &mut State, f: crate::nvn::cmd::FixedState) {
    use crate::nvn::cmd::FixedState;
    match f {
        FixedState::Blend(v) => st.blend_states[(v & 7) as usize] = v,
        FixedState::Color(v) => st.color_state = v,
        FixedState::ChannelMask(v) => st.channel_mask = v,
        FixedState::BlendColor(c) => st.blend_color = c,
        FixedState::DepthStencil(v) => st.depth_stencil = v,
        FixedState::Stencil { kind, faces, value } => {
            // NVNface: FRONT 1, BACK 2, FRONT_AND_BACK 3.
            let k = (kind as usize).min(2);
            if faces & 1 != 0 {
                st.stencil[k][0] = value as u8;
            }
            if faces & 2 != 0 {
                st.stencil[k][1] = value as u8;
            }
        }
        FixedState::DepthBounds { enable, min, max } => st.depth_bounds = enable.then_some((min, max)),
        FixedState::Polygon(v) => st.polygon = v,
        FixedState::PolygonOffset { factor, units, clamp } => st.polygon_offset = (factor, units, clamp),
        FixedState::Multisample(v) => st.multisample = v,
        FixedState::DepthClamp(v) => st.depth_clamp = v,
        FixedState::Scissor { x, y, w, h } => st.scissor = Some((x, y, w, h)),
        FixedState::Viewport { x, y, w, h } => set_viewport(st, x, y, w, h),
        FixedState::DepthRange { near, far } => st.depth_range = (near, far),
    }
}

unsafe fn update_uniform(st: &mut State, addr: u64, offset: isize, data: &[u8]) {
    if addr > 0x10000 && !data.is_empty() {
        let dest = (addr as isize + offset) as *mut u8;
        std::ptr::copy_nonoverlapping(data.as_ptr(), dest, data.len());
        // Plan-side copies and encode-side copies overlapping the written range are stale now.
        let (lo, hi) = (dest as u64, dest as u64 + data.len() as u64);
        st.plan_ubo.invalidate(lo, hi);
        st.cx.ubo_copies.invalidate(lo, hi);
        st.ubo_plan_epoch += 1;
        GUEST_WRITES.fetch_add(1, Ordering::Relaxed);
    }
}
