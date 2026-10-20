// Write tracking for guest textures the CPU may write behind our back (TotK's font glyph atlas: glyphs are rasterized
// straight into it; terrain tile maps streamed into reused memory). Hashing every such texture each frame to notice
// changes cost ~45 MB of hashing per frame in Hateno (260 terrain textures of 260x260) on the encoding thread.
// Instead a texture's pages are made read-only after each check; the first write to a page faults, the signal handler
// (cpu/crash_handler.rs) makes the page writable again and bumps the page's write generation, and a check is skipped while
// the generations of the texture's pages sum to what they were when it was last hashed.
//
// State is per 16 KiB host page, in flat arrays per registered domain (the guest heap blocks and the address-region
// arena), so the fault handler finds a page by arithmetic and only touches atomics. Pages may be shared with other
// data: a write to a neighbour just costs that texture a re-hash. Callers only track memory inside NVN memory pools.
// Writes by syscalls cannot fault (they fail with EFAULT), so code that lets the host kernel write guest memory (file
// reads) calls `before_host_write` first. GPU writes cannot fault either (and are lost on read-only pages): pages GPU
// buffers access are never protected again until released. --no-dirty-track disables tracking.

use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU8, AtomicUsize, Ordering};

const PAGE: usize = 0x4000;
const DOMAINS: usize = 8;
/// Page flags.
const PROTECTED: u8 = 1; // read-only by us now
const GPU: u8 = 2; // accessed by GPU buffers: never protect
const EVER: u8 = 4; // protected by us at some point since the last release (faults here are ours)

// Generations only increase, so the sum over a texture's pages changes whenever any page is written. Ordering makes a
// skipped check safe: a consumer reads the generations *before* re-protecting, and the fault handler bumps them
// *after* unprotecting, so any page left writable after a consumer's re-protect was unprotected by a handler whose
// bump that consumer has not seen.
struct Domain {
    base: AtomicUsize,
    pages: AtomicUsize,
    gens: AtomicPtr<AtomicU32>,
    flags: AtomicPtr<AtomicU8>,
}

#[allow(clippy::declare_interior_mutable_const)]
const EMPTY: Domain = Domain {
    base: AtomicUsize::new(0),
    pages: AtomicUsize::new(0),
    gens: AtomicPtr::new(std::ptr::null_mut()),
    flags: AtomicPtr::new(std::ptr::null_mut()),
};
static DOMAIN_TABLE: [Domain; DOMAINS] = [EMPTY; DOMAINS];
static FAULTS: AtomicUsize = AtomicUsize::new(0);
/// Pages ever protected (fast path for the per-bind GPU release).
static ANY: AtomicUsize = AtomicUsize::new(0);
static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn log() -> bool {
    *LOG.get_or_init(|| crate::settings::get().log.log_dirty_track)
}

/// Makes [base, base+len) trackable (guest heap blocks, the address-region arena). Arrays are lazily committed zero
/// pages. Call before guest code can touch the range's pages.
pub fn register_domain(base: usize, len: usize) {
    let base = base & !(PAGE - 1);
    let pages = (len + PAGE - 1) / PAGE;
    let map = |bytes: usize| unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            bytes,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    let (g, f) = (map(pages * 4), map(pages));
    if g == libc::MAP_FAILED || f == libc::MAP_FAILED {
        return;
    }
    for d in DOMAIN_TABLE.iter() {
        if d.pages.load(Ordering::Acquire) == 0 {
            d.gens.store(g as *mut AtomicU32, Ordering::Release);
            d.flags.store(f as *mut AtomicU8, Ordering::Release);
            d.base.store(base, Ordering::Release);
            d.pages.store(pages, Ordering::Release);
            println!(
                "[+] Dirty tracking: domain {:#x}..{:#x}",
                base,
                base + pages * PAGE
            );
            return;
        }
    }
}

/// The domain arrays and first page index for [addr, addr+len), if wholly inside one domain.
fn locate(
    addr: usize,
    len: usize,
) -> Option<(&'static [AtomicU32], &'static [AtomicU8], usize, usize)> {
    for d in DOMAIN_TABLE.iter() {
        let pages = d.pages.load(Ordering::Acquire);
        if pages == 0 {
            continue;
        }
        let base = d.base.load(Ordering::Acquire);
        if addr >= base && addr + len <= base + pages * PAGE {
            let first = (addr - base) / PAGE;
            let last = (addr + len.max(1) - 1 - base) / PAGE;
            unsafe {
                let g = std::slice::from_raw_parts(d.gens.load(Ordering::Acquire), pages);
                let f = std::slice::from_raw_parts(d.flags.load(Ordering::Acquire), pages);
                return Some((g, f, first, last));
            }
        }
    }
    None
}

/// The pages overlapping [addr, addr+len) clipped to each domain they touch.
fn for_each_overlap(
    addr: usize,
    len: usize,
    mut f: impl FnMut(&'static [AtomicU32], &'static [AtomicU8], usize, usize, usize),
) {
    for d in DOMAIN_TABLE.iter() {
        let pages = d.pages.load(Ordering::Acquire);
        if pages == 0 || len == 0 {
            continue;
        }
        let base = d.base.load(Ordering::Acquire);
        let (a, b) = (addr.max(base), (addr + len).min(base + pages * PAGE));
        if a >= b {
            continue;
        }
        unsafe {
            let g = std::slice::from_raw_parts(d.gens.load(Ordering::Acquire), pages);
            let fl = std::slice::from_raw_parts(d.flags.load(Ordering::Acquire), pages);
            f(g, fl, (a - base) / PAGE, (b - 1 - base) / PAGE, base);
        }
    }
}

/// Whether [start, start+len) can be tracked.
pub fn trackable(start: usize, len: usize) -> bool {
    len >= PAGE / 2 && locate(start, len).is_some()
}

/// The range's current write token, if every page of it is still protected by us (None: check it).
pub fn generation(start: usize, len: usize) -> Option<u64> {
    let (g, f, first, last) = locate(start, len)?;
    let mut sum = 0u64;
    for p in first..=last {
        if f[p].load(Ordering::Acquire) & (PROTECTED | GPU) != PROTECTED {
            return None;
        }
        sum = sum.wrapping_add(g[p].load(Ordering::Acquire) as u64);
    }
    Some(sum)
}

/// (Re)protects the pages of [start, start+len) and returns the token to remember for this check. Call before reading
/// the contents, so writes made during the check change it. None if the range cannot be tracked now.
pub unsafe fn protect(start: usize, len: usize) -> Option<u64> {
    let (g, f, first, last) = locate(start, len)?;
    if (first..=last).any(|p| f[p].load(Ordering::Acquire) & GPU != 0) {
        return None;
    }
    // Generations before protecting (see the comment on `Domain`).
    let mut sum = 0u64;
    for p in first..=last {
        sum = sum.wrapping_add(g[p].load(Ordering::Acquire) as u64);
    }
    let page0 = start & !(PAGE - 1);
    let end = (start + len + PAGE - 1) & !(PAGE - 1);
    for p in first..=last {
        if f[p].fetch_or(PROTECTED | EVER, Ordering::AcqRel) & EVER == 0 {
            ANY.fetch_add(1, Ordering::Relaxed);
        }
    }
    let r = crate::os::protect(page0, end - page0, crate::os::Prot::Read);
    crate::gpu::backend().pages_reprotected(page0, end - page0);
    if r.is_err() {
        for p in first..=last {
            f[p].fetch_and(!PROTECTED, Ordering::AcqRel);
        }
        return None;
    }
    if log() {
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) % 2000 == 0 {
            println!(
                "[dirty-track] protect {:#x}+{:#x} token {:#x} faults so far {}",
                start,
                len,
                sum,
                FAULTS.load(Ordering::Relaxed)
            );
        }
    }
    Some(sum)
}

/// Signal-handler hook: a write fault at `addr` on a page we protected makes the page writable and bumps its
/// generation. Returns true if the fault was ours (the faulting instruction can be retried). Pages protected at any
/// point since their last release count as ours, so a second thread faulting on a page another thread's handler just
/// unprotected is retried too.
pub unsafe fn handle_fault(addr: usize) -> bool {
    let Some((g, f, p, _)) = locate(addr, 1) else {
        return false;
    };
    if f[p].load(Ordering::Acquire) & EVER == 0 {
        return false;
    }
    // Clear the flag, unprotect, then bump (see the comment on `Domain`). The flag goes first: a check that still saw
    // it set while the page was already writable could skip a texture the write is about to change.
    f[p].fetch_and(!PROTECTED, Ordering::AcqRel);
    let _ = crate::os::protect(((addr & !(PAGE - 1))) as usize, PAGE, crate::os::Prot::ReadWrite);
    crate::gpu::backend().pages_reprotected(addr & !(PAGE - 1), PAGE);
    g[p].fetch_add(1, Ordering::AcqRel);
    FAULTS.fetch_add(1, Ordering::Relaxed);
    true
}

/// Makes the protected pages among [first, last] writable and bumps their generations.
unsafe fn unprotect_pages(
    g: &[AtomicU32],
    f: &[AtomicU8],
    first: usize,
    last: usize,
    base: usize,
    extra: u8,
) {
    let mut p = first;
    while p <= last {
        if f[p].load(Ordering::Acquire) & PROTECTED == 0 {
            f[p].fetch_or(extra, Ordering::AcqRel);
            p += 1;
            continue;
        }
        // A run of protected pages: one mprotect.
        let run = p;
        while p <= last && f[p].load(Ordering::Acquire) & PROTECTED != 0 {
            p += 1;
        }
        for q in run..p {
            f[q].fetch_and(!PROTECTED, Ordering::AcqRel);
            f[q].fetch_or(extra, Ordering::AcqRel);
        }
        let _ = crate::os::protect(((base + run * PAGE)) as usize, (p - run) * PAGE, crate::os::Prot::ReadWrite);
        crate::gpu::backend().pages_reprotected(base + run * PAGE, (p - run) * PAGE);
        for q in run..p {
            g[q].fetch_add(1, Ordering::AcqRel);
        }
    }
}

/// Before the host kernel writes [addr, addr+len) (e.g. read() into a guest buffer): unprotect its pages and mark
/// them written.
pub unsafe fn before_host_write(addr: usize, len: usize) {
    // Pages the GPU backend holds for running GPU work: wait for it.
    crate::gpu::backend().before_host_write(addr, len);
    if ANY.load(Ordering::Relaxed) == 0 {
        return;
    }
    for_each_overlap(addr, len, |g, f, first, last, base| {
        unprotect_pages(g, f, first, last, base, 0)
    });
}

/// GPU work will access [addr, addr+len) through a buffer (it may write, and GPU writes cannot fault): unprotect its
/// pages and never protect them again until released. A texture's memory reused for GPU data otherwise stayed
/// read-only, GPU writes into it were lost and the game stalled on its loading screen.
pub fn release_for_gpu(addr: usize, len: usize) {
    crate::gpu::backend().release_for_gpu(addr, len);
    if ANY.load(Ordering::Relaxed) == 0 {
        return;
    }
    for_each_overlap(addr, len, |g, f, first, last, base| unsafe {
        unprotect_pages(g, f, first, last, base, GPU)
    });
    if log() {
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) % 5000 == 0 {
            println!(
                "[dirty-track] GPU access {:#x}+{:#x}: pages excluded from tracking",
                addr, len
            );
        }
    }
}

/// Guest memory [addr, addr+len) was released: leave its pages writable and forget their flags (they may be reused for
/// anything, including GPU-only data that no longer needs excluding).
pub unsafe fn release(addr: usize, len: usize) {
    if ANY.load(Ordering::Relaxed) == 0 {
        return;
    }
    // Only whole pages: a partial page still holds live neighbours whose flags must stay.
    let first_full = (addr + PAGE - 1) & !(PAGE - 1);
    let end_full = (addr + len) & !(PAGE - 1);
    for_each_overlap(addr, len, |g, f, first, last, base| {
        unprotect_pages(g, f, first, last, base, 0);
        for p in first..=last {
            let a = base + p * PAGE;
            if a >= first_full && a + PAGE <= end_full {
                if f[p].swap(0, Ordering::AcqRel) & EVER != 0 {
                    ANY.fetch_sub(1, Ordering::Relaxed);
                }
            }
        }
    });
}
