// Guest code that mods patch while the game runs, on a host running lifted code: the base library's version of a changed
// function is out of date, so its x86 entry is pointed at a stub that runs the current guest instructions in
// nxport_lifter::interp and goes back to lifted code at the next branch out. `guest.live-patches` turns this on.
//
// Not done yet: compiling a changed function in the background to replace the interpreter, which is also what code the
// interpreter declines has to wait for. Until then such code stops the game with a message.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

use nxport_lifter::abi::Cpu;
use nxport_lifter::interp::{self, Stop};

/// A guest library's function table: addresses sorted, and the x86 function of each.
#[derive(Clone, Copy)]
pub struct Table {
    pub addrs: &'static [u64],
    pub fns: &'static [usize],
    /// An overlay's table of a module's changed functions: the gaps between them belong to other functions.
    pub scattered: bool,
}

struct State {
    tables: Vec<Table>,
    /// Starts of the functions whose lifted version is out of date.
    patched: RwLock<HashSet<u64>>,
    /// x86 entries already pointed at a stub.
    redirected: Mutex<HashSet<usize>>,
    stubs: Mutex<StubPool>,
}

struct StubPool {
    next: usize,
    end: usize,
}

static STATE: OnceLock<State> = OnceLock::new();
/// The guest library's dispatcher, which the stubs' common tail continues in.
#[no_mangle]
static NX_LIVE_RT_DISPATCH: AtomicUsize = AtomicUsize::new(0);

const STUB_SIZE: usize = 32;
const STUB_POOL: usize = 1 << 20;
/// `jmp [rip]` and its address, written over a function's entry. Lifted functions are 16-byte aligned.
const ENTRY_JUMP: usize = 14;

pub fn enabled() -> bool {
    STATE.get().is_some()
}

/// Called with the guest library's tables (the overlay's first), before guest code runs.
pub fn enable(tables: Vec<Table>, rt_dispatch: usize) -> Result<(), String> {
    let pool = unsafe { crate::os::alloc(STUB_POOL, crate::os::Prot::ReadWriteExec) }.map_err(|e| format!("live patches: {e}"))?;
    NX_LIVE_RT_DISPATCH.store(rt_dispatch, Ordering::Relaxed);
    let state = State {
        tables,
        patched: RwLock::new(HashSet::new()),
        redirected: Mutex::new(HashSet::new()),
        stubs: Mutex::new(StubPool { next: pool as usize, end: pool as usize + STUB_POOL }),
    };
    STATE.set(state).map_err(|_| "live patches: already on".to_string())?;
    println!("[+] Live patches: on");
    Ok(())
}

impl State {
    /// The function holding `pc`, by the start at or below it in a table of a whole module. Code in no module's table
    /// (what a mod generates) is no function: it reaches the interpreter through Handlers::unknown.
    fn function(&self, pc: u64) -> Option<(u64, u64)> {
        self.tables.iter().filter(|t| !t.scattered).find_map(|t| {
            let (first, last) = (*t.addrs.first()?, *t.addrs.last()?);
            if pc < first || pc > last.saturating_add(0x10000) {
                return None;
            }
            let i = t.addrs.partition_point(|&a| a <= pc);
            Some((t.addrs[i - 1], t.addrs.get(i).copied().unwrap_or(u64::MAX)))
        })
    }

    /// Every lifted function starting in [start, end), in any table: an overlay may have split the base's function, and
    /// the dispatcher looks in overlays first, so each version needs its entry pointed at the interpreter.
    fn versions(&self, start: u64, end: u64) -> Vec<(u64, usize)> {
        let mut v = Vec::new();
        for t in &self.tables {
            let (lo, hi) = (t.addrs.partition_point(|&a| a < start), t.addrs.partition_point(|&a| a < end));
            v.extend((lo..hi).map(|i| (t.addrs[i], t.fns[i])));
        }
        v
    }

    fn is_start(&self, pc: u64) -> bool {
        self.tables.iter().any(|t| t.addrs.binary_search(&pc).is_ok())
    }

    /// Whether the interpreter runs `pc`: code with no lifted function starting there (trampolines mods wrote, the middle
    /// of a function), and functions out of date. Not HLE functions, which the dispatcher calls.
    fn interpreted(&self, pc: u64) -> bool {
        if pc == nxport_lifter::abi::RETURN_TO_HOST {
            return false;
        }
        if self.is_start(pc) {
            return self.patched.read().unwrap().contains(&pc);
        }
        crate::hle_abi::entry(pc).is_none()
    }
}

/// Runs guest code from c.pc until it leaves for lifted code or the host; c.pc is where it goes on.
fn interpret(c: &mut Cpu) {
    let state = STATE.get().expect("live patches: not on");
    loop {
        match unsafe { interp::run(c, &|pc| state.interpreted(pc)) } {
            Stop::Leave => return,
            Stop::Unsupported(w) => {
                // svc: as lifted code does it.
                if w & 0xFFE0_001F == 0xD400_0001 {
                    let pc = c.pc;
                    (crate::lifted::svc_handler())(c, (w >> 5) & 0xffff, pc);
                    c.pc = pc + 4;
                    if !state.interpreted(c.pc) {
                        return;
                    }
                    continue;
                }
                eprintln!("[-] Live patches: {:#x}: {w:#010x} is not an instruction the interpreter runs, and compiling it is not done", c.pc);
                std::process::abort();
            }
        }
    }
}

/// For Handlers::unknown: a branch to code no lifted function starts at. False when live patches are off.
pub fn run_unknown(c: &mut Cpu) -> bool {
    if !enabled() {
        return false;
    }
    interpret(c);
    true
}

unsafe extern "sysv64" fn stub_entry(c: *mut Cpu) {
    interpret(&mut *c);
}

// A stub's common tail, in the lifted code's convention (System V, rdi = Cpu): run the interpreter, then continue in the
// dispatcher as a tail call, so a long chain of hooks does not grow the host stack.
std::arch::global_asm!(
    ".globl nx_live_tail",
    "nx_live_tail:",
    "push rdi",
    "call {entry}",
    "pop rdi",
    "jmp qword ptr [rip + {dispatch}]",
    entry = sym stub_entry,
    dispatch = sym NX_LIVE_RT_DISPATCH,
);

extern "C" {
    fn nx_live_tail();
}

/// A function's stub: c->pc = its guest address, then the common tail.
fn stub_bytes(pc: u64, tail: u64) -> [u8; STUB_SIZE] {
    let mut s = [0xCCu8; STUB_SIZE];
    s[0..2].copy_from_slice(&[0x48, 0xB8]); // mov rax, imm64
    s[2..10].copy_from_slice(&pc.to_le_bytes());
    s[10..17].copy_from_slice(&[0x48, 0x89, 0x87, 0x08, 0x01, 0x00, 0x00]); // mov [rdi + 264], rax (Cpu::pc)
    s[17..19].copy_from_slice(&[0x49, 0xBB]); // mov r11, imm64
    s[19..27].copy_from_slice(&tail.to_le_bytes());
    s[27..30].copy_from_slice(&[0x41, 0xFF, 0xE3]); // jmp r11
    s
}

fn entry_jump(to: u64) -> [u8; ENTRY_JUMP] {
    let mut j = [0xFFu8, 0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    j[6..14].copy_from_slice(&to.to_le_bytes());
    j
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
    fn Thread32First(snap: *mut c_void, e: *mut ThreadEntry) -> i32;
    fn Thread32Next(snap: *mut c_void, e: *mut ThreadEntry) -> i32;
    fn OpenThread(access: u32, inherit: i32, id: u32) -> *mut c_void;
    fn SuspendThread(t: *mut c_void) -> u32;
    fn ResumeThread(t: *mut c_void) -> u32;
    fn GetThreadContext(t: *mut c_void, ctx: *mut ThreadContext) -> i32;
    fn CloseHandle(h: *mut c_void) -> i32;
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcessId() -> u32;
    fn Sleep(ms: u32);
}

#[repr(C)]
#[derive(Default)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    id: u32,
    owner: u32,
    base_priority: i32,
    delta: i32,
    flags: u32,
}

/// CONTEXT: only ContextFlags (0x30) and Rip (0xF8) are read.
#[repr(C, align(16))]
struct ThreadContext([u8; 1232]);

/// Handles of the process's other threads, opened for suspending.
unsafe fn other_threads() -> Vec<*mut c_void> {
    const TH32CS_SNAPTHREAD: u32 = 4;
    const SUSPEND_AND_CONTEXT: u32 = 0x0002 | 0x0008;
    let (me, pid) = (GetCurrentThreadId(), GetCurrentProcessId());
    let mut out = Vec::with_capacity(256);
    let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    let mut e = ThreadEntry { size: std::mem::size_of::<ThreadEntry>() as u32, ..Default::default() };
    let mut ok = Thread32First(snap, &mut e);
    while ok != 0 {
        if e.owner == pid && e.id != me {
            let h = OpenThread(SUSPEND_AND_CONTEXT, 0, e.id);
            if !h.is_null() {
                out.push(h);
            }
        }
        ok = Thread32Next(snap, &mut e);
    }
    CloseHandle(snap);
    out
}

/// Writes `jumps` (entry, bytes) with every other thread stopped and none inside the bytes being written: a thread
/// stopped partway through those instructions would resume in the middle of the jump. Nothing here may allocate:
/// a stopped thread can hold the heap's lock.
unsafe fn write_entries(jumps: &[(usize, [u8; ENTRY_JUMP])]) -> Result<(), String> {
    const CONTEXT_CONTROL: u32 = 0x0010_0001;
    let threads = other_threads();
    let mut settled = false;
    for _ in 0..200 {
        for &t in &threads {
            SuspendThread(t);
        }
        let inside = threads.iter().any(|&t| {
            let mut ctx = std::mem::MaybeUninit::<ThreadContext>::zeroed().assume_init();
            ctx.0[0x30..0x34].copy_from_slice(&CONTEXT_CONTROL.to_le_bytes());
            if GetThreadContext(t, &mut ctx) == 0 {
                return false;
            }
            let rip = u64::from_le_bytes(ctx.0[0xF8..0x100].try_into().unwrap()) as usize;
            jumps.iter().any(|&(at, _)| rip > at && rip < at + ENTRY_JUMP)
        });
        if !inside {
            settled = true;
            break;
        }
        for &t in &threads {
            ResumeThread(t);
        }
        Sleep(1);
    }
    let mut result = Ok(());
    if settled {
        for &(at, bytes) in jumps {
            let page = at & !0xfff;
            let span = ((at + ENTRY_JUMP + 0xfff) & !0xfff) - page;
            if crate::os::protect(page, span, crate::os::Prot::ReadWriteExec).is_err() {
                result = Err("live patches: a function's entry cannot be written");
                break;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), at as *mut u8, ENTRY_JUMP);
            let _ = crate::os::protect(page, span, crate::os::Prot::ReadExec);
            crate::os::flush_icache(at, ENTRY_JUMP);
        }
        for &t in &threads {
            ResumeThread(t);
        }
    } else {
        result = Err("live patches: threads stayed inside the functions to patch");
    }
    for &t in &threads {
        CloseHandle(t);
    }
    result.map_err(str::to_string)
}

/// Guest code in [start, start + len) has changed: its functions run in the interpreter from now on.
pub fn code_written(start: usize, len: usize) {
    let Some(state) = STATE.get() else { return };
    let mut new: Vec<(u64, usize)> = Vec::new();
    {
        let mut patched = state.patched.write().unwrap();
        let mut seen = HashSet::new();
        let mut pc = start as u64 & !3;
        while pc < (start + len) as u64 {
            if let Some((func, end)) = state.function(pc) {
                if seen.insert(func) {
                    for (start, x86) in state.versions(func, end) {
                        patched.insert(start);
                        new.push((start, x86));
                    }
                }
            }
            pc += 4;
        }
    }
    // A function already patched by an earlier write has its stubs.
    let mut done = state.redirected.lock().unwrap();
    new.retain(|&(_, x86)| done.insert(x86));
    drop(done);
    if new.is_empty() {
        return;
    }
    let mut pool = state.stubs.lock().unwrap();
    let tail = nx_live_tail as *const () as u64;
    let mut jumps = Vec::with_capacity(new.len());
    for &(func, x86) in &new {
        if pool.next + STUB_SIZE > pool.end {
            eprintln!("[-] Live patches: out of stubs");
            std::process::abort();
        }
        let stub = pool.next;
        pool.next += STUB_SIZE;
        unsafe { std::ptr::copy_nonoverlapping(stub_bytes(func, tail).as_ptr(), stub as *mut u8, STUB_SIZE) };
        jumps.push((x86, entry_jump(stub as u64)));
    }
    if let Err(e) = unsafe { write_entries(&jumps) } {
        eprintln!("[-] {e}");
        std::process::abort();
    }
    println!("[+] Live patches: {} more functions interpreted", new.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stub_sets_pc_and_jumps_to_the_tail() {
        let s = stub_bytes(0x1122_3344_5566_7788, 0xAABB_CCDD_EEFF_0011);
        assert_eq!(&s[2..10], &0x1122_3344_5566_7788u64.to_le_bytes());
        // mov [rdi + 264], rax: Cpu::pc's offset.
        assert_eq!(u32::from_le_bytes(s[13..17].try_into().unwrap()) as usize, std::mem::offset_of!(Cpu, pc));
        assert_eq!(&s[19..27], &0xAABB_CCDD_EEFF_0011u64.to_le_bytes());
    }

    #[test]
    fn the_entry_jump_fits_the_smallest_lifted_function() {
        assert!(ENTRY_JUMP <= 16);
        let j = entry_jump(0x1234);
        assert_eq!(&j[..6], &[0xFF, 0x25, 0, 0, 0, 0]);
        assert_eq!(&j[6..14], &0x1234u64.to_le_bytes());
    }

    fn state(tables: Vec<Table>) -> State {
        State {
            tables,
            patched: RwLock::new(HashSet::new()),
            redirected: Mutex::new(HashSet::new()),
            stubs: Mutex::new(StubPool { next: 0, end: 0 }),
        }
    }

    static BASE_A: [u64; 3] = [0x1000, 0x1100, 0x1200];
    static BASE_F: [usize; 3] = [1, 2, 3];
    static OV_A: [u64; 2] = [0x1100, 0x1180];
    static OV_F: [usize; 2] = [11, 12];

    fn tables() -> State {
        state(vec![
            Table { addrs: &OV_A, fns: &OV_F, scattered: true },
            Table { addrs: &BASE_A, fns: &BASE_F, scattered: false },
        ])
    }

    #[test]
    fn a_gap_in_an_overlay_is_not_the_function_below_it() {
        // 0x1190 is past the overlay's 0x1180, which only split 0x1100: the function is still the base's.
        assert_eq!(tables().function(0x1190), Some((0x1100, 0x1200)));
        // 0x1204 has no overlay version nearby at all.
        assert_eq!(tables().function(0x1204), Some((0x1200, u64::MAX)));
        assert_eq!(tables().function(0xffc), None);
    }

    #[test]
    fn every_version_of_a_function_is_redirected() {
        let v = tables().versions(0x1100, 0x1200);
        assert_eq!(v, [(0x1100, 11), (0x1180, 12), (0x1100, 2)]);
    }
}
