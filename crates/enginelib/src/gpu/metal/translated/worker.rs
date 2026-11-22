// The encode worker: draws planned on the presentation thread are encoded on gfx-encoder (packets, their pool,
// pool buffers kept alive until the command buffer completes).

use super::*;

/// What encode_draw works with: this thread's encoding state, the shared caches, the encoder labels, the
/// render encoder's op queue, and the frame's render-target list (traces). No `State`: the second half of a draw can run on another thread.
pub(crate) struct EncodeEnv<'a> {
    pub(crate) cx: &'a mut super::EncodeCtx,
    pub(crate) sh: &'a mut super::SharedCaches,
    pub(crate) lb: &'a mut super::EncLabels,
    pub(crate) enc: super::issuer::Sink,
    pub(crate) frame_rts: &'a [(u64, Tex, u32)],
    /// Draws in the current render encoder before this one (traces).
    pub(crate) enc_draws: u32,
    /// Set when the draw was issued (the caller counts it toward the pass split).
    pub(crate) drew: bool,
}

/// Zero-copy buffers over guest memory pools, keyed by wrapped range (see Gfx::pool_buffer): one map for all threads.
pub(crate) struct PoolBuffers(pub(crate) HashMap<(usize, usize), Retained<ProtocolObject<dyn MTLBuffer>>>);

// Metal buffers may be used from any thread; the map is only touched under its mutex.
unsafe impl Send for PoolBuffers {}

pub(crate) static POOL_BUFFERS: Mutex<Option<PoolBuffers>> = Mutex::new(None);

pub(crate) fn pool_buffers_len() -> usize {
    POOL_BUFFERS
        .lock()
        .unwrap()
        .as_ref()
        .map_or(0, |m| m.0.len())
}

/// The issuer thread's own encode state: its arena, uniform copies and redundant-state filter, and its own copies of the
/// shared caches (pipelines, samplers, constant banks, pool buffers): shared with the encoding thread, its compute
/// dispatches would wait for whole draws.
#[derive(Default)]
pub(crate) struct WorkerCtx {
    cx: super::EncodeCtx,
    lb: super::EncLabels,
    sh: super::SharedCaches,
}

impl WorkerCtx {
    /// Op::SubmitStart: what State's submit start resets for the encoding thread's EncodeCtx.
    pub(crate) fn submit_start(&mut self) {
        self.cx.arena.reset();
        self.cx.ubo_copies.clear();
        self.cx.support_cache.clear();
        self.cx.driver_cb_memo = [None, None];
        self.cx.index_memo = None;
        self.cx.max_index_memo.clear();
    }

    /// Op::Begin: a new render encoder starts with no state set.
    pub(crate) fn encoder_begins(&mut self) {
        self.cx.ec = EncCache::default();
    }
}

/// Packets travelling to the issuer thread and back (no allocation per draw).
pub(crate) static PACKET_POOL: Mutex<Vec<Box<DrawPacket>>> = Mutex::new(Vec::new());

pub(crate) fn get_packet() -> Box<DrawPacket> {
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

pub(crate) fn recycle_packet(pkt: Box<DrawPacket>) {
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
pub(crate) unsafe fn run_draw_job(
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
pub(crate) fn encode_on_worker(st: &State) -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().graphics.encode_worker)
        && !st.issuer.is_inline() && !super::tracing_frame() && !draw_details()
}

pub(crate) fn drop_released_pool_buffers(released: &[(usize, usize, bool)]) {
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
