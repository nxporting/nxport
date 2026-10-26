// Running lifted guest code (nxport-lifter's C, compiled into one library): loading the library, handing it the
// runtime's callbacks, and entering it (docs/windows-port.md). The library's own side, the dispatcher, is the lifter's
// runtime/rt.c.
//
// Guest code that branches to an address no lifted module contains reaches `call`: HLE functions (through their
// trampolines or their own addresses, hle_abi) run through their adapters; anything else goes to the handler the
// embedder gave, as do supervisor calls and traps.

use std::ffi::c_void;
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

thread_local! {
    /// The CPU state of the guest code this thread called into the host from (host_call), for guest::regs.
    static CURRENT: std::cell::Cell<*mut Cpu> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// The guest CPU state of the host call this thread is in, or null outside one.
pub fn current() -> *mut Cpu {
    CURRENT.with(|c| c.get())
}

unsafe extern "C" fn host_call(c: *mut Cpu) {
    let prev = CURRENT.with(|cur| cur.replace(c));
    let c = &mut *c;
    match crate::hle_abi::entry(c.pc) {
        Some(h) => h.call(c),
        None => (handlers().unknown)(c),
    }
    CURRENT.with(|cur| cur.set(prev));
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
        let lib = crate::os::Library::open(path)?;
        let sym = |name: &str| -> Result<*mut c_void, String> {
            let p = lib.symbol(name);
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
        // The registers a guest caller relies on across a call are callee-saved: what the callee does not preserve is
        // the return address, which is the host's for this call.
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

static LIBRARY: OnceLock<GuestLibrary> = OnceLock::new();

/// Loads the process's guest library (GuestLibrary::load), which guest::call and guest threads then run in.
pub fn install(path: &std::path::Path, modules: &[&str], handlers: Handlers) -> Result<(), String> {
    let lib = GuestLibrary::load(path, modules, handlers)?;
    LIBRARY.set(lib).map_err(|_| "lifted: a guest library is already installed".to_string())
}

/// The installed guest library.
pub fn library() -> &'static GuestLibrary {
    LIBRARY.get().expect("lifted: no guest library installed")
}

thread_local! {
    /// This thread's guest CPU state, made when it first runs guest code.
    static THREAD_CPU: std::cell::Cell<*mut Cpu> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// The calling thread's guest CPU state: on first use zeroed, with its guest TLS block in TPIDR_EL0 and its Horizon
/// thread-local region in TPIDRRO_EL0 (guest::tls), and no stack yet (sp 0). It lives as long as the thread.
#[cfg(nx_runtime)]
pub fn thread_cpu() -> &'static mut Cpu {
    let mut c = THREAD_CPU.with(|c| c.get());
    if c.is_null() {
        // Cpu is plain data: all zeros is a valid state.
        let mut cpu: Box<Cpu> = unsafe { Box::new(std::mem::zeroed()) };
        cpu.tpidr = crate::guest::tls::thread_block() as u64;
        cpu.tpidrro = crate::guest::tls::current_fake_tlr() as u64;
        c = Box::into_raw(cpu);
        THREAD_CPU.with(|t| t.set(c));
    }
    unsafe { &mut *c }
}
