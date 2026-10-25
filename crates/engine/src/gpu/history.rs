// Frame history (--frame-history=N): the backend keeps the last N presented frames at half resolution, and a journal
// of engine events per frame (render passes, clears, texture creations and uploads, dispatches, occlusion reports,
// conditional rendering). A capture (F9 / space, or --history-dump-at) writes them to /tmp/nxport_history_<frame>/:
// <frame>.ppm per frame and journal.txt, and logs the frames ranked by single-frame outlier pixels ([history]).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Frames kept (0: off).
pub fn len() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| crate::settings::get().dump.frame_history)
}

static JOURNAL: Mutex<(Vec<String>, VecDeque<(u64, Vec<String>)>)> = Mutex::new((Vec::new(), VecDeque::new()));
static REQUESTED: AtomicBool = AtomicBool::new(false);

#[inline]
pub fn journal_on() -> bool {
    len() > 0
}

/// Adds an event to the frame being recorded.
pub fn journal(line: impl FnOnce() -> String) {
    if journal_on() {
        let l = line();
        JOURNAL.lock().unwrap().0.push(l);
    }
}

/// Closes the journal of presented frame `pn` (everything since the previous present).
pub fn rotate(pn: u64) {
    let mut j = JOURNAL.lock().unwrap();
    let cur = std::mem::take(&mut j.0);
    j.1.push_back((pn, cur));
    while j.1.len() > len() {
        j.1.pop_front();
    }
}

/// Asks for the history to be written at the next present.
pub fn request() {
    REQUESTED.store(true, Ordering::Release);
}

/// Whether the history is to be written at presented frame `pn` (taking a pending request).
pub fn due(pn: u64) -> bool {
    static AT: OnceLock<Option<u64>> = OnceLock::new();
    let at = *AT.get_or_init(|| crate::settings::get().dump.history_dump_at);
    REQUESTED.swap(false, Ordering::AcqRel) || at == Some(pn)
}

/// The capture directory for presented frame `pn`.
pub fn dir(pn: u64) -> String {
    let d = format!("/tmp/nxport_history_{pn}");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Writes the journal of the kept frames.
pub fn write_journal(dir: &str) {
    let j = JOURNAL.lock().unwrap();
    let mut out = String::new();
    for (f, lines) in &j.1 {
        out.push_str(&format!("== frame {} ({} events)\n", f, lines.len()));
        for l in lines {
            out.push_str(l);
            out.push('\n');
        }
    }
    let _ = std::fs::write(format!("{dir}/journal.txt"), out);
}

/// Writes frames (presented frame, width, height, RGB rows top first) as <frame>.ppm, oldest first, and logs them
/// ranked by single-frame outlier pixels (a pixel of frame t far from t-1 and t+1 while those agree; with a moving
/// camera every frame scores, so compare scores, not absolute values).
pub fn write_frames(dir: &str, frames: &mut [(u64, usize, usize, Vec<u8>)]) {
    frames.sort_by_key(|f| f.0);
    for (f, w, h, rgb) in frames.iter() {
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        out.extend_from_slice(rgb);
        let _ = std::fs::write(format!("{dir}/{f:06}.ppm"), out);
    }
    let mut scores: Vec<(usize, u64)> = Vec::new();
    for k in 1..frames.len().saturating_sub(1) {
        let (a, b, c) = (&frames[k - 1].3, &frames[k].3, &frames[k + 1].3);
        if a.len() != b.len() || b.len() != c.len() {
            continue;
        }
        let d = |x: &[u8], y: &[u8], i: usize| (0..3).map(|j| x[i + j].abs_diff(y[i + j])).max().unwrap();
        let n = (0..b.len() / 3)
            .filter(|p| {
                let i = p * 3;
                d(a, c, i) < 16 && d(b, a, i) > 40 && d(b, c, i) > 40
            })
            .count();
        scores.push((n, frames[k].0));
    }
    let mut sorted = scores.clone();
    sorted.sort_unstable();
    let median = sorted.get(sorted.len() / 2).map_or(0, |s| s.0);
    sorted.reverse();
    println!(
        "[history] single-frame outlier pixels: median {}, top: {}",
        median,
        sorted.iter().take(8).map(|(n, f)| format!("f{f} {n}")).collect::<Vec<_>>().join(", ")
    );
    if let (Some(first), Some(last)) = (frames.first(), frames.last()) {
        println!("[+] Frame history: wrote {} frames ({}..{}) and their journal to {dir}", frames.len(), first.0, last.0);
    }
}
