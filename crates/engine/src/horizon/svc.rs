// Horizon supervisor calls for code that issues `svc` itself. The game and SDK never do (their kernel-facing
// functions are HLE'd), but exlaunch modules (UltraCam and other subsdk mods) carry their own libnx-style runtime.
// guest/module_loader.rs rewrites each `svc #n` in such a module into a branch to a stub that saves x0-x7 and calls
// `svc_dispatch(regs, n)` through an HLE trampoline; results go back into the saved registers (Horizon ABI: result
// code in w0, outputs in x1..).
//
// Implemented: what exlaunch needs to find the game's modules and hook them, start a thread and run the IPC trick
// that yields its own process handle:
//   0x06 QueryMemory      a module map of the loaded guest modules (rtld placeholder, main, mods, sdk) plus the alias
//                         region; everything else reads as free
//   0x29 GetInfo          region addresses/sizes, program id
//   0x74 MapProcessMemory a writable alias of guest code (mach_vm_remap), 0x75 UnmapProcessMemory
//   0x08 CreateThread / 0x09 StartThread / 0x0A ExitThread / 0x18 WaitSynchronization (threads only)
//   0x40 CreateSession / 0x21 SendSyncRequest / 0x43 ReplyAndReceive between guest threads, with handle copying
//   0x16 CloseHandle, 0x1E GetSystemTick
// --log-svc logs every call.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

const RESULT_OK: u64 = 0;
const RESULT_INVALID_HANDLE: u64 = 0xE401;
const RESULT_TIMEOUT: u64 = 0xEA01;
const RESULT_NOT_IMPLEMENTED: u64 = 0xF001;
const RESULT_INVALID_ADDRESS: u64 = 0xCC01;
const RESULT_SESSION_CLOSED: u64 = 0xF601;

const PSEUDO_CURRENT_THREAD: u32 = 0xFFFF_8000;
const PSEUDO_CURRENT_PROCESS: u32 = 0xFFFF_8001;

/// Horizon memory types / permissions reported by QueryMemory.
pub const MEM_FREE: u32 = 0;
pub const MEM_CODE_STATIC: u32 = 3;
pub const MEM_CODE_MUTABLE: u32 = 4;
const MEM_ALIAS_CODE: u32 = 0x15;
const MEM_INACCESSIBLE: u32 = 0x10;
pub const PERM_R: u32 = 1;
pub const PERM_RW: u32 = 3;
pub const PERM_RX: u32 = 5;

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub start: usize,
    pub size: usize,
    pub mem_type: u32,
    pub perm: u32,
}

static REGIONS: Mutex<Vec<Region>> = Mutex::new(Vec::new());
/// Engine code mods may hook but QueryMemory does not list (an extra code region would shift exlaunch's module
/// numbering): the HLE trampoline pool, which symbols resolved to HLE functions point into.
static HOOKABLE: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

pub fn add_hookable(start: usize, size: usize) {
    HOOKABLE.lock().unwrap().push((start, size));
}
/// (start, size) of the reserved alias region (MapProcessMemory destinations).
static ALIAS: Mutex<(usize, usize)> = Mutex::new((0, 0));
/// (start, size) of the guest heap arena, reported as the heap region.
static HEAP: Mutex<(usize, usize)> = Mutex::new((0, 0));

/// Registers a loaded module's segments (or any region) for QueryMemory.
pub fn add_region(r: Region) {
    let mut v = REGIONS.lock().unwrap();
    v.retain(|x| x.start + x.size <= r.start || x.start >= r.start + r.size);
    v.push(r);
    v.sort_by_key(|x| x.start);
}

/// Reserves the alias region once (address space only).
fn alias_region() -> (usize, usize) {
    let mut a = ALIAS.lock().unwrap();
    if a.1 == 0 {
        const SIZE: usize = 0x1_0000_0000;
        if let Ok(p) = unsafe { crate::os::reserve(crate::os::At::Anywhere, SIZE) } {
            *a = (p, SIZE);
        }
    }
    *a
}

/// What GetInfo reports as the alias region: a separate small reservation (exlaunch picks mapping addresses in the
/// ASLR region outside the alias region, so the two must differ).
fn small_alias_region() -> (usize, usize) {
    static R: std::sync::OnceLock<(usize, usize)> = std::sync::OnceLock::new();
    *R.get_or_init(|| {
        const SIZE: usize = 0x400_0000;
        unsafe { crate::os::reserve(crate::os::At::Anywhere, SIZE) }.map_or((0, 0), |p| (p, SIZE))
    })
}

fn log_enabled() -> bool {
    static L: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *L.get_or_init(|| crate::settings::get().log.log_svc)
}

// ── handles ──────────────────────────────────────────────────────────────────────────────────────

struct ThreadObj {
    entry: usize,
    arg: usize,
    done: Arc<(Mutex<bool>, Condvar)>,
}

#[derive(Default)]
struct SessionState {
    request: Option<Vec<u8>>,
    reply: Option<Vec<u8>>,
    /// The server end was closed: pending and future requests fail.
    closed: bool,
}

struct Session {
    state: Mutex<SessionState>,
    cv: Condvar,
}

enum Handle {
    Thread(ThreadObj),
    SessionServer(Arc<Session>),
    SessionClient(Arc<Session>),
    Process,
}

static HANDLES: Mutex<Option<HashMap<u32, Handle>>> = Mutex::new(None);
static NEXT_HANDLE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x100);

fn new_handle(h: Handle) -> u32 {
    let id = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    HANDLES
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(id, h);
    id
}

fn session_of(handle: u32, server: bool) -> Option<Arc<Session>> {
    let g = HANDLES.lock().unwrap();
    match g.as_ref()?.get(&handle)? {
        Handle::SessionServer(s) if server => Some(s.clone()),
        Handle::SessionClient(s) if !server => Some(s.clone()),
        _ => None,
    }
}

/// The calling thread's IPC message buffer (its fake Horizon thread-local region).
unsafe fn ipc_buffer() -> Option<&'static mut [u8]> {
    let tlr = crate::guest::tls::current_fake_tlr();
    (tlr != 0).then(|| std::slice::from_raw_parts_mut(tlr as *mut u8, 0x100))
}

/// Copies a request out of the sender's buffer, translating copied/moved handles as the kernel does (the pseudo
/// handle of the current process becomes a real process handle in the receiver).
fn translate_request(buf: &[u8]) -> Vec<u8> {
    let mut msg = buf.to_vec();
    let word = |m: &[u8], i: usize| u32::from_le_bytes(m[i * 4..i * 4 + 4].try_into().unwrap());
    let w1 = word(&msg, 1);
    if w1 >> 31 != 0 {
        let hd = word(&msg, 2);
        let send_pid = hd & 1 != 0;
        let count = ((hd >> 1) & 0xF) + ((hd >> 5) & 0xF);
        let first = 3 + if send_pid { 2 } else { 0 };
        for i in 0..count as usize {
            let idx = first + i;
            if idx * 4 + 4 > msg.len() {
                break;
            }
            let h = word(&msg, idx);
            let t = if h == PSEUDO_CURRENT_PROCESS {
                new_handle(Handle::Process)
            } else {
                h
            };
            msg[idx * 4..idx * 4 + 4].copy_from_slice(&t.to_le_bytes());
        }
    }
    msg
}

// ── dispatch ─────────────────────────────────────────────────────────────────────────────────────

/// Called from the per-site stubs with the guest's x0-x7 (written back on return) and the svc number.
pub unsafe extern "C" fn svc_dispatch(regs: *mut [u64; 8], n: u64) {
    let r = &mut *regs;
    // Writes into copy-mode code views (persistent ones too: exlaunch keeps its trampoline area mapped) reach the code
    // at the mod's next syscall. Installing a hook always maps/unmaps the target page after writing the trampoline, so
    // trampolines are in place before the branch to them is.
    if matches!(n, 0x74 | 0x75) {
        flush_code_views();
    }
    if log_enabled() {
        println!(
            "[svc] {:#04x} x0={:#x} x1={:#x} x2={:#x} x3={:#x} x4={:#x}",
            n, r[0], r[1], r[2], r[3], r[4]
        );
    }
    let result = match n {
        0x06 => query_memory(r),
        0x29 => get_info(r),
        0x74 => map_process_memory(r),
        0x75 => unmap_process_memory(r),
        0x08 => create_thread(r),
        0x09 => start_thread(r),
        0x0A => {
            exit_current_thread();
            RESULT_OK
        }
        0x16 => {
            let removed = HANDLES
                .lock()
                .unwrap()
                .as_mut()
                .and_then(|m| m.remove(&(r[0] as u32)));
            if let Some(Handle::SessionServer(s)) = removed {
                // Closing the server end fails the client's pending request (the IPC handle trick ends this way).
                s.state.lock().unwrap().closed = true;
                s.cv.notify_all();
            }
            RESULT_OK
        }
        0x18 => wait_synchronization(r),
        0x1E => {
            r[0] = crate::horizon::hle::hle_nn_os_GetSystemTick();
            return;
        }
        0x40 => create_session(r),
        0x21 => send_sync_request(r),
        0x43 => reply_and_receive(r),
        _ => {
            println!(
                "[!] svc {:#x} is not implemented (x0={:#x} x1={:#x} x2={:#x})",
                n, r[0], r[1], r[2]
            );
            RESULT_NOT_IMPLEMENTED
        }
    };
    r[0] = result;
    if log_enabled() {
        println!(
            "[svc] {:#04x} -> {:#x} x1={:#x} x2={:#x}",
            n, result, r[1], r[2]
        );
    }
}

#[repr(C)]
struct MemoryInfo {
    addr: u64,
    size: u64,
    mem_type: u32,
    attr: u32,
    perm: u32,
    ipc_refcount: u32,
    device_refcount: u32,
    pad: u32,
}

/// svcQueryMemory(MemoryInfo* out, u32* page_info, u64 addr) -> w1 page_info.
unsafe fn query_memory(r: &mut [u64; 8]) -> u64 {
    let out = r[0] as *mut MemoryInfo;
    let addr = r[2] as usize;
    let (alias_start, alias_size) = alias_region();
    let regions = REGIONS.lock().unwrap().clone();
    let mut info = MemoryInfo {
        addr: 0,
        size: 0,
        mem_type: MEM_FREE,
        attr: 0,
        perm: 0,
        ipc_refcount: 0,
        device_refcount: 0,
        pad: 0,
    };
    if let Some(reg) = regions
        .iter()
        .find(|x| addr >= x.start && addr < x.start + x.size)
    {
        info.addr = reg.start as u64;
        info.size = reg.size as u64;
        info.mem_type = reg.mem_type;
        info.perm = reg.perm;
    } else {
        // Not a guest region. Only the reserved alias region is free (the host owns the rest of the address space:
        // __PAGEZERO, libraries, heaps), so a mod looking for room to map code can only pick addresses we reserved.
        // Block boundaries: guest regions and the alias region's edges.
        let mut edges: Vec<usize> = regions
            .iter()
            .flat_map(|x| [x.start, x.start + x.size])
            .collect();
        edges.extend([alias_start, alias_start + alias_size]);
        let start = edges
            .iter()
            .copied()
            .filter(|&e| e <= addr)
            .max()
            .unwrap_or(0);
        let next = edges.iter().copied().filter(|&e| e > addr).min();
        // The last block reaches the top of the address space, so the next query wraps to 0 (exlaunch stops).
        let size = match next {
            Some(n) => n - start,
            None => 0usize.wrapping_sub(start),
        };
        info.addr = start as u64;
        info.size = size as u64;
        if !(addr >= alias_start && addr < alias_start + alias_size) {
            info.mem_type = MEM_INACCESSIBLE;
        }
    }
    if !out.is_null() {
        std::ptr::write_unaligned(out, info);
    }
    r[1] = 0;
    RESULT_OK
}

/// svcGetInfo(u64* out, u32 id0, Handle handle, u64 id1) -> x1 out.
unsafe fn get_info(r: &mut [u64; 8]) -> u64 {
    let id0 = r[1] as u32;
    let (alias_start, alias_size) = alias_region();
    let (heap_start, heap_size) = *HEAP.lock().unwrap();
    let value: u64 = match id0 {
        0 => 0xF,                   // core mask
        1 => 0xFFFF_FFFF_FFFF_FFFF, // priority mask
        2 => small_alias_region().0 as u64,
        3 => small_alias_region().1 as u64,
        4 => heap_start as u64,
        5 => heap_size as u64,
        6 => 0x1_0000_0000,          // total memory
        7 => 0x8000_0000,            // used memory
        8 => 0,                      // debugger attached
        11 => 0x1234_5678_9abc_def0, // random entropy
        12 => alias_start as u64, // ASLR region: our reservation, the only space a mod may map into
        13 => alias_size as u64,
        14 => 0, // stack region: none reserved
        15 => 0,
        18 => 0x0100_F2C0_115B_6000, // program id
        20 => {
            // User exception context area (newer SDKs' SetDyingMessageRegion writes to +0x1c0).
            static CTX: std::sync::OnceLock<Box<[u8; 0x1000]>> = std::sync::OnceLock::new();
            CTX.get_or_init(|| Box::new([0u8; 0x1000])).as_ptr() as u64
        }
        21 => 0x1_0000_0000,          // total non-system memory
        22 => 0x8000_0000,            // used non-system memory
        23 => 1,                     // is application
        24 => {
            // Free thread count (SDK 11+, Unity 2022+ JobSystem checks >= 1). Limit is 96 threads.
            let active = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8);
            (96usize.saturating_sub(active).max(1)) as u64
        }
        // 65000+ are Mesosphere (Atmosphere kernel) extensions; Nintendo's kernel rejects them, as we do.
        65000.. => return 0xF001 | (0 << 9),
        _ => {
            println!(
                "[!] svcGetInfo: unknown info type {} (sub {:#x})",
                id0, r[3]
            );
            return RESULT_NOT_IMPLEMENTED;
        }
    };
    r[1] = value;
    RESULT_OK
}

#[cfg(nx_native)]
extern "C" {
    static mach_task_self_: u32;
    fn mach_vm_remap(
        target_task: u32,
        target_address: *mut u64,
        size: u64,
        mask: u64,
        flags: i32,
        src_task: u32,
        src_address: u64,
        copy: i32,
        cur_protection: *mut i32,
        max_protection: *mut i32,
        inheritance: u32,
    ) -> i32;
    fn mach_vm_protect(
        task: u32,
        address: u64,
        size: u64,
        set_maximum: i32,
        new_protection: i32,
    ) -> i32;
}


/// The alias half of map_process_memory: [dst, dst + size) maps the same pages as src (mach_vm_remap).
#[cfg(nx_native)]
unsafe fn alias_view(dst: usize, src: usize, size: usize) -> Result<(), u64> {
    const PAGE: usize = 0x4000;
    let mut target = (dst & !(PAGE - 1)) as u64;
    let (mut cur, mut max) = (0i32, 0i32);
    let len = ((dst + size + PAGE - 1) & !(PAGE - 1)) as u64 - target;
    let kr = mach_vm_remap(
        mach_task_self_,
        &mut target,
        len,
        0,
        0x4000, /* FIXED | OVERWRITE */
        mach_task_self_,
        (src & !(PAGE - 1)) as u64,
        0,
        &mut cur,
        &mut max,
        2,
    );
    if kr != 0 || target != (dst & !(PAGE - 1)) as u64 {
        println!(
            "[!] svcMapProcessMemory: mach_vm_remap failed ({}) for {:#x} -> {:#x}",
            kr, src, dst
        );
        return Err(RESULT_INVALID_ADDRESS);
    }
    let kr = mach_vm_protect(
        mach_task_self_,
        target,
        len,
        0,
        libc::PROT_READ | libc::PROT_WRITE,
    );
    if kr != 0 {
        println!("[!] svcMapProcessMemory: cannot make the alias writable ({}); max protection {:#x}", kr, max);
    }
    if log_enabled() {
        println!(
            "[+] svcMapProcessMemory: {:#x}+{:#x} aliases {:#x}",
            dst, size, src
        );
    }
    Ok(())
}

/// Writable views of guest code made by MapProcessMemory whose destination does not share the source's offset within
/// a 16 KiB macOS page (Horizon pages are 4 KiB). Writes made through a view (pages that differ from its baseline, the
/// bytes it was given or last flushed) are copied into the code; comparing against the code itself instead would undo
/// what other views wrote.
struct Shadow {
    dst: usize,
    src: usize,
    size: usize,
    baseline: Vec<u8>,
    /// A view of code that comes from a signed AOT image (cpu/aot.rs, iOS): its writes are checked against the image (which
    /// already holds the mods' patches) and dropped; signed pages cannot change.
    image: bool,
}

/// Bytes mods wrote into image-backed code that differ from the image (cpu/aot.rs): patches they removed again (which the
/// image keeps), plus anything their hooks did differently from when the image was made.
pub static IMAGE_MISMATCH: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static SHADOWS: Mutex<Vec<Shadow>> = Mutex::new(Vec::new());

/// svcMapProcessMemory(u64 dst, Handle process, u64 src, u64 size): a writable view of the process's own code. When
/// dst and src have the same offset within a 16 KiB page this is a real alias (mach_vm_remap); otherwise dst gets a
/// copy, and changed pages are copied back into the code (flush_code_views) on unmap and after each mod's init.
unsafe fn map_process_memory(r: &mut [u64; 8]) -> u64 {
    let (dst, src, size) = (r[0] as usize, r[2] as usize, r[3] as usize);
    let (alias_start, alias_size) = alias_region();
    if dst < alias_start || dst + size > alias_start + alias_size {
        println!(
            "[!] svcMapProcessMemory: destination {:#x}+{:#x} is outside the mappable region",
            dst, size
        );
        return RESULT_INVALID_ADDRESS;
    }
    // The source must be guest memory we know (Horizon fails with InvalidCurrentMemory otherwise).
    {
        let regions = REGIONS.lock().unwrap();
        let mut a = src;
        while a < src + size {
            let hookable = HOOKABLE
                .lock()
                .unwrap()
                .iter()
                .copied()
                .find(|&(st, n)| a >= st && a < st + n);
            match regions
                .iter()
                .find(|x| a >= x.start && a < x.start + x.size && x.mem_type != MEM_ALIAS_CODE)
            {
                Some(x) => a = x.start + x.size,
                None if hookable.is_some() => {
                    let (st, n) = hookable.unwrap();
                    a = st + n;
                }
                None => {
                    let what = match crate::os::address_owner(a) {
                        (Some(lib), sym) => format!("{} {}", lib, sym.unwrap_or_default()),
                        (None, _) => "unknown mapping".to_string(),
                    };
                    println!("[!] svcMapProcessMemory: source {:#x}+{:#x} is not guest memory (first unknown byte {:#x}: {})", src, size, a, what);
                    // Who asked: return addresses up the frame-pointer chain (guest frames sit above the trampoline's).
                    let mut fp = crate::guest_reg!(x29);
                    let mut chain = Vec::new();
                    for _ in 0..16 {
                        if fp < 0x1_0000_0000 || fp & 7 != 0 {
                            break;
                        }
                        chain.push(format!("{:#x}", *((fp + 8) as *const usize)));
                        fp = *(fp as *const usize);
                    }
                    println!("[!]   call chain: {}", chain.join(" <- "));
                    return 0xD401;
                }
            }
        }
    }
    const PAGE: usize = 0x4000;
    // Image code (cpu/aot.rs): always a copy, so writes reach the code only at flushes, where a dump run records each
    // page's new content and a loading run maps the matching signed version.
    let in_image = crate::cpu::aot::active() && crate::cpu::aot::in_region(src, size);
    let image = in_image && crate::cpu::aot::loading();
    // A real alias where the page offsets agree and the host can make one; a copy otherwise.
    if cfg!(nx_native) && dst % PAGE == src % PAGE && !in_image {
        #[cfg(nx_native)]
        if let Err(e) = alias_view(dst, src, size) {
            return e;
        }
    } else {
        let lo = dst & !(PAGE - 1);
        let hi = (dst + size + PAGE - 1) & !(PAGE - 1);
        // Back only the 16 KiB pages no earlier view backs: adjacent views (exlaunch maps a module's segments back to
        // back) share edge pages, and remapping one would wipe the neighbour's copy.
        let covered: Vec<(usize, usize)> = SHADOWS
            .lock()
            .unwrap()
            .iter()
            .map(|v| {
                (
                    v.dst & !(PAGE - 1),
                    (v.dst + v.size + PAGE - 1) & !(PAGE - 1),
                )
            })
            .collect();
        let mut page = lo;
        while page < hi {
            if !covered.iter().any(|&(a, b)| page >= a && page < b) {
                if crate::os::commit(page, PAGE, crate::os::Prot::ReadWrite).is_err() {
                    return RESULT_INVALID_ADDRESS;
                }
            }
            page += PAGE;
        }
        let baseline = std::slice::from_raw_parts(src as *const u8, size).to_vec();
        std::ptr::copy_nonoverlapping(baseline.as_ptr(), dst as *mut u8, size);
        SHADOWS.lock().unwrap().push(Shadow {
            dst,
            src,
            size,
            baseline,
            image,
        });
        if log_enabled() {
            println!("[+] svcMapProcessMemory: {:#x}+{:#x} is a copy of {:#x} (page offsets differ); writes are copied back", dst, size, src);
        }
    }
    add_region(Region {
        start: dst,
        size,
        mem_type: MEM_ALIAS_CODE,
        perm: PERM_RW,
    });
    RESULT_OK
}

/// Copies the 4 KiB pages written through each copy-mode code view into the code it views. Returns pages written.
pub unsafe fn flush_code_views() -> usize {
    let mut shadows = SHADOWS.lock().unwrap();
    shadows.iter_mut().map(|v| flush_one(v)).sum()
}

unsafe fn flush_one(v: &mut Shadow) -> usize {
    let mut written = 0;
    let mut touched: Vec<usize> = Vec::new();
    let mut off = 0;
    while off < v.size {
        let n = (0x1000 - (v.src + off) % 0x1000).min(v.size - off);
        let view = std::slice::from_raw_parts((v.dst + off) as *const u8, n);
        if view != &v.baseline[off..off + n] {
            let page = (v.src + off) & !0x3fff;
            if v.image {
                // Image code already holds the patches (cpu/aot.rs): writes are dropped. Bytes that differ from it are the
                // patches the mod removes again (the image keeps them; the dump run reports how many) or writes the
                // image does not know.
                let code = std::slice::from_raw_parts((v.src + off) as *const u8, n);
                let differ = view.iter().zip(code).filter(|(w, c)| w != c).count();
                if differ > 0 && crate::settings::get().log.log_image_mismatch {
                    for (i, (a, b)) in view.iter().zip(code).enumerate() {
                        if a != b {
                            println!(
                                "[image-mismatch] {:#x}: wrote {:02x}, image {:02x}",
                                v.src + off + i,
                                a,
                                b
                            );
                        }
                    }
                }
                IMAGE_MISMATCH.fetch_add(differ, std::sync::atomic::Ordering::Relaxed);
            } else {
                let span = ((v.src + off + n + 0x3fff) & !0x3fff) - page;
                let _ = crate::os::protect(page as usize, span, crate::os::Prot::ReadWrite);
                std::ptr::copy_nonoverlapping(view.as_ptr(), (v.src + off) as *mut u8, n);
                let _ = crate::os::protect(page as usize, span, crate::os::Prot::ReadExec);
                crate::os::flush_icache(((v.src + off)) as usize, n);
                if !touched.contains(&page) {
                    touched.push(page);
                }
            }
            v.baseline[off..off + n].copy_from_slice(view);
            written += 1;
        }
        off += n;
    }
    for page in touched {
        crate::cpu::aot::record_page(page);
    }
    written
}

/// svcUnmapProcessMemory(u64 dst, Handle process, u64 src, u64 size).
unsafe fn unmap_process_memory(r: &mut [u64; 8]) -> u64 {
    let (dst, src, size) = (r[0] as usize, r[2] as usize, r[3] as usize);
    let shadow = {
        let mut sh = SHADOWS.lock().unwrap();
        let pos = sh.iter().position(|v| v.dst == dst);
        pos.map(|i| sh.remove(i))
    };
    if let Some(mut v) = shadow {
        let pages = flush_one(&mut v);
        if log_enabled() {
            println!(
                "[+] svcUnmapProcessMemory: {} changed pages copied back into {:#x}",
                pages, v.src
            );
        }
    }
    // Back to a reservation, so the region keeps its address space (except pages another view still uses).
    let others: Vec<(usize, usize)> = SHADOWS
        .lock()
        .unwrap()
        .iter()
        .map(|v| (v.dst & !0x3fff, (v.dst + v.size + 0x3fff) & !0x3fff))
        .collect();
    let mut page = dst & !0x3fff;
    while page < (dst + size + 0x3fff) & !0x3fff {
        if !others.iter().any(|&(a, b)| page >= a && page < b) {
            crate::os::decommit(page, 0x4000);
        }
        page += 0x4000;
    }
    REGIONS.lock().unwrap().retain(|x| x.start != dst);
    // Code written through the view must be visible to instruction fetch at the original address.
    crate::os::flush_icache(src as usize, size);
    RESULT_OK
}

/// svcCreateThread(Handle* out, ThreadFunc entry, void* arg, void* stack_top, int prio, int core) -> w1 handle.
unsafe fn create_thread(r: &mut [u64; 8]) -> u64 {
    let h = new_handle(Handle::Thread(ThreadObj {
        entry: r[1] as usize,
        arg: r[2] as usize,
        done: Arc::new((Mutex::new(false), Condvar::new())),
    }));
    r[1] = h as u64;
    RESULT_OK
}

thread_local! {
    static CURRENT_SVC_THREAD_DONE: std::cell::RefCell<Option<Arc<(Mutex<bool>, Condvar)>>> = const { std::cell::RefCell::new(None) };
}

fn mark_done(done: &Arc<(Mutex<bool>, Condvar)>) {
    *done.0.lock().unwrap() = true;
    done.1.notify_all();
}

/// svcStartThread(Handle thread): runs the entry on a new host thread set up like the engine's guest threads.
unsafe fn start_thread(r: &mut [u64; 8]) -> u64 {
    let (entry, arg, done) = {
        let g = HANDLES.lock().unwrap();
        match g.as_ref().and_then(|m| m.get(&(r[0] as u32))) {
            Some(Handle::Thread(t)) => (t.entry, t.arg, t.done.clone()),
            _ => return RESULT_INVALID_HANDLE,
        }
    };
    std::thread::Builder::new()
        .name(format!("svc-thread-{:x}", r[0]))
        .stack_size(1 << 20)
        .spawn(move || {
            crate::guest::tls::mark_guest_thread();
            #[cfg(nx_native)]
            {
                crate::cpu::tls::install_guest_tls();
                crate::cpu::trampoline::init_worker_thread_x18();
            }
            CURRENT_SVC_THREAD_DONE.with(|c| *c.borrow_mut() = Some(done.clone()));
            crate::guest::call(entry, &[arg as u64]);
            mark_done(&done);
        })
        .map_or(RESULT_NOT_IMPLEMENTED, |_| RESULT_OK)
}

/// svcExitThread: ends the calling host thread (guest code never returns from it).
unsafe fn exit_current_thread() {
    CURRENT_SVC_THREAD_DONE.with(|c| {
        if let Some(d) = c.borrow().as_ref() {
            mark_done(d);
        }
    });
    #[cfg(nx_native)]
    libc::pthread_exit(std::ptr::null_mut());
    // A lifted runtime leaves the thread's guest code instead (docs/windows-port.md, step 5).
    #[cfg(not(nx_native))]
    unimplemented!("svcExitThread: the lifted runtime has no per-thread state yet");
}

/// svcWaitSynchronization(s32* index, const Handle* handles, s32 count, s64 timeout) -> w1 index. Threads signal when
/// they have exited; other handles count as signalled.
unsafe fn wait_synchronization(r: &mut [u64; 8]) -> u64 {
    let (handles, count, timeout) = (r[1] as *const u32, r[2] as usize, r[3] as i64);
    let deadline = (timeout >= 0)
        .then(|| std::time::Instant::now() + std::time::Duration::from_nanos(timeout as u64));
    loop {
        for i in 0..count {
            let h = *handles.add(i);
            let signalled = {
                let g = HANDLES.lock().unwrap();
                match g.as_ref().and_then(|m| m.get(&h)) {
                    Some(Handle::Thread(t)) => *t.done.0.lock().unwrap(),
                    Some(_) => true,
                    None if h == PSEUDO_CURRENT_THREAD => false,
                    None => return RESULT_INVALID_HANDLE,
                }
            };
            if signalled {
                r[1] = i as u64;
                return RESULT_OK;
            }
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return RESULT_TIMEOUT;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// svcCreateSession(Handle* server, Handle* client, u32, u64) -> w1 server, w2 client.
unsafe fn create_session(r: &mut [u64; 8]) -> u64 {
    let s = Arc::new(Session {
        state: Mutex::new(SessionState::default()),
        cv: Condvar::new(),
    });
    r[1] = new_handle(Handle::SessionServer(s.clone())) as u64;
    r[2] = new_handle(Handle::SessionClient(s)) as u64;
    RESULT_OK
}

/// svcSendSyncRequest(Handle client): delivers the IPC buffer to the session's server and waits for its reply.
unsafe fn send_sync_request(r: &mut [u64; 8]) -> u64 {
    let Some(s) = session_of(r[0] as u32, false) else {
        println!("[!] svcSendSyncRequest on handle {:#x}: not a session created by svcCreateSession (service IPC is not supported here)", r[0]);
        return RESULT_INVALID_HANDLE;
    };
    let Some(buf) = ipc_buffer() else {
        return RESULT_INVALID_ADDRESS;
    };
    let msg = translate_request(buf);
    let mut st = s.state.lock().unwrap();
    if st.closed {
        return RESULT_SESSION_CLOSED;
    }
    st.request = Some(msg);
    st.reply = None;
    s.cv.notify_all();
    while st.reply.is_none() && !st.closed {
        st = s.cv.wait(st).unwrap();
    }
    let Some(reply) = st.reply.take() else {
        return RESULT_SESSION_CLOSED;
    };
    buf[..reply.len()].copy_from_slice(&reply);
    RESULT_OK
}

/// svcReplyAndReceive(s32* index, const Handle* handles, s32 count, Handle reply_target, s64 timeout) -> w1 index.
unsafe fn reply_and_receive(r: &mut [u64; 8]) -> u64 {
    let (handles, count, reply_target, timeout) =
        (r[1] as *const u32, r[2] as usize, r[3] as u32, r[4] as i64);
    let Some(buf) = ipc_buffer() else {
        return RESULT_INVALID_ADDRESS;
    };
    if reply_target != 0 {
        if let Some(s) = session_of(reply_target, true) {
            let mut st = s.state.lock().unwrap();
            st.reply = Some(buf.to_vec());
            s.cv.notify_all();
        }
    }
    if count == 0 {
        return RESULT_TIMEOUT;
    }
    let deadline = (timeout >= 0)
        .then(|| std::time::Instant::now() + std::time::Duration::from_nanos(timeout as u64));
    loop {
        for i in 0..count {
            let Some(s) = session_of(*handles.add(i), true) else {
                return RESULT_INVALID_HANDLE;
            };
            let mut st = s.state.lock().unwrap();
            if let Some(req) = st.request.take() {
                buf[..req.len()].copy_from_slice(&req);
                r[1] = i as u64;
                return RESULT_OK;
            }
            if count == 1 {
                // Single session: block on its condition variable instead of polling.
                let wait = deadline
                    .map(|d| d.saturating_duration_since(std::time::Instant::now()))
                    .unwrap_or(std::time::Duration::from_millis(50));
                let (st2, _) =
                    s.cv.wait_timeout(st, wait.min(std::time::Duration::from_millis(50)))
                        .unwrap();
                drop(st2);
            }
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return RESULT_TIMEOUT;
        }
        if count > 1 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
