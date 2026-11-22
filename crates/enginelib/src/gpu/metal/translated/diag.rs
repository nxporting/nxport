// Investigation counters and probes of translated draws: batching and reuse statistics, uniform content probes,
// frame render-target roots, stale-sample journal lines and the debug vertex-position buffer.

use super::*;

/// --log-draw-reuse: is a draw in a list recorded in an earlier frame planned exactly as the previous time that
/// list op ran in the same calling context (the op's position in the parent lists)? Every 300 frames: draws in old lists,
/// how many repeat their previous plan, and which plan parts differ when not.
/// --log-batchable (with --log-draw-reuse): runs of consecutive draws in one pass that differ only in texture
/// handles, uniform and storage buffers (the same mesh drawn again with other per-draw data), which an instanced draw
/// with per-instance resources could replace.
pub(crate) fn note_batchable(st: &State, pkt: &DrawPacket) {
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

/// Registers a new pipeline with the batcher (metal/batch.rs) when its draws can be batched: both stages have batched
/// variants (or the fragment stage reads no constant buffer) and no vertex stream steps per instance (a batch's
/// instances are the run's draws).
#[allow(clippy::too_many_arguments)]
pub(crate) fn register_batchable(
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

pub(crate) fn note_draw_reuse(st: &mut State, pkt: &DrawPacket) {
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
pub(crate) unsafe fn ubo_content_probe(pkt: &DrawPacket, vs: &'static CachedStage) {
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

/// Uniform buffer copies made / reused, bytes copied, copy-cache clears by guest writes (timeline log).
pub(crate) static UBO_STATS: [std::sync::atomic::AtomicU64; 4] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 4];

/// Binding views created (cache misses) and full cache clears (timeline log).
pub(crate) static BINDING_VIEW_STATS: [std::sync::atomic::AtomicU64; 2] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Identity of the storage behind a texture: a view's parent texture, else the texture itself.
///
/// The answer is stored on the texture as an associated object (OBJC_ASSOCIATION_ASSIGN: a plain value, never retained
/// or messaged, dropped with the texture), so each texture is walked once in its life. -[IOGPUMetalTexture parentTexture]
/// can take over 100 us a call, even on root textures.
/// --no-root-tag: walk every time (A/B).
/// root_id calls answered by the tag / by walking the parent chain (timeline log).
pub(crate) static ROOT_STATS: [std::sync::atomic::AtomicU64; 2] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Frame journal: render targets drawn this frame by (guest storage, width, height) -> Metal root id, and the stale
/// samples already reported this frame. Cleared when the journal rotates.
pub(crate) static FRAME_RT_ROOTS: std::sync::Mutex<
    Option<(
        rustc_hash::FxHashMap<(usize, usize, usize), u64>,
        rustc_hash::FxHashSet<(usize, u64)>,
    )>,
> = std::sync::Mutex::new(None);

/// The previous frame's render targets (same key): a sample before this frame renders that memory reads last frame's
/// contents (temporal effects), which live in the Metal texture last frame rendered into.
pub(crate) static PREV_RT_ROOTS: std::sync::Mutex<
    Option<rustc_hash::FxHashMap<(usize, usize, usize), u64>>,
> = std::sync::Mutex::new(None);

/// --history-targets=WxH: render targets of that size drawn this frame (guest storage, texture), copied into the
/// target history ring at the present (metal::frame_history).
pub(crate) struct FrameTargets(pub(crate) Vec<(usize, Tex)>);

unsafe impl Send for FrameTargets {}

pub(crate) static FRAME_TARGETS: std::sync::Mutex<FrameTargets> =
    std::sync::Mutex::new(FrameTargets(Vec::new()));

pub(crate) fn history_target_size() -> Option<(usize, usize)> {
    crate::settings::get()
        .dump
        .history_targets
        .map(|s| (s.w as usize, s.h as usize))
}

pub(crate) fn note_frame_rt(storage: usize, t: &Tex) {
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
pub(crate) unsafe fn journal_stale_sample(handle: u64, t: &Tex) {
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

/// Debug (--debug-vpos): a 16 MB shared buffer bound at vertex buffer index 26 for every translated draw, for edited
/// vertex shaders (--debug-vs-file) that write values such as their clip-space position into it; written to
/// /tmp/nxport_vpos.bin at presented frame --debug-vpos-dump-at (default 4800).
pub(crate) struct DebugBuf(pub(crate) Retained<ProtocolObject<dyn MTLBuffer>>);

unsafe impl Send for DebugBuf {}

unsafe impl Sync for DebugBuf {}

/// Size of the --debug-vpos buffer: --debug-vpos-mb (default 16).
pub(crate) fn debug_vpos_len() -> usize {
    static L: OnceLock<usize> = OnceLock::new();
    *L.get_or_init(|| crate::settings::get().debug.debug_vpos_mb << 20)
}

pub(crate) fn debug_vpos_buffer() -> Option<&'static DebugBuf> {
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
