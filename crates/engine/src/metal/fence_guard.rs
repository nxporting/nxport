// --early-fence's write guard: the overlap stops exactly where the game touches memory the GPU still reads.
//
// With --early-fence guest-main's frame fence returns once the frame's command buffers are committed, so the game
// records its next frame while the GPU still runs this one. Draws read vertex, index, storage and some uniform data
// straight from guest memory (zero-copy pool buffers), and TotK writes the next frame's per-instance data into memory
// the running frame still reads (on the Switch the fence would have held it back): single-frame black objects.
//
// At encode time every zero-copy binding records its range: GPU reads (vertex / index / uniform buffers, copy sources)
// and GPU writes (storage buffers, copy destinations). When guest-main's fence returns early, the pages read by
// still-running command buffers are made read-only. A write to one faults: the signal handler blocks that thread until
// the command buffer has completed, then makes the range writable again and the write is retried. Writes by the host
// kernel (file reads into guest buffers) cannot fault, so guest::dirty_track::before_host_write waits here first.
//
// GPU writes are lost on read-only pages, so pages that GPU work in flight (or encoded later) writes are never
// protected; the game reads such results a frame late, as before. Completion is published lock-free by a watcher
// thread (the handler must not take the gfx state lock). Pages held here are first released from dirty_track
// (release_for_gpu), whose fault handler would otherwise unprotect them without waiting; the crash handler asks this
// module first.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLCommandBuffer, MTLCommandBufferStatus};
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const PAGE: usize = 0x4000;
const SLOTS: usize = 8192;

/// Whether the guard runs: with --early-fence unless --early-fence-guard=0 (the old unguarded overlap, for A/B)
/// or the --early-fence-trap diagnostic (which protects pages its own way).
pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        super::early_fence()
            && crate::settings::get().graphics.early_fence_guard
            && crate::settings::get().debug.early_fence_trap != crate::settings::FenceTrap::On
    })
}

// ---- completion ---------------------------------------------------------------------------------------------------

/// Highest serial whose command buffer (and every earlier one) has completed, published by the watcher thread.
static GPU_DONE: AtomicU64 = AtomicU64::new(0);

/// (serial, command buffer, work to run once it completed: its occlusion reports, see metal::push_inflight).
struct Watch(
    u64,
    Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    Option<Box<dyn FnOnce()>>,
);
// Command buffer status queries and waits are thread-safe.
unsafe impl Send for Watch {}

fn watcher() -> &'static Mutex<std::sync::mpsc::Sender<Watch>> {
    static TX: OnceLock<Mutex<std::sync::mpsc::Sender<Watch>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Watch>();
        std::thread::Builder::new()
            .name("gfx-fence-watch".into())
            .spawn(move || {
                // In serial order: completion of n then covers every earlier serial.
                while let Ok(Watch(serial, cb, on_done)) = rx.recv() {
                    loop {
                        match cb.status() {
                            MTLCommandBufferStatus::Completed | MTLCommandBufferStatus::Error => {
                                break
                            }
                            MTLCommandBufferStatus::Committed
                            | MTLCommandBufferStatus::Scheduled => cb.waitUntilCompleted(),
                            // Not committed yet (the issuer commits it after its queued ops).
                            _ => std::thread::sleep(std::time::Duration::from_micros(100)),
                        }
                    }
                    if let Some(f) = on_done {
                        f();
                    }
                    GPU_DONE.fetch_max(serial, Ordering::AcqRel);
                }
            })
            .expect("spawn fence watcher");
        Mutex::new(tx)
    })
}

/// A command buffer entered the in-flight queue with `serial` (metal::push_inflight).
/// `on_done` runs on the watcher thread right after the command buffer completed. Returns false (and drops `on_done`)
/// when the guard is off: then nothing watches command buffers.
pub(super) fn watch(
    serial: u64,
    cb: &Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    on_done: Option<Box<dyn FnOnce()>>,
) -> bool {
    // --report-guard: occlusion reports are written at completion (game threads wait for them) with the fence off too.
    if enabled()
        || (on_done.is_some() && super::report_guard::mode() != crate::settings::ReportGuard::Off)
    {
        let _ = watcher()
            .lock()
            .unwrap()
            .send(Watch(serial, cb.clone(), on_done));
        return true;
    }
    false
}

fn done() -> u64 {
    GPU_DONE
        .load(Ordering::Acquire)
        .max(super::completed_serial())
}

/// Blocks (signal-safe: atomics and nanosleep only) until `serial` completed; false after 2 s (then the caller goes
/// on anyway rather than hang the game).
fn wait_done(serial: u64) -> bool {
    if done() >= serial {
        return true;
    }
    let t0 = std::time::Instant::now();
    let mut spins = 0u32;
    while done() < serial {
        if spins < 64 {
            std::hint::spin_loop();
            spins += 1;
            continue;
        }
        let ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 20_000,
        };
        unsafe { libc::nanosleep(&ts, std::ptr::null_mut()) };
        if t0.elapsed().as_secs() >= 2 {
            TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            return false;
        }
    }
    BLOCKED_US.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
    true
}

/// mprotect, keeping the pages --report-guard holds inaccessible (metal/report_guard.rs; signal-safe).
unsafe fn set_prot(addr: *mut libc::c_void, len: usize, prot: i32) -> i32 {
    let r = libc::mprotect(addr, len, prot);
    super::report_guard::reprotect_held(addr as usize, len);
    super::tex_backing::disc_reprotect(addr as usize, len);
    r
}

/// Where the engine writes host address `a` (GPU results it writes in the GPU's place: counter reports, buffer
/// clears): through a writable alias of the page while this guard runs (it may hold the page read-only), else `a`.
/// On the Switch the GPU writes such memory regardless of the CPU's page protection; writing the page itself here
/// faulted the engine (a report on a page that also holds zero-copy uniforms).
pub(super) fn engine_write_ptr(a: usize) -> usize {
    let page = a & !(PAGE - 1);
    // Always the alias while the guard runs: `holds` answers for the active table only, while a page protected for the
    // submit before (still running on the GPU) is read-only too.
    if !enabled() {
        return a;
    }
    static ALIASES: Mutex<Option<std::collections::HashMap<usize, usize>>> = Mutex::new(None);
    let mut g = ALIASES.lock().unwrap();
    let m = g.get_or_insert_with(Default::default);
    let alias = *m.entry(page).or_insert_with(|| unsafe {
        // The new mapping starts with the page's current protection (read-only while held): open it.
        let alias = super::report_guard::make_alias(page);
        if alias != 0 {
            let r = libc::mprotect(alias as *mut libc::c_void, PAGE, libc::PROT_READ | libc::PROT_WRITE);
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if r != 0 && N.fetch_add(1, Ordering::Relaxed) < 3 {
                println!("[ef-guard] engine write alias of page {:#x} at {:#x}: mprotect -> {} (errno {})", page, alias, r, std::io::Error::last_os_error());
            }
        }
        alias
    });
    if alias == 0 {
        a
    } else {
        alias + (a & (PAGE - 1))
    }
}

/// Whether the guard holds `page` read-only now (report_guard opens such a page read-only, not writable).
pub(super) fn holds(page: usize) -> bool {
    if !enabled() {
        return false;
    }
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    find(t, page).is_some_and(|i| t.released[i].load(Ordering::Acquire) == 0)
}

// ---- recorded ranges ----------------------------------------------------------------------------------------------

/// Zero-copy ranges bound since they were last collected: (page start, page end, serial, GPU writes).
static RECORDS: Mutex<Vec<(usize, usize, u64, bool)>> = Mutex::new(Vec::new());

fn record(addr: usize, len: usize, write: bool) -> Option<(usize, usize)> {
    if addr < 0x10000 || len == 0 {
        return None;
    }
    let (a, b) = (addr & !(PAGE - 1), (addr + len + PAGE - 1) & !(PAGE - 1));
    let serial = super::submitted_serial() + 1;
    let mut r = RECORDS.lock().unwrap();
    // Consecutive binds of one buffer (every draw of a pass) collapse into one record.
    if let Some(last) = r.last_mut() {
        if last.2 == serial && last.3 == write && a <= last.1 && b >= last.0 {
            last.0 = last.0.min(a);
            last.1 = last.1.max(b);
            return Some((a, b));
        }
    }
    r.push((a, b, serial, write));
    Some((a, b))
}

/// GPU work being encoded reads host memory [addr, addr+len) through a zero-copy buffer.
pub(super) fn note_read(addr: usize, len: usize) {
    if enabled() {
        record(addr, len, false);
    }
}

/// GPU work being encoded may write host memory [addr, addr+len) through a zero-copy buffer: those pages must not be
/// read-only when it runs, so any held now are released (the game's own writes there go unguarded, as GPU results).
pub(super) fn note_write(addr: usize, len: usize) {
    if !enabled() {
        return;
    }
    let Some((a, b)) = record(addr, len, true) else {
        return;
    };
    let _g = RECORDS.lock().unwrap(); // against a rebuild protecting these pages concurrently
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    let n = t.n.load(Ordering::Acquire);
    for i in first_slot(t, n, a)..n {
        let (s, e) = (
            t.start[i].load(Ordering::Acquire),
            t.end[i].load(Ordering::Acquire),
        );
        if s >= b {
            break;
        }
        // Only the pages it writes: a fault elsewhere in the range still finds the slot and waits (then unprotects all).
        if e > a && t.released[i].load(Ordering::Acquire) == 0 {
            let (x, y) = (s.max(a), e.min(b));
            unsafe {
                set_prot(
                    x as *mut libc::c_void,
                    y - x,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
            GPU_WRITE_RELEASES.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ---- protected ranges ---------------------------------------------------------------------------------------------

/// Sorted, disjoint protected ranges; the handler searches the active table lock-free. Two tables: a fault raised on
/// the previous generation's ranges (unprotected at the rebuild) is still recognised and retried.
struct Table {
    n: AtomicUsize,
    start: [AtomicUsize; SLOTS],
    end: [AtomicUsize; SLOTS],
    serial: [AtomicU64; SLOTS],
    released: [AtomicU8; SLOTS],
}

#[allow(clippy::declare_interior_mutable_const)]
const TABLE: Table = Table {
    n: AtomicUsize::new(0),
    start: [const { AtomicUsize::new(0) }; SLOTS],
    end: [const { AtomicUsize::new(0) }; SLOTS],
    serial: [const { AtomicU64::new(0) }; SLOTS],
    released: [const { AtomicU8::new(0) }; SLOTS],
};
static TABLES: [Table; 2] = [TABLE, TABLE];
/// Generation counter; the active table is TABLES[ACTIVE & 1].
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// First slot whose range ends after `addr`.
fn first_slot(t: &Table, n: usize, addr: usize) -> usize {
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if t.end[mid].load(Ordering::Acquire) <= addr {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn find(t: &Table, addr: usize) -> Option<usize> {
    let n = t.n.load(Ordering::Acquire).min(SLOTS);
    let i = first_slot(t, n, addr);
    (i < n && t.start[i].load(Ordering::Acquire) <= addr).then_some(i)
}

/// Waits for slot `i` of the generation `gen` table and makes its range writable (unless a rebuild already did).
unsafe fn release_slot(gen: usize, i: usize) {
    let t = &TABLES[gen & 1];
    if t.released[i].load(Ordering::Acquire) != 0 {
        return;
    }
    let (s, e, serial) = (
        t.start[i].load(Ordering::Acquire),
        t.end[i].load(Ordering::Acquire),
        t.serial[i].load(Ordering::Acquire),
    );
    wait_done(serial);
    if ACTIVE.load(Ordering::Acquire) != gen {
        return; // rebuilt meanwhile: that unprotected this generation's ranges
    }
    if t.released[i].swap(1, Ordering::AcqRel) == 0 {
        set_prot(
            s as *mut libc::c_void,
            e - s,
            libc::PROT_READ | libc::PROT_WRITE,
        );
    }
}

/// Signal-handler hook: a fault at `addr` on a page held here. Blocks until the GPU work reading it completed, makes
/// the range writable, returns true (retry the access).
pub(super) unsafe fn fault(addr: usize) -> bool {
    if !enabled() {
        return false;
    }
    let gen = ACTIVE.load(Ordering::Acquire);
    if let Some(i) = find(&TABLES[gen & 1], addr) {
        if TABLES[gen & 1].released[i].load(Ordering::Acquire) == 0 {
            FAULTS.fetch_add(1, Ordering::Relaxed);
        }
        release_slot(gen, i);
        return true;
    }
    // A range of the previous generation: unprotected by the rebuild (or about to be); retry.
    find(&TABLES[(gen + 1) & 1], addr).is_some()
}

/// Before the host kernel writes [addr, addr+len) (file reads into guest buffers; it cannot fault, it fails): wait for
/// and release the held ranges there.
pub(super) unsafe fn before_host_write(addr: usize, len: usize) {
    if !enabled() || len == 0 {
        return;
    }
    let gen = ACTIVE.load(Ordering::Acquire);
    let t = &TABLES[gen & 1];
    let n = t.n.load(Ordering::Acquire).min(SLOTS);
    for i in first_slot(t, n, addr)..n {
        if t.start[i].load(Ordering::Acquire) >= addr + len {
            break;
        }
        if t.released[i].load(Ordering::Acquire) == 0 {
            HOST_WAITS.fetch_add(1, Ordering::Relaxed);
            release_slot(gen, i);
        }
    }
}

/// Guest-main's frame fence `fence` returned early: release the previous ranges (their work completed: at most one
/// frame overlaps) and protect what command buffers up to `fence` that are still running read.
pub(super) fn protect(fence: u64) {
    if !enabled() {
        return;
    }
    let t0 = std::time::Instant::now();
    let mut records = RECORDS.lock().unwrap();
    let completed = done();
    // 1. The previous generation: unprotect what is still held.
    let gen = ACTIVE.load(Ordering::Acquire);
    let old = &TABLES[gen & 1];
    let n_old = old.n.load(Ordering::Acquire);
    for i in 0..n_old {
        if old.released[i].swap(1, Ordering::AcqRel) == 0 {
            let (s, e) = (
                old.start[i].load(Ordering::Acquire),
                old.end[i].load(Ordering::Acquire),
            );
            unsafe {
                set_prot(
                    s as *mut libc::c_void,
                    e - s,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
        }
    }
    // 2. Collect: reads of still-running work up to the fence; writes of any work not yet complete (later work too).
    records.retain(|r| r.2 > completed);
    let mut reads: Vec<(usize, usize, u64)> = records
        .iter()
        .filter(|r| !r.3 && r.2 <= fence)
        .map(|r| (r.0, r.1, r.2))
        .collect();
    let mut writes: Vec<(usize, usize)> =
        records.iter().filter(|r| r.3).map(|r| (r.0, r.1)).collect();
    records.retain(|r| r.3 || r.2 > fence);
    reads.sort_unstable();
    writes.sort_unstable();
    let mut merged: Vec<(usize, usize, u64)> = Vec::with_capacity(reads.len());
    // Only overlapping ranges merge (page granularity): neighbours keep their own serial, so a write waits only for the
    // command buffer that reads its pages.
    for (a, b, s) in reads {
        match merged.last_mut() {
            Some(m) if a < m.1 => {
                m.1 = m.1.max(b);
                m.2 = m.2.max(s);
            }
            _ => merged.push((a, b, s)),
        }
    }
    // 3. Minus the pages GPU work writes.
    let mut ranges: Vec<(usize, usize, u64)> = Vec::with_capacity(merged.len());
    let mut wi = 0;
    for (mut a, b, s) in merged {
        while wi < writes.len() && writes[wi].1 <= a {
            wi += 1;
        }
        let mut k = wi;
        while a < b {
            if k < writes.len() && writes[k].0 < b {
                let (wa, wb) = writes[k];
                if wa > a {
                    ranges.push((a, wa, s));
                }
                a = a.max(wb);
                k += 1;
            } else {
                ranges.push((a, b, s));
                break;
            }
        }
    }
    if ranges.len() > SLOTS {
        OVERFLOW.fetch_add(1, Ordering::Relaxed);
        ranges.truncate(SLOTS); // the rest stays unguarded this frame
    }
    // 4. Publish the new table, then protect (a fault on a new range must find it).
    let next = gen + 1;
    let t = &TABLES[next & 1];
    t.n.store(0, Ordering::Release);
    for (i, &(a, b, s)) in ranges.iter().enumerate() {
        t.start[i].store(a, Ordering::Relaxed);
        t.end[i].store(b, Ordering::Relaxed);
        t.serial[i].store(s, Ordering::Relaxed);
        t.released[i].store(0, Ordering::Relaxed);
    }
    t.n.store(ranges.len(), Ordering::Release);
    ACTIVE.store(next, Ordering::Release);
    let mut pages = 0;
    for (i, &(a, b, _)) in ranges.iter().enumerate() {
        // Write-tracked texture pages here: dirty_track's handler would unprotect them without waiting.
        crate::guest::dirty_track::release_for_gpu(a, b - a);
        if unsafe { set_prot(a as *mut libc::c_void, b - a, libc::PROT_READ) } != 0 {
            t.released[i].store(1, Ordering::Release);
        }
        pages += (b - a) / PAGE;
    }
    drop(records);
    PROTECTS.fetch_add(1, Ordering::Relaxed);
    RANGES.fetch_add(ranges.len() as u64, Ordering::Relaxed);
    PAGES.fetch_add(pages as u64, Ordering::Relaxed);
    PROTECT_US.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
    log_stats();
}

/// The fence waited fully: nothing of earlier frames runs any more; release whatever is still held.
pub(super) fn release_all() {
    if !enabled() {
        return;
    }
    let _records = RECORDS.lock().unwrap();
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    for i in 0..t.n.load(Ordering::Acquire) {
        if t.released[i].swap(1, Ordering::AcqRel) == 0 {
            let (s, e) = (
                t.start[i].load(Ordering::Acquire),
                t.end[i].load(Ordering::Acquire),
            );
            unsafe {
                set_prot(
                    s as *mut libc::c_void,
                    e - s,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
        }
    }
}

// ---- statistics (--log-early-fence) ----------------------------------------------------------------------------

static PROTECTS: AtomicU64 = AtomicU64::new(0);
static RANGES: AtomicU64 = AtomicU64::new(0);
static PAGES: AtomicU64 = AtomicU64::new(0);
static PROTECT_US: AtomicU64 = AtomicU64::new(0);
static FAULTS: AtomicU64 = AtomicU64::new(0);
static BLOCKED_US: AtomicU64 = AtomicU64::new(0);
static HOST_WAITS: AtomicU64 = AtomicU64::new(0);
static GPU_WRITE_RELEASES: AtomicU64 = AtomicU64::new(0);
static TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static OVERFLOW: AtomicU64 = AtomicU64::new(0);

fn log_stats() {
    static LOG: OnceLock<bool> = OnceLock::new();
    if !*LOG.get_or_init(|| crate::settings::get().log.log_early_fence) {
        return;
    }
    let n = PROTECTS.load(Ordering::Relaxed);
    if n % 300 != 0 {
        return;
    }
    let take = |a: &AtomicU64| a.swap(0, Ordering::Relaxed);
    let (r, p, us, f, b) = (
        take(&RANGES),
        take(&PAGES),
        take(&PROTECT_US),
        take(&FAULTS),
        take(&BLOCKED_US),
    );
    println!(
        "[ef-guard] last 300 early fences: {:.0} ranges / {:.0} pages protected per frame ({:.0} us), {:.1} blocked writes per frame \
         ({:.2} ms blocked per frame), host-write waits {}, GPU-write releases {}, timeouts {}, overflows {}",
        r as f64 / 300.0, p as f64 / 300.0, us as f64 / 300.0, f as f64 / 300.0, b as f64 / 300_000.0,
        take(&HOST_WAITS), take(&GPU_WRITE_RELEASES), take(&TIMEOUTS), take(&OVERFLOW)
    );
}
