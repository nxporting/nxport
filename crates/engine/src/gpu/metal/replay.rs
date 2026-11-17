// --replay: re-executes a presented frame's submits (timings, A/B of guarded changes on the same frame), and
// --log-replay: how many draws repeat the previous frame's (the bound for reusing translated draws).

use super::*;

/// This frame's submits (their command list handles), for --replay.
pub(super) static FRAME_SUBMITS: Mutex<Vec<Vec<u64>>> = Mutex::new(Vec::new());
/// Set while a replay runs: nvnSyncWait holds the game's main thread so the frame's memory stays as it was.
pub(super) static REPLAY_HOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// --replay-ab: replay repetitions alternate this flag (A = false, B = true) and report both sides, so a change
/// guarded by `replay_b()` is measured on the very same frame. Always false outside such a replay.
pub(super) static REPLAY_B: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[inline]
pub fn replay_b() -> bool {
    REPLAY_B.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn replay_hold() -> bool {
    REPLAY_HOLD.load(std::sync::atomic::Ordering::Acquire)
}

/// --replay=F:K[:M]: at presented frame F (and every M frames after it) the frame's submits are executed K more times
/// before the present, while the game's main thread waits in nvnSyncWait. Each repetition prints the encode time (CPU,
/// this thread) and the drain time (until the GPU has finished it): identical work, so their minimum / median compare
/// builds and runtime toggles without run-to-run scene noise. Side effects: the frame renders K extra times (counters
/// and occlusion results are written again).
pub(super) fn replay_spec() -> Option<(u64, u32, u64)> {
    crate::settings::get()
        .replay
        .replay
        .map(|r| (r.frame, r.reps, r.every))
}

pub(super) fn replay_frame(g: &Gfx, frame: u64, reps: u32, submits: &[Vec<u64>], presented: usize) {
    let ab = crate::settings::get().replay.replay_ab;
    let mut enc = [Vec::new(), Vec::new()];
    let mut drain = [Vec::new(), Vec::new()];
    let mut cpu = [Vec::new(), Vec::new()];
    let mut gpu = [Vec::new(), Vec::new()];
    let mut threads: [Vec<[u64; 3]>; 2] = [Vec::new(), Vec::new()];
    let mut passes: [Vec<[u64; 6]>; 2] = [Vec::new(), Vec::new()];
    // --replay-diff=N: compare each repetition's presented image with the previous one of the same side. The frame's
    // inputs are frozen, so differences beyond GPU-particle noise are engine races; repetitions with more than N
    // differing pixels (default 2000) are dumped with their predecessor.
    let diff_limit: Option<usize> = crate::settings::get()
        .replay
        .replay_diff
        .map(|n| n as usize);
    let mut prev_img: [Option<Vec<u8>>; 2] = [None, None];
    let mut diff_counts: Vec<usize> = Vec::new();
    for i in 0..reps * if ab { 2 } else { 1 } {
        let side = (ab && i % 2 == 1) as usize;
        REPLAY_B.store(side == 1, std::sync::atomic::Ordering::Relaxed);
        while retire_oldest(g, true) {}
        let busy0 = GPU_BUSY_US.load(std::sync::atomic::Ordering::Relaxed);
        let th0 = crate::host::thread_stats::engine_cpu_us();
        let cnt = || {
            [
                ISSUED_PASSES[0].load(std::sync::atomic::Ordering::Relaxed),
                ISSUED_PASSES[1].load(std::sync::atomic::Ordering::Relaxed),
                TINY_COMPUTE.load(std::sync::atomic::Ordering::Relaxed),
                CLEAR_PASSES.load(std::sync::atomic::Ordering::Relaxed),
                batch::STATS[0].load(std::sync::atomic::Ordering::Relaxed),
                batch::STATS[1].load(std::sync::atomic::Ordering::Relaxed),
            ]
        };
        let n0 = cnt();
        let ph0: [u64; 8] =
            std::array::from_fn(|k| PHASES[k].load(std::sync::atomic::Ordering::Relaxed));
        let pq0: [u64; 5] =
            std::array::from_fn(|k| PHASES2[k].load(std::sync::atomic::Ordering::Relaxed));
        let t0 = std::time::Instant::now();
        let c0 = thread_cpu_ns();
        for handles in submits {
            submit_inner(handles);
        }
        let t1 = std::time::Instant::now();
        cpu[side].push((thread_cpu_ns() - c0) as f64 / 1e6);
        // Drain: a marker command buffer committed after everything the issuer queued.
        let marker = {
            let st = g.state.lock().unwrap();
            let Some(m) = g.queue.commandBuffer() else {
                return;
            };
            st.issuer.commit(m.clone());
            st.issuer.sync();
            m
        };
        marker.waitUntilCompleted();
        // GPU busy time of this repetition: its command buffers are complete, retiring them adds their GPU intervals.
        while retire_oldest(g, true) {}
        gpu[side].push(
            GPU_BUSY_US
                .load(std::sync::atomic::Ordering::Relaxed)
                .saturating_sub(busy0) as f64
                / 1e3,
        );
        let th1 = crate::host::thread_stats::engine_cpu_us();
        threads[side].push([0, 1, 2].map(|k| th1[k].saturating_sub(th0[k])));
        let n1 = cnt();
        passes[side].push([0, 1, 2, 3, 4, 5].map(|k| n1[k] - n0[k]));
        if crate::settings::get().log.log_phases && i + 1 == reps * if ab { 2 } else { 1 } {
            let ph: [u64; 8] = std::array::from_fn(|k| {
                PHASES[k].load(std::sync::atomic::Ordering::Relaxed) - ph0[k]
            });
            let us = |a: u64, n: u64| a as f64 / n.max(1) as f64 / 1e3;
            println!("[phases] frame {}: plan: old-list draws {} x {:.2} us = {:.2} ms, this frame's {} x {:.2} us = {:.2} ms; encode: old {} x {:.2} us = {:.2} ms, this frame's {} x {:.2} us = {:.2} ms",
                frame, ph[1], us(ph[0], ph[1]), ph[0] as f64 / 1e6, ph[3], us(ph[2], ph[3]), ph[2] as f64 / 1e6,
                ph[5], us(ph[4], ph[5]), ph[4] as f64 / 1e6, ph[7], us(ph[6], ph[7]), ph[6] as f64 / 1e6);
            let pq: [u64; 5] = std::array::from_fn(|k| {
                PHASES2[k].load(std::sync::atomic::Ordering::Relaxed) - pq0[k]
            });
            println!("[phases] frame {}: encode sections per frame (ms): pipeline + layout {:.2}, textures + raster state {:.2}, vertex buffers {:.2}, per-stage resources {:.2} (then the draw)",
                frame, pq[4] as f64 / 1e6, pq[0] as f64 / 1e6, pq[1] as f64 / 1e6, pq[2] as f64 / 1e6);
        }
        if let Some(limit) = diff_limit {
            let mut st = g.state.lock().unwrap();
            if let Some(t) = unsafe { g.texture(&mut st.tc, presented) } {
                let (w, h) = (t.width(), t.height());
                let mut raw = vec![0u8; w * h * 4];
                let region = MTLRegion {
                    origin: MTLOrigin { x: 0, y: 0, z: 0 },
                    size: MTLSize {
                        width: w,
                        height: h,
                        depth: 1,
                    },
                };
                unsafe {
                    t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                        std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                        w * 4,
                        region,
                        0,
                    )
                };
                if let Some(prev) = &prev_img[side] {
                    let (mut n, mut x0, mut y0, mut x1, mut y1) = (0usize, w, h, 0, 0);
                    for (k, (a, b)) in raw.chunks_exact(4).zip(prev.chunks_exact(4)).enumerate() {
                        if (0..3).any(|c| a[c].abs_diff(b[c]) > 8) {
                            n += 1;
                            let (x, y) = (k % w, k / w);
                            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                        }
                    }
                    diff_counts.push(n);
                    if n > limit {
                        println!("[replay-diff] frame {} rep {} side {}: {} pixels differ from the previous repetition, bbox ({},{})-({},{})",
                            frame, i, ["A", "B"][side], n, x0, y0, x1, y1);
                        dump_ppm(&t, &format!("/tmp/nxport_replay_{}_r{}.ppm", frame, i));
                        let _ = std::fs::write(
                            format!("/tmp/nxport_replay_{}_r{}_prev.raw", frame, i),
                            prev,
                        );
                    }
                }
                prev_img[side] = Some(raw);
            }
        }
        // --replay-dump: the image each side rendered last (same frame and memory: a deterministic A/B of output).
        // --replay-dump=2: also side A's repetition before (_a2): the same frame's noise floor between two runs of one path.
        let total = reps * if ab { 2 } else { 1 };
        let last = i + 1 >= total - if ab { 1 } else { 0 };
        let dump = crate::settings::get().replay.replay_dump;
        let second_last_a = dump == 2 && ab && i + 4 == total && side == 0;
        if (last || second_last_a) && dump > 0 {
            let mut st = g.state.lock().unwrap();
            if let Some(t) = unsafe { g.texture(&mut st.tc, presented) } {
                let tag = if second_last_a && !last {
                    "a2"
                } else {
                    ["a", "b"][side]
                };
                let path = format!("/tmp/nxport_replay_{}_{}.ppm", frame, tag);
                dump_ppm(&t, &path);
                println!(
                    "[replay] frame {} side {} dumped to {}",
                    frame,
                    ["A", "B"][side],
                    path
                );
                // --replay-dump-tex=WxH[,WxH]: also the raw level-0 contents of every cached texture of those sizes
                // (uncompressed color formats of 4 or 8 bytes per pixel), per side, to locate which target differs.
                let want = &crate::settings::get().replay.replay_dump_tex;
                if !want.is_empty() {
                    let near: Vec<String> = st
                        .tc
                        .textures
                        .values()
                        .filter(|tx| want.iter().any(|w| w.w as usize == tx.width()))
                        .map(|tx| {
                            format!(
                                "{}x{} {:?} {:?} {:?}",
                                tx.width(),
                                tx.height(),
                                tx.pixelFormat(),
                                tx.textureType(),
                                objc2_metal::MTLResource::storageMode(&**tx)
                            )
                        })
                        .collect();
                    println!("[replay] --replay-dump-tex: {} cached textures, of the wanted widths: {:?}", st.tc.textures.len(), near);
                    for (&addr, tx) in &st.tc.textures {
                        if !want.iter().any(|w| w.is(tx.width(), tx.height()))
                            || objc2_metal::MTLResource::storageMode(&**tx)
                                == objc2_metal::MTLStorageMode::Private
                        {
                            continue;
                        }
                        let bpp = match tx.pixelFormat() {
                            MTLPixelFormat::RGBA16Float
                            | MTLPixelFormat::RGBA16Unorm
                            | MTLPixelFormat::RG32Float => 8,
                            MTLPixelFormat::RGBA8Unorm
                            | MTLPixelFormat::RGBA8Unorm_sRGB
                            | MTLPixelFormat::BGRA8Unorm
                            | MTLPixelFormat::RGB10A2Unorm
                            | MTLPixelFormat::RG11B10Float
                            | MTLPixelFormat::R32Float
                            | MTLPixelFormat::RG16Float
                            | MTLPixelFormat::RG16Unorm => 4,
                            _ => continue,
                        };
                        let (w, h) = (tx.width(), tx.height());
                        let mut raw = vec![0u8; w * h * bpp];
                        let region = MTLRegion {
                            origin: MTLOrigin { x: 0, y: 0, z: 0 },
                            size: MTLSize {
                                width: w,
                                height: h,
                                depth: 1,
                            },
                        };
                        unsafe {
                            tx.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                                std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                                w * bpp,
                                region,
                                0,
                            )
                        };
                        let _ = std::fs::write(
                            format!(
                                "/tmp/nxport_replay_tex_{}_{:x}_{}x{}_{:?}.raw",
                                tag,
                                addr,
                                w,
                                h,
                                tx.pixelFormat()
                            ),
                            &raw,
                        );
                    }
                }
            }
        }
        enc[side].push(t1.duration_since(t0).as_secs_f64() * 1e3);
        drain[side].push(t1.elapsed().as_secs_f64() * 1e3);
    }
    REPLAY_B.store(false, std::sync::atomic::Ordering::Relaxed);
    if !diff_counts.is_empty() {
        let mut d = diff_counts.clone();
        d.sort_unstable();
        println!("[replay-diff] frame {}: differing pixels per repetition: median {} p90 {} max {} ({} of {} above the limit)", frame,
            d[d.len() / 2], d[d.len() * 9 / 10], d[d.len() - 1], d.iter().filter(|&&n| n > diff_limit.unwrap_or(0)).count(), d.len());
    }
    let stat = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (v[0], v[v.len() / 2])
    };
    for side in 0..if ab { 2 } else { 1 } {
        let (emin, emed) = stat(&mut enc[side]);
        let (dmin, dmed) = stat(&mut drain[side]);
        let (_, cmed) = stat(&mut cpu[side]);
        let (gmin, gmed) = stat(&mut gpu[side]);
        // Median CPU per repetition of each engine thread.
        let tmed: Vec<String> = (0..3)
            .map(|k| {
                let mut v: Vec<u64> = threads[side].iter().map(|t| t[k]).collect();
                v.sort_unstable();
                format!(
                    "{:.2}",
                    v.get(v.len() / 2).copied().unwrap_or(0) as f64 / 1e3
                )
            })
            .collect();
        println!("[replay] frame {}{}: thread CPU median ms per repetition: Presentation {}, gfx-encoder {}, gfx-issuer {}",
            frame, if ab { ["  A", "  B"][side] } else { "" }, tmed[0], tmed[1], tmed[2]);
        let pmed: Vec<u64> = (0..6)
            .map(|k| {
                let mut v: Vec<u64> = passes[side].iter().map(|t| t[k]).collect();
                v.sort_unstable();
                v.get(v.len() / 2).copied().unwrap_or(0)
            })
            .collect();
        println!("[replay] frame {}{}: per repetition (median): issuer render passes {} (clear passes {}), compute passes {}, tiny compute draws {}, batched draws {} of {} draws",
            frame, if ab { ["  A", "  B"][side] } else { "" }, pmed[0], pmed[3], pmed[1], pmed[2], pmed[4], pmed[5]);
        println!("[replay] frame {}{} x{} ({} submits): encode min {:.2} median {:.2} ms (CPU median {:.2}), drain (GPU after encode) min {:.2} median {:.2} ms, GPU busy min {:.2} median {:.2} ms",
            frame, if ab { ["  A", "  B"][side] } else { "" }, reps, submits.len(), emin, emed, cmed, dmin, dmed, gmin, gmed);
    }
}

/// --log-replay: per 100 frames, how many draws come from command lists recorded in an earlier frame, and how
/// many have the same input state (bindings) and the same uniform contents as the draw at the same list position in
/// the previous frame: the upper bound for caching translated draws across frames.
pub(super) fn replay_log_enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| crate::settings::get().log.log_replay)
}

#[derive(Default)]
pub(super) struct ReplayStats {
    prev: HashMap<(u64, usize), [u64; 8]>,
    prev2: HashMap<(u64, usize), [u64; 8]>,
    cur: HashMap<(u64, usize), [u64; 8]>,
    comp_same: [u64; 8],
    seen2: u64,
    frame: u64,
    draws: u64,
    seen_before: u64,
    start: u64,
}

pub(super) fn replay_note(
    st: &mut State,
    handle: u64,
    op_index: usize,
    _list_old: bool,
    op: &crate::nvn::api::RecordedNvnCommand,
) {
    use std::hash::{Hash, Hasher};
    const N: usize = 8;
    const NAMES: [&str; N] = [
        "program",
        "vertex layout",
        "vertex buffers",
        "uniform slots+sizes",
        "uniform addresses",
        "texture handles",
        "targets",
        "fixed-function+draw args",
    ];
    let f = presented_frames();
    if st.replay.frame != f {
        let r = &mut st.replay;
        r.prev2 = std::mem::take(&mut r.prev);
        r.prev = std::mem::take(&mut r.cur);
        r.frame = f;
        if f >= r.start + 100 && r.draws > 0 {
            let pct = |x: u64| x as f64 * 100.0 / r.draws as f64;
            let comps: Vec<String> = (0..N)
                .map(|k| format!("{} {:.0}%", NAMES[k], pct(r.comp_same[k])))
                .collect();
            println!("[replay] frames {}..{}: {:.0} draws/frame; same list position as 1 frame ago {:.1}% / 2 ago {:.1}%; of all draws, same as 2 frames ago: {}",
                r.start, f, r.draws as f64 / (f - r.start) as f64, pct(r.seen_before), pct(r.seen2), comps.join(", "));
            *r = ReplayStats {
                prev: std::mem::take(&mut r.prev),
                prev2: std::mem::take(&mut r.prev2),
                frame: f,
                start: f,
                ..Default::default()
            };
        }
    }
    let hash = |x: &dyn Fn(&mut rustc_hash::FxHasher)| {
        let mut h = rustc_hash::FxHasher::default();
        x(&mut h);
        h.finish()
    };
    let mut vb: Vec<_> = st.vertex_buffers.iter().map(|(k, v)| (*k, *v)).collect();
    vb.sort_unstable();
    let mut ub: Vec<_> = st.uniform_buffers.iter().map(|(k, v)| (k, *v)).collect();
    ub.sort_unstable();
    let mut tb: Vec<_> = st.textures_bound.iter().map(|(k, v)| (k, *v)).collect();
    tb.sort_unstable();
    let comps: [u64; N] = [
        hash(&|h| st.program.hash(h)),
        hash(&|h| {
            st.vertex_attribs.as_slice().hash(h);
            st.vertex_streams.hash(h);
        }),
        hash(&|h| vb.hash(h)),
        hash(&|h| ub.iter().for_each(|(k, (_, sz))| (*k, *sz).hash(h))),
        hash(&|h| ub.iter().for_each(|(_, (a, _))| a.hash(h))),
        hash(&|h| tb.hash(h)),
        hash(&|h| {
            for c in &st.colors {
                c.texture.hash(h);
            }
            st.depth.map(|d| d.texture).hash(h);
        }),
        hash(&|h| {
            st.blend_states.hash(h);
            (
                st.depth_stencil,
                st.polygon,
                st.viewport,
                st.scissor,
                st.color_state,
                st.channel_mask,
                st.prim_restart,
            )
                .hash(h);
            format!("{:?}", op.typed).hash(h);
        }),
    ];
    let r = &mut st.replay;
    r.draws += 1;
    if let Some(p) = r.prev2.get(&(handle, op_index)) {
        r.seen2 += 1;
        for k in 0..N {
            if p[k] == comps[k] {
                r.comp_same[k] += 1;
            }
        }
    }
    if r.prev.contains_key(&(handle, op_index)) {
        r.seen_before += 1;
    }
    r.cur.insert((handle, op_index), comps);
}
