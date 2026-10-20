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
}
