// Render and compute pipelines of translated stages: asynchronous compiles (slots, queues, waits), precompiling the
// pipeline list, recording new pipelines, pipeline keys and descriptors.

use super::*;

pub(crate) fn enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| crate::settings::get().graphics.translated_shaders)
}

/// Asynchronous pipeline compilation (on by default; --async-pipelines=0 builds synchronously). A render pipeline missing from the cache is
/// built on a pipeline-compile thread; draws wait for it within a per-frame budget of --async-wait-us (default 2000), so pipelines that
/// Metal's own shader cache serves (~0.3 ms) still draw at once, and is skipped while a cold compile (13-50 ms) runs: a
/// brief pop-in the first time something is seen instead of a hitch. --log-hitch lists the skips.
pub(crate) fn async_pipelines() -> bool {
    crate::host::overlay::is_async_shaders()
}

pub(crate) struct PipeJob {
    pub(crate) vs: CachedStage,
    pub(crate) fs: Option<CachedStage>,
    pub(crate) attrs: Vec<(u32, MTLVertexFormat, usize, usize, i32)>,
    pub(crate) streams_used: [bool; MAX_STREAMS],
    pub(crate) streams: Vec<u64>,
    pub(crate) color_pfs: Vec<MTLPixelFormat>,
    pub(crate) depth_pf: MTLPixelFormat,
    pub(crate) depth_gl: bool,
    pub(crate) point_size: f32,
    pub(crate) blends: [u64; 8],
    pub(crate) color_state: u32,
    pub(crate) channel_mask: u32,
    pub(crate) blend_color: [f32; 4],
    /// Build the batched variant (metal/batch.rs: tb_<entry> functions).
    pub(crate) batched: bool,
    /// Build the argument-buffer variant (ab_<entry> functions, --arg-buffers).
    pub(crate) args: bool,
}

pub(crate) enum AsyncPipe {
    Ready(Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>),
    Pending,
    Failed,
}

pub(crate) enum SlotState {
    /// Queued.
    Pending,
    /// Being built (on a pipeline thread or a draw's thread).
    Running,
    Ready(Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>),
    Failed,
}

pub(crate) struct Slot(Mutex<SlotState>, std::sync::Condvar);

// Metal pipeline states and the job's stage descriptions are thread-safe to hand between threads.
unsafe impl Send for Slot {}

unsafe impl Sync for Slot {}

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

pub(crate) struct SendJob(PipeJob, std::sync::Arc<Slot>);

unsafe impl Send for SendJob {}

/// Async-compiled pipelines by key (pending and finished), and pipelines skipped this frame (counted for --log-hitch).
pub(crate) static ASYNC_SLOTS: OnceLock<Mutex<HashMap<Vec<u64>, std::sync::Arc<Slot>>>> = OnceLock::new();

pub(crate) static ASYNC_SKIPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) static ASYNC_STARTED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Microseconds draws of the current frame waited for compiles (reset at each present).
pub(crate) static ASYNC_WAITED_US: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Pipeline thread work: pipelines draws wait for, then the pipeline list's (pipeline_list::precompile).
pub(crate) type ComputePipe = Retained<ProtocolObject<dyn objc2_metal::MTLComputePipelineState>>;

pub(crate) type ComputeCell = std::sync::Arc<OnceLock<Result<ComputePipe, String>>>;

/// The compute pipeline of a kernel, shared by dispatches and the precompile: whoever comes first makes it, the other
/// waits for it.
pub(crate) fn compute_cell(entry: &str) -> ComputeCell {
    static M: OnceLock<Mutex<HashMap<String, ComputeCell>>> = OnceLock::new();
    M.get_or_init(Default::default).lock().unwrap().entry(entry.to_string()).or_default().clone()
}

/// Queues every compute kernel of the shader cache on the pipeline threads, ahead of the listed render pipelines: they
/// need no draw state, and the game dispatches many while it loads (else a first launch compiles them in the middle of
/// loading the world).
pub(crate) fn queue_compute() {
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

#[derive(Default)]
pub(crate) struct PipeQueue {
    pub(crate) draws: std::collections::VecDeque<SendJob>,
    /// Compute kernels to make (queue_compute), before the listed render pipelines.
    pub(crate) compute: std::collections::VecDeque<&'static str>,
    /// Tiny-pass kernels to make (queue_tiny), counted with the compute kernels.
    pub(crate) tiny: std::collections::VecDeque<(&'static str, Vec<i64>)>,
    pub(crate) compute_n: usize,
    pub(crate) compute_done: usize,
    pub(crate) compute_ns: u64,
    pub(crate) listed: std::collections::VecDeque<SendJob>,
    /// Listed pipelines queued and built, failures, compile time (for the summary when the list is done).
    pub(crate) listed_n: usize,
    pub(crate) listed_done: usize,
    pub(crate) listed_failed: usize,
    pub(crate) listed_ns: u64,
}

pub(crate) fn pipe_queue() -> &'static (Mutex<PipeQueue>, std::sync::Condvar) {
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
pub(crate) fn build_compute(entry: &str) {
    let t0 = std::time::Instant::now();
    if let Some(g) = super::GFX.get() {
        if let Some(lib) = g.shader_lib.as_ref() {
            let _ = compute_cell(entry).get_or_init(|| g.make_compute(lib, entry));
        }
    }
    kernel_done(t0);
}

pub(crate) fn kernel_done(t0: std::time::Instant) {
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

pub(crate) fn build_job(j: &PipeJob) -> Option<Retained<ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>>> {
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

pub(crate) fn push_job(job: SendJob, listed: bool) {
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
pub(crate) fn promote(slot: &std::sync::Arc<Slot>) {
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
pub(crate) fn compile_listed(entries: Vec<crate::gpu::pipeline_list::Entry>) {
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

pub(crate) fn queue_precompile(e: &crate::gpu::pipeline_list::Entry) -> bool {
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
pub(crate) fn build_now(
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
pub(crate) fn async_pipeline_nowait(key: &[u64], job: Option<PipeJob>) -> AsyncPipe {
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
pub(crate) fn async_pipeline(key: &[u64], job: PipeJob) -> AsyncPipe {
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
    r
}

/// Waits for the pipeline async_pipeline started for `key` to be made or to fail, however long it takes: for a draw into
/// targets not drawn every frame (State::enc_recur), which a skip could leave undrawn for good.
pub(crate) fn async_pipeline_wait(key: &[u64]) -> AsyncPipe {
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
    r
}

/// Adds a draw's pipeline to the pipeline list.
pub(crate) fn record_pipeline(
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
pub(crate) fn target_pf(tex: u32, view: u32) -> MTLPixelFormat {
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

/// The key of the render pipeline build_translated_pipeline makes from these inputs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pipeline_key(
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
/// The render pipeline descriptor of a translated draw (shaders, function constants, vertex layout, targets, blending);
/// `y_negate`: the window origin the game uses (NVN's lower-left origin flips y).
pub(crate) fn pipeline_descriptor(
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
pub(crate) const DEFAULT_BLEND_STATE: u64 =
    (2 << 4) | (1 << 10) | (2 << 16) | (1 << 22) | (1 << 28) | (1u64 << 32);
