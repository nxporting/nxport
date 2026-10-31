// Guest memory as the port sees it. Predicting (verify mode) keeps every write in an overlay so the game's own run
// afterwards starts from the same state; applying writes guest memory, keeping the old bytes when the run may still be
// undone (rollback).

use rustc_hash::FxHashMap as HashMap;

pub(super) struct Mem {
    /// Predicting: written bytes by 8-byte word (word address -> (bytes, mask of the written bytes)).
    overlay: Option<HashMap<usize, (u64, u64)>>,
    /// Host buffers the port builds and hands to game code (start, length): written directly, never overlaid.
    owned: Vec<(usize, usize)>,
    /// Applying undoably: (address, length, old bytes) of every write, in order.
    undo: Option<Vec<(usize, u8, u64)>>,
}

impl Mem {
    pub(super) fn predicting() -> Self {
        Mem { overlay: Some(HashMap::default()), owned: Vec::new(), undo: None }
    }

    pub(super) fn applying() -> Self {
        Mem { overlay: None, owned: Vec::new(), undo: None }
    }

    /// Applying, with every write undoable until rollback: the run stops where predicting would (calls into game
    /// code that records commands or keeps state are left to the game's own run).
    pub(super) fn applying_undoably() -> Self {
        Mem { overlay: None, owned: Vec::new(), undo: Some(Vec::new()) }
    }

    pub(super) fn predict(&self) -> bool {
        self.overlay.is_some()
    }

    /// Does the run stop at game code the port does not run (predicting, or applying undoably)?
    pub(super) fn stops(&self) -> bool {
        self.overlay.is_some() || self.undo.is_some()
    }

    /// Restores every byte written since applying_undoably, newest first.
    pub(super) unsafe fn rollback(&mut self) {
        if let Some(u) = self.undo.take() {
            for &(a, n, old) in u.iter().rev() {
                std::ptr::copy_nonoverlapping(&old as *const u64 as *const u8, a as *mut u8, n as usize);
            }
        }
    }

    pub(super) unsafe fn r<T: Copy>(&self, a: usize) -> T {
        let v = std::ptr::read_unaligned(a as *const T);
        let Some(o) = &self.overlay else { return v };
        if o.is_empty() {
            return v;
        }
        let mut v = v;
        let p = &mut v as *mut T as *mut u8;
        let n = std::mem::size_of::<T>();
        let mut word = a & !7;
        while word < a + n {
            if let Some(&(bytes, mask)) = o.get(&word) {
                for k in a.max(word)..(a + n).min(word + 8) {
                    let sh = (k - word) * 8;
                    if (mask >> sh) & 0xff != 0 {
                        *p.add(k - a) = (bytes >> sh) as u8;
                    }
                }
            }
            word += 8;
        }
        v
    }

    pub(super) unsafe fn w<T: Copy>(&mut self, a: usize, v: T) {
        if self.owned.iter().any(|&(s, n)| a >= s && a < s + n) {
            return std::ptr::write_unaligned(a as *mut T, v);
        }
        match &mut self.overlay {
            None => {
                if let Some(u) = &mut self.undo {
                    let mut old = 0u64;
                    std::ptr::copy_nonoverlapping(a as *const u8, &mut old as *mut u64 as *mut u8, std::mem::size_of::<T>());
                    u.push((a, std::mem::size_of::<T>() as u8, old));
                }
                std::ptr::write_unaligned(a as *mut T, v)
            }
            Some(o) => {
                let p = &v as *const T as *const u8;
                let n = std::mem::size_of::<T>();
                let mut word = a & !7;
                while word < a + n {
                    let e = o.entry(word).or_insert((0, 0));
                    for k in a.max(word)..(a + n).min(word + 8) {
                        let sh = (k - word) * 8;
                        e.0 = (e.0 & !(0xff << sh)) | (*p.add(k - a) as u64) << sh;
                        e.1 |= 0xff << sh;
                    }
                    word += 8;
                }
            }
        }
    }

    /// A host buffer of the port's (`a..a + len`) that game code may read: written directly from now on (game code
    /// called with it sees what the port wrote).
    pub(super) fn own(&mut self, a: usize, len: usize) {
        self.forget(a, len);
        self.owned.push((a, len));
    }

    /// The end of a buffer given to `own`.
    pub(super) fn disown(&mut self, a: usize) {
        self.owned.retain(|e| e.0 != a);
    }

    fn forget(&mut self, a: usize, len: usize) {
        if let Some(o) = &mut self.overlay {
            if !o.is_empty() {
                for k in a..a + len {
                    let (word, sh) = (k & !7, (k & 7) * 8);
                    if let Some(e) = o.get_mut(&word) {
                        e.1 &= !(0xff << sh);
                        if e.1 == 0 {
                            o.remove(&word);
                        }
                    }
                }
            }
        }
    }

    pub(super) unsafe fn r8(&self, a: usize) -> u8 {
        self.r(a)
    }
    pub(super) unsafe fn i8(&self, a: usize) -> i8 {
        self.r(a)
    }
    pub(super) unsafe fn r16(&self, a: usize) -> u16 {
        self.r(a)
    }
    pub(super) unsafe fn r32(&self, a: usize) -> u32 {
        self.r(a)
    }
    pub(super) unsafe fn i32(&self, a: usize) -> i32 {
        self.r(a)
    }
    pub(super) unsafe fn r64(&self, a: usize) -> usize {
        self.r(a)
    }
    pub(super) unsafe fn f32(&self, a: usize) -> f32 {
        self.r(a)
    }

    /// The game's exclusive-monitor `*a |= bits`, returning the old value.
    pub(super) unsafe fn fetch_or32(&mut self, a: usize, bits: u32) -> u32 {
        match &self.overlay {
            None => {
                let old = (*(a as *const std::sync::atomic::AtomicU32)).fetch_or(bits, std::sync::atomic::Ordering::AcqRel);
                if let Some(u) = &mut self.undo {
                    u.push((a, 4, old as u64));
                }
                old
            }
            Some(_) => {
                let old = self.r32(a);
                self.w(a, old | bits);
                old
            }
        }
    }
}
