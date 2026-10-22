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

/// Releases a reservation: `addr` and `size` as `reserve` returned and was asked for.
/// # Safety
/// Nothing uses the range any more.
pub unsafe fn release(addr: usize, size: usize) {
    imp::release(addr, size)
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

    pub unsafe fn release(addr: usize, size: usize) {
        libc::munmap(addr as *mut libc::c_void, size);
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

    pub unsafe fn release(addr: usize, _size: usize) {
        VirtualFree(addr as *mut c_void, 0, MEM_RELEASE);
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
