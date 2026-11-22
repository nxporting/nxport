// The host operating system's memory primitives, for the runtime. On Unix hosts each is the libc / Darwin call;
// Windows gets the VirtualAlloc family.
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

/// Fills `buf` from the host's cryptographically secure random source.
pub fn random_bytes(buf: &mut [u8]) {
    imp::random_bytes(buf)
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
#[path = "unix.rs"]
mod imp;

#[cfg(windows)]
#[path = "windows.rs"]
mod imp;

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
