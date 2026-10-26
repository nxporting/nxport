// Submits in flight. A submit returns once its command buffers are queued; a watcher thread waits for each submit's
// fence in submit order and does what needs the GPU's results (occlusion reports, storage buffers bound as copies, GPU
// timing), then publishes the submit's serial as completed. The context frees the submit's resources after that.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

use ash::vk;

/// Occlusion query work of a submit, in recording order: the count since the last reset accumulates over submits.
pub(super) enum Occl {
    /// A query ended: its samples count.
    Query(u32),
    /// ResetCounter: counting starts again from zero.
    Reset,
    /// ReportCounter: the count so far is written to this guest address.
    Report(u64),
}

/// What the watcher does once a submit completed.
pub(super) struct Done {
    pub serial: u64,
    /// Null for a submit that queued nothing.
    pub fence: vk::Fence,
    pub occl_pool: vk::QueryPool,
    pub queries: u32,
    pub occl: Vec<Occl>,
    /// Storage buffers bound as copies: (guest address, copy, length), copied back.
    pub writebacks: Vec<(usize, usize, usize)>,
    /// Two timestamps around the submit, then one per pass end with its description when profiling passes.
    pub timestamps: vk::QueryPool,
    pub pass_labels: Vec<String>,
    /// Render-target write-back: (texture, level, copy of its texels, bytes), written to guest memory.
    pub rt_writebacks: Vec<(crate::nvn::api::NvnTexture, u32, usize, usize)>,
}

// The copies are arena memory the context keeps until the serial completed; the handles are plain Vulkan handles.
unsafe impl Send for Done {}

/// Highest serial whose submit (and every earlier one) completed and was processed.
static COMPLETED: AtomicU64 = AtomicU64::new(0);
/// A submit failed: the device is lost and nothing completes any more.
pub(super) static LOST: AtomicBool = AtomicBool::new(false);
static WAKE: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());
static TX: OnceLock<Mutex<std::sync::mpsc::Sender<Done>>> = OnceLock::new();

pub(super) fn completed() -> u64 {
    COMPLETED.load(Ordering::Acquire)
}

/// Starts the watcher (once, when the device is made).
pub(super) fn start(device: ash::Device, timestamp_period: f32) {
    let (tx, rx) = std::sync::mpsc::channel::<Done>();
    if TX.set(Mutex::new(tx)).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("vk-fence-watch".into())
        .spawn(move || {
            super::guard::engine_thread();
            let mut carried = 0u64;
            let mut results: Vec<u64> = Vec::new();
            while let Ok(d) = rx.recv() {
                unsafe { finish(&device, timestamp_period, &mut carried, &mut results, d) };
            }
        })
        .expect("spawn vk-fence-watch");
}

unsafe fn finish(device: &ash::Device, period: f32, carried: &mut u64, results: &mut Vec<u64>, d: Done) {
    let mut ok = !LOST.load(Ordering::Acquire);
    if d.fence != vk::Fence::null() {
        if let Err(e) = device.wait_for_fences(&[d.fence], true, u64::MAX) {
            let frame = super::VULKAN.frames.load(Ordering::Relaxed);
            println!("[vulkan] submit {} at presented frame {frame} failed: {e}; no more GPU work", d.serial);
            LOST.store(true, Ordering::Release);
            ok = false;
        }
        device.destroy_fence(d.fence, None);
    }
    results.clear();
    results.resize(d.queries as usize, 0);
    if ok && d.queries > 0 {
        let _ = device.get_query_pool_results(d.occl_pool, 0, results, vk::QueryResultFlags::TYPE_64);
    }
    for e in &d.occl {
        match *e {
            Occl::Query(q) => *carried += results.get(q as usize).copied().unwrap_or(0),
            Occl::Reset => *carried = 0,
            Occl::Report(addr) => {
                super::exec::write_report(addr, *carried, true);
                let host = crate::nvn::api::translate_virtual(addr as usize);
                crate::gpu::report_guard::close(host);
                let frame = super::VULKAN.frames.load(Ordering::Relaxed) + 1;
                let count = *carried;
                crate::gpu::history::journal(|| format!("occl f{frame} #0 pos [] draws 0 = {count} @{addr:#x} submit {}", d.serial));
            }
        }
    }
    for &(addr, src, len) in &d.writebacks {
        super::guard::write_back(addr, src as *const u8, len);
        super::texture::note_wb_landed(addr);
    }
    if ok {
        for (t, level, src, len) in &d.rt_writebacks {
            crate::nvn::api::guest_write_level(t, *level as usize, std::slice::from_raw_parts(*src as *const u8, *len));
            super::texture::note_wb_landed(t.storage);
        }
    }
    if ok && d.timestamps != vk::QueryPool::null() {
        let mut ts = [0u64; 2];
        if device.get_query_pool_results(d.timestamps, 0, &mut ts, vk::QueryResultFlags::TYPE_64).is_ok() && ts[1] > ts[0] {
            let ns = ((ts[1] - ts[0]) as f64 * period as f64) as u64;
            super::GPU_NS.fetch_add(ns, Ordering::Relaxed);
            super::LAST_GPU_NS.store(ns, Ordering::Relaxed);
            if !d.pass_labels.is_empty() {
                let mut t = vec![0u64; d.pass_labels.len()];
                if device.get_query_pool_results(d.timestamps, 2, &mut t, vk::QueryResultFlags::TYPE_64).is_ok() {
                    let mut prev = ts[0];
                    let mut profile = Vec::with_capacity(t.len());
                    for (end, label) in t.iter().zip(d.pass_labels) {
                        profile.push(((end.saturating_sub(prev) as f64 * period as f64) as u64, label));
                        prev = prev.max(*end);
                    }
                    *super::PASS_PROFILE.lock().unwrap() = profile;
                }
            }
        }
    }
    let _g = WAKE.0.lock().unwrap();
    COMPLETED.fetch_max(d.serial, Ordering::AcqRel);
    WAKE.1.notify_all();
}

/// Hands a queued submit to the watcher.
pub(super) fn watch(d: Done) {
    match TX.get() {
        Some(tx) => {
            let _ = tx.lock().unwrap().send(d);
        }
        None => {
            let _g = WAKE.0.lock().unwrap();
            COMPLETED.fetch_max(d.serial, Ordering::AcqRel);
            WAKE.1.notify_all();
        }
    }
}

/// Blocks until `serial` completed (false after `timeout_ns`; 0 polls).
pub(super) fn wait(serial: u64, timeout_ns: u64) -> bool {
    if completed() >= serial {
        return true;
    }
    if timeout_ns == 0 {
        return false;
    }
    let t0 = std::time::Instant::now();
    let deadline = t0.checked_add(std::time::Duration::from_nanos(timeout_ns));
    let mut g = WAKE.0.lock().unwrap();
    while completed() < serial {
        let left = match deadline {
            Some(d) => match d.checked_duration_since(std::time::Instant::now()) {
                Some(l) if !l.is_zero() => l,
                _ => return false,
            },
            None => std::time::Duration::from_secs(1),
        };
        g = WAKE.1.wait_timeout(g, left).unwrap().0;
    }
    drop(g);
    let ns = t0.elapsed().as_nanos() as u64;
    super::WAIT_NS.fetch_add(ns, Ordering::Relaxed);
    super::LAST_WAIT_NS.fetch_add(ns, Ordering::Relaxed);
    true
}
