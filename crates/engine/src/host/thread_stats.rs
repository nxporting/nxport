// --log-threads: CPU time per thread (user + system, from Mach THREAD_EXTENDED_INFO, with the thread's name), as
// milliseconds per presented frame since the previous report. Shows how much of a frame the game's own threads compute
// (guest-main, workers) versus the engine's (Presentation Thread, gfx-issuer), independent of waiting.

use std::collections::HashMap;
use std::sync::Mutex;

#[repr(C)]
struct ThreadExtendedInfo {
    user_time: u64,
    system_time: u64,
    cpu_usage: i32,
    policy: i32,
    run_state: i32,
    flags: i32,
    sleep_time: i32,
    curpri: i32,
    priority: i32,
    maxpriority: i32,
    name: [u8; 64],
}

extern "C" {
    fn mach_task_self() -> u32;
    fn task_threads(task: u32, list: *mut *mut u32, count: *mut u32) -> i32;
    fn thread_info(thread: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    fn mach_port_deallocate(task: u32, name: u32) -> i32;
    fn vm_deallocate(task: u32, addr: usize, size: usize) -> i32;
}

const THREAD_EXTENDED_INFO: u32 = 5;

/// Previous totals per thread id (Mach port names are reused, so the name is part of the key).
static LAST: Mutex<Option<HashMap<(u32, String), u64>>> = Mutex::new(None);

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().log.log_threads)
}

/// Prints the busiest threads' CPU ms per frame over the last `frames` frames.
pub fn report(present: u64, frames: u64) {
    let mut now: HashMap<(u32, String), u64> = HashMap::new();
    unsafe {
        let task = mach_task_self();
        let mut list: *mut u32 = std::ptr::null_mut();
        let mut count = 0u32;
        if task_threads(task, &mut list, &mut count) != 0 {
            return;
        }
        for i in 0..count as usize {
            let th = *list.add(i);
            let mut info: ThreadExtendedInfo = std::mem::zeroed();
            let mut n = (std::mem::size_of::<ThreadExtendedInfo>() / 4) as u32;
            if thread_info(
                th,
                THREAD_EXTENDED_INFO,
                &mut info as *mut _ as *mut i32,
                &mut n,
            ) == 0
            {
                let len = info.name.iter().position(|&b| b == 0).unwrap_or(64);
                let name = String::from_utf8_lossy(&info.name[..len]).to_string();
                now.insert((th, name), (info.user_time + info.system_time) / 1000);
                // ns -> us
            }
            mach_port_deallocate(task, th);
        }
        vm_deallocate(task, list as usize, count as usize * 4);
    }
    let mut last = LAST.lock().unwrap();
    if let Some(prev) = last.as_ref() {
        let mut rows: Vec<(f64, String)> = now
            .iter()
            .filter_map(|(k, &t)| {
                prev.get(k).map(|&p| {
                    (
                        t.saturating_sub(p) as f64 / 1000.0 / frames.max(1) as f64,
                        k.1.clone(),
                    )
                })
            })
            .filter(|(ms, _)| *ms >= 0.2)
            .collect();
        rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let total: f64 = rows.iter().map(|r| r.0).sum();
        let top: Vec<String> = rows
            .iter()
            .take(14)
            .map(|(ms, n)| format!("{} {:.1}", if n.is_empty() { "?" } else { n }, ms))
            .collect();
        println!(
            "[threads] #{} CPU ms per frame (total {:.1}): {}",
            present,
            total,
            top.join(", ")
        );
    }
    *last = Some(now);
}

/// CPU time (us) of the engine's encoding threads: Presentation Thread, gfx-encoder, gfx-issuer (replay measurements).
pub fn engine_cpu_us() -> [u64; 3] {
    let mut out = [0u64; 3];
    unsafe {
        let task = mach_task_self();
        let mut list: *mut u32 = std::ptr::null_mut();
        let mut count = 0u32;
        if task_threads(task, &mut list, &mut count) != 0 {
            return out;
        }
        for i in 0..count as usize {
            let th = *list.add(i);
            let mut info: ThreadExtendedInfo = std::mem::zeroed();
            let mut n = (std::mem::size_of::<ThreadExtendedInfo>() / 4) as u32;
            if thread_info(
                th,
                THREAD_EXTENDED_INFO,
                &mut info as *mut _ as *mut i32,
                &mut n,
            ) == 0
            {
                let len = info.name.iter().position(|&b| b == 0).unwrap_or(64);
                let slot = match &info.name[..len] {
                    b"Presentation Thread" => Some(0),
                    b"gfx-encoder" => Some(1),
                    b"gfx-issuer" => Some(2),
                    _ => None,
                };
                if let Some(k) = slot {
                    out[k] += (info.user_time + info.system_time) / 1000;
                }
            }
            mach_port_deallocate(task, th);
        }
        vm_deallocate(task, list as usize, count as usize * 4);
    }
    out
}
