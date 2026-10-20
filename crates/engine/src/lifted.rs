// Running lifted guest code (nxport-lifter's C, compiled into one library): loading the library, handing it the
// runtime's callbacks, and entering it (docs/windows-port.md). The library's own side, the dispatcher, is the lifter's
// runtime/rt.c.
//
// Guest code that branches to an address no lifted module contains reaches `call`: HLE functions (through their
// trampolines or their own addresses, hle_abi) run through their adapters; anything else goes to the handler the
// embedder gave, as do supervisor calls and traps.

use std::ffi::{c_char, c_void, CString};
use std::sync::OnceLock;

use nxport_lifter::abi::Cpu;

/// What the embedder handles for lifted code.
pub struct Handlers {
    /// `svc #imm` at `pc`.
    pub svc: fn(&mut Cpu, u32, u64),
    /// brk (kind 0) or an undefined / not lifted instruction (kind 1) at `pc`; does not return.
    pub trap: fn(&mut Cpu, u32, u32, u64) -> !,
    /// A branch to `cpu.pc`, which is neither lifted code nor an HLE function; returns with `cpu.pc` where to go on.
    pub unknown: fn(&mut Cpu),
    /// The guest's CNTPCT_EL0 / CNTVCT_EL0.
    pub ticks: fn() -> u64,
}

static HANDLERS: OnceLock<Handlers> = OnceLock::new();

#[repr(C)]
struct RtHost {
    call: unsafe extern "C" fn(*mut Cpu),
    svc: unsafe extern "C" fn(*mut Cpu, u32, u64),
    trap: unsafe extern "C" fn(*mut Cpu, u32, u32, u64),
    ticks: unsafe extern "C" fn() -> u64,
}

fn handlers() -> &'static Handlers {
    HANDLERS.get().expect("lifted: no handlers installed")
}

unsafe extern "C" fn host_call(c: *mut Cpu) {
    let c = &mut *c;
    match crate::hle_abi::entry(c.pc) {
        Some(h) => h.call(c),
        None => (handlers().unknown)(c),
    }
}

unsafe extern "C" fn host_svc(c: *mut Cpu, imm: u32, pc: u64) {
    (handlers().svc)(&mut *c, imm, pc)
}

unsafe extern "C" fn host_trap(c: *mut Cpu, kind: u32, imm: u32, pc: u64) {
    (handlers().trap)(&mut *c, kind, imm, pc)
}

unsafe extern "C" fn host_ticks() -> u64 {
    (handlers().ticks)()
}

type Enter = unsafe extern "C" fn(*mut Cpu);

/// A loaded guest library.
pub struct GuestLibrary {
    enter: Enter,
}

impl GuestLibrary {
    /// Loads the library at `path` and registers the function tables of `modules` (the names the lifter was given).
    /// The first library loaded installs `handlers`; one per process.
    pub fn load(path: &std::path::Path, modules: &[&str], handlers: Handlers) -> Result<GuestLibrary, String> {
        if HANDLERS.set(handlers).is_err() {
            return Err("lifted: a guest library is already loaded".into());
        }
        let lib = dl::open(path)?;
        let sym = |name: &str| -> Result<*mut c_void, String> {
            let c = CString::new(name).unwrap();
            let p = unsafe { dl::sym(lib, c.as_ptr()) };
            if p.is_null() {
                Err(format!("{}: no symbol {name}", path.display()))
            } else {
                Ok(p)
            }
        };
        unsafe {
            let init: unsafe extern "C" fn(*const RtHost) = std::mem::transmute(sym("nx_rt_init")?);
            let add: unsafe extern "C" fn(*const u64, *const c_void, u64) -> i32 =
                std::mem::transmute(sym("nx_rt_add_module")?);
            let host = RtHost { call: host_call, svc: host_svc, trap: host_trap, ticks: host_ticks };
            init(&host);
            for m in modules {
                let count = *(sym(&format!("{m}_fn_count"))? as *const u64);
                let addrs = sym(&format!("{m}_fn_addrs"))? as *const u64;
                let fns = sym(&format!("{m}_fn_ptrs"))? as *const c_void;
                if add(addrs, fns, count) != 0 {
                    return Err(format!("lifted: too many modules ({m})"));
                }
            }
            Ok(GuestLibrary { enter: std::mem::transmute::<*mut c_void, Enter>(sym("nx_rt_enter")?) })
        }
    }

    /// Runs guest code from `cpu.pc` until it returns to `RETURN_TO_HOST` (the caller sets x30 to it).
    /// # Safety
    /// `cpu` is a guest thread's state, with guest memory mapped where its code expects it.
    pub unsafe fn enter(&self, cpu: &mut Cpu) {
        (self.enter)(cpu)
    }

    /// Calls guest function `addr` with integer arguments on the guest stack `cpu.sp`, as guest::call does natively;
    /// returns x0.
    /// # Safety
    /// As `enter`.
    pub unsafe fn call(&self, cpu: &mut Cpu, addr: u64, args: &[u64]) -> u64 {
        assert!(args.len() <= 8, "lifted call: {} arguments", args.len());
        let saved_lr = cpu.x[30];
        cpu.x[..args.len()].copy_from_slice(args);
        cpu.x[30] = nxport_lifter::abi::RETURN_TO_HOST;
        cpu.pc = addr;
        self.enter(cpu);
        cpu.x[30] = saved_lr;
        cpu.x[0]
    }
}

// Dynamic libraries, through the host's loader.
mod dl {
    use super::*;

    #[cfg(windows)]
    pub fn open(path: &std::path::Path) -> Result<*mut c_void, String> {
        use std::os::windows::ffi::OsStrExt;
        extern "system" {
            fn LoadLibraryW(name: *const u16) -> *mut c_void;
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let h = unsafe { LoadLibraryW(wide.as_ptr()) };
        if h.is_null() {
            Err(format!("{}: {}", path.display(), std::io::Error::last_os_error()))
        } else {
            Ok(h)
        }
    }

    #[cfg(windows)]
    pub unsafe fn sym(lib: *mut c_void, name: *const c_char) -> *mut c_void {
        extern "system" {
            fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        }
        GetProcAddress(lib, name)
    }

    #[cfg(unix)]
    pub fn open(path: &std::path::Path) -> Result<*mut c_void, String> {
        use std::os::unix::ffi::OsStrExt;
        let c = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if h.is_null() {
            Err(format!("{}: {}", path.display(), unsafe {
                std::ffi::CStr::from_ptr(libc::dlerror()).to_string_lossy()
            }))
        } else {
            Ok(h)
        }
    }

    #[cfg(unix)]
    pub unsafe fn sym(lib: *mut c_void, name: *const c_char) -> *mut c_void {
        libc::dlsym(lib, name)
    }
}
