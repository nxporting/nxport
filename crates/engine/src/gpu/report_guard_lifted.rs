// Occlusion reports a submit will write are not readable by the game until written (--report-guard), where guest code is
// lifted. Why: report_guard.rs. A query the game reads before the submit holding it completed reads an older count, and
// a new query reads the zeros its fresh slot holds: objects are culled for a few frames when they switch LOD.
//
// As report_guard.rs, the pages of a submit's SAMPLES_PASSED reports are inaccessible from the submit's start until all
// their reports are written, and guest code touching one waits. Two things differ:
// - Guest memory is private memory that cannot be mapped twice, so the engine works on a copy of each guarded page: it
//   reads the copy and writes reports into it (engine_write), and the bytes written reach the page as it opens. Nothing
//   else can reach the page meanwhile, which is what makes the copy as good as a second mapping. Only the bytes written
//   go back, so nothing else on the page is overwritten if another guard opened it in between.
// - Whether a fault may wait is decided by where the faulting instruction is: in the guest library, or not. The engine
//   never waits (a game thread in our code may hold a lock the submit needs): it opens the page, counted as a host
//   release, and that page is unguarded until its reports are written.
//
// Faults arrive through a vectored exception handler (lib.rs), which may sleep; only Windows installs one, so the guard
// is off on other lifted hosts.

use std::cell::Cell;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::os::Prot;

const PAGE: usize = 0x4000;
const SLOTS: usize = 1024;
/// Written bytes are tracked per 16 (a report's size).
const GRANULE: usize = 16;
const DIRTY_WORDS: usize = PAGE / GRANULE / 64;

/// Per page slot: page address (0 = free), the engine's copy while guarded (0 = go to the page itself), reports not yet
/// written, protected now, a protection change or engine write in progress. The copies are never freed: an engine read
/// that looked one up just before the page opened may still be reading it.
static PAGE_ADDR: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
static COPY: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
static SHADOW: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
static PENDING: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
static PROTECTED: [AtomicU8; SLOTS] = [const { AtomicU8::new(0) }; SLOTS];
static BUSY: [AtomicU8; SLOTS] = [const { AtomicU8::new(0) }; SLOTS];
static DIRTY: [[AtomicU64; DIRTY_WORDS]; SLOTS] = [const { [const { AtomicU64::new(0) }; DIRTY_WORDS] }; SLOTS];
/// Slots in use are below this (lookups scan up to it).
static HIGH: AtomicUsize = AtomicUsize::new(0);
/// Slot changes (insert / free) happen under this lock; lookups (the fault handler too) only read.
static TABLE: Mutex<()> = Mutex::new(());

/// Pages freed recently (a read fault on one raced with its opening: repaired, counted).
const FREED: usize = 64;
static FREED_PAGES: [AtomicUsize; FREED] = [const { AtomicUsize::new(0) }; FREED];
static FREED_NEXT: AtomicUsize = AtomicUsize::new(0);

static CODE_START: AtomicUsize = AtomicUsize::new(0);
static CODE_END: AtomicUsize = AtomicUsize::new(0);

static SUBMITS: AtomicU64 = AtomicU64::new(0);
static WAITS: AtomicU64 = AtomicU64::new(0);
static WAIT_US: AtomicU64 = AtomicU64::new(0);
static MAX_WAIT_US: AtomicU64 = AtomicU64::new(0);
static HOST_RELEASES: AtomicU64 = AtomicU64::new(0);
static TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static OVERFLOW: AtomicU64 = AtomicU64::new(0);
static PROTECT_FAILS: AtomicU64 = AtomicU64::new(0);
static REPAIRS: AtomicU64 = AtomicU64::new(0);

/// The first faults that did not wait since the last print: (pc, address, write).
const HITS: usize = 16;
static HIT_N: AtomicUsize = AtomicUsize::new(0);
static HIT: [(AtomicUsize, AtomicUsize, AtomicU8); HITS] = [const { (AtomicUsize::new(0), AtomicUsize::new(0), AtomicU8::new(0)) }; HITS];

thread_local! {
    /// The last fault answered with a plain retry and when (see `repeated`).
    static LAST_RETRY: Cell<(usize, Option<std::time::Instant>)> = const { Cell::new((0, None)) };
}

/// Guest code: the lifted guest library's image.
pub(crate) fn set_guest_code_range(start: usize, end: usize) {
    CODE_START.store(start, Ordering::Release);
    CODE_END.store(end, Ordering::Release);
}

pub(crate) fn mode() -> crate::settings::ReportGuard {
    if cfg!(windows) {
        crate::settings::get().graphics.report_guard
    } else {
        crate::settings::ReportGuard::Off
    }
}

/// Lifted code calls its HLE functions directly: there is no trampoline pool to return into.
pub(crate) fn in_hle_stubs(_addr: usize) -> bool {
    false
}

fn find(page: usize) -> Option<usize> {
    if page == 0 {
        return None;
    }
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    PAGE_ADDR[..n].iter().position(|p| p.load(Ordering::Acquire) == page)
}

fn freed_recently(page: usize) -> bool {
    page != 0 && FREED_PAGES.iter().any(|p| p.load(Ordering::Acquire) == page)
}

fn lock_slot(i: usize) {
    while BUSY[i].swap(1, Ordering::Acquire) != 0 {
        std::hint::spin_loop();
    }
}

fn unlock_slot(i: usize) {
    BUSY[i].store(0, Ordering::Release);
}

/// The protection of a page not held here: read-only while the early-fence write guard holds it, else read-write.
unsafe fn set_open(page: usize) {
    if !crate::gpu::backend().early_fence_holds(page) {
        let _ = crate::os::protect(page, PAGE, Prot::ReadWrite);
        return;
    }
    let _ = crate::os::protect(page, PAGE, Prot::Read);
    // Released between the check and the protection (the write guard marks a range released, then opens it): left
    // read-only with nothing holding it, the next write to it was a crash.
    if !crate::gpu::backend().early_fence_holds(page) {
        let _ = crate::os::protect(page, PAGE, Prot::ReadWrite);
    }
}

/// Opens a page held here: what the engine wrote to the copy reaches it first.
unsafe fn open_page(i: usize, page: usize) {
    lock_slot(i);
    if PROTECTED[i].load(Ordering::Acquire) != 0 {
        let _ = crate::os::protect(page, PAGE, Prot::ReadWrite);
        let copy = COPY[i].swap(0, Ordering::AcqRel);
        for (w, word) in DIRTY[i].iter().enumerate() {
            let mut bits = word.swap(0, Ordering::Relaxed);
            while bits != 0 {
                let at = (w * 64 + bits.trailing_zeros() as usize) * GRANULE;
                std::ptr::copy_nonoverlapping((copy + at) as *const u8, (page + at) as *mut u8, GRANULE);
                bits &= bits - 1;
            }
        }
        set_open(page);
        PROTECTED[i].store(0, Ordering::Release);
    }
    unlock_slot(i);
}

/// Copies slot `i`'s page and makes it inaccessible. Read-only during the copy: a write then faults and waits here.
unsafe fn close_page(i: usize, page: usize, shadow: usize) -> bool {
    lock_slot(i);
    let ok = crate::os::protect(page, PAGE, Prot::Read).is_ok();
    if ok {
        std::ptr::copy_nonoverlapping(page as *const u8, shadow as *mut u8, PAGE);
        COPY[i].store(shadow, Ordering::Release);
        let _ = crate::os::protect(page, PAGE, Prot::None);
        PROTECTED[i].store(1, Ordering::Release);
    }
    unlock_slot(i);
    ok
}

/// After another module changed the protection of [addr, addr+len): pages held here stay inaccessible.
pub(crate) unsafe fn reprotect_held(addr: usize, len: usize) {
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    for i in 0..n {
        let page = PAGE_ADDR[i].load(Ordering::Acquire);
        if page != 0 && page < addr + len && page + PAGE > addr {
            lock_slot(i);
            if PAGE_ADDR[i].load(Ordering::Acquire) == page && PROTECTED[i].load(Ordering::Acquire) != 0 {
                let _ = crate::os::protect(page, PAGE, Prot::None);
            }
            unlock_slot(i);
        }
    }
}

/// Frees slot `i` (TABLE held): the page opens.
unsafe fn free_slot(i: usize) {
    let page = PAGE_ADDR[i].load(Ordering::Acquire);
    open_page(i, page);
    PENDING[i].store(0, Ordering::Release);
    FREED_PAGES[FREED_NEXT.fetch_add(1, Ordering::Relaxed) % FREED].store(page, Ordering::Release);
    lock_slot(i);
    PAGE_ADDR[i].store(0, Ordering::Release);
    unlock_slot(i);
    let mut n = HIGH.load(Ordering::Acquire);
    while n > 0 && PAGE_ADDR[n - 1].load(Ordering::Acquire) == 0 {
        n -= 1;
    }
    HIGH.store(n, Ordering::Release);
}

/// A submit starts: `addrs` are the host addresses of its SAMPLES_PASSED reports.
pub(crate) fn begin_submit(addrs: &[usize]) {
    if mode() == crate::settings::ReportGuard::Off || addrs.is_empty() {
        return;
    }
    let _g = TABLE.lock().unwrap();
    for &a in addrs {
        if a < 0x10000 {
            continue;
        }
        let page = a & !(PAGE - 1);
        let i = match find(page) {
            Some(i) => i,
            None => {
                let Some(i) = PAGE_ADDR.iter().position(|p| p.load(Ordering::Acquire) == 0) else {
                    OVERFLOW.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                PENDING[i].store(0, Ordering::Release);
                PAGE_ADDR[i].store(page, Ordering::Release);
                if i >= HIGH.load(Ordering::Acquire) {
                    HIGH.store(i + 1, Ordering::Release);
                }
                i
            }
        };
        PENDING[i].fetch_add(1, Ordering::AcqRel);
    }
    for i in 0..HIGH.load(Ordering::Acquire) {
        let page = PAGE_ADDR[i].load(Ordering::Acquire);
        if page == 0 || PENDING[i].load(Ordering::Acquire) == 0 || PROTECTED[i].load(Ordering::Acquire) != 0 {
            continue;
        }
        let mut shadow = SHADOW[i].load(Ordering::Acquire);
        if shadow == 0 {
            shadow = unsafe { crate::os::alloc(PAGE, Prot::ReadWrite) }.map_or(0, |p| p as usize);
            SHADOW[i].store(shadow, Ordering::Release);
        }
        // Write-tracked texture pages: dirty_track's handler would unprotect them without waiting.
        crate::guest::dirty_track::release_for_gpu(page, PAGE);
        if shadow == 0 || !unsafe { close_page(i, page, shadow) } {
            PROTECT_FAILS.fetch_add(1, Ordering::Relaxed);
        }
    }
    SUBMITS.fetch_add(1, Ordering::Relaxed);
}

/// Crash reports: this page's slot state.
pub(crate) fn describe(addr: usize) -> String {
    let page = addr & !(PAGE - 1);
    match find(page) {
        Some(i) => format!(
            "slot {} pending {} protected {} busy {}",
            i,
            PENDING[i].load(Ordering::Acquire),
            PROTECTED[i].load(Ordering::Acquire),
            BUSY[i].load(Ordering::Acquire)
        ),
        None if freed_recently(page) => "not held (freed recently)".into(),
        None => "not held".into(),
    }
}

/// Where the engine reads host address `a`: the page's copy while it is guarded, else `a` itself.
pub(crate) fn engine_ptr(a: usize) -> usize {
    if HIGH.load(Ordering::Acquire) == 0 {
        return a;
    }
    match find(a & !(PAGE - 1)).map(|i| COPY[i].load(Ordering::Acquire)) {
        Some(copy) if copy != 0 => copy + (a & (PAGE - 1)),
        _ => a,
    }
}

/// Copies guest memory at `src` into `dst` (engine side: guarded pages are read through their copies).
pub(crate) unsafe fn engine_copy(dst: &mut [u8], src: *const u8) {
    if HIGH.load(Ordering::Acquire) == 0 {
        std::ptr::copy_nonoverlapping(src, dst.as_mut_ptr(), dst.len());
        return;
    }
    let mut done = 0;
    while done < dst.len() {
        let a = src as usize + done;
        let n = (PAGE - (a & (PAGE - 1))).min(dst.len() - done);
        std::ptr::copy_nonoverlapping(engine_ptr(a) as *const u8, dst.as_mut_ptr().add(done), n);
        done += n;
    }
}

/// The engine writes guest memory at host address `a` (a report): to the page's copy while it is guarded, marked to
/// reach the page as it opens, else to `a` itself.
pub(crate) unsafe fn engine_write(a: usize, src: &[u8]) {
    let off = a & (PAGE - 1);
    if HIGH.load(Ordering::Acquire) != 0 && off + src.len() <= PAGE {
        if let Some(i) = find(a & !(PAGE - 1)) {
            lock_slot(i);
            let copy = COPY[i].load(Ordering::Acquire);
            if copy != 0 {
                std::ptr::copy_nonoverlapping(src.as_ptr(), (copy + off) as *mut u8, src.len());
                for g in off / GRANULE..=(off + src.len() - 1) / GRANULE {
                    DIRTY[i][g / 64].fetch_or(1 << (g % 64), Ordering::Relaxed);
                }
                unlock_slot(i);
                return;
            }
            unlock_slot(i);
        }
    }
    std::ptr::copy_nonoverlapping(src.as_ptr(), a as *mut u8, src.len());
}

/// The report at host address `a` was written (or will never be): one less pending on its page; the last one opens it.
pub(crate) fn close(a: usize) {
    if mode() == crate::settings::ReportGuard::Off || HIGH.load(Ordering::Acquire) == 0 {
        return;
    }
    let _g = TABLE.lock().unwrap();
    let Some(i) = find(a & !(PAGE - 1)) else {
        return;
    };
    let left = PENDING[i].load(Ordering::Acquire).saturating_sub(1);
    if left == 0 {
        unsafe { free_slot(i) };
    } else {
        PENDING[i].store(left, Ordering::Release);
    }
}

/// The host is about to write [addr, addr+len) itself. A file read into a guarded page would fail, not fault: open them.
pub(crate) unsafe fn before_host_write(addr: usize, len: usize) {
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    for i in 0..n {
        let page = PAGE_ADDR[i].load(Ordering::Acquire);
        if page != 0 && page < addr + len && page + PAGE > addr && PROTECTED[i].load(Ordering::Acquire) != 0 {
            HOST_RELEASES.fetch_add(1, Ordering::Relaxed);
            open_page(i, page);
        }
    }
}

fn record_hit(pc: usize, addr: usize, write: bool) {
    let n = HIT_N.fetch_add(1, Ordering::AcqRel);
    if n < HITS {
        HIT[n].0.store(pc, Ordering::Relaxed);
        HIT[n].1.store(addr, Ordering::Relaxed);
        HIT[n].2.store(write as u8, Ordering::Release);
    }
}

/// Whether this fault is the retry of the one just answered at `addr`, rather than a new access: the game touches the
/// same report slot every frame, and taking a retry that had worked frames ago for this one crashed on writes.
fn repeated(addr: usize) -> bool {
    let now = std::time::Instant::now();
    let (last, at) = LAST_RETRY.with(|r| r.replace((addr, Some(now))));
    last == addr && at.is_some_and(|t| now.duration_since(t).as_micros() < 1000)
}

/// Fault handler hook: true if `addr` is on a page held here (the access is then retried).
pub(crate) unsafe fn fault(addr: usize, pc: usize, _lr: usize, write: bool) -> bool {
    if mode() == crate::settings::ReportGuard::Off {
        return false;
    }
    let page = addr & !(PAGE - 1);
    let Some(i) = find(page) else {
        // Freed between the fault and here (its opening raced with this access): retry; a repeated read fault means it
        // was left inaccessible: open it (no other module removes read access).
        if freed_recently(page) {
            if !repeated(addr) {
                return true;
            }
            if !write {
                REPAIRS.fetch_add(1, Ordering::Relaxed);
                set_open(page);
                LAST_RETRY.with(|r| r.set((0, None)));
                return true;
            }
        }
        return false;
    };
    while BUSY[i].load(Ordering::Acquire) != 0 {
        std::hint::spin_loop();
    }
    if PROTECTED[i].load(Ordering::Acquire) == 0 {
        // Opened between the fault and this check: retry once. A second write fault at the same address is another
        // module's protection (write tracking, the write guard): not ours to answer. A second read fault can only be an
        // inaccessible page marked open here: open it again.
        if !repeated(addr) {
            return true;
        }
        if write {
            return false;
        }
        REPAIRS.fetch_add(1, Ordering::Relaxed);
        set_open(page);
        LAST_RETRY.with(|r| r.set((0, None)));
        return true;
    }
    LAST_RETRY.with(|r| r.set((0, None)));
    let guest = crate::guest::tls::is_guest_thread()
        && pc >= CODE_START.load(Ordering::Acquire)
        && pc < CODE_END.load(Ordering::Acquire);
    if mode() == crate::settings::ReportGuard::Log || !guest {
        record_hit(pc, addr, write);
        if !guest {
            HOST_RELEASES.fetch_add(1, Ordering::Relaxed);
        }
        open_page(i, page);
        return true;
    }
    // Wait until every report pending on this page is written (the last close() frees the slot and opens the page).
    let t0 = std::time::Instant::now();
    WAITS.fetch_add(1, Ordering::Relaxed);
    let mut spins = 0u32;
    while PAGE_ADDR[i].load(Ordering::Acquire) == page && (PROTECTED[i].load(Ordering::Acquire) != 0 || BUSY[i].load(Ordering::Acquire) != 0) {
        if spins < 4096 {
            std::hint::spin_loop();
            spins += 1;
            continue;
        }
        if t0.elapsed().as_secs() >= 2 {
            // Something will never write them: open the page rather than stall every access.
            TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            PENDING[i].store(0, Ordering::Release);
            open_page(i, page);
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(20));
    }
    let us = t0.elapsed().as_micros() as u64;
    WAIT_US.fetch_add(us, Ordering::Relaxed);
    MAX_WAIT_US.fetch_max(us, Ordering::Relaxed);
    true
}

/// Called once per presented frame: statistics, and who touched guarded pages without waiting, every 300 frames.
pub(crate) fn frame_end(frame: u64) {
    if mode() == crate::settings::ReportGuard::Off || frame % 300 != 0 {
        return;
    }
    let take = |a: &AtomicU64| a.swap(0, Ordering::Relaxed);
    let (s, w, us) = (take(&SUBMITS), take(&WAITS), take(&WAIT_US));
    println!(
        "[report-guard] last 300 frames: {} guarded submits, {} waits ({:.2} ms per frame, longest {:.2} ms), host releases {}, timeouts {}, overflows {}, protect failures {}, repairs {}",
        s,
        w,
        us as f64 / 300_000.0,
        take(&MAX_WAIT_US) as f64 / 1000.0,
        take(&HOST_RELEASES),
        take(&TIMEOUTS),
        take(&OVERFLOW),
        take(&PROTECT_FAILS),
        take(&REPAIRS),
    );
    for hit in HIT.iter().take(HIT_N.swap(0, Ordering::AcqRel).min(HITS)) {
        let pc = hit.0.load(Ordering::Acquire);
        let owner = crate::os::address_owner(pc).0.unwrap_or_default();
        let kind = if hit.2.load(Ordering::Acquire) != 0 { "write" } else { "read" };
        println!("[report-guard] {kind} of {:#x} from {pc:#x} ({owner})", hit.1.load(Ordering::Relaxed));
    }
}
