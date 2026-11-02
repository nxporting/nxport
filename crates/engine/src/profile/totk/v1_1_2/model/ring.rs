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
    /// Allocated by code the game inlined (the hooks do not see it): resolved from the game's commands.
    inline: bool,
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
    predict_as(base, data, size, false)
}

/// A predicted allocation the game makes with an inlined copy of the allocator.
pub(super) unsafe fn predict_inline(base: usize, data: usize, size: usize) -> usize {
    predict_as(base, data, size, true)
}

/// predict_inline where only the first `known` bytes of the data are the game's (it copies uninitialised bytes after
/// them): only those are compared.
pub(super) unsafe fn predict_inline_known(base: usize, data: usize, known: usize, size: usize) -> usize {
    let a = predict_as(base, data, size, true);
    PREDICTED.with(|p| {
        if let Some(last) = p.borrow_mut().last_mut() {
            last.data.truncate(known);
        }
    });
    a
}

/// predict where only the first `known` bytes of the data are the game's (it copies uninitialised stack after them).
pub(super) unsafe fn predict_known(base: usize, data: usize, known: usize, size: usize) -> usize {
    let a = predict_as(base, data, size, false);
    PREDICTED.with(|p| {
        if let Some(last) = p.borrow_mut().last_mut() {
            last.data.truncate(known);
        }
    });
    a
}

unsafe fn predict_as(base: usize, data: usize, size: usize, inline: bool) -> usize {
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
        p.push(Placeholder { obj, data, inline });
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

/// Starts logging the game's allocations on this thread for a prediction made after the game's call.
pub(super) fn watch_all() {
    ALLOCATED.with(|a| *a.borrow_mut() = Some(Vec::new()));
}

/// Resolves the predicted commands against the game's allocations: Ok(expected with real addresses), Err(reason).
pub(super) unsafe fn resolve(expected: Vec<String>) -> Result<Vec<String>, String> {
    resolve_with(expected, &[])
}

/// As resolve; inline allocations are found in the game's commands `got` (the address at the same place of the same
/// command), their contents checked there.
pub(super) unsafe fn resolve_with(mut expected: Vec<String>, got: &[String]) -> Result<Vec<String>, String> {
    let predicted = PREDICTED.with(|p| std::mem::take(&mut *p.borrow_mut()));
    let allocated = ALLOCATED.with(|a| a.borrow_mut().take());
    if predicted.is_empty() {
        return Ok(expected);
    }
    let allocated = allocated.unwrap_or_default();
    let hooked: Vec<usize> = (0..predicted.len()).filter(|&k| !predicted[k].inline).collect();
    if allocated.len() != hooked.len() {
        return Err(format!("ring allocations: port {}, game {}", hooked.len(), allocated.len()));
    }
    let mut addrs = vec![0u64; predicted.len()];
    for (&k, &b) in hooked.iter().zip(&allocated) {
        addrs[k] = *((b + 0x78) as *const u64);
    }
    if hooked.len() != predicted.len() {
        inline_addresses(&predicted, &expected, got, &mut addrs)?;
    }
    for (&k, &real) in hooked.iter().zip(&allocated) {
        let p = &predicted[k];
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
    for s in expected.iter_mut() {
        *s = substitute(s, &addrs);
    }
    Ok(expected)
}

/// The numbers of `s` (decimal or 0x hex) in order.
fn numbers(s: &str) -> Vec<u64> {
    let b = s.as_bytes();
    let mut v = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let hex = b[i] == b'0' && b.get(i + 1) == Some(&b'x');
            let d = if hex { i + 2 } else { i };
            let mut j = d;
            while j < b.len() && (if hex { b[j].is_ascii_hexdigit() } else { b[j].is_ascii_digit() }) {
                j += 1;
            }
            v.push(u64::from_str_radix(&s[d..j], if hex { 16 } else { 10 }).unwrap_or(0));
            i = j;
        } else {
            i += 1;
        }
    }
    v
}

/// Addresses of inline allocations: the number of the game's command at the place of the symbolic one, with the
/// placeholder's data found at that address.
unsafe fn inline_addresses(predicted: &[Placeholder], expected: &[String], got: &[String], addrs: &mut [u64]) -> Result<(), String> {
    for (i, e) in expected.iter().enumerate() {
        let gn = got.get(i).map(|g| numbers(g)).unwrap_or_default();
        for (j, v) in numbers(e).into_iter().enumerate() {
            let k = v.wrapping_sub(SYMBOLIC) / SPAN;
            if v < SYMBOLIC || k as usize >= predicted.len() || !predicted[k as usize].inline || addrs[k as usize] != 0 {
                continue;
            }
            let Some(&r) = gn.get(j) else { return Err(format!("ring block {k}: no address in the game's command")) };
            let base = r.wrapping_sub(v - SYMBOLIC - k * SPAN);
            let p = &predicted[k as usize];
            let cpu = crate::nvn::api::translate_virtual(base as usize);
            if !p.data.is_empty() && (cpu == 0 || std::slice::from_raw_parts(cpu as *const u8, p.data.len()) != p.data.as_slice()) {
                return Err(format!("ring block {k} (inline): data differs"));
            }
            addrs[k as usize] = base;
        }
    }
    Ok(())
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
