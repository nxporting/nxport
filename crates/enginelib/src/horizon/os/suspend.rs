// nn::os thread suspension and contexts: SuspendThread, ResumeThread, GetThreadSuspendCount, GetThreadContext.
//
// Garbage collectors stop the world with them (Unity's IL2CPP: suspend every other thread, read its registers and
// stack pointer, scan its stack). Each guest thread registers its host thread and guest stack when it starts running
// guest code (thread_stack::run). A thread stopped inside an HLE call runs on its host stack (thread_stack.rs): its
// context then reports the bottom of its guest stack as sp, so a scan covers the whole guest stack (conservative: it
// may keep dead objects alive, it never misses a root on the stack). Callee-saved registers of such a thread are the
// host's values at that moment.

use super::threads::NnThreadType;
use std::sync::Mutex;

struct HostThread {
    port: u32,
    lo: usize,
    hi: usize,
    suspends: i32,
}

static THREADS: Mutex<Vec<(usize, HostThread)>> = Mutex::new(Vec::new());

/// The calling host thread runs `thread`'s guest code on the stack [lo, hi).
pub(crate) fn register_current(thread: *mut NnThreadType, lo: usize, hi: usize) {
    if thread.is_null() {
        return;
    }
    #[cfg(all(nx_native, target_vendor = "apple"))]
    let port = unsafe { libc::mach_thread_self() };
    #[cfg(not(all(nx_native, target_vendor = "apple")))]
    let port = 0;
    let mut t = THREADS.lock().unwrap();
    t.retain(|e| e.0 != thread as usize);
    t.push((thread as usize, HostThread { port, lo, hi, suspends: 0 }));
}

#[cfg(all(nx_native, target_vendor = "apple"))]
mod mach {
    pub const ARM_THREAD_STATE64: i32 = 6;
    pub const ARM_THREAD_STATE64_COUNT: u32 = 68;
    pub const ARM_NEON_STATE64: i32 = 17;
    pub const ARM_NEON_STATE64_COUNT: u32 = 130;
    #[repr(C)]
    #[derive(Default)]
    pub struct ThreadState {
        pub x: [u64; 29],
        pub fp: u64,
        pub lr: u64,
        pub sp: u64,
        pub pc: u64,
        pub cpsr: u32,
        pub pad: u32,
    }
    #[repr(C, align(16))]
    pub struct NeonState {
        pub v: [u128; 32],
        pub fpsr: u32,
        pub fpcr: u32,
    }
    extern "C" {
        pub fn thread_suspend(thread: u32) -> i32;
        pub fn thread_resume(thread: u32) -> i32;
        pub fn thread_get_state(thread: u32, flavor: i32, state: *mut u32, count: *mut u32) -> i32;
    }
}

fn not_supported(what: &str) {
    static LOGGED: std::sync::Once = std::sync::Once::new();
    LOGGED.call_once(|| println!("[!] HLE: nn::os::{what}: thread suspension is not implemented on this host"));
}

/// nn::os::SuspendThread(ThreadType*): the thread stops until as many ResumeThread calls.
pub(crate) unsafe extern "C" fn suspend_thread(thread: *mut NnThreadType) {
    let mut t = THREADS.lock().unwrap();
    let Some(e) = t.iter_mut().find(|e| e.0 == thread as usize) else {
        return;
    };
    #[cfg(all(nx_native, target_vendor = "apple"))]
    {
        if mach::thread_suspend(e.1.port) == 0 {
            e.1.suspends += 1;
        }
    }
    #[cfg(not(all(nx_native, target_vendor = "apple")))]
    {
        let _ = e;
        not_supported("SuspendThread");
    }
}

/// nn::os::ResumeThread(ThreadType*): undoes one SuspendThread.
pub(crate) unsafe extern "C" fn resume_thread(thread: *mut NnThreadType) {
    let mut t = THREADS.lock().unwrap();
    let Some(e) = t.iter_mut().find(|e| e.0 == thread as usize) else {
        return;
    };
    if e.1.suspends <= 0 {
        return;
    }
    #[cfg(all(nx_native, target_vendor = "apple"))]
    {
        if mach::thread_resume(e.1.port) == 0 {
            e.1.suspends -= 1;
        }
    }
    #[cfg(not(all(nx_native, target_vendor = "apple")))]
    not_supported("ResumeThread");
}

/// nn::os::GetThreadSuspendCount(const ThreadType*).
pub(crate) unsafe extern "C" fn get_thread_suspend_count(thread: *const NnThreadType) -> i32 {
    let t = THREADS.lock().unwrap();
    t.iter().find(|e| e.0 == thread as usize).map_or(0, |e| e.1.suspends)
}

/// nn::os::GetThreadContext(ThreadContextInfoAarch64*, const ThreadType*) of a suspended thread. The layout is the
/// kernel's thread context (the SDK copies it through): x0-x28 at 0, fp 0xe8, lr 0xf0, sp 0xf8, pc 0x100, pstate
/// (u32) 0x108, v0-v31 at 0x110, fpcr 0x310, fpsr 0x314.
pub(crate) unsafe extern "C" fn get_thread_context(out: *mut u8, thread: *const NnThreadType) {
    if out.is_null() {
        return;
    }
    std::ptr::write_bytes(out, 0, 0x318);
    let t = THREADS.lock().unwrap();
    let Some(e) = t.iter().find(|e| e.0 == thread as usize) else {
        return;
    };
    #[cfg(all(nx_native, target_vendor = "apple"))]
    {
        let mut s = mach::ThreadState::default();
        let mut n = mach::ARM_THREAD_STATE64_COUNT;
        if mach::thread_get_state(e.1.port, mach::ARM_THREAD_STATE64, &mut s as *mut _ as *mut u32, &mut n) != 0 {
            return;
        }
        let w = |off: usize, v: u64| std::ptr::write_unaligned(out.add(off) as *mut u64, v);
        for (k, x) in s.x.iter().enumerate() {
            w(k * 8, *x);
        }
        let on_guest_stack = (s.sp as usize) >= e.1.lo && (s.sp as usize) < e.1.hi;
        w(0xe8, s.fp);
        w(0xf0, s.lr);
        // In an HLE call (host stack): the whole guest stack is live as far as a scan knows.
        w(0xf8, if on_guest_stack { s.sp } else { e.1.lo as u64 });
        w(0x100, s.pc);
        std::ptr::write_unaligned(out.add(0x108) as *mut u32, s.cpsr);
        let mut v: mach::NeonState = std::mem::zeroed();
        let mut n = mach::ARM_NEON_STATE64_COUNT;
        if mach::thread_get_state(e.1.port, mach::ARM_NEON_STATE64, &mut v as *mut _ as *mut u32, &mut n) == 0 {
            std::ptr::copy_nonoverlapping(v.v.as_ptr() as *const u8, out.add(0x110), 0x200);
            std::ptr::write_unaligned(out.add(0x310) as *mut u32, v.fpcr);
            std::ptr::write_unaligned(out.add(0x314) as *mut u32, v.fpsr);
        }
    }
    #[cfg(not(all(nx_native, target_vendor = "apple")))]
    {
        let _ = e;
        not_supported("GetThreadContext");
    }
}
