// The host operating system's memory primitives, for the runtime (docs/windows-port.md, "OS layer"). On Unix hosts
// each is the libc / Darwin call the runtime made directly before; Windows gets the VirtualAlloc family.
//
// The API states what a caller needs, not mmap's flags, because Windows differs where it matters: memory is released
// only as whole allocations, and mapping over part of a reservation takes placeholders. Operations that only make
// sense as mmap tricks (trimming an over-sized mapping, replacing part of a mapping in place) are not offered here.

use std::io;

/// Page protection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prot {
    None,
    Read,
    ReadWrite,
    ReadExec,
    ReadWriteExec,
}

/// The host's page size.
pub fn page_size() -> usize {
    imp::page_size()
}

/// `size` bytes of zeroed memory anywhere, with protection `prot`. Released with `free(p, size)`.
/// # Safety
/// The caller owns the memory until it frees it.
pub unsafe fn alloc(size: usize, prot: Prot) -> io::Result<*mut u8> {
    imp::alloc(size, prot)
}

/// Releases memory from `alloc`: the whole allocation (`p` and `size` as returned and asked for).
/// # Safety
/// Nothing uses the memory any more.
pub unsafe fn free(p: *mut u8, size: usize) {
    imp::free(p, size)
}

/// Where `reserve` places a reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum At {
    /// Wherever the host puts it.
    Anywhere,
    /// At this address if the range is free, elsewhere if not.
    Near(usize),
    /// At this address, or an error.
    Exactly(usize),
}

/// Reserves `size` bytes of address space, inaccessible until parts of it are committed. A reservation at a chosen
/// address must start on the host's allocation granularity (64 KiB on Windows). Released as a whole with `release`.
/// # Safety
/// The caller owns the range until it releases it.
pub unsafe fn reserve(at: At, size: usize) -> io::Result<usize> {
    imp::reserve(at, size)
}

/// Makes whole pages [addr, addr + size) inside a reservation usable, with protection `prot`. They must not have
/// been committed before; they read as zero.
/// # Safety
/// The range lies in a reservation of the caller's.
pub unsafe fn commit(addr: usize, size: usize, prot: Prot) -> io::Result<()> {
    imp::commit(addr, size, prot)
}

/// Returns committed pages [addr, addr + size) of a reservation to the reserved state: inaccessible, their memory given
/// back, committable again.
/// # Safety
/// Nothing uses the pages any more.
pub unsafe fn decommit(addr: usize, size: usize) {
    imp::decommit(addr, size)
}

/// Lets the host drop the contents of pages [addr, addr + size) when it needs the memory; they stay accessible and read
/// as either their old contents or zero.
/// # Safety
/// Nothing relies on the pages' contents.
pub unsafe fn discard(addr: usize, size: usize) {
    imp::discard(addr, size)
}

/// Releases a reservation: `addr` and `size` as `reserve` returned and was asked for.
/// # Safety
/// Nothing uses the range any more.
pub unsafe fn release(addr: usize, size: usize) {
    imp::release(addr, size)
}

/// A host thread-local slot (a pthread key; fiber-local storage on Windows, which also has destructors).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlsKey(usize);

/// A thread exit destructor: called with a thread's value when that is not null.
pub type TlsDtor = unsafe extern "C" fn(*mut std::ffi::c_void);

/// A new slot, null on every thread.
pub fn tls_create(dtor: Option<TlsDtor>) -> io::Result<TlsKey> {
    unsafe { imp::tls_create(dtor) }
}

/// # Safety
/// Nothing uses the slot any more.
pub unsafe fn tls_delete(key: TlsKey) {
    imp::tls_delete(key)
}

pub fn tls_get(key: TlsKey) -> *mut std::ffi::c_void {
    unsafe { imp::tls_get(key) }
}

pub fn tls_set(key: TlsKey, value: *mut std::ffi::c_void) -> io::Result<()> {
    unsafe { imp::tls_set(key, value) }
}

/// A dynamic library, loaded for the rest of the process (the lifted guest library, the movie shim).
#[derive(Clone, Copy, Debug)]
pub struct Library(*mut std::ffi::c_void);

// A module handle, usable from any thread.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

impl Library {
    pub fn open(path: &std::path::Path) -> Result<Library, String> {
        imp::library_open(path).map(Library)
    }

    /// The address of symbol `name`, or null.
    pub fn symbol(&self, name: &str) -> *mut std::ffi::c_void {
        let Ok(c) = std::ffi::CString::new(name) else {
            return std::ptr::null_mut();
        };
        unsafe { imp::library_symbol(self.0, c.as_ptr()) }
    }
}

/// The calling thread's stack (lo, hi).
pub fn current_thread_stack() -> (usize, usize) {
    imp::current_thread_stack()
}

/// An identifier of the calling thread, unique among live threads.
pub fn current_thread_id() -> u64 {
    imp::current_thread_id()
}

/// The host library containing `addr` and the nearest symbol before it, as far as the host's loader knows (logs).
/// Windows names only the library.
pub fn address_owner(addr: usize) -> (Option<String>, Option<String>) {
    imp::address_owner(addr)
}

/// Local time on the host: `t` (seconds since 1970) broken down, the time zone's abbreviation, whether daylight saving
/// time is in effect, and the UTC offset in seconds.
pub fn local_time(t: i64) -> (libc::tm, String, bool, i32) {
    imp::local_time(t)
}

/// `t` (seconds since 1970) broken down in UTC.
pub fn utc_time(t: i64) -> libc::tm {
    imp::utc_time(t)
}

/// The seconds since 1970 of a UTC time (timegm).
pub fn from_utc_time(tm: &mut libc::tm) -> i64 {
    imp::from_utc_time(tm)
}

/// Changes the protection of whole pages [addr, addr + size).
/// # Safety
/// The pages belong to the caller; code relying on the old protection is not running.
pub unsafe fn protect(addr: usize, size: usize, prot: Prot) -> io::Result<()> {
    imp::protect(addr, size, prot)
}

/// Makes code written to [addr, addr + len) visible to instruction fetch (a no-op where caches are coherent).
pub fn flush_icache(addr: usize, len: usize) {
    imp::flush_icache(addr, len)
}

#[cfg(unix)]
mod imp {
    use super::*;

    fn flags(prot: Prot) -> libc::c_int {
        match prot {
            Prot::None => libc::PROT_NONE,
            Prot::Read => libc::PROT_READ,
            Prot::ReadWrite => libc::PROT_READ | libc::PROT_WRITE,
            Prot::ReadExec => libc::PROT_READ | libc::PROT_EXEC,
            Prot::ReadWriteExec => libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
        }
    }

    pub fn page_size() -> usize {
        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
    }

    pub unsafe fn alloc(size: usize, prot: Prot) -> io::Result<*mut u8> {
        let p = libc::mmap(std::ptr::null_mut(), size, flags(prot), libc::MAP_PRIVATE | libc::MAP_ANON, -1, 0);
        if p == libc::MAP_FAILED {
            Err(io::Error::last_os_error())
        } else {
            Ok(p as *mut u8)
        }
    }

    pub unsafe fn free(p: *mut u8, size: usize) {
        libc::munmap(p as *mut libc::c_void, size);
    }

    pub unsafe fn reserve(at: At, size: usize) -> io::Result<usize> {
        // Never MAP_FIXED: that would replace whatever is mapped there. A hint the kernel cannot honour lands elsewhere.
        let hint = match at {
            At::Anywhere => 0,
            At::Near(a) | At::Exactly(a) => a,
        };
        let p = libc::mmap(
            hint as *mut libc::c_void,
            size,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_NORESERVE,
            -1,
            0,
        );
        if p == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        if matches!(at, At::Exactly(a) if p as usize != a) {
            libc::munmap(p, size);
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("{hint:#x} is not free")));
        }
        Ok(p as usize)
    }

    pub unsafe fn commit(addr: usize, size: usize, prot: Prot) -> io::Result<()> {
        // Fresh anonymous pages over the reserved ones.
        let p = libc::mmap(
            addr as *mut libc::c_void,
            size,
            flags(prot),
            libc::MAP_FIXED | libc::MAP_PRIVATE | libc::MAP_ANON,
            -1,
            0,
        );
        if p == libc::MAP_FAILED {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub unsafe fn decommit(addr: usize, size: usize) {
        // A fresh inaccessible mapping over the pages: drops them, keeps the range reserved.
        libc::mmap(
            addr as *mut libc::c_void,
            size,
            libc::PROT_NONE,
            libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_NORESERVE | libc::MAP_FIXED,
            -1,
            0,
        );
    }

    pub unsafe fn discard(addr: usize, size: usize) {
        #[cfg(target_vendor = "apple")]
        libc::madvise(addr as *mut libc::c_void, size, libc::MADV_FREE_REUSABLE);
        #[cfg(not(target_vendor = "apple"))]
        libc::madvise(addr as *mut libc::c_void, size, libc::MADV_DONTNEED);
    }

    pub unsafe fn release(addr: usize, size: usize) {
        libc::munmap(addr as *mut libc::c_void, size);
    }

    #[cfg(target_vendor = "apple")]
    pub fn current_thread_stack() -> (usize, usize) {
        unsafe {
            let me = libc::pthread_self();
            let hi = libc::pthread_get_stackaddr_np(me) as usize;
            (hi - libc::pthread_get_stacksize_np(me), hi)
        }
    }

    #[cfg(not(target_vendor = "apple"))]
    pub fn current_thread_stack() -> (usize, usize) {
        unsafe {
            let mut attr: libc::pthread_attr_t = std::mem::zeroed();
            let (mut lo, mut size) = (std::ptr::null_mut(), 0usize);
            if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) == 0 {
                libc::pthread_attr_getstack(&attr, &mut lo, &mut size);
                libc::pthread_attr_destroy(&mut attr);
            }
            (lo as usize, lo as usize + size)
        }
    }

    pub fn current_thread_id() -> u64 {
        unsafe { libc::pthread_self() as u64 }
    }

    pub fn address_owner(addr: usize) -> (Option<String>, Option<String>) {
        let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
        if unsafe { libc::dladdr(addr as *const libc::c_void, &mut info) } == 0 {
            return (None, None);
        }
        let s = |p: *const libc::c_char| {
            (!p.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned())
        };
        (s(info.dli_fname), s(info.dli_sname))
    }

    pub fn local_time(t: i64) -> (libc::tm, String, bool, i32) {
        let t = t as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&t, &mut tm) };
        let name = if tm.tm_zone.is_null() {
            "UTC".into()
        } else {
            unsafe { std::ffi::CStr::from_ptr(tm.tm_zone) }.to_string_lossy().into_owned()
        };
        (tm, name, tm.tm_isdst > 0, tm.tm_gmtoff as i32)
    }

    pub fn utc_time(t: i64) -> libc::tm {
        let t = t as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::gmtime_r(&t, &mut tm) };
        tm
    }

    pub fn from_utc_time(tm: &mut libc::tm) -> i64 {
        unsafe { libc::timegm(tm) as i64 }
    }

    pub fn library_open(path: &std::path::Path) -> Result<*mut libc::c_void, String> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if h.is_null() {
            Err(format!("{}: {}", path.display(), unsafe {
                std::ffi::CStr::from_ptr(libc::dlerror()).to_string_lossy()
            }))
        } else {
            Ok(h)
        }
    }

    pub unsafe fn library_symbol(lib: *mut libc::c_void, name: *const libc::c_char) -> *mut libc::c_void {
        libc::dlsym(lib, name)
    }

    pub unsafe fn tls_create(dtor: Option<TlsDtor>) -> io::Result<TlsKey> {
        let mut k: libc::pthread_key_t = 0;
        match libc::pthread_key_create(&mut k, dtor) {
            0 => Ok(TlsKey(k as usize)),
            e => Err(io::Error::from_raw_os_error(e)),
        }
    }

    pub unsafe fn tls_delete(key: TlsKey) {
        libc::pthread_key_delete(key.0 as libc::pthread_key_t);
    }

    pub unsafe fn tls_get(key: TlsKey) -> *mut libc::c_void {
        libc::pthread_getspecific(key.0 as libc::pthread_key_t)
    }

    pub unsafe fn tls_set(key: TlsKey, value: *mut libc::c_void) -> io::Result<()> {
        match libc::pthread_setspecific(key.0 as libc::pthread_key_t, value) {
            0 => Ok(()),
            e => Err(io::Error::from_raw_os_error(e)),
        }
    }

    pub unsafe fn protect(addr: usize, size: usize, prot: Prot) -> io::Result<()> {
        if libc::mprotect(addr as *mut libc::c_void, size, flags(prot)) == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn flush_icache(addr: usize, len: usize) {
        #[cfg(target_vendor = "apple")]
        {
            extern "C" {
                fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
            }
            unsafe { sys_icache_invalidate(addr as *mut libc::c_void, len) };
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            extern "C" {
                fn __clear_cache(start: *mut libc::c_char, end: *mut libc::c_char);
            }
            unsafe { __clear_cache(addr as *mut libc::c_char, (addr + len) as *mut libc::c_char) };
        }
    }
}

#[cfg(windows)]
mod imp {
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
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn flush_icache(addr: usize, len: usize) {
        unsafe { FlushInstructionCache(GetCurrentProcess(), addr as *const c_void, len) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_protect_free() {
        let page = page_size();
        assert!(page.is_power_of_two() && page >= 4096);
        unsafe {
            let p = alloc(4 * page, Prot::ReadWrite).unwrap();
            assert!(std::slice::from_raw_parts(p, 4 * page).iter().all(|&b| b == 0));
            *p.add(page) = 7;
            protect(p as usize + page, page, Prot::Read).unwrap();
            assert_eq!(*p.add(page), 7);
            protect(p as usize + page, page, Prot::ReadWrite).unwrap();
            *p.add(page) = 8;
            flush_icache(p as usize, page);
            free(p, 4 * page);
        }
    }

    #[test]
    fn tls_keys_and_destructors() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static FREED: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn dtor(v: *mut std::ffi::c_void) {
            FREED.fetch_add(v as usize, Ordering::SeqCst);
        }
        let k = tls_create(Some(dtor)).unwrap();
        assert!(tls_get(k).is_null());
        tls_set(k, 7 as *mut _).unwrap();
        assert_eq!(tls_get(k) as usize, 7);
        // Another thread has its own value, and its destructor runs when it exits.
        std::thread::spawn(move || {
            assert!(tls_get(k).is_null());
            tls_set(k, 5 as *mut _).unwrap();
        })
        .join()
        .unwrap();
        assert_eq!(FREED.load(Ordering::SeqCst), 5);
        assert_eq!(tls_get(k) as usize, 7);
        tls_set(k, std::ptr::null_mut()).unwrap();
        unsafe { tls_delete(k) };
    }

    #[test]
    fn reserve_commit_release() {
        // 64 KiB: Windows' allocation granularity, so the range can be reserved again at the same address.
        let size = 16 * 0x10000;
        unsafe {
            let r = reserve(At::Anywhere, size).unwrap();
            assert_eq!(r % 0x10000, 0);
            // Part of it in the middle, as the loader commits main and leaves the module gap reserved.
            commit(r + 0x10000, 0x10000, Prot::ReadWrite).unwrap();
            let p = (r + 0x10000) as *mut u8;
            assert!(std::slice::from_raw_parts(p, 0x10000).iter().all(|&b| b == 0));
            *p.add(0x8000) = 5;
            // Discarded pages stay usable; decommitted ones come back fresh when committed again.
            discard(r + 0x10000, 0x10000);
            *p.add(0x8000) = 6;
            decommit(r + 0x10000, 0x10000);
            commit(r + 0x10000, 0x10000, Prot::ReadWrite).unwrap();
            assert_eq!(*p.add(0x8000), 0);
            // An occupied address cannot be had exactly; nearby lands elsewhere.
            assert!(reserve(At::Exactly(r), size).is_err());
            let other = reserve(At::Near(r), size).unwrap();
            assert_ne!(other, r);
            release(other, size);
            release(r, size);
            // Released: the same address is free again.
            assert_eq!(reserve(At::Exactly(r), size).unwrap(), r);
            release(r, size);
        }
    }
}
