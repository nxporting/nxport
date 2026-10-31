// Guest memory as the port sees it. Predicting (verify mode) keeps every write in an overlay so the game's own run
// afterwards starts from the same state; applying writes guest memory.

use rustc_hash::FxHashMap as HashMap;

pub(super) struct Mem {
    overlay: Option<HashMap<usize, u8>>,
    /// Host buffers the port builds and hands to game code (start, length): written directly, never overlaid.
    owned: Vec<(usize, usize)>,
}

impl Mem {
    pub(super) fn predicting() -> Self {
        Mem { overlay: Some(HashMap::default()), owned: Vec::new() }
    }

    pub(super) fn applying() -> Self {
        Mem { overlay: None, owned: Vec::new() }
    }

    pub(super) fn predict(&self) -> bool {
        self.overlay.is_some()
    }

    pub(super) unsafe fn r<T: Copy>(&self, a: usize) -> T {
        let v = std::ptr::read_unaligned(a as *const T);
        let Some(o) = &self.overlay else { return v };
        if o.is_empty() {
            return v;
        }
        let mut v = v;
        let p = &mut v as *mut T as *mut u8;
        for k in 0..std::mem::size_of::<T>() {
            if let Some(b) = o.get(&(a + k)) {
                *p.add(k) = *b;
            }
        }
        v
    }

    pub(super) unsafe fn w<T: Copy>(&mut self, a: usize, v: T) {
        if self.owned.iter().any(|&(s, n)| a >= s && a < s + n) {
            return std::ptr::write_unaligned(a as *mut T, v);
        }
        match &mut self.overlay {
            None => std::ptr::write_unaligned(a as *mut T, v),
            Some(o) => {
                let p = &v as *const T as *const u8;
                for k in 0..std::mem::size_of::<T>() {
                    o.insert(a + k, *p.add(k));
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
                    o.remove(&k);
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
            None => (*(a as *const std::sync::atomic::AtomicU32)).fetch_or(bits, std::sync::atomic::Ordering::AcqRel),
            Some(_) => {
                let old = self.r32(a);
                self.w(a, old | bits);
                old
            }
        }
    }
}
