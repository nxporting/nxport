// Batched instanced draws: TotK issues runs of consecutive draws that
// differ only in their uniform buffers (413 boxes per Hateno frame with one 256-byte block each, shadow casters, ...).
// The Metal stage of the issuer draws such a run as one instanced draw of the pipeline's batched variant, whose stages
// (crates/cli/src/batch_kernels.rs, `tb_<entry>`) read every constant buffer through a per-instance table of GPU
// addresses. Metal keeps primitive order across the instances of a draw, so the result is that of the separate draws.
//
// Detection needs no knowledge of the draws: translated::EncCache sends only state that changed, so between two draws
// of a run the issuer sees nothing but re-binds of the stages' constant buffers and the identical draw op. The batcher
// holds such ops back while a run lasts; any other op ends the run. A run of one draw is replayed as it came; a longer
// run becomes the batched draw, after which the bindings the encoding side believes are bound are restored.
//
// Off by default: --batch turns it on (needs a cache with tb_ variants); --replay-ab-batch: off on replay side B.
// 2026-10-17, M4, Hateno phone configuration, same-frame A/B: 1,862 draws in 420 batches, image at the noise floor, GPU
// busy unchanged (8.97 vs 8.93 ms) and gfx-issuer +0.6 ms (pipeline switch and re-binds around each batch): no gain on
// the Mac. Untested on the iPhone, whose GPU is ~3x slower.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLBuffer, MTLRenderCommandEncoder, MTLRenderPipelineState, MTLRenderStages, MTLResource,
    MTLResourceUsage,
};

use super::issuer::Op;

type Buf = Retained<ProtocolObject<dyn MTLBuffer>>;
type Pipe = Retained<ProtocolObject<dyn MTLRenderPipelineState>>;
type Enc = Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>;

pub(super) static STATS: [std::sync::atomic::AtomicU64; 3] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 3]; // batches, draws in them, runs of one

pub(super) fn enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| crate::settings::get().graphics.batch)
}

/// A pipeline whose draws can be batched: its stages' table bindings and how to build the batched variant.
pub(super) struct Info {
    vtb: Vec<usize>,
    ftb: Vec<usize>,
    key: Vec<u64>,
    job: Mutex<Option<super::translated::PipeJob>>,
    batched: Mutex<Option<Pipe>>,
    failed: std::sync::atomic::AtomicBool,
}
// Pipeline states and stage descriptions are thread-safe to share.
unsafe impl Send for Info {}
unsafe impl Sync for Info {}

fn registry() -> &'static Mutex<HashMap<usize, Arc<Info>>> {
    static R: OnceLock<Mutex<HashMap<usize, Arc<Info>>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// Called when the encoding side creates `pipe` (key: its pipeline-cache key; job: what built it).
pub(super) fn register(
    pipe: &Pipe,
    vtb: &[u32],
    ftb: &[u32],
    key: &[u64],
    job: super::translated::PipeJob,
) {
    let mut k = key.to_vec();
    k.push(0x7462_0000_0000_0001); // "tb": the batched variant's own async-compile slot
    let info = Info {
        vtb: vtb.iter().map(|&b| b as usize).collect(),
        ftb: ftb.iter().map(|&b| b as usize).collect(),
        key: k,
        job: Mutex::new(Some(job)),
        batched: Mutex::new(None),
        failed: std::sync::atomic::AtomicBool::new(false),
    };
    registry()
        .lock()
        .unwrap()
        .insert(&**pipe as *const _ as *const () as usize, Arc::new(info));
}

fn info_of(pipe: &Pipe) -> Option<Arc<Info>> {
    registry()
        .lock()
        .unwrap()
        .get(&(&**pipe as *const _ as *const () as usize))
        .cloned()
}

impl Info {
    /// The batched pipeline if it is built; `request`: start its compile if not done yet (never waits).
    fn batched(&self, request: bool) -> Option<Pipe> {
        if let Some(p) = self.batched.lock().unwrap().as_ref() {
            return Some(p.clone());
        }
        if self.failed.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        let job = if request {
            self.job.lock().unwrap().take()
        } else {
            None
        };
        if !request && self.job.lock().unwrap().is_some() {
            return None; // never requested
        }
        match super::translated::async_pipeline_nowait(&self.key, job) {
            super::translated::AsyncPipe::Ready(p) => {
                *self.batched.lock().unwrap() = Some(p.clone());
                Some(p)
            }
            super::translated::AsyncPipe::Pending => None,
            super::translated::AsyncPipe::Failed => {
                self.failed
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                println!("[!] Gfx: batched pipeline failed to build; its draws stay separate");
                None
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Batcher {
    pipe: Option<Pipe>,
    info: Option<Arc<Info>>,
    /// Pipeline -> batch info lookups of this thread (registration precedes a pipeline's first use).
    infos: HashMap<usize, Option<Arc<Info>>>,
    /// Buffer bindings as the encoding side sees them (including held ops).
    vb: [Option<(Buf, usize)>; 32],
    fb: [Option<(Buf, usize)>; 32],
    /// The run in progress (storage reused between runs): its draw, draws so far, the batched pipeline (from its second
    /// draw on), each draw's table bindings (vertex then fragment, in table order), and constant-buffer re-binds since
    /// its last draw that were not sent yet.
    run_draw: Option<Op>,
    run_n: usize,
    run_batched: Option<Pipe>,
    run_binds: Vec<(Buf, usize)>,
    run_held: Vec<Op>,
    /// The tracked state is complete: set at an encoder's start while batching is on (bindings are per encoder).
    valid: bool,
}

thread_local! {
    static B: std::cell::RefCell<Batcher> = std::cell::RefCell::new(Batcher::default());
}

fn same_draw(a: &Op, b: &Op) -> bool {
    match (a, b) {
        (
            Op::Draw {
                prim: p0,
                start: s0,
                count: c0,
                instances: i0,
                base_instance: b0,
            },
            Op::Draw {
                prim: p1,
                start: s1,
                count: c1,
                instances: i1,
                base_instance: b1,
            },
        ) => p0 == p1 && s0 == s1 && c0 == c1 && i0 == i1 && b0 == b1,
        (
            Op::DrawIndexed {
                prim: p0,
                count: c0,
                ty: t0,
                buf: x0,
                off: o0,
                instances: i0,
                base_vertex: v0,
                base_instance: b0,
            },
            Op::DrawIndexed {
                prim: p1,
                count: c1,
                ty: t1,
                buf: x1,
                off: o1,
                instances: i1,
                base_vertex: v1,
                base_instance: b1,
            },
        ) => {
            p0 == p1
                && c0 == c1
                && t0 == t1
                && std::ptr::eq(&**x0, &**x1)
                && o0 == o1
                && i0 == i1
                && v0 == v1
                && b0 == b1
        }
        _ => false,
    }
}

impl Batcher {
    /// Appends the current table bindings of `info`'s stages to run_binds; false (nothing appended) if one is unbound.
    fn push_binds(&mut self, info: &Info) -> bool {
        let mark = self.run_binds.len();
        for (list, vertex) in [(&info.vtb, true), (&info.ftb, false)] {
            for &i in list {
                let state = if vertex { &self.vb } else { &self.fb };
                match state.get(i).and_then(|x| x.as_ref()) {
                    Some((b, off)) => self.run_binds.push((b.clone(), *off)),
                    None => {
                        self.run_binds.truncate(mark);
                        return false;
                    }
                }
            }
        }
        true
    }

    fn is_table_binding(&self, vertex: bool, i: usize) -> bool {
        self.info.as_ref().is_some_and(|inf| {
            if vertex {
                inf.vtb.contains(&i)
            } else {
                inf.ftb.contains(&i)
            }
        })
    }

    fn clear_run(&mut self) {
        self.run_draw = None;
        self.run_n = 0;
        self.run_batched = None;
        self.run_binds.clear();
        self.run_held.clear();
    }

    /// Ends the current run: replays a run of one draw as it came, draws a longer one batched.
    unsafe fn finish(&mut self, enc: &mut Option<Enc>, out: &mut dyn FnMut(Op, &mut Option<Enc>)) {
        let Some(draw) = self.run_draw.take() else {
            return;
        };
        if self.run_n == 1 {
            // As it came: the draw with the bindings of its time, then the re-binds that followed it.
            out(draw, enc);
            for op in std::mem::take(&mut self.run_held) {
                out(op, enc);
            }
            STATS[2].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.clear_run();
            return;
        }
        let (Some(info), Some(batched)) = (self.info.clone(), self.run_batched.clone()) else {
            self.clear_run();
            return;
        };
        let Some(e) = enc.as_ref() else {
            self.clear_run();
            return;
        };
        let n = self.run_n;
        let per = info.vtb.len() + info.ftb.len();
        let base_instance = match &draw {
            Op::Draw { base_instance, .. } | Op::DrawIndexed { base_instance, .. } => {
                *base_instance as u64
            }
            _ => 0,
        };
        e.setRenderPipelineState(&batched);
        // Residency of the buffers the tables point into (each once).
        let mut seen: smallvec::SmallVec<[*const ProtocolObject<dyn MTLBuffer>; 8]> =
            smallvec::SmallVec::new();
        for (b, _) in &self.run_binds {
            let ptr = &**b as *const ProtocolObject<dyn MTLBuffer>;
            if !seen.iter().any(|&x| std::ptr::addr_eq(x, ptr)) {
                seen.push(ptr);
                e.useResource_usage_stages(
                    ProtocolObject::<dyn MTLResource>::from_ref(&**b),
                    MTLResourceUsage::Read,
                    MTLRenderStages::Vertex | MTLRenderStages::Fragment,
                );
            }
        }
        let addrs: Vec<u64> = self
            .run_binds
            .iter()
            .map(|(b, off)| b.gpuAddress() + *off as u64)
            .collect();
        // setVertexBytes / setFragmentBytes take at most 4 KB: chunks of draws.
        let max_draws = ((4096 / 8 - 1) / info.vtb.len().max(info.ftb.len()).max(1)).max(1);
        let mut first = 0;
        let mut t: Vec<u64> = Vec::with_capacity(512);
        while first < n {
            let k = (n - first).min(max_draws);
            for (list, skip, vertex) in [
                (&info.vtb, 0usize, true),
                (&info.ftb, info.vtb.len(), false),
            ] {
                if list.is_empty() {
                    continue;
                }
                t.clear();
                t.push(base_instance);
                for d in first..first + k {
                    t.extend_from_slice(&addrs[d * per + skip..d * per + skip + list.len()]);
                }
                let ptr = std::ptr::NonNull::new(t.as_ptr() as *mut libc::c_void).unwrap();
                if vertex {
                    e.setVertexBytes_length_atIndex(ptr, t.len() * 8, list[0]);
                } else {
                    e.setFragmentBytes_length_atIndex(ptr, t.len() * 8, list[0]);
                }
            }
            match &draw {
                Op::Draw { prim, start, count, .. } => e.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(*prim, *start, *count, k, 0),
                Op::DrawIndexed { prim, count, ty, buf, off, base_vertex, .. } => {
                    e.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount_baseVertex_baseInstance(*prim, *count, *ty, buf, *off, k, *base_vertex, 0)
                }
                _ => {}
            }
            first += k;
        }
        STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        STATS[1].fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        // Back to what the encoding side believes is bound: the original pipeline, the last draw's buffers.
        if let Some(p) = &self.pipe {
            e.setRenderPipelineState(p);
        }
        for &i in &info.vtb {
            if let Some((b, off)) = &self.vb[i] {
                e.setVertexBuffer_offset_atIndex(Some(b), *off, i);
            }
        }
        for &i in &info.ftb {
            if let Some((b, off)) = &self.fb[i] {
                e.setFragmentBuffer_offset_atIndex(Some(b), *off, i);
            }
        }
        self.clear_run();
    }

    /// One op in issue order; `out` executes ops this lets through.
    unsafe fn feed(
        &mut self,
        op: Op,
        enc: &mut Option<Enc>,
        out: &mut dyn FnMut(Op, &mut Option<Enc>),
    ) {
        if !enabled() {
            if self.run_draw.is_some() {
                self.finish(enc, out);
            }
            self.valid = false;
            out(op, enc);
            return;
        }
        if matches!(op, Op::Begin { .. }) {
            self.finish(enc, out);
            self.reset(true);
            out(op, enc);
            return;
        }
        if !self.valid {
            out(op, enc);
            return;
        }
        let running = self.run_draw.is_some();
        match &op {
            Op::VBuf(b, off, i) | Op::FBuf(b, off, i) if *i < 32 => {
                let vertex = matches!(op, Op::VBuf(..));
                let slot = if vertex {
                    &mut self.vb[*i]
                } else {
                    &mut self.fb[*i]
                };
                *slot = Some((b.clone(), *off));
                if running && self.is_table_binding(vertex, *i) {
                    self.run_held.push(op);
                    return;
                }
            }
            Op::VBufOff(off, i) | Op::FBufOff(off, i) if *i < 32 => {
                let vertex = matches!(op, Op::VBufOff(..));
                let slot = if vertex {
                    &mut self.vb[*i]
                } else {
                    &mut self.fb[*i]
                };
                if let Some(s) = slot.as_mut() {
                    s.1 = *off;
                }
                if running && self.is_table_binding(vertex, *i) {
                    self.run_held.push(op);
                    return;
                }
            }
            Op::Draw { instances: 1, .. } | Op::DrawIndexed { instances: 1, .. } => {
                if running {
                    if same_draw(self.run_draw.as_ref().unwrap(), &op) {
                        let info = self.info.clone().unwrap();
                        // From the second draw on the run needs the batched pipeline (requested now if never before).
                        if self.run_batched.is_none() {
                            self.run_batched = info.batched(true);
                        }
                        if self.run_batched.is_some() && self.push_binds(&info) {
                            self.run_n += 1;
                            self.run_held.clear(); // reflected in vb / fb, restored after the batched draw
                            return;
                        }
                    }
                    self.finish(enc, out);
                }
                if let Some(info) = self.info.clone() {
                    if !info.failed.load(std::sync::atomic::Ordering::Relaxed)
                        && self.push_binds(&info)
                    {
                        self.run_draw = Some(op);
                        self.run_n = 1;
                        return;
                    }
                }
                out(op, enc);
                return;
            }
            _ => {}
        }
        if running {
            self.finish(enc, out);
        }
        match &op {
            Op::Pipeline(p) => {
                let k = &**p as *const _ as *const () as usize;
                self.info = match self.infos.get(&k) {
                    Some(i) => i.clone(),
                    None => {
                        let i = info_of(p);
                        if self.infos.len() > 1 << 14 {
                            self.infos.clear();
                        }
                        self.infos.insert(k, i.clone());
                        i
                    }
                };
                self.pipe = Some(p.clone());
            }
            Op::End | Op::Commit(..) => self.reset(false),
            _ => {}
        }
        out(op, enc);
    }

    /// Forgets the per-encoder state (keeps the lookup cache and the vectors' storage).
    fn reset(&mut self, valid: bool) {
        self.clear_run();
        self.pipe = None;
        self.info = None;
        self.vb = Default::default();
        self.fb = Default::default();
        self.valid = valid;
    }
}

/// Routes one op through the executing thread's batcher (`exec`: executes an op that passes).
pub(super) unsafe fn feed(
    op: Op,
    enc: &mut Option<Enc>,
    exec: &mut dyn FnMut(Op, &mut Option<Enc>),
) {
    B.with(|b| b.borrow_mut().feed(op, enc, exec))
}

/// Ends a run in progress (the executing thread is about to go idle: the encoding thread may use the encoder next).
pub(super) unsafe fn flush(enc: &mut Option<Enc>, exec: &mut dyn FnMut(Op, &mut Option<Enc>)) {
    B.with(|b| b.borrow_mut().finish(enc, exec))
}

pub(super) fn pending() -> bool {
    B.with(|b| b.borrow().run_draw.is_some())
}
