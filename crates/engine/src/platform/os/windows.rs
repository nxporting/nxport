// The OS layer on Windows: the VirtualAlloc family, placeholders, vectored exception handlers.

use super::*;
use std::ffi::c_void;

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const MEM_RELEASE: u32 = 0x8000;

#[repr(C)]
struct SystemInfo {
    processor_architecture: u16,
    reserved: u16,
    page_size: u32,
    minimum_application_address: *mut c_void,
    maximum_application_address: *mut c_void,
    active_processor_mask: usize,
    number_of_processors: u32,
    processor_type: u32,
    allocation_granularity: u32,
    processor_level: u16,
    processor_revision: u16,
}

extern "system" {
    fn VirtualAlloc(addr: *mut c_void, size: usize, ty: u32, prot: u32) -> *mut c_void;
    fn VirtualFree(addr: *mut c_void, size: usize, ty: u32) -> i32;
    fn VirtualProtect(addr: *mut c_void, size: usize, prot: u32, old: *mut u32) -> i32;
    fn FlushInstructionCache(process: *mut c_void, addr: *const c_void, size: usize) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetSystemInfo(info: *mut SystemInfo);
}

fn flags(prot: Prot) -> u32 {
    match prot {
        Prot::None => 0x01,          // PAGE_NOACCESS
        Prot::Read => 0x02,          // PAGE_READONLY
        Prot::ReadWrite => 0x04,     // PAGE_READWRITE
        Prot::ReadExec => 0x20,      // PAGE_EXECUTE_READ
        Prot::ReadWriteExec => 0x40, // PAGE_EXECUTE_READWRITE
    }
}

pub fn page_size() -> usize {
    let mut info: SystemInfo = unsafe { std::mem::zeroed() };
    unsafe { GetSystemInfo(&mut info) };
    info.page_size as usize
}

pub unsafe fn alloc(size: usize, prot: Prot) -> io::Result<*mut u8> {
    let p = VirtualAlloc(std::ptr::null_mut(), size, MEM_COMMIT | MEM_RESERVE, flags(prot));
    if p.is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(p as *mut u8)
    }
}

pub unsafe fn free(p: *mut u8, _size: usize) {
    heap::forget(p as usize);
    VirtualFree(p as *mut c_void, 0, MEM_RELEASE);
}

pub unsafe fn reserve(at: At, size: usize) -> io::Result<usize> {
    let try_at = |a: usize| VirtualAlloc(a as *mut c_void, size, MEM_RESERVE, flags(Prot::None)) as usize;
    let p = match at {
        At::Anywhere => try_at(0),
        At::Near(a) => match try_at(a) {
            0 => try_at(0),
            p => p,
        },
        // VirtualAlloc rounds the address down to the allocation granularity rather than failing.
        At::Exactly(a) => match try_at(a) {
            p if p == a => p,
            0 => 0,
            p => {
                VirtualFree(p as *mut c_void, 0, MEM_RELEASE);
                return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{a:#x} is not aligned")));
            }
        },
    };
    if p == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(p)
    }
}

/// The guest heap's pages, committed by chunk at the first access, from the fault handler.
///
/// A committed page costs nothing until touched but counts against the system's commit limit, so a whole heap
/// committed at once can exhaust it with plenty of RAM free. Nothing changes for code that touches a committed page.
///
/// VirtualAlloc(MEM_COMMIT) over a committed page sets that page's protection: over a page the report guard or the
/// write tracking holds, it would silently lift the guard. So a chunk is committed once only, under the lock, and
/// protect commits a range's chunks before it can hold any page in them.
mod heap {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard};

    const CHUNK: usize = 1 << 20;
    const CHUNKS: usize = (1 << 32) / CHUNK;
    static BASE: AtomicUsize = AtomicUsize::new(0);
    static LEN: AtomicUsize = AtomicUsize::new(0);
    static DONE: [AtomicU64; CHUNKS / 64] = [const { AtomicU64::new(0) }; CHUNKS / 64];
    static LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn done(c: usize) -> bool {
        DONE[c / 64].load(Ordering::Acquire) & 1 << (c % 64) != 0
    }

    /// The offset of `addr` in the heap, if it is in it.
    pub fn offset(addr: usize) -> Option<usize> {
        let at = addr.wrapping_sub(BASE.load(Ordering::Acquire));
        (at < LEN.load(Ordering::Acquire)).then_some(at)
    }

    /// Where the heap ends, if `addr` is in it.
    pub fn end(addr: usize) -> Option<usize> {
        offset(addr).map(|_| BASE.load(Ordering::Acquire) + LEN.load(Ordering::Acquire))
    }

    unsafe fn commit_chunk(c: usize) -> bool {
        let _g = lock();
        if done(c) {
            return true;
        }
        let (base, len, at) = (BASE.load(Ordering::Acquire), LEN.load(Ordering::Acquire), c * CHUNK);
        if at >= len {
            return false;
        }
        // The system can refuse while it grows the page file, so a refusal is tried again before it counts.
        for _ in 0..10 {
            if !VirtualAlloc((base + at) as *mut c_void, CHUNK.min(len - at), MEM_COMMIT, flags(Prot::ReadWrite)).is_null() {
                DONE[c / 64].fetch_or(1 << (c % 64), Ordering::AcqRel);
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        eprintln!("[-] Guest heap: no commit for 1 MiB at offset {at:#x}: {}", io::Error::last_os_error());
        false
    }

    /// An access fault at `addr`: true if it was for a chunk not committed yet, which now is.
    pub unsafe fn fault(addr: usize) -> bool {
        match offset(addr) {
            Some(at) if !done(at / CHUNK) => commit_chunk(at / CHUNK),
            _ => false,
        }
    }

    pub unsafe fn ensure(addr: usize, len: usize) {
        let (first, heap_len) = (addr.wrapping_sub(BASE.load(Ordering::Acquire)), LEN.load(Ordering::Acquire));
        if first < heap_len {
            for c in first / CHUNK..=(first + len.max(1) - 1).min(heap_len - 1) / CHUNK {
                if !done(c) {
                    commit_chunk(c);
                }
            }
        }
    }

    /// Reserves `size` as the block committed on demand. None if it is small or there is one already: those commit
    /// whole, as before.
    pub unsafe fn adopt(size: usize) -> io::Result<Option<usize>> {
        let _g = lock();
        if LEN.load(Ordering::Acquire) != 0 || size < 256 << 20 || size > CHUNKS * CHUNK {
            return Ok(None);
        }
        let base = VirtualAlloc(std::ptr::null_mut(), size, MEM_RESERVE, flags(Prot::None)) as usize;
        if base == 0 {
            return Err(io::Error::last_os_error());
        }
        BASE.store(base, Ordering::Release);
        LEN.store(size, Ordering::Release);
        Ok(Some(base))
    }

    pub unsafe fn forget(base: usize) {
        let _g = lock();
        if base == BASE.load(Ordering::Acquire) {
            LEN.store(0, Ordering::Release);
            BASE.store(0, Ordering::Release);
            for w in &DONE {
                w.store(0, Ordering::Release);
            }
        }
    }
}

pub unsafe fn alloc_on_demand(size: usize) -> io::Result<*mut u8> {
    match heap::adopt(size)? {
        Some(p) => Ok(p as *mut u8),
        None => alloc(size, Prot::ReadWrite),
    }
}

pub unsafe fn ensure_committed(addr: usize, len: usize) {
    heap::ensure(addr, len)
}

pub unsafe fn commit(addr: usize, size: usize, prot: Prot) -> io::Result<()> {
    if VirtualAlloc(addr as *mut c_void, size, MEM_COMMIT, flags(prot)).is_null() {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub unsafe fn decommit(addr: usize, size: usize) {
    const MEM_DECOMMIT: u32 = 0x4000;
    VirtualFree(addr as *mut c_void, size, MEM_DECOMMIT);
}

pub unsafe fn discard(addr: usize, size: usize) {
    const MEM_RESET: u32 = 0x80000;
    VirtualAlloc(addr as *mut c_void, size, MEM_RESET, flags(Prot::ReadWrite));
}

pub unsafe fn release(addr: usize, _size: usize) {
    VirtualFree(addr as *mut c_void, 0, MEM_RELEASE);
}

pub fn current_thread_stack() -> (usize, usize) {
    extern "system" {
        fn GetCurrentThreadStackLimits(low: *mut usize, high: *mut usize);
    }
    let (mut lo, mut hi) = (0, 0);
    unsafe { GetCurrentThreadStackLimits(&mut lo, &mut hi) };
    (lo, hi)
}

pub fn current_thread_id() -> u64 {
    extern "system" {
        fn GetCurrentThreadId() -> u32;
    }
    unsafe { GetCurrentThreadId() as u64 }
}

pub fn exit_thread() -> ! {
    extern "system" {
        fn ExitThread(code: u32) -> !;
    }
    unsafe { ExitThread(0) }
}

pub fn address_owner(addr: usize) -> (Option<String>, Option<String>) {
    const FROM_ADDRESS: u32 = 0x4; // GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    extern "system" {
        fn GetModuleHandleExW(flags: u32, name: *const c_void, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    }
    let mut m = std::ptr::null_mut();
    if unsafe { GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, addr as *const c_void, &mut m) } == 0 {
        return (None, None);
    }
    let mut name = [0u16; 512];
    let n = unsafe { GetModuleFileNameW(m, name.as_mut_ptr(), name.len() as u32) } as usize;
    ((n > 0).then(|| String::from_utf16_lossy(&name[..n])), None)
}

pub fn image_range(addr: usize) -> Option<(usize, usize)> {
    const FROM_ADDRESS: u32 = 0x4; // GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    extern "system" {
        fn GetModuleHandleExW(flags: u32, name: *const c_void, module: *mut *mut c_void) -> i32;
    }
    let mut m = std::ptr::null_mut();
    if unsafe { GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, addr as *const c_void, &mut m) } == 0 {
        return None;
    }
    // A module handle is its image's base: the PE header's offset at 0x3c, SizeOfImage at 0x50 in the header.
    let base = m as usize;
    let size = unsafe {
        let pe = base + (((base + 0x3c) as *const u32).read_unaligned() as usize);
        ((pe + 0x50) as *const u32).read_unaligned() as usize
    };
    Some((base, base + size))
}

extern "C" {
    fn _localtime64_s(tm: *mut libc::tm, t: *const i64) -> i32;
    fn _gmtime64_s(tm: *mut libc::tm, t: *const i64) -> i32;
    fn _mkgmtime64(tm: *mut libc::tm) -> i64;
    fn _get_tzname(len: *mut usize, buf: *mut std::ffi::c_char, size: usize, index: i32) -> i32;
}

pub fn local_time(t: i64) -> (libc::tm, String, bool, i32) {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { _localtime64_s(&mut tm, &t) };
    let dst = tm.tm_isdst > 0;
    // The local fields read as UTC are ahead of `t` by the offset.
    let mut copy = tm;
    let off = (unsafe { _mkgmtime64(&mut copy) } - t) as i32;
    // Windows names zones in full ("Pacific Standard Time"), not by abbreviation.
    let mut buf = [0 as std::ffi::c_char; 64];
    let mut len = 0usize;
    let name = if unsafe { _get_tzname(&mut len, buf.as_mut_ptr(), buf.len(), dst as i32) } == 0 && len > 1 {
        unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
    } else {
        "UTC".into()
    };
    (tm, name, dst, off)
}

pub fn utc_time(t: i64) -> libc::tm {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { _gmtime64_s(&mut tm, &t) };
    tm
}

pub fn from_utc_time(tm: &mut libc::tm) -> i64 {
    unsafe { _mkgmtime64(tm) }
}

pub fn library_open(path: &std::path::Path) -> Result<*mut c_void, String> {
    use std::os::windows::ffi::OsStrExt;
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let h = unsafe { LoadLibraryW(wide.as_ptr()) };
    if h.is_null() {
        Err(format!("{}: {}", path.display(), io::Error::last_os_error()))
    } else {
        Ok(h)
    }
}

pub unsafe fn library_symbol(lib: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void {
    extern "system" {
        fn GetProcAddress(module: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void;
    }
    GetProcAddress(lib, name)
}

const FLS_OUT_OF_INDEXES: u32 = u32::MAX;

extern "system" {
    fn FlsAlloc(callback: Option<TlsDtor>) -> u32;
    fn FlsFree(index: u32) -> i32;
    fn FlsGetValue(index: u32) -> *mut c_void;
    fn FlsSetValue(index: u32, value: *mut c_void) -> i32;
}

// Fiber-local storage rather than TlsAlloc: only it runs a destructor at thread exit. Its callback is
// extern "system", the same convention as extern "C" on 64-bit Windows.
pub unsafe fn tls_create(dtor: Option<TlsDtor>) -> io::Result<TlsKey> {
    match FlsAlloc(dtor) {
        FLS_OUT_OF_INDEXES => Err(io::Error::last_os_error()),
        i => Ok(TlsKey(i as usize)),
    }
}

pub unsafe fn tls_delete(key: TlsKey) {
    FlsFree(key.0 as u32);
}

pub unsafe fn tls_get(key: TlsKey) -> *mut c_void {
    FlsGetValue(key.0 as u32)
}

pub unsafe fn tls_set(key: TlsKey, value: *mut c_void) -> io::Result<()> {
    if FlsSetValue(key.0 as u32, value) != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub unsafe fn protect(addr: usize, size: usize, prot: Prot) -> io::Result<()> {
    let mut old = 0;
    if VirtualProtect(addr as *mut c_void, size, flags(prot), &mut old) != 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    // A range with heap chunks not committed yet (it changes nothing then): the guards hold what they asked to hold.
    if heap::offset(addr).is_some() || heap::offset(addr + size - 1).is_some() {
        heap::ensure(addr, size);
        if VirtualProtect(addr as *mut c_void, size, flags(prot), &mut old) != 0 {
            return Ok(());
        }
    }
    Err(e)
}

pub fn flush_icache(addr: usize, len: usize) {
    unsafe { FlushInstructionCache(GetCurrentProcess(), addr as *const c_void, len) };
}

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut ExceptionRecord,
    address: *mut c_void,
    parameters: u32,
    information: [usize; 15],
}

#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut c_void,
}

type VectoredHandler = unsafe extern "system" fn(*mut ExceptionPointers) -> i32;

extern "system" {
    fn AddVectoredExceptionHandler(first: u32, handler: VectoredHandler) -> *mut c_void;
}

const EXCEPTION_ACCESS_VIOLATION: u32 = 0xC000_0005;
const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

static HANDLER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

// An access violation's information: [0] the access (0 read, 1 write, 8 execute), [1] the address.
unsafe extern "system" fn vectored(p: *mut ExceptionPointers) -> i32 {
    let r = &*(*p).record;
    let h = HANDLER.load(std::sync::atomic::Ordering::Acquire);
    if r.code != EXCEPTION_ACCESS_VIOLATION || r.parameters < 2 || h == 0 {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    if heap::fault(r.information[1]) {
        return EXCEPTION_CONTINUE_EXECUTION;
    }
    let h: FaultHandler = std::mem::transmute(h);
    if h(r.information[1], r.information[0] == 1, r.address as usize) || heap_chunk_arrived(r.information[1], r.information[0]) {
        EXCEPTION_CONTINUE_EXECUTION
    } else {
        EXCEPTION_CONTINUE_SEARCH
    }
}

/// A fault on a heap chunk that another thread committed between the fault and heap::fault: the access can go ahead.
unsafe fn heap_chunk_arrived(addr: usize, access: usize) -> bool {
    const PAGE_NOACCESS: u32 = 0x01;
    const WRITABLE: u32 = 0x04 | 0x08 | 0x40 | 0x80;
    if access > 1 || heap::offset(addr).is_none() {
        return false;
    }
    let mut info: MemoryBasicInformation = std::mem::zeroed();
    VirtualQuery(addr as *const c_void, &mut info, std::mem::size_of::<MemoryBasicInformation>()) != 0
        && info.state == MEM_COMMIT
        && info.protect & PAGE_NOACCESS == 0
        && (access == 0 || info.protect & WRITABLE != 0)
}

#[repr(C)]
struct MemoryBasicInformation {
    base_address: *mut c_void,
    allocation_base: *mut c_void,
    allocation_protect: u32,
    partition_id: u16,
    region_size: usize,
    state: u32,
    protect: u32,
    ty: u32,
}

extern "system" {
    fn VirtualQuery(addr: *const c_void, info: *mut MemoryBasicInformation, len: usize) -> usize;
}

pub fn readable_len(addr: usize, max: usize) -> usize {
    const PAGE_NOACCESS: u32 = 0x01;
    const PAGE_GUARD: u32 = 0x100;
    let end = addr + max;
    let mut at = addr;
    while at < end {
        let mut info: MemoryBasicInformation = unsafe { std::mem::zeroed() };
        let n = unsafe { VirtualQuery(at as *const c_void, &mut info, std::mem::size_of::<MemoryBasicInformation>()) };
        if let (true, Some(heap_end)) = (n != 0 && info.state != MEM_COMMIT, heap::end(at)) {
            at = (info.base_address as usize + info.region_size).min(heap_end); // a read commits it
            continue;
        }
        if n == 0 || info.state != MEM_COMMIT || info.protect == 0 || info.protect & (PAGE_NOACCESS | PAGE_GUARD) != 0 {
            break;
        }
        at = info.base_address as usize + info.region_size;
    }
    at.min(end) - addr
}

#[cfg(test)]
pub fn committed(addr: usize) -> bool {
    let mut info: MemoryBasicInformation = unsafe { std::mem::zeroed() };
    unsafe { VirtualQuery(addr as *const c_void, &mut info, std::mem::size_of::<MemoryBasicInformation>()) };
    info.state == MEM_COMMIT
}

pub fn read_only_ranges(addr: usize, len: usize) -> Vec<(usize, usize)> {
    const WRITABLE: u32 = 0x04 | 0x08 | 0x40 | 0x80;
    let (end, mut at, mut out) = (addr + len, addr, Vec::new());
    while at < end {
        let mut info: MemoryBasicInformation = unsafe { std::mem::zeroed() };
        let n = unsafe { VirtualQuery(at as *const c_void, &mut info, std::mem::size_of::<MemoryBasicInformation>()) };
        if n == 0 {
            break;
        }
        let next = (info.base_address as usize + info.region_size).min(end);
        if info.state == MEM_COMMIT && info.protect & WRITABLE == 0 {
            out.push((at, next));
        }
        at = next;
    }
    out
}

pub fn writable_len(addr: usize, max: usize) -> usize {
    // PAGE_READWRITE, PAGE_WRITECOPY, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY.
    const WRITABLE: u32 = 0x04 | 0x08 | 0x40 | 0x80;
    let end = addr + max;
    let mut at = addr;
    while at < end {
        let mut info: MemoryBasicInformation = unsafe { std::mem::zeroed() };
        let n = unsafe { VirtualQuery(at as *const c_void, &mut info, std::mem::size_of::<MemoryBasicInformation>()) };
        if let (true, Some(heap_end)) = (n != 0 && info.state != MEM_COMMIT, heap::end(at)) {
            at = (info.base_address as usize + info.region_size).min(heap_end); // a write commits it
            continue;
        }
        if n == 0 || info.state != MEM_COMMIT || info.protect & WRITABLE == 0 {
            break;
        }
        at = info.base_address as usize + info.region_size;
    }
    at.min(end) - addr
}

extern "system" {
    fn SetUnhandledExceptionFilter(filter: Option<VectoredHandler>) -> Option<VectoredHandler>;
    fn GetModuleHandleExW(flags: u32, addr: *const c_void, module: *mut *mut c_void) -> i32;
    fn IsBadReadPtr(addr: *const c_void, len: usize) -> i32;
}

/// The crashing thread's guest registers as of its last call between lifted functions, and what looks like guest
/// code on its guest stack, newest first. An exlaunch plugin that gives up writes its result to 0x6969696969696969.
unsafe fn guest_state(pc: usize) {
    let cpu = crate::guest::exec::lifted::existing_thread_cpu();
    if cpu.is_null() {
        return;
    }
    let cpu = &*cpu;
    let readable = |addr: u64, len: usize| IsBadReadPtr(addr as *const c_void, len) == 0;
    let code = pc.wrapping_sub(8) as u64;
    if readable(code, 8) && std::ptr::read_unaligned(code as *const u64) == 0x6969_6969_6969_6969 {
        let result = cpu.x[0];
        let value = if readable(result, 4) {
            format!("{:#x}", std::ptr::read_unaligned(result as *const u32))
        } else {
            "unreadable".to_string()
        };
        eprintln!("[crash] a plugin mod aborted itself (exlaunch), result {value}");
    }
    for (i, row) in cpu.x.chunks(4).enumerate() {
        let values: Vec<String> = row.iter().map(|v| format!("{v:#018x}")).collect();
        eprintln!("[crash] x{:<2} {}", i * 4, values.join(" "));
    }
    eprintln!("[crash] sp {:#x}", cpu.sp);
    let mut found = Vec::new();
    for i in 0..512u64 {
        let at = cpu.sp.wrapping_add(i * 8);
        if !readable(at, 8) {
            break;
        }
        let v = std::ptr::read_unaligned(at as *const u64);
        if (0x70_0000_0000..0x71_0000_0000).contains(&v) && v & 3 == 0 && found.len() < 32 {
            found.push(format!("sp+{:#x}: {v:#x}", i * 8));
        }
    }
    eprintln!("[crash] guest addresses on the stack: {}", found.join(", "));
}

// An exception nothing handled (the process ends after this): what and where, the module and offset of the faulting
// instruction, and the thread's stack.
unsafe extern "system" fn unhandled(p: *mut ExceptionPointers) -> i32 {
    let r = &*(*p).record;
    let pc = r.address as usize;
    let (module, symbol) = address_owner(pc);
    let base = {
        let mut m = std::ptr::null_mut();
        // GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT
        if GetModuleHandleExW(0x4 | 0x2, pc as *const c_void, &mut m) != 0 { m as usize } else { 0 }
    };
    eprintln!(
        "[crash] exception {:#010x} at {pc:#x} ({}+{:#x}{}), thread {:?}",
        r.code,
        module.as_deref().unwrap_or("?"),
        pc.wrapping_sub(base),
        symbol.map(|s| format!(", {s}")).unwrap_or_default(),
        std::thread::current().name().unwrap_or("?"),
    );
    if r.code == EXCEPTION_ACCESS_VIOLATION && r.parameters >= 2 {
        let what = match r.information[0] {
            0 => "read",
            1 => "write",
            _ => "execute",
        };
        eprintln!("[crash] {what} of {:#x}", r.information[1]);
    }
    guest_state(pc);
    eprintln!("[crash] stack:\n{}", std::backtrace::Backtrace::force_capture());
    crate::config::drain_log();
    EXCEPTION_CONTINUE_SEARCH
}

pub fn report_crashes() {
    unsafe { SetUnhandledExceptionFilter(Some(unhandled)) };
}

pub fn on_access_fault(handler: FaultHandler) {
    if HANDLER.swap(handler as usize, std::sync::atomic::Ordering::AcqRel) == 0 {
        unsafe { AddVectoredExceptionHandler(1, vectored) };
    }
}
