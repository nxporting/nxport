// Guest code that mods patch while the game runs, on a host running lifted code. The base library's version of a changed
// function is out of date, so its x86 entry is pointed at a stub that runs the current guest instructions in
// nxport_lifter::interp, at once; meanwhile the function is lifted and compiled in the background (or found compiled, by
// the hash of its code, from an earlier run) and its entry is pointed at the result. Code the interpreter declines waits
// for that compile. `guest.live-patches` turns all this on.
//
// Calls nest and returns return here as in lifted code (cpu.h, RT_RETURNED), so a return always goes back to the frame
// that made its call, whichever side that frame runs on.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};

use nxport_lifter::abi::Cpu;
use nxport_lifter::interp::{self, Stop};

/// A guest library's function table: addresses sorted, and the x86 function of each.
#[derive(Clone, Copy)]
pub struct Table {
    pub addrs: &'static [u64],
    pub fns: &'static [usize],
    /// A table of a module's changed functions: the gaps between them belong to other functions.
    pub scattered: bool,
}

/// A library's nx_rt_add_module.
pub type AddModule = unsafe extern "C" fn(*const u64, *const c_void, u64) -> i32;

struct State {
    tables: RwLock<Vec<Table>>,
    /// Starts of the functions whose lifted version is out of date.
    patched: RwLock<HashSet<u64>>,
    /// Where each x86 entry was last pointed.
    redirected: Mutex<HashMap<usize, usize>>,
    /// What each redirected entry held before its first jump.
    originals: Mutex<HashMap<usize, [u8; ENTRY_JUMP]>>,
    stubs: Mutex<StubPool>,
    /// The loaded libraries' add functions: a function compiled later must be found by their dispatchers.
    adds: Vec<AddModule>,
    /// The host callbacks, for libraries compiled later.
    host: usize,
    queue: Mutex<Vec<(u64, u64)>>,
    wake: Condvar,
    /// One compile of a given name at a time.
    compiling: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// Applying a compiled function: one at a time.
    applying: Mutex<()>,
    version: u64,
}

struct StubPool {
    next: usize,
    end: usize,
    /// The stub of each function start.
    of: HashMap<u64, usize>,
}

static STATE: OnceLock<State> = OnceLock::new();
/// The guest library's dispatcher, which the stubs' common tail continues in.
#[no_mangle]
static NX_LIVE_RT_DISPATCH: AtomicUsize = AtomicUsize::new(0);

const STUB_SIZE: usize = 32;
const STUB_POOL: usize = 1 << 20;
/// `jmp [rip]` and its address, written over a function's entry. Lifted functions are 16-byte aligned.
const ENTRY_JUMP: usize = 14;
const COMPILERS: usize = 4;
/// How far past a function jump tables may lie (rodata follows a module's text).
const IMAGE_SPAN: usize = 0x500_0000;

pub fn enabled() -> bool {
    STATE.get().is_some()
}

/// Called with the guest library's tables (the overlay's first), before guest code runs.
pub fn enable(tables: Vec<Table>, rt_dispatch: usize, adds: Vec<AddModule>, host: usize) -> Result<(), String> {
    let pool = unsafe { crate::os::alloc(STUB_POOL, crate::os::Prot::ReadWriteExec) }.map_err(|e| format!("live patches: {e}"))?;
    NX_LIVE_RT_DISPATCH.store(rt_dispatch, Ordering::Relaxed);
    let version = std::env::current_exe().ok().and_then(|e| std::fs::read(e).ok()).map_or(0, |b| fnv(&b));
    let state = State {
        tables: RwLock::new(tables),
        patched: RwLock::new(HashSet::new()),
        redirected: Mutex::new(HashMap::new()),
        originals: Mutex::new(HashMap::new()),
        stubs: Mutex::new(StubPool { next: pool as usize, end: pool as usize + STUB_POOL, of: HashMap::new() }),
        adds,
        host,
        queue: Mutex::new(Vec::new()),
        wake: Condvar::new(),
        compiling: Mutex::new(HashMap::new()),
        applying: Mutex::new(()),
        version,
    };
    STATE.set(state).map_err(|_| "live patches: already on".to_string())?;
    for i in 0..COMPILERS {
        let _ = std::thread::Builder::new().name(format!("live-compile{i}")).spawn(compiler);
    }
    println!("[+] Live patches: on");
    Ok(())
}

fn fnv(d: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in d {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

impl State {
    /// The function holding `pc`, by the start at or below it in a table of a whole module: (start, next start). Code in
    /// no module's table (what a mod generates) is no function: it reaches the interpreter through Handlers::unknown.
    fn function(&self, pc: u64) -> Option<(u64, u64)> {
        self.tables.read().unwrap().iter().filter(|t| !t.scattered).find_map(|t| {
            let (first, last) = (*t.addrs.first()?, *t.addrs.last()?);
            if pc < first || pc > last.saturating_add(0x10000) {
                return None;
            }
            let i = t.addrs.partition_point(|&a| a <= pc);
            Some((t.addrs[i - 1], t.addrs.get(i).copied().unwrap_or(last.saturating_add(0x10000))))
        })
    }

    /// Every lifted function starting in [start, end), in any table: an overlay may have split the base's function, and
    /// the dispatcher looks in overlays first, so each version needs its entry pointed at the current one.
    fn versions(&self, start: u64, end: u64) -> Vec<(u64, usize)> {
        let mut v = Vec::new();
        for t in self.tables.read().unwrap().iter() {
            let (lo, hi) = (t.addrs.partition_point(|&a| a < start), t.addrs.partition_point(|&a| a < end));
            v.extend((lo..hi).map(|i| (t.addrs[i], t.fns[i])));
        }
        v
    }

    fn is_start(&self, pc: u64) -> bool {
        self.tables.read().unwrap().iter().any(|t| t.addrs.binary_search(&pc).is_ok())
    }

    /// Whether the interpreter runs `pc`: code with no lifted function starting there (trampolines mods wrote, the middle
    /// of a function), functions out of date, and HLE trampolines a mod hooked. Not the others, which the dispatcher calls,
    /// nor the common code all trampolines branch to (run_hle).
    fn interpreted(&self, pc: u64) -> bool {
        if pc == nxport_lifter::abi::RETURN_TO_HOST || pc == HLE_POOL.load(Ordering::Relaxed) as u64 {
            return false;
        }
        if self.is_start(pc) {
            return self.patched.read().unwrap().contains(&pc);
        }
        if crate::hle_abi::entry(pc).is_some() {
            return hle_hooked(pc);
        }
        true
    }
}

/// The HLE trampolines' pool (its first entry is where they all branch to): (start, length).
static HLE_POOL: AtomicUsize = AtomicUsize::new(0);
static HLE_POOL_LEN: AtomicUsize = AtomicUsize::new(0);
/// HLE trampolines a mod patched: lifted code must not call their functions directly any more.
static HLE_HOOKED: RwLock<Option<HashSet<u64>>> = RwLock::new(None);
static HLE_HOOKED_FILTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_hle_pool(start: usize, len: usize) {
    HLE_POOL.store(start, Ordering::Relaxed);
    HLE_POOL_LEN.store(len, Ordering::Relaxed);
}

/// Whether the HLE trampoline at `pc` was patched by a mod.
pub fn hle_hooked(pc: u64) -> bool {
    // On every HLE call of a game with plugin mods, most of them not hooked: no lock for those.
    if HLE_HOOKED_FILTER.load(Ordering::Relaxed) & hle_filter_bit(pc) == 0 {
        return false;
    }
    HLE_HOOKED.read().unwrap().as_ref().is_some_and(|s| s.contains(&pc))
}

fn hle_filter_bit(pc: u64) -> u64 {
    // Entries are 32 bytes.
    1 << (pc >> 5 & 63)
}

/// The common code of HLE trampolines, reached by a mod calling through its hook to the original. A trampoline's first
/// instructions pushed x29 and x30 and loaded the function into x16; natively the common code calls it, pops and returns.
unsafe fn run_hle(c: &mut Cpu) {
    let sp = c.sp;
    c.x[29] = std::ptr::read(sp as *const u64);
    c.x[30] = std::ptr::read((sp + 8) as *const u64);
    c.sp = sp + 16;
    let f = c.x[16] as *const ();
    match crate::hle_abi::adapter(f) {
        Some(h) => crate::lifted::call_hle(h, c),
        None => {
            eprintln!("[-] Live patches: {f:p} is not an HLE function");
            std::process::abort();
        }
    }
}

thread_local! {
    /// How deep this thread is in the interpreter.
    static INTERPRETING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Code ranges of the libraries compiled while the game runs, for is_guest_code: (start, end), filled in order.
const MAX_RANGES: usize = 4096;
static RANGES: [[AtomicUsize; 2]; MAX_RANGES] = [const { [AtomicUsize::new(0), AtomicUsize::new(0)] }; MAX_RANGES];
static RANGE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Whether a fault at `pc` on this thread comes from guest code that is not in the guest library: the interpreter's, or a
/// function compiled while the game runs. For report_guard, from its fault handler: reads atomics and a thread local.
pub fn is_guest_code(pc: usize) -> bool {
    if INTERPRETING.with(|i| i.get()) > 0 {
        return true;
    }
    (0..RANGE_COUNT.load(Ordering::Acquire).min(MAX_RANGES)).any(|i| {
        pc >= RANGES[i][0].load(Ordering::Relaxed) && pc < RANGES[i][1].load(Ordering::Relaxed)
    })
}

/// Runs guest code from c.pc until it leaves. True: go on at c.pc through the dispatcher; false: return to the caller.
unsafe fn interpret(c: &mut Cpu) -> bool {
    INTERPRETING.with(|i| i.set(i.get() + 1));
    let go_on = interpret_inner(c);
    INTERPRETING.with(|i| i.set(i.get() - 1));
    go_on
}

unsafe fn interpret_inner(c: &mut Cpu) -> bool {
    let state = STATE.get().expect("live patches: not on");
    let dispatch: unsafe extern "sysv64" fn(*mut Cpu) = std::mem::transmute(NX_LIVE_RT_DISPATCH.load(Ordering::Relaxed));
    let pool = HLE_POOL.load(Ordering::Relaxed) as u64;
    loop {
        // Ends as a return: the common code pops the trampoline's frame and returns.
        if pool != 0 && c.pc == pool {
            run_hle(c);
            return false;
        }
        match interp::run(c, &|pc| state.interpreted(pc)) {
            Stop::Branch => {
                if c.pc != pool {
                    return true;
                }
            }
            Stop::Return => return false,
            // As lifted code calls: nested, and on here if it came back here (RT_RETURNED), or through the dispatcher.
            Stop::Call(ra) => {
                if c.pc == pool || state.interpreted(c.pc) {
                    if interpret(c) {
                        dispatch(c);
                    }
                } else {
                    dispatch(c);
                }
                if c.pc != ra {
                    return true;
                }
            }
            Stop::Unsupported(w) => {
                // svc: as lifted code does it.
                if w & 0xFFE0_001F == 0xD400_0001 {
                    let pc = c.pc;
                    (crate::lifted::svc_handler())(c, (w >> 5) & 0xffff, pc);
                    c.pc = pc + 4;
                    if !state.interpreted(c.pc) {
                        return true;
                    }
                    continue;
                }
                // Anything else: this thread waits for the compiled function, entered at this instruction.
                let pc = c.pc;
                let (func, end) = state.function(pc).unwrap_or((pc, pc + 0x100));
                println!("[+] Live patches: {pc:#x} ({w:#010x}) waits for a compile");
                if let Err(e) = compile_and_apply(state, func, end, Some(pc)) {
                    eprintln!("[-] Live patches: {pc:#x}: {w:#010x} needs a compile, which failed: {e}");
                    std::process::abort();
                }
                return true;
            }
        }
    }
}

/// For Handlers::unknown: a branch to code no lifted function starts at. False when live patches are off. Reached through
/// the dispatcher, which returns to its caller when this does, so what the code goes on to runs nested here: the caller's
/// return check must see the return, not the branch.
pub fn run_unknown(c: &mut Cpu) -> bool {
    if !enabled() {
        return false;
    }
    unsafe {
        if interpret(c) {
            let dispatch: unsafe extern "sysv64" fn(*mut Cpu) = std::mem::transmute(NX_LIVE_RT_DISPATCH.load(Ordering::Relaxed));
            dispatch(c);
        }
    }
    true
}

unsafe extern "sysv64" fn stub_entry(c: *mut Cpu) -> u64 {
    interpret(&mut *c) as u64
}

// A stub's common tail, in the lifted code's convention (System V, rdi = Cpu): run the interpreter, then return as a
// lifted function does, or continue in the dispatcher as a tail call, so a long chain of hooks does not grow the stack.
std::arch::global_asm!(
    ".globl nx_live_tail",
    "nx_live_tail:",
    "push rdi",
    "call {entry}",
    "pop rdi",
    "test rax, rax",
    "jnz 2f",
    "ret",
    "2:",
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

/// Points `entries` (x86 entry, where to) there, skipping those already pointing there. An entry pointed at itself gets its
/// own code back: a compiled function can become current again when its code returns to what it was compiled from.
fn redirect(state: &State, entries: &[(usize, usize)]) -> Result<(), String> {
    let mut done = state.redirected.lock().unwrap();
    let mut originals = state.originals.lock().unwrap();
    let mut jumps: Vec<(usize, [u8; ENTRY_JUMP])> = Vec::new();
    for &(at, to) in entries {
        if at == to {
            if let (Some(&was), Some(&code)) = (done.get(&at), originals.get(&at)) {
                if was != at {
                    jumps.push((at, code));
                }
            }
        } else if done.get(&at) != Some(&to) {
            originals.entry(at).or_insert_with(|| unsafe { std::ptr::read(at as *const [u8; ENTRY_JUMP]) });
            jumps.push((at, entry_jump(to as u64)));
        }
    }
    if jumps.is_empty() {
        return Ok(());
    }
    unsafe { write_entries(&jumps)? };
    for &(at, to) in entries {
        done.insert(at, to);
    }
    Ok(())
}

/// Guest code in [start, start + len) has changed: its functions run in the interpreter from now on, until compiled.
pub fn code_written(start: usize, len: usize) {
    let Some(state) = STATE.get() else { return };
    let (pool, pool_len) = (HLE_POOL.load(Ordering::Relaxed), HLE_POOL_LEN.load(Ordering::Relaxed));
    if start >= pool && start < pool + pool_len {
        // An HLE trampoline's own code: entries are 32 bytes.
        let entry = (pool + ((start - pool) & !31)) as u64;
        let mut hooked = HLE_HOOKED.write().unwrap();
        if hooked.get_or_insert_with(HashSet::new).insert(entry) {
            HLE_HOOKED_FILTER.fetch_or(hle_filter_bit(entry), Ordering::Relaxed);
            println!("[+] Live patches: HLE trampoline {entry:#x} hooked");
        }
        return;
    }
    let mut funcs = BTreeSet::new();
    let mut pc = start as u64 & !3;
    while pc < (start + len) as u64 {
        if let Some(f) = state.function(pc) {
            funcs.insert(f);
        }
        pc += 4;
    }
    if funcs.is_empty() {
        return;
    }
    let tail = nx_live_tail as *const () as u64;
    let mut entries = Vec::new();
    {
        let mut patched = state.patched.write().unwrap();
        let mut stubs = state.stubs.lock().unwrap();
        for &(func, end) in &funcs {
            for (s, x86) in state.versions(func, end) {
                patched.insert(s);
                let stub = match stubs.of.get(&s) {
                    Some(&st) => st,
                    None => {
                        if stubs.next + STUB_SIZE > stubs.end {
                            eprintln!("[-] Live patches: out of stubs");
                            std::process::abort();
                        }
                        let st = stubs.next;
                        stubs.next += STUB_SIZE;
                        unsafe { std::ptr::copy_nonoverlapping(stub_bytes(s, tail).as_ptr(), st as *mut u8, STUB_SIZE) };
                        stubs.of.insert(s, st);
                        st
                    }
                };
                entries.push((x86, stub));
            }
        }
    }
    if let Err(e) = redirect(state, &entries) {
        eprintln!("[-] {e}");
        std::process::abort();
    }
    // Compiled by an earlier run: in use at once, before anything can wait for it.
    let mut waiting = Vec::new();
    let mut cached = 0;
    for &(func, end) in &funcs {
        let name = cache_name(state, func, end);
        let ready = entry_points(state, &name, func, end, None).and_then(|s| library_path(&name, &s)).is_ok_and(|d| d.is_file());
        if ready && compile_and_apply(state, func, end, None).is_ok() {
            cached += 1;
        } else {
            waiting.push((func, end));
        }
    }
    println!("[+] Live patches: {} functions changed, {cached} already compiled", funcs.len());
    let mut q = state.queue.lock().unwrap();
    q.extend(waiting);
    state.wake.notify_all();
}

/// A thread compiling changed functions, in the background of the running game.
fn compiler() {
    let state = STATE.get().expect("live patches: not on");
    loop {
        let (func, end) = {
            let mut q = state.queue.lock().unwrap();
            loop {
                if let Some(f) = q.pop() {
                    break f;
                }
                q = state.wake.wait(q).unwrap();
            }
        };
        if let Err(e) = compile_and_apply(state, func, end, None) {
            println!("[!] Live patches: {func:#x} stays interpreted: {e}");
        }
    }
}

fn cache_root() -> Result<PathBuf, String> {
    let tid = crate::game::title_id().unwrap_or(0);
    Ok(crate::config::support_dir().ok_or("no data directory")?.join("games").join(format!("{tid:016x}")).join("live_patches"))
}

fn zig() -> Result<PathBuf, String> {
    let dir = crate::config::support_dir().ok_or("no data directory")?.join("toolchains");
    std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path().join("zig.exe"))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("no C compiler in {}", dir.display()))
}

/// The cache's name for [func, end) with its code as it is now.
fn cache_name(state: &State, func: u64, end: u64) -> String {
    let code = unsafe { std::slice::from_raw_parts(func as *const u8, (end - func) as usize) };
    format!("live_{func:x}_{:016x}", fnv(code) ^ state.version)
}

/// Where a function must be enterable, as earlier runs found: what the base knows, and where the interpreter stopped.
fn entry_points(state: &State, name: &str, func: u64, end: u64, extra: Option<u64>) -> Result<BTreeSet<u64>, String> {
    let mut seeds: BTreeSet<u64> = state.versions(func, end).iter().map(|v| v.0).collect();
    seeds.insert(func);
    seeds.extend(extra);
    let text = std::fs::read_to_string(cache_root()?.join(name).join("entries.txt")).unwrap_or_default();
    seeds.extend(text.lines().filter_map(|l| u64::from_str_radix(l, 16).ok()).filter(|a| (func..end).contains(a)));
    Ok(seeds)
}

fn library_path(name: &str, seeds: &BTreeSet<u64>) -> Result<PathBuf, String> {
    let mut h = 0u64;
    for s in seeds {
        h = (h ^ s).wrapping_mul(0x0100_0000_01b3);
    }
    Ok(cache_root()?.join(name).join(format!("f_{h:016x}.dll")))
}

/// Lifts and compiles [func, end) as it is now, unless an earlier run did: the library's path.
fn compiled(state: &State, name: &str, func: u64, end: u64, seeds: &BTreeSet<u64>) -> Result<PathBuf, String> {
    let dir = cache_root()?.join(name);
    let dll = library_path(name, seeds)?;
    let lock = state.compiling.lock().unwrap().entry(name.to_string()).or_default().clone();
    let _guard = lock.lock().unwrap();
    if dll.is_file() {
        return Ok(dll);
    }
    // Jump tables lie in the module's rodata: the image reaches past the code. Only what the lifter asks for is read.
    let image = unsafe { std::slice::from_raw_parts(func as *const u8, IMAGE_SPAN) };
    let module = nxport_lifter::Module { name: name.to_string(), base: func, image, text: func..end, seeds: seeds.clone() };
    let out = nxport_lifter::lift_module(&module, &Default::default());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (file, body) in nxport_lifter::RUNTIME_FILES {
        std::fs::write(dir.join(file), body).map_err(|e| e.to_string())?;
    }
    let mut sources = vec!["rt.c".to_string()];
    for (file, body) in &out.files {
        std::fs::write(dir.join(file), body).map_err(|e| e.to_string())?;
        sources.push(file.clone());
    }
    let tmp = dll.with_extension("tmp.dll");
    let result = std::process::Command::new(zig()?)
        .current_dir(&dir)
        .args(["cc", "-O1", "-g0", "-fno-strict-aliasing", "-mavx2", "-mfma", "-mbmi", "-mbmi2", "-mlzcnt", "-mmovbe", "-mf16c", "-mpopcnt", "-msse4.2", "-mcx16", "-w", "-shared", "-o"])
        .arg(&tmp)
        .args(&sources)
        .output()
        .map_err(|e| format!("the C compiler: {e}"))?;
    if !result.status.success() {
        return Err(format!("the C compiler: {}", String::from_utf8_lossy(&result.stderr).lines().next().unwrap_or("failed")));
    }
    std::fs::rename(&tmp, &dll).map_err(|e| e.to_string())?;
    let list: String = seeds.iter().map(|a| format!("{a:x}\n")).collect();
    std::fs::write(dir.join("entries.txt"), list).map_err(|e| e.to_string())?;
    Ok(dll)
}

/// The function [func, end) compiled as its code stands, and its entries pointed there.
fn compile_and_apply(state: &State, func: u64, end: u64, extra: Option<u64>) -> Result<(), String> {
    let name = cache_name(state, func, end);
    let seeds = entry_points(state, &name, func, end, extra)?;
    let dll = compiled(state, &name, func, end, &seeds)?;
    // A write meanwhile queued this function again: that compile applies.
    if cache_name(state, func, end) != name {
        return Ok(());
    }
    let _applying = state.applying.lock().unwrap();
    unsafe {
        let lib = crate::os::Library::open(&dll)?;
        let sym = |n: &str| -> Result<*mut c_void, String> {
            let p = lib.symbol(n);
            if p.is_null() {
                Err(format!("{}: no symbol {n}", dll.display()))
            } else {
                Ok(p)
            }
        };
        let init: unsafe extern "C" fn(usize) = std::mem::transmute(sym("nx_rt_init")?);
        let add: AddModule = std::mem::transmute(sym("nx_rt_add_module")?);
        let count = *(sym(&format!("{name}_fn_count"))? as *const u64) as usize;
        let addrs = std::slice::from_raw_parts(sym(&format!("{name}_fn_addrs"))? as *const u64, count);
        let fns = std::slice::from_raw_parts(sym(&format!("{name}_fn_ptrs"))? as *const usize, count);
        init(state.host);
        add(addrs.as_ptr(), fns.as_ptr() as *const c_void, count as u64);
        if let Some((lo, hi)) = fns.first().and_then(|&f| crate::os::image_range(f)) {
            let i = RANGE_COUNT.load(Ordering::Relaxed);
            if i < MAX_RANGES {
                RANGES[i][0].store(lo, Ordering::Relaxed);
                RANGES[i][1].store(hi, Ordering::Relaxed);
                RANGE_COUNT.store(i + 1, Ordering::Release);
            }
        }
        for t in state.tables.read().unwrap().iter() {
            if !t.scattered {
                add(t.addrs.as_ptr(), t.fns.as_ptr() as *const c_void, t.addrs.len() as u64);
            }
        }
        for a in &state.adds {
            a(addrs.as_ptr(), fns.as_ptr() as *const c_void, count as u64);
        }
        let entries: Vec<(usize, usize)> = state
            .versions(func, end)
            .into_iter()
            .filter_map(|(s, x86)| addrs.binary_search(&s).ok().map(|i| (x86, fns[i])))
            .collect();
        state.tables.write().unwrap().insert(0, Table { addrs, fns, scattered: true });
        redirect(state, &entries)?;
        let mut patched = state.patched.write().unwrap();
        for s in addrs {
            patched.remove(s);
        }
    }
    println!("[+] Live patches: {func:#x} compiled");
    Ok(())
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

    static BASE_A: [u64; 3] = [0x1000, 0x1100, 0x1200];
    static BASE_F: [usize; 3] = [1, 2, 3];
    static OV_A: [u64; 2] = [0x1100, 0x1180];
    static OV_F: [usize; 2] = [11, 12];

    fn tables() -> State {
        State {
            tables: RwLock::new(vec![
                Table { addrs: &OV_A, fns: &OV_F, scattered: true },
                Table { addrs: &BASE_A, fns: &BASE_F, scattered: false },
            ]),
            patched: RwLock::new(HashSet::new()),
            redirected: Mutex::new(HashMap::new()),
            originals: Mutex::new(HashMap::new()),
            stubs: Mutex::new(StubPool { next: 0, end: 0, of: HashMap::new() }),
            adds: Vec::new(),
            host: 0,
            queue: Mutex::new(Vec::new()),
            wake: Condvar::new(),
            compiling: Mutex::new(HashMap::new()),
            applying: Mutex::new(()),
            version: 0,
        }
    }

    #[test]
    fn a_gap_in_an_overlay_is_not_the_function_below_it() {
        // 0x1190 is past the overlay's 0x1180, which only split 0x1100: the function is still the base's.
        assert_eq!(tables().function(0x1190), Some((0x1100, 0x1200)));
        assert_eq!(tables().function(0x1204), Some((0x1200, 0x1200 + 0x10000)));
        assert_eq!(tables().function(0xffc), None);
    }

    #[test]
    fn every_version_of_a_function_is_redirected() {
        let v = tables().versions(0x1100, 0x1200);
        assert_eq!(v, [(0x1100, 11), (0x1180, 12), (0x1100, 2)]);
    }
}

// A present layer for mods. The engine replaces NVN, so nothing reaches what is below it in the SDK, and a hook there would
// never run: android::Surface::queueBuffer is where frame limiters hook, as a Switch calls it once per present. With plugin
// mods it is emptied before they load, a hook's call of the original then being harmless, and a present calls it if hooked.

/// The vtable slot of Surface::queueBuffer, counted from the vtable's address point.
const QUEUE_BUFFER_SLOT: usize = 8;
/// The same for Surface::setSwapInterval.
const SET_SWAP_INTERVAL_SLOT: usize = 11;
const ARM64_MOV_W0_0: u32 = 0x5280_0000;
const ARM64_RET: u32 = 0xD65F_03C0;
/// mov w0, #0; then returns, wherever a hook's jump back lands: a hook may overwrite more than one instruction.
const EMPTY_FUNCTION: [u32; 6] = [ARM64_MOV_W0_0, ARM64_RET, ARM64_RET, ARM64_RET, ARM64_RET, ARM64_RET];

/// (start of rodata, end of the image, start of text, end of text) of the SDK.
static SDK_RANGES: Mutex<Option<(usize, usize, usize, usize)>> = Mutex::new(None);
static QUEUE_BUFFER: AtomicUsize = AtomicUsize::new(0);
/// Surface::setSwapInterval, its first instruction as the SDK has it, and the Surface field it stores the interval in.
static SET_SWAP_INTERVAL: AtomicUsize = AtomicUsize::new(0);
static SET_SWAP_INTERVAL_FIRST: AtomicU32 = AtomicU32::new(0);
static SWAP_INTERVAL_FIELD: AtomicUsize = AtomicUsize::new(0);
/// The interval a mod's hook made of the game's, -1 when none did.
static FORCED_INTERVAL: AtomicI32 = AtomicI32::new(-1);

/// The field `str wN, [x0, #field]` writes, in a function's first instructions.
fn field_stored(code: &[u32]) -> Option<usize> {
    code.iter().find(|&&w| w & 0xFFC0_03E0 == 0xB900_0000).map(|&w| ((w >> 10 & 0xFFF) * 4) as usize)
}

/// The Surface the engine has none of: for mods to pass on, or write to.
fn surface() -> usize {
    static SURFACE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *SURFACE.get_or_init(|| Box::leak(vec![0u64; 0x800].into_boxed_slice()).as_mut_ptr() as usize)
}

/// The game set its present interval, which on a Switch reaches Surface::setSwapInterval: what a hook there makes of it.
pub fn present_interval_set(interval: i32) {
    let addr = SET_SWAP_INTERVAL.load(Ordering::Acquire);
    if addr == 0 {
        return;
    }
    unsafe {
        if std::ptr::read_volatile(addr as *const u32) == SET_SWAP_INTERVAL_FIRST.load(Ordering::Relaxed) {
            return;
        }
        let this = surface();
        let field = (this + SWAP_INTERVAL_FIELD.load(Ordering::Relaxed)) as *mut i32;
        std::ptr::write_volatile(field, interval);
        crate::guest::call(addr, &[this as u64, interval as u32 as u64]);
        let forced = std::ptr::read_volatile(field);
        if FORCED_INTERVAL.swap(forced, Ordering::Relaxed) != forced {
            println!("[+] Live patches: a mod makes the present interval {forced} (the game asked for {interval})");
        }
    }
}

/// The present interval a mod forces, if one does: a Switch's vsync would hold frames to it.
pub fn forced_interval() -> Option<i32> {
    Some(FORCED_INTERVAL.load(Ordering::Relaxed)).filter(|&i| i > 0)
}

pub fn set_sdk(rodata: usize, image_end: usize, text: usize, text_end: usize) {
    *SDK_RANGES.lock().unwrap() = Some((rodata, image_end, text, text_end));
}

/// Finds the address held in slot `slot` of the vtable of the class with this mangled name, through its type info.
unsafe fn vtable_slot(mangled: &[u8], slot: usize, (rodata, image_end, text, text_end): (usize, usize, usize, usize)) -> Option<usize> {
    let image = std::slice::from_raw_parts(rodata as *const u8, image_end - rodata);
    let name_at = rodata + image.windows(mangled.len() + 2).position(|w| w[0] == 0 && &w[1..=mangled.len()] == mangled && w[mangled.len() + 1] == 0)? + 1;
    let words = std::slice::from_raw_parts(rodata.next_multiple_of(8) as *const usize, (image_end - rodata.next_multiple_of(8)) / 8);
    let at = |i: usize| rodata.next_multiple_of(8) + i * 8;
    let type_info = words.iter().enumerate().filter(|&(_, &w)| w == name_at).map(|(i, _)| at(i) - 8);
    for ti in type_info {
        for (i, _) in words.iter().enumerate().filter(|&(_, &w)| w == ti) {
            // The primary vtable: offset to top 0, then code.
            if i == 0 || words[i - 1] != 0 {
                continue;
            }
            let Some(slots) = words.get(i + 1..i + 1 + slot + 1) else { continue };
            if slots.iter().all(|&a| (text..text_end).contains(&a)) {
                return Some(slots[slot]);
            }
        }
    }
    None
}

/// Before plugin mods' initialisers.
pub fn empty_present_layer() {
    let Some(_) = STATE.get() else { return };
    let Some(ranges) = *SDK_RANGES.lock().unwrap() else { return };
    let Some(addr) = (unsafe { vtable_slot(b"N7android7SurfaceE", QUEUE_BUFFER_SLOT, ranges) }) else {
        println!("[!] Live patches: no android::Surface in the SDK; mods hooking its present will not run");
        return;
    };
    let len = EMPTY_FUNCTION.len() * 4;
    let page = addr & !0x3fff;
    let span = ((addr + len + 0x3fff) & !0x3fff) - page;
    unsafe {
        let _ = crate::os::protect(page, span, crate::os::Prot::ReadWrite);
        std::ptr::copy_nonoverlapping(EMPTY_FUNCTION.as_ptr(), addr as *mut u32, EMPTY_FUNCTION.len());
        crate::cpu::aot::protect_rx(page, span);
    }
    code_written(addr, len);
    QUEUE_BUFFER.store(addr, Ordering::Release);
    let setter = unsafe { vtable_slot(b"N7android7SurfaceE", SET_SWAP_INTERVAL_SLOT, ranges) };
    if let Some(setter) = setter {
        let code = unsafe { std::slice::from_raw_parts(setter as *const u32, 12) };
        if let Some(field) = field_stored(code).filter(|&f| f + 4 <= 0x800 * 8) {
            SWAP_INTERVAL_FIELD.store(field, Ordering::Relaxed);
            SET_SWAP_INTERVAL_FIRST.store(code[0], Ordering::Relaxed);
            SET_SWAP_INTERVAL.store(setter, Ordering::Release);
        }
    }
    println!("[+] Live patches: android::Surface::queueBuffer at {addr:#x} emptied for hooks");
}

/// A frame was presented: what a mod hooked on queueBuffer. An unhooked function is not called.
pub fn presented() {
    let addr = QUEUE_BUFFER.load(Ordering::Acquire);
    if addr == 0 {
        return;
    }
    unsafe {
        if std::ptr::read_volatile(addr as *const u32) == ARM64_MOV_W0_0 {
            return;
        }
        const NO_FENCE: u64 = 0xFFFF_FFFF;
        crate::guest::call(addr, &[surface() as u64, 0, NO_FENCE]);
    }
}

#[cfg(test)]
mod surface_tests {
    #[test]
    fn the_field_set_swap_interval_stores() {
        // TotK 1.1.2's SDK: ldp, cmp, csel, cmp, csel, str w8, [x0, #0xf24], mov, ret.
        let code = [0x2949a408, 0x6b01011f, 0x1a81c108, 0x6b09011f, 0x1a88c128, 0xb90f2408, 0x2a1f03e0, 0xd65f03c0];
        assert_eq!(super::field_stored(&code), Some(0xf24));
        // A store through another register is not one.
        assert_eq!(super::field_stored(&[0xb90f2428]), None);
    }
}
