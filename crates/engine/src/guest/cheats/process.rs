// The game as the cheat VM sees it (vm::Process): dmnt's debug handle on a Switch, guest memory in this process here.
//
// Regions (dmnt reads them from the process: ldr's module list for main, svcGetInfo for the others):
// - main: main's image as guest::loader placed it;
// - heap: the nn::os memory heap, through the layout a Switch would have given the game's memory blocks
//   (horizon::heap_layout); its blocks are separate allocations here;
// - alias and aslr: the regions svcGetInfo reports to guest code here (horizon::svc), reserved address space;
// - absolute addresses: the process's own. Guest pointers are host pointers on every platform (native code, and
//   lifted code at the guest's addresses), so the pointer chains cheats follow lead to the right places.
//
// A debugger reads and writes any mapped memory whatever its protection, and fails elsewhere. Here an access first asks
// the host which pages are mapped, so a cheat following a stale pointer gets a failed read instead of a crash. Writes:
// - into pages write tracking or a GPU backend guards (dirty_track::before_host_write), as the host kernel's writes are
//   prepared, so the write counts as the CPU write it is;
// - into main's read-only pages (code and read-only data) as a debugger does. Natively through a second, writable
//   mapping of the page, so the code stays executable for the threads running it; code mapped from a signed image
//   (iOS: cpu/aot.rs) cannot change, and such writes are dropped. On Windows through WriteProcessMemory, and code there
//   is lifted: a write to main's code goes through live_patch, which runs the changed functions in the interpreter
//   from then on;
// - a write that leaves memory as it was is skipped (nothing changes, and guarded pages stay guarded).
// The check and the access are not one step: memory the game unmaps in between still faults (memory blocks cannot,
// see heap_layout::with_host_address).

use super::vm::{Address, Process, Region};
use std::collections::HashSet;

pub struct GuestProcess {
    main: usize,
    main_size: usize,
    text_size: usize,
    /// Messages already logged (each once).
    said: HashSet<String>,
}

impl GuestProcess {
    pub fn new(main: usize, main_size: usize, text_size: usize) -> Self {
        GuestProcess { main, main_size, text_size, said: HashSet::new() }
    }

    pub(super) fn say_once(&mut self, message: String) {
        if self.said.insert(message.clone()) {
            println!("{message}");
        }
    }

    /// Runs `access` on the host address of `at` for `len` bytes, if there is one.
    fn with_address<R>(&self, at: Address, len: usize, access: impl FnOnce(usize) -> R) -> Option<R> {
        let host = match at.region {
            Region::Main => self.main.wrapping_add(at.offset as usize),
            Region::Heap => return crate::horizon::heap_layout::with_host_address(at.offset, len, access),
            Region::Alias => crate::horizon::svc::alias_region_extents().0.wrapping_add(at.offset as usize),
            Region::Aslr => crate::horizon::svc::aslr_region_extents().0.wrapping_add(at.offset as usize),
            Region::Absolute => at.offset as usize,
        };
        Some(access(host))
    }

    fn in_main_text(&self, addr: usize, len: usize) -> bool {
        addr < self.main + self.text_size && addr + len > self.main
    }

    fn in_main(&self, addr: usize, len: usize) -> bool {
        addr >= self.main && addr + len <= self.main + self.main_size
    }

    /// Writes into main's read-only pages; false if they cannot change.
    fn write_read_only(&mut self, addr: usize, data: &[u8]) -> bool {
        #[cfg(nx_native)]
        {
            if crate::cpu::aot::loading() && crate::cpu::aot::in_region(addr, data.len()) {
                self.say_once(format!(
                    "[!] Cheats: a cheat writes main's code at main+{:#x}, which comes from the signed code image here: not \
                     applied",
                    addr - self.main
                ));
                return false;
            }
            if unsafe { !native::write_through_alias(addr, data) } {
                return false;
            }
            crate::os::flush_icache(addr, data.len());
        }
        #[cfg(windows)]
        {
            if unsafe { !windows::write_process_memory(addr, data) } {
                return false;
            }
            if self.in_main_text(addr, data.len()) {
                if !crate::live_patch::enabled() {
                    self.say_once(format!(
                        "[!] Cheats: a cheat writes main's code at main+{:#x}, which the lifted code does not see without \
                         live patches",
                        addr - self.main
                    ));
                }
                crate::live_patch::code_written(addr, data.len());
            }
        }
        true
    }
}

impl Process for GuestProcess {
    fn read(&mut self, at: Address, out: &mut [u8]) -> bool {
        self.with_address(at, out.len(), |addr| {
            if !readable(addr, out.len()) {
                return false;
            }
            unsafe { std::ptr::copy_nonoverlapping(addr as *const u8, out.as_mut_ptr(), out.len()) };
            true
        })
        .unwrap_or(false)
    }

    fn write(&mut self, at: Address, data: &[u8]) {
        let Some(addr) = self.with_address(at, data.len(), |addr| {
            if !readable(addr, data.len()) {
                return None;
            }
            let now = unsafe { std::slice::from_raw_parts(addr as *const u8, data.len()) };
            if now == data {
                return None;
            }
            unsafe { crate::guest::dirty_track::before_host_write(addr, data.len()) };
            if writable(addr, data.len()) {
                unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), addr as *mut u8, data.len()) };
                // Main's code may be writable natively (no AOT image, before a module's code is sealed); instruction
                // fetch must see it either way.
                if self.in_main_text(addr, data.len()) {
                    crate::os::flush_icache(addr, data.len());
                    #[cfg(windows)]
                    crate::live_patch::code_written(addr, data.len());
                }
                return None;
            }
            Some(addr)
        })
        .flatten() else {
            return;
        };
        // Read-only: only main's pages are written through their protection (dmnt writes any, but elsewhere here a
        // read-only page is the host's own or a guard nothing should defeat).
        if self.in_main(addr, data.len()) {
            self.write_read_only(addr, data);
        } else {
            self.say_once(format!("[!] Cheats: a cheat writes read-only memory at {addr:#x}: not applied"));
        }
    }

    fn pause(&mut self) {
        self.say_once("[!] Cheats: a cheat pauses the game (opcode FF0), which cannot be done here: ignored".into());
    }

    fn resume(&mut self) {
        self.say_once("[!] Cheats: a cheat resumes the game (opcode FF1), which cannot be done here: ignored".into());
    }

    fn log(&mut self, id: u32, value: u64) {
        println!("[cheat-log] {id:08x}: {value:016x}");
    }
}

fn readable(addr: usize, len: usize) -> bool {
    if addr < 0x10000 || addr.checked_add(len).is_none() {
        return false;
    }
    #[cfg(nx_native)]
    return unsafe { crate::cpu::crash_handler::readable_len(addr, len) } == len;
    #[cfg(windows)]
    return crate::os::readable_len(addr, len) == len;
}

fn writable(addr: usize, len: usize) -> bool {
    #[cfg(nx_native)]
    return native::writable(addr, len);
    #[cfg(windows)]
    return crate::os::writable_len(addr, len) == len;
}

#[cfg(nx_native)]
mod native {
    const PAGE: usize = 0x4000;

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
        fn mach_vm_protect(task: u32, address: u64, size: u64, set_maximum: i32, new_protection: i32) -> i32;
        fn mach_vm_deallocate(task: u32, address: u64, size: u64) -> i32;
    }

    pub fn writable(addr: usize, len: usize) -> bool {
        let mut page = addr & !(PAGE - 1);
        while page < addr + len {
            match unsafe { crate::cpu::crash_handler::vm_region(page) } {
                Some((start, end, prot)) if start <= page && prot & 2 != 0 => page = end,
                _ => return false,
            }
        }
        true
    }

    /// Writes `data` at `addr` through a writable mapping of the same pages, leaving their own protection alone.
    pub unsafe fn write_through_alias(addr: usize, data: &[u8]) -> bool {
        let start = addr & !(PAGE - 1);
        let len = ((addr + data.len() + PAGE - 1) & !(PAGE - 1)) - start;
        let (mut alias, mut cur, mut max) = (0u64, 0i32, 0i32);
        const VM_FLAGS_ANYWHERE: i32 = 1;
        const VM_INHERIT_NONE: u32 = 2;
        let kr = mach_vm_remap(
            mach_task_self_,
            &mut alias,
            len as u64,
            0,
            VM_FLAGS_ANYWHERE,
            mach_task_self_,
            start as u64,
            0,
            &mut cur,
            &mut max,
            VM_INHERIT_NONE,
        );
        if kr != 0 {
            println!("[!] Cheats: no second mapping of {start:#x} (mach_vm_remap {kr}): write not applied");
            return false;
        }
        let ok = mach_vm_protect(mach_task_self_, alias, len as u64, 0, libc::PROT_READ | libc::PROT_WRITE) == 0;
        if ok {
            std::ptr::copy_nonoverlapping(data.as_ptr(), (alias as usize + (addr - start)) as *mut u8, data.len());
        } else {
            println!("[!] Cheats: {start:#x} cannot be written (maximum protection {max:#x}): write not applied");
        }
        mach_vm_deallocate(mach_task_self_, alias, len as u64);
        ok
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;

    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn WriteProcessMemory(process: *mut c_void, base: *mut c_void, buffer: *const c_void, size: usize, written: *mut usize) -> i32;
    }

    /// A debugger's write: WriteProcessMemory makes read-only pages writable for the copy and restores them.
    pub unsafe fn write_process_memory(addr: usize, data: &[u8]) -> bool {
        let mut written = 0;
        WriteProcessMemory(GetCurrentProcess(), addr as *mut c_void, data.as_ptr() as *const c_void, data.len(), &mut written) != 0
            && written == data.len()
    }
}
