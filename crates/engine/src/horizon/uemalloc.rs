// Unreal Engine FMalloc host allocator hook.
//
// Unreal Engine's FMallocBinned3 attempts to reserve 37 GB of address space up front (74 pools of 512 MB).
// When restricted to Switch memory or when running on hosts without extended virtual addressing entitlements,
// it panics and aborts.
// We replace the allocator's vtable with host allocator callbacks (malloc, free, realloc, quantize) backed by
// host posix_memalign / malloc_size.

use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

const NSLOT: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UeLayout {
    Ue5,
    Ue4_27,
    Ue5_6,
}

struct LayoutSlots {
    nslot: usize,
    malloc_: usize,
    try_malloc: usize,
    realloc_: usize,
    try_realloc: usize,
    free_: usize,
    quantize: usize,
    get_size: usize,
    nothing: &'static [usize],
}

const LAYOUTS: [LayoutSlots; 3] = [
    // Ue5
    LayoutSlots {
        nslot: 26,
        malloc_: 6,
        try_malloc: 7,
        realloc_: 8,
        try_realloc: 9,
        free_: 10,
        quantize: 11,
        get_size: 12,
        nothing: &[13, 14, 15, 19],
    },
    // Ue4_27
    LayoutSlots {
        nslot: 21,
        malloc_: 3,
        try_malloc: 4,
        realloc_: 5,
        try_realloc: 6,
        free_: 7,
        quantize: 8,
        get_size: 9,
        nothing: &[10, 11, 12, 16],
    },
    // Ue5_6
    LayoutSlots {
        nslot: 32,
        malloc_: 6,
        try_malloc: 7,
        realloc_: 8,
        try_realloc: 9,
        free_: 10,
        quantize: 13,
        get_size: 14,
        nothing: &[15, 16, 17, 18, 19, 23],
    },
];

struct UeTarget {
    build_id: &'static str,
    name: &'static str,
    layout: UeLayout,
    obj_offset: usize,
    vtable_offset: usize,
    cut_offset: usize,
}

const TARGETS: &[UeTarget] = &[
    // Octopath Traveler 0 1.0.5
    UeTarget {
        build_id: "9FE60A212CFF9B9A3656AB63D4F882E5",
        name: "Octopath Traveler 0 1.0.5",
        layout: UeLayout::Ue5,
        obj_offset: 0x68022c8,
        vtable_offset: 0x5b06f38,
        cut_offset: 0x12e40,
    },
    // DRAGON QUEST VII Reimagined 1.2.0
    UeTarget {
        build_id: "F7EC03E4CB9DE3BD8319D4E899D2E9CC",
        name: "DRAGON QUEST VII Reimagined 1.2.0",
        layout: UeLayout::Ue4_27,
        obj_offset: 0,
        vtable_offset: 0x5e4c0e8,
        cut_offset: 0x32758dc,
    },
    // Pikmin 4 1.1.1
    UeTarget {
        build_id: "060F827BD2B38703E9EA11E3210D89BC",
        name: "Pikmin 4 1.1.1",
        layout: UeLayout::Ue4_27,
        obj_offset: 0,
        vtable_offset: 0x8da4758,
        cut_offset: 0x353f788,
    },
    // OCTOPATH TRAVELER II 1.1.1
    UeTarget {
        build_id: "0D9649011312F62E93F6A5673A09D7E9",
        name: "OCTOPATH TRAVELER II 1.1.1",
        layout: UeLayout::Ue4_27,
        obj_offset: 0,
        vtable_offset: 0x64710b0,
        cut_offset: 0xbecc08,
    },
    // FINAL FANTASY RESONANCE 1.0.1
    UeTarget {
        build_id: "84E182F72EE1DA4F477804BD05B54C15",
        name: "FINAL FANTASY RESONANCE 1.0.1",
        layout: UeLayout::Ue5_6,
        obj_offset: 0x77c6210,
        vtable_offset: 0x67df800,
        cut_offset: 0x137f0,
    },
];

static mut HOST_VTABLE: [*const c_void; NSLOT] = [std::ptr::null(); NSLOT];
static USED_BYTES: AtomicI64 = AtomicI64::new(0);
static PEAK_BYTES: AtomicI64 = AtomicI64::new(0);
static IS_ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn track_alloc(size: usize) {
    let s = size as i64;
    let after = USED_BYTES.fetch_add(s, Ordering::Relaxed) + s;
    let mut peak = PEAK_BYTES.load(Ordering::Relaxed);
    while after > peak {
        match PEAK_BYTES.compare_exchange_weak(peak, after, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(p) => peak = p,
        }
    }
}

fn track_free(size: usize) {
    USED_BYTES.fetch_sub(size as i64, Ordering::Relaxed);
}

fn align_up(a: u32) -> usize {
    if a > 16 {
        a as usize
    } else {
        16
    }
}

unsafe fn host_alloc(n: usize, alignment: u32) -> *mut c_void {
    let n = if n == 0 { 1 } else { n };
    let align = align_up(alignment);
    let mut ptr: *mut c_void = std::ptr::null_mut();
    #[cfg(unix)]
    {
        let ret = libc::posix_memalign(&mut ptr, align, n);
        if ret != 0 || ptr.is_null() {
            return std::ptr::null_mut();
        }
        #[cfg(target_os = "macos")]
        track_alloc(libc::malloc_size(ptr));
        #[cfg(not(target_os = "macos"))]
        track_alloc(n);
    }
    #[cfg(windows)]
    {
        extern "C" {
            fn _aligned_malloc(size: usize, alignment: usize) -> *mut c_void;
        }
        ptr = _aligned_malloc(n, align);
        if !ptr.is_null() {
            track_alloc(n);
        }
    }
    ptr
}

unsafe fn host_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    #[cfg(unix)]
    {
        #[cfg(target_os = "macos")]
        track_free(libc::malloc_size(ptr));
        #[cfg(not(target_os = "macos"))]
        track_free(0);
        libc::free(ptr);
    }
    #[cfg(windows)]
    {
        extern "C" {
            fn _aligned_free(ptr: *mut c_void);
        }
        _aligned_free(ptr);
    }
}

unsafe fn host_size(ptr: *mut c_void) -> usize {
    if ptr.is_null() {
        return 0;
    }
    #[cfg(target_os = "macos")]
    {
        libc::malloc_size(ptr)
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}

unsafe fn host_realloc(p: *mut c_void, n: usize, alignment: u32) -> *mut c_void {
    if p.is_null() {
        return host_alloc(n, alignment);
    }
    if n == 0 {
        host_free(p);
        return std::ptr::null_mut();
    }
    let old = host_size(p);
    let align = align_up(alignment);
    if (p as usize) & (align - 1) == 0 && old >= n {
        return p;
    }
    let q = host_alloc(n, alignment);
    if q.is_null() {
        return std::ptr::null_mut();
    }
    let copy_len = if old != 0 && old < n { old } else { n };
    std::ptr::copy_nonoverlapping(p as *const u8, q as *mut u8, copy_len);
    host_free(p);
    q
}

// ---- Exported C functions matching the FMalloc vtable ----

unsafe extern "C" fn m_malloc(_self: *mut c_void, n: usize, alignment: u32) -> *mut c_void {
    let p = host_alloc(n, alignment);
    if p.is_null() && n != 0 {
        eprintln!("[!] Unreal heap out of memory: {} bytes (align {})", n, alignment);
    }
    p
}

unsafe extern "C" fn m_try_malloc(_self: *mut c_void, n: usize, alignment: u32) -> *mut c_void {
    host_alloc(n, alignment)
}

unsafe extern "C" fn m_realloc(_self: *mut c_void, p: *mut c_void, n: usize, alignment: u32) -> *mut c_void {
    let q = host_realloc(p, n, alignment);
    if q.is_null() && n != 0 {
        eprintln!("[!] Unreal heap out of memory in Realloc: {} bytes", n);
    }
    q
}

unsafe extern "C" fn m_try_realloc(_self: *mut c_void, p: *mut c_void, n: usize, alignment: u32) -> *mut c_void {
    host_realloc(p, n, alignment)
}

unsafe extern "C" fn m_free(_self: *mut c_void, p: *mut c_void) {
    host_free(p);
}

unsafe extern "C" fn m_quantize_size(_self: *mut c_void, n: usize, _alignment: u32) -> usize {
    #[cfg(target_os = "macos")]
    {
        libc::malloc_good_size(n)
    }
    #[cfg(not(target_os = "macos"))]
    {
        (n + 15) & !15
    }
}

unsafe extern "C" fn m_get_allocation_size(_self: *mut c_void, p: *mut c_void, out: *mut usize) -> bool {
    if p.is_null() || out.is_null() {
        return false;
    }
    *out = host_size(p);
    true
}

unsafe extern "C" fn m_nothing(_self: *mut c_void) {}

/// Checks whether the loaded main module matches a known Unreal Engine title and installs the host allocator.
pub unsafe fn install(main_base: usize, main_build_id: &str) {
    let id_upper = main_build_id.trim().to_ascii_uppercase();
    for target in TARGETS {
        if id_upper.starts_with(target.build_id) {
            let layout_idx = match target.layout {
                UeLayout::Ue5 => 0,
                UeLayout::Ue4_27 => 1,
                UeLayout::Ue5_6 => 2,
            };
            let lay = &LAYOUTS[layout_idx];

            // 1. Cut the FMallocBinned3 constructor (bl -> nop 0xd503201f)
            let cut_addr = (main_base + target.cut_offset) as *mut u32;
            let original_insn = *cut_addr;
            *cut_addr = 0xd503_201f; // NOP
            crate::os::flush_icache(cut_addr as usize, 4);

            // 2. Clone the original vtable into our host vtable
            let src_vt = (main_base + target.vtable_offset) as *const *const c_void;
            for i in 0..lay.nslot.min(NSLOT) {
                HOST_VTABLE[i] = *src_vt.add(i);
            }

            // 3. Populate host allocator function pointers
            HOST_VTABLE[lay.malloc_] = m_malloc as *const c_void;
            HOST_VTABLE[lay.try_malloc] = m_try_malloc as *const c_void;
            HOST_VTABLE[lay.realloc_] = m_realloc as *const c_void;
            HOST_VTABLE[lay.try_realloc] = m_try_realloc as *const c_void;
            HOST_VTABLE[lay.free_] = m_free as *const c_void;
            HOST_VTABLE[lay.quantize] = m_quantize_size as *const c_void;
            HOST_VTABLE[lay.get_size] = m_get_allocation_size as *const c_void;
            for &slot in lay.nothing {
                if slot < NSLOT {
                    HOST_VTABLE[slot] = m_nothing as *const c_void;
                }
            }

            // 4. If static object in .bss, install the vtable immediately
            if target.obj_offset != 0 {
                let obj_ptr = (main_base + target.obj_offset) as *mut *const c_void;
                *obj_ptr = std::ptr::addr_of!(HOST_VTABLE) as *const c_void;
                println!(
                    "[+] Unreal Engine: {} ({}) allocator (FMallocBinned3 at main+{:#x}) redirected to host allocator",
                    target.name, target.build_id, target.obj_offset
                );
            } else {
                println!(
                    "[+] Unreal Engine: {} ({}) cut constructor {:#x} (was {:#x}) for dynamic allocator",
                    target.name, target.build_id, target.cut_offset, original_insn
                );
            }

            IS_ACTIVE.store(1, Ordering::Release);
            return;
        }
    }
}

/// Returns true if Unreal Engine host malloc has been installed.
pub fn is_active() -> bool {
    IS_ACTIVE.load(Ordering::Acquire) != 0
}
