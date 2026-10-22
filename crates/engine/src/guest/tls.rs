// Guest thread-local storage: each guest thread's TPIDR_EL0 block (the ELF/musl thread pointer), built from the main
// module's TLS template, and its fake Horizon thread-local region. How a thread's guest code gets the block into
// TPIDR_EL0 depends on the CPU path: natively cpu::tls (Darwin TSD slots), lifted the thread's Cpu.

use std::cell::Cell;

const TLS_BLOCK_SIZE: usize = 0x4000; // empirically observed max offset 0x1000, generous margin
/// Offset in the guest TLS block of the pointer to the thread's fake Horizon thread-local region.
pub const FAKE_TLR_SLOT: usize = 0x3ff8;

thread_local! {
    pub static CURRENT_GUEST_TLS_BLOCK: Cell<usize> = const { Cell::new(0) };
    static GUEST_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// The current thread's fake Horizon thread-local region (IPC buffer), or 0 before it has a block.
pub fn current_fake_tlr() -> usize {
    let block = CURRENT_GUEST_TLS_BLOCK.with(|c| c.get());
    if block == 0 {
        0
    } else {
        unsafe { *((block + FAKE_TLR_SLOT) as *const usize) }
    }
}

/// The main module's TLS initialization image (.tdata) and its offset from the thread pointer (AArch64 variant I: the
/// 16-byte TCB, rounded up to the segment's alignment), set by __nnmusl_init_dso (horizon/hle.rs).
static TLS_TEMPLATE: std::sync::Mutex<(Vec<u8>, usize)> = std::sync::Mutex::new((Vec::new(), 16));

pub fn set_module_tls_template(image: Vec<u8>, align: usize) {
    let off = (16 + align.max(1) - 1) / align.max(1) * align.max(1);
    let mut t = TLS_TEMPLATE.lock().unwrap();
    *t = (image, off);
    // Blocks made before the module registered (the loader thread's) get it too.
    let block = CURRENT_GUEST_TLS_BLOCK.with(|c| c.get());
    if block != 0 {
        unsafe { apply_template(block, &t) };
    }
}

unsafe fn apply_template(block: usize, t: &(Vec<u8>, usize)) {
    if !t.0.is_empty() && t.1 + t.0.len() <= 0x1f0 {
        std::ptr::copy_nonoverlapping(t.0.as_ptr(), (block + t.1) as *mut u8, t.0.len());
    }
}

/// The calling thread's guest TLS block, made on first use: zeroed, its self pointer at +0x1f0, the module's template
/// applied. Leaked intentionally: it lives as long as the OS thread, which for this engine is the process.
pub fn thread_block() -> usize {
    let block = CURRENT_GUEST_TLS_BLOCK.with(|c| c.get());
    if block != 0 {
        return block;
    }
    unsafe {
        let layout = std::alloc::Layout::from_size_align(TLS_BLOCK_SIZE, 0x1000).unwrap();
        let ptr = std::alloc::alloc_zeroed(layout);
        *(ptr.add(0x1f0) as *mut usize) = ptr as usize;
        // A Horizon thread-local region (0x200 bytes, IPC message buffer at +0) for modules that read tpidrro_el0
        // themselves (exlaunch mods; see guest/module_loader.rs): on macOS tpidrro_el0 is Darwin's TSD, which must not be
        // written. Those reads are rewritten to load this pointer.
        let tlr = std::alloc::alloc_zeroed(std::alloc::Layout::from_size_align(0x200, 0x200).unwrap());
        *(ptr.add(FAKE_TLR_SLOT) as *mut usize) = tlr as usize;
        apply_template(ptr as usize, &TLS_TEMPLATE.lock().unwrap());
        CURRENT_GUEST_TLS_BLOCK.with(|c| c.set(ptr as usize));
        ptr as usize
    }
}

/// Called first on every thread that runs guest code (guest-main and the threads the guest creates).
pub fn mark_guest_thread() {
    GUEST_THREAD.with(|g| g.set(true));
}

/// Whether this thread runs guest code. Signal-safe.
pub fn is_guest_thread() -> bool {
    GUEST_THREAD.with(|g| g.get())
}
