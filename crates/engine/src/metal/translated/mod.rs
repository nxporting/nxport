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
mod pipelines;
mod shaders;
mod vertex;
mod worker;
mod enc_cache;
mod diag;
pub(crate) use binding::*;
pub(crate) use tiny::*;
pub(crate) use pipelines::*;
pub(crate) use shaders::*;
pub(crate) use vertex::*;
pub(crate) use worker::*;
pub(crate) use enc_cache::*;
pub(crate) use diag::*;

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















/// --draw-details: per-draw debug groups and per-draw binding details for GPU error/hang reports.
fn draw_details() -> bool {
    crate::settings::get().debug.draw_details
}

/// Encoder labels with attachment shapes ([c<i>:WxH:fmt,...,d:WxH:fmt]): --enc-labels or --draw-details.
fn full_labels() -> bool {
    crate::settings::get().debug.enc_labels || draw_details()
}

/// --gfx-trace=N logs the state of the first N translated draws.



fn trace_limit() -> u64 {
    static T: OnceLock<u64> = OnceLock::new();
    *T.get_or_init(|| crate::settings::get().trace.gfx_trace)
}








thread_local! {
    static LOCAL_PACKETS: std::cell::RefCell<Vec<Box<DrawPacket>>> = const { std::cell::RefCell::new(Vec::new()) };
}








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


/// Code and control section of a translated stage (const_bank needs them).
#[derive(Clone, Copy)]
pub(super) struct CodeRef {
    pub(super) data: u64,
    pub(super) control: u64,
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



// ---- executor ---------------------------------------------------------------------------------------------------









use super::issuer::Op;
use objc2::Message as _;





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

