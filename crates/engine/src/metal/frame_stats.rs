// Frame timing and statistics: the frame timeline (--log-timeline), command buffer tails (--log-tail), encoder
// statistics (--enc-stats), slow frames (--log-hitch) and the displayed frame rate.

use super::*;

/// Frame timeline (--log-timeline, printed with the fps line): encode time in submit(), guest time blocked in
/// nvnSyncWait, GPU tail after the frame's last submit, presentation-thread gap between submits.
pub(crate) static TL_ENCODE_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// CPU time of the encoding thread during those submits (the rest of TL_ENCODE_US is waiting or preemption).
pub(super) static TL_ENCODE_CPU_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// This thread's CPU time.
pub(crate) fn thread_cpu_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
pub(crate) static TL_SYNC_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static TL_TAIL_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// --log-tail: every 50th frame, each committed command buffer's commit time and GPU start / end (one clock,
/// ms from the frame's first commit), to see whether the GPU tail is the last chunk's work or a backlog.
pub(super) fn tail_log_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().log.log_tail)
}

/// Command buffers are thread-safe Metal objects; only these debug lists hold them across threads.
pub(super) struct TailCbs(Vec<(Retained<ProtocolObject<dyn MTLCommandBuffer>>, f64)>);
unsafe impl Send for TailCbs {}
pub(super) static TAIL_CBS: Mutex<TailCbs> = Mutex::new(TailCbs(Vec::new()));
pub(super) static TAIL_PREV: Mutex<Option<(u64, TailCbs)>> = Mutex::new(None);

pub(super) fn host_secs() -> f64 {
    crate::platform::clock::now_secs()
}


/// Names a command buffer after the encoders it holds (--log-tail).
pub(super) fn tail_log_label(st: &State, cb: &ProtocolObject<dyn MTLCommandBuffer>) {
    if !tail_log_enabled() {
        return;
    }
    let short = |l: &String| {
        l.split(" | ")
            .next()
            .unwrap_or("")
            .chars()
            .take(60)
            .collect::<String>()
    };
    let label = match (st.lb.enc_labels.first(), st.lb.enc_labels.last()) {
        (Some(a), Some(b)) => format!(
            "{} enc: {} .. {}",
            st.lb.enc_labels.len(),
            short(a),
            short(b)
        ),
        _ => "empty".to_string(),
    };
    cb.setLabel(Some(&objc2_foundation::NSString::from_str(&label)));
}

pub(super) fn tail_log_commit(cb: &Retained<ProtocolObject<dyn MTLCommandBuffer>>) {
    if tail_log_enabled() {
        TAIL_CBS.lock().unwrap().0.push((cb.clone(), host_secs()));
    }
}

/// At a present: the previous logged frame's command buffers have completed by now (TotK waited for them).
pub(super) fn tail_log_present(frame: u64) {
    if !tail_log_enabled() {
        return;
    }
    let cbs = std::mem::replace(&mut *TAIL_CBS.lock().unwrap(), TailCbs(Vec::new()));
    let prev = TAIL_PREV.lock().unwrap().replace((frame, cbs));
    let Some((f, TailCbs(cbs))) = prev else {
        return;
    };
    if f % 50 != 0 || cbs.is_empty() {
        return;
    }
    let t0 = cbs[0].1;
    let ms = |t: f64| (t - t0) * 1e3;
    let mut line = format!("[tail] f{} {} cbs:", f, cbs.len());
    let mut gpu_busy = 0.0;
    let mut last_end = t0;
    for (cb, c) in &cbs {
        let (gs, ge) = (cb.GPUStartTime(), cb.GPUEndTime());
        gpu_busy += (ge - gs).max(0.0);
        last_end = last_end.max(ge);
        line.push_str(&format!(
            "\n    c{:5.1} g{:5.1}-{:5.1} ({:4.2} ms) {}",
            ms(*c),
            ms(gs),
            ms(ge),
            (ge - gs) * 1e3,
            cb.label().map(|l| l.to_string()).unwrap_or_default()
        ));
    }
    let last_commit = cbs.last().map(|x| x.1).unwrap_or(t0);
    let backlog: f64 = cbs
        .iter()
        .map(|(cb, _)| {
            (cb.GPUEndTime().min(f64::MAX) - cb.GPUStartTime().max(last_commit)).max(0.0)
        })
        .sum();
    line.push_str(&format!(" | GPU busy {:.1} ms, last commit {:.1}, GPU done {:.1} (tail {:.1}, of which GPU work after last commit {:.1})",
        gpu_busy * 1e3, ms(last_commit), ms(last_end), (last_end - last_commit) * 1e3, backlog * 1e3));
    println!("{}", line);
}
pub(crate) static TL_GAP_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(super) static TL_LAST_SUBMIT_END: Mutex<Option<std::time::Instant>> = Mutex::new(None);
pub(crate) fn timeline_enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| crate::settings::get().log.log_timeline)
}
/// Microseconds since the last submit ended (for the GPU tail measured by nvnSyncWait).
pub(crate) fn since_last_submit_us() -> Option<u64> {
    TL_LAST_SUBMIT_END
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_micros() as u64)
}

pub(crate) fn enc_stats_enabled() -> bool {
    static E: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *E.get_or_init(|| crate::settings::get().log.enc_stats)
}
pub(super) static ENC_END_SITES: std::sync::Mutex<std::collections::BTreeMap<(&'static str, u32), u64>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
pub(crate) static ENC_REOPEN_SAME: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static ENC_OPENED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static CLEARS_FOLDED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static CLEARS_FLUSHED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
/// Render passes those clears took (clears of equal size share one, flush_clears_matching).
pub(crate) static CLEAR_PASSES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(super) fn print_enc_stats(frames: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    let mut sites = ENC_END_SITES.lock().unwrap();
    let mut v: Vec<_> = sites.iter().map(|(k, n)| (*n, *k)).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    let f = frames.max(1) as f64;
    println!("[enc-stats] per frame: translated passes opened {:.1}, reopened same targets {:.1}, clears folded {:.1} / own pass {:.1}; ends by site: {}",
        ENC_OPENED.swap(0, Relaxed) as f64 / f, ENC_REOPEN_SAME.swap(0, Relaxed) as f64 / f,
        CLEARS_FOLDED.swap(0, Relaxed) as f64 / f, CLEARS_FLUSHED.swap(0, Relaxed) as f64 / f,
        v.iter().take(10).map(|(n, (file, line))| format!("{}:{}={:.1}", file.rsplit('/').next().unwrap_or(file), line, *n as f64 / f)).collect::<Vec<_>>().join(" "));
    sites.clear();
}

#[cfg(target_os = "macos")]
pub fn display_fps() -> f32 {
    f32::from_bits(DISPLAY_FPS.load(std::sync::atomic::Ordering::Relaxed))
}

pub(super) fn count_display_frame() {
    if enc_stats_enabled() {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 600 == 599 {
            print_enc_stats(600);
        }
    }
    static WINDOW: std::sync::Mutex<Option<(std::time::Instant, u32)>> =
        std::sync::Mutex::new(None);
    let now = std::time::Instant::now();
    let mut w = WINDOW.lock().unwrap();
    let (start, n) = w.get_or_insert((now, 0));
    *n += 1;
    let dt = now.duration_since(*start).as_secs_f32();
    if dt >= 0.5 {
        DISPLAY_FPS.store(
            (*n as f32 / dt).to_bits(),
            std::sync::atomic::Ordering::Relaxed,
        );
        *w = Some((now, 0));
    }
}

/// nvnQueuePresentTexture: show `nvn_tex` (a window texture) in the window.
/// --log-hitch=<ms>: per-frame work counters, printed for frames whose present-to-present time exceeds the threshold.
pub(crate) static HITCH_PIPE_NS: AtomicU64 = AtomicU64::new(0);
pub(crate) static HITCH_PIPE_N: AtomicU64 = AtomicU64::new(0);
pub(crate) static HITCH_TEX_N: AtomicU64 = AtomicU64::new(0);
pub(super) static HITCH_UPLOAD_NS: AtomicU64 = AtomicU64::new(0);
pub(super) static HITCH_HASH_NS: AtomicU64 = AtomicU64::new(0);
pub(super) static HITCH_TEX: Mutex<Vec<String>> = Mutex::new(Vec::new());
pub(super) static HITCH_MS: OnceLock<Option<f64>> = OnceLock::new();
pub(super) fn hitch_enabled() -> bool {
    HITCH_MS
        .get_or_init(|| crate::settings::get().log.log_hitch)
        .is_some()
}

pub(super) fn log_hitch() {
    hitch_enabled();
    let Some(thr) = *HITCH_MS.get().unwrap() else {
        return;
    };
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let now = std::time::Instant::now();
    let prev = LAST.lock().unwrap().replace(now);
    let pipe_ns = HITCH_PIPE_NS.swap(0, Ordering::Relaxed);
    let pipe_n = HITCH_PIPE_N.swap(0, Ordering::Relaxed);
    let async_started = translated::ASYNC_STARTED.swap(0, Ordering::Relaxed);
    let async_skips = translated::ASYNC_SKIPS.swap(0, Ordering::Relaxed);
    let tex_n = HITCH_TEX_N.swap(0, Ordering::Relaxed);
    let texs = std::mem::take(&mut *HITCH_TEX.lock().unwrap());
    let up_ns = HITCH_UPLOAD_NS.swap(0, Ordering::Relaxed);
    let hash_ns = HITCH_HASH_NS.swap(0, Ordering::Relaxed);
    if let Some(p) = prev {
        let ms = (now - p).as_secs_f64() * 1e3;
        if ms > thr {
            println!("[hitch] frame {} {:.1} ms: {} pipelines built in {:.1} ms, {} textures created (upload {:.1} ms, hash {:.1} ms)", presented_frames(), ms,
                pipe_n, pipe_ns as f64 / 1e6, tex_n, up_ns as f64 / 1e6, hash_ns as f64 / 1e6);
            if async_started + async_skips > 0 {
                println!("[hitch]   async pipelines: {} compiles started, {} draws skipped while compiling", async_started, async_skips);
            }
            for t in &texs {
                println!("[hitch]   new texture {}", t);
            }
        }
    }
}
