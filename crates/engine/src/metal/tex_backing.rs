// Guest memory under textures whose content lives in Metal (--tex-backing).
//
// A texture the game fills from the CPU (file data, WriteTexels) exists twice: the game's block-linear bytes in guest
// memory, and our deswizzled copy in a Metal texture (Apple GPUs cannot sample Tegra layouts). Walking through Hateno
// that is ~900 MB of each (--log-tex-mem). The guest copy is only needed when the CPU touches it again (TotK moves
// textures with CPU copies when it compacts its heaps), so it is dropped and rebuilt from Metal on access.
//
// Held ranges: every 60 frames the whole 16 KiB pages inside copied textures' storage (the union over textures, so
// small textures packed together count) become inaccessible. The first access opens the range again (write tracking,
// guest/dirty_track.rs, is told the pages may have changed) and keeps it out for 600 frames. While a texture lies inside a
// held range its change checks are skipped (`holds`): the CPU cannot have touched it.
//
// Discarded ranges (--tex-backing): a worker thread takes held ranges that stayed untouched for 120 frames, rebuilds
// their bytes from the Metal textures into scratch memory (zeros, then every texture's texels swizzled back with
// nvn::layout::write_texels_2d) and compares them with the guest bytes (read through an alias mapping). Only an exact
// match is discarded (madvise MADV_FREE_REUSABLE: out of the footprint); the range keeps references to its textures. Any
// access to a discarded page faults; the handler has the worker rebuild it (the same bytes, written through an alias)
// and waits, then the access proceeds. Discarded pages stay inaccessible whatever other modules do with protections
// (`reprotect`, called after their mprotects). Host writes (file reads) and GPU buffer use rebuild first.
//
// --tex-backing=log: hold and log accesses only (thread, PC, caller, read / write), no discarding.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLOrigin, MTLRegion, MTLSize, MTLTexture, MTLTextureType};
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const PAGE: usize = 0x4000;
const SLOTS: usize = 16384;
const DISC_SLOTS: usize = 8192;

/// 0 off, 1 discard (--tex-backing), 2 log only.
pub(super) fn mode() -> crate::settings::TexBacking {
    crate::settings::get().graphics.tex_backing
}

/// A texture over guest storage, as handed in by metal::tex_backing_frame.
pub(super) struct TexRec {
    pub(super) storage: usize,
    pub(super) len: usize,
    pub(super) params: crate::nvn::layout::TextureParams,
    pub(super) tex: Retained<ProtocolObject<dyn MTLTexture>>,
}
// Metal textures may be read from any thread (getBytes), and nothing here writes them.
unsafe impl Send for TexRec {}

/// Whether `tex_backing` can rebuild guest bytes from this Metal texture.
pub(super) fn supported(
    tex: &ProtocolObject<dyn MTLTexture>,
    params: &crate::nvn::layout::TextureParams,
) -> bool {
    use objc2_metal::MTLPixelFormat as F;
    let tt = tex.textureType();
    let fmt = tex.pixelFormat();
    // Not linear textures (0x10 / 0x40) yet: their rebuilt rows did not match (pitch).
    params.flags & (0x10 | 0x40) == 0
        && matches!(
            tt,
            MTLTextureType::Type1D
                | MTLTextureType::Type2D
                | MTLTextureType::Type2DArray
                | MTLTextureType::TypeCube
                | MTLTextureType::TypeCubeArray
        )
        && tex.sampleCount() == 1
        && crate::nvn::layout::format_info(params.format).is_some()
        && !matches!(
            fmt,
            F::Depth16Unorm
                | F::Depth32Float
                | F::Depth24Unorm_Stencil8
                | F::Depth32Float_Stencil8
                | F::Stencil8
                | F::X32_Stencil8
                | F::X24_Stencil8
        )
}

// ---- held ranges ---------------------------------------------------------------------------------------------------

/// Sorted, disjoint held ranges (page-aligned start, end); searched lock-free by the fault handler.
struct Table {
    n: AtomicUsize,
    start: [AtomicUsize; SLOTS],
    end: [AtomicUsize; SLOTS],
    open: [AtomicU8; SLOTS],
    /// The range's guest bytes were verified equal to what its Metal textures rebuild (the worker): only then may change
    /// checks skip its textures (a range held while a copy was already stale would otherwise never be refreshed).
    verified: [AtomicU8; SLOTS],
}
#[allow(clippy::declare_interior_mutable_const)]
const TABLE: Table = Table {
    n: AtomicUsize::new(0),
    start: [const { AtomicUsize::new(0) }; SLOTS],
    end: [const { AtomicUsize::new(0) }; SLOTS],
    open: [const { AtomicU8::new(0) }; SLOTS],
    verified: [const { AtomicU8::new(0) }; SLOTS],
};
static TABLES: [Table; 2] = [TABLE, TABLE];
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Ranges touched recently (start -> frame), not held again for a while; under the rebuild lock.
static TOUCHED: Mutex<Option<std::collections::HashMap<usize, u64>>> = Mutex::new(None);
/// Starts of ranges touched since the last rebuild (handler side, lock-free).
const TOUCH_RING: usize = 4096;
static TOUCH_STARTS: [AtomicUsize; TOUCH_RING] = [const { AtomicUsize::new(0) }; TOUCH_RING];
static TOUCH_N: AtomicUsize = AtomicUsize::new(0);

/// Held ranges for the worker: start -> (end, frame first held, inner discard candidates (whole pages inside a union,
/// with its textures)).
static HELD_TEX: Mutex<
    Option<std::collections::HashMap<usize, (usize, u64, Vec<(usize, usize, Vec<TexRec>)>)>>,
> = Mutex::new(None);
/// Ranges whose verification failed (start -> frame): not tried again for a while.
static VERIFY_FAILED: Mutex<Option<std::collections::HashMap<usize, u64>>> = Mutex::new(None);

fn find(t: &Table, addr: usize) -> Option<usize> {
    let n = t.n.load(Ordering::Acquire).min(SLOTS);
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if t.end[mid].load(Ordering::Acquire) <= addr {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    (lo < n && t.start[lo].load(Ordering::Acquire) <= addr).then_some(lo)
}

/// Makes [s, e) accessible (except discarded pages there) and tells write tracking its pages may have changed.
unsafe fn open_pages(s: usize, e: usize) {
    libc::mprotect(
        s as *mut libc::c_void,
        e - s,
        libc::PROT_READ | libc::PROT_WRITE,
    );
    let mut p = s;
    while p < e {
        // A no-op for pages it does not track; else it bumps their write generation (a re-check, never a missed write).
        crate::guest::dirty_track::handle_fault(p);
        if super::fence_guard::holds(p) {
            libc::mprotect(p as *mut libc::c_void, PAGE, libc::PROT_READ);
        }
        p += PAGE;
    }
    super::report_guard::reprotect_held(s, e - s);
    disc_reprotect(s, e - s);
}

/// Opens range `i` of table `t`. Always does the mprotect, also for a range already marked open: a fault there means
/// the pages are inaccessible anyway (answering "retry" without opening looped forever in memmove, 2026-10-15).
unsafe fn open_range(t: &Table, i: usize) {
    let first = t.open[i].swap(1, Ordering::AcqRel) == 0;
    let (s, e) = (
        t.start[i].load(Ordering::Acquire),
        t.end[i].load(Ordering::Acquire),
    );
    open_pages(s, e);
    if !first {
        return;
    }
    let k = TOUCH_N.fetch_add(1, Ordering::AcqRel);
    if k < TOUCH_RING {
        TOUCH_STARTS[k].store(s, Ordering::Release);
    }
}

/// Crash reports: the held / discarded ranges over `addr`.
pub(super) fn describe(addr: usize) -> String {
    if mode() == crate::settings::TexBacking::Off {
        return "off".into();
    }
    let gen = ACTIVE.load(Ordering::Acquire);
    let mut out = Vec::new();
    for (k, g) in [gen, gen.wrapping_sub(1)].into_iter().enumerate() {
        let t = &TABLES[g & 1];
        if let Some(i) = find(t, addr) {
            out.push(format!(
                "{} table {:#x}..{:#x} open {} verified {}",
                if k == 0 { "active" } else { "previous" },
                t.start[i].load(Ordering::Acquire),
                t.end[i].load(Ordering::Acquire),
                t.open[i].load(Ordering::Acquire),
                t.verified[i].load(Ordering::Acquire)
            ));
        }
    }
    if let Some(i) = disc_find(addr) {
        out.push(format!(
            "discarded {:#x}..{:#x} state {}",
            DISC_START[i].load(Ordering::Acquire),
            DISC_END[i].load(Ordering::Acquire),
            DISC_STATE[i].load(Ordering::Acquire)
        ));
    }
    if out.is_empty() {
        "not held".into()
    } else {
        out.join(", ")
    }
}

/// Whether [addr, addr+len) lies inside a range held (or discarded) now: the CPU cannot have touched it since (any
/// access faults and opens the range, which bumps write tracking's generations), so change checks can skip it.
pub(super) fn holds(addr: usize, len: usize) -> bool {
    if mode() == crate::settings::TexBacking::Off || len == 0 {
        return false;
    }
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    if find(t, addr).is_some_and(|i| {
        t.open[i].load(Ordering::Acquire) == 0
            && t.verified[i].load(Ordering::Acquire) != 0
            && addr + len <= t.end[i].load(Ordering::Acquire)
    }) {
        return true;
    }
    // Inside a discarded range (its held slot may have gone with a table rebuild).
    disc_find(addr).is_some_and(|i| addr + len <= DISC_END[i].load(Ordering::Acquire))
}

/// Opens every range overlapping [addr, addr+len), rebuilding discarded ones first: the host kernel is about to write
/// there (it cannot fault: file reads into protected memory failed with EFAULT and loading stalled), or GPU buffers
/// access it. Not for the fault handler of the range itself (it waits for the worker).
pub(super) unsafe fn open_overlapping(addr: usize, len: usize) {
    if mode() == crate::settings::TexBacking::Off || len == 0 {
        return;
    }
    disc_rebuild_overlapping(addr, len);
    let gen = ACTIVE.load(Ordering::Acquire);
    for g in [gen, gen.wrapping_sub(1)] {
        let t = &TABLES[g & 1];
        let n = t.n.load(Ordering::Acquire).min(SLOTS);
        // First range ending after addr (binary search: this runs for every GPU buffer bind).
        let (mut lo, mut hi) = (0, n);
        while lo < hi {
            let mid = (lo + hi) / 2;
            if t.end[mid].load(Ordering::Acquire) <= addr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let mut i = lo;
        while i < n && t.start[i].load(Ordering::Acquire) < addr + len {
            open_range(t, i);
            i += 1;
        }
    }
}

/// Every 60 frames: the textures whose content is a copy of guest data.
pub(super) fn rebuild(frame: u64, texs: Vec<TexRec>) {
    if mode() == crate::settings::TexBacking::Off {
        return;
    }
    start_worker();
    let mut touched = TOUCHED.lock().unwrap();
    let touched = touched.get_or_insert_with(Default::default);
    let n = TOUCH_N.swap(0, Ordering::AcqRel).min(TOUCH_RING);
    for k in 0..n {
        touched.insert(TOUCH_STARTS[k].load(Ordering::Acquire), frame);
    }
    touched.retain(|_, f| *f + 600 > frame);
    // Union of the textures' bytes first (small textures packed side by side cover whole pages together), then whole
    // pages only: a page shared with other data stays as it is.
    let mut texs: Vec<TexRec> = texs
        .into_iter()
        .filter(|r| r.storage >= 0x10000 && r.len > 0)
        .collect();
    texs.sort_unstable_by_key(|r| r.storage);
    let mut groups: Vec<(usize, usize, Vec<TexRec>)> = Vec::new();
    for r in texs {
        let (s, e) = (r.storage, r.storage + r.len);
        match groups.last_mut() {
            Some(g) if s <= g.1 => {
                g.1 = g.1.max(e);
                g.2.push(r);
            }
            _ => groups.push((s, e, vec![r])),
        }
    }
    // Held ranges are the unions rounded out to whole pages, so every texture of a union lies inside its held range
    // and its change checks are skipped (`holds`); with ranges rounded in, textures sticking out over an end were hashed
    // as a whole by refresh_cpu_written, which read the discarded pages and rebuilt them again and again (2026-10-15:
    // ~2300 rebuilds in 1400 frames). Only the pages wholly inside a union are discarded (`inner`); the edge pages may
    // hold other data, whose use just opens the held range. Unions sharing an edge page make one held range.
    let mut held_groups: Vec<(usize, usize, Vec<(usize, usize, Vec<TexRec>)>)> = Vec::new();
    for (a, b, t) in groups {
        let (hs, he) = (a & !(PAGE - 1), (b + PAGE - 1) & !(PAGE - 1));
        let (s, e) = ((a + PAGE - 1) & !(PAGE - 1), b & !(PAGE - 1));
        let inner = (e > s).then_some((s, e, t));
        match held_groups.last_mut() {
            Some(m) if hs < m.1 => {
                m.1 = m.1.max(he);
                m.2.extend(inner);
            }
            _ => held_groups.push((hs, he, inner.into_iter().collect())),
        }
    }
    held_groups.retain(|r| !touched.contains_key(&r.0));
    held_groups.truncate(SLOTS);
    let merged: Vec<(usize, usize)> = held_groups.iter().map(|g| (g.0, g.1)).collect();
    // Old table: open what is still closed (their textures may be gone), then publish and protect the new one.
    let gen = ACTIVE.load(Ordering::Acquire);
    let old = &TABLES[gen & 1];
    for i in 0..old.n.load(Ordering::Acquire) {
        // 2: closed until this rebuild (its verification carries over to the same range below); 1: opened by an access.
        if old.open[i]
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let (s, e) = (
                old.start[i].load(Ordering::Acquire),
                old.end[i].load(Ordering::Acquire),
            );
            unsafe { open_pages(s, e) };
        }
    }
    let t = &TABLES[(gen + 1) & 1];
    t.n.store(0, Ordering::Release);
    let mut bytes = 0;
    for (i, (s, e)) in merged.iter().enumerate() {
        t.start[i].store(*s, Ordering::Relaxed);
        t.end[i].store(*e, Ordering::Relaxed);
        t.open[i].store(0, Ordering::Relaxed);
        // Still verified if the same range stayed held (closed) all along.
        let kept = find(old, *s).is_some_and(|j| {
            old.start[j].load(Ordering::Acquire) == *s
                && old.end[j].load(Ordering::Acquire) == *e
                && old.verified[j].load(Ordering::Acquire) != 0
                && old.open[j].load(Ordering::Acquire) == 2
        });
        t.verified[i].store(kept as u8, Ordering::Relaxed);
        bytes += e - s;
    }
    t.n.store(merged.len(), Ordering::Release);
    ACTIVE.store(gen + 1, Ordering::Release);
    // Published before protecting (a fault must find its range); a range opened by a fault in between must not be
    // closed again: check after the mprotect, reopen if it was (an open right after the check does its own mprotect).
    for (i, (s, e)) in merged.iter().enumerate() {
        unsafe { libc::mprotect(*s as *mut libc::c_void, e - s, libc::PROT_NONE) };
        if t.open[i].load(Ordering::Acquire) != 0 {
            unsafe { open_pages(*s, *e) };
        }
    }
    // The worker's view: discard candidates (inner pages of held unions) with their textures and the frame each was
    // first held at.
    {
        let mut held = HELD_TEX.lock().unwrap();
        let held = held.get_or_insert_with(Default::default);
        let mut next = std::collections::HashMap::with_capacity(held_groups.len());
        for (s, e, inner) in held_groups {
            if inner.is_empty() {
                continue;
            }
            let since = held.get(&s).filter(|h| h.0 == e).map_or(frame, |h| h.1);
            next.insert(s, (e, since, inner));
        }
        *held = next;
    }
    if frame % 600 == 0 {
        println!("[tex-backing] f{}: {} ranges, {} MB held; {} recently touched ranges left out; discarded {} MB ({} ranges); \
            verified {} MB ok / {} failed, rebuilt {} ({} MB, {:.1} ms), waits {} ({:.1} ms), lost {}",
            frame, t.n.load(Ordering::Relaxed), bytes >> 20, touched.len(), DISC_BYTES.load(Ordering::Relaxed) >> 20,
            DISC_N.load(Ordering::Relaxed), VERIFIED_BYTES.swap(0, Ordering::Relaxed) >> 20, VERIFY_FAILS.swap(0, Ordering::Relaxed),
            REBUILDS.swap(0, Ordering::Relaxed), REBUILT_BYTES.swap(0, Ordering::Relaxed) >> 20, REBUILD_US.swap(0, Ordering::Relaxed) as f64 / 1000.0,
            WAITS.swap(0, Ordering::Relaxed), WAIT_US.swap(0, Ordering::Relaxed) as f64 / 1000.0, LOST.load(Ordering::Relaxed));
    }
}

// ---- discarded ranges ----------------------------------------------------------------------------------------------

/// Discarded ranges: fixed slots (never moved), searched lock-free. State: 0 free, 1 discarded, 2 rebuild requested.
static DISC_START: [AtomicUsize; DISC_SLOTS] = [const { AtomicUsize::new(0) }; DISC_SLOTS];
static DISC_END: [AtomicUsize; DISC_SLOTS] = [const { AtomicUsize::new(0) }; DISC_SLOTS];
static DISC_STATE: [AtomicU8; DISC_SLOTS] = [const { AtomicU8::new(0) }; DISC_SLOTS];
static DISC_HIGH: AtomicUsize = AtomicUsize::new(0);
/// Textures of the discarded ranges (start -> textures).
static DISC_TEX: Mutex<Option<std::collections::HashMap<usize, Vec<TexRec>>>> = Mutex::new(None);
static DISC_BYTES: AtomicUsize = AtomicUsize::new(0);
static DISC_N: AtomicUsize = AtomicUsize::new(0);

fn disc_find(addr: usize) -> Option<usize> {
    let n = DISC_HIGH.load(Ordering::Acquire).min(DISC_SLOTS);
    (0..n).find(|&i| {
        let s = DISC_START[i].load(Ordering::Acquire);
        s != 0
            && addr >= s
            && addr < DISC_END[i].load(Ordering::Acquire)
            && DISC_STATE[i].load(Ordering::Acquire) != 0
    })
}

/// After another module changed the protection of [addr, addr+len): discarded pages stay inaccessible (signal-safe).
pub(super) unsafe fn disc_reprotect(addr: usize, len: usize) {
    let n = DISC_HIGH.load(Ordering::Acquire).min(DISC_SLOTS);
    for i in 0..n {
        let (s, e) = (
            DISC_START[i].load(Ordering::Acquire),
            DISC_END[i].load(Ordering::Acquire),
        );
        if s != 0 && s < addr + len && e > addr && DISC_STATE[i].load(Ordering::Acquire) != 0 {
            let (a, b) = (
                s.max(addr & !(PAGE - 1)),
                e.min((addr + len + PAGE - 1) & !(PAGE - 1)),
            );
            libc::mprotect(a as *mut libc::c_void, b - a, libc::PROT_NONE);
        }
    }
}

/// Asks for slot `i`'s rebuild and waits for it (signal-safe: atomics and nanosleep).
unsafe fn disc_wait(i: usize, s: usize) {
    let _ = DISC_STATE[i].compare_exchange(1, 2, Ordering::AcqRel, Ordering::Acquire);
    let t0 = std::time::Instant::now();
    WAITS.fetch_add(1, Ordering::Relaxed);
    while DISC_START[i].load(Ordering::Acquire) == s && DISC_STATE[i].load(Ordering::Acquire) != 0 {
        if t0.elapsed().as_secs() >= 10 {
            // The worker cannot rebuild it: the bytes are lost. Open it rather than hang the game (logged).
            LOST.fetch_add(1, Ordering::Relaxed);
            DISC_STATE[i].store(0, Ordering::Release);
            libc::mprotect(
                s as *mut libc::c_void,
                DISC_END[i].load(Ordering::Acquire) - s,
                libc::PROT_READ | libc::PROT_WRITE,
            );
            break;
        }
        let ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 20_000,
        };
        libc::nanosleep(&ts, std::ptr::null_mut());
    }
    WAIT_US.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
}

/// Rebuilds the discarded ranges overlapping [addr, addr+len) and waits for them.
unsafe fn disc_rebuild_overlapping(addr: usize, len: usize) {
    let n = DISC_HIGH.load(Ordering::Acquire).min(DISC_SLOTS);
    for i in 0..n {
        let (s, e) = (
            DISC_START[i].load(Ordering::Acquire),
            DISC_END[i].load(Ordering::Acquire),
        );
        if s != 0 && s < addr + len && e > addr && DISC_STATE[i].load(Ordering::Acquire) != 0 {
            disc_wait(i, s);
        }
    }
}

// ---- worker: verify + discard, rebuild ----------------------------------------------------------------------------

static VERIFIED_BYTES: AtomicUsize = AtomicUsize::new(0);
static VERIFY_FAILS: AtomicUsize = AtomicUsize::new(0);
static REBUILDS: AtomicUsize = AtomicUsize::new(0);
static REBUILT_BYTES: AtomicUsize = AtomicUsize::new(0);
static REBUILD_US: AtomicU64 = AtomicU64::new(0);
static WAITS: AtomicU64 = AtomicU64::new(0);
static WAIT_US: AtomicU64 = AtomicU64::new(0);
static LOST: AtomicUsize = AtomicUsize::new(0);
static FRAME: AtomicU64 = AtomicU64::new(0);

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
    fn mach_vm_protect(
        task: u32,
        address: u64,
        size: u64,
        set_maximum: i32,
        new_protection: i32,
    ) -> i32;
}

/// A shared, read-write second mapping of [s, e) (the same memory, its own protection).
struct Alias(usize, usize);
impl Alias {
    unsafe fn new(s: usize, e: usize) -> Option<Alias> {
        let (mut target, mut cur, mut max) = (0u64, 0i32, 0i32);
        // VM_FLAGS_ANYWHERE, copy = false (shared), VM_INHERIT_NONE.
        if mach_vm_remap(
            mach_task_self_,
            &mut target,
            (e - s) as u64,
            0,
            1,
            mach_task_self_,
            s as u64,
            0,
            &mut cur,
            &mut max,
            2,
        ) != 0
        {
            return None;
        }
        // The source is inaccessible: give the alias read-write access (VM_PROT_READ | VM_PROT_WRITE).
        if mach_vm_protect(mach_task_self_, target, (e - s) as u64, 0, 3) != 0 {
            mach_vm_deallocate(mach_task_self_, target, (e - s) as u64);
            return None;
        }
        Some(Alias(target as usize, e - s))
    }
}
impl Drop for Alias {
    fn drop(&mut self) {
        unsafe { mach_vm_deallocate(mach_task_self_, self.0 as u64, self.1 as u64) };
    }
}

/// The bytes of [s, e) rebuilt from the textures: zeros, then every texture's texels in guest layout. None if a texture
/// could not be read back.
unsafe fn rebuild_bytes(s: usize, e: usize, texs: &[TexRec]) -> Option<Vec<u8>> {
    // Scratch over the textures' whole extent (they may start before / end after the range's whole pages).
    let base = texs.iter().map(|r| r.storage).min()?.min(s);
    let top = texs.iter().map(|r| r.storage + r.len).max()?.max(e);
    let mut scratch = vec![0u8; top - base];
    let mut buf: Vec<u8> = Vec::new();
    for r in texs {
        let p = &r.params;
        let info = crate::nvn::layout::format_info(p.format)?;
        let layout = crate::nvn::layout::compute_layout(p);
        let is_linear = (p.flags & 0x10 != 0) || (p.flags & 0x40 != 0);
        let levels = if is_linear {
            1
        } else {
            r.tex.mipmapLevelCount()
        };
        let layers = layout.layers.max(1) as usize;
        let tt = r.tex.textureType();
        let dst = scratch.as_mut_ptr().add(r.storage - base);
        for layer in 0..layers {
            let slice_off = layer * layout.layer_stride as usize;
            let slice = if matches!(tt, MTLTextureType::Type1D | MTLTextureType::Type2D) {
                0
            } else {
                layer
            };
            if slice
                >= r.tex.arrayLength()
                    * if matches!(tt, MTLTextureType::TypeCube | MTLTextureType::TypeCubeArray) {
                        6
                    } else {
                        1
                    }
            {
                break;
            }
            for level in 0..levels {
                let (lw, lh) = (
                    ((p.width as usize) >> level).max(1),
                    ((p.height as usize) >> level).max(1),
                );
                let wb = (lw + info.block_w as usize - 1) / info.block_w as usize;
                let hb = (lh + info.block_h as usize - 1) / info.block_h as usize;
                let bpr = wb * info.bytes as usize;
                let region = MTLRegion {
                    origin: MTLOrigin { x: 0, y: 0, z: 0 },
                    size: MTLSize {
                        width: lw,
                        height: lh,
                        depth: 1,
                    },
                };
                if is_linear {
                    let pitch = if p.stride > 0 { p.stride as usize } else { bpr };
                    if slice_off + pitch * hb > r.len {
                        return None;
                    }
                    r.tex
                        .getBytes_bytesPerRow_bytesPerImage_fromRegion_mipmapLevel_slice(
                            std::ptr::NonNull::new(dst.add(slice_off) as *mut libc::c_void)?,
                            pitch,
                            0,
                            region,
                            level,
                            slice,
                        );
                } else {
                    buf.resize(bpr * hb, 0);
                    r.tex
                        .getBytes_bytesPerRow_bytesPerImage_fromRegion_mipmapLevel_slice(
                            std::ptr::NonNull::new(buf.as_mut_ptr() as *mut libc::c_void)?,
                            bpr,
                            0,
                            region,
                            level,
                            slice,
                        );
                    if !crate::nvn::layout::write_texels_2d(
                        dst,
                        slice_off,
                        p,
                        level,
                        0,
                        0,
                        lw as i32,
                        lh as i32,
                        buf.as_ptr(),
                    ) {
                        return None;
                    }
                }
            }
        }
    }
    Some(scratch[s - base..e - base].to_vec())
}

/// Whether inner range [s, e) rebuilds to exactly its guest bytes.
unsafe fn verify(s: usize, e: usize, texs: &[TexRec]) -> bool {
    let Some(bytes) = rebuild_bytes(s, e, texs) else {
        return false;
    };
    let Some(alias) = Alias::new(s, e) else {
        return false;
    };
    let guest = std::slice::from_raw_parts(alias.0 as *const u8, e - s);
    // Exact comparison only. (Accepting image textures by an unchanged upload hash and rebuilding them from Metal crashed
    // tera::ImageResourceMgr during loading, 2026-10-15: an unchanged guest hash does not prove the Metal copy holds every
    // byte, e.g. levels or layers it lacks come back as zeros.)
    let mismatch = guest != &bytes[..];
    if mismatch {
        // --tex-backing-log: where the first difference is (which texture, how many bytes differ).
        static LOG: OnceLock<bool> = OnceLock::new();
        static N: AtomicUsize = AtomicUsize::new(0);
        if *LOG.get_or_init(|| crate::settings::get().log.tex_backing_log)
            && N.fetch_add(1, Ordering::Relaxed) < 40
        {
            let first = guest
                .iter()
                .zip(bytes.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            let diff = guest
                .iter()
                .zip(bytes.iter())
                .filter(|(a, b)| a != b)
                .count();
            let at = s + first;
            let owner: Vec<String> = texs
                .iter()
                .filter(|r| at >= r.storage && at < r.storage + r.len)
                .map(|r| {
                    format!(
                        "{}x{} fmt {} lv {} flags {:#x} +{:#x}",
                        r.params.width,
                        r.params.height,
                        r.params.format,
                        r.params.levels,
                        r.params.flags,
                        at - r.storage
                    )
                })
                .collect();
            let zero_ours = bytes[first] == 0 && guest[first] != 0;
            println!("[tex-backing] verify {:#x}..{:#x} ({} textures): {} of {} bytes differ, first at +{:#x} (guest {:#04x} rebuilt {:#04x}{}) in {:?}",
                s, e, texs.len(), diff, e - s, first, guest[first], bytes[first], if zero_ours { ", rebuilt zero: padding or unread" } else { "" }, owner);
            // Per level: Metal's texels vs the guest bytes deswizzled the way the upload does (read_texels_2d). Equal: the
            // rebuild's layout is wrong; different: the Metal copy is stale.
            for r in texs
                .iter()
                .filter(|r| r.storage >= s && r.storage + r.len <= e)
                .take(2)
            {
                let Some(info) = crate::nvn::layout::format_info(r.params.format) else {
                    continue;
                };
                let mut lv = Vec::new();
                for level in 0..r.tex.mipmapLevelCount().min(4) {
                    let (lw, lh) = (
                        ((r.params.width as usize) >> level).max(1),
                        ((r.params.height as usize) >> level).max(1),
                    );
                    let wb = (lw + info.block_w as usize - 1) / info.block_w as usize;
                    let hb = (lh + info.block_h as usize - 1) / info.block_h as usize;
                    let bpr = wb * info.bytes as usize;
                    let (mut a, mut b) = (vec![0u8; bpr * hb], vec![0u8; bpr * hb]);
                    let region = MTLRegion {
                        origin: MTLOrigin { x: 0, y: 0, z: 0 },
                        size: MTLSize {
                            width: lw,
                            height: lh,
                            depth: 1,
                        },
                    };
                    r.tex
                        .getBytes_bytesPerRow_bytesPerImage_fromRegion_mipmapLevel_slice(
                            std::ptr::NonNull::new(a.as_mut_ptr() as *mut libc::c_void).unwrap(),
                            bpr,
                            0,
                            region,
                            level,
                            0,
                        );
                    let ok = crate::nvn::layout::read_texels_2d(
                        (alias.0 + (r.storage - s)) as *const u8,
                        0,
                        &r.params,
                        level,
                        0,
                        0,
                        lw as i32,
                        lh as i32,
                        b.as_mut_ptr(),
                    );
                    let same = a.iter().zip(&b).filter(|(x, y)| x == y).count();
                    lv.push(format!(
                        "l{} {}% same{}",
                        level,
                        same * 100 / a.len().max(1),
                        if ok { "" } else { " (read failed)" }
                    ));
                }
                println!("[tex-backing]   {}x{} fmt {} levels guest {} metal {}: Metal vs deswizzled guest: {}", r.params.width, r.params.height, r.params.format,
                    r.params.levels, r.tex.mipmapLevelCount(), lv.join(", "));
            }
        }
        return false;
    }
    true
}

/// Tries to verify held range [hs, he) (all its inner ranges) and discard its inner ranges.
unsafe fn try_discard_group(hs: usize, he: usize, inner: Vec<(usize, usize, Vec<TexRec>)>) {
    let frame = FRAME.load(Ordering::Relaxed);
    for (s, e, texs) in &inner {
        if !verify(*s, *e, texs) {
            VERIFY_FAILS.fetch_add(1, Ordering::Relaxed);
            VERIFY_FAILED
                .lock()
                .unwrap()
                .get_or_insert_with(Default::default)
                .insert(hs, frame);
            return;
        }
        VERIFIED_BYTES.fetch_add(e - s, Ordering::Relaxed);
    }
    // Still held as verified (closed all along)? Then mark it so change checks may skip its textures.
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    let Some(i) = find(t, hs).filter(|&i| {
        t.start[i].load(Ordering::Acquire) == hs
            && t.end[i].load(Ordering::Acquire) == he
            && t.open[i].load(Ordering::Acquire) == 0
    }) else {
        return;
    };
    t.verified[i].store(1, Ordering::Release);
    if t.open[i].load(Ordering::Acquire) != 0 {
        t.verified[i].store(0, Ordering::Release);
        return;
    }
    for (s, e, texs) in inner {
        discard(s, e, texs);
    }
}

/// Discards verified inner range [s, e).
unsafe fn discard(s: usize, e: usize, texs: Vec<TexRec>) {
    // Still held (a fault in between opened it and the compare is stale)? Then publish as discarded, then drop the pages.
    let t = &TABLES[ACTIVE.load(Ordering::Acquire) & 1];
    if !find(t, s).is_some_and(|i| {
        t.open[i].load(Ordering::Acquire) == 0
            && t.start[i].load(Ordering::Acquire) <= s
            && e <= t.end[i].load(Ordering::Acquire)
    }) {
        return;
    }
    let Some(slot) = (0..DISC_SLOTS).find(|&i| DISC_START[i].load(Ordering::Acquire) == 0) else {
        return;
    };
    DISC_TEX
        .lock()
        .unwrap()
        .get_or_insert_with(Default::default)
        .insert(s, texs);
    DISC_END[slot].store(e, Ordering::Release);
    DISC_STATE[slot].store(1, Ordering::Release);
    DISC_START[slot].store(s, Ordering::Release);
    if slot >= DISC_HIGH.load(Ordering::Acquire) {
        DISC_HIGH.store(slot + 1, Ordering::Release);
    }
    // Protected again in case the range was opened between the check above and the publish (disc_reprotect is what
    // every opening path ends with, so from here on the pages stay inaccessible).
    libc::mprotect(s as *mut libc::c_void, e - s, libc::PROT_NONE);
    let held_again = find(t, s).is_some_and(|i| t.open[i].load(Ordering::Acquire) == 0);
    if !held_again {
        // Opened meanwhile: the guest bytes may change from now on; take the slot back without discarding.
        disc_free(slot, s, e, false);
        return;
    }
    libc::madvise(s as *mut libc::c_void, e - s, libc::MADV_FREE_REUSABLE);
    DISC_BYTES.fetch_add(e - s, Ordering::Relaxed);
    DISC_N.fetch_add(1, Ordering::Relaxed);
}

/// Releases discard slot `slot` ([s, e)): `rebuilt` = the pages were discarded and have been rewritten.
unsafe fn disc_free(slot: usize, s: usize, e: usize, rebuilt: bool) {
    DISC_TEX.lock().unwrap().as_mut().map(|m| m.remove(&s));
    DISC_STATE[slot].store(0, Ordering::Release);
    DISC_START[slot].store(0, Ordering::Release);
    if rebuilt {
        DISC_BYTES.fetch_sub(e - s, Ordering::Relaxed);
        DISC_N.fetch_sub(1, Ordering::Relaxed);
    }
    // The held table's slots over it: opened (the bytes may change from now on).
    let gen = ACTIVE.load(Ordering::Acquire);
    for g in [gen, gen.wrapping_sub(1)] {
        let t = &TABLES[g & 1];
        if let Some(i) = find(t, s) {
            t.open[i].store(1, Ordering::Release);
        }
    }
    open_pages(s, e);
}

/// Rebuilds discarded slot `slot` into guest memory and opens it.
unsafe fn rebuild_slot(slot: usize) {
    let (s, e) = (
        DISC_START[slot].load(Ordering::Acquire),
        DISC_END[slot].load(Ordering::Acquire),
    );
    if s == 0 {
        return;
    }
    let t0 = std::time::Instant::now();
    let texs = DISC_TEX.lock().unwrap().as_mut().and_then(|m| m.remove(&s));
    let bytes = texs.as_ref().and_then(|t| rebuild_bytes(s, e, t));
    libc::madvise(s as *mut libc::c_void, e - s, libc::MADV_FREE_REUSE);
    match (bytes, Alias::new(s, e)) {
        (Some(b), Some(alias)) => {
            std::ptr::copy_nonoverlapping(b.as_ptr(), alias.0 as *mut u8, e - s)
        }
        _ => {
            LOST.fetch_add(1, Ordering::Relaxed);
            println!(
                "[!!] tex-backing: could not rebuild {:#x}..{:#x}: its guest bytes are lost",
                s, e
            );
        }
    }
    disc_free(slot, s, e, true);
    REBUILDS.fetch_add(1, Ordering::Relaxed);
    REBUILT_BYTES.fetch_add(e - s, Ordering::Relaxed);
    REBUILD_US.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
}

thread_local! {
    static IS_WORKER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn start_worker() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    if mode() != crate::settings::TexBacking::On {
        return;
    }
    STARTED.call_once(|| {
        std::thread::Builder::new()
            .name("tex-backing".into())
            .spawn(|| loop {
                IS_WORKER.with(|w| w.set(true));
                // Rebuild requests first (a thread is waiting for each).
                let n = DISC_HIGH.load(Ordering::Acquire).min(DISC_SLOTS);
                let mut served = false;
                for i in 0..n {
                    if DISC_STATE[i].load(Ordering::Acquire) == 2 {
                        objc2::rc::autoreleasepool(|_| unsafe { rebuild_slot(i) });
                        served = true;
                    }
                }
                if served {
                    continue;
                }
                // One held range untouched for 120 frames, not yet discarded or recently failed.
                let frame = FRAME.load(Ordering::Acquire);
                let pick = {
                    let mut held = HELD_TEX.lock().unwrap();
                    let failed = VERIFY_FAILED.lock().unwrap();
                    let disc = DISC_TEX.lock().unwrap();
                    held.as_mut().and_then(|h| {
                        let key = h
                            .iter()
                            .find(|(s, (_, since, inner))| {
                                since + 120 <= frame
                                    && !failed.as_ref().is_some_and(|f| {
                                        f.get(s).is_some_and(|ff| ff + 1800 > frame)
                                    })
                                    && !inner.iter().any(|(is, _, _)| {
                                        disc.as_ref().is_some_and(|d| d.contains_key(is))
                                    })
                            })
                            .map(|(s, _)| *s)?;
                        h.remove(&key).map(|(e, _, inner)| (key, e, inner))
                    })
                };
                match pick {
                    Some((s, e, inner)) => {
                        objc2::rc::autoreleasepool(|_| unsafe { try_discard_group(s, e, inner) })
                    }
                    None => std::thread::sleep(std::time::Duration::from_millis(2)),
                }
            })
            .expect("spawn tex-backing worker");
    });
}

// ---- fault handling and logging ------------------------------------------------------------------------------------

const HITS: usize = 512;
static HIT_N: AtomicUsize = AtomicUsize::new(0);
static HIT: [[AtomicUsize; 5]; HITS] = [const { [const { AtomicUsize::new(0) }; 5] }; HITS];
static HIT_NAME: [[AtomicU64; 4]; HITS] = [const { [const { AtomicU64::new(0) }; 4] }; HITS];

unsafe fn record_hit(pc: usize, lr: usize, addr: usize, len: usize, write: bool, rebuild: bool) {
    let k = HIT_N.fetch_add(1, Ordering::AcqRel);
    if k >= HITS {
        return;
    }
    let mut name = [0u8; 32];
    libc::pthread_getname_np(
        libc::pthread_self(),
        name.as_mut_ptr() as *mut libc::c_char,
        name.len(),
    );
    for j in 0..4 {
        HIT_NAME[k][j].store(
            u64::from_le_bytes(name[j * 8..j * 8 + 8].try_into().unwrap()),
            Ordering::Relaxed,
        );
    }
    let guest = crate::cpu::tls::get_current_guest_tls_block() >= 0x10000;
    for (j, v) in [
        pc,
        lr,
        addr,
        len,
        write as usize | (guest as usize) << 1 | (rebuild as usize) << 2,
    ]
    .into_iter()
    .enumerate()
    {
        HIT[k][j].store(v, Ordering::Release);
    }
}

/// Signal-handler hook: true if `addr` is in a range held or discarded here (the access is then retried).
pub(super) unsafe fn fault(addr: usize, pc: usize, lr: usize, write: bool) -> bool {
    if mode() == crate::settings::TexBacking::Off {
        return false;
    }
    if let Some(i) = disc_find(addr) {
        let s = DISC_START[i].load(Ordering::Acquire);
        if mode() == crate::settings::TexBacking::Log || IS_WORKER.with(|w| w.get()) {
            return false; // never: discarding is off in log mode, and the worker only uses aliases
        }
        record_hit(
            pc,
            lr,
            addr,
            DISC_END[i].load(Ordering::Acquire) - s,
            write,
            true,
        );
        disc_wait(i, s);
        return true;
    }
    let gen = ACTIVE.load(Ordering::Acquire);
    for g in [gen, gen.wrapping_sub(1)] {
        let t = &TABLES[g & 1];
        if let Some(i) = find(t, addr) {
            let first = t.open[i].load(Ordering::Acquire) == 0;
            // Already open, and a write to a page the early-fence write guard holds read-only: that fault is the write
            // guard's (it waits for the GPU, then allows the write). Claiming it reopened the page read-only forever
            // (a fault loop at the title screen, 2026-10-15).
            if !first && write && super::fence_guard::holds(addr & !(PAGE - 1)) {
                return false;
            }
            if first {
                record_hit(
                    pc,
                    lr,
                    addr,
                    t.end[i].load(Ordering::Acquire) - t.start[i].load(Ordering::Acquire),
                    write,
                    false,
                );
            }
            open_range(t, i);
            return true;
        }
    }
    false
}

/// Once per presented frame: prints the logged accesses (log mode) and advances the worker's clock.
pub(super) fn frame_end(frame: u64) {
    if mode() == crate::settings::TexBacking::Off {
        return;
    }
    FRAME.store(frame, Ordering::Release);
    extern "C" {
        fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
    }
    let n = HIT_N.swap(0, Ordering::AcqRel).min(HITS);
    static LOG: OnceLock<bool> = OnceLock::new();
    if mode() != crate::settings::TexBacking::Log
        && !*LOG.get_or_init(|| crate::settings::get().log.tex_backing_log)
    {
        return;
    }
    let base = crate::cpu::crash_handler::get_guest_base();
    let slide = unsafe { _dyld_get_image_vmaddr_slide(0) } as usize;
    for k in 0..n {
        let v: Vec<usize> = (0..5).map(|j| HIT[k][j].load(Ordering::Acquire)).collect();
        let mut name = [0u8; 32];
        for j in 0..4 {
            name[j * 8..j * 8 + 8]
                .copy_from_slice(&HIT_NAME[k][j].load(Ordering::Relaxed).to_le_bytes());
        }
        let name =
            String::from_utf8_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(32)])
                .to_string();
        let at = |a: usize| {
            if a >= base && a < base + (256 << 20) {
                format!("main+{:#x}", a - base)
            } else {
                format!("engine {:#x}", a.wrapping_sub(slide))
            }
        };
        println!(
            "[tex-backing] f{} {} {} {} {:#x} (range {} KB) pc {} lr {} thread {}",
            frame,
            if v[4] & 4 != 0 { "REBUILD" } else { "open   " },
            if v[4] & 1 != 0 { "write" } else { "read " },
            if v[4] & 2 != 0 { "guest-tls" } else { "no-tls" },
            v[2],
            v[3] >> 10,
            at(v[0]),
            at(v[1]),
            name
        );
    }
}
