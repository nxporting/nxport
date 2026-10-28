// Vertex and index data in device memory. Imported guest memory is host memory the GPU snoops (HOST_CACHED, the type
// the driver gives imported host pointers), and TotK's main geometry pass read its meshes from there with every draw
// (609 draws beneath Hyrule Castle: 8 ms of GPU time at 1280x720 on an RTX 4080). Most of that data is written once and
// drawn for many frames, so a range drawn from is copied into device-local memory and used from there while it is
// unchanged: the write tracking (guest::dirty_track) watches ranges large enough for its pages, smaller ones are
// compared by content hash once per submit. A range written again is copied again, into a new block (submits still
// running read the old one); one written again and again (streamed or animated geometry) is not cached any more and is
// read in place as before.

use std::collections::BTreeMap;

use ash::vk;
use rustc_hash::FxHashMap;

use super::device::Dev;

const CHUNK: u64 = 64 << 20;
const ALIGN: u64 = 256;
/// Copies of one range after which it counts as rewritten all the time.
const MAX_UPLOADS: u32 = 4;
/// Ranges smaller than this are checked by hash (the write tracking needs half a page at least).
const TRACKED: usize = 0x2000;
/// Frames a range may go undrawn before its block is freed.
const IDLE_FRAMES: u64 = 600;

struct Chunk {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    /// Free blocks by offset: length.
    free: BTreeMap<u64, u64>,
}

struct Entry {
    chunk: usize,
    off: u64,
    size: u64,
    /// Write-tracking token, or the contents' hash for small ranges.
    token: Option<u64>,
    hash: u64,
    /// The submit that last drew from it, and the one that last checked it.
    used: u64,
    checked: u64,
    used_frame: u64,
    uploads: u32,
    dynamic: bool,
}

#[derive(Default)]
pub(crate) struct VCache {
    chunks: Vec<Chunk>,
    entries: FxHashMap<(usize, usize), Entry>,
    /// Blocks no longer drawn from, freed once the submit that last used them completed: (serial, chunk, offset, size).
    retired: Vec<(u64, usize, u64, u64)>,
    swept_frame: u64,
    /// Small ranges drawn from this submit before their hash was checked (verify).
    unverified: Vec<(usize, usize)>,
}

impl VCache {
    unsafe fn alloc(&mut self, dev: &Dev, size: u64) -> Option<(usize, u64)> {
        let size = size.next_multiple_of(ALIGN);
        for (i, c) in self.chunks.iter_mut().enumerate() {
            if let Some((&off, &len)) = c.free.iter().find(|(_, &l)| l >= size) {
                c.free.remove(&off);
                if len > size {
                    c.free.insert(off + size, len - size);
                }
                return Some((i, off));
            }
        }
        if size > CHUNK {
            return None;
        }
        let usage = vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER | vk::BufferUsageFlags::TRANSFER_DST;
        let buffer = dev.device.create_buffer(&vk::BufferCreateInfo::default().size(CHUNK).usage(usage), None).ok()?;
        let req = dev.device.get_buffer_memory_requirements(buffer);
        let memory = match dev.alloc(req, vk::MemoryPropertyFlags::DEVICE_LOCAL) {
            Ok(m) if dev.device.bind_buffer_memory(buffer, m, 0).is_ok() => m,
            Ok(m) => {
                dev.device.free_memory(m, None);
                dev.device.destroy_buffer(buffer, None);
                return None;
            }
            Err(_) => {
                dev.device.destroy_buffer(buffer, None);
                return None;
            }
        };
        let mut free = BTreeMap::new();
        if size < CHUNK {
            free.insert(size, CHUNK - size);
        }
        self.chunks.push(Chunk { buffer, memory, free });
        Some((self.chunks.len() - 1, 0))
    }

    fn release(&mut self, chunk: usize, off: u64, size: u64) {
        let free = &mut self.chunks[chunk].free;
        let (mut off, mut size) = (off, size.next_multiple_of(ALIGN));
        if let Some((&prev, &len)) = free.range(..off).next_back() {
            if prev + len == off {
                free.remove(&prev);
                off = prev;
                size += len;
            }
        }
        if let Some(len) = free.remove(&(off + size)) {
            size += len;
        }
        free.insert(off, size);
    }

    /// The device copy of guest memory [a, a + len), made now if it has none or is out of date; None where it is not
    /// cached (read it in place). `upload(dst, dst_offset, src, len)` records the copy of the contents into the block.
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn get(
        &mut self,
        dev: &Dev,
        a: usize,
        len: usize,
        serial: u64,
        frame: u64,
        upload: &mut dyn FnMut(vk::Buffer, u64, *const u8, usize) -> bool,
    ) -> Option<(vk::Buffer, u64)> {
        if len == 0 || len as u64 > CHUNK {
            return None;
        }
        let tracked = len >= TRACKED && crate::guest::dirty_track::trackable(a, len);
        let mut uploads = 0;
        if let Some(e) = self.entries.get_mut(&(a, len)) {
            if e.dynamic {
                return None;
            }
            // Small ranges are drawn from as they were and hashed before the submit (verify), on several threads:
            // hashing them here, one by one, was over a millisecond of each frame's recording.
            let fresh = e.checked == serial
                || match e.token {
                    Some(t) => crate::guest::dirty_track::generation(a, len) == Some(t),
                    None => {
                        self.unverified.push((a, len));
                        true
                    }
                };
            if fresh {
                e.checked = serial;
                e.used = serial;
                e.used_frame = frame;
                return Some((self.chunks[e.chunk].buffer, e.off));
            }
            // Written since it was copied: its block goes once the submits drawing from it completed.
            let e = self.entries.remove(&(a, len)).unwrap();
            self.retired.push((e.used, e.chunk, e.off, e.size));
            uploads = e.uploads + 1;
            if uploads >= MAX_UPLOADS {
                self.entries.insert((a, len), Entry { dynamic: true, uploads, ..e });
                return None;
            }
        }
        // The token first, so writes made while copying change it.
        let token = if tracked { Some(crate::guest::dirty_track::protect(a, len)?) } else { None };
        let src = std::slice::from_raw_parts(a as *const u8, len);
        let hash = if tracked { 0 } else { fast_hash(src) };
        let (chunk, off) = self.alloc(dev, len as u64)?;
        if !upload(self.chunks[chunk].buffer, off, src.as_ptr(), len) {
            self.release(chunk, off, len as u64);
            return None;
        }
        self.entries.insert(
            (a, len),
            Entry { chunk, off, size: len as u64, token, hash, used: serial, checked: serial, used_frame: frame, uploads, dynamic: false },
        );
        Some((self.chunks[chunk].buffer, off))
    }

    /// Hashes the ranges drawn from unchecked (get) and copies those written since into their blocks, recorded with
    /// `upload` ahead of the draws (the uploads run first in the queue, after what earlier submits read there). Before
    /// anything recorded since the last call is submitted.
    pub(super) unsafe fn verify(&mut self, serial: u64, upload: &mut dyn FnMut(vk::Buffer, u64, *const u8, usize) -> bool) {
        if self.unverified.is_empty() {
            return;
        }
        let ranges = std::mem::take(&mut self.unverified);
        let bytes: usize = ranges.iter().map(|r| r.1).sum();
        let pool = if bytes < 256 << 10 { None } else { hash_pool().lock().ok() };
        let hashes: Vec<u64> = match pool {
            None => ranges.iter().map(|r| hash_range(r)).collect(),
            Some(pool) => {
                // This thread hashes the first part, the pool the others.
                let per = ranges.len().div_ceil(pool.len() + 1);
                let mut parts = ranges.chunks(per);
                let mine = parts.next().unwrap_or(&[]);
                let sent: smallvec::SmallVec<[(&[(usize, usize)], bool); 8]> =
                    parts.zip(pool.iter()).map(|(part, (tx, _))| (part, tx.send(HashJob(part.as_ptr(), part.len())).is_ok())).collect();
                let mut hashes: Vec<u64> = mine.iter().map(|r| hash_range(r)).collect();
                for (&(part, ok), (_, rx)) in sent.iter().zip(pool.iter()) {
                    match if ok { rx.recv().ok() } else { None } {
                        Some(h) => hashes.extend(h),
                        None => hashes.extend(part.iter().map(|r| hash_range(r))),
                    }
                }
                hashes
            }
        };
        for (&(a, len), h) in ranges.iter().zip(hashes) {
            let Some(e) = self.entries.get_mut(&(a, len)) else { continue };
            if e.hash == h || e.dynamic {
                continue;
            }
            // This submit's draws read the block: the new contents go there.
            e.hash = h;
            e.uploads += 1;
            let (chunk, off) = (e.chunk, e.off);
            let ok = upload(self.chunks[chunk].buffer, off, a as *const u8, len);
            let e = self.entries.get_mut(&(a, len)).unwrap();
            if !ok || e.uploads >= MAX_UPLOADS {
                // Rewritten all the time: read in place from now on, its block freed after this submit.
                e.dynamic = true;
                let (chunk, off, size) = (e.chunk, e.off, e.size);
                self.retired.push((serial, chunk, off, size));
            }
        }
    }

    /// Frees retired blocks whose submits completed, and retires ranges not drawn for a while.
    pub(super) fn collect(&mut self, completed: u64, frame: u64) {
        if frame >= self.swept_frame + 60 {
            self.swept_frame = frame;
            let idle: Vec<(usize, usize)> =
                self.entries.iter().filter(|(_, e)| !e.dynamic && e.used_frame + IDLE_FRAMES < frame).map(|(k, _)| *k).collect();
            for k in idle {
                let e = self.entries.remove(&k).unwrap();
                self.retired.push((e.used, e.chunk, e.off, e.size));
            }
        }
        let (done, running): (Vec<_>, Vec<_>) = std::mem::take(&mut self.retired).into_iter().partition(|r| r.0 <= completed);
        self.retired = running;
        for (_, chunk, off, size) in done {
            self.release(chunk, off, size);
        }
    }
}

unsafe fn hash_range(&(a, len): &(usize, usize)) -> u64 {
    fast_hash(std::slice::from_raw_parts(a as *const u8, len))
}

/// Ranges for a hashing thread, which the sender waits for its hashes of.
struct HashJob(*const (usize, usize), usize);
unsafe impl Send for HashJob {}

type HashPool = std::sync::Mutex<Vec<(std::sync::mpsc::Sender<HashJob>, std::sync::mpsc::Receiver<Vec<u64>>)>>;

/// Threads hashing for verify (vk-hash): spawning them for each submit cost more than the hashing.
fn hash_pool() -> &'static HashPool {
    static POOL: std::sync::OnceLock<HashPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism().map_or(1, |n| n.get()).saturating_sub(2).clamp(1, 5);
        let workers = (0..n)
            .filter_map(|i| {
                let (tx, rx) = std::sync::mpsc::channel::<HashJob>();
                let (htx, hrx) = std::sync::mpsc::channel();
                std::thread::Builder::new()
                    .name(format!("vk-hash-{i}"))
                    .spawn(move || {
                        while let Ok(HashJob(p, n)) = rx.recv() {
                            let ranges = unsafe { std::slice::from_raw_parts(p, n) };
                            let _ = htx.send(ranges.iter().map(|r| unsafe { hash_range(r) }).collect());
                        }
                    })
                    .ok()
                    .map(|_| (tx, hrx))
            })
            .collect();
        std::sync::Mutex::new(workers)
    })
}

/// Change detection for small ranges, checked once per submit for every one drawn (several MB a frame): four lanes of
/// 64-bit multiplies folded from 128 bits, 64 bytes a step. Only compared with itself within a session.
fn fast_hash(b: &[u8]) -> u64 {
    const P: [u64; 4] = [0xa076_1d64_78bd_642f, 0xe703_7ed1_a0b4_28db, 0x8ebc_6af0_9c88_c6e3, 0x5899_65cc_7537_4cc3];
    let mix = |a: u64, b: u64| {
        let m = (a as u128) * (b as u128);
        (m as u64) ^ ((m >> 64) as u64)
    };
    let word = |c: &[u8], i: usize| u64::from_le_bytes(c[i * 8..i * 8 + 8].try_into().unwrap());
    let mut l = [P[0] ^ b.len() as u64, P[1], P[2], P[3]];
    let (body, rest) = b.split_at(b.len() / 64 * 64);
    for c in body.chunks_exact(64) {
        for (k, lane) in l.iter_mut().enumerate() {
            *lane = mix(word(c, 2 * k) ^ P[k], word(c, 2 * k + 1) ^ *lane);
        }
    }
    let mut h = mix(l[0] ^ l[2], l[1] ^ l[3] ^ P[0]);
    let (words, tail) = rest.split_at(rest.len() / 8 * 8);
    for w in words.chunks_exact(8) {
        h = mix(h ^ u64::from_le_bytes(w.try_into().unwrap()), P[1]);
    }
    let mut last = [0u8; 8];
    last[..tail.len()].copy_from_slice(tail);
    mix(h ^ u64::from_le_bytes(last), P[2] ^ tail.len() as u64)
}
