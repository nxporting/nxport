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
            let fresh = e.checked == serial
                || match e.token {
                    Some(t) => crate::guest::dirty_track::generation(a, len) == Some(t),
                    None => super::texture::hash_bytes(std::slice::from_raw_parts(a as *const u8, len)) == e.hash,
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
        let hash = if tracked { 0 } else { super::texture::hash_bytes(src) };
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
