// Occlusion reports a submit will write are not readable by the CPU until written (--report-guard).
//
// On the Switch nvnQueueSubmitCommands hands the command list to the GPU, which writes each SAMPLES_PASSED report as it
// reaches it. TotK reads its occlusion queries on a worker ~1.3 ms after the submit that holds them starts (the main
// view expects a query issued in one frame to be readable at the next frame's read); the
// Switch GPU has run the culling pass by then. We translate the submit to Metal first and write reports on the CPU
// once the command buffer completed, so the read always came first: every query read the count of three frames earlier,
// and a new query read the zeros its fresh slots held (signposts culled for three frames at their LOD switch).
//
// At submit start (before any encoding) the pages of the submit's SAMPLES_PASSED reports are made inaccessible. Guest
// code touching one waits until every report pending on that page is written, then its access is retried and sees the
// results, as on the Switch where the GPU has written them by then.
//
// Host code never waits and never opens a page early: each guarded page gets a second, shared mapping (an alias, made
// before the page is protected), and the engine's accesses go through it: report writes (metal::write_counter), uniform /
// index copies (translated::Arena), conditional rendering. Any other host-code fault on a guarded page (an engine thread,
// or our HLE code on a game thread, which may hold the gfx state lock the producing submit needs) releases that page and
// is counted ("host releases"): such a page is unguarded until its reports are written. Zero-copy Metal buffers
// made before the protection keep working (GPU access ignores it; a standalone test, 2026-10-15); creating one over a
// protected page fails and the draw falls back to an arena copy.
//
// On by default since 2026-10-15 (--report-guard=0: off). Walking in Hateno (fence on), with the occlusion wait it
// replaces: 72.6 fps (three runs) vs 65.9 (two, interleaved), guest blocked in SyncWait 0 vs 5 ms; the reading workers
// wait ~5.5 ms per frame off the critical path. Sky dips over frames 8500-12500: 0. =log never waits and logs each
// first fault per page with thread, PC, caller and whether it hit a pending report slot (to see what shares report pages).

use std::cell::Cell;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;

const PAGE: usize = 0x4000;
const SLOTS: usize = 1024;
/// Pending report slot addresses (log mode: did a fault hit a report or other data on the page).
const REPORTS: usize = 4096;

thread_local! {
    /// Inside engine work on a game thread (the Presentation Thread's submit encodes the very reports it would wait for).
    static IN_ENGINE: Cell<bool> = const { Cell::new(false) };
    /// Address of the last fault answered with a plain retry (see `fault`).
    static LAST_RETRY: Cell<usize> = const { Cell::new(0) };
}

/// Marks the current thread as running engine work until the returned guard is dropped.
pub(crate) fn engine_scope() -> impl Drop {
    struct Scope(bool);
    impl Drop for Scope {
        fn drop(&mut self) {
            IN_ENGINE.with(|e| e.set(self.0));
        }
    }
    Scope(IN_ENGINE.with(|e| e.replace(true)))
}

static CODE_START: AtomicUsize = AtomicUsize::new(0);
static CODE_END: AtomicUsize = AtomicUsize::new(0);

pub(super) fn set_guest_code_range(start: usize, end: usize) {
    CODE_START.store(start, Ordering::Release);
    CODE_END.store(end, Ordering::Release);
}

/// Only guest code waits: a game thread inside our HLE code may hold a lock the submit producing the reports needs (the
/// gfx state lock), which deadlocked until the timeout at gameplay start (2026-10-15).
pub(super) fn may_wait(pc: usize) -> bool {
    crate::cpu::tls::is_guest_thread()
        && !IN_ENGINE.with(|e| e.get())
        && pc >= CODE_START.load(Ordering::Acquire)
        && pc < CODE_END.load(Ordering::Acquire)
}

/// 1 on (default), 0 off (--report-guard=0), 2 log only (--report-guard=log).
pub(crate) fn mode() -> crate::settings::ReportGuard {
    crate::settings::get().graphics.report_guard
}

extern "C" {
    static mach_task_self_: u32;
    fn mach_vm_remap(
        target_task: u32,
        target_address: *mut u64,
        size: u64,
        mask: u64,
        flags: i32,
        src_task: u32,
        src_address: u64,
        copy: i32,
        cur_protection: *mut i32,
        max_protection: *mut i32,
        inheritance: u32,
    ) -> i32;
    fn mach_vm_deallocate(task: u32, address: u64, size: u64) -> i32;
}

/// Per page slot: page address (0 = free), its alias (0 = none), reports not yet written, protected now.
static PAGE_ADDR: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
static ALIAS: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
static PENDING: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
static PROTECTED: [AtomicU8; SLOTS] = [const { AtomicU8::new(0) }; SLOTS];
/// Slots in use are below this (lookups scan up to it).
static HIGH: AtomicUsize = AtomicUsize::new(0);
/// Slot changes (insert / free) happen under this lock; lookups (the fault handler too) only read.
static TABLE: Mutex<()> = Mutex::new(());
/// Aliases of freed slots with the frame they were freed in, unmapped a few frames later (see free_slot).
static RETIRED: Mutex<Vec<(usize, u64)>> = Mutex::new(Vec::new());

static PENDING_REPORTS: [AtomicUsize; REPORTS] = [const { AtomicUsize::new(0) }; REPORTS];

/// The slot holding `page` (never page 0: free slots hold 0).
fn find(page: usize) -> Option<usize> {
    if page == 0 {
        return None;
    }
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    PAGE_ADDR[..n]
        .iter()
        .position(|p| p.load(Ordering::Acquire) == page)
}

/// A shared second mapping of `page` (same physical memory, its own protection).
pub(super) unsafe fn make_alias(page: usize) -> usize {
    let (mut target, mut cur, mut max) = (0u64, 0i32, 0i32);
    // VM_FLAGS_ANYWHERE, copy = false (shared), VM_INHERIT_NONE.
    let kr = mach_vm_remap(
        mach_task_self_,
        &mut target,
        PAGE as u64,
        0,
        1,
        mach_task_self_,
        page as u64,
        0,
        &mut cur,
        &mut max,
        2,
    );
    if kr == 0 {
        target as usize
    } else {
        0
    }
}

/// The protection of a page that is not held here: read-only while the early-fence write guard holds it
/// (metal/fence_guard.rs), else read-write.
fn open_prot(page: usize) -> i32 {
    if super::fence_guard::holds(page) {
        libc::PROT_READ
    } else {
        libc::PROT_READ | libc::PROT_WRITE
    }
}

/// Per slot: a protection change is in progress. The flag and the page's protection change together under it (a waiter
/// that saw the flag clear before the mprotect opened the page retried into a still-inaccessible page; if the slot was
/// freed meanwhile the fault was no longer recognised and the game crashed, 2026-10-15). Held only around one or two
/// mprotect calls, never while taking another lock; spun on in the signal handler.
static BUSY: [AtomicU8; SLOTS] = [const { AtomicU8::new(0) }; SLOTS];

fn lock_slot(i: usize) {
    while BUSY[i].swap(1, Ordering::Acquire) != 0 {
        std::hint::spin_loop();
    }
}

fn unlock_slot(i: usize) {
    BUSY[i].store(0, Ordering::Release);
}

/// Opens a page held here.
unsafe fn open_page(i: usize, page: usize) {
    lock_slot(i);
    if PROTECTED[i].load(Ordering::Acquire) != 0 {
        libc::mprotect(page as *mut libc::c_void, PAGE, open_prot(page));
        super::tex_backing::disc_reprotect(page, PAGE);
        PROTECTED[i].store(0, Ordering::Release);
    }
    unlock_slot(i);
}

/// Makes slot `i`'s page inaccessible (if it still holds `page`). `again`: re-apply to a page marked protected, after
/// another module changed its protection.
unsafe fn close_page(i: usize, page: usize, again: bool) {
    lock_slot(i);
    if PAGE_ADDR[i].load(Ordering::Acquire) == page
        && (PROTECTED[i].load(Ordering::Acquire) != 0) == again
    {
        libc::mprotect(page as *mut libc::c_void, PAGE, libc::PROT_NONE);
        PROTECTED[i].store(1, Ordering::Release);
    }
    unlock_slot(i);
}

/// After another module changed the protection of [addr, addr+len): pages held here stay inaccessible (signal-safe).
pub(super) unsafe fn reprotect_held(addr: usize, len: usize) {
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    for i in 0..n {
        let page = PAGE_ADDR[i].load(Ordering::Acquire);
        if page != 0
            && page < addr + len
            && page + PAGE > addr
            && PROTECTED[i].load(Ordering::Acquire) != 0
        {
            close_page(i, page, true);
        }
    }
}

/// Pages freed recently (a read fault on one is a fault that raced with its opening: repaired, counted).
const FREED: usize = 64;
static FREED_PAGES: [AtomicUsize; FREED] = [const { AtomicUsize::new(0) }; FREED];
static FREED_NEXT: AtomicUsize = AtomicUsize::new(0);

/// Whether `page` was freed recently (0 never is: empty entries hold 0, and a null-pointer access must stay a crash).
fn freed_recently(page: usize) -> bool {
    page != 0 && FREED_PAGES.iter().any(|p| p.load(Ordering::Acquire) == page)
}

/// Frees slot `i` (TABLE held): unprotects the page, drops its alias.
unsafe fn free_slot(i: usize) {
    let page = PAGE_ADDR[i].load(Ordering::Acquire);
    open_page(i, page);
    PENDING[i].store(0, Ordering::Release);
    FREED_PAGES[FREED_NEXT.fetch_add(1, Ordering::Relaxed) % FREED].store(page, Ordering::Release);
    lock_slot(i);
    PAGE_ADDR[i].store(0, Ordering::Release);
    unlock_slot(i);
    let alias = ALIAS[i].swap(0, Ordering::AcqRel);
    if alias != 0 {
        // Not unmapped yet: an engine copy that looked the alias up just before may still read it (gfx-encoder crashed
        // in memcpy on an unmapped alias after ~12,000 frames of play, 2026-10-15). frame_end unmaps it later.
        RETIRED
            .lock()
            .unwrap()
            .push((alias, super::presented_frames()));
    }
    let mut n = HIGH.load(Ordering::Acquire);
    while n > 0 && PAGE_ADDR[n - 1].load(Ordering::Acquire) == 0 {
        n -= 1;
    }
    HIGH.store(n, Ordering::Release);
}

/// A submit starts: `addrs` are the host addresses of its SAMPLES_PASSED reports.
pub(super) fn begin_submit(addrs: &[usize]) {
    if mode() == crate::settings::ReportGuard::Off || addrs.is_empty() {
        return;
    }
    let _g = TABLE.lock().unwrap();
    for &a in addrs {
        if a < 0x10000 {
            continue;
        }
        if mode() == crate::settings::ReportGuard::Log {
            if let Some(r) = PENDING_REPORTS
                .iter()
                .find(|r| r.load(Ordering::Relaxed) == 0)
            {
                r.store(a, Ordering::Release);
            }
        }
        let page = a & !(PAGE - 1);
        let i = match find(page) {
            Some(i) => i,
            None => {
                let Some(i) = PAGE_ADDR
                    .iter()
                    .position(|p| p.load(Ordering::Acquire) == 0)
                else {
                    OVERFLOW.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                let alias = unsafe { make_alias(page) };
                if alias == 0 {
                    ALIAS_FAILS.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                ALIAS[i].store(alias, Ordering::Release);
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
    let n = HIGH.load(Ordering::Acquire);
    for i in 0..n {
        let page = PAGE_ADDR[i].load(Ordering::Acquire);
        if page != 0
            && PENDING[i].load(Ordering::Acquire) > 0
            && PROTECTED[i].load(Ordering::Acquire) == 0
        {
            unsafe {
                // Write-tracked texture pages: dirty_track's handler would unprotect them without waiting.
                crate::guest::dirty_track::release_for_gpu(page, PAGE);
                close_page(i, page, false);
            }
        }
    }
    SUBMITS.fetch_add(1, Ordering::Relaxed);
}

/// Crash reports: this page's slot state.
pub(super) fn describe(addr: usize) -> String {
    let page = addr & !(PAGE - 1);
    match find(page) {
        Some(i) => format!(
            "slot {} pending {} protected {} busy {}",
            i,
            PENDING[i].load(Ordering::Acquire),
            PROTECTED[i].load(Ordering::Acquire),
            BUSY[i].load(Ordering::Acquire)
        ),
        None if freed_recently(page) => {
            "not held (freed recently)".into()
        }
        None => "not held".into(),
    }
}

/// Where the engine accesses host address `a`: through the alias while its page is guarded, else `a` itself.
pub(super) fn engine_ptr(a: usize) -> usize {
    if HIGH.load(Ordering::Acquire) == 0 {
        return super::rt_guard::engine_ptr(a).unwrap_or(a);
    }
    match find(a & !(PAGE - 1)) {
        Some(i) => match ALIAS[i].load(Ordering::Acquire) {
            0 => a,
            alias => alias + (a & (PAGE - 1)),
        },
        None => a,
    }
}

/// Copies guest memory at `src` into `dst` (engine side: guarded pages are read through their aliases).
pub(super) unsafe fn engine_copy(dst: &mut [u8], src: *const u8) {
    if HIGH.load(Ordering::Acquire) == 0 && !super::rt_guard::any_held() {
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

/// The report at host address `a` was written (or will never be): one less pending on its page; the last one opens it.
pub(super) fn close(a: usize) {
    if mode() == crate::settings::ReportGuard::Off || HIGH.load(Ordering::Acquire) == 0 {
        return;
    }
    let _g = TABLE.lock().unwrap();
    if mode() == crate::settings::ReportGuard::Log {
        if let Some(r) = PENDING_REPORTS
            .iter()
            .find(|r| r.load(Ordering::Relaxed) == a)
        {
            r.store(0, Ordering::Release);
        }
    }
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

// ---- fault handling (signal context: atomics, mprotect and nanosleep only) -----------------------------------------

const HITS: usize = 256;
static HIT_N: AtomicUsize = AtomicUsize::new(0);
/// (pc, address, flags: bit 0 guest code (it waits), bit 1 hit a pending report slot, bit 2 write)
static HIT: [(AtomicUsize, AtomicUsize, AtomicUsize); HITS] = [const {
    (
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    )
}; HITS];
static HIT_LR: [AtomicUsize; HITS] = [const { AtomicUsize::new(0) }; HITS];
static HIT_NAME: [[AtomicU64; 4]; HITS] = [const { [const { AtomicU64::new(0) }; 4] }; HITS];

static SUBMITS: AtomicU64 = AtomicU64::new(0);
static WAITS: AtomicU64 = AtomicU64::new(0);
static WAIT_US: AtomicU64 = AtomicU64::new(0);
static MAX_WAIT_US: AtomicU64 = AtomicU64::new(0);
static HOST_RELEASES: AtomicU64 = AtomicU64::new(0);
static TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static OVERFLOW: AtomicU64 = AtomicU64::new(0);
static ALIAS_FAILS: AtomicU64 = AtomicU64::new(0);
static REPAIRS: AtomicU64 = AtomicU64::new(0);

unsafe fn record_hit(pc: usize, lr: usize, addr: usize, guest: bool, write: bool) {
    let n = HIT_N.fetch_add(1, Ordering::AcqRel);
    if n >= HITS {
        return;
    }
    let report = PENDING_REPORTS.iter().any(|r| {
        let s = r.load(Ordering::Acquire);
        s != 0 && addr >= s && addr < s + 16
    });
    let mut name = [0u8; 32];
    libc::pthread_getname_np(
        libc::pthread_self(),
        name.as_mut_ptr() as *mut libc::c_char,
        name.len(),
    );
    for k in 0..4 {
        HIT_NAME[n][k].store(
            u64::from_le_bytes(name[k * 8..k * 8 + 8].try_into().unwrap()),
            Ordering::Relaxed,
        );
    }
    HIT[n].0.store(pc, Ordering::Relaxed);
    HIT_LR[n].store(lr, Ordering::Relaxed);
    HIT[n].1.store(addr, Ordering::Relaxed);
    HIT[n].2.store(
        guest as usize | (report as usize) << 1 | (write as usize) << 2,
        Ordering::Release,
    );
}

/// Signal-handler hook: true if `addr` is on a page held here (the access is then retried).
pub(super) unsafe fn fault(addr: usize, pc: usize, lr: usize, write: bool) -> bool {
    if mode() == crate::settings::ReportGuard::Off {
        return false;
    }
    let page = addr & !(PAGE - 1);
    // No slot (none in use at all, or this page's was freed): the fault may still be from before its page was opened.
    // Bailing out on "no slots in use" before the freed-pages check below turned such a late fault into a crash on the
    // iPhone (the occlusion reader, main+0x10c4ec8, with the page already read-write when the crash report looked).
    let Some(i) = (if HIGH.load(Ordering::Acquire) == 0 {
        None
    } else {
        find(page)
    }) else {
        // Freed between the fault and here (its opening raced with this access): retry; a repeated read fault on it
        // means it was left inaccessible: open it (no other module removes read access).
        if freed_recently(page) {
            if LAST_RETRY.with(|r| r.replace(addr)) != addr {
                return true;
            }
            if !write {
                REPAIRS.fetch_add(1, Ordering::Relaxed);
                libc::mprotect(page as *mut libc::c_void, PAGE, open_prot(page));
                LAST_RETRY.with(|r| r.set(0));
                return true;
            }
        }
        return false;
    };
    // A transition in progress: wait for it, then look again.
    while BUSY[i].load(Ordering::Acquire) != 0 {
        std::hint::spin_loop();
    }
    if PROTECTED[i].load(Ordering::Acquire) == 0 {
        // Opened between the fault and this check: retry once. A second write fault at the same address is another
        // module's protection (dirty_track write tracking, the write guard: read-only pages): not ours to answer. A
        // second read fault can only be an inaccessible page marked open here (no other module removes read access):
        // open it again.
        if LAST_RETRY.with(|r| r.replace(addr)) != addr {
            return true;
        }
        if write {
            return false;
        }
        REPAIRS.fetch_add(1, Ordering::Relaxed);
        libc::mprotect(page as *mut libc::c_void, PAGE, open_prot(page));
        LAST_RETRY.with(|r| r.set(0));
        return true;
    }
    LAST_RETRY.with(|r| r.set(0));
    let guest = may_wait(pc);
    if mode() == crate::settings::ReportGuard::Log || !guest {
        record_hit(pc, lr, addr, guest, write);
        if !guest {
            HOST_RELEASES.fetch_add(1, Ordering::Relaxed);
        }
        open_page(i, page);
        return true;
    }
    // Wait until every report pending on this page is written (the last close() frees the slot and opens the page).
    let t0 = std::time::Instant::now();
    WAITS.fetch_add(1, Ordering::Relaxed);
    while PAGE_ADDR[i].load(Ordering::Acquire) == page
        && (PROTECTED[i].load(Ordering::Acquire) != 0 || BUSY[i].load(Ordering::Acquire) != 0)
    {
        if t0.elapsed().as_secs() >= 2 {
            // Something will never write them: open the page (close() frees the slot later, or the next submit
            // reuses it) rather than stall every access.
            TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            PENDING[i].store(0, Ordering::Release); // not re-protected by the next submit for these
            open_page(i, page);
            break;
        }
        let ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 20_000,
        };
        libc::nanosleep(&ts, std::ptr::null_mut());
    }
    let us = t0.elapsed().as_micros() as u64;
    WAIT_US.fetch_add(us, Ordering::Relaxed);
    MAX_WAIT_US.fetch_max(us, Ordering::Relaxed);
    true
}

/// Called once per presented frame: prints the faults seen (log mode) and statistics every 300 frames.
pub(super) fn frame_end(frame: u64) {
    if mode() == crate::settings::ReportGuard::Off {
        return;
    }
    RETIRED.lock().unwrap().retain(|&(alias, f)| {
        if f + 4 > frame {
            return true;
        }
        unsafe { mach_vm_deallocate(mach_task_self_, alias as u64, PAGE as u64) };
        false
    });
    unsafe { print_hits(frame, HIT_N.swap(0, Ordering::AcqRel).min(HITS)) };
    if frame % 300 == 0 {
        let take = |a: &AtomicU64| a.swap(0, Ordering::Relaxed);
        let (s, w, us) = (take(&SUBMITS), take(&WAITS), take(&WAIT_US));
        println!("[report-guard] last 300 frames: {} guarded submits, {} waits ({:.2} ms per frame, longest {:.2} ms), host releases {}, \
            timeouts {}, overflows {}, alias failures {}, repairs {}, late faults retried {}",
            s, w, us as f64 / 300_000.0, take(&MAX_WAIT_US) as f64 / 1000.0, take(&HOST_RELEASES), take(&TIMEOUTS), take(&OVERFLOW),
            take(&ALIAS_FAILS), take(&REPAIRS), crate::cpu::crash_handler::LATE_RETRIES.load(Ordering::Relaxed));
    }
}

unsafe fn print_hits(frame: u64, n: usize) {
    extern "C" {
        fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
    }
    let base = crate::cpu::crash_handler::get_guest_base();
    let slide = _dyld_get_image_vmaddr_slide(0) as usize;
    for k in 0..n {
        let pc = HIT[k].0.load(Ordering::Acquire);
        let fl = HIT[k].2.load(Ordering::Acquire);
        let mut name = [0u8; 32];
        for j in 0..4 {
            name[j * 8..j * 8 + 8]
                .copy_from_slice(&HIT_NAME[k][j].load(Ordering::Relaxed).to_le_bytes());
        }
        let name =
            String::from_utf8_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(32)])
                .to_string();
        let at = if pc >= base && pc < base + (256 << 20) {
            format!("main+{:#x}", pc - base)
        } else {
            format!("{:#x}", pc)
        };
        let lr = HIT_LR[k].load(Ordering::Relaxed);
        println!(
            "[report-guard] f{} {} {} {} {:#x} {} lr {:#x} (engine {:#x}) thread {}",
            frame,
            if fl & 1 != 0 { "guest " } else { "host  " },
            if fl & 4 != 0 { "write" } else { "read " },
            if fl & 2 != 0 { "REPORT" } else { "other " },
            HIT[k].1.load(Ordering::Relaxed),
            at,
            lr,
            lr.wrapping_sub(slide),
            name
        );
    }
}
