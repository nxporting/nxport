// --log-frame-breakdown: where each presented frame's time goes. On the game's main thread (guest-main): its CPU time and
// its waits by kind: the frame-rate limiter's sleep, the GPU fence, the render-target guard (GPU results it reads back),
// guest mutexes (the render thread's lock, held through our encoding), and events / objects (its Workers). Next to them:
// our submit (encode) time and the GPU's busy time. Frames slower than 1.5x the median are printed with their parts, and
// a summary every 300 frames: what makes slow frames slow, measured instead of guessed.

use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

#[derive(Clone, Copy)]
pub enum Wait {
    Pacing = 0,
    Fence = 1,
    Guard = 2,
    Lock = 3,
    Event = 4,
}
const KINDS: usize = 5;
const NAMES: [&str; KINDS] = ["pacing", "fence", "guard", "lock", "events"];

static WAIT_NS: [AtomicU64; KINDS] = [const { AtomicU64::new(0) }; KINDS];
static MAIN_CPU_NS: AtomicU64 = AtomicU64::new(0);
static ENCODE_NS: AtomicU64 = AtomicU64::new(0);
static GPU_NS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The game's main thread (set when it starts: signal handlers read it, so no name lookup there).
    static MAIN: Cell<bool> = const { Cell::new(false) };
}

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().log.log_frame_breakdown)
}

/// The calling thread is the game's main thread.
pub fn mark_main() {
    MAIN.with(|m| m.set(true));
}

fn on_main() -> bool {
    MAIN.try_with(|m| m.get()).unwrap_or(false)
}

/// A wait of kind `w` on the main thread, timed until dropped (signal-safe).
pub struct Timer(Option<(Wait, u64)>);

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some((w, t0)) = self.0 {
            WAIT_NS[w as usize].fetch_add(crate::platform::clock::now_ns().saturating_sub(t0), Relaxed);
        }
    }
}

pub fn wait(w: Wait) -> Timer {
    Timer((enabled() && on_main()).then(|| (w, crate::platform::clock::now_ns())))
}

/// The main thread's CPU clock, read on the main thread (once a frame, at its GPU sync).
pub fn sample_main_cpu() {
    if !enabled() || !on_main() {
        return;
    }
    #[cfg(unix)]
    unsafe {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts);
        MAIN_CPU_NS.store(ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64, Relaxed);
    }
}

pub fn add_encode_ns(ns: u64) {
    if enabled() {
        ENCODE_NS.fetch_add(ns, Relaxed);
    }
}

pub fn add_gpu_ns(ns: u64) {
    if enabled() {
        GPU_NS.fetch_add(ns, Relaxed);
    }
}

/// One frame's parts (ms): interval, main CPU, waits by kind, encode, GPU.
type Parts = [f32; KINDS + 5];

struct State {
    last_ns: u64,
    last_cpu: u64,
    frames: Vec<Parts>,
}

static STATE: std::sync::Mutex<State> = std::sync::Mutex::new(State { last_ns: 0, last_cpu: 0, frames: Vec::new() });

/// A frame was presented.
pub fn frame_end(frame: u64) {
    if !enabled() {
        return;
    }
    let now = crate::platform::clock::now_ns();
    let mut s = STATE.lock().unwrap();
    let cpu = MAIN_CPU_NS.load(Relaxed);
    let mut p: Parts = [0.0; KINDS + 5];
    p[KINDS + 4] = frame as f32;
    let ms = |ns: u64| ns as f32 / 1e6;
    let first = s.last_ns == 0;
    p[0] = ms(now.saturating_sub(s.last_ns));
    p[1] = ms(cpu.saturating_sub(s.last_cpu));
    for k in 0..KINDS {
        p[2 + k] = ms(WAIT_NS[k].swap(0, Relaxed));
    }
    p[2 + KINDS] = ms(ENCODE_NS.swap(0, Relaxed));
    p[3 + KINDS] = ms(GPU_NS.swap(0, Relaxed));
    s.last_ns = now;
    s.last_cpu = cpu;
    if first {
        return;
    }
    s.frames.push(p);
    if s.frames.len() < 300 {
        return;
    }
    let frames = std::mem::take(&mut s.frames);
    drop(s);
    report(frame, &frames);
}

fn line(p: &Parts) -> String {
    let mut out = format!("{:.1} ms: main CPU {:.1}", p[0], p[1]);
    for k in 0..KINDS {
        out += &format!(", {} {:.1}", NAMES[k], p[2 + k]);
    }
    out += &format!(", other {:.1}", (p[0] - p[1] - p[2..2 + KINDS].iter().sum::<f32>()).max(0.0));
    out += &format!(" | encode {:.1}, GPU {:.1}", p[2 + KINDS], p[3 + KINDS]);
    out
}

fn report(frame: u64, frames: &[Parts]) {
    let mut iv: Vec<f32> = frames.iter().map(|p| p[0]).collect();
    iv.sort_by(|a, b| a.total_cmp(b));
    let median = iv[iv.len() / 2];
    let slow: Vec<&Parts> = frames.iter().filter(|p| p[0] > 1.5 * median).collect();
    let mean = |set: &[&Parts]| {
        let mut m: Parts = [0.0; KINDS + 5];
        for p in set {
            for k in 0..m.len() {
                m[k] += p[k] / set.len().max(1) as f32;
            }
        }
        m
    };
    let all: Vec<&Parts> = frames.iter().collect();
    println!(
        "[frame-breakdown] frames {}..{}: median {:.1} ms, p90 {:.1}, slow (> 1.5x median) {}",
        (frame + 1).saturating_sub(frames.len() as u64),
        frame,
        median,
        iv[iv.len() * 9 / 10],
        slow.len()
    );
    println!("[frame-breakdown]   mean of all:  {}", line(&mean(&all)));
    if !slow.is_empty() {
        println!("[frame-breakdown]   mean of slow: {}", line(&mean(&slow)));
        let mut worst = slow.clone();
        worst.sort_by(|a, b| b[0].total_cmp(&a[0]));
        for p in worst.iter().take(3) {
            println!("[frame-breakdown]   slow frame {}", line(p));
        }
        let nums: Vec<String> = slow.iter().map(|p| format!("{}", p[KINDS + 4] as u64)).collect();
        println!("[frame-breakdown]   slow frames: {}", nums.join(" "));
    }
}
