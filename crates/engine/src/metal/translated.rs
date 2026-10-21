// Draw path that runs the game's own shaders (translated ahead of time, see nxport-cli shaders build and
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
//   - clip space          the vertex stage negates y and remaps z (crates/cli/src/msl_specialize.rs); the winding flips
//                         accordingly
//
// Guest memory is wrapped, not copied: each NVN memory pool becomes one no-copy MTLBuffer (see `pool_buffer`).
// A draw it cannot express returns false and is skipped.

use super::*;

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
/// (must match EXTRA_SAMPLER_BASE / EXTRA_SAMPLER_BUFFER in crates/cli/src/msl_specialize.rs).
const EXTRA_SAMPLER_BASE: usize = 16;
const EXTRA_SAMPLER_BUFFER: usize = 29;
const MAX_STREAMS: usize = 11;
/// Buffer index of the (0,0,0,1) default-attribute buffer (Metal vertex buffer indices must be < 31; streams use 20..30).
const DEFAULT_ATTR_BUFFER: usize = 19;
/// Byte offset in the driver cbuf (guest cbuf 0, Metal binding 17) where each stage's constant buffer sizes go, one u32
/// (float4 count) per Metal binding 0..17 (must match CBUF_LIMITS_VEC4 in crates/cli/src/msl_specialize.rs). The game never
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
#[derive(Default)]
struct PipeQueue {
    draws: std::collections::VecDeque<SendJob>,
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
    )
}

fn push_job(job: SendJob, listed: bool) {
    let (q, cv) = pipe_queue();
    let mut g = q.lock().unwrap();
    if listed {
        g.listed.push_back(job);
        g.listed_n += 1;
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
    }
}

/// Queues a pipeline-list entry for compilation: false when its shaders are not in the cache, it was recorded under
/// another window origin or depth mode, or its pipeline is already known.
pub(super) fn queue_precompile(e: &super::pipeline_list::Entry) -> bool {
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
    let color_pfs = e.color_pfs.map(|f| MTLPixelFormat(f as _));
    let depth_pf = MTLPixelFormat(e.depth_pf as _);
    let point_size = f32::from_bits(e.point_size);
    let l = derive_layout(vs, &e.attr_bits, &e.streams);
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

/// What a stage's driver cbuf (guest cbuf 0) holds: EncodeCtx::driver_cb_memo.
#[derive(Clone, PartialEq)]
pub(crate) struct DriverCbKey {
    base_vertex: i32,
    base_instance: i32,
    sb_descs: Vec<(usize, u64, u64)>,
    limits: [u32; 18],
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

/// Metal texture type a shader binding declares (manifest type strings from tools/sass2spv).

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
) {
    super::pipeline_list::record(super::pipeline_list::Entry {
        vs: vs.entry.clone(),
        fs: fs.map(|f| f.entry.clone()),
        point_size: point_size.to_bits(),
        depth_gl,
        y_negate: y_negate(),
        color_state: p.color_state,
        channel_mask: p.channel_mask,
        depth_pf: p.depth_pf.0 as u64,
        color_pfs: p.color_pfs.map(|f| f.0 as u64),
        blends: *blends,
        attr_bits: p.attr_bits.to_vec(),
        streams: p.vertex_streams.to_vec(),
    });
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
    /// Run as compute instead of in a render pass (decided by Gfx::tiny_plan).
    pub(super) tiny: Option<TinyPlan>,
    /// The draw came from a command list recorded in an earlier frame (--log-phases).
    pub(super) from_old_list: bool,
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
            tiny: None,
            from_old_list: false,
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
    ) {
        self.mode = mode;
        self.prim = prim;
        self.call = call;
        self.from_old_list = st.in_old_list;
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
            let v = std::slice::from_raw_parts(addr as *const u8, n);
            let r = restart.filter(|&r| r <= 0xFF).map(|r| r as u8);
            let m = lanes::<u8, 32>(v, r);
            (
                m as u64,
                m == 0 && r.is_some() && v.iter().all(|&x| Some(x) == r),
            )
        }
        1 => {
            let v = std::slice::from_raw_parts(addr as *const u16, n);
            let r = restart.filter(|&r| r <= 0xFFFF).map(|r| r as u16);
            let m = lanes::<u16, 16>(v, r);
            (
                m as u64,
                m == 0 && r.is_some() && v.iter().all(|&x| Some(x) == r),
            )
        }
        _ => {
            let v = std::slice::from_raw_parts(addr as *const u32, n);
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

/// Whether `v` contains `x`, four indices per 64-bit word (a lane equals x iff it is zero after XOR; the classic
/// has-zero-lane test, which has no false positives). ~4x the element-by-element scan.
fn contains_u16(v: &[u16], x: u16) -> bool {
    const LO: u64 = 0x0001_0001_0001_0001;
    const HI: u64 = 0x8000_8000_8000_8000;
    let pat = x as u64 * LO;
    let (head, words, tail) = unsafe { v.align_to::<u64>() };
    if head.contains(&x) || tail.contains(&x) {
        return true;
    }
    for c in words.chunks(4) {
        let mut hit = 0u64;
        for &w in c {
            let y = w ^ pat;
            hit |= y.wrapping_sub(LO) & !y & HI;
        }
        if hit != 0 {
            return true;
        }
    }
    false
}

/// NVN primitive restart for an indexed draw whose indices contain the restart value (None: draw as is). Metal restarts
/// only strips, only on 0xFFFF / 0xFFFFFFFF, and compares after adding the base vertex: TotK's strips use restart index
/// 0xFFFF with base vertices in the thousands, so the restart became a real (garbage) vertex and triangles streaked
/// across the screen whenever such a mesh was in view. The rewrite produces 32-bit indices with the base vertex added:
/// strips keep a 0xFFFFFFFF restart (Metal's own), lists drop the unfinished primitive at each restart (NVN/GL rules).
unsafe fn restart_rewrite(
    mode: i32,
    index_type: i32,
    addr: u64,
    count: i32,
    restart: u32,
    base_vertex: i32,
) -> Option<Vec<u32>> {
    if addr < 0x10000 || count <= 0 {
        return None;
    }
    let n = count as usize;
    // Checked before anything is allocated: most indexed draws contain no restart index.
    let values: Vec<u32> = match index_type {
        0 => {
            let v = std::slice::from_raw_parts(addr as *const u8, n);
            if restart > 0xFF || !v.contains(&(restart as u8)) {
                return None;
            }
            v.iter().map(|&x| x as u32).collect()
        }
        1 => {
            let v = std::slice::from_raw_parts(addr as *const u16, n);
            if restart > 0xFFFF || !contains_u16(v, restart as u16) {
                return None;
            }
            v.iter().map(|&x| x as u32).collect()
        }
        2 => {
            let v = std::slice::from_raw_parts(addr as *const u32, n);
            if !v.contains(&restart) {
                return None;
            }
            v.to_vec()
        }
        _ => return None,
    };
    let rebase = |v: u32| (v as i64 + base_vertex as i64).clamp(0, u32::MAX as i64 - 1) as u32;
    let per_prim = match mode {
        0 => 1,     // points
        1 => 2,     // lines
        4 => 3,     // triangles
        3 | 5 => 0, // line strip, triangle strip: Metal restarts these itself
        _ => return None,
    };
    let mut out = Vec::with_capacity(n);
    if per_prim == 0 {
        out.extend(
            values
                .iter()
                .map(|&v| if v == restart { u32::MAX } else { rebase(v) }),
        );
    } else {
        let mut cur = Vec::with_capacity(3);
        for &v in &values {
            if v == restart {
                cur.clear();
                continue;
            }
            cur.push(rebase(v));
            if cur.len() == per_prim {
                out.extend_from_slice(&cur);
                cur.clear();
            }
        }
    }
    Some(out)
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

impl Gfx {
    /// A Metal buffer + offset covering `[addr, addr + len)` of guest memory without copying it.
    pub(super) unsafe fn pool_buffer(
        &self,
        sh: &mut SharedCaches,
        addr: u64,
        len: usize,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        let addr = crate::nvn::api::translate_virtual(addr as usize);
        // Per-thread memo by exact (address, length), valid while horizon::hle::pools_gen() is unchanged (pool registrations,
        // virtual mappings, releases and unmaps bump it, and the slow path below handles releases first): the same
        // static vertex / index ranges are looked up every frame, and each lookup took the release queue, the global
        // wrapper table and the pool registry. --no-pool-buffer-memo: off.
        thread_local! {
            static MEMO: std::cell::RefCell<(u64, HashMap<(usize, usize), (Retained<ProtocolObject<dyn MTLBuffer>>, usize)>)> =
                std::cell::RefCell::new((0, HashMap::default()));
        }
        let gen = crate::nvn::api::pools_gen();
        {
            let hit = MEMO.with(|m| {
                let m = m.borrow();
                if m.0 == gen {
                    m.1.get(&(addr, len)).cloned()
                } else {
                    None
                }
            });
            if hit.is_some() {
                return hit;
            }
        }
        let r = self.pool_buffer_slow(sh, addr, len);
        {
            if let Some(x) = &r {
                MEMO.with(|m| {
                    let mut m = m.borrow_mut();
                    if m.0 != gen || m.1.len() > 1 << 16 {
                        m.1.clear();
                        m.0 = gen;
                    }
                    m.1.insert((addr, len), x.clone());
                });
            }
        }
        r
    }

    /// pool_buffer without the memo (`addr` already translated).
    unsafe fn pool_buffer_slow(
        &self,
        sh: &mut SharedCaches,
        addr: usize,
        len: usize,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        // One wrapper per pool for every thread: Metal tracks hazards per buffer object, so a draw reading memory through
        // another wrapper than the compute pass that wrote it would not wait for it; and take_released_ranges hands each
        // release to one caller only.
        let _ = sh;
        let released = crate::nvn::api::take_released_ranges();
        drop_released_pool_buffers(&released);
        let mut pool_buffers = POOL_BUFFERS.lock().unwrap();
        let pool_buffers = &mut pool_buffers
            .get_or_insert_with(|| PoolBuffers(HashMap::default()))
            .0;
        let (mut base, mut size) = crate::nvn::api::find_memory_pool(addr)?;
        if addr + len > base + size {
            // A pool nested inside a larger one covers the start only.
            (base, size) = crate::nvn::api::containing_pool(addr, len)?;
        }
        let wrap_base = base & !(VM_PAGE - 1);
        let wrap_end = (base + size + VM_PAGE - 1) & !(VM_PAGE - 1);
        // Keyed by the exact wrapped range: pools come and go, and several small pools can share a page, so a buffer
        // made for an earlier pool at the same page is not valid for this one.
        if let Some(b) = pool_buffers.get(&(wrap_base, wrap_end)) {
            let off = addr - wrap_base;
            if off + len <= b.length() {
                return Some((b.clone(), off));
            }
        }
        // Metal refuses a no-copy buffer over inaccessible pages; buffers made before a page is protected keep working.
        // --tex-backing holds pages under copied textures, which share pools with buffers: open them first.
        unsafe { super::tex_backing::open_overlapping(wrap_base, wrap_end - wrap_base) };
        let Some(buf) = self
            .device
            .newBufferWithBytesNoCopy_length_options_deallocator(
                std::ptr::NonNull::new(wrap_base as *mut libc::c_void)?,
                wrap_end - wrap_base,
                MTLResourceOptions::empty(),
                None,
            )
        else {
            static WARNED: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
            let mut w = WARNED.lock().unwrap_or_else(|e| e.into_inner());
            if !w.contains(&wrap_base) {
                w.push(wrap_base);
                println!("[!] Gfx: Metal refused a zero-copy buffer over pool 0x{:x}..0x{:x} ({} MiB; device max buffer {} MiB)",
                    wrap_base, wrap_end, (wrap_end - wrap_base) >> 20, self.device.maxBufferLength() >> 20);
            }
            return None;
        };
        // Pools whose page-rounded ranges overlap (several small pools on one page) each keep their own buffer: Metal
        // allows no-copy buffers over the same memory. Evicting overlapping buffers here made two such pools re-wrap each
        // other on every bind (millions of buffer creations per session). Buffers over freed memory are dropped through
        // take_released_ranges above.
        pool_buffers.insert((wrap_base, wrap_end), buf.clone());
        if pool_buffers.len() % 256 == 1 {
            println!(
                "[+] Gfx: wrapped memory pool 0x{:x}..0x{:x} as a zero-copy buffer ({} wrapped)",
                wrap_base,
                wrap_end,
                pool_buffers.len()
            );
        }
        let off = addr - wrap_base;
        if off + len > buf.length() {
            return None;
        }
        Some((buf, off))
    }

    /// Buffer for guest memory: zero-copy from a pool when possible, else a copy in the arena.
    pub(super) unsafe fn guest_buffer(
        &self,
        sh: &mut SharedCaches,
        cx: &mut EncodeCtx,
        addr: u64,
        len: usize,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        // The GPU may write this memory and its writes cannot fault: stop write-tracking it.
        let host = crate::nvn::api::translate_virtual(addr as usize);
        crate::guest::dirty_track::release_for_gpu(host, len);
        super::fence_guard::note_write(host, len);
        self.guest_buffer_rw(sh, cx, addr, len, true)
    }

    /// guest_buffer for memory the GPU only reads (vertex, index and uniform buffers, copy sources): its pages may stay
    /// write-tracked. (Releasing them on every bind walked every page of every vertex buffer per draw.)
    pub(super) unsafe fn guest_buffer_ro(
        &self,
        sh: &mut SharedCaches,
        cx: &mut EncodeCtx,
        addr: u64,
        len: usize,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        self.guest_buffer_rw(sh, cx, addr, len, false)
    }

    unsafe fn guest_buffer_rw(
        &self,
        sh: &mut SharedCaches,
        cx: &mut EncodeCtx,
        addr: u64,
        len: usize,
        gpu_writes: bool,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        let addr = crate::nvn::api::translate_virtual(addr as usize) as u64;
        {
            if let Some(r) = self.pool_buffer(sh, addr, len) {
                // The GPU reads guest memory itself: the early-fence guard keeps the game from overwriting it meanwhile.
                if !gpu_writes {
                    super::fence_guard::note_read(addr as usize, len);
                }
                return Some(r);
            }
        }
        let limit = ARENA_CHUNK / 2;
        if addr < 0x10000 || len > limit {
            if addr >= 0x10000 {
                println!("[!] Gfx: {} bytes at 0x{:x} are outside every memory pool and too large to copy", len, addr);
            }
            return None;
        }
        cx.arena.alloc(
            &self.device,
            std::slice::from_raw_parts(addr as *const u8, len),
        )
    }

    /// A zeroed 64KB buffer bound for storage buffers that cannot be resolved, so shaders never read through a null
    /// buffer binding (which faults the GPU).
    fn zero_buffer(
        &self,
        sh: &mut SharedCaches,
    ) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        if let Some(b) = &sh.zero_buffer {
            return Some(b.clone());
        }
        let b = self
            .device
            .newBufferWithLength_options(0x10000, MTLResourceOptions::empty())?;
        sh.zero_buffer = Some(b.clone());
        Some(b)
    }

    /// cbuf 1: the stage's compiler-generated constants. Their offset (from the start of the stage data) and size
    /// are in the GLSLC control section at +0x700 / +0x6fc (confirmed on every captured TotK stage); without a
    /// control section, fall back to the end of the decoded code aligned to 0x100, which is wrong whenever the
    /// decoder stops before the program's trailing instructions.
    unsafe fn const_bank(
        &self,
        sh: &mut SharedCaches,
        stage_data: u64,
        control: u64,
        stage: &CachedStage,
    ) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        let key = (stage_data, stage as *const CachedStage as usize); // no String clone per draw
        if let Some(b) = sh.const_banks.get(&key) {
            return Some(b.clone());
        }
        let (off, size) = match control_const_bank(control) {
            Some(x) => x,
            None => ((stage.code_end + 0xFF) & !0xFF, CONST_BANK_SIZE),
        };
        let bytes = std::slice::from_raw_parts(
            (stage_data as usize + off) as *const u8,
            size.min(CBUF_DECL_SIZE),
        );
        let mut padded = bytes.to_vec();
        padded.resize(CBUF_DECL_SIZE, 0);
        let buf = self.device.newBufferWithBytes_length_options(
            std::ptr::NonNull::new(padded.as_mut_ptr() as *mut libc::c_void)?,
            padded.len(),
            MTLResourceOptions::empty(),
        )?;
        sh.const_banks.insert(key, buf.clone());
        Some(buf)
    }

    fn sampler_state(
        &self,
        sh: &mut SharedCaches,
        id: i32,
    ) -> Option<Retained<ProtocolObject<dyn MTLSamplerState>>> {
        // Per-thread memo by sampler id while no sampler was registered since (horizon::hle::samplers_gen): the pool lookup took
        // a global lock per texture binding. --no-sampler-memo / --replay-ab-sampler (side B): off.
        thread_local! {
            static MEMO: std::cell::RefCell<(u64, HashMap<i32, Retained<ProtocolObject<dyn MTLSamplerState>>>)> =
                std::cell::RefCell::new((0, HashMap::default()));
        }
        let gen = crate::nvn::api::samplers_gen();
        {
            if let Some(s) = MEMO.with(|m| {
                let m = m.borrow();
                if m.0 == gen {
                    m.1.get(&id).cloned()
                } else {
                    None
                }
            }) {
                return Some(s);
            }
        }
        let r = self.sampler_state_slow(sh, id);
        if let Some(x) = &r {
            MEMO.with(|m| {
                let mut m = m.borrow_mut();
                if m.0 != gen || m.1.len() > 1 << 14 {
                    m.1.clear();
                    m.0 = gen;
                }
                m.1.insert(id, x.clone());
            });
        }
        r
    }

    fn sampler_state_slow(
        &self,
        sh: &mut SharedCaches,
        id: i32,
    ) -> Option<Retained<ProtocolObject<dyn MTLSamplerState>>> {
        let desc = crate::nvn::api::lookup_pool_sampler(id)?;
        if let Some((d, s)) = sh.samplers.get(&id) {
            if *d == desc {
                return Some(s.clone());
            }
        }
        let sd = MTLSamplerDescriptor::new();
        let (min_linear, mip) = match desc.min_filter {
            0 => (false, MTLSamplerMipFilter::NotMipmapped),
            1 => (true, MTLSamplerMipFilter::NotMipmapped),
            2 => (false, MTLSamplerMipFilter::Nearest),
            3 => (true, MTLSamplerMipFilter::Nearest),
            4 => (false, MTLSamplerMipFilter::Linear),
            _ => (true, MTLSamplerMipFilter::Linear),
        };
        let mm = |linear: bool| {
            if linear {
                MTLSamplerMinMagFilter::Linear
            } else {
                MTLSamplerMinMagFilter::Nearest
            }
        };
        sd.setMinFilter(mm(min_linear));
        sd.setMagFilter(mm(desc.mag_filter != 0));
        sd.setMipFilter(mip);
        sd.setSAddressMode(address_mode(desc.wrap_s));
        sd.setTAddressMode(address_mode(desc.wrap_t));
        sd.setRAddressMode(address_mode(desc.wrap_r));
        sd.setLodMinClamp(desc.lod_min);
        // --debug-max-lod=N (diagnostic): clamp every sampler's maximum LOD (0 = base level only), to test mip contents.
        let max_lod = crate::settings::get()
            .debug
            .debug_max_lod
            .map_or(desc.lod_max, |m| desc.lod_max.min(m));
        sd.setLodMaxClamp(max_lod);
        sd.setMaxAnisotropy((desc.max_anisotropy.round() as usize).clamp(1, 16));
        if desc.compare_mode != 0 {
            sd.setCompareFunction(compare_func(desc.compare_func as u32));
        }
        let [r, g, b, a] = desc.border_color;
        sd.setBorderColor(if a < 0.5 {
            MTLSamplerBorderColor::TransparentBlack
        } else if r + g + b > 1.5 {
            MTLSamplerBorderColor::OpaqueWhite
        } else {
            MTLSamplerBorderColor::OpaqueBlack
        });
        sd.setSupportArgumentBuffers(true); // samplers 16+ are passed by handle (see bind_extra_samplers)
        let s = self.device.newSamplerStateWithDescriptor(&sd)?;
        sh.samplers.insert(id, (desc, s.clone()));
        Some(s)
    }

    fn depth_stencil_state(
        &self,
        sh: &mut SharedCaches,
        stencil: [[u8; 2]; 3],
        bits: u64,
        has_stencil: bool,
    ) -> Option<Retained<ProtocolObject<dyn MTLDepthStencilState>>> {
        // The stencil test only exists with a stencil attachment; its masks are dynamic state in NVN but part of the
        // state object in Metal, so they join the cache key.
        let stencil_on = has_stencil && (bits >> 6) & 1 != 0;
        let key: u128 = if stencil_on {
            bits as u128
                | 1 << 127
                | (u32::from_le_bytes([stencil[1][0], stencil[1][1], stencil[2][0], stencil[2][1]])
                    as u128)
                    << 64
        } else {
            (bits & !(1 << 6)) as u128
        };
        if let Some(d) = sh.ds_states.get(&key) {
            return Some(d.clone());
        }
        let test = bits & 1 != 0;
        let write = (bits >> 1) & 1 != 0;
        let func = ((bits >> 2) & 0xF) as u32;
        let desc = MTLDepthStencilDescriptor::new();
        // GL semantics (NVN follows them): with the depth test off the depth buffer is neither tested nor written.
        let cmp = compare_func(func);
        desc.setDepthCompareFunction(if test {
            cmp
        } else {
            MTLCompareFunction::Always
        });
        desc.setDepthWriteEnabled(test && write);
        if stencil_on {
            // NVNdepthStencilState bits (nvn/state.rs): front func 7..11, fail 11..15, depth fail 15..19, pass 19..23;
            // back at 23.. The stencil funcs share the depth compare enum (NEVER 1 .. ALWAYS 8).
            let op = |v: u64| match v {
                2 => objc2_metal::MTLStencilOperation::Zero,
                3 => objc2_metal::MTLStencilOperation::Replace,
                4 => objc2_metal::MTLStencilOperation::IncrementClamp,
                5 => objc2_metal::MTLStencilOperation::DecrementClamp,
                6 => objc2_metal::MTLStencilOperation::Invert,
                7 => objc2_metal::MTLStencilOperation::IncrementWrap,
                8 => objc2_metal::MTLStencilOperation::DecrementWrap,
                _ => objc2_metal::MTLStencilOperation::Keep,
            };
            for (face, base) in [(0usize, 7u32), (1, 23)] {
                let sd = objc2_metal::MTLStencilDescriptor::new();
                sd.setStencilCompareFunction(compare_func(((bits >> base) & 0xF) as u32));
                sd.setStencilFailureOperation(op((bits >> (base + 4)) & 0xF));
                sd.setDepthFailureOperation(op((bits >> (base + 8)) & 0xF));
                sd.setDepthStencilPassOperation(op((bits >> (base + 12)) & 0xF));
                sd.setReadMask(stencil[1][face] as u32);
                sd.setWriteMask(stencil[2][face] as u32);
                if face == 0 {
                    desc.setFrontFaceStencil(Some(&sd));
                } else {
                    desc.setBackFaceStencil(Some(&sd));
                }
            }
        }
        let d = self.device.newDepthStencilStateWithDescriptor(&desc)?;
        sh.ds_states.insert(key, d.clone());
        Some(d)
    }

    /// A 1x1 texture of `ty` to bind when the game's texture can't be expressed (yet).
    unsafe fn dummy_texture(&self, tc: &mut TexCache, ty: &str) -> Option<Tex> {
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
    unsafe fn bind_memo_frame_check(
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

    unsafe fn bound_texture(
        &self,
        st: &mut State,
        stage: i32,
        tb: &TexBinding,
    ) -> (Option<Tex>, Option<i32>) {
        let (t, s, _) = self.bound_texture_root(st, stage, tb);
        (t, s)
    }

    /// bound_texture plus the texture's root id (from the binding memo when it hits).
    unsafe fn bound_texture_root(
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
    unsafe fn bound_texture_handle(
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
                texture_for_binding(&mut tc.view_cache, &t, view.as_ref(), want, tex_swizzle)
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

    /// Runs one draw with the translated shaders. Returns false when it can't (caller falls back).
    /// nvnCommandBufferDispatchCompute: runs the bound compute program's translated kernel.
    ///
    /// The NVN driver cbuf (c0) of a compute stage holds {u64 address, u32 size} descriptors of the bound uniform
    /// buffers at 0x210 + 0x10 * index and of the storage buffers at 0x310 + 0x10 * index (inferred from TotK's
    /// kernels against the game's BindUniformBuffer / BindStorageBuffer calls: Maxwell compute has only 8 hardware
    /// cbufs, so uniform buffers 5+ are read through these pointers). Uniform buffer N is also cbuf N + 3.
    pub(super) unsafe fn dispatch_compute(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let shm = st.sh.clone();
        const STAGE_COMPUTE: i32 = 5;
        let Some(lib) = &self.shader_lib else { return };
        let Some(program) = st.compute_program else {
            return;
        };
        let Some(stages) = super::programs::stages(program) else {
            return;
        };
        let Some(code) = stages
            .iter()
            .find(|s| s.cached.map_or(false, |c| c.kind == StageKind::Compute))
            .copied()
        else {
            warn_once(
                format!("compute-missing-{:x}", program),
                format!(
                    "compute program 0x{:x} has no translated kernel: dispatch skipped",
                    program
                ),
            );
            return;
        };
        let cached = code.cached.unwrap();
        super::journal(|| format!("dispatch {} {}x{}x{}", cached.entry, x, y, z));
        if x <= 0 || y <= 0 || z <= 0 {
            return;
        }
        if denylist().lock().unwrap().contains(&cached.entry) {
            return;
        }
        let pipe = match st.compute_pipelines.get(&cached.entry) {
            Some(p) => p.clone(),
            None => {
                let f = match lib.function(&cached.entry, &MTLFunctionConstantValues::new()) {
                    Ok(f) => f,
                    Err(e) => {
                        warn_once(
                            format!("kfn-{}", cached.entry),
                            format!("compute function {}: {}", cached.entry, e),
                        );
                        return;
                    }
                };
                match self.device.newComputePipelineStateWithFunction_error(&f) {
                    Ok(p) => {
                        st.compute_pipelines.insert(cached.entry.clone(), p.clone());
                        p
                    }
                    Err(e) => {
                        warn_once(
                            format!("kpipe-{}", cached.entry),
                            format!(
                                "compute pipeline {}: {}",
                                cached.entry,
                                e.localizedDescription()
                            ),
                        );
                        return;
                    }
                }
            }
        };
        let [lx, ly, lz] = cached.local_size;
        if (lx * ly * lz) as usize > pipe.maxTotalThreadsPerThreadgroup() {
            warn_once(
                format!("klocal-{}", cached.entry),
                format!(
                    "compute {} workgroup {}x{}x{} exceeds the device limit",
                    cached.entry, lx, ly, lz
                ),
            );
            return;
        }

        // Driver cbuf contents.
        let mut c0 = vec![0u8; 0x1000];
        for i in 0..18 {
            if let Some(&(a, sz)) = st.uniform_buffers.get(&(STAGE_COMPUTE, i)) {
                let o = 0x210 + 0x10 * i as usize;
                c0[o..o + 8].copy_from_slice(&a.to_le_bytes());
                c0[o + 8..o + 12].copy_from_slice(&(sz as u32).to_le_bytes());
            }
        }
        for i in 0..16 {
            if let Some((a, sz)) = super::storage_buffer(st, STAGE_COMPUTE, i) {
                let o = 0x310 + 0x10 * i as usize;
                c0[o..o + 8].copy_from_slice(&a.to_le_bytes());
                c0[o + 8..o + 12].copy_from_slice(&(sz as u32).to_le_bytes());
            }
        }
        let mut notes: Vec<String> = Vec::new();
        // Per-draw detail text is only for traces and GPU-error reports (first 16 draws of an encoder); formatting it for
        // every draw cost ~20% of the executor.
        let want_notes = super::tracing_frame()
            || (draw_details() && st.lb.enc_details.last().map_or(false, |d| d.len() < 16));
        // Encoded on the issuer thread like render passes (no sync: a sync per dispatch cost ~2% of the thread).
        let _ = cb;
        let Some(cur_cb) = st.cur_cb.clone() else {
            return;
        };
        // Consecutive dispatches (and tiny compute draws) share one compute encoder: serial dispatch keeps their order
        // and makes each see the previous one's writes; any other op ends it (issuer CJoin / CLinger).
        st.issuer.push(super::issuer::Op::CJoin(cur_cb, cached.entry.clone()));
        self.note_encoder(
            st,
            format!(
                "compute {} [{}x{}x{} groups of {}x{}x{}]",
                cached.entry, x, y, z, lx, ly, lz
            ),
        );
        st.issuer.push(super::issuer::Op::CPipeline(pipe.clone()));

        // Ryujinx support buffer at 0: render scales 1.0 for texture size queries.
        let mut sup = vec![0u8; SUPPORT_BUFFER_SIZE];
        for i in 0..73 {
            sup[192 + i * 16..196 + i * 16].copy_from_slice(&1.0f32.to_le_bytes());
        }
        if let Some((buf, off)) = st.cx.arena.alloc(&self.device, &sup) {
            st.issuer.push(super::issuer::Op::CBuf(buf, off, 0));
        }
        // Bound size of each constant buffer the kernel reads, in the driver cbuf (see CBUF_LIMITS_OFFSET).
        if c0.len() >= CBUF_LIMITS_OFFSET + 18 * 4 {
            for b in 0..18usize {
                let l = match cached
                    .cbuffers
                    .iter()
                    .find(|c| c.0 as usize == b)
                    .map(|c| c.1)
                {
                    Some(0) => cbuf_limit(c0.len()),
                    Some(n) if n >= 3 => st
                        .uniform_buffers
                        .get(&(STAGE_COMPUTE, n as i32 - 3))
                        .map_or(0, |&(a, sz)| if a > 0x10000 { cbuf_limit(sz) } else { 0 }),
                    _ => 4096,
                };
                c0[CBUF_LIMITS_OFFSET + b * 4..CBUF_LIMITS_OFFSET + b * 4 + 4]
                    .copy_from_slice(&l.to_le_bytes());
            }
        }
        if cached.cbuf_limits && !cached.cbuffers.iter().any(|&(_, slot)| slot == 0) {
            if let Some((buf, off)) = st.cx.arena.alloc(&self.device, &c0) {
                st.issuer.push(super::issuer::Op::CBuf(buf, off, 17));
            }
        }
        for &(binding, slot) in &cached.cbuffers {
            let bound = match slot {
                0 => st.cx.arena.alloc(&self.device, &c0),
                1 => {
                    let mut g = shm.lock().unwrap();
                    self.const_bank(&mut g, code.data, code.control, cached)
                }
                .map(|b| (b, 0)),
                n if n >= 3 => match st
                    .uniform_buffers
                    .get(&(STAGE_COMPUTE, n as i32 - 3))
                    .copied()
                {
                    Some((addr, size))
                        if addr > 0x10000 && size > 0 && gpu_written(&st.gpu_dirty, addr, size) =>
                    {
                        let mut g = shm.lock().unwrap();
                        self.guest_buffer_ro(&mut g, &mut st.cx, addr, size.min(CBUF_DECL_SIZE))
                    }
                    Some((addr, size)) if addr > 0x10000 && size > 0 => st.cx.arena.alloc(
                        &self.device,
                        std::slice::from_raw_parts(addr as *const u8, size.min(CBUF_DECL_SIZE)),
                    ),
                    _ => None,
                },
                _ => None,
            };
            let (buf, off) = match bound {
                Some(b) => b,
                None => {
                    if want_notes {
                        notes.push(format!("cbuf{} -> mtl{} zero buffer", slot, binding));
                    }
                    match {
                        let mut g = shm.lock().unwrap();
                        self.zero_buffer(&mut g)
                    } {
                        Some(z) => (z, 0),
                        None => continue,
                    }
                }
            };
            st.issuer
                .push(super::issuer::Op::CBuf(buf, off, binding as usize));
        }
        for &(binding, _slot, cb_slot, cb_off) in &cached.sbuffers {
            let byte = cb_off as usize * 4;
            let desc = if cb_slot == 0 && byte + 12 <= c0.len() {
                let a = u64::from_le_bytes(c0[byte..byte + 8].try_into().unwrap());
                let sz = u32::from_le_bytes(c0[byte + 8..byte + 12].try_into().unwrap()) as u64;
                Some((a, sz))
            } else if cb_slot >= 3 {
                st.uniform_buffers
                    .get(&(STAGE_COMPUTE, cb_slot as i32 - 3))
                    .copied()
                    .and_then(|(ua, usz)| {
                        (ua > 0x10000 && byte + 12 <= usz).then(|| {
                            let p = (ua as usize + byte) as *const u8;
                            (
                                std::ptr::read_unaligned(p as *const u64),
                                std::ptr::read_unaligned(p.add(8) as *const u32) as u64,
                            )
                        })
                    })
            } else {
                None
            };
            // Storage buffers the shader never writes are GPU reads (write tracking and the early-fence guard keep them).
            let ro = cached.sb_readonly.contains(&binding);
            let bound = desc.and_then(|(a, sz)| {
                if a > 0x10000 && sz > 0 {
                    let mut g = shm.lock().unwrap();
                    if ro {
                        self.guest_buffer_ro(&mut g, &mut st.cx, a, sz as usize)
                    } else {
                        super::note_gpu_guest_write(super::GuestWrite::Storage, a, sz as usize);
                        self.guest_buffer(&mut g, &mut st.cx, a, sz as usize)
                    }
                } else {
                    None
                }
            });
            if let (Some((a, sz)), Some(_)) = (desc, &bound) {
                super::mark_gpu_dirty(st, a, a + sz);
            }
            if want_notes {
                notes.push(format!(
                    "sb mtl{} desc cb{}+{:#x} -> {:x?} {}",
                    binding,
                    cb_slot,
                    byte,
                    desc,
                    if bound.is_some() { "bound" } else { "zero" }
                ));
            }
            let (buf, off) = match bound {
                Some(b) => b,
                None => match {
                    let mut g = shm.lock().unwrap();
                    self.zero_buffer(&mut g)
                } {
                    Some(z) => (z, 0),
                    None => continue,
                },
            };
            st.issuer
                .push(super::issuer::Op::CBuf(buf, off, binding as usize));
        }
        let mut extra_samplers: Vec<(usize, Retained<ProtocolObject<dyn MTLSamplerState>>)> =
            Vec::new();
        for tb in &cached.textures {
            let (tex, sampler_id) = self.bound_texture(st, STAGE_COMPUTE, tb);
            let Some(tex) = tex.or_else(|| self.dummy_texture(&mut st.tc, &tb.ty)) else {
                continue;
            };
            if want_notes {
                notes.push(format!(
                    "tex b{} {} -> {:?} {}x{} {:?}",
                    tb.binding,
                    tb.ty,
                    tex.textureType(),
                    tex.width(),
                    tex.height(),
                    tex.pixelFormat()
                ));
            }
            let sampler = {
                let mut g = shm.lock().unwrap();
                sampler_id
                    .and_then(|id| self.sampler_state(&mut g, id))
                    .or_else(|| self.default_sampler(&mut g))
            };
            st.issuer
                .push(super::issuer::Op::CTex(tex, tb.binding as usize));
            if (tb.binding as usize) < EXTRA_SAMPLER_BASE {
                st.issuer
                    .push(super::issuer::Op::CSamp(sampler, tb.binding as usize));
            } else if let Some(s) = sampler {
                extra_samplers.push((tb.binding as usize - EXTRA_SAMPLER_BASE, s));
            }
        }
        if let Some((buf, off)) = self.extra_sampler_buffer(&mut st.cx, &extra_samplers) {
            st.issuer
                .push(super::issuer::Op::CBuf(buf, off, EXTRA_SAMPLER_BUFFER));
        }
        // Storage images: the shader reads image unit i's handle from the same cbuf 2 table as textures, at entry 32 + i
        // (byte 0x120 + 8i, after the 32 texture entries), which BindImage fills.
        for tb in &cached.images {
            let unit = (tb.handle_index.wrapping_sub(8) / 2) as i32 - 32;
            let tex = st.images_bound.get(&(STAGE_COMPUTE, unit)).copied().and_then(|handle| {
                let (tex_addr, view) = crate::nvn::api::lookup_pool_texture((handle & 0xFFFFF) as i32)?;
                let t = self.texture(&mut st.tc, tex_addr)?;
                if !t.usage().contains(MTLTextureUsage::ShaderWrite) {
                    warn_once(format!("kimg-usage-{:x}", tex_addr), format!("image {:#x} bound for {} was not created with NVN_TEXTURE_FLAGS_IMAGE", tex_addr, cached.entry));
                    return None;
                }
                texture_for_binding(&mut st.tc.view_cache, &t, view.as_ref(), MTLTextureType(tb.declared as _), [2, 3, 4, 5])
            });
            let Some(tex) = tex else {
                warn_once(
                    format!("kimg-{}-{}", cached.entry, unit),
                    format!(
                        "compute {} image unit {} has no usable image bound",
                        cached.entry, unit
                    ),
                );
                continue;
            };
            st.tc.gpu_written.insert(super::storage_id(&tex)); // contents now differ from guest memory
            if want_notes {
                notes.push(format!(
                    "img b{} unit {} -> {:?} {}x{}x{} {:?}",
                    tb.binding,
                    unit,
                    tex.textureType(),
                    tex.width(),
                    tex.height(),
                    tex.depth(),
                    tex.pixelFormat()
                ));
            }
            st.issuer
                .push(super::issuer::Op::CTex(tex, tb.binding as usize));
        }
        if super::tracing_frame() {
            println!(
                "[ftrace] DISPATCH {} groups {}x{}x{} local {}x{}x{} | {}",
                cached.entry,
                x,
                y,
                z,
                lx,
                ly,
                lz,
                notes.join(" | ")
            );
        }
        st.issuer.push(super::issuer::Op::CDispatch(
            MTLSize {
                width: x as usize,
                height: y as usize,
                depth: z as usize,
            },
            MTLSize {
                width: lx as usize,
                height: ly as usize,
                depth: lz as usize,
            },
        ));
        st.issuer.push(super::issuer::Op::CLinger);
        st.stats.passes += 1;
    }

    /// Render-target feedback. TotK's terrain decals (and ~20 other shaders, ~50 draws per frame) sample G-buffer targets
    /// of the pass they draw into, at their own pixel, after an nvnCommandBufferBarrier(ORDER_FRAGMENTS |
    /// INVALIDATE_TEXTURE): on the Switch they read what was rendered before the barrier. A Metal render pass keeps its
    /// attachments in tile memory, so sampling one inside the pass reads stale memory; the decals' fades then came out as
    /// solid dark patches that changed from frame to frame. For each such binding: once per barrier epoch, end the pass,
    /// copy the attachment's root texture, and bind the copy (a matching view of it); the next pass loads the targets.
    /// Draws after the first one in an epoch read the same copy, as the texture cache on the Switch would until the next
    /// barrier. --no-fb-snapshot disables.
    unsafe fn feedback_snapshots(
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

    pub(super) unsafe fn draw_translated(
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

    unsafe fn draw_translated_inner(
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

    /// Whether this draw runs as compute (tkv_ / tkf_ kernels the cache builder generated from the
    /// two stages) instead of in a render pass (~40 us of fixed GPU time per pass on Apple GPUs, ~1-2 us per dispatch).
    /// Only when the result cannot differ from rasterizing it: see the checks. On success the open render pass is ended
    /// (pending clears first) and the targets are marked GPU-written; the packet then carries the plan.
    /// --tiny-compute enables; --replay-ab-tiny renders on replay side B; traces log declines and reasons.
    #[allow(clippy::too_many_arguments)]
    unsafe fn tiny_plan(
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
    unsafe fn tiny_plan_inner(
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
        // Large passes are slower as kernels than as tile-based fragment shading (2026-10-17: full-resolution post passes
        // as compute nearly doubled the frame's GPU time); the gain is the per-pass overhead of small ones.
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
            st.tc.gpu_written.insert(super::storage_id(t));
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
    unsafe fn emit_tiny(
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
        // Pipelines: the kernels with the function constants the render pipeline would get.
        let set_int = |cv: &MTLFunctionConstantValues, id: usize, v: i32| {
            cv.setConstantValue_type_atIndex(
                std::ptr::NonNull::new(&v as *const i32 as *mut libc::c_void).unwrap(),
                MTLDataType::Int,
                id,
            );
        };
        let set_bool = |cv: &MTLFunctionConstantValues, id: usize, v: bool| {
            cv.setConstantValue_type_atIndex(
                std::ptr::NonNull::new(&v as *const bool as *mut libc::c_void).unwrap(),
                MTLDataType::Bool,
                id,
            );
        };
        // Pipelines: the kernels with the function constants the render pipeline would get; the constant values are
        // only built when a pipeline is missing (keys: stage + the constants' inputs).
        let mut vkey: Vec<i64> = vec![
            0,
            depth_gl as i64,
            y_negate() as i64,
            point_size.to_bits() as i64,
        ];
        for (loc, _, _, _, kind) in attr_descs {
            vkey.extend([*loc as i64, *kind as i64]);
        }
        let mut fkey: Vec<i64> = vec![1];
        for &loc in &fs.colors {
            fkey.extend([
                loc as i64,
                pf_kind(
                    p.color_pfs
                        .get(loc as usize)
                        .copied()
                        .unwrap_or(MTLPixelFormat::Invalid),
                ) as i64,
            ]);
        }
        for &loc in &fs.varyings {
            fkey.extend([100 + loc as i64, vs.varyings.contains(&loc) as i64]);
        }
        let pipe = |sh: &mut SharedCaches,
                    stage: &CachedStage,
                    key: Vec<i64>|
         -> Result<
            Retained<ProtocolObject<dyn objc2_metal::MTLComputePipelineState>>,
            String,
        > {
            let k = (stage as *const CachedStage as usize, key);
            if let Some(p) = sh.tiny_pipes.get(&k) {
                return Ok(p.clone());
            }
            let cv = MTLFunctionConstantValues::new();
            let name = if k.1[0] == 0 {
                for (loc, _, _, _, kind) in attr_descs {
                    set_int(&cv, *loc as usize, *kind);
                }
                set_bool(&cv, 40, depth_gl);
                set_bool(&cv, 41, y_negate());
                cv.setConstantValue_type_atIndex(
                    std::ptr::NonNull::new(&point_size as *const f32 as *mut libc::c_void).unwrap(),
                    MTLDataType::Float,
                    42,
                );
                format!("tkv_{}", stage.entry)
            } else {
                for &loc in &fs.colors {
                    set_int(
                        &cv,
                        32 + loc as usize,
                        pf_kind(
                            p.color_pfs
                                .get(loc as usize)
                                .copied()
                                .unwrap_or(MTLPixelFormat::Invalid),
                        ),
                    );
                }
                set_bool(&cv, 43, false);
                for &loc in &fs.varyings {
                    set_bool(&cv, 48 + loc as usize, vs.varyings.contains(&loc));
                }
                format!("tkf_{}", stage.entry)
            };
            let f = lib.kernel(&stage.entry, &name, &cv)?;
            let p = self
                .device
                .newComputePipelineStateWithFunction_error(&f)
                .map_err(|e| e.localizedDescription().to_string())?;
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

    /// First half of a translated draw, in command order: shader lookup and skips, render targets, the feedback decision,
    /// the render pass. Err(consumed) ends the draw here (see draw_translated's return value).
    unsafe fn plan_draw(
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
                        (crate::metal::shader_cache::fnv1a64(b), (w0 >> 10) & 0xF)
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
                            if !st
                                .bc_pending
                                .iter()
                                .any(|(q, _, _)| Retained::as_ptr(q) == Retained::as_ptr(&p))
                            {
                                st.bc_pending
                                    .push((p.clone(), t.clone(), rt.level as usize));
                            }
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

                (
                    std::sync::Arc::new(super::CachedPassRts {
                        color_texs,
                        depth_tex,
                        color_pfs,
                        depth_pf,
                        enc_key,
                        target_size,
                    }),
                    targetless,
                )
            };
        let color_texs = &rts.color_texs;
        let depth_tex = &rts.depth_tex;
        let (color_pfs, depth_pf, target_size) = (rts.color_pfs, rts.depth_pf, rts.target_size);
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
            let h = crate::metal::shader_cache::fnv1a64(
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
            st.tex_reuse = Some((key, resolved.clone()));
        }
        if st.enc.is_some() && st.enc_fb_epoch != st.fb_epoch {
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
                color_pfs,
                depth_pf,
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
                att.setClearDepth(clear_depth.unwrap_or(0.0) as f64);
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
                            "[nvn {:x} gen {} fmt {} vfmt {} flags {:#x} lv {}]",
                            rt.texture, gen, x.format, rt.view_format, x.flags, x.levels
                        );
                    }
                }
                if let Some((t, d)) = &depth_tex {
                    let storage =
                        unsafe { super::tex_info(d.texture) }.map_or(0, |(_, x)| x.storage);
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
            st.enc_fb_epoch = st.fb_epoch;
            st.cx.ec = EncCache::default();
            // --no-enc-cache: every draw sets all state (A/B).
            st.enc_key = enc_key.clone();
            self.note_encoder(st, format!("translated pass {}", shape_label));
            // Every draw of the pass has these targets (the pass key compares them): marked once per pass.
            for (_, t, _) in color_texs.iter() {
                st.tc.gpu_written.insert(super::storage_id(t));
            }
            if let Some((t, _)) = &depth_tex {
                st.tc.gpu_written.insert(super::storage_id(t));
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
            color_pfs,
            depth_pf,
        );
        self.plan_uniforms(st, pkt);
        Ok(())
    }

    /// Uniform buffer decisions that depend on command order (the encode half may run later, after the plan has
    /// applied this submit's inline uniform updates and counter reports to guest memory, or marked more memory as
    /// GPU-written): zero-copy for GPU-written memory, a copy made now for memory an executor write of this submit
    /// overlaps (st.ubo_updated covers every such range), and the storage buffer descriptors shaders read from uniform
    /// buffers.
    unsafe fn plan_uniforms(&self, st: &mut State, pkt: &mut DrawPacket) {
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
                                        let bytes =
                                            std::slice::from_raw_parts(addr as *const u8, n);
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
                        let q = (ua as usize + byte) as *const u8;
                        (
                            std::ptr::read_unaligned(q as *const u64),
                            std::ptr::read_unaligned(q.add(8) as *const u32) as u64,
                        )
                    })
                });
                pkt.sb_desc.push((stage, binding, desc));
            }
        }
    }

    /// Second half of a translated draw: everything that only needs the draw's packet and the caches (vertex buffers,
    /// pipeline, resources, the draw itself). Reads no NVN state from `State`: the packet holds what the draw uses.
    unsafe fn encode_draw(&self, env: &mut EncodeEnv, p: &DrawPacket) -> bool {
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
                super::pipeline_list::precompile();
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
                let key = pipeline_key(
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
                    };
                    match async_pipeline(key.as_slice(), job) {
                        AsyncPipe::Ready(pipe) => {
                            env.sh
                                .translated_pipelines
                                .insert(key.to_vec(), pipe.clone());
                            record_pipeline(vs, fs, p, point_size, depth_gl, &blends);
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
                    record_pipeline(vs, fs, p, point_size, depth_gl, &blends);
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
            };
            // One copy per distinct content: consecutive draws mostly share it (--no-driver-cb-memo: a copy per draw).
            let dkey = DriverCbKey {
                base_vertex: base_vertex as i32,
                base_instance: base_instance_value,
                sb_descs: sb_descs.clone(),
                limits,
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
            if cached.cbuf_limits && !cached.cbuffers.iter().any(|&(_, slot)| slot == 0) {
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
                                        // Only ranges no executor write of this submit touches get here (plan_uniforms): never invalidated.
                                        env.cx.ubo_copies.insert(key, f.clone(), false);
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
                        let nz = if t.storage > 0x10000 && crate::cpu::crash_handler::readable(t.storage as usize, 0x4000) { std::slice::from_raw_parts(t.storage as *const u8, 0x4000).iter().filter(|&&x| x != 0).count() } else { 0 };
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
                let sampler = sampler_id
                    .and_then(|id| self.sampler_state(&mut *env.sh, id))
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
                        restart_rewrite(mode, index_type, index_addr, count, r, base_vertex)
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
                        let u8s =
                            std::slice::from_raw_parts(index_addr as *const u8, count as usize);
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

    /// For a render target that is a block-compressed texture written through an uncompressed view whose texel is one
    /// block (e.g. RGBA16UI over BC4), an uncompressed texture with one texel per block to render into instead.
    /// The target drawn through a view of another format (TotK renders the RGBA8 swapchain through an RGBA8_SRGB view,
    /// so the hardware encodes linear output to sRGB). Views are cached so consecutive draws keep sharing an encoder.
    fn rt_format_view(&self, st: &mut State, t: &Tex, rt: &RtRef) -> Option<Tex> {
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

    fn bc_proxy(&self, st: &mut State, t: &Tex, rt: &RtRef) -> Option<Tex> {
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

    /// Samplers at index 16+ (beyond Metal's per-stage sampler table): an argument buffer of their GPU handles, laid out
    /// as crates/cli/src/msl_specialize.rs's `spvExtraSamplers` (one MTLResourceID per slot).
    fn extra_sampler_buffer(
        &self,
        cx: &mut EncodeCtx,
        samplers: &[(usize, Retained<ProtocolObject<dyn MTLSamplerState>>)],
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        let n = samplers.iter().map(|(i, _)| i + 1).max()?;
        let mut ids = vec![0u64; n];
        for (i, s) in samplers {
            ids[*i] = unsafe { std::mem::transmute::<_, u64>(s.gpuResourceID()) };
            // MTLResourceID is one u64
        }
        let bytes = unsafe { std::slice::from_raw_parts(ids.as_ptr() as *const u8, ids.len() * 8) };
        cx.arena.alloc(&self.device, bytes)
    }
    fn default_sampler(
        &self,
        sh: &mut SharedCaches,
    ) -> Option<Retained<ProtocolObject<dyn MTLSamplerState>>> {
        if let Some(s) = &sh.default_sampler {
            return Some(s.clone());
        }
        let sd = MTLSamplerDescriptor::new();
        sd.setMinFilter(MTLSamplerMinMagFilter::Linear);
        sd.setMagFilter(MTLSamplerMinMagFilter::Linear);
        sd.setSAddressMode(MTLSamplerAddressMode::ClampToEdge);
        sd.setTAddressMode(MTLSamplerAddressMode::ClampToEdge);
        sd.setSupportArgumentBuffers(true);
        let s = self.device.newSamplerStateWithDescriptor(&sd)?;
        sh.default_sampler = Some(s.clone());
        Some(s)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_translated_pipeline(
        &self,
        lib: &ShaderLibs,
        vs: &CachedStage,
        fs: Option<&CachedStage>,
        attrs: &[(u32, MTLVertexFormat, usize, usize, i32)],
        streams_used: &[bool; MAX_STREAMS],
        vertex_streams: &[u64],
        color_pfs: &[MTLPixelFormat],
        depth_pf: MTLPixelFormat,
        depth_gl: bool,
        point_size: f32,
        blends: &[u64; 8],
        color_state: u32,
        channel_mask: u32,
        _blend_color: [f32; 4],
        batched: bool,
    ) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
        let desc = pipeline_descriptor(
            &self.device,
            lib,
            vs,
            fs,
            attrs,
            streams_used,
            vertex_streams,
            color_pfs,
            depth_pf,
            depth_gl,
            y_negate(),
            point_size,
            blends,
            color_state,
            channel_mask,
            batched,
        )?;
        super::pipeline_archive::create(&self.device, &desc, &vs.entry, fs.map_or("-", |f| f.entry.as_str()))
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
mod contains_u16_tests {
    #[test]
    fn matches_slice_contains() {
        let mut seed = 0x1234_5678u32;
        for len in 0..80usize {
            for &x in &[0u16, 1, 0x7FFF, 0x8000, 0xFFFE, 0xFFFF] {
                for off in 0..4 {
                    let mut v: Vec<u16> = (0..len + off)
                        .map(|_| {
                            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                            (seed >> 16) as u16 | 1
                        })
                        .collect();
                    v.retain(|&y| y != x);
                    v.resize(len + off, if x == 1 { 2 } else { 1 });
                    let s = &v[off.min(v.len())..];
                    assert_eq!(super::contains_u16(s, x), s.contains(&x));
                    if !s.is_empty() {
                        let mut w = s.to_vec();
                        let i = (seed as usize) % w.len();
                        w[i] = x;
                        assert!(
                            super::contains_u16(&w, x),
                            "len {} x {:#x} at {}",
                            w.len(),
                            x,
                            i
                        );
                    }
                }
            }
        }
    }
}

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
        if let Some(r) = &mut self.rec {
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
        if let Some(r) = &mut self.rec {
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
        if let Some(r) = &mut self.rec {
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
        if let Some(r) = &mut self.rec {
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
        if let Some(r) = &mut self.rec {
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
        if let Some(r) = &mut self.rec {
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
    if writers && !crate::cpu::crash_handler::WRITER_LOG.load(std::sync::atomic::Ordering::Relaxed)
    {
        crate::cpu::crash_handler::WRITER_LOG.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let frame = super::presented_frames();
    if frame != p.frame {
        if writers {
            // Faults since the last frame change: those on pages this frame's uniforms sat on name their writers.
            let base = crate::cpu::crash_handler::get_guest_base() as u64;
            let end =
                crate::cpu::crash_handler::WRITER_IDX.load(std::sync::atomic::Ordering::Relaxed);
            let ring = &crate::cpu::crash_handler::WRITER_RING;
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
                if k < crate::cpu::crash_handler::WRITER_PAGES.len() * 64 {
                    crate::cpu::crash_handler::WRITER_ARMED[k / 64]
                        .fetch_or(1 << (k % 64), std::sync::atomic::Ordering::Relaxed);
                    crate::cpu::crash_handler::WRITER_PAGES[k / 64]
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
