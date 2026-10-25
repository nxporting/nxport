// Running recorded NVN command lists on Vulkan.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::hash::{Hash, Hasher};

use ash::vk;
use sass2spv::accessor::GraphicsState;
use sass2spv::types::AttributeType;

use super::device::Dev;
use super::shader::{self, Feed, Stage};
use super::texture::{self, Textures};
use super::{format, pipelines, state, Arena, Imports};
use crate::gpu::{GpuOnlyTexture, Texels};
use crate::nvn::api::NvnTexture;
use crate::nvn::cmd::{AttribState, Cmd, RtRef};

#[path = "copy.rs"]
mod copy;
#[path = "draw.rs"]
mod draw;
pub(super) use draw::{desc_streams, fragment_graphics_state, vertex_graphics_state, vertex_streams};

/// NVN state the commands set (it persists across submits, as the queue's does).
#[derive(Default)]
struct Nvn {
    colors: Vec<RtRef>,
    depth: Option<RtRef>,
    viewport: (i32, i32, i32, i32),
    scissor: Option<(i32, i32, i32, i32)>,
    depth_range: (f32, f32),
    blend: [u64; 8],
    color_state: u32,
    channel_mask: Option<u32>,
    depth_stencil: Option<u64>,
    polygon: u32,
    multisample: u64,
    /// Stencil reference, value (read) mask and write mask, [front, back].
    stencil: [[u32; 2]; 3],
    blend_color: [f32; 4],
    program: usize,
    compute_program: usize,
    vertex_buffers: [(u64, usize); 16],
    attribs: AttribState,
    streams: Vec<u64>,
    uniforms: HashMap<(i32, i32), (u64, usize)>,
    storage: HashMap<(i32, i32), (u64, usize)>,
    textures: HashMap<(i32, i32), u64>,
    images: HashMap<(i32, i32), u64>,
    render_disabled: bool,
    prim_restart: Option<u32>,
    depth_clamp: bool,
    polygon_offset: (f32, f32, f32),
    point_size: f32,
}

/// The render pass being recorded.
struct Pass {
    /// The attached images (render-target feedback: a draw sampling one of them).
    images: Vec<vk::Image>,
    colors: Vec<Option<(vk::ImageView, vk::Format, i32)>>,
    depth: Option<(vk::ImageView, vk::Format)>,
    /// The targets' NVN formats (texture, view; pipeline lists).
    nvn_colors: [(u32, u32); 8],
    nvn_depth: (u32, u32),
    extent: (u32, u32),
    samples: vk::SampleCountFlags,
    /// Rendering begins at the first draw: clears of whole attachments before it become their load operation (a tiled
    /// GPU then never loads them), as the Metal backend folds clears into its load actions.
    begun: bool,
    color_clears: Vec<Option<vk::ClearColorValue>>,
    depth_clear: Option<(vk::ImageAspectFlags, vk::ClearDepthStencilValue)>,
    pub(super) draws: u32,
}

// The context is only used under the backend's lock; its raw pointers point into guest memory and mapped buffers.
unsafe impl Send for Ctx {}

pub struct Ctx {
    dev: std::sync::Arc<Dev>,
    /// Program stages: (code, control section, content hash).
    pub programs: HashMap<usize, Vec<(u64, u64, u64)>>,
    tex: Textures,
    imports: Imports,
    arena: Arena,
    stages: HashMap<(u64, u64), Stage>,
    failed_stages: HashSet<(u64, u64)>,
    pipelines: HashMap<u64, (vk::Pipeline, vk::PipelineLayout)>,
    layouts: HashMap<(vk::DescriptorSetLayout, vk::DescriptorSetLayout), vk::PipelineLayout>,
    /// Vertex / fragment stage pairs: (set layout, pipeline layout, descriptors pushed).
    graphics_layouts: HashMap<((u64, u64), Option<(u64, u64)>), (vk::DescriptorSetLayout, vk::PipelineLayout, bool)>,
    empty_layout: vk::DescriptorSetLayout,
    desc_pools: Vec<vk::DescriptorPool>,
    st: Nvn,
    cb: vk::CommandBuffer,
    pass: Option<Pass>,
    /// The device was lost: nothing more is submitted.
    lost: bool,
    /// Storage buffers bound as copies (the device could not use guest memory in place): (guest address, copy, length),
    /// copied back once the submit completed.
    writebacks: Vec<(usize, *const u8, usize)>,
    /// Guest memory GPU work of this submit writes (copy and clear destinations, storage buffers): [start, end). A uniform
    /// buffer there is bound in place, so it sees those writes in GPU order (a copy taken while recording would not).
    gpu_dirty: Ranges,
    /// Render-target feedback: a draw sampling an attachment of its own pass reads a copy of it taken at the first such
    /// draw after the last barrier that invalidates textures (NVN: it sees the writes from before the barrier; sampling
    /// an attachment inside a Vulkan render pass is undefined). Copies by texture description, with their epoch.
    fb_snaps: rustc_hash::FxHashMap<texture::Desc, (u64, texture::Image)>,
    fb_epoch: u64,
    /// GPU texture compression: the game renders compressed blocks through an integer view of a block-compressed texture
    /// (one texel per block). Those passes render into an uncompressed proxy, copied into the texture's level when the
    /// pass ends. Proxies by (texture, level, view format); the ones the current pass renders.
    bc_proxies: rustc_hash::FxHashMap<(texture::Desc, u32, vk::Format), texture::Image>,
    bc_pending: Vec<(texture::Desc, u32, vk::Format)>,
    /// Render-target write-back: TotK moves textures with CPU copies (heap compaction) and reads some on the CPU, so
    /// GPU-rendered contents must reach guest memory, as on the Switch where the GPU renders into memory the CPU shares.
    /// Targets not rendered in the previous 4 frames (generated maps, not per-frame targets) are copied back at the
    /// present of the frame that drew them (the Metal backend's writeback_pass waits one more frame for its GPU work;
    /// submits here complete before they return). Last frame each storage was rendered in; this frame's candidates.
    rt_history: HashMap<usize, u64>,
    writeback_rts: Vec<NvnTexture>,
    /// Candidates of presented frames, read by the next submit; that submit's reads (texture, level, copy, bytes) and
    /// the images whose guest bytes it rewrites (settled again once it completed).
    pending_rt_reads: Vec<NvnTexture>,
    rt_reads: Vec<(NvnTexture, u32, usize, usize)>,
    rt_resettle: Vec<texture::Desc>,
    /// Staging memory of the uploads of the submit being recorded.
    staging: Vec<super::device::HostBuffer>,
    default_sampler: vk::Sampler,
    /// Zeros for unbound vertex attributes and resources (never written).
    pub(super) zeros: super::device::HostBuffer,
    /// Repacked vertex streams of this submit, by (guest range, layout, elements).
    pub(super) repacked: rustc_hash::FxHashMap<u64, (vk::Buffer, u64)>,
    /// Texture bindings resolved in this submit: (handle, view type) -> (image, view, sampler).
    pub(super) bindings: rustc_hash::FxHashMap<(u64, vk::ImageViewType), (vk::Image, vk::ImageView, vk::Sampler)>,
    /// Repacked vertex streams kept across submits (draw.rs cached_repack), by (memory, layout).
    pub(super) repack_cache: rustc_hash::FxHashMap<u64, draw::RepackedStream>,
    repack_swept: u64,
    /// Largest index of index ranges scanned in this submit, by (address, count, type, restart index).
    pub(super) max_indices: rustc_hash::FxHashMap<(usize, i32, i32, Option<u32>), u32>,
    /// Vertex streams bound in place or as copies in this submit, by (host address, size).
    pub(super) vb_memo: rustc_hash::FxHashMap<(usize, usize), (vk::Buffer, u64)>,
    /// Scratch for the driver constant buffer of a draw (draw.rs driver_cbuf).
    pub(super) driver_scratch: Vec<u8>,
    /// Descriptor sets written in this submit, by contents.
    pub(super) set_memo: rustc_hash::FxHashMap<u64, vk::DescriptorSet>,
    /// The dynamic state set in the current pass (draw.rs dynamic_state).
    pub(super) dynamic_memo: Option<u64>,
    /// The vertex layout of the bound attribute and stream state, and its hash (draw.rs vertex_layout).
    pub(super) vl_cache: Option<(std::sync::Arc<[draw::VStream]>, u64, u64)>,
    /// Support buffers of this submit, by contents (viewport, bound sizes).
    pub(super) supports: rustc_hash::FxHashMap<u64, (vk::Buffer, u64)>,
    /// Restart rewrites of this submit (gpu::restart), by (mode, index type, indices, count, restart, base vertex).
    pub(super) restart_memo: rustc_hash::FxHashMap<(i32, i32, usize, i32, u32, i32), Option<(vk::Buffer, u64, u32)>>,
    /// Uniform buffer copies of this submit by (host address, length, epoch); the epoch advances at every write of guest
    /// memory by this submit's commands (inline uniform updates, reports).
    pub(super) ubo_copies: rustc_hash::FxHashMap<(usize, usize, u64), (vk::Buffer, u64)>,
    pub(super) ubo_epoch: u64,
    /// Two timestamps around each submit's commands: the GPU's own time for it (null if the queue has none); then, when
    /// profiling passes, one at the end of each pass, with a description of the pass.
    timestamps: vk::QueryPool,
    timestamps_supported: bool,
    pub(super) profile_passes: bool,
    pass_labels: Vec<String>,
    occl: Occlusion,
    /// The submit's command buffers already handed to the queue (the GPU starts on them while the rest is recorded: an
    /// idle GPU runs at a fraction of its speed), and when the current one began.
    chunks: Vec<vk::CommandBuffer>,
    chunk_start: std::time::Instant,
    /// Index of the command being executed in this submit (in run_list order).
    op_index: u64,
    /// Guest memory this submit's commands write while it is recorded (inline uniform updates, reports): uniform
    /// buffers there are copied when bound (draws before the write must not see it), others are read in place.
    pub(super) cmd_writes: Ranges,
    /// The submit's command lists by handle (fetched once by the pre-scan).
    lists: rustc_hash::FxHashMap<u64, std::sync::Arc<crate::nvn::api::RecordedCommandList>>,
    /// Pre-scans of lists without calls, by list.
    scan_cache: rustc_hash::FxHashMap<usize, (std::sync::Arc<crate::nvn::api::RecordedCommandList>, ListScan)>,
    inflight: std::collections::VecDeque<InFlight>,
    /// The command pool of the submit being recorded (one per submit in flight); retired pools' command buffers are freed
    /// on a helper thread (vk-pool-free), which then returns the pools for reuse.
    pool: vk::CommandPool,
    pool_tx: std::sync::mpsc::Sender<(vk::CommandPool, Vec<vk::CommandBuffer>)>,
    free_pools: std::sync::Arc<std::sync::Mutex<Vec<vk::CommandPool>>>,
    /// Query and descriptor pools of retired submits, for reuse.
    free_occl: Vec<vk::QueryPool>,
    free_timestamps: Vec<vk::QueryPool>,
    free_desc_pools: Vec<vk::DescriptorPool>,
    warned: HashSet<String>,
    /// The pipeline cache (stage_cache.rs; made at the first pipeline), and pipelines created since it was last saved.
    pipeline_cache: Option<vk::PipelineCache>,
    /// Pipelines created at the last save, and its frame.
    saved_pipelines: u64,
    pipeline_cache_saved: u64,
    /// Window output (vulkan/window.rs), made at the first present when the display has a layer.
    window: Option<super::window::Window>,
    window_tried: bool,
    /// Commands warned about as not implemented (by name, by kind).
    unimplemented: HashSet<&'static str>,
    unimplemented_kinds: HashSet<std::mem::Discriminant<Cmd>>,
}

unsafe fn dev_sampler(dev: &Dev) -> vk::Sampler {
    dev.device.create_sampler(&vk::SamplerCreateInfo::default(), None).unwrap_or(vk::Sampler::null())
}

/// Writes a report (NVNcounterData: value, timestamp) when the command is recorded: what follows in the stream
/// (conditional rendering, uniform copies) sees it in order. Submits complete before they return, so the guest cannot
/// observe it earlier than the GPU's own write.
pub(super) unsafe fn write_report(addr: u64, value: u64, timestamp: bool) {
    let a = crate::nvn::api::translate_virtual(addr as usize);
    if a < 0x10000 {
        return;
    }
    super::guard::note_write(a, 16);
    std::ptr::write_unaligned(a as *mut u64, value);
    std::ptr::write_unaligned((a + 8) as *mut u64, if timestamp { crate::clock::now_ns() } else { 0 });
}

/// Disjoint address ranges [start, end), merged as they are added.
#[derive(Default)]
pub(super) struct Ranges(std::collections::BTreeMap<usize, usize>);

impl Ranges {
    pub(super) fn add(&mut self, a: usize, b: usize) {
        if a >= b {
            return;
        }
        let (mut lo, mut hi) = (a, b);
        if let Some((&s, &e)) = self.0.range(..=lo).next_back() {
            if e >= lo {
                if e >= hi {
                    return;
                }
                lo = s;
            }
        }
        while let Some((&s, &e)) = self.0.range(lo..=hi).next() {
            self.0.remove(&s);
            hi = hi.max(e);
        }
        self.0.insert(lo, hi);
    }

    pub(super) fn overlaps(&self, a: usize, b: usize) -> bool {
        self.0.range(..b).next_back().is_some_and(|(_, &e)| e > a)
    }

    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
}

/// Pass-end timestamps per submit when profiling passes.
const PASS_TIMESTAMPS: u32 = 1000;

/// --split-ms=T (default 1, 0 = one command buffer per submit): the command buffer is handed to the queue between
/// passes once T ms were spent recording it.
fn chunk() -> Option<std::time::Duration> {
    static T: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    let ms = *T.get_or_init(|| crate::settings::get().graphics.split_ms);
    (ms > 0.0).then(|| std::time::Duration::from_secs_f64(ms / 1e3))
}

/// Size of the zero buffer.
const ZEROS: u64 = 4096;

/// Occlusion queries (SAMPLES_PASSED counters). Each stretch of a render pass gets its own Vulkan query (one cannot span
/// passes); a report sums those since the counter's last reset and is written to guest memory once its submit completed,
/// as the GPU would. Conditional rendering on a report still pending in the submit being recorded renders (its value is
/// not known yet; the Metal backend and Ryujinx do the same).
pub(super) struct Occlusion {
    pool: vk::QueryPool,
    /// Queries of this submit: next free, the one running in the current pass.
    next: u32,
    active: Option<u32>,
    /// This submit's query ends, resets and reports in order (the watcher sums them, inflight::Occl), and its report
    /// addresses.
    events: Vec<super::inflight::Occl>,
    reports: Vec<u64>,
    precise: bool,
    /// The submit's counter resets (false) and reports (true) by command index (Ctx::op_index), and the next one.
    upcoming: Vec<(u64, bool)>,
    cursor: usize,
}

impl Occlusion {
    const QUERIES: u32 = 8192;

    fn new(dev: &Dev) -> Occlusion {
        let pool = Self::new_pool(dev);
        Occlusion {
            pool,
            next: 0,
            active: None,
            events: Vec::new(),
            reports: Vec::new(),
            precise: dev.precise_occlusion,
            upcoming: Vec::new(),
            cursor: 0,
        }
    }

    fn new_pool(dev: &Dev) -> vk::QueryPool {
        unsafe {
            dev.device
                .create_query_pool(&vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::OCCLUSION).query_count(Self::QUERIES), None)
                .unwrap_or(vk::QueryPool::null())
        }
    }
}

/// A submit the GPU may still be running, with what it uses: freed once its serial completed (inflight.rs).
struct InFlight {
    serial: u64,
    pool: vk::CommandPool,
    cbs: Vec<vk::CommandBuffer>,
    desc_pools: Vec<vk::DescriptorPool>,
    chunks: Vec<super::device::HostBuffer>,
    occl_pool: vk::QueryPool,
    timestamps: vk::QueryPool,
    imports: Vec<(vk::Buffer, vk::DeviceMemory)>,
    /// Occlusion report addresses, written by the watcher.
    reports: Vec<u64>,
    /// Images whose guest bytes the watcher rewrote (render-target write-back).
    resettle: Vec<texture::Desc>,
    /// Staging memory of its uploads.
    staging: Vec<super::device::HostBuffer>,
}

pub(super) fn hash_of<T: Hash>(v: &T) -> u64 {
    hash(v)
}

fn hash<T: Hash>(v: &T) -> u64 {
    let mut h = rustc_hash::FxHasher::default();
    v.hash(&mut h);
    h.finish()
}

fn attr_type_code(t: AttributeType) -> u8 {
    match t {
        AttributeType::Float => 0,
        AttributeType::Sint => 1,
        AttributeType::Uint => 2,
        AttributeType::Sscaled => 3,
        AttributeType::Uscaled => 4,
    }
}

impl Ctx {
    pub fn new(dev: Dev) -> Result<Ctx, String> {
        let default_sampler = unsafe { dev_sampler(&dev) };
        let occl = Occlusion::new(&dev);
        let timestamps_supported = dev.limits.timestamp_compute_and_graphics != 0;
        let timestamps = unsafe { new_timestamp_pool(&dev, timestamps_supported) };
        super::inflight::start(dev.device.clone(), dev.limits.timestamp_period);
        let free_pools: std::sync::Arc<std::sync::Mutex<Vec<vk::CommandPool>>> = Default::default();
        let (pool_tx, pool_rx) = std::sync::mpsc::channel::<(vk::CommandPool, Vec<vk::CommandBuffer>)>();
        {
            let device = dev.device.clone();
            let free = free_pools.clone();
            std::thread::Builder::new()
                .name("vk-pool-free".into())
                .spawn(move || {
                    while let Ok((pool, cbs)) = pool_rx.recv() {
                        unsafe { device.free_command_buffers(pool, &cbs) };
                        free.lock().unwrap().push(pool);
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        let zeros = unsafe {
            let usage = vk::BufferUsageFlags::UNIFORM_BUFFER | vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::VERTEX_BUFFER;
            let b = dev.host_buffer(ZEROS, usage)?;
            std::ptr::write_bytes(b.ptr, 0, ZEROS as usize);
            b
        };
        // Vertex formats the device cannot fetch (their attributes would read garbage).
        let missing: Vec<u32> = (1..=120)
            .filter(|&f| {
                format::vertex(f).is_some_and(|(vf, _)| unsafe {
                    !dev.instance.get_physical_device_format_properties(dev.pdev, vf).buffer_features.contains(vk::FormatFeatureFlags::VERTEX_BUFFER)
                })
            })
            .collect();
        if !missing.is_empty() {
            println!("[vulkan] vertex formats the device cannot fetch (NVN formats): {missing:?}");
        }
        let align = dev
            .limits
            .min_uniform_buffer_offset_alignment
            .max(dev.limits.min_storage_buffer_offset_alignment)
            .max(256);
        let empty_layout = unsafe {
            dev.device
                .create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default(), None)
                .map_err(|e| e.to_string())?
        };
        Ok(Ctx {
            dev: std::sync::Arc::new(dev),
            programs: Default::default(),
            tex: Textures::default(),
            imports: Imports::default(),
            arena: Arena { chunks: Vec::new(), free: Vec::new(), cur: 0, used: 0, align },
            stages: Default::default(),
            failed_stages: Default::default(),
            pipelines: Default::default(),
            layouts: Default::default(),
            graphics_layouts: Default::default(),
            empty_layout,
            desc_pools: Vec::new(),
            st: Nvn {
                depth_range: (0.0, 1.0),
                stencil: [[0, 0], [0xFF, 0xFF], [0xFF, 0xFF]],
                point_size: 1.0,
                ..Default::default()
            },
            cb: vk::CommandBuffer::null(),
            pass: None,
            lost: false,
            writebacks: Vec::new(),
            gpu_dirty: Ranges::default(),
            fb_snaps: Default::default(),
            bc_proxies: Default::default(),
            bc_pending: Vec::new(),
            rt_history: Default::default(),
            writeback_rts: Vec::new(),
            pending_rt_reads: Vec::new(),
            rt_reads: Vec::new(),
            rt_resettle: Vec::new(),
            staging: Vec::new(),
            fb_epoch: 1,
            default_sampler,
            zeros,
            repacked: Default::default(),
            bindings: Default::default(),
            max_indices: Default::default(),
            vb_memo: Default::default(),
            driver_scratch: Vec::new(),
            set_memo: Default::default(),
            dynamic_memo: None,
            vl_cache: None,
            repack_cache: Default::default(),
            repack_swept: 0,
            supports: Default::default(),
            restart_memo: Default::default(),
            ubo_copies: Default::default(),
            ubo_epoch: 0,
            timestamps,
            timestamps_supported,
            profile_passes: false,
            pass_labels: Vec::new(),
            occl,
            chunks: Vec::new(),
            chunk_start: std::time::Instant::now(),
            op_index: 0,
            cmd_writes: Ranges::default(),
            scan_cache: Default::default(),
            lists: Default::default(),
            inflight: Default::default(),
            pool: vk::CommandPool::null(),
            pool_tx,
            free_pools,
            free_occl: Vec::new(),
            free_timestamps: Vec::new(),
            free_desc_pools: Vec::new(),
            warned: Default::default(),
            unimplemented: Default::default(),
            pipeline_cache: None,
            saved_pipelines: 0,
            pipeline_cache_saved: 0,
            window: None,
            window_tried: false,
            unimplemented_kinds: Default::default(),
        })
    }

    fn warn(&mut self, what: impl Into<String>) {
        let w = what.into();
        if self.warned.insert(w.clone()) {
            println!("[vulkan] {w}");
        }
    }

    /// Records and queues a submit with `serial` (inflight.rs completes it).
    pub unsafe fn submit(&mut self, lists: &[u64], serial: u64) {
        let empty = |serial| super::inflight::Done {
            serial,
            fence: vk::Fence::null(),
            occl_pool: vk::QueryPool::null(),
            queries: 0,
            occl: Vec::new(),
            writebacks: Vec::new(),
            timestamps: vk::QueryPool::null(),
            pass_labels: Vec::new(),
            rt_writebacks: Vec::new(),
        };
        self.retire();
        if self.lost || super::inflight::LOST.load(std::sync::atomic::Ordering::Acquire) {
            self.lost = true;
            super::inflight::watch(empty(serial));
            return;
        }
        // At most a few submits ahead of the GPU.
        while self.inflight.len() >= 3 {
            let oldest = self.inflight.front().map_or(0, |f| f.serial);
            super::inflight::wait(oldest, u64::MAX);
            self.retire();
        }
        let pool = self.free_pools.lock().unwrap().pop();
        self.pool = match pool {
            Some(p) => p,
            None => match self.dev.device.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(self.dev.family).flags(vk::CommandPoolCreateFlags::TRANSIENT),
                None,
            ) {
                Ok(p) => p,
                Err(_) => {
                    super::inflight::watch(empty(serial));
                    return;
                }
            },
        };
        let d = &self.dev.device;
        let Ok(cbs) = d.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default().command_pool(self.pool).command_buffer_count(1),
        ) else {
            super::inflight::watch(empty(serial));
            return;
        };
        self.cb = cbs[0];
        self.tex.begin_batch(self.pool);
        super::guard::RECORDING.store(serial, std::sync::atomic::Ordering::Relaxed);
        self.tex.begin_submit();
        self.occl.pool = self.free_occl.pop().unwrap_or_else(|| Occlusion::new_pool(&self.dev));
        if self.timestamps_supported {
            self.timestamps = self.free_timestamps.pop().unwrap_or_else(|| new_timestamp_pool(&self.dev, true));
        }
        let d = &self.dev.device;
        let _ = d.begin_command_buffer(self.cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT));
        if self.occl.pool != vk::QueryPool::null() {
            d.cmd_reset_query_pool(self.cb, self.occl.pool, 0, Occlusion::QUERIES);
        }
        if self.timestamps != vk::QueryPool::null() {
            d.cmd_reset_query_pool(self.cb, self.timestamps, 0, 2 + if self.profile_passes { PASS_TIMESTAMPS } else { 0 });
            self.pass_labels.clear();
            d.cmd_write_timestamp(self.cb, vk::PipelineStageFlags::TOP_OF_PIPE, self.timestamps, 0);
        }
        self.record_rt_reads();
        let t0 = std::time::Instant::now();
        self.chunk_start = t0;
        let mut upcoming = std::mem::take(&mut self.occl.upcoming);
        upcoming.clear();
        let mut n = 0;
        let mut writes = std::mem::take(&mut self.cmd_writes);
        writes.clear();
        for &h in lists {
            Self::scan_counters(h, 0, &mut n, &mut upcoming, &mut writes, &mut self.scan_cache, &mut self.lists);
        }
        self.cmd_writes = writes;
        // Lists not reached in this submit may be gone: forget them now and then.
        if self.scan_cache.len() > 1 << 15 {
            self.scan_cache.clear();
        }
        self.occl.upcoming = upcoming;
        self.occl.cursor = 0;
        self.op_index = 0;
        for &h in lists {
            self.run_list(h, 0);
        }
        self.end_pass();
        if let Some(q) = self.occl.active.take() {
            self.dev.device.cmd_end_query(self.cb, self.occl.pool, q);
            self.occl.events.push(super::inflight::Occl::Query(q));
        }
        if self.timestamps != vk::QueryPool::null() {
            self.dev.device.cmd_write_timestamp(self.cb, vk::PipelineStageFlags::BOTTOM_OF_PIPE, self.timestamps, 1);
        }
        let d = &self.dev.device;
        let _ = d.end_command_buffer(self.cb);
        let fence = d.create_fence(&vk::FenceCreateInfo::default(), None).unwrap_or(vk::Fence::null());
        let cbs = self.with_uploads(self.cb);
        self.tex.end_batch();
        let d = &self.dev.device;
        if let Err(e) = d.queue_submit(self.dev.queue, &[vk::SubmitInfo::default().command_buffers(&cbs)], fence) {
            let frame = super::VULKAN.frames.load(std::sync::atomic::Ordering::Relaxed);
            println!("[vulkan] submit of {} lists at presented frame {frame}: vkQueueSubmit: {e}; no more GPU work", lists.len());
            self.lost = true;
        }
        super::SUBMITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let record = t0.elapsed().as_nanos() as u64;
        super::RECORD_NS.fetch_add(record, std::sync::atomic::Ordering::Relaxed);
        super::LAST_RECORD_NS.store(record, std::sync::atomic::Ordering::Relaxed);
        super::LAST_WAIT_NS.store(0, std::sync::atomic::Ordering::Relaxed);
        let mut all = std::mem::take(&mut self.chunks);
        all.extend_from_slice(&cbs);
        let cbs = all;
        self.cb = vk::CommandBuffer::null();
        let occl_pool = std::mem::replace(&mut self.occl.pool, vk::QueryPool::null());
        let timestamps = std::mem::replace(&mut self.timestamps, vk::QueryPool::null());
        let reports = std::mem::take(&mut self.occl.reports);
        super::inflight::watch(super::inflight::Done {
            serial,
            fence: if self.lost { vk::Fence::null() } else { fence },
            occl_pool,
            queries: self.occl.next,
            occl: std::mem::take(&mut self.occl.events),
            writebacks: std::mem::take(&mut self.writebacks).into_iter().map(|(a, p, n)| (a, p as usize, n)).collect(),
            timestamps,
            pass_labels: if self.profile_passes { std::mem::take(&mut self.pass_labels) } else { Vec::new() },
            rt_writebacks: std::mem::take(&mut self.rt_reads),
        });
        if self.lost && fence != vk::Fence::null() {
            self.dev.device.destroy_fence(fence, None);
        }
        self.occl.next = 0;
        self.inflight.push_back(InFlight {
            serial,
            pool: self.pool,
            cbs,
            desc_pools: std::mem::take(&mut self.desc_pools),
            chunks: self.arena.take_used(),
            occl_pool,
            timestamps,
            imports: std::mem::take(&mut self.imports.retired),
            reports,
            resettle: std::mem::take(&mut self.rt_resettle),
            staging: std::mem::take(&mut self.staging),
        });
        self.gpu_dirty.clear();
        // Guest memory under GPU-written images is CPU memory: what it holds now is what later CPU writes change.
        self.tex.settle();
        self.repacked.clear();
        self.bindings.clear();
        self.max_indices.clear();
        self.lists.clear();
        self.vb_memo.clear();
        self.set_memo.clear();
        self.supports.clear();
        self.restart_memo.clear();
        self.ubo_copies.clear();
    }

    /// The pipeline cache.
    pub(super) unsafe fn pipeline_cache(&mut self) -> vk::PipelineCache {
        *self.pipeline_cache.get_or_insert_with(|| super::stage_cache::open_pipeline_cache(&self.dev))
    }

    /// Frees what completed submits used.
    unsafe fn retire(&mut self) {
        // The pipeline cache to disk now and then (the process has no clean exit to save it at).
        let frame = super::VULKAN.frames.load(std::sync::atomic::Ordering::Relaxed);
        let total = super::PIPELINES_TOTAL.load(std::sync::atomic::Ordering::Relaxed);
        if total > self.saved_pipelines && frame >= self.pipeline_cache_saved + 1800 {
            if let Some(c) = self.pipeline_cache {
                super::stage_cache::save_pipeline_cache(&self.dev, c);
            }
            self.saved_pipelines = total;
            self.pipeline_cache_saved = frame;
        }
        // Repacked streams no draw used for 600 frames (no submit in flight reads them).
        let frame = super::VULKAN.frames.load(std::sync::atomic::Ordering::Relaxed);
        if frame >= self.repack_swept + 600 {
            self.repack_swept = frame;
            let dev = &self.dev;
            self.repack_cache.retain(|_, e| {
                let keep = e.used + 600 > frame;
                if !keep {
                    dev.free_host_buffer(&e.buf);
                }
                keep
            });
        }
        let done = super::inflight::completed();
        while self.inflight.front().is_some_and(|f| f.serial <= done) {
            let f = self.inflight.pop_front().unwrap();
            let d = &self.dev.device;
            // Freeing MoltenVK command buffers releases every recorded command: done on another thread.
            let _ = self.pool_tx.send((f.pool, f.cbs));
            for p in f.desc_pools {
                if d.reset_descriptor_pool(p, vk::DescriptorPoolResetFlags::empty()).is_ok() {
                    self.free_desc_pools.push(p);
                } else {
                    d.destroy_descriptor_pool(p, None);
                }
            }
            self.arena.give_back(&self.dev, f.chunks);
            if f.occl_pool != vk::QueryPool::null() {
                self.free_occl.push(f.occl_pool);
            }
            if f.timestamps != vk::QueryPool::null() {
                self.free_timestamps.push(f.timestamps);
            }
            // The image's own contents: not a CPU write.
            for d in &f.resettle {
                self.tex.resettle(d);
            }
            for b in &f.staging {
                self.dev.free_host_buffer(b);
            }
            for (b, m) in f.imports {
                d.destroy_buffer(b, None);
                d.free_memory(m, None);
            }
        }
    }

    /// Whether `addr` (or the report after it, which conditions compare it with) is an occlusion report not yet written.
    fn report_pending(&self, addr: u64) -> bool {
        let hit = |r: &u64| *r == addr || *r == addr + 16;
        let done = super::inflight::completed();
        self.occl.reports.iter().any(hit) || self.inflight.iter().any(|f| f.serial > done && f.reports.iter().any(hit))
    }

    /// Records a color target this submit renders: a write-back candidate unless it was rendered in the previous 4 frames.
    fn note_rendered(&mut self, t: &NvnTexture) {
        let f = super::VULKAN.frames.load(std::sync::atomic::Ordering::Relaxed) + 1;
        let prev = self.rt_history.insert(t.storage, f);
        if prev.is_none_or(|p| p + 4 < f) && !self.writeback_rts.iter().any(|w| w.storage == t.storage) {
            self.writeback_rts.push(*t);
        }
        if self.rt_history.len() > 1 << 14 {
            self.rt_history.retain(|_, &mut v| v + 600 > f);
        }
    }

    /// At a present: this frame's write-back candidates are read by the next submit (when this frame's GPU work is
    /// done: one frame later, as the Metal backend's write-back), and written to guest memory when it completed.
    pub fn write_back_rts(&mut self) {
        let done = std::mem::take(&mut self.writeback_rts);
        self.pending_rt_reads.extend(done);
    }

    /// Records the copies of the pending write-back candidates (2D single-sample uncompressed color, every level) into
    /// host memory, at the start of the submit.
    unsafe fn record_rt_reads(&mut self) {
        let pending = std::mem::take(&mut self.pending_rt_reads);
        if pending.is_empty() {
            return;
        }
        self.dev.full_barrier(self.cb);
        for t in pending {
            let Some(info) = crate::nvn::layout::format_info(t.format) else { continue };
            if info.block_w != 1 || t.target != crate::nvn::layout::NVN_TARGET_2D || t.samples > 1 || t.storage < 0x10000 {
                continue;
            }
            // Only memory still inside a live memory pool: freed texture memory may hold game data by now.
            let len = crate::nvn::layout::compute_layout(&texture::Desc::of(&t).params).total as usize;
            if crate::nvn::api::containing_pool(t.storage, len).is_none() {
                continue;
            }
            let (d, at_level, at_layer) = self.tex.locate(&t);
            let Some(img) = self.tex.get_desc(&self.dev, &d) else { continue };
            if format::is_depth(img.format) {
                continue;
            }
            for level in 0..t.levels.max(1) as u32 {
                let Some((mut region, _, _, n)) = Textures::read_region(img, t.format, level + at_level, at_layer) else { break };
                let Some((buf, off, ptr)) = self.arena.alloc(&self.dev, n as usize) else { break };
                region.buffer_offset = off;
                self.dev.device.cmd_copy_image_to_buffer(self.cb, img.image, vk::ImageLayout::GENERAL, buf, &[region]);
                self.rt_reads.push((t, level, ptr as usize, n as usize));
            }
            // The watcher writes it: the write guard must not hold these pages.
            crate::vulkan::guard::note_write(t.storage, len);
            self.rt_resettle.push(d);
        }
        self.dev.full_barrier(self.cb);
    }

    /// Shows the presented texture in the window, if there is one.
    pub unsafe fn present_window(&mut self, texture: usize, crop: [i32; 4]) {
        if self.lost || crate::settings::get().display.headless {
            return;
        }
        if self.window.is_none() {
            #[cfg(target_os = "macos")]
            let layer = crate::host::display::layer_ptr();
            #[cfg(not(target_os = "macos"))]
            let layer = 0;
            if layer == 0 || self.window_tried {
                return;
            }
            self.window_tried = true;
            self.window = super::window::Window::new(&self.dev, layer);
            if self.window.is_none() {
                println!("[vulkan] cannot present to the window");
                return;
            }
        }
        let Some((_, t)) = crate::nvn::api::texture_snapshot(texture) else { return };
        let Some((img, level, layer)) = self.tex.get(&self.dev, &t) else { return };
        let (image, size) = (img.image, ((img.extent.width >> level).max(1), (img.extent.height >> level).max(1)));
        if let Some(w) = self.window.as_mut() {
            w.present(&self.dev, image, level, layer, size, crop);
        }
    }

    /// The occlusion counter resets and reports of the lists, by command index (as run_list counts them), and the guest
    /// memory their commands write (inline uniform updates, reports).
    unsafe fn scan_counters(
        handle: u64,
        depth: u32,
        index: &mut u64,
        out: &mut Vec<(u64, bool)>,
        writes: &mut Ranges,
        cache: &mut rustc_hash::FxHashMap<usize, (std::sync::Arc<crate::nvn::api::RecordedCommandList>, ListScan)>,
        lists: &mut rustc_hash::FxHashMap<u64, std::sync::Arc<crate::nvn::api::RecordedCommandList>>,
    ) {
        let list = match lists.get(&handle) {
            Some(l) => l.clone(),
            None => {
                let Some(l) = crate::nvn::api::command_list(handle, depth > 0) else { return };
                lists.insert(handle, l.clone());
                l
            }
        };
        // Lists without calls (most: TotK records each mesh's list once) are scanned once.
        if list.summary().1.is_empty() {
            let key = std::sync::Arc::as_ptr(&list) as usize;
            let scan = &cache.entry(key).or_insert_with(|| (list.clone(), ListScan::of(&list))).1;
            out.extend(scan.events.iter().map(|&(i, report)| (*index + i, report)));
            for &(addr, len) in &scan.writes {
                let a = crate::nvn::api::translate_virtual(addr as usize);
                writes.add(a, a + len);
            }
            *index += scan.ops;
            return;
        }
        for op in &list.ops {
            match &op.typed {
                Some(Cmd::CallCommands(hs)) if depth < 16 => {
                    for &h in hs.iter() {
                        Self::scan_counters(h, depth + 1, index, out, writes, cache, lists);
                    }
                    continue;
                }
                Some(c) => {
                    if let Some((report, write)) = ListScan::classify(c) {
                        if let Some(r) = report {
                            out.push((*index, r));
                        }
                        if let Some((addr, len)) = write {
                            let a = crate::nvn::api::translate_virtual(addr as usize);
                            writes.add(a, a + len);
                        }
                    }
                }
                None => {}
            }
            *index += 1;
        }
    }

    unsafe fn run_list(&mut self, handle: u64, depth: u32) {
        // Fetched by the pre-scan.
        let list = match self.lists.get(&handle) {
            Some(l) => l.clone(),
            None => match crate::nvn::api::command_list(handle, depth > 0) {
                Some(l) => l,
                None => return,
            },
        };
        for op in &list.ops {
            match &op.typed {
                Some(Cmd::CallCommands(hs)) if depth < 16 => {
                    for &h in hs.iter() {
                        self.run_list(h, depth + 1);
                    }
                    continue;
                }
                Some(c) => self.exec(c),
                None => {
                    let name = op.name;
                    if self.unimplemented.insert(name) {
                        self.warn(format!("{name} is not implemented"));
                    }
                }
            }
            self.op_index += 1;
        }
    }

    unsafe fn exec(&mut self, c: &Cmd) {
        match c {
            Cmd::SetRenderTargets { colors, depth } => {
                self.end_pass();
                self.st.colors = colors.clone();
                self.st.depth = *depth;
            }
            Cmd::ClearColor { index, rgba, mask } => self.clear_color(*index, *rgba, *mask),
            Cmd::ClearDepthStencil { depth, depth_mask, stencil, stencil_mask } => {
                self.clear_depth(*depth, *depth_mask, *stencil, *stencil_mask)
            }
            Cmd::SetViewport { x, y, w, h } => self.st.viewport = (*x, *y, *w, *h),
            Cmd::SetScissor { x, y, w, h } => self.st.scissor = Some((*x, *y, *w, *h)),
            Cmd::SetDepthRange { near, far } => self.st.depth_range = (*near, *far),
            Cmd::SetStencilValue { kind, faces, value } => {
                for f in 0..2 {
                    if faces & (1 << f) != 0 {
                        self.st.stencil[*kind as usize % 3][f] = *value as u32;
                    }
                }
            }
            Cmd::SetDepthClamp(e) => self.st.depth_clamp = *e,
            Cmd::SetPrimitiveRestart { enable, index } => self.st.prim_restart = enable.then_some(*index),
            Cmd::SetPolygonOffset { factor, units, clamp } => self.st.polygon_offset = (*factor, *units, *clamp),
            Cmd::SetBlendColor(c) => self.st.blend_color = *c,
            Cmd::BindProgram { program, stages } => {
                // Compute and graphics programs are bound independently.
                if stages & 0x20 != 0 {
                    self.st.compute_program = *program;
                }
                if stages & 0x1F != 0 || *stages == 0 {
                    self.st.program = *program;
                }
            }
            Cmd::DispatchCompute { x, y, z } => self.dispatch(*x, *y, *z),
            Cmd::CopyBufferToBuffer { .. }
            | Cmd::ClearBuffer { .. }
            | Cmd::CopyBufferToTexture { .. }
            | Cmd::CopyTextureToBuffer { .. }
            | Cmd::CopyTextureToTexture(_)
            | Cmd::ClearTexture(_) => self.exec_copy(c),
            Cmd::BindVertexBuffer { index, addr, size } => {
                if let Some(v) = self.st.vertex_buffers.get_mut(*index as usize) {
                    *v = (*addr, *size);
                }
            }
            Cmd::BindVertexAttribState(a) => {
                self.st.attribs = *a;
                self.vl_cache = None;
            }
            Cmd::BindVertexStreamState(s) => {
                self.vl_cache = None;
                self.st.streams.clear();
                self.st.streams.extend_from_slice(s);
            }
            Cmd::BindBlendState(b) => {
                let target = (b & 0xF) as usize;
                if target < 8 {
                    self.st.blend[target] = *b;
                }
            }
            Cmd::BindColorState(c) => self.st.color_state = *c,
            Cmd::BindChannelMaskState(m) => self.st.channel_mask = Some(*m),
            Cmd::BindDepthStencilState(d) => self.st.depth_stencil = Some(*d),
            Cmd::BindPolygonState(p) => self.st.polygon = *p,
            Cmd::BindMultisampleState(m) => self.st.multisample = *m,
            Cmd::Barrier(bits) => {
                self.end_pass();
                self.dev.full_barrier(self.cb);
                // INVALIDATE_TEXTURE: textures read after it see what was rendered before it.
                if bits & 0x10 != 0 {
                    self.fb_epoch += 1;
                }
            }
            Cmd::BindTexture { stage, index, handle } => {
                self.st.textures.insert((*stage, *index), *handle);
            }
            Cmd::BindImage { stage, index, handle } => {
                self.st.images.insert((*stage, *index), *handle);
            }
            Cmd::BindTexturesDeferred { stage, first, count, handles } => {
                for i in 0..(*count).clamp(0, 32) {
                    let h = if *handles > 0x10000 { std::ptr::read_unaligned((*handles as *const u64).add(i as usize)) } else { 0 };
                    self.st.textures.insert((*stage, *first + i), h);
                }
            }
            Cmd::SetRenderEnable(e) => self.st.render_disabled = !*e,
            Cmd::SetRenderEnableConditional { mode, counters } => {
                let a = crate::nvn::api::translate_virtual(*counters as usize);
                if self.report_pending(*counters) {
                    self.st.render_disabled = false;
                } else if a > 0x10000 {
                    let (x, y) = (std::ptr::read_unaligned(a as *const u64), std::ptr::read_unaligned((a + 16) as *const u64));
                    self.st.render_disabled = if *mode == 0 { x != y } else { x == y };
                }
            }
            Cmd::BindUniformBuffer { stage, index, addr, size } => {
                self.st.uniforms.insert((*stage, *index), (*addr, *size));
            }
            Cmd::BindStorageBuffer { stage, index, addr, size } => {
                self.st.storage.insert((*stage, *index), (*addr, *size));
            }
            Cmd::UpdateUniformBuffer { addr, offset, data } => {
                // In stream order: draws recorded before it took their copies already.
                let a = crate::nvn::api::translate_virtual((*addr as i64 + *offset as i64) as usize);
                self.ubo_epoch += 1;
                if a > 0x10000 {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), a as *mut u8, data.len());
                }
            }
            Cmd::DrawArrays { mode, first, count, .. } => self.draw(*mode, Draw::Arrays { first: *first, count: *count }, 0, 1),
            Cmd::DrawArraysInstanced { mode, first, count, base_instance, instances, .. } => {
                self.draw(*mode, Draw::Arrays { first: *first, count: *count }, *base_instance, *instances)
            }
            Cmd::DrawElements { mode, index_type, count, index_addr, base_vertex, .. } => self.draw(
                *mode,
                Draw::Elements { index_type: *index_type, count: *count, addr: *index_addr, base_vertex: *base_vertex },
                0,
                1,
            ),
            Cmd::DrawElementsInstanced { mode, index_type, count, index_addr, base_vertex, base_instance, instances, .. } => {
                self.draw(
                    *mode,
                    Draw::Elements { index_type: *index_type, count: *count, addr: *index_addr, base_vertex: *base_vertex },
                    *base_instance,
                    *instances,
                )
            }
            Cmd::ReportValue { value, addr } => {
                self.ubo_epoch += 1;
                write_report(*addr, *value as u64, true)
            }
            Cmd::ReportCounter { counter, addr } => {
                // SAMPLES_PASSED (1): occlusion queries. TIMESTAMP (0) reports the time, TIMESTAMP_TOP (0x10) zero (games
                // reset the markers of conditional rendering with it). ZCULL_STATS (0xF) has no timestamp.
                if *counter == 1 {
                    self.occlusion_report(*addr);
                } else {
                    let v = if *counter == 0 { crate::clock::now_ns() } else { 0 };
                    self.ubo_epoch += 1;
                    write_report(*addr, v, *counter != 0xF);
                }
            }
            Cmd::ResetCounter { counter: 1 } => self.occlusion_reset(),
            Cmd::SetPointSize(s) => self.st.point_size = *s,
            Cmd::ResetCounter { .. } | Cmd::SetDepthBounds { .. } => {}
            other => {
                // Once per kind (formatting the command for every occurrence was a cost per frame).
                if self.unimplemented_kinds.insert(std::mem::discriminant(other)) {
                    let name = format!("{other:?}");
                    let name = name.split([' ', '(', '{']).next().unwrap_or("").to_string();
                    self.warn(format!("command {name} is not implemented"));
                }
            }
        }
    }

    /// The image and NVN texture a render target reference names now.
    unsafe fn target(&mut self, r: &RtRef) -> Option<(NvnTexture, vk::ImageView, vk::Format, (u32, u32), vk::SampleCountFlags, vk::Image)> {
        let addr = crate::nvn::api::resolve_texture_ref(r.texture, r.gen);
        let (_, t) = crate::nvn::api::texture_snapshot(addr)?;
        if r.view_format != 0 {
            if let Some(p) = self.bc_proxy(&t, r) {
                return Some(p);
            }
        }
        let (img, at_level, at_layer) = self.tex.get(&self.dev, &t)?;
        img.mark_gpu_write();
        let fmt = if r.view_format != 0 && format::view_compatible(t.format, r.view_format as i32) {
            texture::device_format(&self.dev, r.view_format as i32)
                .filter(|f| format::aspects(*f) == format::aspects(img.format))
                .unwrap_or(img.format)
        } else {
            img.format
        };
        // A 3D texture renders into one depth slice (the reference's layer) through a 2D view of its 2D_ARRAY_COMPATIBLE
        // image; the slice counts as the view's layer.
        let (level, layer, ty) = (r.level as u32 + at_level, r.layer as u32 + at_layer, vk::ImageViewType::TYPE_2D);
        let extent = ((img.extent.width >> level).max(1), (img.extent.height >> level).max(1));
        let samples = img.samples;
        let image = img.image;
        let view = Textures::view(&self.dev, img, ty, fmt, level, 1, layer, 1, [2, 3, 4, 5], false)?;
        Some((t, view, fmt, extent, samples, image))
    }

    unsafe fn begin_pass(&mut self) -> bool {
        if self.pass.is_some() {
            return true;
        }
        let mut colors = Vec::new();
        let mut nvn_colors = [(0u32, 0u32); 8];
        let mut nvn_depth = (0u32, 0u32);
        let mut images = Vec::new();
        let mut extent = (u32::MAX, u32::MAX);
        let mut samples = vk::SampleCountFlags::TYPE_1;
        for r in self.st.colors.clone() {
            if r.texture == 0 {
                colors.push(None);
                continue;
            }
            match self.target(&r) {
                Some((t, view, fmt, e, s, image)) => {
                    self.note_rendered(&t);
                    extent = (extent.0.min(e.0), extent.1.min(e.1));
                    samples = s;
                    images.push(image);
                    // The format rendered: the view format where the attachment uses it (an integer view of a
                    // compressed texture's proxy, a reinterpreting view); the fragment stage's outputs follow it.
                    let rendered = if r.view_format != 0 && texture::device_format(&self.dev, r.view_format as i32) == Some(fmt) {
                        r.view_format as i32
                    } else {
                        t.format
                    };
                    if colors.len() < 8 {
                        nvn_colors[colors.len()] = (t.format as u32, r.view_format as u32);
                    }
                    colors.push(Some((view, fmt, rendered)));
                }
                None => colors.push(None),
            }
        }
        let depth = match self.st.depth {
            Some(r) if r.texture != 0 => self.target(&r).map(|(t, view, fmt, e, s, image)| {
                nvn_depth = (t.format as u32, r.view_format as u32);
                extent = (extent.0.min(e.0), extent.1.min(e.1));
                samples = s;
                images.push(image);
                (view, fmt)
            }),
            _ => None,
        };
        if extent.0 == u32::MAX {
            if self.st.colors.is_empty() && self.st.depth.is_none() {
                return false;
            }
            // Only targets that could not be made: the draws go nowhere.
            return false;
        }
        // Unbound slots after the last target are left out (pipelines for the same targets then match).
        while colors.last().is_some_and(|c| c.is_none()) {
            colors.pop();
        }
        let n = colors.len();
        self.pass = Some(Pass {
            images,
            colors,
            depth,
            nvn_colors,
            nvn_depth,
            extent,
            samples,
            begun: false,
            color_clears: vec![None; n],
            depth_clear: None,
            draws: 0,
        });
        true
    }

    unsafe fn begin_query(&mut self) {
        let o = &mut self.occl;
        if o.pool == vk::QueryPool::null() || o.active.is_some() || self.pass.as_ref().is_none_or(|p| !p.begun) {
            return;
        }
        // Samples count only toward a report: none are needed when the counter is reset before the next report.
        while o.cursor < o.upcoming.len() && o.upcoming[o.cursor].0 <= self.op_index {
            o.cursor += 1;
        }
        if o.upcoming.get(o.cursor).is_some_and(|&(_, report)| !report) {
            return;
        }
        if o.next >= Occlusion::QUERIES {
            if self.warned.insert("occlusion queries".into()) {
                println!("[vulkan] more than {} render passes in one submit: later ones are not counted", Occlusion::QUERIES);
            }
            return;
        }
        let q = o.next;
        o.next += 1;
        let flags = if o.precise { vk::QueryControlFlags::PRECISE } else { vk::QueryControlFlags::empty() };
        self.dev.device.cmd_begin_query(self.cb, o.pool, q, flags);
        o.active = Some(q);
    }

    unsafe fn end_query(&mut self) {
        if let Some(q) = self.occl.active.take() {
            self.dev.device.cmd_end_query(self.cb, self.occl.pool, q);
            self.occl.events.push(super::inflight::Occl::Query(q));
        }
    }

    /// ResetCounter(SAMPLES_PASSED): counting starts again from zero.
    unsafe fn occlusion_reset(&mut self) {
        if let Some(q) = self.occl.active.take() {
            self.dev.device.cmd_end_query(self.cb, self.occl.pool, q);
        }
        self.occl.events.push(super::inflight::Occl::Reset);
        self.begin_query();
    }

    /// ReportCounter(SAMPLES_PASSED): the samples counted since the reset, written once the submit completed.
    unsafe fn occlusion_report(&mut self, addr: u64) {
        self.end_query();
        self.occl.events.push(super::inflight::Occl::Report(addr));
        super::guard::note_write(crate::nvn::api::translate_virtual(addr as usize), 16);
        self.occl.reports.push(addr);
        self.begin_query();
    }

    /// Begins rendering for the current pass if it has not begun, with its pending clears as load operations.
    pub(super) unsafe fn ensure_begun(&mut self) {
        let Some(p) = self.pass.as_mut() else { return };
        if p.begun {
            return;
        }
        p.begun = true;
        self.dynamic_memo = None;
        let ca: Vec<vk::RenderingAttachmentInfo> = p
            .colors
            .iter()
            .zip(&p.color_clears)
            .map(|(c, clear)| match c {
                Some((v, _, _)) => {
                    let a = vk::RenderingAttachmentInfo::default()
                        .image_view(*v)
                        .image_layout(vk::ImageLayout::GENERAL)
                        .store_op(vk::AttachmentStoreOp::STORE);
                    match clear {
                        Some(value) => a.load_op(vk::AttachmentLoadOp::CLEAR).clear_value(vk::ClearValue { color: *value }),
                        None => a.load_op(vk::AttachmentLoadOp::LOAD),
                    }
                }
                None => vk::RenderingAttachmentInfo::default(),
            })
            .collect();
        let att = |aspect: vk::ImageAspectFlags| {
            p.depth.map(|(v, _)| {
                let a = vk::RenderingAttachmentInfo::default()
                    .image_view(v)
                    .image_layout(vk::ImageLayout::GENERAL)
                    .store_op(vk::AttachmentStoreOp::STORE);
                match p.depth_clear {
                    Some((mask, value)) if mask.contains(aspect) => {
                        a.load_op(vk::AttachmentLoadOp::CLEAR).clear_value(vk::ClearValue { depth_stencil: value })
                    }
                    _ => a.load_op(vk::AttachmentLoadOp::LOAD),
                }
            })
        };
        let (dd, ds) = (att(vk::ImageAspectFlags::DEPTH), att(vk::ImageAspectFlags::STENCIL));
        let has_stencil = p.depth.is_some_and(|(_, f)| format::aspects(f).contains(vk::ImageAspectFlags::STENCIL));
        let has_depth = p.depth.is_some_and(|(_, f)| format::aspects(f).contains(vk::ImageAspectFlags::DEPTH));
        let extent = p.extent;
        let mut info = vk::RenderingInfo::default()
            .render_area(vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: vk::Extent2D { width: extent.0, height: extent.1 } })
            .layer_count(1)
            .color_attachments(&ca);
        if has_depth {
            if let Some(d) = dd.as_ref() {
                info = info.depth_attachment(d);
            }
        }
        if has_stencil {
            if let Some(d) = ds.as_ref() {
                info = info.stencil_attachment(d);
            }
        }
        self.dev.full_barrier(self.cb);
        self.dev.device.cmd_begin_rendering(self.cb, &info);
        self.begin_query();
    }

    unsafe fn end_pass(&mut self) {
        // A pass with only clears still performs them.
        self.ensure_begun();
        self.end_query();
        if let Some(p) = self.pass.take() {
            self.dev.device.cmd_end_rendering(self.cb);
            if self.profile_passes && self.timestamps != vk::QueryPool::null() && self.pass_labels.len() < PASS_TIMESTAMPS as usize {
                let target = match p.colors.iter().flatten().next() {
                    Some((_, f, _)) => format!("{f:?}"),
                    None => p.depth.map(|(_, f)| format!("depth {f:?}")).unwrap_or_default(),
                };
                self.dev.device.cmd_write_timestamp(self.cb, vk::PipelineStageFlags::BOTTOM_OF_PIPE, self.timestamps, 2 + self.pass_labels.len() as u32);
                self.pass_labels.push(format!("{}x{} {target}, {} draws", p.extent.0, p.extent.1, p.draws));
            }
            self.dev.full_barrier(self.cb);
            self.resolve_bc_proxies();
            if chunk().is_some_and(|c| self.chunk_start.elapsed() > c) {
                self.submit_chunk();
            }
        }
    }

    /// Hands the command buffer recorded so far to the queue and continues in a new one.
    /// `cb` with the uploads recorded while it was (queued first: what it draws with), as one batch for the queue.
    unsafe fn with_uploads(&mut self, cb: vk::CommandBuffer) -> smallvec::SmallVec<[vk::CommandBuffer; 2]> {
        let (upload, staging) = self.tex.take_batch(&self.dev);
        self.staging.extend(staging);
        upload.into_iter().chain(std::iter::once(cb)).collect()
    }

    unsafe fn submit_chunk(&mut self) {
        let d = &self.dev.device;
        let Ok(next) =
            d.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(self.pool).command_buffer_count(1))
        else {
            return;
        };
        let _ = d.end_command_buffer(self.cb);
        let cbs = self.with_uploads(self.cb);
        let d = &self.dev.device;
        if let Err(e) = d.queue_submit(self.dev.queue, &[vk::SubmitInfo::default().command_buffers(&cbs)], vk::Fence::null()) {
            println!("[vulkan] vkQueueSubmit: {e}");
            self.lost = true;
        }
        self.chunks.extend_from_slice(&cbs);
        self.cb = next[0];
        let _ = d.begin_command_buffer(self.cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT));
        self.chunk_start = std::time::Instant::now();
    }

    /// The proxy a render target that names a block-compressed texture through a view format of one block per texel
    /// renders into (None for other targets).
    unsafe fn bc_proxy(&mut self, t: &NvnTexture, r: &RtRef) -> Option<(NvnTexture, vk::ImageView, vk::Format, (u32, u32), vk::SampleCountFlags, vk::Image)> {
        let info = crate::nvn::layout::format_info(t.format)?;
        let view = crate::nvn::layout::format_info(r.view_format as i32)?;
        if info.block_w <= 1 || view.block_w != 1 || view.bytes != info.bytes {
            return None;
        }
        let fmt = texture::device_format(&self.dev, r.view_format as i32)?;
        let (d, at_level, _) = self.tex.locate(t);
        let level = r.level as u32 + at_level;
        let key = (d, level, fmt);
        if !self.bc_proxies.contains_key(&key) {
            let mut pd = d;
            pd.storage = 0;
            pd.params.width = ((d.params.width >> level).max(1) as u32).div_ceil(info.block_w) as i32;
            pd.params.height = ((d.params.height >> level).max(1) as u32).div_ceil(info.block_h) as i32;
            pd.params.depth = if d.params.target == crate::nvn::layout::NVN_TARGET_3D { (d.params.depth >> level).max(1) } else { d.params.depth };
            pd.params.levels = 1;
            pd.params.format = r.view_format as i32;
            pd.params.flags = 0;
            pd.params.stride = 0;
            let p = self.tex.create(&self.dev, &pd)?;
            println!(
                "[vulkan] rendering compressed format {} ({}x{}, level {level}) through a format {} proxy (frame {})",
                t.format,
                t.width,
                t.height,
                r.view_format,
                super::VULKAN.frames.load(std::sync::atomic::Ordering::Relaxed) + 1
            );
            self.bc_proxies.insert(key, p);
        }
        if !self.bc_pending.contains(&key) {
            self.bc_pending.push(key);
        }
        if let Some(img) = self.tex.get_desc(&self.dev, &d) {
            img.mark_gpu_write();
        }
        let p = self.bc_proxies.get_mut(&key).unwrap();
        let (extent, samples, image) = ((p.extent.width, p.extent.height), p.samples, p.image);
        let v = Textures::view(&self.dev, p, vk::ImageViewType::TYPE_2D, fmt, 0, 1, r.layer as u32, 1, [2, 3, 4, 5], false)?;
        Some((*t, v, fmt, extent, samples, image))
    }

    /// Copies the proxies the pass that just ended rendered into their compressed textures: each proxy texel is one
    /// block, so the bytes go through a buffer unchanged.
    unsafe fn resolve_bc_proxies(&mut self) {
        for key in std::mem::take(&mut self.bc_pending) {
            let (d, level, _) = key;
            let Some(info) = crate::nvn::layout::format_info(d.params.format) else { continue };
            let Some(p) = self.bc_proxies.get(&key) else { continue };
            let Some(target) = self.tex.images.get(&d) else { continue };
            let (bw, bh, depth, layers) = (p.extent.width, p.extent.height, p.extent.depth, p.layers);
            let bytes = (bw * bh * depth * layers) as usize * info.bytes as usize;
            let (pimg, timg) = (p.image, target.image);
            let (tw, th) = ((target.extent.width >> level).max(1), (target.extent.height >> level).max(1));
            let td = if target.extent.depth > 1 { (target.extent.depth >> level).max(1) } else { 1 };
            let Some((buf, off, _)) = self.arena.alloc(&self.dev, bytes) else { continue };
            let sub = |level| vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).mip_level(level).layer_count(layers);
            let out = vk::BufferImageCopy::default()
                .buffer_offset(off)
                .image_subresource(sub(0))
                .image_extent(vk::Extent3D { width: bw, height: bh, depth });
            self.dev.device.cmd_copy_image_to_buffer(self.cb, pimg, vk::ImageLayout::GENERAL, buf, &[out]);
            self.dev.full_barrier(self.cb);
            let back = vk::BufferImageCopy::default()
                .buffer_offset(off)
                .buffer_row_length(bw * info.block_w)
                .buffer_image_height(bh * info.block_h)
                .image_subresource(sub(level))
                .image_extent(vk::Extent3D { width: tw, height: th, depth: td.min(depth) });
            self.dev.device.cmd_copy_buffer_to_image(self.cb, buf, timg, vk::ImageLayout::GENERAL, &[back]);
            self.dev.full_barrier(self.cb);
        }
    }

    unsafe fn clear_color(&mut self, index: i32, rgba: [f32; 4], mask: i32) {
        if self.st.render_disabled || !self.begin_pass() {
            return;
        }
        let p = self.pass.as_ref().unwrap();
        let Some(Some((_, _, nvn_fmt))) = p.colors.get(index as usize) else { return };
        let (nvn_fmt, (w, h)) = (*nvn_fmt, p.extent);
        if mask & 0xF != 0xF {
            self.warn(format!("partial color clears (mask {mask:#x}) are cleared whole"));
        }
        let value = match format::kind(nvn_fmt) {
            format::Kind::Float => vk::ClearColorValue { float32: rgba },
            format::Kind::Uint => vk::ClearColorValue { uint32: rgba.map(|v| v as u32) },
            format::Kind::Sint => vk::ClearColorValue { int32: rgba.map(|v| v as i32) },
        };
        let Some((rect, whole)) = self.clear_rect(w, h) else { return };
        let p = self.pass.as_mut().unwrap();
        if !p.begun && whole && mask & 0xF == 0xF {
            p.color_clears[index as usize] = Some(value);
            return;
        }
        self.ensure_begun();
        let a = [vk::ClearAttachment::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .color_attachment(index as u32)
            .clear_value(vk::ClearValue { color: value })];
        let r = [vk::ClearRect { rect, base_array_layer: 0, layer_count: 1 }];
        self.dev.device.cmd_clear_attachments(self.cb, &a, &r);
    }

    unsafe fn clear_depth(&mut self, depth: f32, depth_mask: bool, stencil: i32, stencil_mask: i32) {
        if self.st.render_disabled || !self.begin_pass() {
            return;
        }
        let p = self.pass.as_ref().unwrap();
        let Some((_, fmt)) = p.depth else { return };
        let mut aspect = vk::ImageAspectFlags::empty();
        let has = format::aspects(fmt);
        if depth_mask && has.contains(vk::ImageAspectFlags::DEPTH) {
            aspect |= vk::ImageAspectFlags::DEPTH;
        }
        if stencil_mask != 0 && has.contains(vk::ImageAspectFlags::STENCIL) {
            aspect |= vk::ImageAspectFlags::STENCIL;
        }
        if aspect.is_empty() {
            return;
        }
        let (w, h) = p.extent;
        let value = vk::ClearDepthStencilValue { depth, stencil: stencil as u32 };
        let Some((rect, whole)) = self.clear_rect(w, h) else { return };
        let p = self.pass.as_mut().unwrap();
        if !p.begun && whole {
            // Depth and stencil cleared now and then (separate calls) merge; each aspect keeps its latest value.
            let (old_mask, old) = p.depth_clear.unwrap_or((vk::ImageAspectFlags::empty(), value));
            let merged = vk::ClearDepthStencilValue {
                depth: if aspect.contains(vk::ImageAspectFlags::DEPTH) { depth } else { old.depth },
                stencil: if aspect.contains(vk::ImageAspectFlags::STENCIL) { stencil as u32 } else { old.stencil },
            };
            p.depth_clear = Some((old_mask | aspect, merged));
            return;
        }
        self.ensure_begun();
        let a = [vk::ClearAttachment::default().aspect_mask(aspect).clear_value(vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue { depth, stencil: stencil as u32 },
        })];
        let r = [vk::ClearRect { rect, base_array_layer: 0, layer_count: 1 }];
        self.dev.device.cmd_clear_attachments(self.cb, &a, &r);
    }

    /// The area a clear of a `w` x `h` pass covers: clears honor the scissor (TotK clears its Depths lit-area mask in
    /// 50x50 tiles, each with the scissor on the tile), and whether that is the whole pass. None when it is empty.
    fn clear_rect(&self, w: u32, h: u32) -> Option<(vk::Rect2D, bool)> {
        let (sx, sy, sw, sh) = self.st.scissor.unwrap_or((0, 0, w as i32, h as i32));
        let (x0, y0) = (sx.clamp(0, w as i32), sy.clamp(0, h as i32));
        let (x1, y1) = ((sx.saturating_add(sw)).clamp(0, w as i32), (sy.saturating_add(sh)).clamp(0, h as i32));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let rect = vk::Rect2D {
            offset: vk::Offset2D { x: x0, y: y0 },
            extent: vk::Extent2D { width: (x1 - x0) as u32, height: (y1 - y0) as u32 },
        };
        Some((rect, (x0, y0, x1, y1) == (0, 0, w as i32, h as i32)))
    }

    pub unsafe fn read_texture(&mut self, texture: usize, gen: u32, level: u32, layer: u32) -> Option<Texels> {
        let addr = crate::nvn::api::resolve_texture_ref(texture, gen);
        let (_, t) = crate::nvn::api::texture_snapshot(addr)?;
        let (img, at_level, at_layer) = self.tex.get(&self.dev, &t)?;
        Textures::read(&self.dev, img, t.format, level + at_level, layer + at_layer)
    }

    pub unsafe fn gpu_only_textures(&mut self, f: &mut dyn FnMut(GpuOnlyTexture)) -> usize {
        let mut skipped = 0;
        for (d, img) in &self.tex.images {
            if !img.gpu_written {
                continue;
            }
            let is3d = img.extent.depth > 1;
            for level in 0..img.levels {
                let n = if is3d { (img.extent.depth >> level).max(1) } else { img.layers };
                for layer in 0..n {
                    match Textures::read(&self.dev, img, d.params.format, level, layer) {
                        Some(texels) => f(GpuOnlyTexture { storage: d.storage, params: d.params, level, layer, texels, last_written: 0 }),
                        None => skipped += 1,
                    }
                }
            }
        }
        skipped
    }
}

#[derive(Clone, Copy)]
enum Draw {
    Arrays { first: i32, count: i32 },
    Elements { index_type: i32, count: i32, addr: u64, base_vertex: i32 },
}


unsafe fn new_timestamp_pool(dev: &Dev, supported: bool) -> vk::QueryPool {
    if !supported {
        return vk::QueryPool::null();
    }
    dev.device
        .create_query_pool(&vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::TIMESTAMP).query_count(2 + PASS_TIMESTAMPS), None)
        .unwrap_or(vk::QueryPool::null())
}

/// What a list without calls contributes to a submit's pre-scan (Ctx::scan_counters): its command count, its counter
/// resets (false) and reports (true) by command index in the list, and the guest memory its commands write.
pub(super) struct ListScan {
    ops: u64,
    events: Vec<(u64, bool)>,
    writes: Vec<(u64, usize)>,
}

impl ListScan {
    fn of(list: &crate::nvn::api::RecordedCommandList) -> ListScan {
        let mut s = ListScan { ops: list.ops.len() as u64, events: Vec::new(), writes: Vec::new() };
        for (i, op) in list.ops.iter().enumerate() {
            if let Some((report, write)) = op.typed.as_ref().and_then(Self::classify) {
                if let Some(r) = report {
                    s.events.push((i as u64, r));
                }
                if let Some(w) = write {
                    s.writes.push(w);
                }
            }
        }
        s
    }

    /// A command's occlusion counter event and the guest memory it writes (untranslated address, length).
    fn classify(c: &Cmd) -> Option<(Option<bool>, Option<(u64, usize)>)> {
        match c {
            Cmd::ResetCounter { counter: 1 } => Some((Some(false), None)),
            Cmd::ReportCounter { counter, addr } => Some(((*counter == 1).then_some(true), Some((*addr, 16)))),
            Cmd::ReportValue { addr, .. } => Some((None, Some((*addr, 16)))),
            Cmd::UpdateUniformBuffer { addr, offset, data } => Some((None, Some(((*addr as i64 + *offset as i64) as u64, data.len())))),
            _ => None,
        }
    }
}
