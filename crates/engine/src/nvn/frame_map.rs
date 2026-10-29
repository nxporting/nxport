//! --log-frame-map=N: every NVN command the game records during presented frame N, in recording order per command
//! buffer, with the recording thread and the game's call chain (main+ return addresses). Shows which game function sets
//! up each pass (targets, clears, barriers, state) and draws in it. Printed as "[fmap]" lines when frame N+1 begins.

use std::sync::Mutex;

struct Entry {
    cmd: usize,
    thread: u64,
    name: &'static str,
    chain: [u32; 16],
}

static LOG: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The game's call chain at an HLE entry (frame-pointer walk over the guest stack; HLE entry stubs skipped).
pub(crate) unsafe fn guest_chain<const N: usize>() -> [u32; N] {
    let base = crate::guest::loader::main_base();
    let mut fp = crate::guest_reg!(x29);
    let mut key = [0u32; N];
    let mut n = 0;
    for _ in 0..64 {
        if fp < 0x1000 || fp & 7 != 0 || n == N {
            break;
        }
        let next = *(fp as *const usize);
        let ret = *((fp + 8) as *const usize);
        let stub = crate::gpu::report_guard::in_hle_stubs(ret);
        if !stub && ret >= base && ret < base + (256 << 20) {
            key[n] = (ret - base) as u32;
            n += 1;
        }
        if next <= fp && !stub {
            break;
        }
        fp = next;
    }
    key
}

pub(crate) unsafe fn note(name: &'static str, cmd: usize) {
    let Some(f) = crate::settings::get().log.log_frame_map else { return };
    if DONE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let cur = crate::gpu::backend().presented_frames() + 1;
    if cur < f {
        return;
    }
    if cur > f {
        print();
        return;
    }
    let chain = guest_chain::<16>();
    let thread = libc::pthread_mach_thread_np(libc::pthread_self()) as u64;
    LOG.lock().unwrap().push(Entry { cmd, thread, name, chain });
}

fn print() {
    if DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let log = std::mem::take(&mut *LOG.lock().unwrap());
    println!("[fmap] {} commands", log.len());
    for e in &log {
        let chain: Vec<String> = e.chain.iter().filter(|&&a| a != 0).map(|a| format!("{:x}", a)).collect();
        println!("[fmap] cb {:x} t {} {} <- {}", e.cmd, e.thread, e.name, chain.join(" "));
    }
}
