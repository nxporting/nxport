// Render-encoder calls of translated draws, executed on a second thread.
//
// The Presentation Thread both resolves draws (NVN state -> Metal objects) and makes the Metal calls; the calls
// (encoder creation, state, draws) were ~20% of its time. Here they are queued in order and a worker thread executes
// them while the Presentation Thread resolves the next draws. Ops are batched on the encoding side without locks (only
// the encoding thread touches the batch) and handed over in chunks.
//
// Ordering contract: everything else that uses the command buffer on the encoding thread (other encoders, commits,
// presents) must call `sync()` first, which waits until every queued op has run. --no-issuer runs each op inline.

use std::cell::UnsafeCell;
use std::sync::{Arc, Condvar, Mutex};

use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBuffer, MTLCommandBuffer, MTLCommandEncoder, MTLComputeCommandEncoder,
    MTLComputePipelineState, MTLCullMode, MTLDepthClipMode, MTLDepthStencilState, MTLIndexType,
    MTLPrimitiveType, MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLRenderPipelineState,
    MTLSamplerState, MTLScissorRect, MTLSize, MTLTexture, MTLTriangleFillMode, MTLViewport,
    MTLVisibilityResultMode, MTLWinding,
};

type Buf = Retained<ProtocolObject<dyn MTLBuffer>>;
type Tex = Retained<ProtocolObject<dyn MTLTexture>>;
type Enc = Retained<ProtocolObject<dyn MTLRenderCommandEncoder>>;
type CEnc = Retained<ProtocolObject<dyn MTLComputeCommandEncoder>>;

/// sync() calls and microseconds the encoding thread waited in them (--log-timeline).
pub(super) static SYNC_STATS: [std::sync::atomic::AtomicU64; 2] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

pub(super) enum Op {
    Begin {
        cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
        desc: Retained<MTLRenderPassDescriptor>,
        label: String,
    },
    End,
    Pipeline(Retained<ProtocolObject<dyn MTLRenderPipelineState>>),
    Viewport(MTLViewport),
    Scissor(MTLScissorRect),
    Cull(MTLCullMode),
    Clip(MTLDepthClipMode),
    Winding(MTLWinding),
    Fill(MTLTriangleFillMode),
    Bias(f32, f32, f32),
    DepthStencil(Retained<ProtocolObject<dyn MTLDepthStencilState>>),
    StencilRef(u32, u32),
    Blend([f32; 4]),
    VBuf(Buf, usize, usize),
    FBuf(Buf, usize, usize),
    /// New offset for the buffer already bound at the index (translated::EncCache knows it is the same buffer).
    VBufOff(usize, usize),
    FBufOff(usize, usize),
    VTex(Tex, usize),
    FTex(Tex, usize),
    VSamp(Option<Retained<ProtocolObject<dyn MTLSamplerState>>>, usize),
    FSamp(Option<Retained<ProtocolObject<dyn MTLSamplerState>>>, usize),
    Visibility(MTLVisibilityResultMode, usize),
    Draw {
        prim: MTLPrimitiveType,
        start: usize,
        count: usize,
        instances: usize,
        base_instance: usize,
    },
    DrawIndexed {
        prim: MTLPrimitiveType,
        count: usize,
        ty: MTLIndexType,
        buf: Buf,
        off: usize,
        instances: usize,
        base_vertex: isize,
        base_instance: usize,
    },
    PushGroup(String),
    PopGroup,
    /// Commits a command buffer whose encoders were all ended by earlier ops (see `Issuer::commit`), with its in-flight
    /// serial if it has one (published as committed, see metal::COMMITTED_SERIAL).
    Commit(Retained<ProtocolObject<dyn MTLCommandBuffer>>, Option<u64>),
    /// A compute pass: begin (with its label), state, one dispatch (threadgroups, threads per group), end.
    CBegin(Retained<ProtocolObject<dyn MTLCommandBuffer>>, String),
    CPipeline(Retained<ProtocolObject<dyn MTLComputePipelineState>>),
    CBuf(Buf, usize, usize),
    CTex(Tex, usize),
    CSamp(Option<Retained<ProtocolObject<dyn MTLSamplerState>>>, usize),
    CDispatch(MTLSize, MTLSize),
    /// Tiny compute draws: continue the compute encoder a previous tiny draw left open on the same
    /// command buffer (its dispatches run in order: serial dispatch), else begin one.
    CJoin(Retained<ProtocolObject<dyn MTLCommandBuffer>>, String),
    /// Leave the compute encoder open for the next CJoin; any other op (or the issuer going idle) ends it first.
    CLinger,
    /// A blit pass (buffer / texture copies, fills): `f` encodes the commands. Queued in order so the encoding thread does
    /// not wait for the pipeline to drain before using the command buffer itself.
    Blit {
        cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
        label: &'static str,
        f: Box<dyn FnOnce(&ProtocolObject<dyn objc2_metal::MTLBlitCommandEncoder>)>,
    },
    /// A planned draw: the issuer thread runs its encode half (translated::run_draw_job) onto its current encoder.
    DrawJob(Box<super::translated::DrawPacket>),
    /// A new submit begins: the issuer thread's encode state resets its per-submit memory (arena, uniform copies).
    SubmitStart,
}

// Metal objects may be used from another thread as long as no two threads use one at the same time; the queue hands
// each op to exactly one thread.
unsafe impl Send for Op {}

thread_local! {
    /// The command buffer of a compute encoder left open by CLinger (only the thread executing ops touches it).
    static LINGER: std::cell::RefCell<Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>> = const { std::cell::RefCell::new(None) };
    /// The command buffer of the compute encoder the last CJoin continued or began.
    static JOINED: std::cell::RefCell<Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>> = const { std::cell::RefCell::new(None) };
}

/// Ends a compute encoder CLinger left open.
fn end_linger(cenc: &mut Option<CEnc>) {
    if LINGER.with(|l| l.borrow_mut().take()).is_some() {
        if let Some(e) = cenc.take() {
            e.endEncoding();
        }
    }
}

/// Executes one op on the render or compute encoder it is building.
unsafe fn exec(op: Op, enc: &mut Option<Enc>, cenc: &mut Option<CEnc>) {
    // Render-encoder ops go through the batcher (metal/batch.rs); anything else ends a run it holds first.
    let render = !matches!(
        op,
        Op::CBegin(..)
            | Op::CPipeline(_)
            | Op::CBuf(..)
            | Op::CTex(..)
            | Op::CSamp(..)
            | Op::CDispatch(..)
            | Op::CJoin(..)
            | Op::CLinger
            | Op::Blit { .. }
            | Op::DrawJob(_)
            | Op::SubmitStart
    );
    if !render && super::batch::pending() {
        super::batch::flush(enc, &mut |o, e| exec_render(o, e));
    }
    if let Op::CJoin(cb, label) = op {
        JOINED.with(|j| *j.borrow_mut() = Some(cb.clone()));
        let same = LINGER
            .with(|l| l.borrow_mut().take())
            .is_some_and(|c| std::ptr::eq(&*c, &*cb))
            && cenc.is_some();
        if !same {
            if let Some(e) = cenc.take() {
                e.endEncoding();
            }
            return exec(Op::CBegin(cb, label), enc, cenc);
        }
        return;
    }
    if let Op::CLinger = op {
        let cb = JOINED.with(|j| j.borrow_mut().take());
        if cenc.is_some() {
            LINGER.with(|l| *l.borrow_mut() = cb);
        }
        return;
    }
    end_linger(cenc);
    match op {
        Op::CBegin(cb, label) => {
            super::ISSUED_PASSES[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            *cenc = match super::pass_prof::compute_pass(&label) {
                Some(d) => cb.computeCommandEncoderWithDescriptor(&d),
                None => cb.computeCommandEncoder(),
            };
            match cenc.as_ref() {
                Some(e) => e.setLabel(Some(&NSString::from_str(&label))),
                None => println!(
                    "[!] Gfx issuer: compute encoder creation failed ({}): its dispatch is dropped",
                    label
                ),
            }
            return;
        }
        Op::CPipeline(p) => {
            if let Some(e) = cenc.as_ref() {
                e.setComputePipelineState(&p)
            }
            return;
        }
        Op::CBuf(b, off, i) => {
            if let Some(e) = cenc.as_ref() {
                e.setBuffer_offset_atIndex(Some(&b), off, i)
            }
            return;
        }
        Op::CTex(t, i) => {
            if let Some(e) = cenc.as_ref() {
                e.setTexture_atIndex(Some(&t), i)
            }
            return;
        }
        Op::CSamp(s, i) => {
            if let Some(e) = cenc.as_ref() {
                e.setSamplerState_atIndex(s.as_deref(), i)
            }
            return;
        }
        Op::CDispatch(groups, threads) => {
            if let Some(e) = cenc.as_ref() {
                e.dispatchThreadgroups_threadsPerThreadgroup(groups, threads)
            }
            return;
        }
        Op::Blit { cb, label, f } => {
            if let Some(e) = enc.take() {
                e.endEncoding(); // never expected: the encoding thread ends its render encoder first
            }
            match cb.blitCommandEncoder() {
                Some(b) => {
                    b.setLabel(Some(&NSString::from_str(label)));
                    f(&b);
                    b.endEncoding();
                }
                None => println!(
                    "[!] Gfx issuer: blit encoder creation failed ({}): the copy is dropped",
                    label
                ),
            }
            return;
        }
        op => super::batch::feed(op, enc, &mut |o, e| exec_render(o, e)),
    }
}

/// Executes one render-encoder (or commit) op.
unsafe fn exec_render(op: Op, enc: &mut Option<Enc>) {
    if let Op::Begin { cb, desc, label } = op {
        super::pass_prof::render_pass(&desc);
        super::ISSUED_PASSES[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *enc = cb.renderCommandEncoderWithDescriptor(&desc);
        if let Some(e) = enc.as_ref() {
            e.setLabel(Some(&NSString::from_str(&label)));
        } else {
            println!(
                "[!] Gfx issuer: render encoder creation failed ({}): its draws are dropped",
                label
            );
        }
        return;
    }
    if let Op::End = op {
        if let Some(e) = enc.take() {
            e.endEncoding();
        }
        return;
    }
    if let Op::Commit(cb, serial) = op {
        if let Some(e) = enc.take() {
            e.endEncoding(); // never expected: the encoding thread ends its encoders before committing
        }
        cb.commit();
        super::tail_log_commit(&cb);
        if let Some(s) = serial {
            super::note_committed(s);
        }
        return;
    }
    let Some(e) = enc.as_ref() else { return };
    match op {
        Op::Pipeline(p) => e.setRenderPipelineState(&p),
        Op::Viewport(v) => e.setViewport(v),
        Op::Scissor(r) => e.setScissorRect(r),
        Op::Cull(m) => e.setCullMode(m),
        Op::Clip(m) => e.setDepthClipMode(m),
        Op::Winding(w) => e.setFrontFacingWinding(w),
        Op::Fill(f) => e.setTriangleFillMode(f),
        Op::Bias(u, f, c) => e.setDepthBias_slopeScale_clamp(u, f, c),
        Op::DepthStencil(d) => e.setDepthStencilState(Some(&d)),
        Op::StencilRef(f, b) => e.setStencilFrontReferenceValue_backReferenceValue(f, b),
        Op::Blend(c) => e.setBlendColorRed_green_blue_alpha(c[0], c[1], c[2], c[3]),
        Op::VBuf(b, off, i) => e.setVertexBuffer_offset_atIndex(Some(&b), off, i),
        Op::FBuf(b, off, i) => e.setFragmentBuffer_offset_atIndex(Some(&b), off, i),
        Op::VBufOff(off, i) => e.setVertexBufferOffset_atIndex(off, i),
        Op::FBufOff(off, i) => e.setFragmentBufferOffset_atIndex(off, i),
        Op::VTex(t, i) => e.setVertexTexture_atIndex(Some(&t), i),
        Op::FTex(t, i) => e.setFragmentTexture_atIndex(Some(&t), i),
        Op::VSamp(s, i) => e.setVertexSamplerState_atIndex(s.as_deref(), i),
        Op::FSamp(s, i) => e.setFragmentSamplerState_atIndex(s.as_deref(), i),
        Op::Visibility(m, off) => e.setVisibilityResultMode_offset(m, off),
        // --replay-ab-null-draws: replay side B encodes every pass and its state but no draw calls (measurement:
        // what the passes' loads / stores and state cost without geometry).
        Op::Draw { .. } | Op::DrawIndexed { .. } if null_draws_b() == 2 || (super::replay_b() && null_draws_b() == 1) => {}
        Op::Draw { prim, start, count, instances, base_instance } => {
            e.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(prim, start, count, instances, base_instance)
        }
        Op::DrawIndexed { prim, count, ty, buf, off, instances, base_vertex, base_instance } => {
            e.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount_baseVertex_baseInstance(
                prim, count, ty, &buf, off, instances, base_vertex, base_instance,
            )
        }
        Op::PushGroup(s) => e.pushDebugGroup(&NSString::from_str(&s)),
        Op::PopGroup => e.popDebugGroup(),
        Op::Begin { .. } | Op::End | Op::Commit(..) => {}
        Op::CBegin(..) | Op::CPipeline(_) | Op::CBuf(..) | Op::CTex(..) | Op::CSamp(..) | Op::CDispatch(..) | Op::CJoin(..) | Op::CLinger | Op::Blit { .. } => {}
        Op::DrawJob(_) | Op::SubmitStart => {} // handled by the issuer thread's loop (never queued in inline mode)
    }
}

struct Shared {
    q: Mutex<Queue>,
    work: Condvar,
    idle: Condvar,
}

struct Queue {
    batches: Vec<Vec<Op>>,
    busy: bool,
    /// The worker waits on `work`: only then does a flush signal it (a condvar signal is a syscall).
    sleeping: bool,
    /// Emptied batch vectors handed back for reuse.
    free: Vec<Vec<Op>>,
    /// sync() is waiting: the Metal stage ends a draw run its batcher holds (metal/batch.rs) before it counts as idle.
    /// Without a sync the run waits for the next ops (the issuer catches up with the encoding side often mid-pass).
    flush_req: bool,
}

struct Inner {
    shared: Arc<Shared>,
    inline: bool,
    /// The Metal stage behind an encode stage (--encode-worker): this handle's queue feeds a thread that runs draw
    /// jobs and forwards every op, in order, to `next`, whose thread makes the Metal calls.
    next: Option<Issuer>,
    /// Encoding-thread side: ops not handed over yet (inline mode: the encoder being built).
    batch: UnsafeCell<Vec<Op>>,
    inline_enc: UnsafeCell<Option<Enc>>,
    inline_cenc: UnsafeCell<Option<CEnc>>,
}

/// Where a draw's encoder ops go: the issuer's queue (resolved on the encoding thread, executed here later), or straight
/// onto the encoder of the thread running the draw (parallel encoding: the issuer thread resolves queued draws itself).
#[derive(Clone)]
pub(super) enum Sink {
    Queue(Issuer),
    Direct(DirectEnc),
}

/// The encoders of the thread executing ops (valid while that thread runs the draw it was made for).
#[derive(Clone, Copy)]
pub(super) struct DirectEnc {
    pub(super) enc: *mut Option<Enc>,
    pub(super) cenc: *mut Option<CEnc>,
}

impl Sink {
    pub(super) fn push(&self, op: Op) {
        match self {
            Sink::Queue(i) => i.push(op),
            Sink::Direct(d) => unsafe { exec(op, &mut *d.enc, &mut *d.cenc) },
        }
    }
}

/// The encoding thread's handle. Only the encoding thread (holding the executor state lock) calls it.
#[derive(Clone)]
pub(super) struct Issuer(Arc<Inner>);
unsafe impl Send for Issuer {}
unsafe impl Sync for Issuer {}

const BATCH: usize = 96;

impl Issuer {
    pub(super) fn new() -> Self {
        let inline = crate::settings::get().graphics.no_issuer;
        let worker = crate::settings::get().graphics.encode_worker;
        let metal = Self::new_metal_stage(inline);
        if inline || !worker {
            return metal;
        }
        Self::new_encode_stage(metal)
    }

    /// The encode stage: runs queued draw jobs (their ops go into `next`'s queue) and forwards all other ops in order.
    fn new_encode_stage(next: Issuer) -> Self {
        let shared = Arc::new(Shared {
            q: Mutex::new(Queue {
                batches: Vec::new(),
                busy: false,
                sleeping: false,
                free: Vec::new(),
                flush_req: false,
            }),
            work: Condvar::new(),
            idle: Condvar::new(),
        });
        let sh = shared.clone();
        let out = next.clone();
        std::thread::Builder::new()
            .name("gfx-encoder".into())
            .spawn(move || {
                unsafe {
                    extern "C" {
                        fn pthread_set_qos_class_self_np(qos: u32, relpri: i32) -> i32;
                    }
                    pthread_set_qos_class_self_np(0x21, 0);
                }
                let mut done: Vec<Vec<Op>> = Vec::new();
                let mut worker = super::translated::WorkerCtx::default();
                let sink = Sink::Queue(out.clone());
                loop {
                    let batches = {
                        let mut g = sh.q.lock().unwrap();
                        g.free.append(&mut done);
                        if g.batches.is_empty() {
                            // Everything taken so far is forwarded before this stage counts as idle (sync waits for it).
                            drop(g);
                            out.flush();
                            g = sh.q.lock().unwrap();
                        }
                        while g.batches.is_empty() {
                            g.busy = false;
                            sh.idle.notify_all();
                            g.sleeping = true;
                            g = sh.work.wait(g).unwrap();
                            g.sleeping = false;
                        }
                        g.busy = true;
                        std::mem::take(&mut g.batches)
                    };
                    for mut b in batches {
                        autoreleasepool(|_| {
                            for op in b.drain(..) {
                                match op {
                                    Op::DrawJob(job) => unsafe {
                                        super::translated::run_draw_job(
                                            job,
                                            sink.clone(),
                                            &mut worker,
                                        )
                                    },
                                    Op::SubmitStart => worker.submit_start(),
                                    Op::Commit(cb, serial) => {
                                        // The GPU should start on it as soon as possible.
                                        out.push(Op::Commit(cb, serial));
                                        out.flush();
                                    }
                                    op => {
                                        if matches!(op, Op::Begin { .. }) {
                                            worker.encoder_begins();
                                        }
                                        out.push(op);
                                    }
                                }
                            }
                        });
                        done.push(b);
                    }
                }
            })
            .expect("spawn gfx-encoder");
        Issuer(Arc::new(Inner {
            shared,
            inline: false,
            next: Some(next),
            batch: UnsafeCell::new(Vec::with_capacity(BATCH)),
            inline_enc: UnsafeCell::new(None),
            inline_cenc: UnsafeCell::new(None),
        }))
    }

    /// The Metal stage (the gfx-issuer thread): executes ops on its encoders.
    fn new_metal_stage(inline: bool) -> Self {
        let shared = Arc::new(Shared {
            q: Mutex::new(Queue {
                batches: Vec::new(),
                busy: false,
                sleeping: false,
                free: Vec::new(),
                flush_req: false,
            }),
            work: Condvar::new(),
            idle: Condvar::new(),
        });
        if !inline {
            let sh = shared.clone();
            std::thread::Builder::new()
                .name("gfx-issuer".into())
                .spawn(move || {
                    unsafe {
                        extern "C" {
                            fn pthread_set_qos_class_self_np(qos: u32, relpri: i32) -> i32;
                        }
                        pthread_set_qos_class_self_np(0x21, 0);
                    }
                    let mut enc: Option<Enc> = None;
                    let mut cenc: Option<CEnc> = None;
                    let mut done: Vec<Vec<Op>> = Vec::new();
                    let mut worker = super::translated::WorkerCtx::default();
                    loop {
                        let batches = {
                            let mut g = sh.q.lock().unwrap();
                            g.free.append(&mut done);
                            loop {
                                if !g.batches.is_empty() {
                                    break;
                                }
                                if LINGER.with(|l| l.borrow().is_some())
                                    || (g.flush_req && super::batch::pending())
                                {
                                    // Going idle: the encoding thread may use the command buffer next (sync).
                                    drop(g);
                                    end_linger(&mut cenc);
                                    if super::batch::pending() && sh.q.lock().unwrap().flush_req {
                                        unsafe {
                                            super::batch::flush(&mut enc, &mut |o, e| {
                                                exec_render(o, e)
                                            })
                                        };
                                    }
                                    g = sh.q.lock().unwrap();
                                    continue;
                                }
                                if !super::batch::pending() {
                                    g.flush_req = false;
                                    g.busy = false;
                                    sh.idle.notify_all();
                                }
                                g.sleeping = true;
                                g = sh.work.wait(g).unwrap();
                                g.sleeping = false;
                            }
                            g.busy = true;
                            std::mem::take(&mut g.batches)
                        };
                        for mut b in batches {
                            autoreleasepool(|_| {
                                for op in b.drain(..) {
                                    match op {
                                        Op::DrawJob(job) => unsafe {
                                            super::translated::run_draw_job(
                                                job,
                                                Sink::Direct(DirectEnc {
                                                    enc: &mut enc,
                                                    cenc: &mut cenc,
                                                }),
                                                &mut worker,
                                            )
                                        },
                                        Op::SubmitStart => worker.submit_start(),
                                        op => {
                                            if matches!(op, Op::Begin { .. }) {
                                                worker.encoder_begins();
                                            }
                                            unsafe { exec(op, &mut enc, &mut cenc) }
                                        }
                                    }
                                }
                            });
                            done.push(b);
                        }
                    }
                })
                .expect("spawn gfx-issuer");
        }
        Issuer(Arc::new(Inner {
            shared,
            inline,
            next: None,
            batch: UnsafeCell::new(Vec::with_capacity(BATCH)),
            inline_enc: UnsafeCell::new(None),
            inline_cenc: UnsafeCell::new(None),
        }))
    }

    pub(super) fn push(&self, op: Op) {
        unsafe {
            if self.0.inline {
                exec(
                    op,
                    &mut *self.0.inline_enc.get(),
                    &mut *self.0.inline_cenc.get(),
                );
                return;
            }
            let b = &mut *self.0.batch.get();
            b.push(op);
            if b.len() >= BATCH {
                self.flush();
            }
        }
    }

    /// True when ops run on the calling thread (--no-issuer): no worker to hand draws to.
    pub(super) fn is_inline(&self) -> bool {
        self.0.inline
    }

    /// Hands the pending ops to the worker.
    pub(super) fn flush(&self) {
        unsafe {
            let b = &mut *self.0.batch.get();
            if b.is_empty() {
                return;
            }
            let mut g = self.0.shared.q.lock().unwrap();
            let fresh = g.free.pop().unwrap_or_else(|| Vec::with_capacity(BATCH));
            let batch = std::mem::replace(b, fresh);
            g.batches.push(batch);
            g.busy = true;
            if g.sleeping {
                self.0.shared.work.notify_one();
            }
        }
    }

    /// Commits `cb` after the queued ops, without waiting for them: the GPU starts on it as soon as the worker gets there,
    /// and the encoding thread goes on resolving draws (a sync here cost ~5% of the Presentation Thread). Retirement only
    /// polls the buffer's status, which stays uncommitted until then. --sync-commit: sync, then commit here.
    pub(super) fn commit(&self, cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>) {
        self.commit_serial(cb, None);
    }

    /// `commit` for an in-flight command buffer: its serial is published as committed once it is.
    pub(super) fn commit_serial(
        &self,
        cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
        serial: Option<u64>,
    ) {
        if self.0.inline {
            self.sync();
            cb.commit();
            if let Some(s) = serial {
                super::note_committed(s);
            }
            return;
        }
        self.push(Op::Commit(cb, serial));
        self.flush();
    }

    /// Waits until every queued op has run (before the encoding thread uses the command buffer itself).
    pub(super) fn sync(&self) {
        if self.0.inline {
            end_linger(unsafe { &mut *self.0.inline_cenc.get() });
            unsafe {
                super::batch::flush(&mut *self.0.inline_enc.get(), &mut |o, e| exec_render(o, e))
            };
            return;
        }
        self.flush();
        let t0 = std::time::Instant::now();
        let mut g = self.0.shared.q.lock().unwrap();
        if self.0.next.is_none() {
            request_flush(&self.0.shared, &mut g);
        }
        while !g.batches.is_empty() || g.busy {
            g = self.0.shared.idle.wait(g).unwrap();
        }
        drop(g);
        // An encode stage is idle only after forwarding everything: then wait for the Metal stage behind it.
        if let Some(n) = &self.0.next {
            let mut g = n.0.shared.q.lock().unwrap();
            request_flush(&n.0.shared, &mut g);
            while !g.batches.is_empty() || g.busy {
                g = n.0.shared.idle.wait(g).unwrap();
            }
        }
        SYNC_STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SYNC_STATS[1].fetch_add(
            t0.elapsed().as_micros() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

/// Asks a Metal stage to end its batcher's run before going idle (sync waits for that).
fn request_flush(sh: &Shared, g: &mut std::sync::MutexGuard<'_, Queue>) {
    if g.busy {
        g.flush_req = true;
        if g.sleeping {
            sh.work.notify_one();
        }
    }
}

/// 1: on replay side B; 2: always (pass profiles of the passes alone).
fn null_draws_b() -> u8 {
    static F: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *F.get_or_init(|| crate::settings::get().replay.replay_ab_null_draws)
}
