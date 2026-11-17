// nn::ui2d layouts as the profiles' HUD hooks read and change them (TotK's hud.rs, BotW's hud.rs): finding a pane by
// name, and a float field written over the game's value while keeping that value.

/// A pane's children: the list head (a child's node is at child + 8, its next pointer at node + 8).
const PANE_CHILDREN: usize = 0x20;

/// The NUL-terminated string at `p`, at most `max` bytes (empty for a null pointer).
pub(crate) unsafe fn c_str<'a>(p: usize, max: usize) -> &'a [u8] {
    if p == 0 {
        return &[];
    }
    let s = std::slice::from_raw_parts(p as *const u8, max);
    &s[..s.iter().position(|&c| c == 0).unwrap_or(max)]
}

/// The pane named `name` in the tree under `pane` (a pane's name is at `name_at` in it).
pub(crate) unsafe fn find_pane(pane: usize, name: &[u8], name_at: usize) -> Option<usize> {
    if c_str(pane + name_at, 24) == name {
        return Some(pane);
    }
    let head = pane + PANE_CHILDREN;
    let mut node = *((head + 8) as *const usize);
    while node != head && node != 0 {
        let child = node - 8;
        if let Some(p) = find_pane(child, name, name_at) {
            return Some(p);
        }
        node = *((node + 8) as *const usize);
    }
    None
}

/// A float a hook writes: the game's own value and what was last written over it.
pub(crate) struct Field {
    at: usize,
    base: f32,
    written: f32,
}

impl Field {
    pub(crate) unsafe fn new(at: usize) -> Field {
        let v = *(at as *const f32);
        Field {
            at,
            base: v,
            written: f32::NAN,
        }
    }

    /// Writes f(base); returns whether the value changed. A value the game or an animation wrote since becomes the new
    /// base.
    pub(crate) unsafe fn set(&mut self, f: impl Fn(f32) -> f32) -> bool {
        let cur = *(self.at as *const f32);
        if cur.to_bits() != self.written.to_bits() {
            self.base = cur;
        }
        let v = f(self.base);
        self.written = v;
        if v.to_bits() == cur.to_bits() {
            return false;
        }
        *(self.at as *mut f32) = v;
        true
    }
}
