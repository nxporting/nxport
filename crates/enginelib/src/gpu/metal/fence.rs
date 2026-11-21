// Fences: committed and completed command buffer serials, nvnSyncWait (wait_fence), the early fence (frame overlap)
// with its trap diagnostics (--early-fence-trap) and GPU writes to guest memory it must account for.

use super::*;

/// Last in-flight serial the issuer committed: all work up to it is encoded and handed to the GPU.
pub(super) static COMMITTED_SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Highest command buffer serial (possibly not yet assigned) with GPU writes the game can read: storage buffers, copy
/// destinations in guest memory, occlusion reports (written when it completes). --early-fence waits fully while one
/// is pending. Encoding stages that lag the plan thread may record a later serial than the right one (a longer wait).
pub(super) static GPU_GUEST_WRITE_SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub(super) enum GuestWrite {
    Storage,
    Copy,
    Occlusion,
}

/// Records GPU work that writes guest memory (results the game can read after the frame's fence) into the command
/// buffer being encoded. --early-fence does not wait for it: TotK reads such results right after each frame fence
/// (occlusion report slots; small compute storage buffers, e.g. s_e7935105553f7e48's 0x160-byte outputs, read by
/// TotK 1.1.2 main+0x10c4xxx), so it gets them one frame late, as emulators' asynchronous GPU modes do. --early-fence-strict
/// falls back to a full wait while any is pending: correct, but in Hateno play every frame has some (one buffer copy,
/// ~77 occlusion reports, compute storage), so it gains nothing there. --early-fence-trap traps their pages.
pub(super) fn note_gpu_guest_write(kind: GuestWrite, addr: u64, len: usize) {
    if !matches!(kind, GuestWrite::Occlusion) {
        // (Reports end holds where they are recorded, Cmd::ReportCounter / ReportValue.)
        rt_guard::superseded(crate::nvn::api::translate_virtual(addr as usize), len);
    }
    if !early_fence() {
        return;
    }
    if early_fence_trap() && (!trap_occl_only() || matches!(kind, GuestWrite::Occlusion)) {
        let start = crate::nvn::api::translate_virtual(addr as usize);
        if start >= 0x10000 && len > 0 {
            TRAP_PENDING.lock().unwrap().push((
                start & !0x3fff,
                (start + len + 0x3fff) & !0x3fff,
                submitted_serial() + 1,
            ));
            TRAP_ORIGINS.lock().unwrap().push((
                start,
                start + len,
                addr,
                len,
                submitted_serial() + 1,
            ));
        }
    }
    static STRICT: OnceLock<bool> = OnceLock::new();
    if !*STRICT.get_or_init(|| crate::settings::get().graphics.early_fence_strict) {
        return;
    }
    GUEST_WRITE_KINDS[kind as usize].fetch_add(1, Ordering::Relaxed);
    GPU_GUEST_WRITE_SERIAL.fetch_max(submitted_serial() + 1, Ordering::AcqRel);
}

/// GPU writes the strict mode waits for, per kind (storage, copy, occlusion), for --log-early-fence.
pub(super) static GUEST_WRITE_KINDS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

/// --early-fence-trap (diagnostic, with --early-fence): when a fence returns early, the pages of guest memory
/// that still-running command buffers write (storage buffers, copy destinations, occlusion reports) are made
/// inaccessible; the first access to each logs its PC (guest main+offset, or a host PC with callers), read / write,
/// whether the GPU work was still running ("RACE") and the bindings on the page (* = the access hit one), then the
/// page is unprotected without waiting. Showed which game code reads GPU results early (see note_gpu_guest_write).
/// Pages shared with write-tracked textures lose their tracking: diagnostic runs only.
pub(super) fn early_fence_trap() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        crate::settings::get().debug.early_fence_trap != crate::settings::FenceTrap::Off
    })
}

/// --early-fence-trap=occl: trap only occlusion report pages.
pub(super) fn trap_occl_only() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        crate::settings::get().debug.early_fence_trap == crate::settings::FenceTrap::Occl
    })
}

/// Storage-buffer ranges (page-aligned start, end, serial) bound since the last early fence.
pub(super) static TRAP_PENDING: std::sync::Mutex<Vec<(usize, usize, u64)>> = std::sync::Mutex::new(Vec::new());
/// The bindings behind them: (host start, host end, guest address, length, serial), for the hit report.
pub(super) static TRAP_ORIGINS: std::sync::Mutex<Vec<(usize, usize, u64, usize, u64)>> =
    std::sync::Mutex::new(Vec::new());
pub(super) const TRAP_SLOTS: usize = 512;
/// Protected ranges, searched lock-free by the signal handler (start, end; 0 = free).
pub(super) static TRAPPED: [(AtomicUsize, AtomicUsize); TRAP_SLOTS] =
    [const { (AtomicUsize::new(0), AtomicUsize::new(0)) }; TRAP_SLOTS];
/// Per slot: the last serial whose storage bindings it covers (a hit after it completed is no race).
pub(super) static TRAPPED_SERIAL: [AtomicU64; TRAP_SLOTS] = [const { AtomicU64::new(0) }; TRAP_SLOTS];
pub(super) static TRAP_RACE: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];
/// Per slot: already hit since it was protected (only the first access is logged).
pub(super) static TRAPPED_HIT: [AtomicUsize; TRAP_SLOTS] = [const { AtomicUsize::new(0) }; TRAP_SLOTS];
/// The previous generation's ranges (unprotected again): a fault delivered late for one of them is retried.
pub(super) static TRAPPED_OLD: [(AtomicUsize, AtomicUsize); TRAP_SLOTS] =
    [const { (AtomicUsize::new(0), AtomicUsize::new(0)) }; TRAP_SLOTS];
/// Faults seen: (guest pc, address, write).
pub(super) static TRAP_HITS: [(AtomicUsize, AtomicUsize, AtomicUsize); 256] = [const {
    (
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    )
}; 256];
/// Per hit: lr and the next return addresses from the frame-pointer chain (host callers of a host faulting PC).
pub(super) static TRAP_CALLERS: [[AtomicUsize; 4]; 256] = [const { [const { AtomicUsize::new(0) }; 4] }; 256];
pub(super) static TRAP_HIT_N: AtomicUsize = AtomicUsize::new(0);

/// Signal-handler hook for --early-fence-trap: true if `addr` was in a trapped range (now accessible again).
pub(super) unsafe fn early_fence_fault(
    addr: usize,
    pc: usize,
    write: bool,
    lr: usize,
    fp: usize,
) -> bool {
    for (slot, (s, e)) in TRAPPED.iter().enumerate() {
        let start = s.load(Ordering::Acquire);
        if start == 0 || addr < start || addr >= e.load(Ordering::Acquire) {
            continue;
        }
        let end = e.load(Ordering::Acquire);
        let race =
            COMPLETED_SERIAL.load(Ordering::Acquire) < TRAPPED_SERIAL[slot].load(Ordering::Acquire);
        // The slot stays until the next retrap: other threads faulting on it before this mprotect still find it.
        libc::mprotect(
            start as *mut libc::c_void,
            end - start,
            libc::PROT_READ | libc::PROT_WRITE,
        );
        if TRAPPED_HIT[slot].swap(1, Ordering::AcqRel) != 0 {
            return true;
        }
        let i = TRAP_HIT_N.fetch_add(1, Ordering::AcqRel);
        if i < TRAP_HITS.len() {
            TRAP_HITS[i].0.store(pc, Ordering::Relaxed);
            TRAP_HITS[i].1.store(addr, Ordering::Relaxed);
            TRAP_RACE[i].store(race as usize, Ordering::Relaxed);
            TRAP_CALLERS[i][0].store(lr, Ordering::Relaxed);
            let mut f = fp;
            for k in 1..4 {
                if f < 0x10000 || f & 7 != 0 {
                    break;
                }
                TRAP_CALLERS[i][k].store(*((f + 8) as *const usize), Ordering::Relaxed);
                f = *(f as *const usize);
            }
            TRAP_HITS[i].2.store(write as usize, Ordering::Release);
        }
        return true;
    }
    // Unprotected by a retrap between the fault and this handler: retry the access.
    TRAPPED_OLD.iter().any(|(s, e)| {
        let start = s.load(Ordering::Acquire);
        start != 0 && addr >= start && addr < e.load(Ordering::Acquire)
    })
}

extern "C" {
    fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
}

/// At an early fence return: unprotect the previous traps (their work completed), print the hits, protect the
/// storage ranges of command buffers up to `serial` that are still running.
pub(super) fn early_fence_retrap(serial: u64) {
    for ((s, e), (os, oe)) in TRAPPED.iter().zip(TRAPPED_OLD.iter()) {
        let start = s.load(Ordering::Acquire);
        os.store(0, Ordering::Release);
        if start != 0 {
            let end = e.load(Ordering::Acquire);
            oe.store(end, Ordering::Release);
            os.store(start, Ordering::Release);
            unsafe {
                libc::mprotect(
                    start as *mut libc::c_void,
                    end - start,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
            s.store(0, Ordering::Release);
        }
    }
    let n = TRAP_HIT_N.swap(0, Ordering::AcqRel).min(TRAP_HITS.len());
    let base = crate::guest::exec::crash_handler::get_guest_base();
    let slide = unsafe { _dyld_get_image_vmaddr_slide(0) } as usize;
    for ((h, c), race) in TRAP_HITS[..n]
        .iter()
        .zip(TRAP_CALLERS.iter())
        .zip(TRAP_RACE.iter())
    {
        let pc = h.0.load(Ordering::Acquire);
        let rel = if pc >= base && pc < base + (256 << 20) {
            format!("main+{:#x}", pc - base)
        } else {
            format!("{:#x}", pc)
        };
        let callers: Vec<String> = c
            .iter()
            .map(|a| format!("{:#x}", a.swap(0, Ordering::Relaxed)))
            .collect();
        let hit = h.1.load(Ordering::Relaxed);
        let origins: Vec<String> = TRAP_ORIGINS
            .lock()
            .unwrap()
            .iter()
            .filter(|o| hit >= o.0 & !0x3fff && hit < (o.1 + 0x3fff) & !0x3fff)
            .map(|o| {
                format!(
                    "{:#x}+{:#x}{}",
                    o.2,
                    o.3,
                    if hit >= o.0 && hit < o.1 { "*" } else { "" }
                )
            })
            .collect();
        println!("[ef-trap]   bindings on that page: {}", origins.join(" "));
        println!(
            "[ef-trap] f{} {} {} {} {:#x} callers {} slide {:#x}",
            presented_frames(),
            if race.load(Ordering::Relaxed) != 0 {
                "RACE"
            } else {
                "late"
            },
            if h.2.load(Ordering::Acquire) != 0 {
                "write"
            } else {
                "read "
            },
            rel,
            h.1.load(Ordering::Relaxed),
            callers.join(","),
            slide
        );
    }
    let completed = COMPLETED_SERIAL.load(Ordering::Acquire);
    TRAP_ORIGINS.lock().unwrap().retain(|o| o.4 > completed);
    let mut pending = TRAP_PENDING.lock().unwrap();
    pending.retain(|r| r.2 > completed);
    let mut now: Vec<(usize, usize, u64)> =
        pending.iter().filter(|r| r.2 <= serial).copied().collect();
    pending.retain(|r| r.2 > serial);
    drop(pending);
    now.sort_unstable();
    let mut merged: Vec<(usize, usize, u64)> = Vec::new();
    for (a, b, sr) in now {
        match merged.last_mut() {
            Some(m) if a <= m.1 => {
                m.1 = m.1.max(b);
                m.2 = m.2.max(sr);
            }
            _ => merged.push((a, b, sr)),
        }
    }
    for ((((a, b, sr), slot), ss), hit) in merged
        .iter()
        .zip(TRAPPED.iter())
        .zip(TRAPPED_SERIAL.iter())
        .zip(TRAPPED_HIT.iter())
    {
        ss.store(*sr, Ordering::Release);
        hit.store(0, Ordering::Release);
        slot.1.store(*b, Ordering::Release);
        slot.0.store(*a, Ordering::Release);
        unsafe { libc::mprotect(*a as *mut libc::c_void, b - a, libc::PROT_NONE) };
    }
}

/// Crash reports: what the page guards know about `addr`'s page and its actual protection.
pub(super) fn describe_fault_page(addr: usize) -> String {
    extern "C" {
        static mach_task_self_: u32;
        fn mach_vm_region(
            task: u32,
            address: *mut u64,
            size: *mut u64,
            flavor: i32,
            info: *mut i32,
            count: *mut u32,
            object: *mut u32,
        ) -> i32;
    }
    let (mut a, mut size, mut count, mut obj) = (addr as u64, 0u64, 9u32, 0u32);
    let mut info = [0i32; 9]; // vm_region_basic_info_64: protection, max_protection, ...
    let kr = unsafe {
        mach_vm_region(
            mach_task_self_,
            &mut a,
            &mut size,
            9, /* VM_REGION_BASIC_INFO_64 */
            info.as_mut_ptr(),
            &mut count,
            &mut obj,
        )
    };
    let prot = if kr == 0 && a <= addr as u64 {
        format!("prot {} max {}", info[0], info[1])
    } else {
        format!("region query {}", kr)
    };
    format!(
        "{}; report guard: {}; texture backing: {}; write guard holds {}",
        prot,
        report_guard::describe(addr),
        tex_backing::describe(addr),
        fence_guard::holds(addr & !0x3fff)
    )
}

/// The address range of guest code (--report-guard: only accesses made by guest code wait for reports).
pub(crate) fn set_hle_stub_range(start: usize, end: usize) {
    report_guard::set_hle_stub_range(start, end);
}

pub(super) fn note_committed(serial: u64) {
    COMMITTED_SERIAL.fetch_max(serial, std::sync::atomic::Ordering::AcqRel);
}

/// --early-fence (on by default, =0 disables; changes behaviour): guest-main's nvnSyncWait returns once the frame's
/// work is committed instead of when the GPU has finished it, so the game's next frame overlaps the GPU's work on this
/// one (TotK waits on each frame's fence before starting the next); at most one frame overlaps. The game then reads GPU
/// results a frame late (occlusion reports, compute outputs: see note_gpu_guest_write) and may write data the GPU is
/// still reading (seen in the opening). The engine's own readers wait or keep GPU order: render-target write-back
/// waits for its targets, uniform buffers in memory GPU work of earlier submits writes stay zero-copy until it
/// completed (State::carried_dirty).
/// Without it the GPU idles between frames and macOS lowers its clock (about twice the GPU time per frame, under half
/// the frame rate in TotK's Hateno). Each GPU result the game reads right after its fence still needs a
/// special case until guest memory the GPU writes is kept coherent for CPU reads in general.
pub(crate) fn early_fence() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().graphics.early_fence)
}

pub(super) fn on_guest_main() -> bool {
    thread_local! {
        static MAIN: bool = std::thread::current().name() == Some("guest-main");
    }
    MAIN.with(|m| *m)
}

/// The game's fence wait: GPU completion of `serial`, or with --early-fence its commit.
pub fn wait_fence(serial: u64, timeout_ns: u64) -> bool {
    if journal_on() && on_guest_main() {
        let t0 = host_secs() * 1e3;
        let ok = wait_fence_inner(serial, timeout_ns);
        journal(|| format!("fence-wait {} t {:.3}-{:.3}", serial, t0, host_secs() * 1e3));
        return ok;
    }
    wait_fence_inner(serial, timeout_ns)
}

pub(super) fn wait_fence_inner(serial: u64, timeout_ns: u64) -> bool {
    // Only the frame loop's wait (guest-main): other threads wait for GPU work whose results they read back (loading
    // stalled when their fences returned early).
    if !early_fence() || !on_guest_main() {
        return wait_serial(serial, timeout_ns);
    }
    // The previous fence this game waited on must have completed on the GPU: at most one frame overlaps.
    static PREV: AtomicU64 = AtomicU64::new(0);
    let prev = PREV.load(Ordering::Acquire);
    if prev < serial && !wait_serial(prev, timeout_ns) {
        return false;
    }
    if serial > prev {
        PREV.store(serial, Ordering::Release);
    }
    // GPU results the game may read after this fence (storage buffers, copies, occlusion reports) are not there yet.
    static N: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];
    let full =
        GPU_GUEST_WRITE_SERIAL.load(Ordering::Acquire) > COMPLETED_SERIAL.load(Ordering::Acquire);
    N[full as usize].fetch_add(1, Ordering::Relaxed);
    let (early, fulls) = (N[0].load(Ordering::Relaxed), N[1].load(Ordering::Relaxed));
    if (early + fulls) % 600 == 0 && crate::settings::get().log.log_early_fence {
        let k: Vec<u64> = GUEST_WRITE_KINDS
            .iter()
            .map(|k| k.load(Ordering::Relaxed))
            .collect();
        println!("[early-fence] waits returned early {} / full {} (f{}); forcing writes: storage {} copy {} occlusion {}", early, fulls,
            presented_frames(), k[0], k[1], k[2]);
    }
    if full {
        let ok = wait_serial(serial, timeout_ns);
        if early_fence_trap() {
            early_fence_retrap(serial);
        }
        fence_guard::release_all();
        return ok;
    }
    let deadline =
        std::time::Instant::now().checked_add(std::time::Duration::from_nanos(timeout_ns));
    let trap = early_fence_trap();
    while COMMITTED_SERIAL.load(std::sync::atomic::Ordering::Acquire) < serial
        && COMPLETED_SERIAL.load(std::sync::atomic::Ordering::Acquire) < serial
    {
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_micros(50));
    }
    if trap {
        early_fence_retrap(serial);
    }
    // Pages the still-running work reads become read-only: a game write to one waits for it (metal/fence_guard.rs).
    fence_guard::protect(serial);
    true
}

/// Blocks until command buffer `serial` has completed, or `timeout_ns` passed (false).
pub fn wait_serial(serial: u64, timeout_ns: u64) -> bool {
    let Some(g) = GFX.get() else { return true };
    let deadline =
        std::time::Instant::now().checked_add(std::time::Duration::from_nanos(timeout_ns));
    while COMPLETED_SERIAL.load(std::sync::atomic::Ordering::Acquire) < serial {
        if retire_oldest(g, true) {
            continue;
        }
        if g.state.lock().unwrap().inflight.is_empty() {
            return true; // nothing left on the GPU (e.g. rendering halted after an error)
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }
    true
}
