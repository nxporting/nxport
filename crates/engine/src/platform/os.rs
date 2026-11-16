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

/// As alloc, for the guest's large memory block (TotK's heap, 2.9 GB, about half of it ever touched), released with free.
/// Windows charges commit for a page from its commit on, touched or not, so there this block's pages are committed a
/// chunk at a time, by the first access to each: see imp::heap. Elsewhere pages have no such charge.
/// # Safety
/// As for alloc.
pub unsafe fn alloc_on_demand(size: usize) -> io::Result<*mut u8> {
    imp::alloc_on_demand(size)
}

/// Bytes free for this user on the file system holding `dir`.
#[cfg(unix)]
pub fn free_space(dir: &std::path::Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(s.f_bavail as u64 * s.f_frsize as u64)
}

/// An unlinked sparse file of `size` bytes in `dir`, for map_file / punch_hole. Refused when the file system has less
/// free space than `need`: its blocks are taken as the guest writes, and a full disk would fail those writes.
#[cfg(unix)]
pub fn sparse_file_with_room(size: usize, need: u64, dir: &std::path::Path) -> io::Result<std::fs::File> {
    let free = free_space(dir)?;
    if free < need {
        return Err(io::Error::other(format!("{} MB free, {} MB needed", free >> 20, need >> 20)));
    }
    sparse_file(size, dir)
}

/// An unlinked sparse file of `size` bytes in `dir`, for map_file / punch_hole.
#[cfg(unix)]
pub fn sparse_file(size: usize, dir: &std::path::Path) -> io::Result<std::fs::File> {
    let path = dir.join(format!("nxport-arena-{}-{size:x}", std::process::id()));
    let f = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
    let _ = std::fs::remove_file(&path);
    f.set_len(size as u64)?;
    Ok(f)
}

/// Maps [offset, offset + size) of `fd` read-write and shared over [addr, addr + size) (reserved by us).
#[cfg(unix)]
pub unsafe fn map_file(addr: usize, size: usize, fd: i32, offset: usize) -> io::Result<()> {
    let p = libc::mmap(
        addr as *mut libc::c_void,
        size,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_FIXED | libc::MAP_SHARED,
        fd,
        offset as libc::off_t,
    );
    if p == libc::MAP_FAILED {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Frees the file's blocks in [offset, offset + size): they read zero again and take no disk space.
#[cfg(target_vendor = "apple")]
pub unsafe fn punch_hole(fd: i32, offset: usize, size: usize) {
    #[repr(C)]
    struct FPunchhole {
        fp_flags: u32,
        reserved: u32,
        fp_offset: libc::off_t,
        fp_length: libc::off_t,
    }
    const F_PUNCHHOLE: libc::c_int = 99;
    let a = FPunchhole { fp_flags: 0, reserved: 0, fp_offset: offset as libc::off_t, fp_length: size as libc::off_t };
    libc::fcntl(fd, F_PUNCHHOLE, &a);
}

/// alloc_on_demand backed by an unlinked sparse file in `dir` instead of anonymous memory: its written pages are file
/// pages the kernel can write out, not anonymous memory counted whole against the process's limit (iOS ends an app whose
/// footprint reaches it). Pages read zero until written, as anonymous ones do.
#[cfg(unix)]
pub unsafe fn alloc_file_backed(size: usize, dir: &std::path::Path) -> io::Result<*mut u8> {
    use std::os::unix::io::AsRawFd;
    // Room for every page of the block, plus 1 GB for the rest of the system.
    let f = sparse_file_with_room(size, size as u64 + (1 << 30), dir)?;
    let p = libc::mmap(std::ptr::null_mut(), size, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0);
    if p == libc::MAP_FAILED {
        Err(io::Error::last_os_error())
    } else {
        Ok(p as *mut u8)
    }
}

/// Commits now what alloc_on_demand left for its first access, in [addr, addr + len). For the accesses no fault can serve:
/// a buffer the kernel fills (it fails with no exception), memory handed to a driver to import. Nothing elsewhere.
/// # Safety
/// As for touching the range.
pub unsafe fn ensure_committed(addr: usize, len: usize) {
    imp::ensure_committed(addr, len)
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

/// A dynamic library, loaded for the rest of the process (the lifted guest library, libopus).
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

/// Ends the calling thread without returning or unwinding (guest code that exits its thread, svcExitThread).
pub fn exit_thread() -> ! {
    imp::exit_thread()
}

/// The host library containing `addr` and the nearest symbol before it, as far as the host's loader knows (logs).
/// Windows names only the library.
pub fn address_owner(addr: usize) -> (Option<String>, Option<String>) {
    imp::address_owner(addr)
}

/// The image of the loaded module containing `addr`: (start, end).
#[cfg(windows)]
pub fn image_range(addr: usize) -> Option<(usize, usize)> {
    imp::image_range(addr)
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

/// How many bytes from `addr` (up to `max`) can be read without faulting. Native hosts ask cpu::crash_handler, which
/// also knows the guest's page guards.
#[cfg(windows)]
pub fn readable_len(addr: usize, max: usize) -> usize {
    imp::readable_len(addr, max)
}

/// Prints what an exception nothing handled was, where (module and offset) and the thread's stack, before the process
/// ends (native hosts report crashes in cpu::crash_handler).
#[cfg(windows)]
pub fn report_crashes() {
    imp::report_crashes()
}

/// The committed parts of [addr, addr + len) that are not writable, in one pass over the memory regions.
#[cfg(windows)]
pub fn read_only_ranges(addr: usize, len: usize) -> Vec<(usize, usize)> {
    imp::read_only_ranges(addr, len)
}

/// How many bytes from `addr` (up to `max`) can be written without faulting.
#[cfg(windows)]
pub fn writable_len(addr: usize, max: usize) -> usize {
    imp::writable_len(addr, max)
}

/// An access fault at `addr` (a write if `write`) by the instruction at `pc`. True if handled: the access is retried.
pub type FaultHandler = unsafe fn(addr: usize, write: bool, pc: usize) -> bool;

/// Has `handler` see every access fault in the process before anything else does (the page guards: write tracking, the
/// GPU backends' guards). Native hosts handle faults in cpu::crash_handler, which also rewrites the guest's registers.
#[cfg(windows)]
pub fn on_access_fault(handler: FaultHandler) {
    imp::on_access_fault(handler)
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
        let cpu = crate::lifted::existing_thread_cpu();
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

    /// The heap committed by first access: faults serve reads and writes, a guard over chunks not committed yet holds, and
    /// committing a neighbouring chunk does not lift it.
    #[cfg(windows)]
    #[test]
    fn heap_committed_on_first_access() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static GUARD_FAULTS: AtomicUsize = AtomicUsize::new(0);
        unsafe fn guard(addr: usize, _write: bool, _pc: usize) -> bool {
            GUARD_FAULTS.fetch_add(1, Ordering::SeqCst);
            protect(addr & !0xfff, 0x1000, Prot::ReadWrite).is_ok()
        }
        const MB: usize = 1 << 20;
        unsafe {
            on_access_fault(guard);
            let size = 300 * MB + 0x4000;
            let base = alloc_on_demand(size).unwrap() as usize;
            // First access, a read and a write, in chunks not committed.
            assert_eq!(*((base + 5 * MB) as *const u8), 0);
            *((base + 3 * MB + 8) as *mut u8) = 1;
            assert_eq!(GUARD_FAULTS.load(Ordering::SeqCst), 0);
            assert_eq!(readable_len(base, size), size, "not committed counts as readable");
            assert_eq!(writable_len(base + 0x1000, 8 * MB), 8 * MB);
            // A protection over chunks committed and not: the whole range holds, and its contents survive.
            protect(base + 3 * MB, 2 * MB + 0x1000, Prot::Read).unwrap();
            assert_eq!(*((base + 3 * MB + 8) as *const u8), 1);
            *((base + 6 * MB) as *mut u8) = 2; // another chunk's first access
            *((base + 4 * MB) as *mut u8) = 3;
            assert_eq!(GUARD_FAULTS.load(Ordering::SeqCst), 1, "a chunk's commit lifted a guard in it");
            *((base + 3 * MB + 8) as *mut u8) = 4;
            assert_eq!(GUARD_FAULTS.load(Ordering::SeqCst), 2);
            // Committed ahead for the kernel: a read of the file fills a chunk no one has touched.
            ensure_committed(base + 7 * MB, MB);
            assert_eq!(read_only_ranges(base + 7 * MB, MB), vec![]);
            assert!(imp::committed(base + 7 * MB + 0x1000));
            assert!(!imp::committed(base + 2 * MB), "a chunk nothing touched is not committed");
            // The partial last chunk, then past the end: not the block's.
            *((base + 300 * MB) as *mut u8) = 5;
            assert_eq!(readable_len(base + size, 0x1000), 0);
            // Freed and allocated again: fresh zeros, chunks commit afresh. A second block commits whole.
            free(base as *mut u8, size);
            assert_eq!(readable_len(base, 0x1000), 0);
            let again = alloc_on_demand(size).unwrap() as usize;
            assert_eq!(*((again + 4 * MB) as *const u8), 0);
            let other = alloc_on_demand(size).unwrap() as usize;
            assert!(imp::committed(other + 7 * MB));
            free(other as *mut u8, size);
            free(again as *mut u8, size);
            assert_eq!(GUARD_FAULTS.load(Ordering::SeqCst), 2);
        }
    }

    #[test]
    fn reserve_commit_release() {
        // 64 KiB: Windows' allocation granularity, so the range can be reserved again at the same address.
        let size = 16 * 0x10000;
        unsafe {
            let r = reserve(At::Anywhere, size).unwrap();
            #[cfg(windows)]
            assert_eq!(r % 0x10000, 0);
            #[cfg(not(windows))]
            assert_eq!(r % page_size(), 0);
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
