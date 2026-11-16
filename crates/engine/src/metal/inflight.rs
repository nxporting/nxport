// Command buffers in flight: committing (split by draws, time or encoders), timestamps, retiring finished ones and
// the submitted / completed serials.

use super::*;

/// A committed command buffer and what is needed to retire it: its encoders' labels/details for error reports and the
/// occlusion reports it will produce.
pub(super) struct InFlight {
    pub(super) serial: u64,
    /// The nvnQueueSubmitCommands call it belongs to (a submit may be split into several command buffers).
    pub(super) group: u64,
    pub(super) cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    pub(super) labels: Vec<String>,
    pub(super) details: Vec<Vec<String>>,
    pub(super) occl: Vec<(Vec<usize>, u64)>,
    /// Reports the fence watcher writes when the command buffer completes; whichever of it and `retire_oldest` comes
    /// first writes them (taken out of the Option), so the serial is never marked complete before they are in memory.
    pub(super) occl_watched: Option<std::sync::Arc<std::sync::Mutex<Option<Vec<(Vec<usize>, u64)>>>>>,
    /// TIMESTAMP reports to rewrite with GPU times (see gpu_timestamps); taken by whichever of the watcher and
    /// retire_oldest comes first.
    pub(super) ts: std::sync::Arc<std::sync::Mutex<Option<Vec<(u64, bool)>>>>,
}

/// --gpu-timestamps: TIMESTAMP counter reports carry the time the GPU ran them, as on the Switch: the GPU start
/// time of their command buffer if no GPU work precedes them in it, else its end time (written at completion; the CPU
/// time at encode stays until then). Encode-time CPU times made TotK's per-frame GPU span (a start / end pair per frame,
/// read two frames later) our ~6-10 ms encode span, whatever the GPU took; its dynamic resolution never saw a slow GPU
/// (the iPhone: GPU ~26 ms per frame, frames just over 33 ms).
pub(super) fn gpu_timestamps() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().graphics.gpu_timestamps)
}

/// Writes TIMESTAMP reports of a completed command buffer with its GPU start / end time (nanoseconds, host clock).
pub(super) fn resolve_timestamps(cb: &ProtocolObject<dyn MTLCommandBuffer>, reports: &[(u64, bool)]) {
    let (start, end) = (cb.GPUStartTime(), cb.GPUEndTime());
    if !(end > 0.0 && start > 0.0) {
        return; // no GPU timing (error): the provisional CPU times stay
    }
    static LOG: OnceLock<bool> = OnceLock::new();
    let log = *LOG.get_or_init(|| crate::settings::get().log.log_timestamps);
    for &(addr, at_start) in reports {
        let t = if at_start { start } else { end };
        let v = (t * 1e9) as u64;
        if log {
            println!(
                "[ts-gpu] f{} addr {:#x} {} t {:.3} ms",
                presented_frames(),
                addr,
                if at_start { "start" } else { "end  " },
                v as f64 / 1e6
            );
        }
        let host = crate::nvn::api::translate_virtual(addr as usize);
        let p = if fence_guard::enabled() {
            fence_guard::engine_write_ptr(host)
        } else {
            report_guard::engine_ptr(host)
        } as *mut u64;
        unsafe {
            p.write_unaligned(v);
            p.add(1).write_unaligned(v);
        }
    }
}

/// Serial of the last committed command buffer and of the last one known complete (Metal runs a queue's command buffers
/// in order, so completion of serial n implies all earlier ones). nvnQueueFenceSync records the submitted serial and
/// nvnSyncWait waits for it.
pub(super) static SUBMITTED_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(super) static COMPLETED_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn completed_serial() -> u64 {
    COMPLETED_SERIAL.load(std::sync::atomic::Ordering::Acquire)
}

pub fn submitted_serial() -> u64 {
    SUBMITTED_SERIAL.load(std::sync::atomic::Ordering::Acquire)
}

/// Command buffers allowed on the GPU at once (--frames-in-flight, default 2, 1..=4). With one, the CPU encodes the
/// next frame only after the GPU finished the previous: frame time is CPU + GPU instead of the larger of the two.
pub fn frames_in_flight() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| crate::settings::get().graphics.frames_in_flight.clamp(1, 4))
}

/// Retires the oldest in-flight command buffer: waits for it (unless `only_if_done` and it is still running), then
/// publishes its occlusion reports and GPU time and handles errors. Returns whether one was retired.
pub(super) fn retire_oldest(g: &Gfx, only_if_done: bool) -> bool {
    let entry = {
        let mut st = g.state.lock().unwrap();
        match st.inflight.front() {
            None => return false,
            Some(e)
                if only_if_done
                    && !matches!(
                        e.cb.status(),
                        MTLCommandBufferStatus::Completed | MTLCommandBufferStatus::Error
                    ) =>
            {
                return false
            }
            _ => {}
        }
        st.inflight.pop_front().unwrap()
    };
    let prev = entry.cb.clone();
    wait_or_abort(&prev, &entry.labels);
    if let Some(list) = entry.ts.lock().unwrap().take() {
        resolve_timestamps(&prev, &list);
    }
    if journal_on() && (!entry.occl.is_empty() || entry.occl_watched.is_some()) {
        let (s0, e0) = (prev.GPUStartTime(), prev.GPUEndTime());
        journal(|| {
            format!(
                "occl-cb {} gpu {:.3}-{:.3} retired t {:.3}",
                entry.serial,
                s0 * 1e3,
                e0 * 1e3,
                host_secs() * 1e3
            )
        });
    }
    g.state.lock().unwrap().occl.resolve(entry.occl);
    // Reports of a watched command buffer: written by the fence watcher, unless it has not got to them yet (the game's
    // fence wait must not see this serial complete before they are in memory).
    if let Some(shared) = &entry.occl_watched {
        let mut w = shared.lock().unwrap();
        if let Some(reports) = w.take() {
            let buf = g.state.lock().unwrap().occl.buf.clone();
            if let Some(buf) = buf {
                resolve_occlusion(&buf, &reports);
            }
        }
    }
    {
        // GPU busy time = union of the command buffers' GPU intervals (a frame is split into several command
        // buffers; the gaps between them are idle time, not work).
        let (start, end) = (prev.GPUStartTime(), prev.GPUEndTime());
        if end > start && end - start < 1.0 {
            static LAST_END: std::sync::Mutex<f64> = std::sync::Mutex::new(0.0);
            let mut last = LAST_END.lock().unwrap();
            let busy = end - start.max(*last);
            if busy > 0.0 {
                GPU_BUSY_US.fetch_add((busy * 1e6) as u64, std::sync::atomic::Ordering::Relaxed);
            }
            *last = last.max(end);
        }
        if prev.status() == MTLCommandBufferStatus::Error {
            let victim = prev.error().map_or(false, |e| {
                e.localizedDescription().to_string().contains("victim")
            });
            let mut st = g.state.lock().unwrap();
            if victim {
                // Discarded because the GPU was recovering from an error caused by something else (another process, or
                // a previous run that was just killed). Transient: skip this frame's work and carry on, but stop if it
                // keeps happening.
                st.victims_in_a_row += 1;
                println!("[!] GPU: a command buffer was discarded as an innocent victim of a GPU recovery (#{} in a row); continuing", st.victims_in_a_row);
                if st.victims_in_a_row >= 8 {
                    st.gpu_halted = true;
                    println!("[!!] GPU: rendering halted: the GPU keeps discarding work");
                }
            } else {
                let (labels, details) = (entry.labels.clone(), entry.details.clone());
                drop(st);
                report_gpu_error(&prev, &labels, &details);
                g.state.lock().unwrap().gpu_halted = true;
                println!("[!!] GPU: rendering halted to protect the system; the game keeps running without drawing");
            }
        } else {
            g.state.lock().unwrap().victims_in_a_row = 0;
        }
    }
    COMPLETED_SERIAL.fetch_max(entry.serial, std::sync::atomic::Ordering::AcqRel);
    true
}

/// Encoders per Metal command buffer before `maybe_split` commits it (--split-encoders, default 24; 0 = one
/// command buffer per submit).
/// --split-draws=N (default 600, 0 = off): end a render encoder after N draws and commit, continuing in a new encoder
/// that loads the targets. The GPU cannot start an encoder before it ends and its command buffer is committed: TotK's
/// G-buffer pass (~1000 draws, ~10 ms to encode) ran entirely after it was encoded, in the GPU tail the game waits for.
/// Each cut stores and reloads every attachment (~50 MB for the G-buffer): 600 beat 250 in GPU-bound Hyrule Field
/// (42.6 / 38.2 vs 37.7 / 36.0 fps) and matched it in Hateno; 1500 left a ~13 ms GPU tail.
/// Re-measured 2026-10-09 in Hateno (feedback in place, GPU ~10.8 ms): 150 / 300 / 600 / 1200 gave tail 3.7 / 3.8 / 4.4 /
/// 4.9 ms, total GPU 12.4 / 11.6 / 10.8 / 10.6 ms, 57.9 / 58.4 / 57.4 / 56.9 fps; 0 = a 21 ms tail. 300 wins ~1 fps where
/// encode dominates but costs GPU time, which GPU-bound scenes pay: 600 kept.
pub(super) fn split_draws() -> u32 {
    let s = crate::settings::get();
    // Profiling: --split-draws-from=F gives every draw its own encoder from presented frame F on, so a GPU trace times
    // draws individually (each also pays a pass's load / store) without the slow-down before reaching the scene.
    if s.graphics
        .split_draws_from
        .is_some_and(|f| presented_frames() >= f)
    {
        return 1;
    }
    // --replay-ab-split=N: replay side B cuts every N draws instead (same-frame A/B of the cut's GPU cost).
    match s.replay.replay_ab_split {
        Some(b) if replay_b() => b,
        _ => s.graphics.split_draws,
    }
}

/// --split-ms=T (default 1.0, 0 = off): commit the command buffer between passes once T ms were spent encoding it.
pub(super) fn split_ms() -> f64 {
    static T: OnceLock<f64> = OnceLock::new();
    *T.get_or_init(|| crate::settings::get().graphics.split_ms)
}

pub(super) fn split_encoders() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| crate::settings::get().graphics.split_encoders)
}

/// Records a just-committed command buffer as in flight, with the encoder labels/details and occlusion reports
/// encoded into it.
pub(super) fn push_inflight(st: &mut State, cb: Retained<ProtocolObject<dyn MTLCommandBuffer>>) -> u64 {
    let serial = SUBMITTED_SERIAL.fetch_add(1, std::sync::atomic::Ordering::AcqRel) + 1;
    rt_guard::committed(serial, &cb);
    // Occlusion reports are written as soon as the command buffer completes (on the fence watcher thread) instead of
    // when the executor later retires it: with --early-fence the game reads them before its next submit, and a
    // report still unwritten then read as stale memory (an object that just came into range or switched LOD was culled
    // for a frame: Hateno signposts, 2026-10-13). Retiring then skips them: it runs later, and rewriting a report the
    // game reuses next frame would put the older value back over the newer one.
    let mut occl = std::mem::take(&mut st.occl.pending);
    if journal_on() && !occl.is_empty() {
        journal(|| {
            format!(
                "occl-cb {} committed with {} reports t {:.3}",
                serial,
                occl.len(),
                host_secs() * 1e3
            )
        });
    }
    let mut occl_watched = None;
    let ts_list = std::mem::take(&mut st.ts_pending);
    let ts = std::sync::Arc::new(std::sync::Mutex::new(
        (!ts_list.is_empty()).then_some(ts_list),
    ));
    struct SendBuf(Retained<ProtocolObject<dyn MTLBuffer>>);
    unsafe impl Send for SendBuf {}
    struct SendCb(Retained<ProtocolObject<dyn MTLCommandBuffer>>);
    unsafe impl Send for SendCb {}
    let occl_part = match (&st.occl.buf, occl.is_empty()) {
        (Some(buf), false) => {
            let shared = std::sync::Arc::new(std::sync::Mutex::new(Some(occl.clone())));
            occl_watched = Some(shared.clone());
            Some((SendBuf(buf.clone()), shared))
        }
        _ => None,
    };
    let has_ts = ts.lock().unwrap().is_some();
    let on_done: Option<Box<dyn FnOnce()>> = if occl_part.is_some() || has_ts {
        let ts = ts.clone();
        let cbs = SendCb(cb.clone());
        Some(Box::new(move || {
            let (occl_part, cbs) = (occl_part, cbs);
            if let Some((buf, shared)) = occl_part {
                let mut g = shared.lock().unwrap();
                if let Some(reports) = g.take() {
                    resolve_occlusion(&buf.0, &reports);
                }
            }
            if let Some(list) = ts.lock().unwrap().take() {
                resolve_timestamps(&cbs.0, &list);
            }
        }))
    } else {
        None
    };
    if fence_guard::watch(serial, &cb, on_done) {
        occl.clear();
    } else {
        occl_watched = None;
    }
    let entry = InFlight {
        serial,
        group: st.stats.submits,
        cb,
        labels: std::mem::take(&mut st.lb.enc_labels),
        details: std::mem::take(&mut st.lb.enc_details),
        occl,
        occl_watched,
        ts,
    };
    st.inflight.push_back(entry);
    serial
}

/// Submits with command buffers still in flight.
pub(super) fn groups_in_flight(st: &State) -> usize {
    match (st.inflight.front(), st.inflight.back()) {
        (Some(a), Some(b)) => (b.group - a.group + 1) as usize,
        _ => 0,
    }
}
