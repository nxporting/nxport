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
/// Per command buffer being recorded: (op index, call chain) of its pass-level commands (render-target binds, clears,
/// barriers, compute, copies) since its last EndRecording.
static RECORDERS: Mutex<Option<rustc_hash::FxHashMap<usize, Vec<(u32, [u32; 16])>>>> = Mutex::new(None);

/// EndRecording: the chains that recorded the list's pass-level commands (with --log-frame-map).
pub(crate) fn take_recorder(cmd: usize) -> Option<Box<Vec<(u32, [u32; 16])>>> {
    crate::settings::get().log.log_frame_map?;
    RECORDERS.lock().unwrap().as_mut()?.remove(&cmd).map(Box::new)
}

fn pass_level(name: &str) -> bool {
    ["RenderTargets", "Clear", "Barrier", "Dispatch", "Copy"].iter().any(|k| name.contains(k))
}
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
    if pass_level(name) {
        // Recorded right after this (the op's index in the list being recorded).
        if let Some(index) = crate::nvn::api::recording_len(cmd) {
            let index = index as u32;
            let chain = guest_chain::<16>();
            RECORDERS.lock().unwrap().get_or_insert_with(Default::default).entry(cmd).or_default().push((index, chain));
        }
    }
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
    let thread = crate::os::current_thread_id();
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
