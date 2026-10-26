// Guest memory GPU work writes, kept in GPU memory while a submit uses it (shadows).
//
// Imported guest memory is host memory the GPU snoops (the driver gives imported host pointers HOST_CACHED memory),
// and shaders that write it often (atomics into small counters and histograms) spent most of a frame there: TotK's
// title screen on an RTX 4080 took 46 ms of GPU time per frame, 21 ms with those buffers elsewhere. So a range GPU work
// writes gets a slot in device-local memory the CPU can map (the BAR window where there is one), filled from guest
// memory, or on the GPU from the slot of a submit still running that wrote it last (queue order: that submit's writes
// come first), and copied back to guest memory once the submit completed. Other uses of the range in the same submit
// bind the slot, so they see those writes in GPU order as they would in place.

use std::collections::BTreeMap;

use ash::vk;

use super::device::{Dev, HostBuffer};

struct Slot {
    buffer: vk::Buffer,
    off: u64,
    ptr: *mut u8,
    len: usize,
    written: bool,
}

/// A chunk and the newest submit that uses it (one that wrote it, or copied from a slot in it): it is reused once that
/// submit completed.
struct Chunk {
    buf: HostBuffer,
    user: u64,
}

#[derive(Default)]
pub(crate) struct Shadows {
    /// This submit's chunks (the last is being filled) and how much of the last is used.
    chunks: Vec<HostBuffer>,
    used: u64,
    /// Chunks of earlier submits, until their last user completed; then free ones.
    retired: Vec<Chunk>,
    free: Vec<HostBuffer>,
    /// This submit's slots, by guest start.
    slots: BTreeMap<usize, Slot>,
    /// Slots earlier submits wrote, by guest start: (serial, buffer, offset, length); valid until that submit completed.
    last: BTreeMap<usize, (u64, vk::Buffer, u64, usize)>,
}

impl Shadows {
    const CHUNK: u64 = 4 << 20;
    /// Offsets of slots: every binding kind's alignment (storage and uniform offsets are at most 256 bytes on desktop).
    const ALIGN: u64 = 256;
    /// The largest range given a slot. The buffers that wanted one are small (TotK: 128 B to 58 KB of counters and
    /// histograms); storage bindings sized by the memory mapped after them run to 1 MiB, and a slot each, filled and
    /// copied back with every submit, made the in-flight watcher copy for a second per frame. Those stay in place.
    const MAX_SLOT: usize = 64 << 10;

    unsafe fn alloc(&mut self, dev: &Dev, len: usize) -> Option<(vk::Buffer, u64, *mut u8)> {
        let len = (len as u64).max(16);
        let off = self.used.next_multiple_of(Self::ALIGN);
        if let Some(c) = self.chunks.last() {
            if off + len <= c.size {
                self.used = off + len;
                return Some((c.buffer, off, c.ptr.add(off as usize)));
            }
        }
        let chunk = match self.free.iter().position(|c| c.size >= len) {
            Some(i) => self.free.swap_remove(i),
            None => {
                let usage = vk::BufferUsageFlags::UNIFORM_BUFFER
                    | vk::BufferUsageFlags::STORAGE_BUFFER
                    | vk::BufferUsageFlags::VERTEX_BUFFER
                    | vk::BufferUsageFlags::INDEX_BUFFER
                    | vk::BufferUsageFlags::TRANSFER_SRC
                    | vk::BufferUsageFlags::TRANSFER_DST;
                dev.mapped_device_buffer(Self::CHUNK.max(len), usage).ok()?
            }
        };
        let r = (chunk.buffer, 0, chunk.ptr);
        self.chunks.push(chunk);
        self.used = len;
        Some(r)
    }

    /// Whether [a, a + len) has a slot in this submit, or was written by a submit that has not completed.
    pub(super) fn shadowed(&self, a: usize, len: usize, completed: u64) -> bool {
        self.slot_overlaps(a, len) || self.last.range(..a + len).next_back().is_some_and(|(s, v)| v.0 > completed && s + v.3 > a)
    }

    fn slot_overlaps(&self, a: usize, len: usize) -> bool {
        self.slots.range(..a + len).next_back().is_some_and(|(s, v)| s + v.len > a)
    }

    /// The slot for [a, a + len) as a buffer range, made if this submit has none (`write`: GPU work writes it, so it is
    /// copied back). `upload` records a copy into the command buffer queued ahead of what is being recorded. None when
    /// the range covers only part of a slot (the caller binds guest memory instead).
    pub(super) unsafe fn get(
        &mut self,
        dev: &Dev,
        a: usize,
        len: usize,
        write: bool,
        serial: u64,
        completed: u64,
        upload: &mut dyn FnMut(vk::Buffer, u64, vk::Buffer, u64, u64),
    ) -> Option<(vk::Buffer, u64)> {
        if let Some((&s, slot)) = self.slots.range_mut(..=a).next_back() {
            if s + slot.len >= a + len {
                slot.written |= write;
                return Some((slot.buffer, slot.off + (a - s) as u64));
            }
        }
        if self.slot_overlaps(a, len) {
            return None;
        }
        if len > Self::MAX_SLOT && !self.last.range(..a + len).next_back().is_some_and(|(s, v)| s + v.3 > a) {
            return None;
        }
        // The range a running submit wrote last: its whole slot, copied on the GPU after that submit's writes.
        self.last.retain(|_, v| v.0 > completed);
        let prev = self.last.range(..=a).next_back().filter(|(s, v)| *s + v.3 >= a + len).map(|(&s, &v)| (s, v));
        if prev.is_none() && self.last.range(..a + len).next_back().is_some_and(|(s, v)| s + v.3 > a) {
            return None;
        }
        let (start, n) = prev.map_or((a, len), |(s, v)| (s, v.3));
        let (buffer, off, ptr) = self.alloc(dev, n)?;
        match prev {
            Some((_, (_, pbuf, poff, _))) => {
                for c in &mut self.retired {
                    if c.buf.buffer == pbuf {
                        c.user = c.user.max(serial);
                    }
                }
                upload(pbuf, poff, buffer, off, n as u64);
            }
            None => {
                std::ptr::copy_nonoverlapping(start as *const u8, ptr, n);
                super::guard::patch(start, ptr, n);
            }
        }
        self.slots.insert(start, Slot { buffer, off, ptr, len: n, written: write });
        Some((buffer, off + (a - start) as u64))
    }

    /// The submit `serial` was recorded: its written slots, to copy back to guest memory once it completed (guest
    /// address, source, length); later submits copy from them while it runs.
    pub(super) fn end_submit(&mut self, serial: u64) -> Vec<(usize, *const u8, usize)> {
        let mut writebacks = Vec::new();
        for (start, s) in std::mem::take(&mut self.slots) {
            if s.written {
                writebacks.push((start, s.ptr as *const u8, s.len));
                self.last.insert(start, (serial, s.buffer, s.off, s.len));
            }
        }
        for buf in self.chunks.drain(..) {
            self.retired.push(Chunk { buf, user: serial });
        }
        self.used = 0;
        writebacks
    }

    /// Chunks whose last user completed become free.
    pub(super) fn collect(&mut self, completed: u64) {
        let (done, running): (Vec<_>, Vec<_>) = std::mem::take(&mut self.retired).into_iter().partition(|c| c.user <= completed);
        self.retired = running;
        self.free.extend(done.into_iter().map(|c| c.buf));
    }
}
