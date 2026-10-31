// The game's per-frame GPU ring (main+0x96b068: allocates a uniform block, copies `size` bytes into it and returns the
// block object). Applying calls it. Predicting cannot: it initializes NVN buffer objects, and the game's own call
// right after would get another block. A predicted allocation is a placeholder block (a copy of a ring block object
// with a symbolic GPU address); the hook on the allocator logs the blocks the game's call then allocates on this
// thread, and the comparison maps each symbolic address onto the real block's and checks the copied bytes.

use std::cell::RefCell;

pub(super) const SITE: usize = 0x96b068;
pub(super) const SITE_INSTRUCTION: u32 = 0xd103c3ff; // sub sp, sp, #0xf0
/// main+0x960088(ring, data, size): the same allocation returning the block.
pub(super) const BLOCK_SITE: usize = 0x960088;
pub(super) const BLOCK_SITE_INSTRUCTION: u32 = 0xd10383ff; // sub sp, sp, #0xe0
pub(super) const RING_HOLDER: usize = 0x4634f48;
/// Symbolic GPU addresses: placeholder k at SYMBOLIC + k * SPAN.
const SYMBOLIC: u64 = 0x7e57_0000_0000;
const SPAN: u64 = 1 << 24;

struct Placeholder {
    obj: Box<[u8; 0x88]>,
    data: Vec<u8>,
}

thread_local! {
    static PREDICTED: RefCell<Vec<Placeholder>> = const { RefCell::new(Vec::new()) };
    /// Output cells of this thread's running allocator calls.
    static OUT: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    /// The blocks the game allocated on this thread while a prediction is checked.
    static ALLOCATED: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) };
}

/// A predicted allocation: `data` (`size` bytes) -> the placeholder block object.
pub(super) unsafe fn predict(base: usize, data: usize, size: usize) -> usize {
    let ring = *(((*((base + RING_HOLDER) as *const usize)) as *const usize));
    let template = *((ring + 0x50) as *const usize);
    let mut obj = Box::new([0u8; 0x88]);
    std::ptr::copy_nonoverlapping(template as *const u8, obj.as_mut_ptr(), 0x88);
    let data = if data == 0 { Vec::new() } else { std::slice::from_raw_parts(data as *const u8, size).to_vec() };
    PREDICTED.with(|p| {
        let mut p = p.borrow_mut();
        let addr = SYMBOLIC + p.len() as u64 * SPAN;
        obj[0x78..0x80].copy_from_slice(&addr.to_le_bytes());
        obj[0x18..0x20].copy_from_slice(&0u64.to_le_bytes());
        // The block's size (+0x38, again at +0x3c): the data size rounded up to 0x100 (ring offsets stay aligned).
        let rounded = ((size + 0xff) & !0xff) as u32;
        obj[0x38..0x3c].copy_from_slice(&rounded.to_le_bytes());
        obj[0x3c..0x40].copy_from_slice(&rounded.to_le_bytes());
        let a = obj.as_ptr() as usize;
        p.push(Placeholder { obj, data });
        a
    })
}

/// The predictions of this thread are dropped (a prediction stopped).
pub(super) fn discard() {
    PREDICTED.with(|p| p.borrow_mut().clear());
}

/// Starts logging the game's allocations on this thread (when a prediction made any).
pub(super) fn watch() {
    let any = PREDICTED.with(|p| !p.borrow().is_empty());
    ALLOCATED.with(|a| *a.borrow_mut() = any.then(Vec::new));
}

/// Resolves the predicted commands against the game's allocations: Ok(expected with real addresses), Err(reason).
pub(super) unsafe fn resolve(mut expected: Vec<String>) -> Result<Vec<String>, String> {
    let predicted = PREDICTED.with(|p| std::mem::take(&mut *p.borrow_mut()));
    let allocated = ALLOCATED.with(|a| a.borrow_mut().take());
    if predicted.is_empty() {
        return Ok(expected);
    }
    let allocated = allocated.unwrap_or_default();
    if allocated.len() != predicted.len() {
        return Err(format!("ring allocations: port {}, game {}", predicted.len(), allocated.len()));
    }
    for (k, (p, &real)) in predicted.iter().zip(&allocated).enumerate() {
        // The fields a binding reads (vtable, size, count, shift) are the template's: they must be the block's.
        let r = std::slice::from_raw_parts(real as *const u8, 0x88);
        for o in (0..8).chain(0x38..0x43) {
            if p.obj[o] != r[o] {
                return Err(format!("ring block {k}: byte {o:#x} port {:#x} game {:#x}", p.obj[o], r[o]));
            }
        }
        let cpu = *((real + 0x18) as *const usize);
        if !p.data.is_empty() && (cpu == 0 || std::slice::from_raw_parts(cpu as *const u8, p.data.len()) != p.data.as_slice()) {
            return Err(format!("ring block {k}: data differs"));
        }
    }
    let real: Vec<u64> = allocated.iter().map(|&b| *((b + 0x78) as *const u64)).collect();
    for s in expected.iter_mut() {
        *s = substitute(s, &real);
    }
    Ok(expected)
}

/// Every number in `s` (decimal or 0x hex) inside a symbolic range, replaced by the real address in the same notation.
fn substitute(s: &str, real: &[u64]) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let hex = b[i] == b'0' && b.get(i + 1) == Some(&b'x');
        let digits = if hex { i + 2 } else { i };
        let mut j = digits;
        while j < b.len() && (if hex { b[j].is_ascii_hexdigit() } else { b[j].is_ascii_digit() }) {
            j += 1;
        }
        let word_start = start == 0 || !b[start - 1].is_ascii_alphanumeric();
        if j > digits && word_start {
            let v = u64::from_str_radix(&s[digits..j], if hex { 16 } else { 10 }).unwrap_or(0);
            let k = v.wrapping_sub(SYMBOLIC) / SPAN;
            if v >= SYMBOLIC && (k as usize) < real.len() {
                let r = real[k as usize] + (v - SYMBOLIC - k * SPAN);
                out.push_str(&if hex { format!("{r:#x}") } else { r.to_string() });
                i = j;
                continue;
            }
            out.push_str(&s[start..j]);
            i = j;
            continue;
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

#[no_mangle]
pub unsafe extern "C" fn on_ring_alloc(out: usize) -> u32 {
    OUT.with(|o| o.borrow_mut().push(out));
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_ring_alloc_end() -> u32 {
    let Some(out) = OUT.with(|o| o.borrow_mut().pop()) else { return 0 };
    ALLOCATED.with(|a| {
        if let Some(a) = a.borrow_mut().as_mut() {
            a.push(*(out as *const usize));
        }
    });
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_ring_block() -> u32 {
    0
}

/// The block main+0x960088 returns (passed back as its return value).
#[no_mangle]
pub unsafe extern "C" fn on_ring_block_end(block: usize) -> usize {
    ALLOCATED.with(|a| {
        if let Some(a) = a.borrow_mut().as_mut() {
            a.push(block);
        }
    });
    block
}
