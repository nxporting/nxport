// The OS layer on Unix hosts: mmap, mprotect, signals.

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

pub fn random_bytes(buf: &mut [u8]) {
    // getentropy fills at most 256 bytes a call.
    for chunk in buf.chunks_mut(256) {
        unsafe { libc::getentropy(chunk.as_mut_ptr() as *mut libc::c_void, chunk.len()) };
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

pub unsafe fn alloc_on_demand(size: usize) -> io::Result<*mut u8> {
    alloc(size, Prot::ReadWrite)
}

pub unsafe fn ensure_committed(_addr: usize, _len: usize) {}

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

pub fn exit_thread() -> ! {
    unsafe { libc::pthread_exit(std::ptr::null_mut()) }
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
