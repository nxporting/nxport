// Where the game's memory blocks would lie in a Switch's memory heap, for addresses given relative to the heap (the
// cheat VM's "heap" region: guest/cheats).
//
// On a Switch nn::os::SetMemoryHeapSize grows the process heap (svcSetHeapSize, which always places it at the start of
// the heap region), and nn::os::AllocateMemoryBlock carves blocks out of it, so a block's address is the heap region's
// start plus where the heap manager put it. Here both are HLE'd (horizon/hle.rs) and every block is its own host
// allocation, so "heap + offset" is translated through the layout the heap manager would have made: the same calls,
// replayed on a model of its free list. The model follows nn::os::detail::MemoryHeapManager as Atmosphère's
// libstratosphere implements it (os_memory_heap_manager.cpp): growing the heap adds its new end to the free list; an
// allocation takes the smallest free run that fits (the lowest-addressed one among equal sizes) from its start;
// a freed block goes back into the address-ordered list, merged with its neighbours; shrinking takes the tail off the
// last free run, and fails (nothing changes) when that run does not cover it.
//
// Offsets that fall in free space map to nothing here (on a Switch they are mapped heap memory holding the free list).

use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Block {
    offset: u64,
    size: u64,
    host: usize,
}

#[derive(Default)]
struct Layout {
    size: u64,
    /// Free runs (offset, size), in address order, never adjacent.
    free: Vec<(u64, u64)>,
    blocks: Vec<Block>,
}

impl Layout {
    fn set_size(&mut self, size: u64) {
        if size > self.size {
            let old = self.size;
            self.add_free(old, size - old);
            self.size = size;
        } else if size < self.size {
            let remove = self.size - size;
            match self.free.last_mut() {
                Some(last) if last.0 <= size && last.1 >= remove => {
                    last.1 -= remove;
                    if last.1 == 0 {
                        self.free.pop();
                    }
                    self.size = size;
                }
                // The heap manager refuses (ResultBusy): a block lies in the part that would go.
                _ => {}
            }
        }
    }

    /// Places a block of `size` bytes allocated at `host`; false if the heap has no room for it.
    fn allocate(&mut self, host: usize, size: u64) -> bool {
        let mut best: Option<usize> = None;
        for (i, &(_, n)) in self.free.iter().enumerate() {
            if n >= size && best.is_none_or(|b| n < self.free[b].1) {
                best = Some(i);
            }
        }
        let Some(i) = best else { return false };
        let (offset, n) = self.free[i];
        if n > size {
            self.free[i] = (offset + size, n - size);
        } else {
            self.free.remove(i);
        }
        self.blocks.push(Block { offset, size, host });
        true
    }

    fn release(&mut self, host: usize) {
        if let Some(i) = self.blocks.iter().position(|b| b.host == host) {
            let b = self.blocks.remove(i);
            self.add_free(b.offset, b.size);
        }
    }

    fn add_free(&mut self, offset: u64, size: u64) {
        if size == 0 {
            return;
        }
        let i = self.free.partition_point(|&(o, _)| o < offset);
        self.free.insert(i, (offset, size));
        // With the next run, then with the previous one.
        if i + 1 < self.free.len() && self.free[i].0 + self.free[i].1 == self.free[i + 1].0 {
            self.free[i].1 += self.free[i + 1].1;
            self.free.remove(i + 1);
        }
        if i > 0 && self.free[i - 1].0 + self.free[i - 1].1 == self.free[i].0 {
            self.free[i - 1].1 += self.free[i].1;
            self.free.remove(i);
        }
    }

    /// The host address of heap bytes [offset, offset + len), all in one block.
    fn host_address(&self, offset: u64, len: u64) -> Option<usize> {
        let b = self.blocks.iter().find(|b| offset >= b.offset && offset - b.offset < b.size)?;
        let end = offset.checked_add(len)?;
        (end <= b.offset + b.size).then(|| b.host + (offset - b.offset) as usize)
    }
}

static LAYOUT: Mutex<Layout> = Mutex::new(Layout { size: 0, free: Vec::new(), blocks: Vec::new() });

/// nn::os::SetMemoryHeapSize(size).
pub fn set_size(size: usize) {
    LAYOUT.lock().unwrap().set_size(size as u64);
}

/// nn::os::AllocateMemoryBlock gave the game `size` bytes at `host`.
pub fn allocated(host: usize, size: usize) {
    if !LAYOUT.lock().unwrap().allocate(host, size as u64) {
        println!(
            "[!] HLE: memory block {host:#x} ({} MB) does not fit in the memory heap a Switch would have; heap-relative \
             addresses (cheats) do not reach it",
            size >> 20
        );
    }
}

/// nn::os::FreeMemoryBlock released the block at `host`.
pub fn freed(host: usize) {
    LAYOUT.lock().unwrap().release(host);
}

/// Runs `access` on the host address of heap bytes [offset, offset + len), if a memory block holds them. The block stays
/// allocated meanwhile: FreeMemoryBlock forgets it here before it frees the memory.
pub fn with_host_address<R>(offset: u64, len: usize, access: impl FnOnce(usize) -> R) -> Option<R> {
    let layout = LAYOUT.lock().unwrap();
    layout.host_address(offset, len as u64).map(access)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1 << 20;

    #[test]
    fn first_block_at_the_heap_start() {
        let mut l = Layout::default();
        l.set_size(64 * MB);
        assert!(l.allocate(0x7000_0000, 32 * MB));
        assert_eq!(l.host_address(0x10, 4), Some(0x7000_0010));
        assert_eq!(l.host_address(32 * MB - 4, 4), Some(0x7000_0000 + 32 * MB as usize - 4));
        // Across the block's end, and in free space.
        assert_eq!(l.host_address(32 * MB - 2, 4), None);
        assert_eq!(l.host_address(40 * MB, 4), None);
    }

    #[test]
    fn best_fit_then_lowest() {
        let mut l = Layout::default();
        l.set_size(16 * MB);
        assert!(l.allocate(0xA000_0000, 4 * MB)); // 0..4
        assert!(l.allocate(0xB000_0000, 2 * MB)); // 4..6
        assert!(l.allocate(0xC000_0000, 4 * MB)); // 6..10
        l.release(0xA000_0000); // free: 0..4, 10..16
        // 2 MB fits both runs: the smaller one (0..4) wins over the larger, and the block goes at its start.
        assert!(l.allocate(0xD000_0000, 2 * MB));
        assert_eq!(l.host_address(0, 1), Some(0xD000_0000));
        // 5 MB fits only 10..16.
        assert!(l.allocate(0xE000_0000, 5 * MB));
        assert_eq!(l.host_address(10 * MB, 1), Some(0xE000_0000));
        assert!(!l.allocate(0xF000_0000, 4 * MB));
    }

    #[test]
    fn frees_merge_and_shrink_takes_the_tail() {
        let mut l = Layout::default();
        l.set_size(8 * MB);
        assert!(l.allocate(1 << 32, 2 * MB));
        assert!(l.allocate(2 << 32, 2 * MB));
        l.release(1 << 32);
        l.release(2 << 32);
        assert_eq!(l.free, vec![(0, 8 * MB)]);
        l.set_size(6 * MB);
        assert_eq!((l.size, l.free.clone()), (6 * MB, vec![(0, 6 * MB)]));
        // A block in the tail keeps the size.
        assert!(l.allocate(3 << 32, 6 * MB));
        l.set_size(2 * MB);
        assert_eq!(l.size, 6 * MB);
        // Growing adds the new end.
        l.set_size(10 * MB);
        assert!(l.allocate(4 << 32, 4 * MB));
        assert_eq!(l.host_address(6 * MB, 8), Some(4 << 32));
    }
}
