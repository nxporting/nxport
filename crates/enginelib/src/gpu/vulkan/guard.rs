// Frame overlap (--early-fence): guest-main's frame fence returns once the frame is queued, so the game prepares the
// next frame while the GPU runs this one. Draws read vertex, index and buffer data (and uniforms GPU work writes) from
// guest memory in place (imported pools), and the game writes the next frame's data into memory the running frame still reads (the fence
// would have held it back on the Switch). So once guest-main waits for the frame, the pages still-running submits read
// in place are read-only; a write to one faults, waits for the submit that reads it and makes the range writable again,
// and the write is retried (as the Metal backend's write guard does). Not earlier: TotK's other threads finish the
// frame's HUD and text after the submit, and the GPU must see that.
//
// Uniform data is copied when recorded (only the extent each stage reads), never read in place, as on Metal: in place,
// the game's next-frame writes to those pages would wait for the GPU for milliseconds per frame. Vertex, index and buffer
// data stay in place. Copies of all of them (pages the game had written while GPU work read them, "hot") would draw HUD
// and inventory text from other strings, and holding every submit's pages from the submit on would draw UI elements
// scaled up when the inventory opens.
//
// A page the game wrote while GPU work read it is hot. The recording's copies must hold what such pages held when the
// submit was made, but recording takes milliseconds and the game's other threads go on writing meanwhile (per-frame
// constants would be copied half rewritten). So hot pages are read-only while a submit is
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
    // Never read-only under a live import (dirty_track::imported): the caller treats the range as not held.
    if !write && crate::guest::dirty_track::imported(a, len) {
        return false;
    }
    let prot = if write { crate::platform::os::Prot::ReadWrite } else { crate::platform::os::Prot::Read };
    let ok = crate::platform::os::protect(a, len, prot).is_ok();
    if write {
        crate::guest::dirty_track::reprotect_held(a, len);
    }
    // Occlusion report pages stay inaccessible until written, whatever this does to the range around them.
    crate::gpu::report_guard::reprotect_held(a, len);
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
    static WATCHER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The watcher's last faulting page and how many times in a row (fault).
    static REPEATS: std::cell::Cell<(usize, u32)> = const { std::cell::Cell::new((0, 0)) };
}
static STUCK_REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Marks this thread as the one that completes submits (inflight.rs): it never waits for a submit to complete.
pub(super) fn watcher_thread() {
    engine_thread();
    WATCHER.with(|w| w.set(true));
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
        if crate::guest::dirty_track::imported(a, b - a) {
            continue;
        }
        unsafe {
            let _ = crate::platform::os::protect(a, b - a, crate::platform::os::Prot::Read);
            crate::gpu::report_guard::reprotect_held(a, b - a);
        }
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
    if trap_on() && a >= 0x10000 && len > 0 {
        let serial = RECORDING.load(Ordering::Relaxed);
        let mut x = EXACT.lock().unwrap();
        if x.last() != Some(&(a, a + len, serial)) {
            x.push((a, a + len, serial));
        }
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
    // The watcher writing back an earlier submit's results cannot wait for a later one (it completes them itself: each
    // such fault would wait out the 2 s timeout).
    if !WATCHER.with(|w| w.get()) {
        wait_done(serial);
    }
    if ACTIVE.load(Ordering::Acquire) != gen {
        return; // rebuilt meanwhile: that unprotected this generation's ranges
    }
    if t.released[i].swap(1, Ordering::AcqRel) == 0 {
        set_prot(s, e - s, true);
    }
}

/// Signal-handler hook: a fault at `addr`. True if it is a page held here (after waiting for the GPU work reading it
/// and making it writable): the access is retried.
pub(super) unsafe fn fault(addr: usize, write: bool) -> bool {
    if trap_on() && trap_fault(addr, write) {
        return true;
    }
    let handled = fault_held(addr);
    // The watcher (write-backs) leaves with the page open whatever held it: every submit waits for it, so a page left
    // read-only under it (a race with a protect, a hot page) would fault forever and the game would stop.
    if handled && WATCHER.with(|w| w.get()) {
        set_prot(addr & !(PAGE - 1), PAGE, true);
        crate::guest::dirty_track::handle_fault(addr);
    }
    // Backstop: the watcher faulting on one page over and over (a page left read-only that nothing here holds, which the
    // write tracking takes for one already opened) opens it, once reported.
    if !handled && enabled() && WATCHER.with(|w| w.get()) {
        let page = addr & !(PAGE - 1);
        let n = REPEATS.with(|r| {
            let (p, n) = r.get();
            let n = if p == page { n + 1 } else { 1 };
            r.set((page, n));
            n
        });
        if n >= 64 {
            if !STUCK_REPORTED.swap(true, Ordering::Relaxed) {
                let gen = ACTIVE.load(Ordering::Acquire);
                eprintln!(
                    "[vk-guard] the watcher's write to {addr:#x} faulted {n} times on a page nothing holds (old table {}, hot {}, hot held {}): opened",
                    find(&TABLES[(gen + 1) & 1], addr).is_some(),
                    hot(addr, 1),
                    HOT_HELD.load(Ordering::Acquire)
                );
            }
            REPEATS.with(|r| r.set((0, 0)));
            set_prot(page, PAGE, true);
            return true;
        }
    }
    handled
}

unsafe fn fault_held(addr: usize) -> bool {
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
        // Released already, yet the page faulted: a reprotect raced the release and left it read-only. Retrying
        // without opening it would fault forever.
        let stale = TABLES[gen & 1].released[i].load(Ordering::Acquire) != 0;
        release_slot(gen, i);
        if stale && ACTIVE.load(Ordering::Acquire) == gen {
            set_prot(addr & !(PAGE - 1), PAGE, true);
        }
        // A page the write tracking holds too: the write is its to record (and its handler makes the page writable).
        crate::guest::dirty_track::handle_fault(addr);
        return true;
    }
    if find(&TABLES[(gen + 1) & 1], addr).is_some() {
        // Possibly a hot page held while a submit is recorded too: opened for the backend's threads as below. Left
        // read-only, the watcher's write-back would fault until the recording ended while the recording waits for the
        // watcher: a deadlock.
        // The previous generation's ranges were all released at the rebuild: a page of one still read-only (a reprotect
        // that raced a release) is opened, else every write to it would fault forever.
        if !hot_fault(addr) {
            set_prot(addr & !(PAGE - 1), PAGE, true);
        }
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
            if x < y && !crate::guest::dirty_track::imported(x, y - x) {
                let _ = crate::platform::os::protect(x, y - x, crate::platform::os::Prot::Read);
                // Released meanwhile (release_slot made it writable before this mprotect): writable again.
                if t.released[i].load(Ordering::Acquire) != 0 {
                    set_prot(x, y - x, true);
                }
            }
        }
    }
}

/// Copies GPU results back to guest memory (writebacks, on the in-flight watcher thread) without waiting on the pages
/// this guard holds: a held page is made writable around the copy. The bytes written are what GPU work wrote, which no
/// running submit reads in place (they read the shadow, shadow.rs); a write that faulted would wait for submits only
/// this thread sees complete.
pub(super) unsafe fn write_back(addr: usize, src: *const u8, len: usize) {
    // No table is rebuilt meanwhile (protect, release_all): a page checked as held and released by a rebuild before the
    // copy would be made read-only again here with no table holding it, and every later write to it, the watcher's and
    // the game's, would fault forever (the write tracking takes such a page for one already opened).
    let _records = RECORDS.lock().unwrap();
    let mut at = addr;
    while at < addr + len {
        let page = at & !(PAGE - 1);
        let n = (page + PAGE).min(addr + len) - at;
        let held = enabled() && holds(at);
        if held {
            let _ = crate::platform::os::protect(page, PAGE, crate::platform::os::Prot::ReadWrite);
        }
        std::ptr::copy_nonoverlapping(src.add(at - addr), at as *mut u8, n);
        // Unless a fault released it meanwhile (it stays open then, as release_slot left it). A release between the
        // check and the mprotect (release_slot marks the slot, then opens the page) is seen after it: the page is opened
        // again. Left read-only with the slot released, no rebuild would open it, and once no table held it every write
        // to it would fault forever.
        if held && holds(at) {
            let _ = crate::platform::os::protect(page, PAGE, crate::platform::os::Prot::Read);
            if !holds(at) {
                set_prot(page, PAGE, true);
            }
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
    if trap_on() {
        let pending: Vec<(usize, usize, u64)> = {
            let mut x = EXACT.lock().unwrap();
            x.retain(|r| r.2 > completed);
            x.clone()
        };
        trap_writes(&pending);
    }
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
        } else if t.released[i].load(Ordering::Acquire) != 0 {
            // A fault released it before this mprotect (a page already read-only for the write tracking or a hot
            // hold faults as soon as the table is published): open again, as release_slot left it.
            unsafe { set_prot(a, b - a, true) };
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

// ---- visibility check (--early-fence-trap) ------------------------------------------------------------------------
//
// The early fence lets the game go on while GPU work that writes guest memory (storage buffers, copy destinations,
// reports) still runs, so the game can read those results before they are there: on the console its fence would have
// held it until they were (BotW's white water: lighting values read a frame late). A test oracle for that class of
// bug: when the fence returns early, the pages such still-running work writes are made inaccessible; a guest access to
// one while the write is still running is reported once per guest caller ("[visibility]"), then the page opens without
// waiting, so the run goes on as it would have. Host writes (the watcher's write-backs) open pages silently. Diagnostic
// runs only: it changes the timing it watches, and pages shared with the write tracking lose their tracking.

const TRAP_SLOTS: usize = 1024;
static TRAP_START: [AtomicUsize; TRAP_SLOTS] = [const { AtomicUsize::new(0) }; TRAP_SLOTS];
static TRAP_END: [AtomicUsize; TRAP_SLOTS] = [const { AtomicUsize::new(0) }; TRAP_SLOTS];
static TRAP_SERIAL: [AtomicU64; TRAP_SLOTS] = [const { AtomicU64::new(0) }; TRAP_SLOTS];
static TRAP_N: AtomicUsize = AtomicUsize::new(0);
static TRAP_RACES: AtomicU64 = AtomicU64::new(0);
static TRAP_LATE: AtomicU64 = AtomicU64::new(0);
static TRAP_NEIGHBOURS: AtomicU64 = AtomicU64::new(0);
static EXACT: Mutex<Vec<(usize, usize, u64)>> = Mutex::new(Vec::new());

fn trap_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| enabled() && crate::settings::get().debug.early_fence_trap != crate::settings::FenceTrap::Off)
}

/// Opens what the last early fence trapped and traps `pending` (page ranges still-running work writes, its serial).
fn trap_writes(pending: &[(usize, usize, u64)]) {
    static FENCES: AtomicU64 = AtomicU64::new(0);
    if FENCES.fetch_add(1, Ordering::Relaxed) % 300 == 299 {
        eprintln!(
            "[visibility] last 300 early fences: {} early reads ({} stale, {} unchanged by the GPU, {} unjudged), {} after it finished, {} to other data on those pages; {} ranges trapped now",
            TRAP_RACES.swap(0, Ordering::Relaxed),
            STALE.swap(0, Ordering::Relaxed),
            SAME.swap(0, Ordering::Relaxed),
            UNJUDGED.swap(0, Ordering::Relaxed),
            TRAP_LATE.swap(0, Ordering::Relaxed),
            TRAP_NEIGHBOURS.swap(0, Ordering::Relaxed),
            pending.len().min(TRAP_SLOTS)
        );
    }
    // Open what the last early fence trapped (whole host pages around each range).
    for i in 0..TRAP_N.load(Ordering::Acquire) {
        let s = TRAP_START[i].swap(0, Ordering::AcqRel);
        if s != 0 {
            let (a, b) = host_pages(s, TRAP_END[i].load(Ordering::Acquire));
            unsafe { set_prot(a, b - a, true) };
        }
    }
    TRAP_N.store(0, Ordering::Release);
    check_reads();
    let n = pending.len().min(TRAP_SLOTS);
    for (i, &(a, b, serial)) in pending[..n].iter().enumerate() {
        TRAP_END[i].store(b, Ordering::Relaxed);
        TRAP_SERIAL[i].store(serial, Ordering::Relaxed);
        TRAP_START[i].store(a, Ordering::Release);
    }
    TRAP_N.store(n, Ordering::Release);
    for &(a, b, _) in &pending[..n] {
        let (a, b) = host_pages(a, b);
        let _ = unsafe { crate::platform::os::protect(a, b - a, crate::platform::os::Prot::None) };
    }
}

/// The host pages around [a, b).
fn host_pages(a: usize, b: usize) -> (usize, usize) {
    let p = crate::platform::os::page_size();
    (a & !(p - 1), (b + p - 1) & !(p - 1))
}

/// A fault on a trapped page. A guest read inside a range a still-running submit writes is recorded (the 8 bytes it
/// saw, the guest's pc and lr) and judged once the submit completed (check_reads); writes, the watcher's and other
/// data on the page are not. The page is then opened.
unsafe fn trap_fault(addr: usize, write: bool) -> bool {
    let n = TRAP_N.load(Ordering::Acquire);
    let mut page_hit = false;
    let mut exact = None;
    for i in 0..n {
        let s = TRAP_START[i].load(Ordering::Acquire);
        if s == 0 {
            continue;
        }
        let e = TRAP_END[i].load(Ordering::Acquire);
        let (a, b) = host_pages(s, e);
        if addr >= a && addr < b {
            page_hit = true;
            if addr >= s && addr < e && exact.is_none_or(|j: usize| TRAP_SERIAL[i].load(Ordering::Acquire) > TRAP_SERIAL[j].load(Ordering::Acquire)) {
                exact = Some(i);
            }
        }
    }
    if !page_hit {
        return false;
    }
    // The page opens for reading (a guest write to it later cancels the reads recorded on it: the game changed those
    // bytes itself, check_reads must not blame the GPU); writes open it fully.
    let p = crate::platform::os::page_size();
    let page = addr & !(p - 1);
    let watcher = WATCHER.with(|w| w.get());
    if write || watcher {
        if write && !watcher {
            for k in 0..READ_SLOTS {
                if READ_AT[k].load(Ordering::Relaxed) & !(p - 1) == page {
                    READ_SERIAL[k].store(0, Ordering::Release);
                }
            }
        }
        set_prot(page, p, true);
        return true;
    }
    let _ = crate::platform::os::protect(page, p, crate::platform::os::Prot::Read);
    match exact {
        Some(i) if super::inflight::completed() < TRAP_SERIAL[i].load(Ordering::Acquire) => {
            TRAP_RACES.fetch_add(1, Ordering::Relaxed);
            #[cfg(all(nx_runtime, not(nx_native)))]
            let (pc, lr) = {
                let c = crate::guest::exec::lifted::existing_thread_cpu();
                if c.is_null() { (0, 0) } else { ((*c).pc, (*c).x[30]) }
            };
            #[cfg(not(all(nx_runtime, not(nx_native))))]
            let (pc, lr) = (0u64, 0u64);
            let at = addr & !7;
            let k = READ_W.fetch_add(1, Ordering::AcqRel) % READ_SLOTS;
            READ_AT[k].store(at, Ordering::Relaxed);
            READ_VAL[k].store(std::ptr::read_volatile(at as *const u64), Ordering::Relaxed);
            READ_PC[k].store(pc, Ordering::Relaxed);
            READ_LR[k].store(lr, Ordering::Relaxed);
            READ_SERIAL[k].store(TRAP_SERIAL[i].load(Ordering::Acquire), Ordering::Release);
        }
        Some(_) => {
            TRAP_LATE.fetch_add(1, Ordering::Relaxed);
        }
        None => {
            TRAP_NEIGHBOURS.fetch_add(1, Ordering::Relaxed);
        }
    }
    true
}

/// Early reads recorded by trap_fault, judged by check_reads (a ring: the fault handler writes, guest-main reads).
const READ_SLOTS: usize = 4096;
static READ_AT: [AtomicUsize; READ_SLOTS] = [const { AtomicUsize::new(0) }; READ_SLOTS];
static READ_VAL: [AtomicU64; READ_SLOTS] = [const { AtomicU64::new(0) }; READ_SLOTS];
static READ_PC: [AtomicU64; READ_SLOTS] = [const { AtomicU64::new(0) }; READ_SLOTS];
static READ_LR: [AtomicU64; READ_SLOTS] = [const { AtomicU64::new(0) }; READ_SLOTS];
/// The submit that writes the bytes; 0: the slot is empty.
static READ_SERIAL: [AtomicU64; READ_SLOTS] = [const { AtomicU64::new(0) }; READ_SLOTS];
static READ_W: AtomicUsize = AtomicUsize::new(0);
static STALE: AtomicU64 = AtomicU64::new(0);
static SAME: AtomicU64 = AtomicU64::new(0);
static UNJUDGED: AtomicU64 = AtomicU64::new(0);

/// Judges the early reads whose submit completed: the GPU changed the bytes the game read before it was done (the
/// game used stale data: "[visibility] STALE", once per guest caller), or left them as they were (harmless).
fn check_reads() {
    let completed = super::inflight::completed();
    for k in 0..READ_SLOTS {
        let serial = READ_SERIAL[k].load(Ordering::Acquire);
        if serial == 0 {
            continue;
        }
        // Not done by the next fence: its page was opened for writing again (trap_writes), so a guest write to those
        // bytes could no longer be told from the GPU's; left unjudged.
        if serial > completed {
            READ_SERIAL[k].store(0, Ordering::Release);
            UNJUDGED.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let (at, seen) = (READ_AT[k].load(Ordering::Relaxed), READ_VAL[k].load(Ordering::Relaxed));
        let (pc, lr) = (READ_PC[k].load(Ordering::Relaxed), READ_LR[k].load(Ordering::Relaxed));
        READ_SERIAL[k].store(0, Ordering::Release);
        // The page is open again (trap_writes opened what the last fence trapped before this runs).
        let now = unsafe { std::ptr::read_volatile(at as *const u64) };
        if now == seen {
            SAME.fetch_add(1, Ordering::Relaxed);
        } else {
            STALE.fetch_add(1, Ordering::Relaxed);
            report_stale(at, serial, seen, now, pc, lr);
        }
    }
}

/// "[visibility] STALE" once per guest caller.
fn report_stale(at: usize, serial: u64, seen: u64, now: u64, pc: u64, lr: u64) {
    static SEEN: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    let mut callers = SEEN.lock().unwrap();
    if callers.contains(&lr) || callers.len() >= 256 {
        return;
    }
    callers.push(lr);
    let main = crate::guest::loader::main_base() as u64;
    let name = |a: u64| {
        let rel = if a >= main && main != 0 { format!("main+{:#x}", a - main) } else { format!("{a:#x}") };
        match crate::guest::module_loader::nearest_symbol(a as usize) {
            Some((n, off)) => format!("{rel} ({n}+{off:#x})"),
            None => rel,
        }
    };
    eprintln!(
        "[visibility] STALE: the guest read {at:#x} before GPU submit {serial} wrote it ({seen:#018x}, then {now:#018x}): pc {} lr {}",
        name(pc),
        name(lr)
    );
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
