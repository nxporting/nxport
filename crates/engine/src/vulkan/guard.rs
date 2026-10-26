// Frame overlap (--early-fence): guest-main's frame fence returns once the frame is queued, so the game prepares the
// next frame while the GPU runs this one. Draws read vertex, index and buffer data (and uniforms GPU work writes) from
// guest memory in place (imported pools), and the game writes the next frame's data into memory the running frame still reads (the fence
// would have held it back on the Switch). So once guest-main waits for the frame, the pages still-running submits read
// in place are read-only; a write to one faults, waits for the submit that reads it and makes the range writable again,
// and the write is retried (as the Metal backend's write guard does). Not earlier: TotK's other threads finish the
// frame's HUD and text after the submit, and the GPU must see that.
//
// Uniform data is copied when recorded (only the extent each stage reads), never read in place, as on Metal: in place,
// the game's next-frame writes to those pages waited for the GPU for milliseconds per frame. Vertex, index and buffer
// data stay in place. Copies of all of them (pages the game had written while GPU work read them, "hot") drew HUD and
// inventory text from other strings (the "L" key icon as a digit, wrong item counts), and holding every submit's pages
// from the submit on drew UI elements scaled up (a screen-sized apple icon) when the inventory opened.
//
// A page the game wrote while GPU work read it is hot. The recording's copies must hold what such pages held when the
// submit was made, but recording takes milliseconds and the game's other threads go on writing meanwhile (TotK swimming:
// per-frame constants copied half rewritten blended whole frames to cyan). So hot pages are read-only while a submit is
// recorded: the first write to one saves the page's contents (still the submit's) and goes ahead, and the recording's
// copies of guest memory take the saved pages (patch). The backend's own writes (reports, inline updates) are not saved.
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

/// A submit is being recorded with the hot pages read-only (hold_hot).
static HOT_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static HOT_RANGES: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
const SAVED_MAX: usize = 1024;
const SAVING: usize = 1 << 63;
static SAVED_KEYS: [AtomicUsize; SAVED_MAX] = [const { AtomicUsize::new(0) }; SAVED_MAX];
static SAVED_N: AtomicUsize = AtomicUsize::new(0);
static SAVED_POOL: std::sync::atomic::AtomicPtr<u8> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static SAVE_LOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local! {
    static RECORDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Marks this thread as one of the backend's own (its writes to held hot pages are not saved).
pub(super) fn engine_thread() {
    RECORDER.with(|r| r.set(true));
}

/// Before a submit is recorded: hot pages become read-only until release_hot.
pub(super) fn hold_hot() {
    if !enabled() || !ANY_HOT.load(Ordering::Acquire) {
        return;
    }
    let mut pages: Vec<usize> = HOT.iter().filter_map(|h| h.load(Ordering::Acquire).checked_sub(1)).collect();
    pages.sort_unstable();
    let mut ranges = HOT_RANGES.lock().unwrap();
    ranges.clear();
    for p in pages {
        match ranges.last_mut() {
            Some(r) if r.1 == p * PAGE => r.1 += PAGE,
            _ => ranges.push((p * PAGE, (p + 1) * PAGE)),
        }
    }
    engine_thread();
    if SAVED_POOL.load(Ordering::Acquire).is_null() {
        let layout = std::alloc::Layout::from_size_align(SAVED_MAX * PAGE, PAGE).unwrap();
        SAVED_POOL.store(unsafe { std::alloc::alloc(layout) }, Ordering::Release);
    }
    SAVED_N.store(0, Ordering::Release);
    HOT_HELD.store(true, Ordering::Release);
    for &(a, b) in ranges.iter() {
        let _ = unsafe { crate::os::protect(a, b - a, crate::os::Prot::Read) };
    }
}

/// The submit is recorded: hot pages are writable again (write tracking keeps the pages it holds).
pub(super) fn release_hot() {
    if !HOT_HELD.load(Ordering::Acquire) {
        return;
    }
    let ranges = HOT_RANGES.lock().unwrap();
    for &(a, b) in ranges.iter() {
        unsafe { set_prot(a, b - a, true) };
    }
    HOT_HELD.store(false, Ordering::Release);
}

/// Saves held page `page` (once) before its first write in this recording; false when there is no room.
unsafe fn save_page(page: usize) -> bool {
    let key = page / PAGE + 1;
    while SAVE_LOCK.swap(true, Ordering::Acquire) {
        std::hint::spin_loop();
    }
    let n = SAVED_N.load(Ordering::Acquire);
    let done = SAVED_KEYS[..n].iter().any(|k| k.load(Ordering::Acquire) & !SAVING == key);
    let ok = if done {
        true
    } else if n < SAVED_MAX {
        let slot = SAVED_POOL.load(Ordering::Acquire).add(n * PAGE);
        SAVED_KEYS[n].store(key | SAVING, Ordering::Release);
        SAVED_N.store(n + 1, Ordering::Release);
        std::ptr::copy_nonoverlapping(page as *const u8, slot, PAGE);
        SAVED_KEYS[n].store(key, Ordering::Release);
        true
    } else {
        false
    };
    SAVE_LOCK.store(false, Ordering::Release);
    ok
}

/// A fault on a hot page (held during recording, or released since the access faulted).
unsafe fn hot_fault(addr: usize) -> bool {
    if !hot(addr, 1) {
        return false;
    }
    let page = addr & !(PAGE - 1);
    if RECORDER.with(|r| r.get()) || !HOT_HELD.load(Ordering::Acquire) || save_page(page) {
        set_prot(page, PAGE, true);
        return true;
    }
    let t0 = std::time::Instant::now();
    while HOT_HELD.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_micros(20));
        if t0.elapsed().as_secs() >= 2 {
            TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            break;
        }
    }
    BLOCKED_US.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
    set_prot(page, PAGE, true);
    true
}

/// A copy of guest memory [a, a+len) made at `dst` during a recording: parts on pages saved since the recording
/// began are replaced by the saved contents.
pub(super) unsafe fn patch(a: usize, dst: *mut u8, len: usize) {
    if !HOT_HELD.load(Ordering::Acquire) || len == 0 {
        return;
    }
    std::sync::atomic::fence(Ordering::SeqCst);
    let n = SAVED_N.load(Ordering::Acquire);
    if n == 0 {
        return;
    }
    let pool = SAVED_POOL.load(Ordering::Acquire);
    for i in 0..n {
        let mut k = SAVED_KEYS[i].load(Ordering::Acquire);
        while k & SAVING != 0 {
            std::hint::spin_loop();
            k = SAVED_KEYS[i].load(Ordering::Acquire);
        }
        let p = (k - 1) * PAGE;
        let (x, y) = (p.max(a), (p + PAGE).min(a + len));
        if x < y {
            std::ptr::copy_nonoverlapping(pool.add(i * PAGE + (x - p)), dst.add(x - a), y - x);
        }
    }
}

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
    // A held hot page is saved before anything makes it writable (it may also be held for in-place reads).
    if HOT_HELD.load(Ordering::Acquire) && hot(addr, 1) && !RECORDER.with(|r| r.get()) {
        save_page(addr & !(PAGE - 1));
    }
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
    if hot_fault(addr) {
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

/// Copies GPU results back to guest memory (writebacks, on the in-flight watcher thread) without waiting on the pages
/// this guard holds: a held page is made writable around the copy. The bytes written are what GPU work wrote, which no
/// running submit reads in place (they read the shadow, shadow.rs); a write that faulted would wait for submits only
/// this thread sees complete.
pub(super) unsafe fn write_back(addr: usize, src: *const u8, len: usize) {
    let mut at = addr;
    while at < addr + len {
        let page = at & !(PAGE - 1);
        let n = (page + PAGE).min(addr + len) - at;
        let held = enabled() && holds(at);
        if held {
            let _ = crate::os::protect(page, PAGE, crate::os::Prot::ReadWrite);
        }
        std::ptr::copy_nonoverlapping(src.add(at - addr), at as *mut u8, n);
        if held {
            let _ = crate::os::protect(page, PAGE, crate::os::Prot::Read);
        }
        at += n;
    }
}

/// Whether a page is held now (crash reports).
pub(super) fn holds(addr: usize) -> bool {
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    find(t, addr).is_some_and(|i| t.released[i].load(Ordering::Acquire) == 0)
}

/// After submit `fence` (and when guest-main's wait for it returns early): release the previous ranges and protect
/// what submits up to `fence` that are still running read.
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
