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
pub(crate) use textures::*;
mod state;
pub(crate) use state::*;
mod formats;
pub(crate) use formats::*;
pub(crate) mod backend;
mod batch;
mod copy;
mod downscale;
mod fence_guard;
mod issuer;
pub use nxport_metalshaderlib::msl_specialize;
mod pass_graph;
mod pass_plan;
pub use nxport_metalshaderlib::source_pack;
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


/// translated::root_id (memoized on the texture itself).
fn cached_root_id(_st: &mut State, t: &Tex) -> u64 {
    translated::root_id(t)
}


/// Texture bindings resolved by bound_texture: [memo hit without lookups, memo hit after lookups, resolved].
pub(super) static BIND_STATS: [std::sync::atomic::AtomicU64; 3] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 3];

/// Alias views created (not found in alias_views), for the timeline log.
static ALIAS_NEW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);






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












