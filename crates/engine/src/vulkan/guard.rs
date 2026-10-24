// Frame overlap (--early-fence): guest-main's frame fence returns once the frame is queued, so the game prepares the
// next frame while the GPU runs this one. Draws read vertex, index and some uniform data from guest memory in place
// (imported pools), and the game writes the next frame's data into memory the running frame still reads (the fence
// would have held it back on the Switch). So when a fence returns early, the pages that still-running submits read in
// place become read-only; a write to one faults, waits for the submit that reads it and makes the range writable
// again, and the write is retried (as the Metal backend's write guard does).
//
// A page the game wrote that way is hot: later submits copy what they read there when they are recorded (what the GPU
// would read, since on the Switch the game writes only after the fence) instead of reading it in place, so the game
// stops waiting for whole frames.
//
// Pages GPU work writes (storage buffers, copy destinations, report slots) are never protected: GPU writes cannot
// fault, and the watcher writes reports and copied buffers into them. The game reads such results a frame late.

use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const PAGE: usize = 0x4000;
const SLOTS: usize = 8192;

pub(super) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let s = crate::settings::get();
        s.graphics.early_fence && s.graphics.early_fence_guard
    })
}

/// Serial of the submit being recorded (records name it).
pub(super) static RECORDING: AtomicU64 = AtomicU64::new(0);

/// Blocks (signal-safe: atomics and sleeps) until `serial` completed; false after 2 s (the access then goes ahead
/// rather than hang the game).
fn wait_done(serial: u64) -> bool {
    let t0 = std::time::Instant::now();
    let mut spins = 0u32;
    while super::inflight::completed() < serial && !super::inflight::LOST.load(Ordering::Acquire) {
        if spins < 64 {
            std::hint::spin_loop();
            spins += 1;
            continue;
        }
        std::thread::sleep(std::time::Duration::from_micros(20));
        if t0.elapsed().as_secs() >= 2 {
            TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            return false;
        }
    }
    BLOCKED_US.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
    true
}

/// Protection as this guard needs it; pages the write tracking (guest::dirty_track) holds stay read-only.
unsafe fn set_prot(a: usize, len: usize, write: bool) -> bool {
    let prot = if write { crate::os::Prot::ReadWrite } else { crate::os::Prot::Read };
    let ok = crate::os::protect(a, len, prot).is_ok();
    if write {
        crate::guest::dirty_track::reprotect_held(a, len);
    }
    ok
}

// ---- hot pages ----------------------------------------------------------------------------------------------------

const HOT_SLOTS: usize = 1 << 14;
/// Open-addressed set of pages (page number + 1; 0 = empty), filled by the fault handler (no allocation there).
static HOT: [AtomicUsize; HOT_SLOTS] = [const { AtomicUsize::new(0) }; HOT_SLOTS];
static ANY_HOT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn hot_slot(page: usize) -> usize {
    (page.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) % HOT_SLOTS
}

fn mark_hot(addr: usize) {
    let key = addr / PAGE + 1;
    let start = hot_slot(key);
    for k in 0..16 {
        let slot = &HOT[(start + k) % HOT_SLOTS];
        match slot.compare_exchange(0, key, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(v) if v == key => break,
            Err(_) => {}
        }
    }
    ANY_HOT.store(true, Ordering::Release);
}

/// Whether the game wrote any page of [a, a+len) while GPU work read it in place.
pub(super) fn hot(a: usize, len: usize) -> bool {
    if !ANY_HOT.load(Ordering::Acquire) || len == 0 {
        return false;
    }
    (a / PAGE..=(a + len - 1) / PAGE).any(|p| {
        let key = p + 1;
        let start = hot_slot(key);
        for k in 0..16 {
            match HOT[(start + k) % HOT_SLOTS].load(Ordering::Acquire) {
                0 => return false,
                v if v == key => return true,
                _ => {}
            }
        }
        false
    })
}

// ---- recorded ranges ----------------------------------------------------------------------------------------------

/// Ranges submits use in place since they were last collected: (page start, page end, serial, GPU writes).
static RECORDS: Mutex<Vec<(usize, usize, u64, bool)>> = Mutex::new(Vec::new());

fn record(a: usize, len: usize, write: bool) -> Option<(usize, usize)> {
    if a < 0x10000 || len == 0 {
        return None;
    }
    let (a, b) = (a & !(PAGE - 1), (a + len + PAGE - 1) & !(PAGE - 1));
    let serial = RECORDING.load(Ordering::Relaxed);
    let mut r = RECORDS.lock().unwrap();
    // Consecutive uses of one buffer (every draw of a pass) collapse into one record.
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

/// The submit being recorded reads host memory [a, a+len) in place.
pub(super) fn note_read(a: usize, len: usize) {
    if enabled() {
        record(a, len, false);
    }
}

/// The submit being recorded (or the watcher, for it) writes host memory [a, a+len): pages held now are released.
pub(super) fn note_write(a: usize, len: usize) {
    if !enabled() {
        return;
    }
    let Some((a, b)) = record(a, len, true) else { return };
    let _g = RECORDS.lock().unwrap(); // against a rebuild protecting these pages concurrently
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    let n = t.n.load(Ordering::Acquire);
    for i in first_slot(t, n, a)..n {
        let (s, e) = (t.start[i].load(Ordering::Acquire), t.end[i].load(Ordering::Acquire));
        if s >= b {
            break;
        }
        if e > a && t.released[i].load(Ordering::Acquire) == 0 {
            let (x, y) = (s.max(a), e.min(b));
            unsafe { set_prot(x, y - x, true) };
        }
    }
}

// ---- protected ranges ---------------------------------------------------------------------------------------------

/// Sorted, disjoint protected ranges; the fault handler searches the active table lock-free. Two tables: a fault on the
/// previous generation's ranges (unprotected at the rebuild) is still recognised and retried.
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

/// Waits for slot `i` of generation `gen` and makes its range writable (unless a rebuild already did).
unsafe fn release_slot(gen: usize, i: usize) {
    let t = &TABLES[gen & 1];
    if t.released[i].load(Ordering::Acquire) != 0 {
        return;
    }
    let (s, e, serial) = (t.start[i].load(Ordering::Acquire), t.end[i].load(Ordering::Acquire), t.serial[i].load(Ordering::Acquire));
    wait_done(serial);
    if ACTIVE.load(Ordering::Acquire) != gen {
        return; // rebuilt meanwhile: that unprotected this generation's ranges
    }
    if t.released[i].swap(1, Ordering::AcqRel) == 0 {
        set_prot(s, e - s, true);
    }
}

/// Signal-handler hook: a fault at `addr`. True if it is a page held here (after waiting for the GPU work reading it
/// and making it writable): the access is retried.
pub(super) unsafe fn fault(addr: usize) -> bool {
    if !enabled() {
        return false;
    }
    let gen = ACTIVE.load(Ordering::Acquire);
    if let Some(i) = find(&TABLES[gen & 1], addr) {
        if TABLES[gen & 1].released[i].load(Ordering::Acquire) == 0 {
            FAULTS.fetch_add(1, Ordering::Relaxed);
        }
        mark_hot(addr);
        release_slot(gen, i);
        // A page the write tracking holds too: the write is its to record (and its handler makes the page writable).
        crate::guest::dirty_track::handle_fault(addr);
        return true;
    }
    if find(&TABLES[(gen + 1) & 1], addr).is_some() {
        crate::guest::dirty_track::handle_fault(addr);
        return true;
    }
    false
}

/// Before the host kernel writes [addr, addr+len) (it cannot fault, it fails): wait for and release held ranges there.
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
        release_slot(gen, i);
    }
}

/// Write tracking made [addr, addr+len) writable: the ranges held here become read-only again.
pub(super) unsafe fn reprotect_held(addr: usize, len: usize) {
    if !enabled() {
        return;
    }
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    let n = t.n.load(Ordering::Acquire).min(SLOTS);
    for i in first_slot(t, n, addr)..n {
        let (s, e) = (t.start[i].load(Ordering::Acquire), t.end[i].load(Ordering::Acquire));
        if s >= addr + len {
            break;
        }
        if t.released[i].load(Ordering::Acquire) == 0 {
            let (x, y) = (s.max(addr & !(PAGE - 1)), e.min((addr + len + PAGE - 1) & !(PAGE - 1)));
            if x < y {
                let _ = crate::os::protect(x, y - x, crate::os::Prot::Read);
            }
        }
    }
}

/// Whether a page is held now (crash reports).
pub(super) fn holds(addr: usize) -> bool {
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    find(t, addr).is_some_and(|i| t.released[i].load(Ordering::Acquire) == 0)
}

/// Guest-main's fence `fence` returned early: release the previous ranges (their work completed: at most one frame
/// overlaps) and protect what submits up to `fence` that are still running read.
pub(super) fn protect(fence: u64) {
    if !enabled() {
        return;
    }
    let t0 = std::time::Instant::now();
    let mut records = RECORDS.lock().unwrap();
    let completed = super::inflight::completed();
    let gen = ACTIVE.load(Ordering::Acquire);
    let old = &TABLES[gen & 1];
    for i in 0..old.n.load(Ordering::Acquire) {
        if old.released[i].swap(1, Ordering::AcqRel) == 0 {
            let (s, e) = (old.start[i].load(Ordering::Acquire), old.end[i].load(Ordering::Acquire));
            unsafe { set_prot(s, e - s, true) };
        }
    }
    // Reads of still-running work up to the fence; writes of any work not yet complete (later work too).
    records.retain(|r| r.2 > completed);
    let mut reads: Vec<(usize, usize, u64)> = records.iter().filter(|r| !r.3 && r.2 <= fence).map(|r| (r.0, r.1, r.2)).collect();
    let mut writes: Vec<(usize, usize)> = records.iter().filter(|r| r.3).map(|r| (r.0, r.1)).collect();
    records.retain(|r| r.3 || r.2 > fence);
    reads.sort_unstable();
    writes.sort_unstable();
    let mut merged: Vec<(usize, usize, u64)> = Vec::with_capacity(reads.len());
    for (a, b, s) in reads {
        match merged.last_mut() {
            Some(m) if a < m.1 => {
                m.1 = m.1.max(b);
                m.2 = m.2.max(s);
            }
            _ => merged.push((a, b, s)),
        }
    }
    // Minus the pages GPU work writes.
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
    ranges.truncate(SLOTS);
    // Publish the new table, then protect (a fault on a new range must find it).
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
        if !unsafe { set_prot(a, b - a, false) } {
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

/// The fence waited fully: release whatever is still held.
pub(super) fn release_all() {
    if !enabled() {
        return;
    }
    let _records = RECORDS.lock().unwrap();
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    for i in 0..t.n.load(Ordering::Acquire) {
        if t.released[i].swap(1, Ordering::AcqRel) == 0 {
            let (s, e) = (t.start[i].load(Ordering::Acquire), t.end[i].load(Ordering::Acquire));
            unsafe { set_prot(s, e - s, true) };
        }
    }
}

// ---- statistics (--log-early-fence) -------------------------------------------------------------------------------

static PROTECTS: AtomicU64 = AtomicU64::new(0);
static RANGES: AtomicU64 = AtomicU64::new(0);
static PAGES: AtomicU64 = AtomicU64::new(0);
static PROTECT_US: AtomicU64 = AtomicU64::new(0);
static FAULTS: AtomicU64 = AtomicU64::new(0);
static BLOCKED_US: AtomicU64 = AtomicU64::new(0);
static TIMEOUTS: AtomicU64 = AtomicU64::new(0);

fn log_stats() {
    static LOG: OnceLock<bool> = OnceLock::new();
    if !*LOG.get_or_init(|| crate::settings::get().log.log_early_fence) || PROTECTS.load(Ordering::Relaxed) % 300 != 0 {
        return;
    }
    let take = |a: &AtomicU64| a.swap(0, Ordering::Relaxed) as f64 / 300.0;
    println!(
        "[vk-guard] last 300 early fences: {:.0} ranges / {:.0} pages protected per frame ({:.0} us), {:.1} blocked writes per frame \
         ({:.2} ms blocked per frame), timeouts {}",
        take(&RANGES),
        take(&PAGES),
        take(&PROTECT_US),
        take(&FAULTS),
        take(&BLOCKED_US) / 1000.0,
        TIMEOUTS.swap(0, Ordering::Relaxed)
    );
}
