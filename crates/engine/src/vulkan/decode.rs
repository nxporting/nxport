// The submit's commands in the order they run, on a thread of its own (vk-decode): it walks the lists, following
// CallCommands, and publishes each command as it reaches it while the Presentation Thread executes the ones published so
// far. Walking ~7,500 list calls and ~44,000 commands a submit (TotK) was ~5% of that thread, and the pre-scan before
// it (where the occlusion counters are reset and reported) another ~5%; both are done here now, in one walk.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::nvn::api::{RecordedCommandList, RecordedNvnCommand};

/// A command and the list it is in (checkpoints name it).
#[derive(Clone, Copy)]
pub(super) struct Entry {
    pub(super) op: *const RecordedNvnCommand,
    pub(super) list: u64,
}

type Lists = rustc_hash::FxHashMap<u64, Arc<RecordedCommandList>>;

const CHUNK: usize = 4096;
const CHUNKS: usize = 1024;

/// Items in chunks that never move (made as needed, kept for later submits): the reader indexes what was published
/// while the writer appends.
struct Chunks<T: Copy> {
    chunks: Box<[AtomicPtr<T>]>,
    fill: T,
}

impl<T: Copy> Chunks<T> {
    fn new(fill: T) -> Chunks<T> {
        Chunks { chunks: (0..CHUNKS).map(|_| AtomicPtr::new(std::ptr::null_mut())).collect(), fill }
    }

    /// Writes item `i` (false: past the last chunk).
    unsafe fn put(&self, i: usize, v: T) -> bool {
        let Some(slot) = self.chunks.get(i / CHUNK) else { return false };
        let mut chunk = slot.load(Ordering::Relaxed);
        if chunk.is_null() {
            chunk = Box::leak(vec![self.fill; CHUNK].into_boxed_slice()).as_mut_ptr();
            slot.store(chunk, Ordering::Release);
        }
        chunk.add(i % CHUNK).write(v);
        true
    }

    /// Item `i`, published.
    unsafe fn get(&self, i: usize) -> T {
        *self.chunks[i / CHUNK].load(Ordering::Acquire).add(i % CHUNK)
    }
}

/// One submit's walk: what to walk (the lists by handle, the submit's map: Ctx::lists, unchanged until the walk ended)
/// and the lists looked up again because the map lacked them (kept until the submit ended).
/// With `resolve`, the map holds the roots only (nvn::api::pin_command_lists) and the walk fetches the lists they call
/// itself, under one lock per CallCommands it reaches first (nvn::api::fetch_command_lists_at, into `own`): the
/// Presentation Thread no longer walks them before recording (~0.6 ms a frame in TotK).
struct Walk {
    roots: Vec<u64>,
    lists: *const Lists,
    held: Vec<Arc<RecordedCommandList>>,
    resolve: bool,
    own: Lists,
    now: u64,
    len: usize,
    events: usize,
}

struct Shared {
    walk: std::cell::UnsafeCell<Walk>,
    entries: Chunks<Entry>,
    /// Occlusion counter resets (false) and SAMPLES_PASSED reports (true), by command index.
    events: Chunks<(u64, bool)>,
    /// Entries and events published (written before these are raised; an event before the entries after it), and
    /// whether the walk ended.
    avail: AtomicUsize,
    events_avail: AtomicUsize,
    done: AtomicBool,
}

// The walk is the decoder's between start and the done flag, the Presentation Thread's otherwise; published items are
// only read.
unsafe impl Sync for Shared {}
unsafe impl Send for Shared {}

pub(super) struct Decoder {
    shared: Arc<Shared>,
    tx: std::sync::mpsc::Sender<()>,
    /// Entries read so far, and the first counter event not yet passed (peek_counter).
    next: usize,
    event: usize,
}

/// Entries published at a time.
const PUBLISH: usize = 32;

impl Decoder {
    pub(super) fn new() -> Result<Decoder, String> {
        let shared = Arc::new(Shared {
            walk: std::cell::UnsafeCell::new(Walk {
                roots: Vec::new(),
                lists: std::ptr::null(),
                held: Vec::new(),
                resolve: false,
                own: Lists::default(),
                now: 0,
                len: 0,
                events: 0,
            }),
            entries: Chunks::new(Entry { op: std::ptr::null(), list: 0 }),
            events: Chunks::new((0, false)),
            avail: AtomicUsize::new(0),
            events_avail: AtomicUsize::new(0),
            done: AtomicBool::new(true),
        });
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let s = shared.clone();
        std::thread::Builder::new()
            .name("vk-decode".into())
            .spawn(move || {
                let mut pinned = false;
                while rx.recv().is_ok() {
                    // On the recording threads' L3 cache (vulkan::REC_AFFINITY): what it publishes is read right away.
                    if !pinned {
                        let mask = crate::vulkan::rec_affinity();
                        if mask != 0 {
                            unsafe { crate::vulkan::pin_current_thread(mask) };
                            pinned = true;
                        }
                    }
                    unsafe { decode(&s) };
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Decoder { shared, tx, next: 0, event: 0 })
    }

    /// Starts walking `roots`, finding lists in `lists` (which must not change until finish); with `resolve`, fetching
    /// them itself.
    pub(super) fn start(&mut self, roots: &[u64], lists: &Lists, resolve: bool) {
        let w = unsafe { &mut *self.shared.walk.get() };
        w.roots.clear();
        w.roots.extend_from_slice(roots);
        w.lists = lists;
        w.held.clear();
        w.len = 0;
        w.resolve = resolve;
        w.now = if resolve { crate::nvn::api::list_walk_clock() } else { 0 };
        w.events = 0;
        (self.next, self.event) = (0, 0);
        self.shared.avail.store(0, Ordering::Relaxed);
        self.shared.events_avail.store(0, Ordering::Relaxed);
        self.shared.done.store(false, Ordering::Release);
        if self.tx.send(()).is_err() {
            self.shared.done.store(true, Ordering::Release);
        }
    }

    /// Waits until `ready` holds or the walk ended.
    fn wait(&self, ready: impl Fn() -> bool) -> bool {
        let mut spins = 0u32;
        loop {
            if ready() {
                return true;
            }
            if self.shared.done.load(Ordering::Acquire) {
                // The last items are published with the done flag.
                return ready();
            }
            spins += 1;
            if spins < 512 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }

    /// The next command, waiting for the walk to reach it; None once the walk ended and every command was read.
    pub(super) fn next(&mut self) -> Option<Entry> {
        let i = self.next;
        if !self.wait(|| i < self.shared.avail.load(Ordering::Acquire)) {
            return None;
        }
        self.next += 1;
        Some(unsafe { self.shared.entries.get(i) })
    }

    /// Whether the next occlusion counter event after command `index` resets the counter (Some(Some(false))) or reports it
    /// (Some(Some(true))); Some(None): none follows. None while the walk has neither found it nor ended (no waiting).
    pub(super) fn peek_counter(&mut self, index: u64) -> Option<Option<bool>> {
        loop {
            let e = self.event;
            // The done flag is published after the last events: read first, an event seen after it is not missed.
            let done = self.shared.done.load(Ordering::Acquire);
            if e >= self.shared.events_avail.load(Ordering::Acquire) {
                return done.then_some(None);
            }
            let (at, report) = unsafe { self.shared.events.get(e) };
            if at > index {
                return Some(Some(report));
            }
            self.event += 1;
        }
    }

    /// Waits for the walk to end (every command was read).
    pub(super) fn finish(&mut self) {
        while !self.shared.done.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        unsafe { (*self.shared.walk.get()).held.clear() };
    }
}

unsafe fn decode(s: &Shared) {
    let w = &mut *s.walk.get();
    // The last submit's lists (its commands all ran), let go here rather than on the Presentation Thread.
    w.own.clear();
    let roots = std::mem::take(&mut w.roots);
    for &h in &roots {
        if !walk_list(s, w, h, 0) {
            break;
        }
    }
    w.roots = roots;
    s.events_avail.store(w.events, Ordering::Release);
    s.avail.store(w.len, Ordering::Release);
    s.done.store(true, Ordering::Release);
}

/// A counter event at the next command (published before it is).
unsafe fn counter_event(s: &Shared, w: &mut Walk, report: bool) {
    if s.events.put(w.events, (w.len as u64, report)) {
        w.events += 1;
        s.events_avail.store(w.events, Ordering::Release);
    }
}

/// Walks list `handle`: calls are followed to 16 levels, deeper ones are commands of their own. False when the entries
/// are full (the commands after are left out, with a warning).
unsafe fn walk_list(s: &Shared, w: &mut Walk, handle: u64, depth: u32) -> bool {
    let found = if w.resolve { w.own.get(&handle).or_else(|| (*w.lists).get(&handle)) } else { (*w.lists).get(&handle) };
    let list: *const RecordedCommandList = match found {
        Some(l) => Arc::as_ptr(l),
        None => match crate::nvn::api::command_list(handle, depth > 0) {
            Some(l) => {
                let p = Arc::as_ptr(&l);
                w.held.push(l);
                p
            }
            None => return true,
        },
    };
    for op in &(*list).ops {
        use crate::nvn::cmd::Cmd;
        match &op.typed {
            Some(Cmd::CallCommands(hs)) if depth < 16 => {
                // The lists it calls, fetched under one lock when the walk first reaches them: fetching all the lists
                // a root reaches before its first command (~3,000 in TotK) kept the Presentation Thread waiting ~1 ms
                // for that command.
                if w.resolve && !hs.iter().all(|h| w.own.contains_key(h)) {
                    crate::nvn::api::fetch_command_lists_at(w.now, hs, &mut w.own);
                }
                for &h in hs.iter() {
                    if !walk_list(s, w, h, depth + 1) {
                        return false;
                    }
                }
                continue;
            }
            // Occlusion counter resets and SAMPLES_PASSED reports, at this command's index.
            Some(Cmd::ResetCounter { counter: 1 }) => counter_event(s, w, false),
            Some(Cmd::ReportCounter { counter: 1, .. }) => counter_event(s, w, true),
            _ => {}
        }
        if !s.entries.put(w.len, Entry { op, list: handle }) {
            println!("[vulkan] a submit of more than {} commands: the rest is left out", CHUNK * CHUNKS);
            return false;
        }
        w.len += 1;
        if w.len % PUBLISH == 0 {
            s.avail.store(w.len, Ordering::Release);
        }
    }
    true
}
