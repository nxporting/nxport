// Draw path that runs the game's own shaders (translated ahead of time, see nxport-builder shaders build and
// metal/shader_cache.rs).
//
// What it maps (conventions in the project notes; sources: Ryujinx Graphics.Shader, deko3d):
//   - vertex attributes   NVN attrib/stream state -> MTLVertexDescriptor; the attribute class (float / uint /
//                         sint) is passed to the pipeline as function constants, so the shader sees the raw
//                         32-bit words the Switch's fetch hardware would deliver
//   - fragment outputs    same idea from the render target formats
//   - cbuf 0              driver cbuf: base vertex / base instance / draw index at 0x640 / 0x644 / 0x648
//   - cbuf 1              the stage's compiler-generated constant bank (inside the stage data, after the code)
//   - cbuf N+3            user uniform buffer N
//   - textures            texture handle table (cbuf 2, 8-byte entries from byte 0x20) -> BindTexture index
//   - clip space          the vertex stage negates y and remaps z (metal/msl_specialize.rs); the winding flips
//                         accordingly
//
// Guest memory is wrapped, not copied: each NVN memory pool becomes one no-copy MTLBuffer (see `pool_buffer`).
// A draw it cannot express returns false and is skipped.

use super::*;

mod buffers;
mod states;
mod binding;
mod compute;
mod tiny;
mod plan;
mod encode;
mod pipeline_build;

use objc2_foundation::NSURL;
use objc2_metal::{
    MTLBuffer, MTLColorWriteMask, MTLCompareFunction, MTLCullMode, MTLDataType,
    MTLDepthStencilDescriptor, MTLDepthStencilState, MTLFunction, MTLFunctionConstantValues,
    MTLSamplerAddressMode, MTLSamplerBorderColor, MTLSamplerDescriptor, MTLSamplerMinMagFilter,
    MTLSamplerMipFilter, MTLSamplerState, MTLTextureSwizzle, MTLTextureSwizzleChannels,
    MTLTextureType, MTLTriangleFillMode, MTLVertexDescriptor, MTLVertexFormat,
    MTLVertexStepFunction, MTLWinding,
};

use crate::metal::shader_cache::{CachedStage, StageKind, TexBinding};

const STAGE_VERTEX: i32 = 0;
const STAGE_FRAGMENT: i32 = 1;

/// Size of Ryujinx's SupportBuffer (FragmentAlphaTest, FragmentIsBgra[8], ViewportInverse, ViewportSize,
/// FragmentRenderScaleCount, RenderScale[73], TfeOffset, TfeVertexCount; one vec4 each).
const SUPPORT_BUFFER_SIZE: usize = 16 * (1 + 8 + 1 + 1 + 1 + 73 + 1 + 1);

/// Metal buffer index of vertex stream `s` (cbufs use 0..17, storage buffers 18..).
const VB_BASE: usize = 30;
/// Metal's per-stage sampler table holds 16; higher sampler indices go through an argument buffer at this index
/// (must match EXTRA_SAMPLER_BASE / EXTRA_SAMPLER_BUFFER in metal/msl_specialize.rs).
const EXTRA_SAMPLER_BASE: usize = 16;
const EXTRA_SAMPLER_BUFFER: usize = 29;
const MAX_STREAMS: usize = 11;
/// Buffer index of the (0,0,0,1) default-attribute buffer (Metal vertex buffer indices must be < 31; streams use 20..30).
const DEFAULT_ATTR_BUFFER: usize = 19;
/// Byte offset in the driver cbuf (guest cbuf 0, Metal binding 17) where each stage's constant buffer sizes go, one u32
/// (float4 count) per Metal binding 0..17 (must match CBUF_LIMITS_VEC4 in metal/msl_specialize.rs). The game never
/// reads there. Computed-index reads beyond a buffer's bound size return zero, as on the GPU.
const CBUF_LIMITS_OFFSET: usize = 0xF00;

/// Float4 count a shader may read from a constant buffer bound with `size` bytes.
fn cbuf_limit(size: usize) -> u32 {
    ((size + 15) / 16).min(4096) as u32
}

/// Declared size of every cbuf in the translated MSL (4096 x float4).
const CBUF_DECL_SIZE: usize = 0x10000;
/// Bytes of a stage's constant bank copied from the stage data.
const CONST_BANK_SIZE: usize = 0x2000;
const VM_PAGE: usize = 0x4000;
/// Scratch chunk size. A submit's synthesized data (driver cbuf, small cbufs, widened indices) is a few KB, so
/// chunks stay small: they are allocated per submit and freed once the GPU is done with them.
const ARENA_CHUNK: usize = 1 << 20;

/// Whether vertex stages negate clip y: only for NVN's LOWER_LEFT window origin (UPPER_LEFT matches Metal).
fn y_negate() -> bool {
    crate::nvn::api::window_origin_mode() != 1
}

pub(super) fn enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| crate::settings::get().graphics.translated_shaders)
}

/// Asynchronous pipeline compilation (on by default; --async-pipelines=0 builds synchronously). A render pipeline missing from the cache is
/// built on a pipeline-compile thread; draws wait for it within a per-frame budget of --async-wait-us (default 2000), so pipelines that
/// Metal's own shader cache serves (~0.3 ms) still draw at once, and is skipped while a cold compile (13-50 ms) runs: a
/// brief pop-in the first time something is seen instead of a hitch. --log-hitch lists the skips.
pub(super) fn async_pipelines() -> bool {
    crate::host::overlay::is_async_shaders()
}

pub(super) struct PipeJob {
    vs: CachedStage,
    fs: Option<CachedStage>,
    attrs: Vec<(u32, MTLVertexFormat, usize, usize, i32)>,
    streams_used: [bool; MAX_STREAMS],
    streams: Vec<u64>,
    color_pfs: Vec<MTLPixelFormat>,
    depth_pf: MTLPixelFormat,
    depth_gl: bool,
    point_size: f32,
    blends: [u64; 8],
    color_state: u32,
    channel_mask: u32,
    blend_color: [f32; 4],
    /// Build the batched variant (metal/batch.rs: tb_<entry> functions).
    batched: bool,
    /// Build the argument-buffer variant (ab_<entry> functions, --arg-buffers).
    args: bool,
}

pub(super) enum AsyncPipe {
    Ready(Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>),
    Pending,
    Failed,
}

enum SlotState {
    /// Queued.
    Pending,
    /// Being built (on a pipeline thread or a draw's thread).
    Running,
    Ready(Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>),
    Failed,
}
struct Slot(Mutex<SlotState>, std::sync::Condvar);
// Metal pipeline states and the job's stage descriptions are thread-safe to hand between threads.
unsafe impl Send for Slot {}
unsafe impl Sync for Slot {}
struct SendJob(PipeJob, std::sync::Arc<Slot>);
unsafe impl Send for SendJob {}

impl Slot {
    fn new() -> std::sync::Arc<Slot> {
        std::sync::Arc::new(Slot(Mutex::new(SlotState::Pending), std::sync::Condvar::new()))
    }
    fn finish(&self, built: Option<Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>>) {
        *self.0.lock().unwrap() = match built {
            Some(p) => SlotState::Ready(p),
            None => SlotState::Failed,
        };
        self.1.notify_all();
    }
}

/// Async-compiled pipelines by key (pending and finished), and pipelines skipped this frame (counted for --log-hitch).
static ASYNC_SLOTS: OnceLock<Mutex<HashMap<Vec<u64>, std::sync::Arc<Slot>>>> = OnceLock::new();
pub(super) static ASYNC_SKIPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(super) static ASYNC_STARTED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
/// Microseconds draws of the current frame waited for compiles (reset at each present).
pub(super) static ASYNC_WAITED_US: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Pipeline thread work: pipelines draws wait for, then the pipeline list's (pipeline_list::precompile).
type ComputePipe = Retained<ProtocolObject<dyn objc2_metal::MTLComputePipelineState>>;
type ComputeCell = std::sync::Arc<OnceLock<Result<ComputePipe, String>>>;

/// The compute pipeline of a kernel, shared by dispatches and the precompile: whoever comes first makes it, the other
/// waits for it.
fn compute_cell(entry: &str) -> ComputeCell {
    static M: OnceLock<Mutex<HashMap<String, ComputeCell>>> = OnceLock::new();
    M.get_or_init(Default::default).lock().unwrap().entry(entry.to_string()).or_default().clone()
}

/// Queues every compute kernel of the shader cache on the pipeline threads, ahead of the listed render pipelines: they
/// need no draw state, and the game dispatches many while it loads (a first launch on the iPhone otherwise compiled
/// them in the middle of loading the world).
fn queue_compute() {
    let entries = super::shader_cache::compute_entries();
    if entries.is_empty() {
        return;
    }
    let (q, cv) = pipe_queue();
    let mut g = q.lock().unwrap();
    g.compute_n += entries.len();
    crate::gpu::pipeline_list::listed_queued(entries.len());
    g.compute.extend(entries);
    drop(g);
    cv.notify_all();
}

type TinyCell = std::sync::Arc<OnceLock<Result<ComputePipe, String>>>;

/// A tiny-pass kernel, shared by draws and the precompile like compute_cell; true when the key is new.
fn tiny_cell(entry: &str, key: &[i64]) -> (TinyCell, bool) {
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
fn tiny_keys(
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
fn queue_tiny(
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
fn build_tiny(entry: &str, key: &[i64]) {
    let t0 = std::time::Instant::now();
    if let Some(g) = super::GFX.get() {
        if let Some(lib) = g.shader_lib.as_ref() {
            let _ = tiny_cell(entry, key).0.get_or_init(|| g.make_tiny(lib, entry, key));
        }
    }
    kernel_done(t0);
}

#[derive(Default)]
struct PipeQueue {
    draws: std::collections::VecDeque<SendJob>,
    /// Compute kernels to make (queue_compute), before the listed render pipelines.
    compute: std::collections::VecDeque<&'static str>,
    /// Tiny-pass kernels to make (queue_tiny), counted with the compute kernels.
    tiny: std::collections::VecDeque<(&'static str, Vec<i64>)>,
    compute_n: usize,
    compute_done: usize,
    compute_ns: u64,
    listed: std::collections::VecDeque<SendJob>,
    /// Listed pipelines queued and built, failures, compile time (for the summary when the list is done).
    listed_n: usize,
    listed_done: usize,
    listed_failed: usize,
    listed_ns: u64,
}

fn pipe_queue() -> &'static (Mutex<PipeQueue>, std::sync::Condvar) {
    static Q: OnceLock<(Mutex<PipeQueue>, std::sync::Condvar)> = OnceLock::new();
    Q.get_or_init(|| {
        let n: usize = crate::settings::get().graphics.pipeline_threads;
        for i in 0..n.max(1) {
            std::thread::Builder::new()
                .name(format!("pipe-compile-{}", i))
                .spawn(|| loop {
                    let (job, listed) = {
                        let (q, cv) = pipe_queue();
                        let mut g = q.lock().unwrap();
                        loop {
                            if let Some(j) = g.draws.pop_front() {
                                break (j, false);
                            }
                            if let Some(entry) = g.compute.pop_front() {
                                drop(g);
                                build_compute(entry);
                                g = q.lock().unwrap();
                                continue;
                            }
                            if let Some((entry, key)) = g.tiny.pop_front() {
                                drop(g);
                                build_tiny(entry, &key);
                                g = q.lock().unwrap();
                                continue;
                            }
                            if let Some(j) = g.listed.pop_front() {
                                break (j, true);
                            }
                            g = cv.wait(g).unwrap();
                        }
                    };
                    let SendJob(j, slot) = job;
                    // A draw may have taken the job over (build_now) or a listed job may have been moved ahead.
                    let take = {
                        let mut st = slot.0.lock().unwrap();
                        let take = matches!(*st, SlotState::Pending);
                        if take {
                            *st = SlotState::Running;
                        }
                        take
                    };
                    let (mut ok, mut ns) = (true, 0);
                    if take {
                        let t0 = std::time::Instant::now();
                        let built = build_job(&j);
                        ns = t0.elapsed().as_nanos() as u64;
                        ok = built.is_some();
                        slot.finish(built);
                    }
                    if !listed {
                        super::HITCH_PIPE_NS.fetch_add(ns, std::sync::atomic::Ordering::Relaxed);
                        super::HITCH_PIPE_N.fetch_add(take as u64, std::sync::atomic::Ordering::Relaxed);
                        continue;
                    }
                    crate::gpu::pipeline_list::listed_done();
                    let mut g = pipe_queue().0.lock().unwrap();
                    g.listed_done += 1;
                    g.listed_failed += !ok as usize;
                    g.listed_ns += ns;
                    if g.listed.is_empty() && g.listed_done == g.listed_n {
                        println!(
                            "[+] Pipelines: {} listed compiled ({} failed), {:.1} s of compile time{}",
                            g.listed_done,
                            g.listed_failed,
                            g.listed_ns as f64 / 1e9,
                            super::pipeline_archive::stats()
                                .map_or(String::new(), |(h, m)| format!("; {h} from binary archives, {m} not in them"))
                        );
                    }
                })
                .expect("spawn pipeline compile thread");
        }
        (Mutex::new(PipeQueue::default()), std::sync::Condvar::new())
    })
}

/// Makes one queued compute kernel's pipeline (or finds it made by a dispatch).
fn build_compute(entry: &str) {
    let t0 = std::time::Instant::now();
    if let Some(g) = super::GFX.get() {
        if let Some(lib) = g.shader_lib.as_ref() {
            let _ = compute_cell(entry).get_or_init(|| g.make_compute(lib, entry));
        }
    }
    kernel_done(t0);
}

fn kernel_done(t0: std::time::Instant) {
    crate::gpu::pipeline_list::listed_done();
    let mut q = pipe_queue().0.lock().unwrap();
    q.compute_done += 1;
    q.compute_ns += t0.elapsed().as_nanos() as u64;
    if q.compute_done == q.compute_n && q.compute.is_empty() && q.tiny.is_empty() {
        println!(
            "[+] Pipelines: {} compute and tiny-pass kernels made before their first use, {:.1} s of compile time",
            q.compute_done,
            q.compute_ns as f64 / 1e9
        );
    }
}

fn build_job(j: &PipeJob) -> Option<Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>> {
    let g = super::GFX.get()?;
    let lib = g.shader_lib.as_ref()?;
    g.build_translated_pipeline(
        lib,
        &j.vs,
        j.fs.as_ref(),
        &j.attrs,
        &j.streams_used,
        &j.streams,
        &j.color_pfs,
        j.depth_pf,
        j.depth_gl,
        j.point_size,
        &j.blends,
        j.color_state,
        j.channel_mask,
        j.blend_color,
        j.batched,
        j.args,
    )
}

fn push_job(job: SendJob, listed: bool) {
    let (q, cv) = pipe_queue();
    let mut g = q.lock().unwrap();
    if listed {
        g.listed.push_back(job);
        g.listed_n += 1;
        crate::gpu::pipeline_list::listed_queued(1);
    } else {
        g.draws.push_back(job);
    }
    drop(g);
    cv.notify_one();
}

/// Moves a listed pipeline's job ahead of the rest of the list (a draw waits for it).
fn promote(slot: &std::sync::Arc<Slot>) {
    let mut g = pipe_queue().0.lock().unwrap();
    if let Some(i) = g.listed.iter().position(|j| std::sync::Arc::ptr_eq(&j.1, slot)) {
        let j = g.listed.remove(i).unwrap();
        g.draws.push_front(j);
        g.listed_n -= 1;
        crate::gpu::pipeline_list::listed_unqueued();
    }
}

/// Queues a pipeline-list entry for compilation: false when its shaders are not in the cache, it was recorded under
/// another window origin or depth mode, or its pipeline is already known.
/// Queues listed pipelines on the pipeline threads (gpu::pipeline_list).
fn compile_listed(entries: Vec<crate::gpu::pipeline_list::Entry>) {
    // The archives first: compiling without them would do what they hold.
    if let Some(g) = super::GFX.get() {
        super::pipeline_archive::load(&g.device);
    }
    queue_compute();
    let (mut queued, mut skipped) = (0, 0);
    for e in &entries {
        if queue_precompile(e) {
            queued += 1;
        } else {
            skipped += 1;
        }
    }
    println!(
        "[+] Pipelines: {queued} of {} listed queued for compilation ({skipped} the same pipeline as another entry, or with missing shaders or other \
         state)",
        entries.len()
    );
}

pub(super) fn queue_precompile(e: &crate::gpu::pipeline_list::Entry) -> bool {
    use super::shader_cache::by_entry;
    if e.y_negate != y_negate() || e.depth_gl != (crate::nvn::api::depth_mode() == 0) {
        return false;
    }
    let Some(vs) = by_entry(&e.vs) else { return false };
    let fs = match &e.fs {
        Some(f) => match by_entry(f) {
            Some(f) => Some(f),
            None => return false,
        },
        None => None,
    };
    let (color_pfs, depth_pf) = match &e.nvn {
        Some(n) => (n.colors.map(|(t, v)| target_pf(t, v)), target_pf(n.depth.0, n.depth.1)),
        None => (e.color_pfs.map(|f| MTLPixelFormat(f as _)), MTLPixelFormat(e.depth_pf as _)),
    };
    let point_size = f32::from_bits(e.point_size);
    let l = derive_layout(vs, &e.attr_bits, &e.streams);
    if e.tiny {
        queue_tiny(vs, fs, &l.attr_descs, e.depth_gl, point_size, &color_pfs);
    }
    let key = pipeline_key(
        point_size,
        vs,
        fs,
        e.depth_gl,
        e.color_state,
        e.channel_mask,
        depth_pf,
        &color_pfs,
        &e.blends,
        &l.attr_descs,
        &l.streams_used,
        &l.streams_eff,
    );
    let slot = {
        let mut m = ASYNC_SLOTS.get_or_init(Default::default).lock().unwrap();
        if m.contains_key(key.as_slice()) {
            return false;
        }
        let s = Slot::new();
        m.insert(key.to_vec(), s.clone());
        s
    };
    let job = PipeJob {
        vs: vs.clone(),
        fs: fs.cloned(),
        attrs: l.attr_descs.to_vec(),
        streams_used: l.streams_used,
        streams: l.streams_eff.to_vec(),
        color_pfs: color_pfs.to_vec(),
        depth_pf,
        depth_gl: e.depth_gl,
        point_size,
        blends: e.blends,
        color_state: e.color_state,
        channel_mask: e.channel_mask,
        blend_color: [0.0; 4],
        batched: false,
        args: false,
    };
    push_job(SendJob(job, slot), true);
    true
}

/// The pipeline for `key` built on this thread, unless a pipeline thread has it (listed): then its result, waiting for
/// it if it is being built.
fn build_now(
    key: &[u64],
    build: impl FnOnce() -> Option<Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>>,
) -> Option<Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>> {
    let slot = ASYNC_SLOTS.get().and_then(|m| m.lock().unwrap().get(key).cloned());
    let Some(slot) = slot else { return build() };
    let mut g = slot.0.lock().unwrap();
    loop {
        match &*g {
            SlotState::Ready(p) => return Some(p.clone()),
            SlotState::Failed => return None,
            SlotState::Running => g = slot.1.wait(g).unwrap(),
            SlotState::Pending => {
                *g = SlotState::Running;
                drop(g);
                let built = build();
                slot.finish(built.clone());
                return built;
            }
        }
    }
}

/// async_pipeline without waiting (the issuer thread's batched pipelines, metal/batch.rs): `job` is needed on the first
/// call only.
pub(super) fn async_pipeline_nowait(key: &[u64], job: Option<PipeJob>) -> AsyncPipe {
    let slots = ASYNC_SLOTS.get_or_init(Default::default);
    let slot = {
        let mut m = slots.lock().unwrap();
        match m.get(key) {
            Some(s) => s.clone(),
            None => {
                let Some(job) = job else {
                    return AsyncPipe::Failed;
                };
                let s = Slot::new();
                m.insert(key.to_vec(), s.clone());
                drop(m);
                push_job(SendJob(job, s.clone()), false);
                s
            }
        }
    };
    let g = slot.0.lock().unwrap();
    match &*g {
        SlotState::Ready(p) => AsyncPipe::Ready(p.clone()),
        SlotState::Failed => AsyncPipe::Failed,
        SlotState::Pending | SlotState::Running => AsyncPipe::Pending,
    }
}

/// The pipeline for `key`: ready (now or within the wait), still compiling (the draw is skipped), or failed.
pub(super) fn async_pipeline(key: &[u64], job: PipeJob) -> AsyncPipe {
    let slots = ASYNC_SLOTS.get_or_init(Default::default);
    let (slot, new) = {
        let mut m = slots.lock().unwrap();
        match m.get(key) {
            Some(s) => (s.clone(), false),
            None => {
                let s = Slot::new();
                m.insert(key.to_vec(), s.clone());
                (s, true)
            }
        }
    };
    if new {
        ASYNC_STARTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        push_job(SendJob(job, slot.clone()), false);
    } else if matches!(*slot.0.lock().unwrap(), SlotState::Pending) {
        promote(&slot);
    }
    // The wait is a per-frame budget (a scene change brings dozens of new pipelines at once).
    static WAIT: OnceLock<u64> = OnceLock::new();
    let budget = *WAIT.get_or_init(|| crate::settings::get().graphics.async_wait_us);
    let left = budget.saturating_sub(ASYNC_WAITED_US.load(std::sync::atomic::Ordering::Relaxed));
    let t0 = std::time::Instant::now();
    let g = slot.0.lock().unwrap();
    let (g, _) = slot
        .1
        .wait_timeout_while(
            g,
            std::time::Duration::from_micros(if new { left } else { 0 }),
            |s| matches!(s, SlotState::Pending | SlotState::Running),
        )
        .unwrap();
    ASYNC_WAITED_US.fetch_add(
        t0.elapsed().as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    let r = match &*g {
        SlotState::Ready(p) => AsyncPipe::Ready(p.clone()),
        SlotState::Failed => AsyncPipe::Failed,
        SlotState::Pending | SlotState::Running => AsyncPipe::Pending,
    };
    drop(g);
    // Finished slots stay: the encode worker and the Presentation Thread keep separate pipeline caches (WorkerCtx), and
    // frames encoded on this thread (traces, captures, per-draw details) would otherwise find every pipeline missing
    // and skip nearly all draws while they compile again.
    if matches!(r, AsyncPipe::Pending) {
        ASYNC_SKIPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    r
}

/// Waits for the pipeline async_pipeline started for `key` to be made or to fail, however long it takes: for a draw into
/// targets not drawn every frame (State::enc_recur), which a skip could leave undrawn for good.
pub(super) fn async_pipeline_wait(key: &[u64]) -> AsyncPipe {
    let Some(slot) = ASYNC_SLOTS.get_or_init(Default::default).lock().unwrap().get(key).cloned() else {
        return AsyncPipe::Failed;
    };
    if matches!(*slot.0.lock().unwrap(), SlotState::Pending) {
        promote(&slot);
    }
    let t0 = std::time::Instant::now();
    let g = slot.1.wait_while(slot.0.lock().unwrap(), |s| matches!(s, SlotState::Pending | SlotState::Running)).unwrap();
    ASYNC_WAITED_US.fetch_add(t0.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed);
    let r = match &*g {
        SlotState::Ready(p) => AsyncPipe::Ready(p.clone()),
        SlotState::Failed => AsyncPipe::Failed,
        SlotState::Pending | SlotState::Running => AsyncPipe::Pending,
    };
    if !matches!(r, AsyncPipe::Pending) {
        ASYNC_SKIPS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
    r
}

/// What a stage's driver cbuf (guest cbuf 0) holds: EncodeCtx::driver_cb_memo.
#[derive(Clone, PartialEq)]
pub(crate) struct DriverCbKey {
    base_vertex: i32,
    base_instance: i32,
    sb_descs: Vec<(usize, u64, u64)>,
    limits: [u32; 18],
    heap: Option<HeapCb>,
}

/// A heap stage's driver cbuf part (metal/tex_heap.rs): heap addresses, (handle index, handle) per sampled texture.
#[derive(Clone, PartialEq, Default)]
pub(crate) struct HeapCb {
    addresses: [u64; super::tex_heap::HEAPS],
    handles: smallvec::SmallVec<[(u16, u32); 8]>,
}

/// Registers a new pipeline with the batcher (metal/batch.rs) when its draws can be batched: both stages have batched
/// variants (or the fragment stage reads no constant buffer) and no vertex stream steps per instance (a batch's
/// instances are the run's draws).
#[allow(clippy::too_many_arguments)]
fn register_batchable(
    pipe: &Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>,
    key: &[u64],
    vs: &CachedStage,
    fs: Option<&CachedStage>,
    attrs: &[(u32, MTLVertexFormat, usize, usize, i32)],
    streams_used: &[bool; MAX_STREAMS],
    streams: &[u64],
    color_pfs: &[MTLPixelFormat],
    depth_pf: MTLPixelFormat,
    depth_gl: bool,
    point_size: f32,
    blends: &[u64; 8],
    p: &DrawPacket,
) {
    let Some(vtb) = &vs.batch_cbufs else { return };
    let ftb: &[u32] = match fs {
        Some(f) => match &f.batch_cbufs {
            Some(b) => b,
            None if f.cbuffers.is_empty() => &[],
            None => return,
        },
        None => &[],
    };
    if (0..MAX_STREAMS).any(|s| streams_used[s] && streams.get(s).is_some_and(|&b| b >> 32 != 0)) {
        return;
    }
    let job = PipeJob {
        vs: vs.clone(),
        fs: fs.cloned(),
        attrs: attrs.to_vec(),
        streams_used: *streams_used,
        streams: streams.to_vec(),
        color_pfs: color_pfs.to_vec(),
        depth_pf,
        depth_gl,
        point_size,
        blends: *blends,
        color_state: p.color_state,
        channel_mask: p.channel_mask,
        blend_color: p.blend_color,
        batched: true,
        args: false,
    };
    super::batch::register(pipe, vtb, ftb, key, job);
}

/// Debug (--debug-vpos): a 16 MB shared buffer bound at vertex buffer index 26 for every translated draw, for edited
/// vertex shaders (--debug-vs-file) that write values such as their clip-space position into it; written to
/// /tmp/nxport_vpos.bin at presented frame --debug-vpos-dump-at (default 4800).
pub(super) struct DebugBuf(pub(super) Retained<ProtocolObject<dyn MTLBuffer>>);
unsafe impl Send for DebugBuf {}
unsafe impl Sync for DebugBuf {}
/// Size of the --debug-vpos buffer: --debug-vpos-mb (default 16).
pub(super) fn debug_vpos_len() -> usize {
    static L: OnceLock<usize> = OnceLock::new();
    *L.get_or_init(|| crate::settings::get().debug.debug_vpos_mb << 20)
}

pub(super) fn debug_vpos_buffer() -> Option<&'static DebugBuf> {
    static B: OnceLock<Option<DebugBuf>> = OnceLock::new();
    B.get_or_init(|| {
        if !crate::settings::get().debug.debug_vpos {
            return None;
        }
        let g = super::GFX.get()?;
        let b = g.device.newBufferWithLength_options(
            debug_vpos_len(),
            objc2_metal::MTLResourceOptions::StorageModeShared,
        )?;
        unsafe { std::ptr::write_bytes(b.contents().as_ptr() as *mut u8, 0, debug_vpos_len()) };
        Some(DebugBuf(b))
    })
    .as_ref()
}

/// The translated shader libraries (shaders_NNNN.metallib) with an entry-point -> library map.
pub(super) struct ShaderLibs {
    libs: Vec<Retained<ProtocolObject<dyn MTLLibrary>>>,
    by_entry: HashMap<String, usize>,
    dynamic_libs: Mutex<HashMap<String, Retained<ProtocolObject<dyn MTLLibrary>>>>,
}

static DYNAMIC_LIBS: OnceLock<Mutex<HashMap<String, Retained<ProtocolObject<dyn MTLLibrary>>>>> =
    OnceLock::new();

fn dynamic_libs() -> &'static Mutex<HashMap<String, Retained<ProtocolObject<dyn MTLLibrary>>>> {
    DYNAMIC_LIBS.get_or_init(Default::default)
}

impl ShaderLibs {
    /// A kernel the cache builder appended to `base`'s stage (same library), with function constants.
    fn kernel(
        &self,
        base: &str,
        name: &str,
        constants: &MTLFunctionConstantValues,
    ) -> Result<Retained<ProtocolObject<dyn MTLFunction>>, String> {
        if let Some(&i) = self.by_entry.get(base) {
            return super::pipeline_archive::function(&self.libs[i], name, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        if let Some(lib) = self.dynamic_libs.lock().unwrap().get(base) {
            return super::pipeline_archive::function(&lib, name, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        if let Some(lib) = dynamic_libs().lock().unwrap().get(base) {
            return super::pipeline_archive::function(&lib, name, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        Err(format!("{} is not in any shader library", base))
    }

    fn function(
        &self,
        entry: &str,
        constants: &MTLFunctionConstantValues,
    ) -> Result<Retained<ProtocolObject<dyn MTLFunction>>, String> {
        if let Some(&i) = self.by_entry.get(entry) {
            return super::pipeline_archive::function(&self.libs[i], entry, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        if let Some(lib) = self.dynamic_libs.lock().unwrap().get(entry) {
            return super::pipeline_archive::function(&lib, entry, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        if let Some(lib) = dynamic_libs().lock().unwrap().get(entry) {
            return super::pipeline_archive::function(&lib, entry, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        Err(format!("{} is not in any shader library", entry))
    }

    pub(super) fn register_dynamic(
        &self,
        entry: String,
        lib: Retained<ProtocolObject<dyn MTLLibrary>>,
    ) {
        self.dynamic_libs.lock().unwrap().insert(entry, lib);
    }
}

pub fn register_dynamic_library(entry: &str, lib: Retained<ProtocolObject<dyn MTLLibrary>>) {
    dynamic_libs().lock().unwrap().insert(entry.to_string(), lib.clone());
    if let Some(gfx) = super::GFX.get() {
        if let Some(shader_lib) = &gfx.shader_lib {
            shader_lib.register_dynamic(entry.to_string(), lib);
        }
    }
}

/// Prints `msg` the first time `key` is seen (process-wide: callers include worker-side texture resolution).
pub(super) fn warn_once(key: String, msg: String) {
    static SEEN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    if SEEN
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(key)
    {
        println!("[!] Gfx: {}", msg);
    }
}

/// Shaders that hung the GPU on an earlier run. A hang makes macOS reset the GPU and can freeze the whole machine for
/// many seconds, so after the first one the offender is never run again: the engine writes the entries of the encoder
/// that did not complete to <cache>/hang_denylist.txt and skips every draw that uses them.
fn denylist() -> &'static Mutex<std::collections::HashSet<String>> {
    static D: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    D.get_or_init(|| {
        let path = crate::metal::shader_cache::cache_dir().join("hang_denylist.txt");
        let set: std::collections::HashSet<String> = std::fs::read_to_string(&path)
            .map(|t| {
                t.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| l.starts_with("s_"))
                    .collect()
            })
            .unwrap_or_default();
        if !set.is_empty() {
            println!(
                "[!] Gfx: {} shader(s) are on the GPU-hang denylist ({}) and will not run",
                set.len(),
                path.display()
            );
        }
        Mutex::new(set)
    })
}

/// Whether [addr, addr + size) overlaps guest memory written by GPU work earlier in this submit (`State::gpu_dirty`).
fn gpu_written(gpu_dirty: &std::collections::BTreeMap<u64, u64>, addr: u64, size: usize) -> bool {
    // The ranges are merged and disjoint: only the last one starting below `end` can overlap.
    let end = addr + size as u64;
    gpu_dirty
        .range(..end)
        .next_back()
        .is_some_and(|(_, &e)| e > addr)
}

/// Entries in the hang denylist (checked per draw: nearly always zero, so no lock).
static DENIED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

fn is_denied(entry: &str) -> bool {
    let n = DENIED.load(std::sync::atomic::Ordering::Relaxed);
    if n == 0 {
        return false;
    }
    let d = denylist().lock().unwrap();
    DENIED.store(d.len(), std::sync::atomic::Ordering::Relaxed);
    d.contains(entry)
}

fn draw_masks() -> Option<&'static [crate::settings::DrawMask]> {
    let m = &crate::settings::get().debug.gfx_draw_mask;
    (!m.is_empty()).then_some(m.as_slice())
}

/// (offset, size) of a stage's constant bank from its GLSLC control section (magic 0x98761234, total size at +0x20).
unsafe fn control_const_bank(control: u64) -> Option<(usize, usize)> {
    if control < 0x1_0000_0000 || *(control as *const u32) != 0x9876_1234 {
        return None;
    }
    let ctrl_size = *((control + 0x20) as *const u32) as u64;
    if ctrl_size < 0x708 {
        return None;
    }
    let size = *((control + 0x6fc) as *const u32) as usize;
    let off = *((control + 0x700) as *const u32) as usize;
    if off == 0 || off > 0x10_0000 || size > CBUF_DECL_SIZE {
        return None;
    }
    Some((off, size))
}

/// Metal texture type a shader binding declares (manifest type strings from sass2spv).

/// Where a 16-bit packed format's channels sit in the Metal format it is stored as, as swizzle codes for R, G, B, A
/// (2..5 = Metal's red..alpha): NVN orders them from the low bits, Metal's layouts put blue there (metal_format).
fn packed_channels(pf: MTLPixelFormat) -> Option<[u8; 4]> {
    match pf {
        MTLPixelFormat::B5G6R5Unorm | MTLPixelFormat::BGR5A1Unorm => Some([4, 3, 2, 5]),
        MTLPixelFormat::ABGR4Unorm => Some([5, 4, 3, 2]),
        _ => None,
    }
}

/// The texture a shader binding should see: `t` restricted by the NVN texture view (format, swizzle, mip range,
/// layer range) and re-typed to `want` when Metal allows that view; None when the types cannot be reconciled.
unsafe fn texture_for_binding(
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
pub(super) fn sub_view_2d(
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
pub(super) fn view_cache_cap() -> usize {
    static CAP: OnceLock<usize> = OnceLock::new();
    *CAP.get_or_init(|| crate::settings::get().graphics.view_cache_cap)
}

/// Frame end: drops binding views not used in this or the previous frame. TotK's textures are mostly stable from frame to
/// frame (a bound texture needs the same swizzle / type view each frame: ~450 views per Hateno frame, re-created every
/// frame when the cache was cleared at 512 entries, ~1.3 ms of encode), while transient ones disappear; keeping views of
/// those alive longer would keep their textures alive (a 4096-entry cache without aging doubled memory to ~9 GB).
pub(super) fn trim_view_cache(
    cache: &mut HashMap<(usize, usize, usize, [usize; 4], [isize; 4]), (Tex, u64)>,
) {
    let frame = super::presented_frames();
    cache.retain(|_, (_, used)| *used + 1 >= frame);
    if cache.len() > view_cache_cap() {
        cache.clear();
    }
}

/// Uniform buffer copies made / reused, bytes copied, copy-cache clears by guest writes (timeline log).
pub(super) static UBO_STATS: [std::sync::atomic::AtomicU64; 4] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 4];

/// Binding views created (cache misses) and full cache clears (timeline log).
pub(super) static BINDING_VIEW_STATS: [std::sync::atomic::AtomicU64; 2] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Identity of the storage behind a texture: a view's parent texture, else the texture itself.
///
/// The answer is stored on the texture as an associated object (OBJC_ASSOCIATION_ASSIGN: a plain value, never retained
/// or messaged, dropped with the texture), so each texture is walked once in its life. -[IOGPUMetalTexture parentTexture]
/// took >100 us per call in Hateno (~20% of encode time with a per-submit memo), even on root textures.
/// --no-root-tag: walk every time (A/B).
/// root_id calls answered by the tag / by walking the parent chain (timeline log).
pub(super) static ROOT_STATS: [std::sync::atomic::AtomicU64; 2] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];
/// Frame journal: render targets drawn this frame by (guest storage, width, height) -> Metal root id, and the stale
/// samples already reported this frame. Cleared when the journal rotates.
pub(super) static FRAME_RT_ROOTS: std::sync::Mutex<
    Option<(
        rustc_hash::FxHashMap<(usize, usize, usize), u64>,
        rustc_hash::FxHashSet<(usize, u64)>,
    )>,
> = std::sync::Mutex::new(None);
/// The previous frame's render targets (same key): a sample before this frame renders that memory reads last frame's
/// contents (temporal effects), which live in the Metal texture last frame rendered into.
pub(super) static PREV_RT_ROOTS: std::sync::Mutex<
    Option<rustc_hash::FxHashMap<(usize, usize, usize), u64>>,
> = std::sync::Mutex::new(None);

/// --history-targets=WxH: render targets of that size drawn this frame (guest storage, texture), copied into the
/// target history ring at the present (metal::frame_history).
pub(super) struct FrameTargets(pub(super) Vec<(usize, Tex)>);
unsafe impl Send for FrameTargets {}
pub(super) static FRAME_TARGETS: std::sync::Mutex<FrameTargets> =
    std::sync::Mutex::new(FrameTargets(Vec::new()));

pub(super) fn history_target_size() -> Option<(usize, usize)> {
    crate::settings::get()
        .dump
        .history_targets
        .map(|s| (s.w as usize, s.h as usize))
}

fn note_frame_rt(storage: usize, t: &Tex) {
    if storage < 0x10000 {
        return;
    }
    if history_target_size() == Some((t.width(), t.height()))
        && !super::copy::is_depth_stencil(t.pixelFormat())
    {
        let mut g = FRAME_TARGETS.lock().unwrap();
        let id = super::tex_id(t);
        if !g.0.iter().any(|(_, x)| super::tex_id(x) == id) {
            g.0.push((storage, t.clone()));
        }
    }
    let id = root_id(t);
    let mut g = FRAME_RT_ROOTS.lock().unwrap();
    g.get_or_insert_with(Default::default)
        .0
        .insert((storage, t.width(), t.height()), id);
}

/// A sampled texture whose guest memory (same size) was rendered this frame into a different Metal texture: the draw
/// reads contents the game did not write this frame (the same memory must always resolve to one Metal texture).
unsafe fn journal_stale_sample(handle: u64, t: &Tex) {
    let Some((addr, _)) = crate::nvn::api::lookup_pool_texture((handle & 0xFFFFF) as i32) else {
        return;
    };
    let Some((gen, info)) = super::tex_info(addr) else {
        return;
    };
    let id = root_id(t);
    let mut g = FRAME_RT_ROOTS.lock().unwrap();
    let (rts, seen) = g.get_or_insert_with(Default::default);
    let key = (info.storage, t.width(), t.height());
    if let Some(&rt) = rts.get(&key) {
        if rt != id && seen.insert((info.storage, id)) {
            super::journal(|| {
                format!("STALE sample: handle {:#x} nvn {:x} gen {} @{:x} {}x{} fmt {} -> {:x}, but rendered this frame into {:x}",
                handle, addr, gen, info.storage, t.width(), t.height(), info.format, id, rt)
            });
        }
    } else if let Some(&rt) = PREV_RT_ROOTS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|p| p.get(&key))
    {
        if rt != id && seen.insert((info.storage, id)) {
            super::journal(|| {
                format!("STALE sample (last frame): handle {:#x} nvn {:x} gen {} @{:x} {}x{} fmt {} -> {:x}, but last frame rendered it into {:x}",
                handle, addr, gen, info.storage, t.width(), t.height(), info.format, id, rt)
            });
        }
    }
}

pub(super) fn root_id(t: &Tex) -> u64 {
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


/// Tiny passes as compute: on by default; --tiny-compute=0 renders them. Without a cache built with kernels (index lines `tk`) no draw qualifies.
pub(super) fn tiny_compute_on() -> bool {
    static F: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *F.get_or_init(|| crate::settings::get().graphics.tiny_compute)
}

/// --draw-details: per-draw debug groups and per-draw binding details for GPU error/hang reports.
fn draw_details() -> bool {
    crate::settings::get().debug.draw_details
}

/// Encoder labels with attachment shapes ([c<i>:WxH:fmt,...,d:WxH:fmt]): --enc-labels or --draw-details.
fn full_labels() -> bool {
    crate::settings::get().debug.enc_labels || draw_details()
}

/// --gfx-trace=N logs the state of the first N translated draws.

/// --tiny-compute runs qualifying small draws as compute; off on replay side B with
/// --replay-ab-tiny.
/// Formats compute kernels cannot write as color targets (depth / stencil, sRGB).
pub(super) fn is_depth_or_srgb(pf: MTLPixelFormat) -> bool {
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

pub(super) fn tiny_enabled() -> bool {
    tiny_compute_on()
}

fn trace_limit() -> u64 {
    static T: OnceLock<u64> = OnceLock::new();
    *T.get_or_init(|| crate::settings::get().trace.gfx_trace)
}

pub(super) fn load_shader_lib(device: &Device) -> Option<ShaderLibs> {
    if !enabled() {
        return None;
    }
    let dir = crate::metal::shader_cache::cache_dir();
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).map_or(false, |n| {
                n.starts_with("shaders_") && n.ends_with(".metallib")
            })
        })
        .collect();
    paths.sort();
    let mut libs = Vec::new();
    let mut by_entry = HashMap::default();
    for path in &paths {
        let path = path.canonicalize().unwrap_or_else(|_| path.clone());
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
        match device.newLibraryWithURL_error(&url) {
            Ok(lib) => {
                for name in lib.functionNames().iter() {
                    by_entry.insert(name.to_string(), libs.len());
                }
                libs.push(lib);
            }
            Err(e) => println!(
                "[!] Gfx: cannot load {}: {}",
                path.display(),
                e.localizedDescription()
            ),
        }
    }
    if libs.is_empty() {
        if !crate::settings::get().graphics.dynamic_shaders {
            println!("[!] Gfx: no shader libraries in {} and --dynamic-shaders off: nothing is drawn", dir.display());
            return None;
        }
        println!("[+] Gfx: no shader libraries in {}: shaders are translated as the game loads them", dir.display());
    }
    println!(
        "[+] Gfx: {} translated shader libraries, {} entry points",
        libs.len(),
        by_entry.len()
    );
    let initial_dynamic = dynamic_libs().lock().unwrap().clone();
    Some(ShaderLibs {
        libs,
        by_entry,
        dynamic_libs: Mutex::new(initial_dynamic),
    })
}

/// What encode_draw works with: this thread's encoding state, the shared caches, the encoder labels, the
/// render encoder's op queue, and the frame's render-target list (traces). No `State`: the second half of a draw can run on another thread.
pub(super) struct EncodeEnv<'a> {
    pub(super) cx: &'a mut super::EncodeCtx,
    pub(super) sh: &'a mut super::SharedCaches,
    pub(super) lb: &'a mut super::EncLabels,
    pub(super) enc: super::issuer::Sink,
    pub(super) frame_rts: &'a [(u64, Tex, u32)],
    /// Draws in the current render encoder before this one (traces).
    pub(super) enc_draws: u32,
    /// Set when the draw was issued (the caller counts it toward the pass split).
    pub(super) drew: bool,
}

/// Zero-copy buffers over guest memory pools, keyed by wrapped range (see Gfx::pool_buffer): one map for all threads.
struct PoolBuffers(HashMap<(usize, usize), Retained<ProtocolObject<dyn MTLBuffer>>>);
// Metal buffers may be used from any thread; the map is only touched under its mutex.
unsafe impl Send for PoolBuffers {}
static POOL_BUFFERS: Mutex<Option<PoolBuffers>> = Mutex::new(None);

pub(super) fn pool_buffers_len() -> usize {
    POOL_BUFFERS
        .lock()
        .unwrap()
        .as_ref()
        .map_or(0, |m| m.0.len())
}

/// The issuer thread's own encode state: its arena, uniform copies and redundant-state filter, and its own copies of the
/// shared caches (pipelines, samplers, constant banks, pool buffers): sharing them with the encoding thread made its
/// compute dispatches wait for whole draws (~20% of its time).
#[derive(Default)]
pub(super) struct WorkerCtx {
    cx: super::EncodeCtx,
    lb: super::EncLabels,
    sh: super::SharedCaches,
}

impl WorkerCtx {
    /// Op::SubmitStart: what State's submit start resets for the encoding thread's EncodeCtx.
    pub(super) fn submit_start(&mut self) {
        self.cx.arena.reset();
        self.cx.ubo_copies.clear();
        self.cx.support_cache.clear();
        self.cx.driver_cb_memo = [None, None];
        self.cx.index_memo = None;
        self.cx.max_index_memo.clear();
    }

    /// Op::Begin: a new render encoder starts with no state set.
    pub(super) fn encoder_begins(&mut self) {
        self.cx.ec = EncCache::default();
    }
}

/// Packets travelling to the issuer thread and back (no allocation per draw).
static PACKET_POOL: Mutex<Vec<Box<DrawPacket>>> = Mutex::new(Vec::new());

thread_local! {
    static LOCAL_PACKETS: std::cell::RefCell<Vec<Box<DrawPacket>>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn get_packet() -> Box<DrawPacket> {
    LOCAL_PACKETS.with(|cell| {
        let mut local = cell.borrow_mut();
        if let Some(pkt) = local.pop() {
            return pkt;
        }
        let mut pool = PACKET_POOL.lock().unwrap();
        if pool.is_empty() {
            Box::default()
        } else {
            let take = pool.len().min(64);
            let start = pool.len() - take;
            local.extend(pool.drain(start..));
            local.pop().unwrap_or_default()
        }
    })
}

fn recycle_packet(pkt: Box<DrawPacket>) {
    LOCAL_PACKETS.with(|cell| {
        let mut local = cell.borrow_mut();
        local.push(pkt);
        if local.len() >= 64 {
            let mut pool = PACKET_POOL.lock().unwrap();
            if pool.len() < 4096 {
                let move_count = local.len().min(32);
                let start = local.len() - move_count;
                pool.extend(local.drain(start..));
            } else {
                local.truncate(32);
            }
        }
    });
}

/// Runs a queued draw's encode half on the encode stage (its ops go to the Metal stage) or the issuer thread (onto its
/// current encoder).
pub(super) unsafe fn run_draw_job(
    mut packet: Box<DrawPacket>,
    sink: super::issuer::Sink,
    w: &mut WorkerCtx,
) {
    if let Some(g) = super::GFX.get() {
        let t0 = crate::settings::get()
            .log
            .log_phases
            .then(std::time::Instant::now);
        let mut env = EncodeEnv {
            cx: &mut w.cx,
            sh: &mut w.sh,
            lb: &mut w.lb,
            enc: sink,
            frame_rts: &[],
            enc_draws: 0,
            drew: false,
        };
        g.encode_draw(&mut env, &packet);
        if let Some(t0) = t0 {
            let k = if packet.from_old_list { 0 } else { 1 };
            super::PHASES[4 + k * 2].fetch_add(
                t0.elapsed().as_nanos() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
            super::PHASES[5 + k * 2].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    packet.release();
    recycle_packet(packet);
}

/// Draws' encode halves run on the gfx-encoder thread while this thread plans the next draws (--encode-worker=0: on
/// this thread). Not with --no-issuer, frame traces or per-draw details (their output needs this thread's state).
fn encode_on_worker(st: &State) -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().graphics.encode_worker)
        && !st.issuer.is_inline() && !super::tracing_frame() && !draw_details()
}

/// plan_draw's texture-resolution reuse key (State::tex_reuse).
#[derive(PartialEq)]
pub(crate) struct TexReuseKey {
    vs: usize,
    fs: usize,
    handles: smallvec::SmallVec<[Option<u64>; 16]>,
    epochs: [u64; 4],
}

/// Draws whose textures were [reused from the previous draw, resolved].
pub(super) static TEX_REUSE_STATS: [std::sync::atomic::AtomicU64; 2] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 2];

/// A thread's texture binding memos (EncodeCtx::bind_memo).
pub(super) type BindMemos = HashMap<(i32, usize), super::BindMemo>;

/// Access to the texture cache for the paths that need it (binding memo misses, the per-frame CPU-write check). The
/// encoding thread holds it as `&mut TexCache`; a worker thread locks it per use (parallel encoding).
pub(super) trait TcAccess {
    fn with<R>(&mut self, f: impl FnOnce(&mut TexCache) -> R) -> R;
}

impl TcAccess for &mut TexCache {
    fn with<R>(&mut self, f: impl FnOnce(&mut TexCache) -> R) -> R {
        f(self)
    }
}

/// A cached plan_uniforms decision (State::ubo_decisions).
pub(crate) struct UboDecision {
    addr: u64,
    size: usize,
    len: usize,
    epoch: u64,
    pre: UboPre,
}

/// How a uniform buffer binding is bound (decided by plan_uniforms, in command order).
#[derive(Clone)]
pub(crate) enum UboPre {
    /// GPU work of this submit writes it: bound zero-copy, the GPU reads it in order.
    ZeroCopy,
    /// An executor write later in this submit (inline uniform update, counter report) overlaps it: copied by the plan.
    Snapshot(Retained<ProtocolObject<dyn MTLBuffer>>, usize),
    /// Nothing in this submit writes it: encode_draw copies it whenever it runs.
    Copy,
}

/// Textures a draw binds: (stage, binding, texture, sampler id, root id).
pub(super) type ResolvedTex =
    smallvec::SmallVec<[(i32, u32, Option<Tex>, Option<i32>, Option<u64>); 16]>;

/// Code and control section of a translated stage (const_bank needs them).
#[derive(Clone, Copy)]
pub(super) struct CodeRef {
    pub(super) data: u64,
    pub(super) control: u64,
}

/// What encode_draw needs from a draw: the plan's results (shaders, render targets) and a copy of the NVN state the
/// draw reads (only the bindings its shaders use), so the second half needs no NVN state from `State` (parallel
/// encoding).
/// encode_draw's vertex layout and pipeline for one set of inputs (EncodeCtx::layout_memo).
pub(crate) struct LayoutMemo {
    attr_descs: smallvec::SmallVec<[(u32, MTLVertexFormat, usize, usize, i32); 16]>,
    streams_used: [bool; MAX_STREAMS],
    needs_default_attr: bool,
    streams_eff: smallvec::SmallVec<[u64; 16]>,
    repack: Vec<(usize, u64, Vec<(usize, usize, usize)>)>,
    pipe: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
}

/// Adds a draw's pipeline to the pipeline list.
fn record_pipeline(
    vs: &CachedStage,
    fs: Option<&CachedStage>,
    p: &DrawPacket,
    point_size: f32,
    depth_gl: bool,
    blends: &[u64; 8],
    tiny: bool,
) {
    let (colors, depth, samples) = p.nvn_targets;
    // The formats a list entry gives back must be the ones drawn with, or listed pipelines are never the drawn ones.
    let derived = (colors.map(|(t, v)| target_pf(t, v)), target_pf(depth.0, depth.1));
    if derived != (p.color_pfs, p.depth_pf) {
        warn_once(
            format!("pipeline-list-pf-{colors:?}-{depth:?}"),
            format!("pipeline list: targets {colors:?} / {depth:?} drawn as {:?} / {:?}, listed as {:?} / {:?}", p.color_pfs, p.depth_pf, derived.0, derived.1),
        );
    }
    crate::gpu::pipeline_list::record(
        crate::gpu::pipeline_list::Entry {
            vs: vs.entry.clone(),
            fs: fs.map(|f| f.entry.clone()),
            point_size: point_size.to_bits(),
            depth_gl,
            y_negate: y_negate(),
            color_state: p.color_state,
            channel_mask: p.channel_mask,
            depth_pf: 0,
            color_pfs: [0; 8],
            blends: *blends,
            attr_bits: p.attr_bits.to_vec(),
            streams: p.vertex_streams.to_vec(),
            nvn: Some(nxport_pipelines::Nvn {
                colors,
                depth,
                samples,
                mode: p.mode as u32,
                restart: p.prim_restart.is_some(),
                polygon: p.polygon,
                depth_stencil: p.depth_stencil,
                multisample: p.multisample,
                depth_clamp: p.depth_clamp,
            }),
            tiny,
        }
        .normalized(),
    );
}

/// The Metal pixel format a target of NVN format `tex` viewed as `view` (0: its own) renders in (as plan_draw resolves
/// targets: GPU compression through an integer view renders into a proxy of the view's format, other compatible views
/// render in the view's format).
pub(super) fn target_pf(tex: u32, view: u32) -> MTLPixelFormat {
    if tex == 0 {
        return MTLPixelFormat::Invalid;
    }
    let own = metal_format(tex as i32).0;
    if view == 0 || view == tex {
        return own;
    }
    let pf = metal_format(view as i32).0;
    if let (Some(i), Some(v)) = (crate::nvn::layout::format_info(tex as i32), crate::nvn::layout::format_info(view as i32)) {
        if i.block_w > 1 && v.block_w == 1 && v.bytes == i.bytes {
            return pf;
        }
    }
    if pf == MTLPixelFormat::Invalid || pf == own || !super::is_view_compatible(own, pf) {
        own
    } else {
        pf
    }
}

/// A draw's vertex attribute descriptors and stream layout (the vertex half of LayoutMemo).
pub(super) struct Layout {
    pub(super) attr_descs: smallvec::SmallVec<[(u32, MTLVertexFormat, usize, usize, i32); 16]>,
    pub(super) streams_used: [bool; MAX_STREAMS],
    pub(super) needs_default_attr: bool,
    pub(super) streams_eff: smallvec::SmallVec<[u64; 16]>,
    pub(super) repack: Vec<(usize, u64, Vec<(usize, usize, usize)>)>,
}

/// The vertex layout for `vs` with NVN vertex attribute bits (per entry of vs.attrs) and stream bits.
pub(super) fn derive_layout(vs: &CachedStage, attr_bits: &[Option<u32>], vertex_streams: &[u64]) -> Layout {
    let mut attr_descs: smallvec::SmallVec<[(u32, MTLVertexFormat, usize, usize, i32); 16]> =
        smallvec::SmallVec::new(); // loc, fmt, bufIdx, offset, kind
    let mut streams_used = [false; MAX_STREAMS];
    let mut needs_default_attr = false;
    for (attr_i, &loc) in vs.attrs.iter().enumerate() {
        let bits = attr_bits.get(attr_i).copied().flatten();
        let va = bits.map(decode_attr);
        match va.as_ref().and_then(|a| vertex_format(a.format).map(|f| (a, f))) {
            Some((a, (fmt, kind))) if a.stream < MAX_STREAMS => {
                streams_used[a.stream] = true;
                attr_descs.push((loc, fmt, VB_BASE - a.stream, a.offset as usize, kind));
            }
            _ => {
                needs_default_attr = true;
                attr_descs.push((loc, MTLVertexFormat::Float4, DEFAULT_ATTR_BUFFER, 0, 0));
            }
        }
    }

    // Metal needs 4-byte aligned vertex strides and attribute offsets; NVN does not (TotK's skinned shadow meshes use
    // a 14-byte stride with attributes at offsets 6 and 10). Metal misfetched them: garbage bone indices, skinning
    // matrices read out of bounds, vertices flung across the screen. Such per-vertex streams are repacked on the CPU
    // into an aligned layout (each attribute copied to its own aligned slot) for the vertices the draw fetches.
    let mut streams_eff: smallvec::SmallVec<[u64; 16]> = vertex_streams.iter().copied().collect();
    let mut repack: Vec<(usize, u64, Vec<(usize, usize, usize)>)> = Vec::new(); // (stream, new stride, [(src off, dst off, len)])
    for s in 0..MAX_STREAMS {
        if !streams_used[s] {
            continue;
        }
        let sbits = vertex_streams.get(s).copied().unwrap_or(0);
        let (sstride, sdiv) = (sbits & 0xFFFF_FFFF, sbits >> 32);
        let buf_idx = VB_BASE - s;
        let misaligned = sstride % 4 != 0 || attr_descs.iter().any(|a| a.2 == buf_idx && a.3 % 4 != 0);
        if sstride == 0 || sdiv != 0 || !misaligned {
            continue;
        }
        let mut cursor = 0usize;
        let mut copies = Vec::new();
        for a in attr_descs.iter_mut().filter(|a| a.2 == buf_idx) {
            let len = vertex_format_size(a.1);
            let dst = (cursor + 3) & !3;
            copies.push((a.3, dst, len));
            a.3 = dst;
            cursor = dst + len;
        }
        let new_stride = ((cursor + 3) & !3).max(4) as u64;
        if let Some(e) = streams_eff.get_mut(s) {
            *e = new_stride;
        }
        repack.push((s, new_stride, copies));
    }
    Layout { attr_descs, streams_used, needs_default_attr, streams_eff, repack }
}

/// Pipeline key marker of argument-buffer pipelines (ab_ variants).
const AB_KEY: u64 = 0xAB00_0000_0000_0000;

/// --arg-buffers, or replay side B with --replay-ab-args.
fn args_on() -> bool {
    static ON: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
    let (on, ab) = *ON.get_or_init(|| {
        let s = crate::settings::get();
        (s.graphics.arg_buffers, s.replay.replay_ab_args)
    });
    on || (ab && super::replay_b())
}

/// The struct of an argument-buffer variant (index line `ab`): per member the GPU address of the buffer the plain path
/// bound at that binding, or the resource ID of its texture / sampler. Unbound buffers point at the zero buffer.
unsafe fn compose_args(
    stage: &CachedStage,
    bufs: &[(Retained<ProtocolObject<dyn MTLBuffer>>, usize, usize)],
    tex: &[(Tex, usize)],
    samp: &[(Option<Retained<ProtocolObject<dyn MTLSamplerState>>>, usize)],
    zero: Option<&ProtocolObject<dyn MTLBuffer>>,
    res: &mut Vec<(Retained<ProtocolObject<dyn objc2_metal::MTLResource>>, objc2_metal::MTLResourceUsage)>,
) -> Vec<u64> {
    use objc2_metal::{MTLResource, MTLResourceUsage};
    let members = stage.args.as_deref().unwrap_or(&[]);
    let mut out = Vec::with_capacity(members.len());
    for &(class, b) in members {
        let b = b as usize;
        out.push(match class {
            b'c' | b'b' => {
                // Storage buffers the shader may write (older caches list them as c: by the stage's storage bindings).
                let written = class == b'b'
                    || (stage.sbuffers.iter().any(|s| s.0 as usize == b) && !stage.sb_readonly.iter().any(|&r| r as usize == b));
                let usage = if written { MTLResourceUsage::Read | MTLResourceUsage::Write } else { MTLResourceUsage::Read };
                let (buf, off) = match bufs.iter().rev().find(|x| x.2 == b) {
                    Some((buf, off, _)) => (Some(&**buf), *off),
                    None => (zero, 0),
                };
                match buf {
                    Some(buf) => {
                        res.push((ProtocolObject::<dyn MTLResource>::from_ref(buf).retain(), usage));
                        buf.gpuAddress() + off as u64
                    }
                    None => 0,
                }
            }
            b't' => match tex.iter().rev().find(|x| x.1 == b) {
                Some((t, _)) => {
                    res.push((ProtocolObject::<dyn MTLResource>::from_ref(&**t).retain(), MTLResourceUsage::Read));
                    std::mem::transmute::<_, u64>(t.gpuResourceID())
                }
                None => 0,
            },
            b's' => samp
                .iter()
                .rev()
                .find(|x| x.1 == b)
                .and_then(|(s, _)| s.as_ref())
                .map_or(0, |s| std::mem::transmute::<_, u64>(s.gpuResourceID())),
            _ => 0,
        });
    }
    out
}

/// The key of the render pipeline build_translated_pipeline makes from these inputs.
#[allow(clippy::too_many_arguments)]
pub(super) fn pipeline_key(
    point_size: f32,
    vs: &CachedStage,
    fs: Option<&CachedStage>,
    depth_gl: bool,
    color_state: u32,
    channel_mask: u32,
    depth_pf: MTLPixelFormat,
    color_pfs: &[MTLPixelFormat],
    blends: &[u64; 8],
    attr_descs: &[(u32, MTLVertexFormat, usize, usize, i32)],
    streams_used: &[bool; MAX_STREAMS],
    streams_eff: &[u64],
) -> smallvec::SmallVec<[u64; 64]> {
    // Only the state build_translated_pipeline reads: targets without a format take none, blend factors and equations
    // (bits 4..35) count where blending is enabled, write masks for the outputs the fragment shader writes. Draws that
    // differ only in state the pipeline ignores share it.
    let (mut cs, mut cm, mut bl) = (0u64, 0u64, [0u64; 8]);
    for (i, pf) in color_pfs.iter().enumerate().take(8) {
        if *pf == MTLPixelFormat::Invalid {
            continue;
        }
        if pf_kind(*pf) == 0 && (color_state >> i) & 1 != 0 {
            cs |= 1 << i;
            bl[i] = blends[i] & 0xF_FFFF_FFF0;
        }
        if fs.is_some_and(|f| f.colors.contains(&(i as u32))) {
            cm |= (((channel_mask >> (i * 4)) & 0xF) as u64) << (i * 4);
        }
    }
    let mut key: smallvec::SmallVec<[u64; 64]> = smallvec::smallvec![
        point_size.to_bits() as u64,
        vs as *const CachedStage as u64,
        fs.map_or(0, |f| f as *const CachedStage as u64),
        depth_gl as u64,
        cs,
        cm,
        depth_pf.0 as u64,
    ];
    key.extend(color_pfs.iter().map(|p| p.0 as u64));
    key.extend(bl);
    for (loc, fmt, buf, off, _) in attr_descs {
        key.extend([*loc as u64, fmt.0 as u64, *buf as u64, *off as u64]);
    }
    for s in 0..MAX_STREAMS {
        if streams_used[s] {
            key.push(0xFFFF_0000 | s as u64);
            key.push(streams_eff.get(s).copied().unwrap_or(0));
        }
    }
    key
}

pub(super) struct DrawPacket {
    pub(super) mode: i32,
    pub(super) prim: MTLPrimitiveType,
    pub(super) call: DrawKind,
    pub(super) vs: Option<&'static CachedStage>,
    pub(super) fs: Option<&'static CachedStage>,
    pub(super) vs_code: CodeRef,
    pub(super) fs_code: Option<CodeRef>,
    /// Textures resolved by the plan (feedback decisions), else None.
    pub(super) resolved_tex: Option<ResolvedTex>,
    /// Vertex attribute bits per entry of vs.attrs.
    pub(super) attr_bits: smallvec::SmallVec<[Option<u32>; 16]>,
    pub(super) vertex_streams: smallvec::SmallVec<[u64; 16]>,
    pub(super) vertex_buffers: [Option<(u64, usize)>; MAX_STREAMS],
    /// Non-empty storage buffer bindings of the two stages: (stage, index, address, size).
    pub(super) sbufs: smallvec::SmallVec<[(i32, i32, u64, usize); 8]>,
    /// Uniform buffers the shaders read: (stage, index, binding).
    pub(super) ubos: smallvec::SmallVec<[(i32, i32, Option<(u64, usize)>); 16]>,
    /// Texture handles the shaders read: (stage, handle-table index, handle).
    pub(super) handles: smallvec::SmallVec<[(i32, i32, Option<u64>); 16]>,
    /// How each uniform buffer binding (stage, binding) is bound, decided in command order (plan_uniforms).
    pub(super) ubo_pre: smallvec::SmallVec<[(i32, u32, UboPre); 16]>,
    /// Storage buffer descriptors the shaders read from their uniform buffers (stage, binding), read in command order.
    pub(super) sb_desc: smallvec::SmallVec<[(i32, u32, Option<(u64, u64)>); 8]>,
    pub(super) blend_states: [u64; 8],
    pub(super) color_state: u32,
    pub(super) channel_mask: u32,
    pub(super) point_size: f32,
    pub(super) blend_color: [f32; 4],
    pub(super) viewport: Option<(i32, i32, i32, i32)>,
    pub(super) scissor: Option<(i32, i32, i32, i32)>,
    pub(super) depth_range: (f32, f32),
    pub(super) depth_clamp: bool,
    pub(super) polygon_offset: (f32, f32, f32),
    pub(super) polygon: u32,
    pub(super) depth_stencil: u64,
    pub(super) stencil: [[u8; 2]; 3],
    pub(super) multisample: u64,
    pub(super) prim_restart: Option<u32>,
    pub(super) target_size: (f64, f64),
    pub(super) color_pfs: [MTLPixelFormat; 8],
    pub(super) depth_pf: MTLPixelFormat,
    /// The targets' NVN formats and sample count (CachedPassRts).
    pub(super) nvn_targets: ([(u32, u32); 8], (u32, u32), u32),
    /// Run as compute instead of in a render pass (decided by Gfx::tiny_plan).
    pub(super) tiny: Option<TinyPlan>,
    /// The draw came from a command list recorded in an earlier frame (--log-phases).
    pub(super) from_old_list: bool,
    /// A pass target was drawn in the previous frame too (State::enc_recur): a skip while the pipeline compiles is
    /// made up next frame; otherwise the draw waits for its pipeline.
    pub(super) targets_recur: bool,
}

/// A draw the tiny-pass path runs as compute: the command buffer, its color outputs (color index, target texture) and
/// the triangles over its vertex records.
pub(super) struct TinyPlan {
    pub(super) cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    pub(super) outputs: smallvec::SmallVec<[(usize, Tex); 8]>,
    pub(super) tris: [[u32; 4]; 2],
    pub(super) ntri: u32,
    pub(super) nverts: u32,
    /// Dispatch rectangle: x, y, width, height (viewport and scissor within the targets).
    pub(super) rect: (u32, u32, u32, u32),
    /// Pending clears folded into the kernel: (colour output, clear colour).
    pub(super) clears: smallvec::SmallVec<[(usize, [f32; 4]); 8]>,
}

// Metal objects may be used from any thread; a packet is handed to one thread at a time (plan, then the issuer thread,
// then back to the pool).
unsafe impl Send for DrawPacket {}

impl Default for DrawPacket {
    fn default() -> Self {
        DrawPacket {
            mode: 0,
            prim: MTLPrimitiveType::Triangle,
            call: DrawKind::Arrays {
                first: 0,
                count: 0,
                base_instance: 0,
                instances: 0,
            },
            vs: None,
            fs: None,
            vs_code: CodeRef {
                data: 0,
                control: 0,
            },
            fs_code: None,
            resolved_tex: None,
            attr_bits: smallvec::SmallVec::new(),
            vertex_streams: smallvec::SmallVec::new(),
            vertex_buffers: [None; MAX_STREAMS],
            sbufs: smallvec::SmallVec::new(),
            ubos: smallvec::SmallVec::new(),
            handles: smallvec::SmallVec::new(),
            ubo_pre: smallvec::SmallVec::new(),
            sb_desc: smallvec::SmallVec::new(),
            blend_states: [0; 8],
            color_state: 0,
            channel_mask: 0,
            point_size: 1.0,
            blend_color: [0.0; 4],
            viewport: None,
            scissor: None,
            depth_range: (0.0, 1.0),
            depth_clamp: false,
            polygon_offset: (0.0, 0.0, 0.0),
            polygon: 0,
            depth_stencil: 0,
            stencil: [[0; 2]; 3],
            multisample: 0,
            prim_restart: None,
            target_size: (0.0, 0.0),
            color_pfs: [MTLPixelFormat::Invalid; 8],
            depth_pf: MTLPixelFormat::Invalid,
            nvn_targets: ([(0, 0); 8], (0, 0), 1),
            tiny: None,
            from_old_list: false,
            targets_recur: false,
        }
    }
}

impl DrawPacket {
    /// Fills the packet in place (one packet is reused for every draw: moving a ~3 KB value per draw cost ~0.5 ms per frame).
    #[allow(clippy::too_many_arguments)]
    unsafe fn fill(
        &mut self,
        st: &State,
        mode: i32,
        prim: MTLPrimitiveType,
        call: DrawKind,
        vs: &'static CachedStage,
        fs: Option<&'static CachedStage>,
        vs_code: &super::programs::StageCode,
        fs_code: Option<&super::programs::StageCode>,
        resolved_tex: Option<ResolvedTex>,
        target_size: (f64, f64),
        color_pfs: [MTLPixelFormat; 8],
        depth_pf: MTLPixelFormat,
        nvn_targets: ([(u32, u32); 8], (u32, u32), u32),
    ) {
        self.nvn_targets = nvn_targets;
        self.mode = mode;
        self.prim = prim;
        self.call = call;
        self.from_old_list = st.in_old_list;
        self.targets_recur = st.enc_recur;
        self.vs = Some(vs);
        self.fs = fs;
        self.vs_code = CodeRef {
            data: vs_code.data,
            control: vs_code.control,
        };
        self.fs_code = fs_code.map(|c| CodeRef {
            data: c.data,
            control: c.control,
        });
        self.resolved_tex = resolved_tex;
        self.target_size = target_size;
        self.color_pfs = color_pfs;
        self.depth_pf = depth_pf;
        self.attr_bits.clear();
        self.vertex_buffers = [None; MAX_STREAMS];
        for &l in &vs.attrs {
            let bits = st.vertex_attribs.get(l as usize).copied();
            if let Some(b) = bits {
                let s = decode_attr(b).stream;
                if s < MAX_STREAMS && self.vertex_buffers[s].is_none() {
                    self.vertex_buffers[s] = st.vertex_buffers.get(&(s as i32)).copied();
                }
            }
            self.attr_bits.push(bits);
        }
        self.vertex_streams.clear();
        self.vertex_streams.extend_from_slice(&st.vertex_streams);
        self.sbufs.clear();
        self.ubos.clear();
        self.handles.clear();
        for (stage, cached) in [(STAGE_VERTEX, Some(vs)), (STAGE_FRAGMENT, fs)] {
            let Some(c) = cached else { continue };
            // Storage buffer descriptors: for the stage's storage buffers and its driver cbuf (slot 0).
            if !c.sbuffers.is_empty() || c.cbuffers.iter().any(|&(_, slot)| slot == 0) {
                for i in 0..16 {
                    if let Some((a, sz)) = super::storage_buffer(st, stage, i) {
                        self.sbufs.push((stage, i, a, sz));
                    }
                }
            }
            let ubos = &mut self.ubos;
            let mut want_ubo = |idx: i32| {
                if !ubos.iter().any(|u| u.0 == stage && u.1 == idx) {
                    ubos.push((stage, idx, st.uniform_buffers.get(&(stage, idx)).copied()));
                }
            };
            for &(_, slot) in &c.cbuffers {
                if slot >= 3 {
                    want_ubo(slot as i32 - 3);
                }
            }
            for &(_, _, cb_slot, _) in &c.sbuffers {
                if cb_slot >= 3 {
                    want_ubo(cb_slot as i32 - 3);
                }
            }
            for tb in &c.textures {
                let k = (tb.handle_index.wrapping_sub(8) / 2) as i32;
                self.handles
                    .push((stage, k, st.textures_bound.get(&(stage, k)).copied()));
            }
        }
        self.blend_states = st.blend_states;
        self.color_state = st.color_state;
        self.channel_mask = st.channel_mask;
        self.point_size = st.point_size;
        self.blend_color = st.blend_color;
        self.viewport = st.viewport;
        self.scissor = st.scissor;
        self.depth_range = st.depth_range;
        self.depth_clamp = st.depth_clamp;
        self.polygon_offset = st.polygon_offset;
        self.polygon = st.polygon;
        self.depth_stencil = st.depth_stencil;
        self.stencil = st.stencil;
        self.multisample = st.multisample;
        self.prim_restart = st.prim_restart;
    }

    /// Drops the packet's texture and buffer references (it stays allocated for the next draw).
    fn release(&mut self) {
        self.resolved_tex = None;
        self.ubo_pre.clear();
        self.tiny = None;
    }

    pub(super) fn ubo_pre(&self, stage: i32, binding: u32) -> Option<&UboPre> {
        self.ubo_pre
            .iter()
            .find(|u| u.0 == stage && u.1 == binding)
            .map(|u| &u.2)
    }

    pub(super) fn sb_desc(&self, stage: i32, binding: u32) -> Option<(u64, u64)> {
        self.sb_desc
            .iter()
            .find(|d| d.0 == stage && d.1 == binding)
            .and_then(|d| d.2)
    }

    pub(super) fn sbuf(&self, stage: i32, index: i32) -> Option<(u64, usize)> {
        self.sbufs
            .iter()
            .find(|b| b.0 == stage && b.1 == index)
            .map(|b| (b.2, b.3))
    }

    pub(super) fn ubo(&self, stage: i32, index: i32) -> Option<(u64, usize)> {
        self.ubos
            .iter()
            .find(|u| u.0 == stage && u.1 == index)
            .and_then(|u| u.2)
    }

    pub(super) fn handle(&self, stage: i32, k: i32) -> Option<&u64> {
        self.handles
            .iter()
            .find(|h| h.0 == stage && h.1 == k)
            .and_then(|h| h.2.as_ref())
    }
}

/// Size of a storage buffer whose descriptor a shader reads from one of the game's own uniform buffers. Only the
/// driver's descriptors in cbuf 0 carry a size; there the word after the address is whatever the game keeps next to it
/// (TotK's Depths terrain: a texture handle, read as 258 MB, so the binding failed and the walls were drawn from zeros).
/// The mapped size from the address instead, at most 1 MiB, as Ryujinx does.
fn user_sb_size(addr: u64) -> u64 {
    const MAX: usize = 0x100000;
    if addr <= 0x10000 {
        return 0;
    }
    let a = crate::nvn::api::translate_virtual(addr as usize);
    match crate::nvn::api::containing_pool(a, 1) {
        Some((base, size)) => (base + size).saturating_sub(a).min(MAX) as u64,
        // Outside every pool (not GPU-mapped on the console): as much as the copy path takes.
        None => (ARENA_CHUNK / 2) as u64,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum DrawKind {
    Arrays {
        first: i32,
        count: i32,
        base_instance: i32,
        instances: i32,
    },
    Elements {
        index_type: i32,
        count: i32,
        index_addr: u64,
        base_vertex: i32,
        base_instance: i32,
        instances: i32,
    },
}

/// Largest of `n` indices at `addr` that is not the restart index (None if there is none). Written as fixed-width
/// lanes with the restart value mapped to 0 so it vectorizes (this scan ran for most draws, ~8% of the encoding thread
/// as an iterator with a filter).
unsafe fn max_index(index_type: i32, addr: u64, n: usize, restart: Option<u32>) -> Option<u64> {
    fn lanes<T: Copy + Ord + Default + PartialEq, const L: usize>(v: &[T], r: Option<T>) -> T {
        let mut acc = [T::default(); L];
        let mut chunks = v.chunks_exact(L);
        match r {
            Some(r) => {
                for c in &mut chunks {
                    for k in 0..L {
                        let x = if c[k] == r { T::default() } else { c[k] };
                        acc[k] = acc[k].max(x);
                    }
                }
            }
            None => {
                for c in &mut chunks {
                    for k in 0..L {
                        acc[k] = acc[k].max(c[k]);
                    }
                }
            }
        }
        let mut m = acc.iter().copied().max().unwrap_or_default();
        for &x in chunks.remainder() {
            if Some(x) != r {
                m = m.max(x);
            }
        }
        m
    }
    if addr < 0x10000 || n == 0 {
        return None;
    }
    let (m, all_restart) = match index_type {
        0 => {
            let held = super::rt_guard::guest_copy::<u8>(addr as usize, n);
            let v = held.as_deref().unwrap_or_else(|| std::slice::from_raw_parts(addr as *const u8, n));
            let r = restart.filter(|&r| r <= 0xFF).map(|r| r as u8);
            let m = lanes::<u8, 32>(v, r);
            (
                m as u64,
                m == 0 && r.is_some() && v.iter().all(|&x| Some(x) == r),
            )
        }
        1 => {
            let held = super::rt_guard::guest_copy::<u16>(addr as usize, n);
            let v = held.as_deref().unwrap_or_else(|| std::slice::from_raw_parts(addr as *const u16, n));
            let r = restart.filter(|&r| r <= 0xFFFF).map(|r| r as u16);
            let m = lanes::<u16, 16>(v, r);
            (
                m as u64,
                m == 0 && r.is_some() && v.iter().all(|&x| Some(x) == r),
            )
        }
        _ => {
            let held = super::rt_guard::guest_copy::<u32>(addr as usize, n);
            let v = held.as_deref().unwrap_or_else(|| std::slice::from_raw_parts(addr as *const u32, n));
            let m = lanes::<u32, 8>(v, restart);
            (
                m as u64,
                m == 0 && restart.is_some() && v.iter().all(|&x| Some(x) == restart),
            )
        }
    };
    (!all_restart).then_some(m)
}

/// exact_max_vertex with the index scan memoized for the rest of the submit (EncodeCtx::max_index_memo, cleared at
/// each submit start): a mesh's index range is scanned once although several passes draw it (shadow cascades,
/// G-buffer). The executor encodes a submit when it is made, so a rescan would read the same bytes. --no-maxidx-memo:
/// scan every draw; --replay-ab-maxidx: scan on replay side B.
unsafe fn cached_max_vertex(
    memo: &mut HashMap<(u64, i32, i32, Option<u32>), Option<u64>>,
    call: &DrawKind,
    restart: Option<u32>,
) -> Option<u64> {
    let DrawKind::Elements {
        index_type,
        count,
        index_addr,
        base_vertex,
        ..
    } = *call
    else {
        return exact_max_vertex(call, restart);
    };
    if count <= 0 {
        return exact_max_vertex(call, restart);
    }
    let m = *memo
        .entry((index_addr, count, index_type, restart))
        .or_insert_with(|| max_index(index_type, index_addr, count as usize, restart));
    Some(m? + base_vertex.max(0) as u64)
}

/// Highest vertex index a draw fetches (base vertex included, restart indices excluded).
unsafe fn exact_max_vertex(call: &DrawKind, restart: Option<u32>) -> Option<u64> {
    match *call {
        DrawKind::Arrays { first, count, .. } => {
            Some((first.max(0) as u64 + count.max(1) as u64).saturating_sub(1))
        }
        DrawKind::Elements {
            index_type,
            count,
            index_addr,
            base_vertex,
            ..
        } => {
            if count <= 0 {
                return None;
            }
            Some(
                max_index(index_type, index_addr, count as usize, restart)?
                    + base_vertex.max(0) as u64,
            )
        }
    }
}

/// Bytes a vertex attribute of this Metal format occupies.
fn vertex_format_size(f: MTLVertexFormat) -> usize {
    use MTLVertexFormat as V;
    match f {
        V::UChar | V::Char | V::UCharNormalized | V::CharNormalized => 1,
        V::UChar2
        | V::Char2
        | V::UChar2Normalized
        | V::Char2Normalized
        | V::UShort
        | V::Short
        | V::UShortNormalized
        | V::ShortNormalized
        | V::Half => 2,
        V::UChar3 | V::Char3 | V::UChar3Normalized | V::Char3Normalized => 3,
        V::UChar4
        | V::Char4
        | V::UChar4Normalized
        | V::Char4Normalized
        | V::UShort2
        | V::Short2
        | V::UShort2Normalized
        | V::Short2Normalized
        | V::Half2
        | V::Float
        | V::Int
        | V::UInt
        | V::Int1010102Normalized
        | V::UInt1010102Normalized
        | V::UChar4Normalized_BGRA
        | V::FloatRG11B10
        | V::FloatRGB9E5 => 4,
        V::UShort3 | V::Short3 | V::UShort3Normalized | V::Short3Normalized | V::Half3 => 6,
        V::UShort4
        | V::Short4
        | V::UShort4Normalized
        | V::Short4Normalized
        | V::Half4
        | V::Float2
        | V::Int2
        | V::UInt2 => 8,
        V::Float3 | V::Int3 | V::UInt3 => 12,
        _ => 16,
    }
}

/// Highest vertex index a draw fetches from per-vertex streams (base vertex included, restart indices excluded), or
/// None when it cannot exceed a `size`-byte stream of `stride` (cheap bound first: no index scan for most draws).
unsafe fn max_fetched_vertex(
    call: &DrawKind,
    stride: u64,
    size: u64,
    exact: impl FnOnce() -> Option<u64>,
) -> Option<u64> {
    let fits = |v: u64| (v + 1) * stride <= size;
    match *call {
        DrawKind::Arrays { first, count, .. } => {
            let last = (first.max(0) as u64 + count.max(1) as u64).saturating_sub(1);
            (!fits(last)).then_some(last)
        }
        DrawKind::Elements {
            index_type,
            count,
            index_addr,
            base_vertex,
            ..
        } => {
            if index_addr < 0x10000 || count <= 0 {
                return None;
            }
            let type_max: u64 = match index_type {
                0 => 0xFF,
                1 => 0xFFFF,
                _ => u32::MAX as u64,
            };
            if fits(type_max + base_vertex.max(0) as u64) {
                return None;
            }
            let last = exact()?;
            (!fits(last)).then_some(last)
        }
    }
}

/// Per-submit scratch memory for data that has to be synthesized (driver cbuf, u8 indices, ...).
#[derive(Default)]
pub(crate) struct Arena {
    chunks: Vec<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)>,
    /// Standard-size chunks of earlier submits, with the submitted serial at their reset: reusable once the GPU has
    /// completed that serial (a new 1 MB Metal buffer per chunk per submit showed in profiles).
    free: Vec<(Retained<ProtocolObject<dyn MTLBuffer>>, u64)>,
}

impl Arena {
    pub(super) fn reset(&mut self) {
        // Every command buffer that used these chunks is committed by now (a reset starts a submit).
        let tag = super::submitted_serial();
        for (b, _) in self.chunks.drain(..) {
            if b.length() == ARENA_CHUNK
                && self.free.len() < 16
            {
                self.free.push((b, tag));
            }
        }
    }

    fn alloc(
        &mut self,
        device: &Device,
        data: &[u8],
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        // Guest memory on a page --report-guard holds is read through its alias (report_guard::engine_copy).
        self.alloc_with(device, data.len(), |dst| unsafe {
            super::report_guard::engine_copy(dst, data.as_ptr())
        })
    }

    /// Allocates `len` bytes and lets `fill` write them in place (it must write all of them: the memory is reused).
    fn alloc_with(
        &mut self,
        device: &Device,
        len: usize,
        fill: impl FnOnce(&mut [u8]),
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        let need = (len + 255) & !255;
        let fits = |used: usize, size: usize| used + need + CBUF_DECL_SIZE <= size;
        if self
            .chunks
            .last()
            .map_or(true, |(b, used)| !fits(*used, b.length()))
        {
            let size = ARENA_CHUNK.max(need + CBUF_DECL_SIZE + 256);
            let done = super::completed_serial();
            let reuse = if size == ARENA_CHUNK {
                self.free.iter().position(|(_, tag)| *tag <= done)
            } else {
                None
            };
            let buf = match reuse {
                Some(i) => self.free.swap_remove(i).0,
                None => device.newBufferWithLength_options(size, MTLResourceOptions::empty())?,
            };
            self.chunks.push((buf, 0));
        }
        let (buf, used) = self.chunks.last_mut()?;
        let off = *used;
        unsafe {
            fill(std::slice::from_raw_parts_mut(
                (buf.contents().as_ptr() as *mut u8).add(off),
                len,
            ));
        }
        *used += need;
        Some((buf.clone(), off))
    }
}

// ---- NVN format tables ----------------------------------------------------------------------------------------

/// Class of a vertex attribute as the shader's raw inputs see it: 0 float, 1 uint, 2 sint.
fn vertex_format(nvn: u32) -> Option<(MTLVertexFormat, i32)> {
    use MTLVertexFormat as V;
    Some(match nvn {
        1 => (V::UCharNormalized, 0),
        2 => (V::CharNormalized, 0),
        3 => (V::UChar, 1),
        4 => (V::Char, 2),
        5 => (V::Half, 0),
        6 => (V::UShortNormalized, 0),
        7 => (V::ShortNormalized, 0),
        8 => (V::UShort, 1),
        9 => (V::Short, 2),
        10 => (V::Float, 0),
        11 => (V::UInt, 1),
        12 => (V::Int, 2),
        13 => (V::UChar2Normalized, 0),
        14 => (V::Char2Normalized, 0),
        15 => (V::UChar2, 1),
        16 => (V::Char2, 2),
        17 => (V::Half2, 0),
        18 => (V::UShort2Normalized, 0),
        19 => (V::Short2Normalized, 0),
        20 => (V::UShort2, 1),
        21 => (V::Short2, 2),
        22 => (V::Float2, 0),
        23 => (V::UInt2, 1),
        24 => (V::Int2, 2),
        25 => (V::UChar3Normalized, 0),
        26 => (V::Char3Normalized, 0),
        27 => (V::UChar3, 1),
        28 => (V::Char3, 2),
        29 => (V::Half3, 0),
        30 => (V::UShort3Normalized, 0),
        31 => (V::Short3Normalized, 0),
        32 => (V::UShort3, 1),
        33 => (V::Short3, 2),
        34 => (V::Float3, 0),
        35 => (V::UInt3, 1),
        36 => (V::Int3, 2),
        37 | 109 => (V::UChar4Normalized, 0),
        38 | 110 => (V::Char4Normalized, 0),
        39 | 111 => (V::UChar4, 1),
        40 | 112 => (V::Char4, 2),
        41 | 113 => (V::Half4, 0),
        42 | 114 => (V::UShort4Normalized, 0),
        43 | 115 => (V::Short4Normalized, 0),
        44 | 116 => (V::UShort4, 1),
        45 | 117 => (V::Short4, 2),
        46 | 118 => (V::Float4, 0),
        47 | 119 => (V::UInt4, 1),
        48 | 120 => (V::Int4, 2),
        61 => (V::UInt1010102Normalized, 0),
        105 => (V::Int1010102Normalized, 0),
        63 => (V::FloatRG11B10, 0),
        64 => (V::FloatRGB9E5, 0),
        // *_UI2F / *_I2F: integer data converted to float by the fetch unit; Metal value-converts an integer
        // vertex format feeding a float input, which is the same operation.
        81 => (V::UChar, 0),
        82 => (V::Char, 0),
        83 => (V::UShort, 0),
        84 => (V::Short, 0),
        85 => (V::UInt, 0),
        86 => (V::Int, 0),
        87 => (V::UChar2, 0),
        88 => (V::Char2, 0),
        89 => (V::UShort2, 0),
        90 => (V::Short2, 0),
        91 => (V::UInt2, 0),
        92 => (V::Int2, 0),
        93 => (V::UChar3, 0),
        94 => (V::Char3, 0),
        95 => (V::UShort3, 0),
        96 => (V::Short3, 0),
        97 => (V::UInt3, 0),
        98 => (V::Int3, 0),
        99 => (V::UChar4, 0),
        100 => (V::Char4, 0),
        101 => (V::UShort4, 0),
        102 => (V::Short4, 0),
        103 => (V::UInt4, 0),
        104 => (V::Int4, 0),
        _ => return None,
    })
}

pub(super) fn pf_kind(pf: MTLPixelFormat) -> i32 {
    use MTLPixelFormat as P;
    match pf {
        P::R8Uint
        | P::R16Uint
        | P::R32Uint
        | P::RG8Uint
        | P::RG16Uint
        | P::RG32Uint
        | P::RGBA8Uint
        | P::RGBA16Uint
        | P::RGBA32Uint
        | P::RGB10A2Uint => 1,
        P::R8Sint
        | P::R16Sint
        | P::R32Sint
        | P::RG8Sint
        | P::RG16Sint
        | P::RG32Sint
        | P::RGBA8Sint
        | P::RGBA16Sint
        | P::RGBA32Sint => 2,
        _ => 0,
    }
}

fn compare_func(nvn: u32) -> MTLCompareFunction {
    use MTLCompareFunction as C;
    match nvn {
        1 => C::Never,
        2 => C::Less,
        3 => C::Equal,
        4 => C::LessEqual,
        5 => C::Greater,
        6 => C::NotEqual,
        7 => C::GreaterEqual,
        _ => C::Always,
    }
}

fn address_mode(nvn: i32) -> MTLSamplerAddressMode {
    use MTLSamplerAddressMode as A;
    match nvn {
        0 | 7 => A::ClampToEdge,
        1 => A::Repeat,
        2 | 3 | 4 => A::MirrorClampToEdge,
        5 => A::ClampToBorderColor,
        6 => A::MirrorRepeat,
        _ => A::ClampToEdge,
    }
}

struct VAttr {
    stream: usize,
    format: u32,
    offset: u32,
}

fn decode_attr(bits: u32) -> VAttr {
    VAttr {
        stream: (bits & 31) as usize,
        format: (bits >> 5) & 0x1FF,
        offset: bits >> 14,
    }
}

// ---- executor ---------------------------------------------------------------------------------------------------

pub(super) fn drop_released_pool_buffers(released: &[(usize, usize, bool)]) {
    if released.is_empty() {
        return;
    }
    let mut pool_buffers = POOL_BUFFERS.lock().unwrap();
    let pool_buffers = &mut pool_buffers
        .get_or_insert_with(|| PoolBuffers(HashMap::default()))
        .0;
    for &(rb, rs, unmapped) in released {
        let before = pool_buffers.len();
        pool_buffers.retain(|&(b, e), _| e <= rb || b >= rb + rs);
        if unmapped && pool_buffers.len() != before {
            // Wrapped again after the free: without this drop the buffer would have kept the old pages.
            println!("[+] Gfx: dropped {} zero-copy buffer(s) over unmapped memory 0x{:x}+0x{:x} (wrapped after the free)", before - pool_buffers.len(), rb, rs);
        }
    }
}


/// The render pipeline descriptor of a translated draw (shaders, function constants, vertex layout, targets, blending);
/// `y_negate`: the window origin the game uses (NVN's lower-left origin flips y).
pub(super) fn pipeline_descriptor(
    device: &super::Device,
    lib: &ShaderLibs,
    vs: &CachedStage,
    fs: Option<&CachedStage>,
    attrs: &[(u32, MTLVertexFormat, usize, usize, i32)],
    streams_used: &[bool; MAX_STREAMS],
    vertex_streams: &[u64],
    color_pfs: &[MTLPixelFormat],
    depth_pf: MTLPixelFormat,
    depth_gl: bool,
    y_negate: bool,
    point_size: f32,
    blends: &[u64; 8],
    color_state: u32,
    channel_mask: u32,
    batched: bool,
    args: bool,
) -> Option<Retained<MTLRenderPipelineDescriptor>> {

    let set_int = |cv: &MTLFunctionConstantValues, id: usize, v: i32| unsafe {
        cv.setConstantValue_type_atIndex(
            std::ptr::NonNull::new(&v as *const i32 as *mut libc::c_void).unwrap(),
            MTLDataType::Int,
            id,
        );
    };

    let vcv = MTLFunctionConstantValues::new();
    for (loc, _, _, _, kind) in attrs {
        set_int(&vcv, *loc as usize, *kind);
    }
    unsafe {
        let b = depth_gl;
        vcv.setConstantValue_type_atIndex(
            std::ptr::NonNull::new(&b as *const bool as *mut libc::c_void).unwrap(),
            MTLDataType::Bool,
            40,
        );
        let yn = y_negate;
        vcv.setConstantValue_type_atIndex(
            std::ptr::NonNull::new(&yn as *const bool as *mut libc::c_void).unwrap(),
            MTLDataType::Bool,
            41,
        );
        vcv.setConstantValue_type_atIndex(
            std::ptr::NonNull::new(&point_size as *const f32 as *mut libc::c_void).unwrap(),
            MTLDataType::Float,
            42,
        );
    }
    // Batched pipelines (metal/batch.rs) use the stages' tb_<entry> variants; a fragment stage without one reads no
    // constant buffer and stays as it is.
    let vres = if batched {
        lib.kernel(&vs.entry, &format!("tb_{}", vs.entry), &vcv)
    } else if args {
        lib.kernel(&vs.entry, &format!("ab_{}", vs.entry), &vcv)
    } else {
        lib.function(&vs.entry, &vcv)
    };
    let mut vfn = match vres {
        Ok(f) => f,
        Err(e) => {
            println!("[!] Gfx: vertex function {}: {}", vs.entry, e);
            return None;
        }
    };
    // Debug: --debug-vs-file=<path.metal> replaces the vertex function whose entry name the file defines (an
    // edited copy of a translated shader), compiled at runtime with the same function constants.
    // A comma-separated list of files replaces each vertex function one of them defines.
    if let Some(src) = crate::settings::get()
        .debug
        .debug_vs_file
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find(|src| src.contains(&format!("vertex {}_out {}(", vs.entry, vs.entry)))
    {
        match device
            .newLibraryWithSource_options_error(&NSString::from_str(&src), None)
        {
            Ok(l) => match l
                .newFunctionWithName_constantValues_error(&NSString::from_str(&vs.entry), &vcv)
            {
                Ok(f) => {
                    println!(
                        "[!] Gfx: --debug-vs-file: {} replaced by the debug source",
                        vs.entry
                    );
                    vfn = f;
                }
                Err(e) => println!("[!] Gfx: debug VS function: {}", e),
            },
            Err(e) => println!("[!] Gfx: debug VS source: {}", e),
        }
    }

    let desc = MTLRenderPipelineDescriptor::new();
    desc.setVertexFunction(Some(&vfn));

    if let Some(fs) = fs {
        let fcv = MTLFunctionConstantValues::new();
        for &loc in &fs.colors {
            let pf = color_pfs
                .get(loc as usize)
                .copied()
                .unwrap_or(MTLPixelFormat::Invalid);
            set_int(&fcv, 32 + loc as usize, pf_kind(pf));
        }
        let set_bool = |cv: &MTLFunctionConstantValues, id: usize, v: bool| unsafe {
            cv.setConstantValue_type_atIndex(
                std::ptr::NonNull::new(&v as *const bool as *mut libc::c_void).unwrap(),
                MTLDataType::Bool,
                id,
            );
        };
        // A depth write with no depth target is dropped (as on the GPU); Metal rejects the pipeline otherwise.
        set_bool(&fcv, 43, depth_pf != MTLPixelFormat::Invalid);
        // Varyings the vertex stage does not write read as zero (Metal rejects an unmatched fragment input).
        for &loc in &fs.varyings {
            set_bool(&fcv, 48 + loc as usize, vs.varyings.contains(&loc));
        }
        let fres = if batched && fs.batch_cbufs.is_some() {
            lib.kernel(&fs.entry, &format!("tb_{}", fs.entry), &fcv)
        } else if args && fs.args.is_some() {
            lib.kernel(&fs.entry, &format!("ab_{}", fs.entry), &fcv)
        } else {
            lib.function(&fs.entry, &fcv)
        };
        let ffn = match fres {
            Ok(f) => f,
            Err(e) => {
                println!("[!] Gfx: fragment function {}: {}", fs.entry, e);
                return None;
            }
        };
        // Debug: --debug-red-fs=<fragment entry>[,..] draws solid red instead (separates geometry from shading).
        let red = crate::settings::get()
            .debug
            .debug_red_fs
            .as_deref()
            .is_some_and(|l| l.split(',').any(|e| e == fs.entry));
        // Debug: --debug-fs-file=<path.metal> replaces the fragment function whose entry name the file defines
        // (an edited copy of a translated shader), compiled at runtime with the same function constants.
        let debug_src = crate::settings::get()
            .debug
            .debug_fs_file
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .find(|src| src.contains(&format!("fragment {}_out {}(", fs.entry, fs.entry)));
        if let Some(src) = debug_src {
            match device
                .newLibraryWithSource_options_error(&NSString::from_str(&src), None)
            {
                Ok(l) => match l.newFunctionWithName_constantValues_error(
                    &NSString::from_str(&fs.entry),
                    &fcv,
                ) {
                    Ok(f) => {
                        println!(
                            "[!] Gfx: --debug-fs-file: {} replaced by the debug source",
                            fs.entry
                        );
                        desc.setFragmentFunction(Some(&f));
                    }
                    Err(e) => println!("[!] Gfx: debug FS function: {}", e),
                },
                Err(e) => println!("[!] Gfx: debug FS source: {}", e),
            }
        } else if red {
            let src = "#include <metal_stdlib>\nusing namespace metal;\nfragment float4 dbg_red() { return float4(1.0, 0.0, 0.0, 1.0); }\n";
            match device
                .newLibraryWithSource_options_error(&NSString::from_str(src), None)
            {
                Ok(l) => {
                    if let Some(f) = l.newFunctionWithName(&NSString::from_str("dbg_red")) {
                        println!(
                            "[!] Gfx: --debug-red-fs: {} replaced with solid red",
                            fs.entry
                        );
                        desc.setFragmentFunction(Some(&f));
                    }
                }
                Err(e) => println!("[!] Gfx: debug red shader: {}", e),
            }
        } else {
            desc.setFragmentFunction(Some(&ffn));
        }
    }

    // Vertex descriptor.
    let vd = MTLVertexDescriptor::new();
    unsafe {
        for (loc, fmt, buf, off, _) in attrs {
            let a = vd.attributes().objectAtIndexedSubscript(*loc as usize);
            a.setFormat(*fmt);
            a.setOffset(*off);
            a.setBufferIndex(*buf);
        }
        for s in 0..MAX_STREAMS {
            if !streams_used[s] {
                continue;
            }
            let bits = vertex_streams.get(s).copied().unwrap_or(0);
            let (stride, divisor) = ((bits & 0xFFFF_FFFF) as usize, (bits >> 32) as usize);
            let l = vd.layouts().objectAtIndexedSubscript(VB_BASE - s);
            if stride == 0 {
                l.setStepFunction(MTLVertexStepFunction::Constant);
                l.setStepRate(0);
                l.setStride(16);
            } else if divisor == 0 {
                l.setStepFunction(MTLVertexStepFunction::PerVertex);
                l.setStepRate(1);
                l.setStride(stride);
            } else {
                l.setStepFunction(MTLVertexStepFunction::PerInstance);
                l.setStepRate(divisor);
                l.setStride(stride);
            }
        }
        if attrs.iter().any(|a| a.2 == DEFAULT_ATTR_BUFFER) {
            let l = vd.layouts().objectAtIndexedSubscript(DEFAULT_ATTR_BUFFER);
            l.setStepFunction(MTLVertexStepFunction::Constant);
            l.setStepRate(0);
            l.setStride(16);
        }
    }
    if !attrs.is_empty() {
        desc.setVertexDescriptor(Some(&vd));
    }

    // Blend state: factors/equations from NVNblendState, per-target enable from NVNcolorState, write masks from
    // NVNchannelMaskState (4 bits per target: r, g, b, a).
    unsafe {
        for (i, pf) in color_pfs.iter().enumerate() {
            if *pf == MTLPixelFormat::Invalid {
                continue;
            }
            let blend = blends[i];
            let src_rgb = nvn_blend_factor(((blend >> 4) & 0x3F) as u32);
            let dst_rgb = nvn_blend_factor(((blend >> 10) & 0x3F) as u32);
            let src_a = nvn_blend_factor(((blend >> 16) & 0x3F) as u32);
            let dst_a = nvn_blend_factor(((blend >> 22) & 0x3F) as u32);
            let rgb_op = nvn_blend_op(((blend >> 28) & 0xF) as u32);
            let a_op = nvn_blend_op(((blend >> 32) & 0xF) as u32);
            let ca = desc.colorAttachments().objectAtIndexedSubscript(i);
            ca.setPixelFormat(*pf);
            let blendable = pf_kind(*pf) == 0;
            ca.setBlendingEnabled(blendable && (color_state >> i) & 1 != 0);
            ca.setRgbBlendOperation(rgb_op);
            ca.setAlphaBlendOperation(a_op);
            ca.setSourceRGBBlendFactor(src_rgb);
            ca.setDestinationRGBBlendFactor(dst_rgb);
            ca.setSourceAlphaBlendFactor(src_a);
            ca.setDestinationAlphaBlendFactor(dst_a);
            // Outputs the fragment shader never writes must leave the target untouched (Maxwell keeps it; Apple GPUs
            // store undefined data, in practice zeros, which wiped G-buffer targets in passes with fewer outputs).
            let written = fs.map_or(false, |f| f.colors.contains(&(i as u32)));
            let m = if written {
                (channel_mask >> (i * 4)) & 0xF
            } else {
                0
            };
            let mut mask = MTLColorWriteMask::empty();
            if m & 1 != 0 {
                mask |= MTLColorWriteMask::Red;
            }
            if m & 2 != 0 {
                mask |= MTLColorWriteMask::Green;
            }
            if m & 4 != 0 {
                mask |= MTLColorWriteMask::Blue;
            }
            if m & 8 != 0 {
                mask |= MTLColorWriteMask::Alpha;
            }
            ca.setWriteMask(mask);
        }
    }
    if depth_pf != MTLPixelFormat::Invalid {
        desc.setDepthAttachmentPixelFormat(depth_pf);
        if matches!(
            depth_pf,
            MTLPixelFormat::Depth32Float_Stencil8 | MTLPixelFormat::Stencil8
        ) {
            desc.setStencilAttachmentPixelFormat(depth_pf);
        }
    }
    if fs.is_none() {
        desc.setRasterizationEnabled(false);
    }

    Some(desc)
}

/// NVNblendState defaults (nvnBlendStateSetDefaults): ONE/ZERO factors, ADD equations.
pub(super) const DEFAULT_BLEND_STATE: u64 =
    (2 << 4) | (1 << 10) | (2 << 16) | (1 << 22) | (1 << 28) | (1u64 << 32);

#[cfg(test)]
mod index_scan_tests {
    use super::max_index;

    fn naive(v: &[u64], r: Option<u64>) -> Option<u64> {
        v.iter().copied().filter(|&x| Some(x) != r).max()
    }

    #[test]
    fn max_index_matches_naive() {
        let mut seed = 0x1234_5678u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for n in [1usize, 2, 7, 15, 16, 17, 31, 33, 64, 100, 1000] {
            for case in 0..6 {
                let v16: Vec<u16> = (0..n)
                    .map(|_| match case {
                        0 => (rnd() % 600) as u16,
                        1 => {
                            if rnd() % 3 == 0 {
                                0xFFFF
                            } else {
                                (rnd() % 600) as u16
                            }
                        }
                        2 => 0xFFFF,
                        3 => 0,
                        4 => {
                            if rnd() % 2 == 0 {
                                0xFFFF
                            } else {
                                0
                            }
                        }
                        _ => (rnd() % 0x10000) as u16,
                    })
                    .collect();
                let wide: Vec<u64> = v16.iter().map(|&x| x as u64).collect();
                for r in [None, Some(0xFFFFu32), Some(0u32)] {
                    let got = unsafe { max_index(1, v16.as_ptr() as u64, n, r) };
                    assert_eq!(
                        got,
                        naive(&wide, r.map(|x| x as u64)),
                        "u16 n={} case={} r={:?}",
                        n,
                        case,
                        r
                    );
                }
                let v32: Vec<u32> = v16
                    .iter()
                    .map(|&x| if x == 0xFFFF { u32::MAX } else { x as u32 * 3 })
                    .collect();
                let wide32: Vec<u64> = v32.iter().map(|&x| x as u64).collect();
                for r in [None, Some(u32::MAX)] {
                    let got = unsafe { max_index(2, v32.as_ptr() as u64, n, r) };
                    assert_eq!(
                        got,
                        naive(&wide32, r.map(|x| x as u64)),
                        "u32 n={} case={}",
                        n,
                        case
                    );
                }
                let v8: Vec<u8> = v16.iter().map(|&x| x as u8).collect();
                let wide8: Vec<u64> = v8.iter().map(|&x| x as u64).collect();
                for r in [None, Some(0xFFu32), Some(0xFFFFu32)] {
                    let got = unsafe { max_index(0, v8.as_ptr() as u64, n, r) };
                    let rr = r.filter(|&x| x <= 0xFF).map(|x| x as u64);
                    assert_eq!(got, naive(&wide8, rr), "u8 n={} case={} r={:?}", n, case, r);
                }
            }
        }
    }
}

/// What the open render encoder has bound, so draws skip Objective-C calls for state that did not change (TotK's
/// consecutive draws share most of it; every draw used to set ~11 fixed-function states and every binding). Reset when
/// an encoder is created. Objects are compared by pointer: the encoder retains what it was given, so a pointer cannot
/// be reused for another object while it is bound.
pub(crate) struct EncCache {
    pipe: usize,
    viewport: [u64; 6],
    scissor: (usize, usize, usize, usize),
    cull: isize,
    clip: isize,
    winding: isize,
    fill: isize,
    bias: [u32; 3],
    ds: usize,
    stencil_ref: u32,
    blend: [u32; 4],
    vbuf: [(usize, usize); 32],
    fbuf: [(usize, usize); 32],
    vtex: [usize; 32],
    ftex: [usize; 32],
    vsamp: [usize; 32],
    fsamp: [usize; 32],
    /// Tiny-pass compute mode: bindings are recorded per stage for the kernels instead of being
    /// set on a render encoder, fixed-function state is ignored.
    pub(super) rec: Option<TinyRec>,
    /// Argument-buffer draws (--arg-buffers): resource bindings are recorded for the draw's structs; fixed-function
    /// state is set as usual.
    pub(super) bind_rec: Option<TinyRec>,
    /// Resources made resident for heap reads in this encoder (Op::Use), by pointer.
    heap_used: rustc_hash::FxHashSet<usize>,
}

/// Bindings encode_draw made for a draw that runs as compute: (object, offset / none, index) per stage.
#[derive(Default)]
pub(super) struct TinyRec {
    pub(super) vbufs: Vec<(Retained<ProtocolObject<dyn MTLBuffer>>, usize, usize)>,
    pub(super) fbufs: Vec<(Retained<ProtocolObject<dyn MTLBuffer>>, usize, usize)>,
    pub(super) vtex: Vec<(Tex, usize)>,
    pub(super) ftex: Vec<(Tex, usize)>,
    pub(super) vsamp: Vec<(Option<Retained<ProtocolObject<dyn MTLSamplerState>>>, usize)>,
    pub(super) fsamp: Vec<(Option<Retained<ProtocolObject<dyn MTLSamplerState>>>, usize)>,
    /// Resources the stages read through descriptor heaps (made resident, not bound).
    pub(super) uses: Vec<Retained<ProtocolObject<dyn objc2_metal::MTLResource>>>,
}

impl Default for EncCache {
    fn default() -> Self {
        EncCache {
            pipe: 0,
            viewport: [u64::MAX; 6],
            scissor: (usize::MAX, 0, 0, 0),
            cull: -1,
            clip: -1,
            winding: -1,
            fill: -1,
            bias: [u32::MAX; 3],
            ds: 0,
            stencil_ref: u32::MAX,
            blend: [u32::MAX; 4],
            vbuf: [(0, usize::MAX); 32],
            fbuf: [(0, usize::MAX); 32],
            vtex: [usize::MAX; 32],
            ftex: [usize::MAX; 32],
            vsamp: [usize::MAX; 32],
            fsamp: [usize::MAX; 32],
            rec: None,
            bind_rec: None,
            heap_used: Default::default(),
        }
    }
}

type Enc = super::issuer::Sink;
use super::issuer::Op;
use objc2::Message as _;
fn ptr_of<T: ?Sized>(o: Option<&T>) -> usize {
    o.map_or(0, |r| r as *const T as *const () as usize)
}

impl EncCache {
    /// Makes a resource read through the descriptor heaps resident (once per encoder; recorded for kernels).
    pub(super) unsafe fn heap_use(
        &mut self,
        enc: &Enc,
        r: &ProtocolObject<dyn objc2_metal::MTLResource>,
    ) {
        if let Some(rec) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            rec.uses.push(r.retain());
            return;
        }
        if self.heap_used.insert(ptr_of(Some(r))) {
            enc.push(Op::Use(r.retain(), objc2_metal::MTLResourceUsage::Read));
        }
    }
    pub(super) unsafe fn pipeline(
        &mut self,
        enc: &Enc,
        p: &ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>,
    ) {
        if self.rec.is_some() {
            return;
        }
        let k = ptr_of(Some(p));
        if self.pipe != k {
            self.pipe = k;
            enc.push(Op::Pipeline(p.retain()));
        }
    }
    pub(super) unsafe fn viewport(&mut self, enc: &Enc, v: MTLViewport) {
        if self.rec.is_some() {
            return;
        }
        let k = [
            v.originX.to_bits(),
            v.originY.to_bits(),
            v.width.to_bits(),
            v.height.to_bits(),
            v.znear.to_bits(),
            v.zfar.to_bits(),
        ];
        if self.viewport != k {
            self.viewport = k;
            enc.push(Op::Viewport(v));
        }
    }
    pub(super) unsafe fn scissor(&mut self, enc: &Enc, r: MTLScissorRect) {
        if self.rec.is_some() {
            return;
        }
        let k = (r.x, r.y, r.width, r.height);
        if self.scissor != k {
            self.scissor = k;
            enc.push(Op::Scissor(r));
        }
    }
    pub(super) unsafe fn cull(&mut self, enc: &Enc, m: MTLCullMode) {
        if self.rec.is_some() {
            return;
        }
        if self.cull != m.0 as isize {
            self.cull = m.0 as isize;
            enc.push(Op::Cull(m));
        }
    }
    pub(super) unsafe fn clip(&mut self, enc: &Enc, m: objc2_metal::MTLDepthClipMode) {
        if self.rec.is_some() {
            return;
        }
        if self.clip != m.0 as isize {
            self.clip = m.0 as isize;
            enc.push(Op::Clip(m));
        }
    }
    pub(super) unsafe fn winding(&mut self, enc: &Enc, w: MTLWinding) {
        if self.rec.is_some() {
            return;
        }
        if self.winding != w.0 as isize {
            self.winding = w.0 as isize;
            enc.push(Op::Winding(w));
        }
    }
    pub(super) unsafe fn fill(&mut self, enc: &Enc, f: MTLTriangleFillMode) {
        if self.rec.is_some() {
            return;
        }
        if self.fill != f.0 as isize {
            self.fill = f.0 as isize;
            enc.push(Op::Fill(f));
        }
    }
    pub(super) unsafe fn bias(&mut self, enc: &Enc, units: f32, factor: f32, clamp: f32) {
        if self.rec.is_some() {
            return;
        }
        let k = [units.to_bits(), factor.to_bits(), clamp.to_bits()];
        if self.bias != k {
            self.bias = k;
            enc.push(Op::Bias(units, factor, clamp));
        }
    }
    pub(super) unsafe fn stencil_ref(&mut self, enc: &Enc, r: [u8; 2]) {
        if self.rec.is_some() {
            return;
        }
        let k = r[0] as u32 | (r[1] as u32) << 8;
        if self.stencil_ref != k {
            self.stencil_ref = k;
            enc.push(Op::StencilRef(r[0] as u32, r[1] as u32));
        }
    }
    pub(super) unsafe fn depth_stencil(
        &mut self,
        enc: &Enc,
        d: &ProtocolObject<dyn MTLDepthStencilState>,
    ) {
        if self.rec.is_some() {
            return;
        }
        let k = ptr_of(Some(d));
        if self.ds != k {
            self.ds = k;
            enc.push(Op::DepthStencil(d.retain()));
        }
    }
    pub(super) unsafe fn blend(&mut self, enc: &Enc, c: [f32; 4]) {
        if self.rec.is_some() {
            return;
        }
        let k = c.map(f32::to_bits);
        if self.blend != k {
            self.blend = k;
            enc.push(Op::Blend(c));
        }
    }
    // The same buffer at a new offset only moves the offset (setVertexBufferOffset: no retain here, no resource-list
    // work in the driver): uniform copies of consecutive draws sit in one arena chunk. --no-buf-offset-ops: off.
    pub(super) unsafe fn vbuf(
        &mut self,
        enc: &Enc,
        b: &ProtocolObject<dyn MTLBuffer>,
        off: usize,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.vbufs.push((b.retain(), off, i));
            return;
        }
        let k = (ptr_of(Some(b)), off);
        if i < 32 && self.vbuf[i] == k {
            return;
        }
        let same_buf = i < 32 && self.vbuf[i].0 == k.0 && k.0 != 0;
        if i < 32 {
            self.vbuf[i] = k;
        }
        enc.push(if same_buf {
            Op::VBufOff(off, i)
        } else {
            Op::VBuf(b.retain(), off, i)
        });
    }
    pub(super) unsafe fn fbuf(
        &mut self,
        enc: &Enc,
        b: &ProtocolObject<dyn MTLBuffer>,
        off: usize,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.fbufs.push((b.retain(), off, i));
            return;
        }
        let k = (ptr_of(Some(b)), off);
        if i < 32 && self.fbuf[i] == k {
            return;
        }
        let same_buf = i < 32 && self.fbuf[i].0 == k.0 && k.0 != 0;
        if i < 32 {
            self.fbuf[i] = k;
        }
        enc.push(if same_buf {
            Op::FBufOff(off, i)
        } else {
            Op::FBuf(b.retain(), off, i)
        });
    }
    pub(super) unsafe fn vtex(
        &mut self,
        enc: &Enc,
        t: &ProtocolObject<dyn objc2_metal::MTLTexture>,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.vtex.push((t.retain(), i));
            return;
        }
        let k = ptr_of(Some(t));
        if i < 32 && self.vtex[i] == k {
            return;
        }
        if i < 32 {
            self.vtex[i] = k;
        }
        enc.push(Op::VTex(t.retain(), i));
    }
    pub(super) unsafe fn ftex(
        &mut self,
        enc: &Enc,
        t: &ProtocolObject<dyn objc2_metal::MTLTexture>,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.ftex.push((t.retain(), i));
            return;
        }
        let k = ptr_of(Some(t));
        if i < 32 && self.ftex[i] == k {
            return;
        }
        if i < 32 {
            self.ftex[i] = k;
        }
        enc.push(Op::FTex(t.retain(), i));
    }
    pub(super) unsafe fn vsamp(
        &mut self,
        enc: &Enc,
        s: Option<&ProtocolObject<dyn MTLSamplerState>>,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.vsamp.push((s.map(|x| x.retain()), i));
            return;
        }
        let k = ptr_of(s);
        if i < 32 && self.vsamp[i] == k {
            return;
        }
        if i < 32 {
            self.vsamp[i] = k;
        }
        enc.push(Op::VSamp(s.map(|x| x.retain()), i));
    }
    pub(super) unsafe fn fsamp(
        &mut self,
        enc: &Enc,
        s: Option<&ProtocolObject<dyn MTLSamplerState>>,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.fsamp.push((s.map(|x| x.retain()), i));
            return;
        }
        let k = ptr_of(s);
        if i < 32 && self.fsamp[i] == k {
            return;
        }
        if i < 32 {
            self.fsamp[i] = k;
        }
        enc.push(Op::FSamp(s.map(|x| x.retain()), i));
    }
}

/// --log-draw-reuse: is a draw in a list recorded in an earlier frame planned exactly as the previous time that
/// list op ran in the same calling context (the op's position in the parent lists)? Every 300 frames: draws in old lists,
/// how many repeat their previous plan, and which plan parts differ when not.
/// --log-batchable (with --log-draw-reuse): runs of consecutive draws in one pass that differ only in texture
/// handles, uniform and storage buffers (the same mesh drawn again with other per-draw data), which an instanced draw
/// with per-instance resources could replace.
fn note_batchable(st: &State, pkt: &DrawPacket) {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    pkt.mode.hash(&mut h);
    format!("{:?}", pkt.call).hash(&mut h);
    pkt.vs
        .map(|v| v as *const CachedStage as usize)
        .hash(&mut h);
    pkt.fs
        .map(|v| v as *const CachedStage as usize)
        .hash(&mut h);
    pkt.attr_bits.hash(&mut h);
    pkt.vertex_streams.hash(&mut h);
    pkt.vertex_buffers.hash(&mut h);
    (
        pkt.blend_states,
        pkt.color_state,
        pkt.channel_mask,
        pkt.polygon,
        pkt.depth_stencil,
        pkt.stencil,
        pkt.prim_restart,
    )
        .hash(&mut h);
    (pkt.viewport, pkt.scissor).hash(&mut h);
    pkt.color_pfs.map(|p| p.0).hash(&mut h);
    (st.enc_key.as_slice(), st.enc.is_some()).hash(&mut h);
    let sig = h.finish();
    struct B {
        last: u64,
        run: u64,
        draws: u64,
        in_runs: u64,
        runs: u64,
        hist: [u64; 6],
        frame: u64,
    }
    static S: std::sync::Mutex<B> = std::sync::Mutex::new(B {
        last: 0,
        run: 0,
        draws: 0,
        in_runs: 0,
        runs: 0,
        hist: [0; 6],
        frame: 0,
    });
    let mut b = S.lock().unwrap();
    let close = |b: &mut B| {
        if b.run >= 2 {
            b.in_runs += b.run;
            b.runs += 1;
            let k = match b.run {
                2 => 0,
                3..=4 => 1,
                5..=8 => 2,
                9..=32 => 3,
                33..=128 => 4,
                _ => 5,
            };
            b.hist[k] += 1;
        }
    };
    if sig == b.last {
        b.run += 1;
    } else {
        close(&mut b);
        b.run = 1;
        b.last = sig;
    }
    b.draws += 1;
    let frame = super::presented_frames();
    if frame >= b.frame + 300 {
        let f = (frame - b.frame).max(1) as f64;
        b.frame = frame;
        println!("[batchable] f{}: {:.0} draws per frame, {:.1}% in runs of identical draws but per-draw resources; {:.0} runs per frame (2: {:.0}, 3-4: {:.0}, 5-8: {:.0}, 9-32: {:.0}, 33-128: {:.0}, more: {:.0}); draws after batching {:.0}",
            frame, b.draws as f64 / f, 100.0 * b.in_runs as f64 / b.draws.max(1) as f64, b.runs as f64 / f,
            b.hist[0] as f64 / f, b.hist[1] as f64 / f, b.hist[2] as f64 / f, b.hist[3] as f64 / f, b.hist[4] as f64 / f, b.hist[5] as f64 / f,
            (b.draws - b.in_runs + b.runs) as f64 / f);
        b.draws = 0;
        b.in_runs = 0;
        b.runs = 0;
        b.hist = [0; 6];
    }
}

fn note_draw_reuse(st: &mut State, pkt: &DrawPacket) {
    if crate::settings::get().log.log_batchable {
        note_batchable(st, pkt);
    }
    use std::hash::{Hash, Hasher};
    // --log-draw-items: cur_caller is the draw's item token (gfx_cmd note_draw_caller): keyed by item and targets,
    // how often does a draw repeat everything but its uniform bindings from the item's previous run?
    if st.cur_caller != 0 && crate::settings::get().totk.diagnostics.log_draw_items {
        use std::hash::{Hash, Hasher};
        let hh = |g: &dyn Fn(&mut rustc_hash::FxHasher)| {
            let mut h = rustc_hash::FxHasher::default();
            g(&mut h);
            h.finish()
        };
        let ctx = hh(&|h| {
            pkt.color_pfs.map(|p| p.0).hash(h);
            (
                pkt.depth_pf.0,
                pkt.target_size.0.to_bits(),
                pkt.target_size.1.to_bits(),
            )
                .hash(h);
            (pkt.viewport, pkt.scissor).hash(h);
        });
        let plan = hh(&|h| {
            pkt.mode.hash(h);
            format!("{:?}", pkt.call).hash(h);
            pkt.vs.map(|v| v as *const CachedStage as usize).hash(h);
            pkt.fs.map(|v| v as *const CachedStage as usize).hash(h);
            pkt.attr_bits.hash(h);
            pkt.vertex_streams.hash(h);
            pkt.vertex_buffers.hash(h);
            pkt.sbufs.hash(h);
            pkt.handles.hash(h);
            pkt.sb_desc.hash(h);
            (pkt.blend_states, pkt.color_state, pkt.channel_mask).hash(h);
            (
                pkt.polygon,
                pkt.depth_stencil,
                pkt.stencil,
                pkt.prim_restart,
            )
                .hash(h);
        });
        static ITEMS: std::sync::Mutex<(
            rustc_hash::FxHashMap<(u32, u64), ([u64; 8], u64, [u64; 8])>,
            [u64; 5],
            [u64; 8],
        )> = std::sync::Mutex::new((
            rustc_hash::FxHashMap::with_hasher(rustc_hash::FxBuildHasher),
            [0; 5],
            [0; 8],
        ));
        let mut g = ITEMS.lock().unwrap();
        let (m, c, parts) = &mut *g;
        let frame = super::presented_frames();
        let pv = [
            hh(&|h| {
                pkt.mode.hash(h);
                format!("{:?}", pkt.call).hash(h);
            }),
            hh(&|h| {
                pkt.vs.map(|v| v as *const CachedStage as usize).hash(h);
                pkt.fs.map(|v| v as *const CachedStage as usize).hash(h);
            }),
            hh(&|h| {
                pkt.attr_bits.hash(h);
                pkt.vertex_streams.hash(h);
            }),
            hh(&|h| pkt.vertex_buffers.hash(h)),
            hh(&|h| {
                pkt.sbufs.hash(h);
                pkt.sb_desc.hash(h);
            }),
            hh(&|h| pkt.handles.hash(h)),
            hh(&|h| (pkt.blend_states, pkt.color_state, pkt.channel_mask).hash(h)),
            hh(&|h| {
                (
                    pkt.polygon,
                    pkt.depth_stencil,
                    pkt.stencil,
                    pkt.prim_restart,
                )
                    .hash(h)
            }),
        ];
        let _ = plan;
        let key = (st.cur_caller, ctx);
        let prev = m.get(&key).copied();
        match prev {
            Some((p, f0, _)) if p == pv => {
                c[0] += 1;
                if f0 + 1 >= frame {
                    c[3] += 1;
                }
            }
            Some((p, _, pp)) => {
                c[1] += 1;
                if pp == pv {
                    c[4] += 1;
                }
                for i in 0..8 {
                    if p[i] != pv[i] {
                        parts[i] += 1;
                    }
                }
            }
            None => c[2] += 1,
        }
        m.insert(key, (pv, frame, prev.map_or([0; 8], |p| p.0)));
        // Examples of texture changes for the same draw (first 40): which binding, which textures.
        {
            static H: std::sync::Mutex<
                Option<
                    rustc_hash::FxHashMap<
                        (u32, u64),
                        smallvec::SmallVec<[(i32, i32, Option<u64>); 16]>,
                    >,
                >,
            > = std::sync::Mutex::new(None);
            static SHOWN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let mut hm = H.lock().unwrap();
            let hm = hm.get_or_insert_with(Default::default);
            if let Some(old) = hm.insert(key, pkt.handles.clone()) {
                if old != pkt.handles
                    && frame > 8000
                    && SHOWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 40
                {
                    let desc = |h: Option<u64>| -> String {
                        let Some(h) = h else { return "-".into() };
                        let id = (h & 0xFFFFF) as i32;
                        match crate::nvn::api::lookup_pool_texture(id)
                            .and_then(|(o, _)| unsafe { super::tex_info(o) })
                        {
                            Some((_, t)) => format!(
                                "{:#x}[{}x{} fmt {} storage {:#x}]",
                                h, t.width, t.height, t.format, t.storage
                            ),
                            None => format!("{:#x}[?]", h),
                        }
                    };
                    let diffs: Vec<String> = pkt
                        .handles
                        .iter()
                        .zip(old.iter())
                        .filter(|(a, b)| a != b)
                        .map(|(a, b)| format!("s{} t{}: {} -> {}", a.0, a.1, desc(b.2), desc(a.2)))
                        .collect();
                    println!(
                        "[draw-items] f{} key {:08x}: textures changed: {}",
                        frame,
                        key.0,
                        diffs.join("; ")
                    );
                }
            }
        }
        static LASTI: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if frame >= LASTI.load(std::sync::atomic::Ordering::Relaxed) + 300 {
            LASTI.store(frame, std::sync::atomic::Ordering::Relaxed);
            let n = (c[0] + c[1] + c[2]).max(1) as f64;
            const PN: [&str; 8] = [
                "call",
                "shaders",
                "attrs",
                "vertex buffers",
                "storage",
                "textures",
                "blend",
                "raster/depth",
            ];
            let pl: Vec<String> = (0..8)
                .filter(|&i| parts[i] > 0)
                .map(|i| {
                    format!(
                        "{} {:.0}%",
                        PN[i],
                        100.0 * parts[i] as f64 / c[1].max(1) as f64
                    )
                })
                .collect();
            println!("[draw-items] f{}: {:.0} item draws per frame; same plan but uniforms as the item's previous run {:.1}% (of those from the previous frame {:.1}%), changed {:.1}% (equal to the run before that {:.1}%), first {:.1}%; {} items known; changed parts: {}",
                frame, n / 300.0, 100.0 * c[0] as f64 / n, 100.0 * c[3] as f64 / c[0].max(1) as f64, 100.0 * c[1] as f64 / n, 100.0 * c[4] as f64 / n, 100.0 * c[2] as f64 / n, m.len(), pl.join(", "));
            *c = [0; 5];
            *parts = [0; 8];
            if m.len() > 1 << 20 {
                m.clear();
            }
        }
    }
    let Some((list, op, old)) = st.cur_list_op else {
        return;
    };
    const NAMES: [&str; 12] = [
        "call",
        "shaders",
        "attrs/streams",
        "vertex buffers",
        "storage buffers",
        "uniform bindings",
        "texture handles",
        "sb descriptors",
        "blend/color",
        "viewport/scissor/depth",
        "raster/depth-stencil",
        "targets",
    ];
    let mut f = [0u64; 12];
    let mut hh = |i: usize, g: &dyn Fn(&mut rustc_hash::FxHasher)| {
        let mut h = rustc_hash::FxHasher::default();
        g(&mut h);
        f[i] = h.finish();
    };
    hh(0, &|h| {
        pkt.mode.hash(h);
        format!("{:?}", pkt.call).hash(h);
    });
    hh(1, &|h| {
        pkt.vs.map(|v| v as *const CachedStage as usize).hash(h);
        pkt.fs.map(|v| v as *const CachedStage as usize).hash(h);
    });
    hh(2, &|h| {
        pkt.attr_bits.hash(h);
        pkt.vertex_streams.hash(h);
    });
    hh(3, &|h| pkt.vertex_buffers.hash(h));
    hh(4, &|h| pkt.sbufs.hash(h));
    hh(5, &|h| pkt.ubos.hash(h));
    hh(6, &|h| pkt.handles.hash(h));
    hh(7, &|h| pkt.sb_desc.hash(h));
    hh(8, &|h| {
        (
            pkt.blend_states,
            pkt.color_state,
            pkt.channel_mask,
            pkt.point_size.to_bits(),
        )
            .hash(h);
        pkt.blend_color.map(f32::to_bits).hash(h);
    });
    hh(9, &|h| {
        (
            pkt.viewport,
            pkt.scissor,
            pkt.depth_range.0.to_bits(),
            pkt.depth_range.1.to_bits(),
            pkt.depth_clamp,
        )
            .hash(h)
    });
    hh(10, &|h| {
        (
            pkt.polygon_offset.0.to_bits(),
            pkt.polygon_offset.1.to_bits(),
            pkt.polygon_offset.2.to_bits(),
            pkt.polygon,
            pkt.depth_stencil,
            pkt.stencil,
            pkt.multisample,
            pkt.prim_restart,
        )
            .hash(h)
    });
    hh(11, &|h| {
        pkt.color_pfs.map(|p| p.0).hash(h);
        (
            pkt.depth_pf.0,
            pkt.target_size.0.to_bits(),
            pkt.target_size.1.to_bits(),
        )
            .hash(h);
    });
    // Uniform bindings vs the runs 1..4 before (does the game's uniform memory rotate through a fixed set?).
    static LAGS: std::sync::Mutex<(
        rustc_hash::FxHashMap<(usize, usize, u64), [u64; 4]>,
        [u64; 6],
    )> = std::sync::Mutex::new((
        rustc_hash::FxHashMap::with_hasher(rustc_hash::FxBuildHasher),
        [0; 6],
    ));
    {
        let mut l = LAGS.lock().unwrap();
        let (hist, cnt) = &mut *l;
        let key = (list, op, st.call_ctx ^ f[11].rotate_left(7));
        // Keyed by (list, op), everything but the uniform bindings, against the last 4 different plans of that command
        // (one list runs in several passes per frame): how often would a compiled list's draw be reused?
        {
            static RECENT: std::sync::Mutex<(
                rustc_hash::FxHashMap<(usize, usize), [[u64; 12]; 16]>,
                [u64; 3],
                [u64; 12],
            )> = std::sync::Mutex::new((
                rustc_hash::FxHashMap::with_hasher(rustc_hash::FxBuildHasher),
                [0; 3],
                [0; 12],
            ));
            let mut m = RECENT.lock().unwrap();
            let (prev, c, pd) = &mut *m;
            let mut g = f;
            g[5] = 0;
            g[0] |= 1;
            let e = prev.entry((list, op)).or_insert([[0; 12]; 16]);
            if e.contains(&g) {
                c[0] += 1
            } else if e[0][0] == 0 {
                c[2] += 1
            } else {
                c[1] += 1;
                // Parts that differ from the closest of the recent plans.
                let best = e
                    .iter()
                    .filter(|x| x[0] != 0)
                    .min_by_key(|x| (0..12).filter(|&i| x[i] != g[i]).count())
                    .copied()
                    .unwrap_or([0; 12]);
                for i in 0..12 {
                    if best[i] != g[i] {
                        pd[i] += 1;
                    }
                }
            }
            if !e.contains(&g) {
                e.rotate_right(1);
                e[0] = g;
            }
            let frame = super::presented_frames();
            static LASTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            if frame >= LASTR.load(std::sync::atomic::Ordering::Relaxed) + 300 {
                LASTR.store(frame, std::sync::atomic::Ordering::Relaxed);
                let n = (c[0] + c[1] + c[2]).max(1) as f64;
                let pl: Vec<String> = (0..12)
                    .filter(|&i| pd[i] > 0)
                    .map(|i| {
                        format!(
                            "{} {:.0}%",
                            NAMES[i],
                            100.0 * pd[i] as f64 / c[1].max(1) as f64
                        )
                    })
                    .collect();
                println!("[draw-reuse] f{}: old-list draws equal (but uniforms) to one of the command's last 16 plans: {:.1}%, new plan {:.1}%, first {:.1}%; new plans differ in: {}",
                    frame, 100.0 * c[0] as f64 / n, 100.0 * c[1] as f64 / n, 100.0 * c[2] as f64 / n, pl.join(", "));
                *c = [0; 3];
                *pd = [0; 12];
            }
        }
        {
            static NOUBO: std::sync::Mutex<(
                rustc_hash::FxHashMap<(usize, usize, u64), [u64; 12]>,
                [u64; 3],
                [u64; 12],
            )> = std::sync::Mutex::new((
                rustc_hash::FxHashMap::with_hasher(rustc_hash::FxBuildHasher),
                [0; 3],
                [0; 12],
            ));
            let mut m = NOUBO.lock().unwrap();
            let (prev, c, parts) = &mut *m;
            match prev.insert((list, op, f[11] ^ f[9].rotate_left(13)), f) {
                Some(p) if (0..12).all(|i| i == 5 || p[i] == f[i]) => c[0] += 1,
                Some(p) => {
                    c[1] += 1;
                    for i in 0..12 {
                        if i != 5 && p[i] != f[i] {
                            parts[i] += 1;
                        }
                    }
                }
                None => c[2] += 1,
            }
            let frame = super::presented_frames();
            static LASTN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            if frame >= LASTN.load(std::sync::atomic::Ordering::Relaxed) + 300 {
                LASTN.store(frame, std::sync::atomic::Ordering::Relaxed);
                let n = (c[0] + c[1] + c[2]).max(1) as f64;
                let pl: Vec<String> = (0..12)
                    .filter(|&i| parts[i] > 0)
                    .map(|i| {
                        format!(
                            "{} {:.0}%",
                            NAMES[i],
                            100.0 * parts[i] as f64 / c[1].max(1) as f64
                        )
                    })
                    .collect();
                println!("[draw-reuse] f{}: keyed by (list, op, targets, viewport), all but uniform bindings: same {:.1}%, changed {:.1}%, first {:.1}%; changed parts: {}",
                    frame, 100.0 * c[0] as f64 / n, 100.0 * c[1] as f64 / n, 100.0 * c[2] as f64 / n, pl.join(", "));
                *c = [0; 3];
                *parts = [0; 12];
            }
        }
        let h = hist.entry(key).or_insert([0; 4]);
        cnt[5] += 1;
        match (0..4).find(|&k| h[k] == f[5]) {
            Some(k) => cnt[k] += 1,
            None => cnt[4] += 1,
        }
        h.rotate_right(1);
        h[0] = f[5];
        let frame = super::presented_frames();
        static LASTL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if frame >= LASTL.load(std::sync::atomic::Ordering::Relaxed) + 300 {
            LASTL.store(frame, std::sync::atomic::Ordering::Relaxed);
            let n = cnt[5].max(1) as f64;
            println!("[draw-reuse] f{}: uniform bindings equal to the run 1 / 2 / 3 / 4 before: {:.1}% / {:.1}% / {:.1}% / {:.1}%, none {:.1}%",
                frame, 100.0 * cnt[0] as f64 / n, 100.0 * cnt[1] as f64 / n, 100.0 * cnt[2] as f64 / n, 100.0 * cnt[3] as f64 / n, 100.0 * cnt[4] as f64 / n);
            *cnt = [0; 6];
            if hist.len() > 1 << 20 {
                hist.clear();
            }
        }
    }
    static STATS: std::sync::Mutex<([u64; 4], [u64; 12])> =
        std::sync::Mutex::new(([0; 4], [0; 12]));
    let mut g = STATS.lock().unwrap();
    let (s, diff) = &mut *g;
    if !old {
        s[0] += 1;
    } else {
        s[1] += 1;
        let key = (list, op, st.call_ctx ^ f[11].rotate_left(7));
        match st.draw_reuse.insert(key, f) {
            Some(p) if p == f => s[2] += 1,
            Some(p) => {
                for i in 0..12 {
                    if p[i] != f[i] {
                        diff[i] += 1;
                    }
                }
            }
            None => s[3] += 1,
        }
    }
    let frame = super::presented_frames();
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if frame >= LAST.load(std::sync::atomic::Ordering::Relaxed) + 300 {
        LAST.store(frame, std::sync::atomic::Ordering::Relaxed);
        let n = s[0] + s[1];
        let changed = s[1] - s[2] - s[3];
        let parts: Vec<String> = (0..12)
            .filter(|&i| diff[i] > 0)
            .map(|i| {
                format!(
                    "{} {:.0}%",
                    NAMES[i],
                    100.0 * diff[i] as f64 / changed.max(1) as f64
                )
            })
            .collect();
        println!("[draw-reuse] f{}: {} draws per frame; in old lists {:.1}%: same plan as the previous run in the same context {:.1}%, first run {:.1}%; changed parts: {}",
            frame, n / 300, 100.0 * s[1] as f64 / n.max(1) as f64, 100.0 * s[2] as f64 / s[1].max(1) as f64, 100.0 * s[3] as f64 / s[1].max(1) as f64, parts.join(", "));
        *s = [0; 4];
        *diff = [0; 12];
        if st.draw_reuse.len() > 1 << 20 {
            st.draw_reuse.clear();
        }
    }
}

/// --log-ubo-content (RE probe): per frame, how many uniform bindings and bytes the draws
/// read, how many distinct (address, contents), and how many of those contents the previous frame also had somewhere.
unsafe fn ubo_content_probe(pkt: &DrawPacket, vs: &'static CachedStage) {
    use std::hash::Hasher;
    struct P {
        frame: u64,
        cur: rustc_hash::FxHashSet<u64>,
        prev: rustc_hash::FxHashSet<u64>,
        addr_cur: rustc_hash::FxHashSet<(u64, u64)>,
        n: u64,
        bytes: u64,
        seen_prev: u64,
        by_slot: [[u64; 3]; 2 * 16],
        frames: u64,
        // --log-ubo-writers: 16 KiB pages of this frame's uniform bindings, ring position, writers (pc, lr) -> faults.
        pages: rustc_hash::FxHashSet<u64>,
        ring_pos: usize,
        writers: rustc_hash::FxHashMap<(u64, u64), u64>,
        // Bound uniform addresses of this frame and the four before; bindings whose address was bound k frames ago.
        addrs: [rustc_hash::FxHashSet<u64>; 5],
        addr_lag: [u64; 5],
    }
    static S: std::sync::Mutex<Option<P>> = std::sync::Mutex::new(None);
    let mut g = S.lock().unwrap();
    let p = g.get_or_insert_with(|| P {
        frame: 0,
        cur: Default::default(),
        prev: Default::default(),
        addr_cur: Default::default(),
        n: 0,
        bytes: 0,
        seen_prev: 0,
        by_slot: [[0; 3]; 32],
        frames: 0,
        pages: Default::default(),
        ring_pos: 0,
        writers: Default::default(),
        addrs: Default::default(),
        addr_lag: [0; 5],
    });
    let writers = crate::settings::get().log.log_ubo_writers;
    if writers && !crate::guest::exec::crash_handler::WRITER_LOG.load(std::sync::atomic::Ordering::Relaxed)
    {
        crate::guest::exec::crash_handler::WRITER_LOG.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let frame = super::presented_frames();
    if frame != p.frame {
        if writers {
            // Faults since the last frame change: those on pages this frame's uniforms sat on name their writers.
            let base = crate::guest::exec::crash_handler::get_guest_base() as u64;
            let end =
                crate::guest::exec::crash_handler::WRITER_IDX.load(std::sync::atomic::Ordering::Relaxed);
            let ring = &crate::guest::exec::crash_handler::WRITER_RING;
            let start = p.ring_pos.max(end.saturating_sub(ring.len()));
            for i in start..end {
                let e = &ring[i % ring.len()];
                let (a, pc, lr) = (
                    e.0.load(std::sync::atomic::Ordering::Relaxed),
                    e.1.load(std::sync::atomic::Ordering::Relaxed),
                    e.2.load(std::sync::atomic::Ordering::Relaxed),
                );
                if a != 0 {
                    let rel = |x: u64| {
                        if x >= base && x < base + (256 << 20) {
                            x - base
                        } else {
                            0
                        }
                    };
                    *p.writers.entry((rel(pc), rel(lr))).or_insert(0) += 1;
                }
            }
            p.ring_pos = end;
            // Arm the pages of the frame just planned: the game's next writes to them are logged.
            for &pg in &p.pages {
                let k = pg as usize;
                if k < crate::guest::exec::crash_handler::WRITER_PAGES.len() * 64 {
                    crate::guest::exec::crash_handler::WRITER_ARMED[k / 64]
                        .fetch_or(1 << (k % 64), std::sync::atomic::Ordering::Relaxed);
                    crate::guest::exec::crash_handler::WRITER_PAGES[k / 64]
                        .fetch_or(1 << (k % 64), std::sync::atomic::Ordering::Relaxed);
                    libc::mprotect((pg << 14) as *mut libc::c_void, 1 << 14, libc::PROT_READ);
                }
            }
            p.pages.clear();
        }
        p.prev = std::mem::take(&mut p.cur);
        p.addrs.rotate_right(1);
        p.addrs[0].clear();
        p.addr_cur.clear();
        p.frame = frame;
        p.frames += 1;
        if p.frames % 300 == 0 {
            let f = 300.0;
            let slots: Vec<String> = (0..32)
                .filter(|&k| p.by_slot[k][0] > 0)
                .map(|k| {
                    format!(
                        "{}c{}: {:.0} binds {:.0}% prev",
                        if k < 16 { "v" } else { "f" },
                        k % 16 + 3,
                        p.by_slot[k][0] as f64 / f,
                        100.0 * p.by_slot[k][1] as f64 / p.by_slot[k][0] as f64
                    )
                })
                .collect();
            println!("[ubo-content] f{}: bound address also bound 1 / 2 / 3 / 4 frames before: {:.1}% / {:.1}% / {:.1}% / {:.1}%, in none {:.1}%",
                frame, 100.0 * p.addr_lag[1] as f64 / p.n.max(1) as f64, 100.0 * p.addr_lag[2] as f64 / p.n.max(1) as f64,
                100.0 * p.addr_lag[3] as f64 / p.n.max(1) as f64, 100.0 * p.addr_lag[4] as f64 / p.n.max(1) as f64, 100.0 * p.addr_lag[0] as f64 / p.n.max(1) as f64);
            p.addr_lag = [0; 5];
            println!("[ubo-content] f{}: per frame {:.0} uniform bindings, {:.0} KB read, {:.0} distinct (address, contents); contents also in the previous frame {:.1}%; by slot: {}",
                frame, p.n as f64 / f, p.bytes as f64 / f / 1024.0, p.addr_cur.len(), 100.0 * p.seen_prev as f64 / p.n.max(1) as f64, slots.join(", "));
            if writers {
                let mut w: Vec<_> = p.writers.iter().map(|(k, v)| (*v, *k)).collect();
                w.sort_unstable_by(|a, b| b.cmp(a));
                let total: u64 = w.iter().map(|x| x.0).sum();
                println!("[ubo-writers] f{}: {:.0} first writes per frame to uniform pages; by (pc, lr):", frame, total as f64 / f);
                for (n, (pc, lr)) in w.iter().take(25) {
                    println!(
                        "[ubo-writers]   {:7.1} per frame  pc main+{:#x}  lr main+{:#x}",
                        *n as f64 / f,
                        pc,
                        lr
                    );
                }
                p.writers.clear();
            }
            p.n = 0;
            p.bytes = 0;
            p.seen_prev = 0;
            p.by_slot = [[0; 3]; 32];
        }
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
            let Some((addr, size)) = pkt.ubo(stage, slot as i32 - 3) else {
                continue;
            };
            if addr <= 0x10000 || size == 0 {
                continue;
            }
            let n = size
                .min(cached.cbuf_read_len(binding, CBUF_DECL_SIZE))
                .max(size.min(16));
            // Literal reads past the bound size (Maxwell returns 0 there; a copy's slack holds other data).
            {
                static OVER: [std::sync::atomic::AtomicU64; 3] =
                    [const { std::sync::atomic::AtomicU64::new(0) }; 3];
                let ext = cached.cbuf_read_len(binding, CBUF_DECL_SIZE);
                OVER[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Known literal extents only (stages with computed reads report the declaration size; those reads are
                // clamped to the bound size by msl_specialize).
                if ext > size && ext < CBUF_DECL_SIZE {
                    OVER[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    OVER[2].fetch_add((ext - size) as u64, std::sync::atomic::Ordering::Relaxed);
                    static SHOWN: std::sync::atomic::AtomicU32 =
                        std::sync::atomic::AtomicU32::new(0);
                    if SHOWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 12 {
                        println!("[ubo-content] {} reads cbuf slot {} (mtl {}) to byte {} but {} bytes are bound at {:#x}",
                            cached.entry, slot, binding, ext, size, addr);
                    }
                }
                if p.frames % 300 == 0 && p.n == 0 {
                    let t = OVER[0].swap(0, std::sync::atomic::Ordering::Relaxed).max(1);
                    let o = OVER[1].swap(0, std::sync::atomic::Ordering::Relaxed);
                    let b = OVER[2].swap(0, std::sync::atomic::Ordering::Relaxed);
                    println!("[ubo-content] uniform bindings whose literal reads go past the bound size: {:.2}% ({} of {}, {:.0} bytes past on average)", 100.0 * o as f64 / t as f64, o, t, b as f64 / o.max(1) as f64);
                }
            }
            let bytes = std::slice::from_raw_parts(addr as *const u8, n);
            let mut h = rustc_hash::FxHasher::default();
            h.write(bytes);
            let hv = h.finish() ^ n as u64;
            p.n += 1;
            p.bytes += n as u64;
            p.addr_cur.insert((addr as u64, hv));
            match (1..5).find(|&k| p.addrs[k].contains(&(addr as u64))) {
                Some(k) => p.addr_lag[k] += 1,
                None => p.addr_lag[0] += 1,
            }
            p.addrs[0].insert(addr as u64);
            if writers {
                for pg in (addr as u64 >> 14)..=((addr as u64 + n as u64 - 1) >> 14) {
                    p.pages.insert(pg);
                }
            }
            let prev = p.prev.contains(&hv);
            if prev {
                p.seen_prev += 1;
            }
            p.cur.insert(hv);
            let k = si * 16 + (slot as usize - 3).min(15);
            p.by_slot[k][0] += 1;
            if prev {
                p.by_slot[k][1] += 1;
            }
        }
    }
}

/// Zero bytes a uniform copy needs after the bound range: Maxwell returns 0 for constant-buffer reads past the bound size,
/// and stages whose literal reads (`extent`, from the shader cache's `cx` lines) go past it would otherwise read whatever
/// follows the copy in the arena (or past a zero-copy buffer's end). Unknown extents (dynamic indexing: the declaration
/// size) need none; msl_specialize clamps those reads to the bound size.
fn cbuf_zero_pad(extent: usize, size: usize) -> usize {
    if extent > size && extent < CBUF_DECL_SIZE {
        extent - size
    } else {
        0
    }
}

thread_local! {
    /// Why try_tiny_compute declined the last draw (frame traces).
    static TINY_REASON: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn tiny_reason(r: String) {
    if super::tracing_frame() {
        TINY_REASON.with(|x| *x.borrow_mut() = r);
    }
}
