// Native Darwin ARM64 Crash Handler & Diagnostic Tracer

use std::sync::atomic::{AtomicUsize, Ordering};

// Watchpoint: mprotect a page PROT_READ, catch writes, log PC, re-allow and remove.
// Up to 4 simultaneous watched pages.
const MAX_WATCHPOINTS: usize = 4;
static WATCH_PAGES: [AtomicUsize; MAX_WATCHPOINTS] = [
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
];
static WATCH_HIT_TOTAL: AtomicUsize = AtomicUsize::new(0);
// Page waiting to be re-protected after the single-step instruction completes.
static PENDING_REPROTECT: AtomicUsize = AtomicUsize::new(0);
// Maximum write-hits per page before the watchpoint is permanently removed.
const MAX_WATCH_HITS: usize = 60;

/// Install a persistent mprotect watchpoint on the 4 KiB page containing `addr`.
/// The page is re-protected after each write via ARM64 software single-step.
/// Returns true if a slot was available.
/// --watch-addr=0x... : the 16 bytes a page watchpoint reports (0 = report every write to the page).
pub fn watch_focus() -> usize {
    static F: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *F.get_or_init(|| crate::settings::get().trace.watch_addr.map_or(0, |a| a.0))
}

pub fn add_watchpoint(addr: usize) -> bool {
    let page = addr & !0x3fff;
    for slot in &WATCH_PAGES {
        if slot
            .compare_exchange(0, page, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let ret = unsafe { crate::os::protect(page, 0x4000, crate::os::Prot::Read) };
            eprintln!(
                "[WATCH] Watchpoint set on page 0x{:x} (addr=0x{:x}, protect: {:?})",
                page, addr, ret
            );
            return true;
        }
    }
    eprintln!("[WATCH] No watchpoint slots available for 0x{:x}", addr);
    false
}

// ── Guest breakpoint tracer (debug aid, enabled with --trace="0xREL,0xREL,...") ──────────
// A trace point replaces one guest instruction with `brk #0x7a1`. On SIGTRAP the handler logs the
// registers, writes the original instruction back and single-steps it (CPSR.SS, the same mechanism
// the page watchpoints use -- editing pc in the signal context is not honoured on macOS arm64),
// then the follow-up step trap re-arms the brk.
const TRACE_MAX: usize = 48;
static TRACE_PCS: [AtomicUsize; TRACE_MAX] = [const { AtomicUsize::new(0) }; TRACE_MAX];
static TRACE_ORIG: [AtomicUsize; TRACE_MAX] = [const { AtomicUsize::new(0) }; TRACE_MAX];
static TRACE_HITS: [AtomicUsize; TRACE_MAX] = [const { AtomicUsize::new(0) }; TRACE_MAX];
static TRACE_DUMP_REG: [AtomicUsize; TRACE_MAX] =
    [const { AtomicUsize::new(usize::MAX) }; TRACE_MAX];
static TRACE_DUMP_DEREF: [AtomicUsize; TRACE_MAX] =
    [const { AtomicUsize::new(usize::MAX) }; TRACE_MAX];
static PENDING_TRACE: AtomicUsize = AtomicUsize::new(0); // slot + 1 while a displaced insn is stepping
static PENDING_NEXT_ORIG: AtomicUsize = AtomicUsize::new(0); // original word at pc+4 (temporary step brk)
const TRACE_MAX_HITS: usize = 20000;
const BRK_TRACE: u32 = 0xd420_f420; // brk #0x7a1

unsafe fn trace_write_insn(pc: usize, insn: u32) {
    // Text is R-X once the engine is running, so flip the page to RW for the write. Another
    // thread executing inside this 16KB page during that window would fault; acceptable for a
    // debug-only tool.
    let page = pc & !0x3fff;
    let _ = crate::os::protect(page as usize, 0x4000, crate::os::Prot::ReadWrite);
    *(pc as *mut u32) = insn;
    let _ = crate::os::protect(page as usize, 0x4000, crate::os::Prot::ReadExec);
    crate::os::flush_icache(pc as usize, 4);
}

/// Install a trace point at guest address `pc` (must be in writable text).
pub unsafe fn install_trace_point(
    pc: usize,
    dump_reg: Option<usize>,
    deref_off: Option<usize>,
) -> bool {
    let w = *(pc as *const u32);
    if (w & 0xfc00_0000) == 0x1400_0000
        || (w & 0xfe00_0000) == 0xd600_0000
        || (w & 0x7e00_0000) == 0x3400_0000
        || (w & 0x7e00_0000) == 0x3600_0000
        || (w & 0xff00_0010) == 0x5400_0000
    {
        eprintln!("[TRACE] {:#x}: {:#010x} is a branch; software single-step needs a fall-through instruction", pc, w);
        return false;
    }
    let Some(slot) = TRACE_PCS.iter().position(|a| a.load(Ordering::SeqCst) == 0) else {
        return false;
    };
    TRACE_DUMP_REG[slot].store(dump_reg.unwrap_or(usize::MAX), Ordering::SeqCst);
    TRACE_DUMP_DEREF[slot].store(deref_off.unwrap_or(usize::MAX), Ordering::SeqCst);
    TRACE_ORIG[slot].store(*(pc as *const u32) as usize, Ordering::SeqCst);
    // Startup: guest text is still writable, so no mprotect (it would break later startup hooks
    // that patch other words in the same page).
    *(pc as *mut u32) = BRK_TRACE;
    crate::os::flush_icache(pc as usize, 4);
    TRACE_PCS[slot].store(pc, Ordering::SeqCst);
    true
}

/// Trace points held back until a presented frame (--trace-from-frame): (pc, dump_reg, deref_off).
static DEFERRED_TRACE: std::sync::Mutex<Vec<(usize, Option<usize>, Option<usize>)>> =
    std::sync::Mutex::new(Vec::new());
pub fn defer_trace_point(pc: usize, dump_reg: Option<usize>, deref_off: Option<usize>) {
    DEFERRED_TRACE
        .lock()
        .unwrap()
        .push((pc, dump_reg, deref_off));
}
/// Installs the deferred trace points (called by present at the requested frame). Text is R-X by then, so the trap is
/// written through trace_write_insn.
pub unsafe fn install_deferred_trace_points() {
    for (pc, dump_reg, deref_off) in std::mem::take(&mut *DEFERRED_TRACE.lock().unwrap()) {
        let Some(slot) = TRACE_PCS.iter().position(|a| a.load(Ordering::SeqCst) == 0) else {
            return;
        };
        TRACE_DUMP_REG[slot].store(dump_reg.unwrap_or(usize::MAX), Ordering::SeqCst);
        TRACE_DUMP_DEREF[slot].store(deref_off.unwrap_or(usize::MAX), Ordering::SeqCst);
        TRACE_ORIG[slot].store(*(pc as *const u32) as usize, Ordering::SeqCst);
        TRACE_PCS[slot].store(pc, Ordering::SeqCst);
        trace_write_insn(pc, BRK_TRACE);
        eprintln!(
            "[+] TRACE point armed at {:#x} (deferred)",
            pc.wrapping_sub(get_guest_base())
        );
    }
}

thread_local! {
    /// Address of the last fault retried as a late one on this thread (see handle_signal).
    static LATE_RETRY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
/// Faults retried because their page had become accessible by the time the handler ran.
pub static LATE_RETRIES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many bytes from `addr` (up to `max`) can be read: whole 16 KiB pages up to the first one that cannot.
pub unsafe fn readable_len(addr: usize, max: usize) -> usize {
    let end = addr + max;
    let mut page = addr & !0x3fff;
    while page < end {
        if !page_accessible(page.max(addr), false) {
            return page.max(addr) - addr;
        }
        page += 0x4000;
    }
    max
}

/// The VM region containing `addr`, or else the next one above it: (start, end, protection). mach_vm_region is
/// signal-safe enough (a kernel trap, no allocation).
pub(crate) unsafe fn vm_region(addr: usize) -> Option<(usize, usize, i32)> {
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
    const VM_REGION_BASIC_INFO_64: i32 = 9;
    let (mut a, mut size, mut count, mut obj) = (addr as u64, 0u64, 9u32, 0u32);
    let mut info = [0i32; 9];
    let kr = mach_vm_region(
        mach_task_self_,
        &mut a,
        &mut size,
        VM_REGION_BASIC_INFO_64,
        info.as_mut_ptr(),
        &mut count,
        &mut obj,
    );
    (kr == 0).then(|| (a as usize, (a + size) as usize, info[0]))
}

/// Whether `addr`'s page allows the access now.
unsafe fn page_accessible(addr: usize, write: bool) -> bool {
    if addr < 0x10000 {
        return false;
    }
    match vm_region(addr) {
        Some((start, _, prot)) if start <= addr => prot & 1 != 0 && (!write || prot & 2 != 0),
        _ => false,
    }
}

static MAIN_SIZE: AtomicUsize = AtomicUsize::new(0);

/// The size of main's image (text through bss) once it is loaded, for crash reports.
pub fn set_main_size(size: usize) {
    MAIN_SIZE.store(size, Ordering::SeqCst);
}

/// Whether `addr` lies in main's image.
fn in_main(addr: usize) -> bool {
    let base = crate::guest::loader::main_base();
    base != 0 && addr >= base && addr - base < MAIN_SIZE.load(Ordering::Relaxed)
}

pub fn get_guest_base() -> usize {
    crate::guest::loader::main_base()
}

pub unsafe fn install_signal_handlers() {
    let mut sa: libc::sigaction = std::mem::zeroed();
    sa.sa_sigaction = handle_signal as *const () as usize;
    // No SA_RESETHAND: this process takes frequent *recoverable* faults (Darwin clobbering tpidr_el0, the
    // tracer's brk, watchpoints) on many threads at once. With a one-shot handler the second thread to
    // fault before the first re-installed it hit SIG_DFL and the whole process died silently (exit 139,
    // no report). A fault inside the handler still terminates: the signal is blocked while it runs.
    // SA_ONSTACK: run on the thread's alternate signal stack (ensure_altstack). Without it a stack overflow cannot be
    // reported: the kernel finds no room for the signal frame and kills the process with a bare SIGILL.
    sa.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;

    libc::sigaction(libc::SIGSEGV, &sa, std::ptr::null_mut());
    libc::sigaction(libc::SIGBUS, &sa, std::ptr::null_mut());
    libc::sigaction(libc::SIGILL, &sa, std::ptr::null_mut());
    libc::sigaction(libc::SIGTRAP, &sa, std::ptr::null_mut());
}

/// Gives the calling thread an alternate signal stack if it has none (Rust's runtime only sets one up for threads it
/// starts while its own handler is installed). Call at the start of every thread that runs guest code.
pub fn ensure_altstack() {
    const SIZE: usize = 256 * 1024;
    unsafe {
        let mut cur: libc::stack_t = std::mem::zeroed();
        if libc::sigaltstack(std::ptr::null(), &mut cur) == 0
            && cur.ss_flags & libc::SS_DISABLE == 0
            && !cur.ss_sp.is_null()
        {
            return;
        }
        let p = libc::mmap(
            std::ptr::null_mut(),
            SIZE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        );
        if p == libc::MAP_FAILED {
            return;
        }
        let st = libc::stack_t {
            ss_sp: p,
            ss_size: SIZE,
            ss_flags: 0,
        };
        libc::sigaltstack(&st, std::ptr::null_mut());
    }
}

#[repr(C)]
struct DlInfo {
    dli_fname: *const libc::c_char,
    dli_fbase: *mut libc::c_void,
    dli_sname: *const libc::c_char,
    dli_saddr: *mut libc::c_void,
}

extern "C" {
    fn dladdr(addr: *const libc::c_void, info: *mut DlInfo) -> libc::c_int;
}

#[cfg(target_vendor = "apple")]
/// Write faults (address, pc, lr) for --log-ubo-writers; written lock-free from the signal handler.
pub static WRITER_LOG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Pages (16 KiB, absolute page numbers, addresses below 16 GiB) the probe made read-only.
pub static WRITER_PAGES: [std::sync::atomic::AtomicU64; 1 << 14] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 1 << 14];
/// Pages the probe ever armed (a concurrent fault on one is retried).
pub static WRITER_ARMED: [std::sync::atomic::AtomicU64; 1 << 14] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 1 << 14];
pub static WRITER_IDX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static WRITER_RING: [(
    std::sync::atomic::AtomicU64,
    std::sync::atomic::AtomicU64,
    std::sync::atomic::AtomicU64,
); 65536] = [const {
    (
        std::sync::atomic::AtomicU64::new(0),
        std::sync::atomic::AtomicU64::new(0),
        std::sync::atomic::AtomicU64::new(0),
    )
}; 65536];

/// A store-exclusive (stxr / stlxr / stxp / stlxp) that faulted on a page a handler just made writable is not retried:
/// it reports failure (status register 1) and is stepped over, and the guest's loop starts again at its load-exclusive.
/// The faulting thread slept in the handler, maybe on another core, between its load-exclusive and this store; a replayed
/// store could succeed although another thread took the location meanwhile (two threads then held a game spinlock:
/// Breath of the Wild ran the same jobs twice). A store-exclusive may always fail spuriously.
unsafe fn fail_store_exclusive(ucontext: *mut libc::c_void) {
    if ucontext.is_null() {
        return;
    }
    let mc = (*(ucontext as *mut libc::ucontext_t)).uc_mcontext;
    if mc.is_null() {
        return;
    }
    let pc = (*mc).__ss.__pc as usize;
    let insn = *(pc as *const u32);
    // Load/store exclusive class with o2 = 0 and L = 0: the store-exclusives; Rs (bits 20:16) receives the status.
    if insn & 0x3fc0_0000 == 0x0800_0000 {
        match ((insn >> 16) & 31) as usize {
            rs @ 0..=28 => (*mc).__ss.__x[rs] = 1,
            29 => (*mc).__ss.__fp = 1,
            30 => (*mc).__ss.__lr = 1,
            _ => {}
        }
        (*mc).__ss.__pc += 4;
    }
}

unsafe extern "C" fn handle_signal(
    sig: i32,
    info: *mut libc::siginfo_t,
    ucontext: *mut libc::c_void,
) {
    let fault_addr = (*info).si_addr as usize;
    let base = crate::guest::loader::main_base();

    // Pages the GPU backend protects to keep guest memory coherent with GPU work (before write tracking, which would
    // claim and unprotect pages it once tracked).
    let access = if (sig == libc::SIGSEGV || sig == libc::SIGBUS) && !ucontext.is_null() {
        let mc = (*(ucontext as *mut libc::ucontext_t)).uc_mcontext;
        (!mc.is_null()).then(|| crate::gpu::Fault {
            addr: fault_addr,
            pc: (*mc).__ss.__pc as usize,
            lr: (*mc).__ss.__lr as usize,
            fp: (*mc).__ss.__fp as usize,
            write: (*mc).__es.__esr & (1 << 6) != 0,
        })
    } else {
        None
    };
    if let Some(f) = &access {
        if crate::gpu::backend().access_fault(f) {
            fail_store_exclusive(ucontext);
            return;
        }
    }
    // --log-ubo-writers (RE probe): pages that held bound uniforms are made read-only after each frame
    // (gfx ubo_content_probe); the first write to one is logged with its instruction and the page made writable again.
    if (sig == libc::SIGSEGV || sig == libc::SIGBUS)
        && !ucontext.is_null()
        && WRITER_LOG.load(Ordering::Relaxed)
    {
        let pg = fault_addr >> 14;
        let mc = (*(ucontext as *mut libc::ucontext_t)).uc_mcontext;
        if pg < WRITER_PAGES.len() * 64
            && WRITER_PAGES[pg / 64].fetch_and(!(1u64 << (pg % 64)), Ordering::Relaxed)
                & (1u64 << (pg % 64))
                != 0
            && !mc.is_null()
        {
            let i = WRITER_IDX.fetch_add(1, Ordering::Relaxed) % WRITER_RING.len();
            WRITER_RING[i].0.store(fault_addr as u64, Ordering::Relaxed);
            WRITER_RING[i]
                .1
                .store((*mc).__ss.__pc as u64, Ordering::Relaxed);
            WRITER_RING[i]
                .2
                .store((*mc).__ss.__lr as u64, Ordering::Relaxed);
            let _ = crate::os::protect(((pg << 14)) as usize, 1 << 14, crate::os::Prot::ReadWrite);
            return;
        }
        // Another thread's fault on the same page unprotected it first: retry the access.
        if pg < WRITER_PAGES.len() * 64
            && WRITER_ARMED
                .get(pg / 64)
                .is_some_and(|w| w.load(Ordering::Relaxed) & (1u64 << (pg % 64)) != 0)
        {
            return;
        }
    }
    // A write to a write-tracked texture page (guest/dirty_track.rs): unprotect it, flag the texture, retry.
    if (sig == libc::SIGSEGV || sig == libc::SIGBUS)
        && crate::guest::dirty_track::handle_fault(fault_addr)
    {
        fail_store_exclusive(ucontext);
        return;
    }
    // The backend's diagnostic traps (--early-fence-trap).
    if let Some(f) = &access {
        if crate::gpu::backend().late_access_fault(f) {
            fail_store_exclusive(ucontext);
            return;
        }
    }

    if sig == libc::SIGTRAP && !ucontext.is_null() {
        let uc = ucontext as *mut libc::ucontext_t;
        let mc = (*uc).uc_mcontext;
        if !mc.is_null() {
            let pending = PENDING_TRACE.load(Ordering::SeqCst);
            if pending != 0 {
                // Software single-step: the displaced instruction ran and we are now at the temporary
                // brk on pc+4. Restore that word, re-arm the trace point, resume at pc+4.
                let i = pending - 1;
                let tpc = TRACE_PCS[i].load(Ordering::SeqCst);
                if ((*mc).__ss.__pc as usize) == tpc + 4 {
                    trace_write_insn(tpc + 4, PENDING_NEXT_ORIG.load(Ordering::SeqCst) as u32);
                    trace_write_insn(tpc, BRK_TRACE);
                    PENDING_TRACE.store(0, Ordering::SeqCst);
                    install_signal_handlers();
                    return;
                }
            }
            let pc = (*mc).__ss.__pc as usize;
            if let Some(i) = TRACE_PCS
                .iter()
                .position(|a| a.load(Ordering::Relaxed) == pc && pc != 0)
            {
                let n = TRACE_HITS[i].fetch_add(1, Ordering::Relaxed);
                if n < TRACE_MAX_HITS {
                    let x = &(*mc).__ss.__x;
                    eprintln!("[TRACE {:#x} #{}] x0={:#x} x1={:#x} x2={:#x} x3={:#x} x4={:#x} x5={:#x} x8={:#x} x19={:#x} x20={:#x} lr={:#x}(rel {:#x}) sp={:#x}",
                        pc.wrapping_sub(base), n, x[0], x[1], x[2], x[3], x[4], x[5], x[8], x[19], x[20],
                        (*mc).__ss.__lr, ((*mc).__ss.__lr as usize).wrapping_sub(base), (*mc).__ss.__sp);
                    // --trace-all-regs: x6..x28 too.
                    if crate::settings::get().trace.trace_all_regs {
                        let rest: Vec<String> =
                            (6..29).map(|r| format!("x{}={:#x}", r, x[r])).collect();
                        eprintln!(
                            "[TRACE {:#x} #{}]   {}",
                            pc.wrapping_sub(base),
                            n,
                            rest.join(" ")
                        );
                    }
                }
                let dr = TRACE_DUMP_REG[i].load(Ordering::Relaxed);
                if dr < 29 && n < TRACE_MAX_HITS {
                    let mut v = (*mc).__ss.__x[dr] as usize;
                    let off = TRACE_DUMP_DEREF[i].load(Ordering::Relaxed);
                    if off != usize::MAX
                        && v >= 0x1_0000_0000
                        && v < 0x8000_0000_0000
                        && (v & 7) == 0
                    {
                        v = *((v + off) as *const usize); // dump [xN + off] instead of xN itself
                    }
                    if v >= 0x1_0000_0000 && v < 0x8000_0000_0000 && (v & 7) == 0 {
                        let words: Vec<String> = (0..24)
                            .map(|k| format!("{:#x}", *((v + k * 8) as *const usize)))
                            .collect();
                        eprintln!("    [TRACE x{} @ {:#x}] {}", dr, v, words.join(" "));
                    }
                }
                trace_write_insn(pc, TRACE_ORIG[i].load(Ordering::SeqCst) as u32);
                // Software single-step: temporary brk on the (fall-through) next instruction.
                PENDING_NEXT_ORIG.store(*((pc + 4) as *const u32) as usize, Ordering::SeqCst);
                trace_write_insn(pc + 4, BRK_TRACE);
                PENDING_TRACE.store(i + 1, Ordering::SeqCst);
                install_signal_handlers(); // SA_RESETHAND cleared them
                return;
            }
        }
    }

    // ── Watchpoint fast path ─────────────────────────────────────────────────
    // If this SIGSEGV or SIGBUS is a write to a watched (read-only protected)
    // page, log the faulting PC, remove the watchpoint, re-allow writes, and
    // return so the instruction retries successfully.
    // On macOS/ARM64, mprotect(PROT_READ) violations sometimes raise SIGBUS.
    if sig == libc::SIGSEGV || sig == libc::SIGBUS {
        let fault_page = fault_addr & !0x3fff;
        'watch: for slot in &WATCH_PAGES {
            let watched = slot.load(Ordering::Relaxed);
            if watched != 0 && watched == fault_page {
                // Extract PC / LR from ucontext
                let (pc, lr) = if !ucontext.is_null() {
                    let uc = ucontext as *mut libc::ucontext_t;
                    let mc = (*uc).uc_mcontext;
                    if !mc.is_null() {
                        ((*mc).__ss.__pc as usize, (*mc).__ss.__lr as usize)
                    } else {
                        (0, 0)
                    }
                } else {
                    (0, 0)
                };

                // Execution fault: PC == fault_addr means the CPU tried to
                // execute non-executable pool memory (e.g. after a bad blr).
                // These are NOT write watchpoint hits — let the crash handler
                // print full diagnostics and exit.
                if pc == fault_addr {
                    break 'watch;
                }

                let rel_pc = if pc >= base && base != 0 {
                    pc - base
                } else {
                    pc
                };
                let rel_lr = if lr >= base && base != 0 {
                    lr - base
                } else {
                    lr
                };
                // --watch-addr focuses the log on 16 bytes; other writes to the page pass silently and uncounted.
                let focus = watch_focus();
                let in_focus = focus == 0 || (fault_addr >= focus && fault_addr < focus + 16);
                let hit = if in_focus {
                    WATCH_HIT_TOTAL.fetch_add(1, Ordering::Relaxed)
                } else {
                    0
                };
                if in_focus {
                    let x = |i: usize| {
                        if !ucontext.is_null() {
                            let mc = (*(ucontext as *mut libc::ucontext_t)).uc_mcontext;
                            if mc.is_null() {
                                0
                            } else {
                                (*mc).__ss.__x[i]
                            }
                        } else {
                            0
                        }
                    };
                    let insn = *(pc as *const u32);
                    let rt = (insn & 0x1f) as usize;
                    // Engine image base, so host return addresses can be symbolized offline (atos -l <base>).
                    let mut info: DlInfo = std::mem::zeroed();
                    dladdr(handle_signal as *const libc::c_void, &mut info);
                    let fp = if !ucontext.is_null() {
                        let mc = (*(ucontext as *mut libc::ucontext_t)).uc_mcontext;
                        if mc.is_null() {
                            0
                        } else {
                            (*mc).__ss.__fp as usize
                        }
                    } else {
                        0
                    };
                    let mut chain = Vec::new();
                    let mut f = fp;
                    for _ in 0..8 {
                        if f < 0x10000 || f & 7 != 0 {
                            break;
                        }
                        chain.push(format!("{:#x}", *((f + 8) as *const usize)));
                        f = *(f as *const usize);
                    }
                    eprintln!("[WATCH] hit #{} write to 0x{:x} from PC=0x{:x} LR=0x{:x} engine_base={:p} insn={:08x} value(x{})={:#x} frames=[{}]",
                        hit, fault_addr, pc, lr, info.dli_fbase, insn, rt, if rt < 31 { x(rt) } else { 0 }, chain.join(" "));
                    let _ = (rel_pc, rel_lr);
                }

                // Re-enable writes so the faulting instruction can complete.
                let _ = crate::os::protect(fault_page as usize, 0x4000, crate::os::Prot::ReadWrite);
                if hit < MAX_WATCH_HITS {
                    // Arm the single-step bit so we re-protect after one instruction.
                    // ARM64 CPSR bit 21 = SS (Software Step).
                    if !ucontext.is_null() {
                        let uc = ucontext as *mut libc::ucontext_t;
                        let mc = (*uc).uc_mcontext;
                        if !mc.is_null() {
                            (*mc).__ss.__cpsr |= 1 << 21; // SS bit
                        }
                    }
                    // Record which page to re-protect in the SIGTRAP handler.
                    PENDING_REPROTECT.store(fault_page, Ordering::SeqCst);
                    // Keep the slot live so the watchpoint fires again after re-protect.
                } else {
                    // Too many hits — permanently remove this watchpoint.
                    slot.store(0, Ordering::SeqCst);
                    eprintln!(
                        "[WATCH] page 0x{:x} watchpoint removed after {} hits",
                        fault_page, hit
                    );
                }
                // Re-install handlers (SA_RESETHAND cleared them)
                install_signal_handlers();
                return; // faulting instruction will be retried
            }
        }
    }
    // ─────────────────────────────────────────────────────────────────────────

    // ── Single-step re-protect path ──────────────────────────────────────────
    // After a watchpoint hit we armed the SS bit; the processor fires SIGTRAP
    // after the faulting instruction completes.  Re-protect the page and clear
    // the pending record.  If TotK's own code uses software step, this guard
    // keeps us from eating its trap.
    if sig == libc::SIGTRAP {
        let pending = PENDING_REPROTECT.load(Ordering::SeqCst);
        if pending != 0 {
            PENDING_REPROTECT.store(0, Ordering::SeqCst);
            let _ = crate::os::protect(pending as usize, 0x4000, crate::os::Prot::Read);
            // Clear the SS bit so execution continues normally.
            if !ucontext.is_null() {
                let uc = ucontext as *mut libc::ucontext_t;
                let mc = (*uc).uc_mcontext;
                if !mc.is_null() {
                    (*mc).__ss.__cpsr &= !(1u32 << 21);
                }
            }
            install_signal_handlers();
            return; // continue execution; page re-protected, watchpoint still live
        }
    }
    // ─────────────────────────────────────────────────────────────────────────

    // A fault no handler claimed, on a page that is accessible now: delivered late, after a page guard (report guard,
    // texture backing, write guard, write tracking) had already opened the page again. Retry once (a second fault at the
    // same address on this thread is a real one). Three iPhone crashes in the occlusion reader were such late faults.
    if (sig == libc::SIGSEGV || sig == libc::SIGBUS) && !ucontext.is_null() {
        let mc = (*(ucontext as *mut libc::ucontext_t)).uc_mcontext;
        let write = !mc.is_null() && (*mc).__es.__esr & (1 << 6) != 0;
        if page_accessible(fault_addr, write)
            && LATE_RETRY.with(|r| r.replace(fault_addr)) != fault_addr
        {
            LATE_RETRIES.fetch_add(1, Ordering::Relaxed);
            install_signal_handlers();
            return;
        }
    }

    let sig_name = match sig {
        libc::SIGSEGV => "SIGSEGV (Segmentation Fault)",
        libc::SIGBUS => "SIGBUS (Bus Error)",
        libc::SIGILL => "SIGILL (Illegal Instruction)",
        libc::SIGTRAP => "SIGTRAP (Breakpoint / Trap)",
        _ => "Signal",
    };

    eprintln!("\n========================================================");
    eprintln!("[-] GUEST CPU TRAP: Caught {}!", sig_name);
    // Debug: --dump-at-crash=0x... hexdumps 512 bytes from that address (what overwrote a corrupted object).
    if let Some(crate::settings::Addr(a)) = crate::settings::get().dump.dump_at_crash {
        for row in 0..32 {
            let p = (a + row * 16) as *const u32;
            let w = [*p, *p.add(1), *p.add(2), *p.add(3)];
            let f: Vec<String> = w
                .iter()
                .map(|x| format!("{:.4}", f32::from_bits(*x)))
                .collect();
            eprintln!(
                "[crashdump] {:#x}: {:08x} {:08x} {:08x} {:08x}   {}",
                a + row * 16,
                w[0],
                w[1],
                w[2],
                w[3],
                f.join(" ")
            );
        }
    }

    if !ucontext.is_null() {
        let ucontext = ucontext as *mut libc::ucontext_t;
        let mc = (*ucontext).uc_mcontext;
        if !mc.is_null() {
            let pc = (*mc).__ss.__pc as usize;
            let lr = (*mc).__ss.__lr as usize;
            let fp = (*mc).__ss.__fp as usize;
            let sp = (*mc).__ss.__sp as usize;
            let rel_pc = if pc >= base { pc - base } else { pc };
            let rel_lr = if lr >= base { lr - base } else { lr };

            eprintln!(
                "    Program Counter (PC) : 0x{:016X} (Base + 0x{:X})",
                pc, rel_pc
            );
            // Engine code: its image and nearest exported symbol (offsets for `atos -o <binary> -l <image base>`).
            for (what, a) in [("PC", pc as usize), ("LR", (*mc).__ss.__lr as usize)] {
                let mut info: libc::Dl_info = std::mem::zeroed();
                if libc::dladdr(a as *const libc::c_void, &mut info) != 0
                    && !info.dli_fbase.is_null()
                {
                    let sym = if info.dli_sname.is_null() {
                        "?".into()
                    } else {
                        std::ffi::CStr::from_ptr(info.dli_sname)
                            .to_string_lossy()
                            .into_owned()
                    };
                    eprintln!(
                        "    {} in image base {:p} + {:#x}: {} + {:#x}",
                        what,
                        info.dli_fbase,
                        a - info.dli_fbase as usize,
                        sym,
                        a.wrapping_sub(info.dli_saddr as usize)
                    );
                }
            }
            eprintln!("    Faulting Memory Addr : 0x{:016X}", fault_addr);
            match vm_region(fault_addr) {
                Some((lo, hi, prot)) if lo <= fault_addr => {
                    eprintln!("    Fault region         : {:#x}..{:#x} prot {:#x}", lo, hi, prot)
                }
                Some((lo, _, _)) => eprintln!("    Fault region         : unmapped (next region at {:#x})", lo),
                None => eprintln!("    Fault region         : unmapped"),
            }
            // Which page guards know the faulting page (an unclaimed fault on a page one of them protected).
            eprintln!(
                "    Page guards          : {}",
                crate::gpu::backend().describe_page(fault_addr)
            );
            eprintln!("    SP={:#x}  LR={:#x} (Base+{:#x})", sp, lr, rel_lr);
            {
                // Stack overflow check: the thread's stack spans [hi - size, hi); the guard page sits just below.
                let me = libc::pthread_self();
                // Guest code runs on its own stack, HLE calls on the thread's (horizon/thread_stack.rs).
                let (lo, hi) = crate::horizon::thread_stack::current_bounds()
                    .filter(|&(lo, hi)| sp >= lo - 0x10_0000 && sp < hi)
                    .unwrap_or_else(|| {
                        let hi = libc::pthread_get_stackaddr_np(me) as usize;
                        (hi.saturating_sub(libc::pthread_get_stacksize_np(me)), hi)
                    });
                let size = hi - lo;
                let mut name = [0u8; 64];
                libc::pthread_getname_np(me, name.as_mut_ptr() as *mut libc::c_char, name.len());
                let name = std::ffi::CStr::from_bytes_until_nul(&name)
                    .map(|c| c.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let overflow = fault_addr < lo && fault_addr + 0x10_0000 >= lo
                    || sp < lo && sp + 0x10_0000 >= lo;
                eprintln!(
                    "    Thread '{}' stack {:#x}..{:#x} ({} KiB){}",
                    name,
                    lo,
                    hi,
                    size / 1024,
                    if overflow {
                        "  <-- STACK OVERFLOW (fault/SP just below the stack)"
                    } else {
                        ""
                    }
                );
            }
            eprintln!("    Registers at fault:");
            for i in 0..14 {
                eprintln!(
                    "      X{:02}={:#018x}   X{:02}={:#018x}",
                    i,
                    (*mc).__ss.__x[i],
                    i + 14,
                    (*mc).__ss.__x[i + 14]
                );
            }
            eprintln!(
                "      X28={:#018x}   FP={:#018x}   LR={:#018x}",
                (*mc).__ss.__x[28],
                (*mc).__ss.__fp,
                (*mc).__ss.__lr
            );
            // Debug: --crash-stackdump=<bytes> dumps that much raw stack from SP, plus 0x80 bytes behind every
            // stack word that points at readable memory (to recover the objects of the frames above the fault).
            if let Some(crate::settings::Addr(n)) = crate::settings::get().dump.crash_stackdump {
                let readable = |a: usize, len: usize| -> bool {
                    // write() reports EFAULT for unmapped memory instead of faulting.
                    a >= 0x1_0000_0000 && a < 0x10_0000_0000 && {
                        let fd = libc::open(
                            b"/dev/null\0".as_ptr() as *const libc::c_char,
                            libc::O_WRONLY,
                        );
                        let ok = libc::write(fd, a as *const libc::c_void, len) == len as isize;
                        libc::close(fd);
                        ok
                    }
                };
                for off in (0..n).step_by(8) {
                    let w = *((sp + off) as *const usize);
                    eprintln!(
                        "[stackdump] SP+{:#05x}: {:#018x}{}",
                        off,
                        w,
                        if in_main(w) {
                            format!(" (base+{:#x})", w - base)
                        } else {
                            String::new()
                        }
                    );
                    if !in_main(w) && w & 7 == 0 && readable(w, 0x80) {
                        for row in 0..8 {
                            let p = (w + row * 16) as *const usize;
                            eprintln!(
                                "[stackdump]     {:#x}: {:#018x} {:#018x}",
                                w + row * 16,
                                *p,
                                *p.add(1)
                            );
                        }
                    }
                }
            }
            #[cfg(target_arch = "aarch64")]
            {
                let tpidrro: usize;
                let tpidr: usize;
                std::arch::asm!("mrs {}, tpidrro_el0", out(reg) tpidrro);
                std::arch::asm!("mrs {}, tpidr_el0", out(reg) tpidr);
                eprintln!(
                    "    Thread Registers: TPIDRRO_EL0=0x{:016X} TPIDR_EL0=0x{:016X}",
                    tpidrro, tpidr
                );
                if tpidrro > 0x10000 {
                    eprintln!("    At TPIDRRO_EL0:");
                    for off in 0..16 {
                        let val = unsafe { *((tpidrro + off * 8) as *const usize) };
                        eprintln!("      +0x{:02X}: 0x{:016X}", off * 8, val);
                    }
                }
            }
            crate::horizon::hle::dump_hle_call_history();
            let mut dli: DlInfo = std::mem::zeroed();
            if dladdr(pc as *const libc::c_void, &mut dli) != 0 && !dli.dli_sname.is_null() {
                let sname = std::ffi::CStr::from_ptr(dli.dli_sname).to_string_lossy();
                let fname = if !dli.dli_fname.is_null() {
                    std::ffi::CStr::from_ptr(dli.dli_fname).to_string_lossy()
                } else {
                    std::borrow::Cow::Borrowed("unknown")
                };
                eprintln!("      PC Symbol          : {} in {}", sname, fname);
            }

            let mut lr_dli: DlInfo = std::mem::zeroed();
            if dladdr(lr as *const libc::c_void, &mut lr_dli) != 0 && !lr_dli.dli_sname.is_null() {
                let sname = std::ffi::CStr::from_ptr(lr_dli.dli_sname).to_string_lossy();
                eprintln!("      LR Symbol          : {}", sname);
            }

            if in_main(pc) && in_main(pc - 32) {
                let code_base = pc - 32;
                let slice = std::slice::from_raw_parts(code_base as *const u8, 48);
                eprintln!("    Code bytes around PC (-32..+16):");
                eprintln!("      {:02X?}", slice);
            }

            // Scan wider stack range for guest code return addresses (backtrace without FP)
            eprintln!("    Guest-code pointers on stack (SP to SP+0x800):");
            if sp % 8 == 0 && sp > 0x10000 && base != 0 {
                let sp_ptr = sp as *const usize;
                for i in 0..256 {
                    let val = *sp_ptr.add(i);
                    if in_main(val) {
                        eprintln!("      [SP + 0x{:03X}]: base+{:#x}", i * 8, val - base);
                    }
                }
            }

            // Backtrace via Frame Pointers
            eprintln!("    Call Stack Backtrace:");
            let mut cur_fp = fp;
            let mut cur_pc = pc;
            for depth in 0..16 {
                let rel = if in_main(cur_pc) {
                    format!("(Base + 0x{:X})", cur_pc - base)
                } else {
                    "".to_string()
                };
                let mut frame_dli: DlInfo = std::mem::zeroed();
                let sym = if dladdr(cur_pc as *const libc::c_void, &mut frame_dli) != 0
                    && !frame_dli.dli_sname.is_null()
                {
                    format!(
                        " <{}>",
                        std::ffi::CStr::from_ptr(frame_dli.dli_sname).to_string_lossy()
                    )
                } else {
                    "".to_string()
                };
                eprintln!("      #{:02}: 0x{:016X} {}{}", depth, cur_pc, rel, sym);

                if cur_fp < 0x1000 || cur_fp % 8 != 0 {
                    break;
                }
                let fp_ptr = cur_fp as *const usize;
                let next_fp = *fp_ptr;
                let ret_lr = *fp_ptr.add(1);
                if next_fp <= cur_fp || ret_lr == 0 {
                    break;
                }
                cur_fp = next_fp;
                cur_pc = ret_lr;
            }
        }
    }

    eprintln!("========================================================\n");
    std::process::exit(128 + sig);
}

/// Whether `[addr, addr + len)` lies in one readable VM region (diagnostics reading memory a guest object points at).
pub(crate) fn readable(addr: usize, len: usize) -> bool {
    unsafe { vm_region(addr) }.is_some_and(|(start, end, prot)| start <= addr && addr + len <= end && prot & 1 != 0)
}
