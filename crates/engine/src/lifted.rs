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
    resolve_hle: unsafe extern "C" fn(u64) -> *const crate::hle_abi::Hle,
    call_hle: unsafe extern "C" fn(*const crate::hle_abi::Hle, *mut Cpu),
    code_changed: unsafe extern "C" fn(u64),
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
    #[cfg(windows)]
    let hooked = crate::live_patch::enabled() && crate::live_patch::hle_hooked(c.pc);
    #[cfg(not(windows))]
    let hooked = false;
    match crate::hle_abi::entry(c.pc).filter(|_| !hooked) {
        Some(h) => h.call(c),
        None => (handlers().unknown)(c),
    }
    CURRENT.with(|cur| cur.set(prev));
}

/// The dispatcher's cache of HLE targets (runtime/rt.c): what `host_call` would find for `pc`.
unsafe extern "C" fn host_resolve_hle(pc: u64) -> *const crate::hle_abi::Hle {
    #[cfg(windows)]
    if crate::live_patch::enabled() && crate::live_patch::hle_hooked(pc) {
        return std::ptr::null();
    }
    crate::hle_abi::entry_ref(pc).map_or(std::ptr::null(), |h| h as *const _)
}

unsafe extern "C" fn host_call_hle(h: *const crate::hle_abi::Hle, c: *mut Cpu) {
    let prev = CURRENT.with(|cur| cur.replace(c));
    // The dispatcher's cache keeps what it resolved: a mod may have hooked this trampoline since.
    #[cfg(windows)]
    if crate::live_patch::enabled() && crate::live_patch::hle_hooked((*c).pc) {
        (handlers().unknown)(&mut *c);
        CURRENT.with(|cur| cur.set(prev));
        return;
    }
    (*h).call(&mut *c);
    CURRENT.with(|cur| cur.set(prev));
}

/// A guest `ic ivau`: what a plugin mod wrote through a mapping it keeps takes effect now, not at its next svc.
unsafe extern "C" fn host_code_changed(addr: u64) {
    let _ = addr;
    #[cfg(windows)]
    if crate::live_patch::enabled() {
        let pages = crate::horizon::svc::flush_code_view_at(addr as usize);
        if pages > 0 {
            println!("[+] Live patches: {pages} code pages written, seen at a cache flush");
        }
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

#[derive(Clone, Copy)]
struct FnTable {
    count: u64,
    addrs: *const u64,
    fns: *const usize,
}

impl FnTable {
    unsafe fn entries(&self) -> (&[u64], &[usize]) {
        (std::slice::from_raw_parts(self.addrs, self.count as usize), std::slice::from_raw_parts(self.fns, self.count as usize))
    }
}

/// An address in the overlay library's code, 0 without one.
static OVERLAY_CODE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Calls an HLE function for code outside the guest library, as host_call_hle does.
#[cfg(windows)]
pub unsafe fn call_hle(h: crate::hle_abi::Hle, c: &mut Cpu) {
    let prev = CURRENT.with(|cur| cur.replace(c));
    h.call(c);
    CURRENT.with(|cur| cur.set(prev));
}

/// The embedder's svc handler, for code outside the guest library that makes one.
#[cfg(windows)]
pub fn svc_handler() -> fn(&mut Cpu, u32, u64) {
    handlers().svc
}

/// An address in the overlay library's code, if one is loaded.
pub fn overlay_code() -> Option<usize> {
    Some(OVERLAY_CODE.load(std::sync::atomic::Ordering::Relaxed)).filter(|&a| a != 0)
}

/// Points the base library's functions at the overlay's versions: base code calls its functions directly, without the
/// dispatcher. Before any guest code runs. Functions are 16-byte aligned, so `jmp [rip]; address` (14 bytes) fits each.
unsafe fn redirect_entries(base: &[FnTable], over: &[FnTable]) -> Result<(), String> {
    let (mut replaced, mut added) = (0usize, 0usize);
    for o in over {
        let (addrs, fns) = o.entries();
        for (&a, &to) in addrs.iter().zip(fns) {
            let Some(from) = base.iter().find_map(|b| {
                let (ba, bf) = b.entries();
                ba.binary_search(&a).ok().map(|i| bf[i])
            }) else {
                added += 1;
                continue;
            };
            if from % 16 != 0 {
                return Err(format!("lifted: the function at {a:#x} is not 16-byte aligned"));
            }
            let page = from & !0xfff;
            let span = ((from + 14 + 0xfff) & !0xfff) - page;
            crate::os::protect(page, span, crate::os::Prot::ReadWriteExec).map_err(|e| e.to_string())?;
            let mut jmp = [0xffu8, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            jmp[6..14].copy_from_slice(&to.to_le_bytes());
            std::ptr::copy_nonoverlapping(jmp.as_ptr(), from as *mut u8, 14);
            crate::os::protect(page, span, crate::os::Prot::ReadExec).map_err(|e| e.to_string())?;
            crate::os::flush_icache(from, 14);
            replaced += 1;
        }
    }
    println!("[+] Lifted: overlay replaces {replaced} functions and adds {added}");
    Ok(())
}

/// A loaded guest library.
pub struct GuestLibrary {
    enter: Enter,
}

impl GuestLibrary {
    /// Loads the library at `path` and registers the function tables of `modules` (the names the lifter was given).
    /// The first library loaded installs `handlers`; one per process.
    pub fn load(
        path: &std::path::Path,
        modules: &[&str],
        overlay: Option<(&std::path::Path, &[&str])>,
        handlers: Handlers,
    ) -> Result<GuestLibrary, String> {
        if HANDLERS.set(handlers).is_err() {
            return Err("lifted: a guest library is already loaded".into());
        }
        let sym = |lib: &crate::os::Library, path: &std::path::Path, name: &str| -> Result<*mut c_void, String> {
            let p = lib.symbol(name);
            if p.is_null() {
                Err(format!("{}: no symbol {name}", path.display()))
            } else {
                Ok(p)
            }
        };
        unsafe {
            let tables = |lib: &crate::os::Library, path: &std::path::Path, names: &[&str]| -> Result<Vec<FnTable>, String> {
                names
                    .iter()
                    .map(|m| {
                        Ok(FnTable {
                            count: *(sym(lib, path, &format!("{m}_fn_count"))? as *const u64),
                            addrs: sym(lib, path, &format!("{m}_fn_addrs"))? as *const u64,
                            fns: sym(lib, path, &format!("{m}_fn_ptrs"))? as *const usize,
                        })
                    })
                    .collect()
            };
            let host: &'static RtHost = Box::leak(Box::new(RtHost {
                call: host_call,
                svc: host_svc,
                trap: host_trap,
                ticks: host_ticks,
                resolve_hle: host_resolve_hle,
                call_hle: host_call_hle,
                code_changed: host_code_changed,
            }));
            let lib = crate::os::Library::open(path)?;
            let base_tables = tables(&lib, path, modules)?;
            let over = overlay.map(|(p, names)| -> Result<_, String> {
                let l = crate::os::Library::open(p)?;
                let t = tables(&l, p, names)?;
                Ok((l, t, p))
            }).transpose()?;
            // Each library has its own dispatcher (its rt.c), which looks an address up in tables in order: the overlay's
            // versions first in both.
            let over_tables: &[FnTable] = over.as_ref().map_or(&[], |o| &o.1);
            let register = |l: &crate::os::Library, p: &std::path::Path| -> Result<(), String> {
                let init: unsafe extern "C" fn(*const RtHost) = std::mem::transmute(sym(l, p, "nx_rt_init")?);
                let add: unsafe extern "C" fn(*const u64, *const c_void, u64) -> i32 =
                    std::mem::transmute(sym(l, p, "nx_rt_add_module")?);
                init(host);
                for t in over_tables.iter().chain(&base_tables) {
                    if add(t.addrs, t.fns as *const c_void, t.count) != 0 {
                        return Err("lifted: too many modules".to_string());
                    }
                }
                Ok(())
            };
            register(&lib, path)?;
            #[cfg(windows)]
            if crate::settings::get().guest.live_patches {
                let dispatcher = lib.symbol("nx_rt_dispatcher");
                if dispatcher.is_null() {
                    return Err(format!("{}: too old for live patches (no nx_rt_dispatcher): lift it again", path.display()));
                }
                let get: unsafe extern "C" fn() -> usize = std::mem::transmute(dispatcher);
                let names = overlay.map_or(&[][..], |(_, n)| n).iter().chain(modules);
                let tables = over_tables.iter().chain(&base_tables).zip(names).map(|(t, name)| {
                    let (addrs, fns) = t.entries();
                    crate::live_patch::Table {
                        addrs: std::slice::from_raw_parts(addrs.as_ptr(), addrs.len()),
                        fns: std::slice::from_raw_parts(fns.as_ptr(), fns.len()),
                        scattered: name.starts_with("ov_"),
                    }
                }).collect();
                let add_of = |l: &crate::os::Library, p: &std::path::Path| -> Result<crate::live_patch::AddModule, String> {
                    Ok(std::mem::transmute(sym(l, p, "nx_rt_add_module")?))
                };
                let mut adds = vec![add_of(&lib, path)?];
                if let Some((l, _, p)) = &over {
                    adds.push(add_of(l, p)?);
                }
                crate::live_patch::enable(tables, get(), adds, host as *const RtHost as usize)?;
            }
            if let Some((l, _, p)) = &over {
                register(l, p)?;
                redirect_entries(&base_tables, over_tables)?;
                OVERLAY_CODE.store(sym(l, p, "nx_rt_enter")? as usize, std::sync::atomic::Ordering::Relaxed);
            }
            Ok(GuestLibrary { enter: std::mem::transmute::<*mut c_void, Enter>(sym(&lib, path, "nx_rt_enter")?) })
        }
    }

    /// An address in the library's code.
    pub fn entry(&self) -> usize {
        self.enter as usize
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
pub fn install(
    path: &std::path::Path,
    modules: &[&str],
    overlay: Option<(&std::path::Path, &[&str])>,
    handlers: Handlers,
) -> Result<(), String> {
    let lib = GuestLibrary::load(path, modules, overlay, handlers)?;
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

/// The guest CPU state this thread has made, if any. For the crash report, which must not make one.
pub fn existing_thread_cpu() -> *mut Cpu {
    THREAD_CPU.with(|c| c.get())
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
