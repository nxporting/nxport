// Render targets in CPU-visible memory reach guest memory when the CPU touches them (--rt-guard).
//
// On the Switch the GPU renders into memory the CPU shares. Games read some render targets on the CPU (Breath of the
// Wild's terrain and water height maps around the player, rendered every frame) and move others with CPU copies (TotK's
// texture heap compaction), so those bytes must be the GPU's. Here render targets live in Metal textures.
//
// Only pools created with CPU access (CPU_CACHED / CPU_UNCACHED) can be read by the CPU at all. When a pass renders
// into a target in such a pool, the target's subresource pages are made inaccessible ("held") and the hold is tagged
// with the submit's last command buffer. Nothing is copied until the CPU touches a held page: a guest access (guest code,
// or our memcpy / memmove / memset on a game thread) waits while the write-back thread waits for that command buffer,
// writes the texels into guest memory in the texture's layout through a second mapping of the pages, and opens them.
// The access is then retried and reads what the GPU rendered, as on the Switch.
//
// Host code never waits (a game thread inside our HLE code, or an engine thread, may hold a lock the producing submit
// needs): its fault opens that page and turns the hold eager, written back as soon as its command buffer completes.
// Engine reads go through report_guard::engine_ptr / rt_guard::guest_copy, which use the second mapping. Holds not
// touched for AGE frames are written back in the background and end; pool release drops the holds inside it.
// Write-tracked pages keep their tracking: this handler runs first, and opened pages get dirty_track's protection back.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLCommandBuffer, MTLCommandBufferStatus, MTLOrigin, MTLRegion, MTLResource, MTLSize, MTLStorageMode, MTLTexture,
    MTLTextureType,
};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use super::Tex;
use crate::nvn::api::NvnTexture;

const PAGE: usize = 0x4000;
const SLOTS: usize = 512;
/// Serial of a hold whose submit has not finished yet.
const UNASSIGNED: u64 = u64::MAX;

const FREE: u8 = 0;
const HELD: u8 = 1;

pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().graphics.rt_guard)
}

thread_local! {
    /// Inside our memcpy / memmove / memset replacements called by guest code: such faults wait like guest code.
    static GUEST_COPY: Cell<bool> = const { Cell::new(false) };
}

/// Marks the current thread as running a guest-requested memory copy until the guard is dropped.
pub(crate) fn guest_copy_scope() -> impl Drop {
    struct Scope(bool);
    impl Drop for Scope {
        fn drop(&mut self) {
            GUEST_COPY.with(|e| e.set(self.0));
        }
    }
    Scope(GUEST_COPY.with(|e| e.replace(true)))
}

// Slot table. The signal handler reads the atomics only.
static STATE: [AtomicU8; SLOTS] = [const { AtomicU8::new(FREE) }; SLOTS];
/// Page-aligned guest range [START, END) of the held subresource.
static START: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
static END: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
/// Second mapping of the range (engine access and write-back).
static ALIAS: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
static SERIAL: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
/// A guest access waits for this slot's write-back.
static REQ: [AtomicU8; SLOTS] = [const { AtomicU8::new(0) }; SLOTS];
/// Presented frame the slot was last held in.
static FRAME: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
/// Holds not touched for this many frames are written back in the background and end (render-once targets would
/// otherwise keep their slots, and engine lookups scan the table).
const AGE: u64 = 30;
/// Pages whose hold ended recently (a fault racing with the opening is retried; see `fault`).
const RECENT: usize = 64;
static RECENT_PAGES: [AtomicUsize; RECENT] = [const { AtomicUsize::new(0) }; RECENT];
static RECENT_NEXT: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static LAST_RETRY: Cell<usize> = const { Cell::new(0) };
}
/// Guest accesses per slot since the last statistics line (--log-rt-writeback).
static GUEST_READ: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static HIGH: AtomicUsize = AtomicUsize::new(0);
/// Slots held now (engine fast paths skip every lookup while it is 0).
static LIVE: AtomicUsize = AtomicUsize::new(0);
/// Every held range lies in [LO, HI) (widened on hold, never narrowed: a quick reject for engine lookups).
static LO: AtomicUsize = AtomicUsize::new(usize::MAX);
static HI: AtomicUsize = AtomicUsize::new(0);
/// Per slot: an engine access opened some of its pages (the next hold of it protects them again).
static OPENED: [AtomicU8; SLOTS] = [const { AtomicU8::new(0) }; SLOTS];
/// A guest thread waits for a write-back (the write-back thread polls fast while set).
static GUEST_WAITING: AtomicUsize = AtomicUsize::new(0);

/// What the write-back needs (never touched by the signal handler).
struct Payload {
    tex: Tex,
    t: NvnTexture,
    level: usize,
    layer: usize,
    /// The submit's last command buffer (its status says when the rendering is done; the executor's completed serial
    /// only advances at the next submit, which the waiting game thread may be holding back).
    cb: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
}
unsafe impl Send for Payload {}

static TABLE: Mutex<Vec<Option<Payload>>> = Mutex::new(Vec::new());
/// Slots held since the last render pass ended, and slots whose pass has ended but whose command buffer is not queued
/// yet: a hold waits for the command buffer holding the end of the pass that rendered it.
static OPEN: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static CLOSED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

static HOLDS: AtomicU64 = AtomicU64::new(0);
/// Holds ended without a write-back because newer data reached their memory.
static DROPS: AtomicU64 = AtomicU64::new(0);
static WRITEBACKS: AtomicU64 = AtomicU64::new(0);
static WAITS: AtomicU64 = AtomicU64::new(0);
static WAIT_US: AtomicU64 = AtomicU64::new(0);
static HOST_RELEASES: AtomicU64 = AtomicU64::new(0);
static TIMEOUTS: AtomicU64 = AtomicU64::new(0);
static OVERFLOW: AtomicU64 = AtomicU64::new(0);
const RELEASE_LOG: usize = 8;
static RELEASE_PC: [AtomicUsize; RELEASE_LOG] = [const { AtomicUsize::new(0) }; RELEASE_LOG];
static RELEASE_LR: [AtomicUsize; RELEASE_LOG] = [const { AtomicUsize::new(0) }; RELEASE_LOG];
static RELEASE_ADDR: [AtomicUsize; RELEASE_LOG] = [const { AtomicUsize::new(0) }; RELEASE_LOG];
static RELEASE_NAME: [[AtomicU64; 2]; RELEASE_LOG] = [const { [const { AtomicU64::new(0) }; 2] }; RELEASE_LOG];

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

/// A shared, writable second mapping of [start, end).
unsafe fn make_alias(start: usize, end: usize) -> usize {
    let (mut target, mut cur, mut max) = (0u64, 0i32, 0i32);
    let kr = mach_vm_remap(
        mach_task_self_,
        &mut target,
        (end - start) as u64,
        0,
        1,
        mach_task_self_,
        start as u64,
        0,
        &mut cur,
        &mut max,
        2,
    );
    if kr != 0 {
        return 0;
    }
    libc::mprotect(target as *mut libc::c_void, end - start, libc::PROT_READ | libc::PROT_WRITE);
    target as usize
}

fn covers(i: usize, page: usize) -> bool {
    STATE[i].load(Ordering::Acquire) == HELD
        && START[i].load(Ordering::Acquire) <= page
        && END[i].load(Ordering::Acquire) > page
}

fn held(page: usize) -> bool {
    if LIVE.load(Ordering::Acquire) == 0 || page >= HI.load(Ordering::Relaxed) || page + PAGE <= LO.load(Ordering::Relaxed) {
        return false;
    }
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    (0..n).any(|i| covers(i, page))
}

/// The protection of a page no hold covers any more.
fn open_prot(page: usize) -> i32 {
    if super::fence_guard::holds(page) || crate::guest::dirty_track::tracking(page) {
        libc::PROT_READ
    } else {
        libc::PROT_READ | libc::PROT_WRITE
    }
}

/// Opens the pages of [start, end) no other hold covers (signal-safe).
unsafe fn open_range(start: usize, end: usize) {
    let mut p = start;
    while p < end {
        if !held(p) {
            libc::mprotect(p as *mut libc::c_void, PAGE, open_prot(p));
            super::report_guard::reprotect_held(p, PAGE);
            super::tex_backing::disc_reprotect(p, PAGE);
        }
        p += PAGE;
    }
}

/// Ends slot `i`'s hold (written back or dropped) and opens its pages (signal-safe: the payload and alias stay until
/// the slot is reused).
unsafe fn finish(i: usize) {
    if STATE[i]
        .compare_exchange(HELD, FREE, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        LIVE.fetch_sub(1, Ordering::AcqRel);
        REQ[i].store(0, Ordering::Release);
        let (start, end) = (START[i].load(Ordering::Acquire), END[i].load(Ordering::Acquire));
        let mut p = start;
        while p < end && p < start + RECENT * PAGE {
            RECENT_PAGES[RECENT_NEXT.fetch_add(1, Ordering::Relaxed) % RECENT].store(p, Ordering::Release);
            p += PAGE;
        }
        open_range(start, end);
    }
}

/// Ends the holds overlapping [start, end) without a write-back, except one covering exactly `keep` (signal-safe).
/// Newer data reached that memory (or is about to): writing the older rendering back later would overwrite it.
unsafe fn supersede(start: usize, end: usize, keep: Option<(usize, usize)>) {
    if LIVE.load(Ordering::Acquire) == 0 || start >= HI.load(Ordering::Relaxed) || end <= LO.load(Ordering::Relaxed) {
        return;
    }
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    for i in 0..n {
        if STATE[i].load(Ordering::Acquire) != HELD {
            continue;
        }
        let (s, e) = (START[i].load(Ordering::Acquire), END[i].load(Ordering::Acquire));
        if s < end && e > start && keep != Some((s, e)) {
            DROPS.fetch_add(1, Ordering::Relaxed);
            finish(i);
        }
    }
}

/// Newer data reaches guest memory [addr, addr+len) another way than through a hold: an engine write (allocation,
/// file read), a GPU copy or storage-buffer write. The holds there end without a write-back.
pub(crate) fn superseded(addr: usize, len: usize) {
    if !enabled() || len == 0 || HIGH.load(Ordering::Acquire) == 0 {
        return;
    }
    unsafe { supersede(addr & !(PAGE - 1), (addr + len + PAGE - 1) & !(PAGE - 1), None) };
}

/// A pass renders into `tex` (colour or depth) at `level` / `layer`: other holds on that memory are out of date, and
/// the target is held itself when write_back can reproduce it.
pub(super) unsafe fn hold(tex: &Tex, t: &NvnTexture, level: usize, layer: usize) {
    if !enabled() || t.storage < 0x10000 {
        return;
    }
    let p = params(t);
    let layout = crate::nvn::layout::compute_layout(&p);
    let (Some(&off), Some(&size)) = (layout.level_offsets.get(level), layout.level_sizes.get(level)) else {
        return;
    };
    let a = t.storage + layer * layout.layer_stride as usize + off as usize;
    let (start, end) = (a & !(PAGE - 1), (a + size as usize + PAGE - 1) & !(PAGE - 1));
    supersede(start, end, Some((start, end)));
    if !crate::nvn::api::memory_pool_cpu_writable(t.storage) {
        return;
    }
    // Only what write_back reproduces exactly: uncompressed single-sample 2D / 2D-array colour data.
    let Some(info) = crate::nvn::layout::format_info(t.format) else {
        return;
    };
    if info.block_w != 1
        || info.block_h != 1
        || t.samples > 1
        || tex.sampleCount() > 1
        || crate::metal::copy::is_depth_stencil(tex.pixelFormat())
        || !matches!(tex.textureType(), MTLTextureType::Type2D | MTLTextureType::Type2DArray)
        || tex.storageMode() == MTLStorageMode::Private
    {
        // Rendered, but not reproducible: memory under it keeps no hold either (done above).
        return;
    }
    if crate::nvn::api::containing_pool(start, end - start).is_none() {
        return;
    }
    start_thread();
    let mut table = TABLE.lock().unwrap();
    if table.len() < SLOTS {
        table.resize_with(SLOTS, || None);
    }
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    // The same subresource held already: a newer rendering of it.
    let i = match (0..n).find(|&i| {
        STATE[i].load(Ordering::Acquire) == HELD
            && START[i].load(Ordering::Acquire) == start
            && END[i].load(Ordering::Acquire) == end
    }) {
        Some(i) => i,
        None => {
            let free = (0..SLOTS).find(|&i| STATE[i].load(Ordering::Acquire) == FREE);
            let free = match free {
                Some(i) => Some(i),
                None => {
                    // Full: write the oldest finished hold back now and reuse its slot.
                    let oldest = (0..n)
                        .filter(|&i| STATE[i].load(Ordering::Acquire) == HELD && SERIAL[i].load(Ordering::Acquire) != UNASSIGNED)
                        .min_by_key(|&i| FRAME[i].load(Ordering::Acquire));
                    match oldest {
                        Some(i) => {
                            drop(table);
                            let done = rendered(i);
                            if done {
                                write_back(i);
                                finish(i);
                            }
                            table = TABLE.lock().unwrap();
                            (done && STATE[i].load(Ordering::Acquire) == FREE).then_some(i)
                        }
                        None => None,
                    }
                }
            };
            let Some(i) = free else {
                OVERFLOW.fetch_add(1, Ordering::Relaxed);
                return;
            };
            let old = ALIAS[i].swap(0, Ordering::AcqRel);
            if old != 0 {
                mach_vm_deallocate(
                    mach_task_self_,
                    old as u64,
                    (END[i].load(Ordering::Acquire) - START[i].load(Ordering::Acquire)) as u64,
                );
            }
            let alias = make_alias(start, end);
            if alias == 0 {
                return;
            }
            ALIAS[i].store(alias, Ordering::Release);
            START[i].store(start, Ordering::Release);
            END[i].store(end, Ordering::Release);
            REQ[i].store(0, Ordering::Release);
            if i >= n {
                HIGH.store(i + 1, Ordering::Release);
            }
            i
        }
    };
    table[i] = Some(Payload {
        tex: tex.clone(),
        t: *t,
        level,
        layer,
        cb: None,
    });
    SERIAL[i].store(UNASSIGNED, Ordering::Release);
    FRAME[i].store(super::presented_frames(), Ordering::Release);
    LO.fetch_min(start, Ordering::Relaxed);
    HI.fetch_max(end, Ordering::Relaxed);
    let new = STATE[i].swap(HELD, Ordering::AcqRel) != HELD;
    if new {
        LIVE.fetch_add(1, Ordering::AcqRel);
        HOLDS.fetch_add(1, Ordering::Relaxed);
    }
    // A held range again only if an engine access opened pages of it.
    if new || OPENED[i].swap(0, Ordering::AcqRel) != 0 {
        // Write-tracked pages stay tracked: this module's fault handler runs before dirty_track's, and opening a page
        // gives it its tracking protection back (open_prot).
        libc::mprotect(start as *mut libc::c_void, end - start, libc::PROT_NONE);
    }
    drop(table);
    OPEN.lock().unwrap().push(i);
}

/// A render pass ended: the holds made so far belong to the current command buffer.
pub(super) fn pass_end() {
    if !enabled() {
        return;
    }
    let mut o = OPEN.lock().unwrap();
    if !o.is_empty() {
        CLOSED.lock().unwrap().append(&mut o);
    }
}

/// Command buffer `cb` was queued with `serial`: the holds whose passes ended in it wait for it.
pub(super) fn committed(serial: u64, cb: &Retained<ProtocolObject<dyn MTLCommandBuffer>>) {
    if !enabled() {
        return;
    }
    let mut v = CLOSED.lock().unwrap();
    if v.is_empty() {
        return;
    }
    let mut table = TABLE.lock().unwrap();
    for i in v.drain(..) {
        if let Some(Some(p)) = table.get_mut(i) {
            p.cb = Some(cb.clone());
        }
        let _ = SERIAL[i].compare_exchange(UNASSIGNED, serial, Ordering::AcqRel, Ordering::Acquire);
    }
    drop(table);
    drop(v);
    wake();
}

/// Whether slot `i`'s rendering has finished on the GPU.
fn rendered(i: usize) -> bool {
    let s = SERIAL[i].load(Ordering::Acquire);
    if s == UNASSIGNED {
        return false;
    }
    if super::completed_serial() >= s {
        return true;
    }
    let table = TABLE.lock().unwrap();
    match table.get(i).and_then(|p| p.as_ref()).and_then(|p| p.cb.as_ref()) {
        Some(cb) => matches!(cb.status(), MTLCommandBufferStatus::Completed | MTLCommandBufferStatus::Error),
        None => false,
    }
}

fn params(t: &NvnTexture) -> crate::nvn::layout::TextureParams {
    crate::nvn::layout::TextureParams {
        target: t.target,
        width: t.width,
        height: t.height,
        depth: t.depth,
        levels: t.levels,
        format: t.format,
        flags: t.flags,
        samples: t.samples,
        stride: t.stride,
    }
}

/// Writes slot `i`'s texels into guest memory through its alias (write-back thread, after its command buffer completed).
unsafe fn write_back(i: usize) {
    let table = TABLE.lock().unwrap();
    let Some(pl) = table.get(i).and_then(|p| p.as_ref()) else {
        return;
    };
    let Some(info) = crate::nvn::layout::format_info(pl.t.format) else {
        return;
    };
    let p = params(&pl.t);
    let layout = crate::nvn::layout::compute_layout(&p);
    let (w, h) = (
        ((pl.t.width as usize) >> pl.level).max(1),
        ((pl.t.height as usize) >> pl.level).max(1),
    );
    let bpp = info.bytes as usize;
    let mut buf = vec![0u8; w * h * bpp];
    let slice = if pl.tex.textureType() == MTLTextureType::Type2DArray { pl.layer } else { 0 };
    if pl.level >= pl.tex.mipmapLevelCount() || slice >= pl.tex.arrayLength().max(1) {
        return;
    }
    pl.tex.getBytes_bytesPerRow_bytesPerImage_fromRegion_mipmapLevel_slice(
        std::ptr::NonNull::new(buf.as_mut_ptr().cast()).unwrap(),
        w * bpp,
        w * h * bpp,
        MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize { width: w, height: h, depth: 1 },
        },
        pl.level,
        slice,
    );
    let (start, alias) = (START[i].load(Ordering::Acquire), ALIAS[i].load(Ordering::Acquire));
    // The texture's storage as seen through the alias (it may begin before the held range: layers and levels).
    let storage = alias.wrapping_add(pl.t.storage.wrapping_sub(start));
    crate::nvn::layout::write_texels_2d(
        storage as *mut u8,
        pl.layer * layout.layer_stride as usize,
        &p,
        pl.level,
        0,
        0,
        w as i32,
        h as i32,
        buf.as_ptr(),
    );
    WRITEBACKS.fetch_add(1, Ordering::Relaxed);
}

// ---- write-back thread ---------------------------------------------------------------------------------------------

/// Pipe the signal handler writes to (write(2) is async-signal-safe) to wake the write-back thread.
static WAKE_FD: AtomicUsize = AtomicUsize::new(usize::MAX);

fn wake() {
    let fd = WAKE_FD.load(Ordering::Acquire);
    if fd != usize::MAX {
        let b = 1u8;
        unsafe { libc::write(fd as i32, &b as *const u8 as *const libc::c_void, 1) };
    }
}

/// Starts the write-back thread (once, before the first hold).
fn start_thread() {
    if !enabled() {
        return;
    }
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let mut fds = [0i32; 2];
        unsafe {
            libc::pipe(fds.as_mut_ptr());
            libc::fcntl(fds[1], libc::F_SETFL, libc::O_NONBLOCK);
        }
        WAKE_FD.store(fds[1] as usize, Ordering::Release);
        let rfd = fds[0];
        std::thread::Builder::new()
            .name("gfx-rt-writeback".into())
            .spawn(move || loop {
                let mut b = [0u8; 64];
                unsafe { libc::read(rfd, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
                loop {
                    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
                    let mut pending = false;
                    for i in 0..n {
                        if REQ[i].load(Ordering::Acquire) == 0 || STATE[i].load(Ordering::Acquire) != HELD {
                            continue;
                        }
                        let s = SERIAL[i].load(Ordering::Acquire);
                        if !rendered(i) {
                            pending = true;
                            continue;
                        }
                        objc2::rc::autoreleasepool(|_| unsafe { write_back(i) });
                        // A newer rendering held it meanwhile: its own command buffer decides.
                        if SERIAL[i].load(Ordering::Acquire) == s {
                            unsafe { finish(i) };
                        } else {
                            pending = true;
                        }
                    }
                    if !pending {
                        break;
                    }
                    // Fast while a game thread waits; otherwise (background write-backs) block until woken or 2 ms.
                    if GUEST_WAITING.load(Ordering::Acquire) != 0 {
                        std::thread::sleep(std::time::Duration::from_micros(50));
                    } else {
                        let mut pfd = libc::pollfd { fd: rfd, events: libc::POLLIN, revents: 0 };
                        unsafe { libc::poll(&mut pfd, 1, 2) };
                        if pfd.revents != 0 {
                            unsafe { libc::read(rfd, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
                        }
                    }
                }
            })
            .expect("spawn rt write-back");
    });
}

// ---- faults (signal context: atomics, mprotect, write and nanosleep only) -------------------------------------------

/// Signal-handler hook: true if `addr` is on a held page (the access is then retried).
pub(super) unsafe fn fault(addr: usize, pc: usize, lr: usize, guest: bool, write: bool) -> bool {
    if !enabled() || HIGH.load(Ordering::Acquire) == 0 {
        return false;
    }
    let page = addr & !(PAGE - 1);
    // Not near any hold and no hold ended recently: someone else's fault (write tracking, the write guard, ...).
    if (LIVE.load(Ordering::Acquire) == 0 || page >= HI.load(Ordering::Relaxed) || page + PAGE <= LO.load(Ordering::Relaxed))
        && !RECENT_PAGES.iter().any(|p| p.load(Ordering::Relaxed) == page)
    {
        return false;
    }
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    if !(0..n).any(|i| covers(i, page)) {
        // The hold ended between the fault and here: retry; a second fault there on a page left inaccessible is
        // repaired (opened) rather than reported as a crash.
        if page != 0 && RECENT_PAGES.iter().any(|p| p.load(Ordering::Acquire) == page) {
            // A write to a page write tracking or the write guard keeps read-only is theirs: claiming it here
            // reopened the page read-only on every retry and the writer faulted forever.
            if write && open_prot(page) & libc::PROT_WRITE == 0 {
                return false;
            }
            if LAST_RETRY.with(|r| r.replace(addr)) != addr {
                return true;
            }
            LAST_RETRY.with(|r| r.set(0));
            libc::mprotect(page as *mut libc::c_void, PAGE, open_prot(page));
            return true;
        }
        return false;
    }
    LAST_RETRY.with(|r| r.set(0));
    if !(guest || GUEST_COPY.with(|g| g.get())) {
        let k = HOST_RELEASES.fetch_add(1, Ordering::Relaxed) as usize;
        if k < RELEASE_LOG {
            RELEASE_PC[k].store(pc, Ordering::Relaxed);
            RELEASE_LR[k].store(lr, Ordering::Relaxed);
            RELEASE_ADDR[k].store(addr, Ordering::Relaxed);
            let mut name = [0u8; 16];
            libc::pthread_getname_np(libc::pthread_self(), name.as_mut_ptr() as *mut libc::c_char, name.len());
            RELEASE_NAME[k][0].store(u64::from_le_bytes(name[..8].try_into().unwrap()), Ordering::Relaxed);
            RELEASE_NAME[k][1].store(u64::from_le_bytes(name[8..].try_into().unwrap()), Ordering::Relaxed);
        }
        // Engine code writes the page: its data is newer than the rendering, which must not be written over it later.
        if write {
            supersede(page, page + PAGE, None);
            return true;
        }
        // Engine code (often another thread racing with the hold being made) reads the page: open it for reading only
        // (a later write still faults: guest writes wait for the write-back, engine writes end the hold), and write the
        // hold back as soon as its rendering is done instead of on the next guest access.
        for i in 0..n {
            if covers(i, page) {
                REQ[i].store(1, Ordering::Release);
                OPENED[i].store(1, Ordering::Release);
            }
        }
        libc::mprotect(page as *mut libc::c_void, PAGE, libc::PROT_READ);
        wake();
        return true;
    }
    WAITS.fetch_add(1, Ordering::Relaxed);
    for i in 0..n {
        if covers(i, page) {
            REQ[i].store(1, Ordering::Release);
            GUEST_READ[i].fetch_add(1, Ordering::Relaxed);
        }
    }
    GUEST_WAITING.fetch_add(1, Ordering::AcqRel);
    wake();
    let t0 = std::time::Instant::now();
    while held(page) {
        if t0.elapsed().as_secs() >= 2 {
            TIMEOUTS.fetch_add(1, Ordering::Relaxed);
            for i in 0..n {
                if covers(i, page) {
                    finish(i);
                }
            }
            break;
        }
        let ts = libc::timespec { tv_sec: 0, tv_nsec: 20_000 };
        libc::nanosleep(&ts, std::ptr::null_mut());
    }
    GUEST_WAITING.fetch_sub(1, Ordering::AcqRel);
    WAIT_US.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
    true
}

/// After another module changed the protection of [addr, addr+len): held pages stay inaccessible (signal-safe).
pub(super) unsafe fn reprotect_held(addr: usize, len: usize) {
    if LIVE.load(Ordering::Acquire) == 0 || addr >= HI.load(Ordering::Relaxed) || addr + len <= LO.load(Ordering::Relaxed) {
        return;
    }
    let mut p = addr & !(PAGE - 1);
    while p < addr + len {
        if held(p) {
            libc::mprotect(p as *mut libc::c_void, PAGE, libc::PROT_NONE);
        }
        p += PAGE;
    }
}

/// Whether [addr, addr+len) touches a held page.
pub(crate) fn overlaps_held(addr: usize, len: usize) -> bool {
    if LIVE.load(Ordering::Acquire) == 0
        || addr >= HI.load(Ordering::Relaxed)
        || addr + len <= LO.load(Ordering::Relaxed)
    {
        return false;
    }
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    n != 0
        && (0..n).any(|i| {
            STATE[i].load(Ordering::Acquire) == HELD
                && START[i].load(Ordering::Acquire) < addr + len
                && END[i].load(Ordering::Acquire) > addr
        })
}

/// `n` elements of guest memory at `addr` for engine code: a copy read through the holds' second mappings when the
/// range touches a held page (reading it directly would end the hold), else None (read it in place).
pub(crate) unsafe fn guest_copy<T: Copy + Default>(addr: usize, n: usize) -> Option<Vec<T>> {
    let len = n * std::mem::size_of::<T>();
    if !overlaps_held(addr, len) {
        return None;
    }
    let mut v = vec![T::default(); n];
    super::report_guard::engine_copy(std::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut u8, len), addr as *const u8);
    Some(v)
}

/// Runs `f` with the held pages of [addr, addr+len) accessible (Metal refuses a no-copy buffer over inaccessible pages;
/// one made before they are protected keeps working), then protects them again.
pub(crate) unsafe fn with_open<R>(addr: usize, len: usize, f: impl FnOnce() -> R) -> R {
    if !overlaps_held(addr, len) {
        return f();
    }
    let (start, end) = (addr & !(PAGE - 1), (addr + len + PAGE - 1) & !(PAGE - 1));
    let mut opened = Vec::new();
    let mut p = start;
    while p < end {
        if held(p) {
            libc::mprotect(p as *mut libc::c_void, PAGE, open_prot(p));
            opened.push(p);
        }
        p += PAGE;
    }
    let r = f();
    for p in opened {
        if held(p) {
            libc::mprotect(p as *mut libc::c_void, PAGE, libc::PROT_NONE);
        }
    }
    r
}

/// Whether any hold exists (fast path of report_guard::engine_copy).
pub(super) fn any_held() -> bool {
    LIVE.load(Ordering::Acquire) != 0
}

/// Where the engine accesses host address `a`: through the hold's alias while its page is held, else `a` itself.
pub(super) fn engine_ptr(a: usize) -> Option<usize> {
    if LIVE.load(Ordering::Acquire) == 0 || a >= HI.load(Ordering::Relaxed) || a < LO.load(Ordering::Relaxed) {
        return None;
    }
    let page = a & !(PAGE - 1);
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    (0..n)
        .find(|&i| covers(i, page))
        .map(|i| ALIAS[i].load(Ordering::Acquire) + (a - START[i].load(Ordering::Acquire)))
}

/// Guest memory [addr, addr+len) was released (pool finalized, region freed): its holds end without a write-back.
pub(crate) fn released(addr: usize, len: usize) {
    if HIGH.load(Ordering::Acquire) == 0 {
        return;
    }
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    for i in 0..n {
        if STATE[i].load(Ordering::Acquire) == HELD
            && START[i].load(Ordering::Acquire) < addr + len
            && END[i].load(Ordering::Acquire) > addr
        {
            unsafe { finish(i) };
        }
    }
}

/// Statistics every 300 frames (--log-rt-writeback).
pub(super) fn frame_end(frame: u64) {
    if !enabled() {
        return;
    }
    let n = HIGH.load(Ordering::Acquire).min(SLOTS);
    let mut aged = false;
    let (mut lo, mut hi) = (usize::MAX, 0usize);
    for i in 0..n {
        if STATE[i].load(Ordering::Acquire) != HELD {
            continue;
        }
        lo = lo.min(START[i].load(Ordering::Acquire));
        hi = hi.max(END[i].load(Ordering::Acquire));
        if FRAME[i].load(Ordering::Acquire) + AGE < frame {
            REQ[i].store(1, Ordering::Release);
            aged = true;
        }
    }
    // Narrow the quick-reject range to the live holds (a hold made meanwhile widens it again, so this only shrinks it
    // to what was live a moment ago: widen with anything held since).
    LO.store(lo, Ordering::Relaxed);
    HI.store(hi, Ordering::Relaxed);
    for i in 0..n {
        if STATE[i].load(Ordering::Acquire) == HELD {
            LO.fetch_min(START[i].load(Ordering::Acquire), Ordering::Relaxed);
            HI.fetch_max(END[i].load(Ordering::Acquire), Ordering::Relaxed);
        }
    }
    if aged {
        wake();
    }
    if frame % 300 != 0 || !crate::settings::get().log.log_rt_writeback {
        return;
    }
    extern "C" {
        fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
    }
    let slide = unsafe { _dyld_get_image_vmaddr_slide(0) } as usize;
    let base = crate::cpu::crash_handler::get_guest_base();
    let n = (HOST_RELEASES.load(Ordering::Relaxed) as usize).min(RELEASE_LOG);
    for k in 0..n {
        let mut name = [0u8; 16];
        name[..8].copy_from_slice(&RELEASE_NAME[k][0].load(Ordering::Relaxed).to_le_bytes());
        name[8..].copy_from_slice(&RELEASE_NAME[k][1].load(Ordering::Relaxed).to_le_bytes());
        let name = String::from_utf8_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(16)]).to_string();
        let pc = RELEASE_PC[k].load(Ordering::Relaxed);
        let lr = RELEASE_LR[k].load(Ordering::Relaxed);
        let at = |a: usize| if a >= base && a < base + (256 << 20) { format!("main+{:#x}", a - base) } else { format!("engine+{:#x}", a.wrapping_sub(slide)) };
        println!("[rt-guard]   host release: {} pc {} lr {} addr {:#x}", name, at(pc), at(lr), RELEASE_ADDR[k].load(Ordering::Relaxed));
    }
    {
        let table = TABLE.lock().unwrap();
        let mut seen: std::collections::BTreeMap<String, u64> = Default::default();
        for i in 0..HIGH.load(Ordering::Acquire).min(SLOTS) {
            let k = GUEST_READ[i].swap(0, Ordering::Relaxed);
            if k == 0 {
                continue;
            }
            if let Some(Some(p)) = table.get(i) {
                *seen
                    .entry(format!("{}x{} fmt {} flags {:#x} level {} layer {} at {:#x}", p.t.width, p.t.height, p.t.format, p.t.flags, p.level, p.layer, p.t.storage))
                    .or_default() += k;
            }
        }
        for (d, k) in seen {
            println!("[rt-guard]   guest reads {k}: {d}");
        }
    }
    let take = |a: &AtomicU64| a.swap(0, Ordering::Relaxed);
    let held_now = (0..HIGH.load(Ordering::Acquire).min(SLOTS))
        .filter(|&i| STATE[i].load(Ordering::Acquire) == HELD)
        .count();
    println!(
        "[rt-guard] last 300 frames: {} holds, {} write-backs, {} superseded, {} waits ({:.2} ms per frame), engine opens {}, timeouts {}, overflows {}; held now {}",
        take(&HOLDS),
        take(&WRITEBACKS),
        take(&DROPS),
        take(&WAITS),
        take(&WAIT_US) as f64 / 300_000.0,
        take(&HOST_RELEASES),
        take(&TIMEOUTS),
        take(&OVERFLOW),
        held_now
    );
}
