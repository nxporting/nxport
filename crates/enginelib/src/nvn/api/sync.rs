// NVN syncs and events.

use super::*;

// ---------------------------------------------------------------------------
// NVN Sync & Event family. NVNsync is 0x40 (64) bytes, NVNevent is 0x40 (64) bytes.
#[repr(C)]
pub struct NvnSync {
    pub device: usize,
    /// Serial of the last command buffer submitted before nvnQueueFenceSync (0: never fenced, signaled).
    pub serial: u64,
    pub _pad: [u8; 48],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSyncInitialize(sync: *mut NvnSync, device: usize) -> u8 {
    record_hle_call("hle_nvnSyncInitialize");
    if sync.is_null() {
        return 0;
    }
    std::ptr::write_bytes(sync, 0, 1);
    (*sync).device = device;
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSyncFinalize(sync: *mut NvnSync) {
    record_hle_call("hle_nvnSyncFinalize");
    if !sync.is_null() {
        std::ptr::write_bytes(sync, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSyncSetDebugLabel(_sync: *mut NvnSync, _label: *const c_char) {
    record_hle_call("hle_nvnSyncSetDebugLabel");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSyncWait(sync: *const NvnSync, timeout_ns: u64) -> i32 {
    record_hle_call("hle_nvnSyncWait");
    // NVN_SYNC_WAIT_RESULT: ALREADY_SIGNALED 0, CONDITION_SATISFIED 1, TIMEOUT_EXPIRED 2.
    let serial = if sync.is_null() { 0 } else { (*sync).serial };
    let gpu = crate::gpu::backend();
    crate::capture::record::hold(serial);
    if serial == 0 || gpu.wait_fence(serial, 0) {
        return 0;
    }
    let t0 = std::time::Instant::now();
    let r = if gpu.wait_fence(serial, timeout_ns) {
        1
    } else {
        2
    };
    if log_timestamps() {
        let now = crate::platform::clock::now_ms();
        println!(
            "[ts] syncwait for submit {} (latest {}) {:.1} ms, ended t {:.3} ms thread {:?}",
            serial,
            gpu.submitted_serial(),
            t0.elapsed().as_secs_f64() * 1e3,
            now,
            std::thread::current().name()
        );
    }
    // Debug: --log-sync prints every 20th blocking wait: which submit it waited for, relative to the latest.
    static LOG: OnceLock<bool> = OnceLock::new();
    if *LOG.get_or_init(|| crate::settings::get().log.log_sync) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 20 == 0 {
            let fp = crate::guest_reg!(x29);
            println!("[sync] wait for submit {} (latest {}) took {:.1} ms timeout {} thread {:?} callers {}", serial, gpu.submitted_serial(),
                t0.elapsed().as_secs_f64() * 1e3, timeout_ns, std::thread::current().name(), guest_chain(fp));
        }
    }
    r
}

#[repr(C)]
pub struct NvnEvent {
    pub storage: usize,
    pub _pad: [u8; 56],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnEventInitialize(event: *mut NvnEvent, _builder: *const ()) -> u8 {
    record_hle_call("hle_nvnEventInitialize");
    if event.is_null() {
        return 0;
    }
    std::ptr::write_bytes(event, 0, 1);
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnEventFinalize(event: *mut NvnEvent) {
    record_hle_call("hle_nvnEventFinalize");
    if !event.is_null() {
        std::ptr::write_bytes(event, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnEventGetValue(_event: *const NvnEvent) -> u32 {
    record_hle_call("hle_nvnEventGetValue");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnEventSignal(_event: *mut NvnEvent, _mode: i32, _val: u32) {
    record_hle_call("hle_nvnEventSignal");
}
