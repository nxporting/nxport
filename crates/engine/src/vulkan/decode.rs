// The submit's commands in the order they run, on a thread of its own (vk-decode): it walks the lists, following
// CallCommands, and publishes each command as it reaches it while the Presentation Thread executes the ones published so
// far. Walking ~7,500 list calls and ~44,000 commands a submit (TotK) was ~5% of that thread.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::nvn::api::{RecordedCommandList, RecordedNvnCommand};
use crate::nvn::cmd::Cmd;

/// A command and the list it is in (checkpoints name it).
#[derive(Clone, Copy)]
pub(super) struct Entry {
    pub(super) op: *const RecordedNvnCommand,
    pub(super) list: u64,
}

const CHUNK: usize = 4096;
const CHUNKS: usize = 1024;

/// One submit's walk: what to walk, and the lists looked up again when the pre-scan's order ran out (kept until the
/// submit ended).
struct Walk {
    roots: Vec<u64>,
    /// The pre-scan's visiting order (Ctx::scan_counters; null: not found).
    order: Vec<*const RecordedCommandList>,
    held: Vec<Arc<RecordedCommandList>>,
    len: usize,
}

struct Shared {
    walk: std::cell::UnsafeCell<Walk>,
    /// The entries, in chunks that never move (made as needed, kept for later submits).
    chunks: Box<[AtomicPtr<Entry>]>,
    /// Entries published (written before it is raised), and whether the walk ended.
    avail: AtomicUsize,
    done: AtomicBool,
}

// The walk is the decoder's between start and the done flag, the Presentation Thread's otherwise; published entries are
// only read.
unsafe impl Sync for Shared {}
unsafe impl Send for Shared {}

pub(super) struct Decoder {
    shared: Arc<Shared>,
    tx: std::sync::mpsc::Sender<()>,
    /// Entries read so far.
    next: usize,
}

/// Entries published at a time.
const PUBLISH: usize = 32;

impl Decoder {
    pub(super) fn new() -> Result<Decoder, String> {
        let shared = Arc::new(Shared {
            walk: std::cell::UnsafeCell::new(Walk { roots: Vec::new(), order: Vec::new(), held: Vec::new(), len: 0 }),
            chunks: (0..CHUNKS).map(|_| AtomicPtr::new(std::ptr::null_mut())).collect(),
            avail: AtomicUsize::new(0),
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
        Ok(Decoder { shared, tx, next: 0 })
    }

    /// Starts walking `roots` in the pre-scan's `order` (taken; handed back by finish).
    pub(super) fn start(&mut self, roots: &[u64], order: &mut Vec<*const RecordedCommandList>) {
        let w = unsafe { &mut *self.shared.walk.get() };
        w.roots.clear();
        w.roots.extend_from_slice(roots);
        std::mem::swap(&mut w.order, order);
        w.held.clear();
        w.len = 0;
        self.next = 0;
        self.shared.avail.store(0, Ordering::Relaxed);
        self.shared.done.store(false, Ordering::Release);
        if self.tx.send(()).is_err() {
            self.shared.done.store(true, Ordering::Release);
        }
    }

    /// The next command, waiting for the walk to reach it; None once the walk ended and every command was read.
    pub(super) fn next(&mut self) -> Option<Entry> {
        let mut spins = 0u32;
        loop {
            if self.next < self.shared.avail.load(Ordering::Acquire) {
                let i = self.next;
                self.next += 1;
                let chunk = self.shared.chunks[i / CHUNK].load(Ordering::Acquire);
                return Some(unsafe { *chunk.add(i % CHUNK) });
            }
            if self.shared.done.load(Ordering::Acquire) {
                // The last entries are published with the done flag.
                if self.next < self.shared.avail.load(Ordering::Acquire) {
                    continue;
                }
                return None;
            }
            spins += 1;
            if spins < 512 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }

    /// Waits for the walk to end and hands its order back for the next pre-scan.
    pub(super) fn finish(&mut self, order: &mut Vec<*const RecordedCommandList>) {
        while !self.shared.done.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let w = unsafe { &mut *self.shared.walk.get() };
        std::mem::swap(&mut w.order, order);
        w.held.clear();
    }
}

unsafe fn decode(s: &Shared) {
    let w = &mut *s.walk.get();
    let roots = std::mem::take(&mut w.roots);
    let mut at = 0;
    for &h in &roots {
        if !walk_list(s, w, &mut at, h, 0) {
            break;
        }
    }
    w.roots = roots;
    s.avail.store(w.len, Ordering::Release);
    s.done.store(true, Ordering::Release);
}

/// Walks list `handle`: calls are followed to 16 levels (as the pre-scan, Ctx::scan_counters, counts them), deeper ones
/// are commands of their own. False when the entries are full (the commands after are left out, with a warning).
unsafe fn walk_list(s: &Shared, w: &mut Walk, at: &mut usize, handle: u64, depth: u32) -> bool {
    let next = w.order.get(*at).copied();
    *at += 1;
    let list: *const RecordedCommandList = match next {
        // Missing at the pre-scan: left out here too (the lists after it are still the ones it visited next).
        Some(p) if p.is_null() => return true,
        Some(p) => p,
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
        if let Some(Cmd::CallCommands(hs)) = &op.typed {
            if depth < 16 {
                for &h in hs.iter() {
                    if !walk_list(s, w, at, h, depth + 1) {
                        return false;
                    }
                }
                continue;
            }
        }
        let (c, k) = (w.len / CHUNK, w.len % CHUNK);
        if c >= CHUNKS {
            println!("[vulkan] a submit of more than {} commands: the rest is left out", CHUNK * CHUNKS);
            return false;
        }
        let mut chunk = s.chunks[c].load(Ordering::Relaxed);
        if chunk.is_null() {
            chunk = Box::leak(vec![Entry { op: std::ptr::null(), list: 0 }; CHUNK].into_boxed_slice()).as_mut_ptr();
            s.chunks[c].store(chunk, Ordering::Release);
        }
        chunk.add(k).write(Entry { op, list: handle });
        w.len += 1;
        if w.len % PUBLISH == 0 {
            s.avail.store(w.len, Ordering::Release);
        }
    }
    true
}
