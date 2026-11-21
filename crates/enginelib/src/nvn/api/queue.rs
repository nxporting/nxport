// NVN queues: builders, submits, fences, presents.

use super::*;

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetQueueMemory(
    b: *mut NvnQueueBuilder,
    memory: usize,
    size: usize,
) {
    if !b.is_null() {
        (*b).memory = memory;
        (*b).memory_size = size;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetCommandFlushThreshold(
    b: *mut NvnQueueBuilder,
    threshold: i32,
) {
    if !b.is_null() {
        (*b).flush_threshold = threshold;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetQueuePriority(
    b: *mut NvnQueueBuilder,
    priority: i32,
) {
    if !b.is_null() {
        (*b).priority = priority;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetDevice(b: *const NvnQueueBuilder) -> usize {
    if b.is_null() {
        0
    } else {
        (*b).device
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetFlags(b: *const NvnQueueBuilder) -> i32 {
    if b.is_null() {
        0
    } else {
        (*b).flags
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetCommandMemorySize(
    b: *const NvnQueueBuilder,
    out: *mut i32,
) -> u8 {
    if b.is_null() {
        0
    } else {
        queue_size_out((*b).command_mem_size, out)
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetComputeMemorySize(
    b: *const NvnQueueBuilder,
    out: *mut i32,
) -> u8 {
    if b.is_null() {
        0
    } else {
        queue_size_out((*b).compute_mem_size, out)
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetControlMemorySize(
    b: *const NvnQueueBuilder,
    out: *mut i32,
) -> u8 {
    if b.is_null() {
        0
    } else {
        queue_size_out((*b).control_mem_size, out)
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetCommandFlushThreshold(
    b: *const NvnQueueBuilder,
    out: *mut i32,
) -> u8 {
    if b.is_null() {
        0
    } else {
        queue_size_out((*b).flush_threshold.max(0) as usize, out)
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetQueuePriority(b: *const NvnQueueBuilder) -> i32 {
    if b.is_null() {
        100
    } else {
        (*b).priority
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetMemory(b: *const NvnQueueBuilder) -> usize {
    if b.is_null() {
        0
    } else {
        (*b).memory
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetMemorySize(b: *const NvnQueueBuilder) -> i32 {
    if b.is_null() {
        0
    } else {
        (*b).memory_size as i32
    }
}

/// The older form of nvnWindowAcquireTexture: acquire, then make the queue wait for the texture's release sync.
/// Textures are available as soon as they are acquired here, so there is nothing for the queue to wait for.
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueAcquireTexture(
    _queue: *mut (),
    w: *mut NvnWindow,
    out_index: *mut i32,
) -> i32 {
    record_hle_call("hle_nvnQueueAcquireTexture");
    hle_nvnWindowAcquireTexture(w, 0, out_index)
}

// ---------------------------------------------------------------------------
// NVN Queue family. NVNqueue is 0x2000 (8192) bytes, NVNqueueBuilder 0x40
// (64) bytes. The game's own queue object finishes its construction only once these succeed.
#[repr(C)]
pub(crate) struct NvnQueueBuilder {
    device: usize,
    flags: i32,
    command_mem_size: usize,
    compute_mem_size: usize,
    control_mem_size: usize,
    flush_threshold: i32, // 0 = not set
    priority: i32,
    memory: usize,
    memory_size: usize,
}

#[repr(C)]
pub(crate) struct NvnQueue {
    device: usize,
    flags: i32,
    command_mem_used: usize,
    control_mem_used: usize,
    compute_mem_used: usize,
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetDevice(b: *mut NvnQueueBuilder, device: usize) {
    record_hle_call("hle_nvnQueueBuilderSetDevice");
    if !b.is_null() {
        (*b).device = device;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetDefaults(b: *mut NvnQueueBuilder) {
    record_hle_call("hle_nvnQueueBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
        // Real NVN defaults are nonzero (a few MB each); match that order of
        // magnitude so GetQueueMemorySize/the game's own sizing math stays sane
        // even if it never calls the explicit Set*MemorySize overrides.
        (*b).command_mem_size = 0x40000;
        (*b).compute_mem_size = 0x40000;
        (*b).control_mem_size = 0x10000;
        (*b).priority = 100; // NVN_QUEUE_PRIORITY_NORMAL
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetFlags(b: *mut NvnQueueBuilder, flags: i32) {
    record_hle_call("hle_nvnQueueBuilderSetFlags");
    if !b.is_null() {
        (*b).flags = flags;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetCommandMemorySize(
    b: *mut NvnQueueBuilder,
    size: usize,
) {
    record_hle_call("hle_nvnQueueBuilderSetCommandMemorySize");
    if !b.is_null() {
        (*b).command_mem_size = size;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetComputeMemorySize(
    b: *mut NvnQueueBuilder,
    size: usize,
) {
    record_hle_call("hle_nvnQueueBuilderSetComputeMemorySize");
    if !b.is_null() {
        (*b).compute_mem_size = size;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetControlMemorySize(
    b: *mut NvnQueueBuilder,
    size: usize,
) {
    record_hle_call("hle_nvnQueueBuilderSetControlMemorySize");
    if !b.is_null() {
        (*b).control_mem_size = size;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetQueueMemorySize(b: *const NvnQueueBuilder) -> usize {
    record_hle_call("hle_nvnQueueBuilderGetQueueMemorySize");
    if b.is_null() {
        0
    } else {
        (*b).command_mem_size + (*b).compute_mem_size + (*b).control_mem_size
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueInitialize(
    q: *mut NvnQueue,
    builder: *const NvnQueueBuilder,
) -> u8 {
    record_hle_call("hle_nvnQueueInitialize");
    if q.is_null() || builder.is_null() {
        return 0;
    }
    std::ptr::write(
        q,
        NvnQueue {
            device: (*builder).device,
            flags: (*builder).flags,
            command_mem_used: 0,
            control_mem_used: 0,
            compute_mem_used: 0,
        },
    );
    println!(
        "[+] NVN: Queue initialized (device={:#x}, flags={:#x})",
        (*builder).device,
        (*builder).flags
    );
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueFinalize(q: *mut NvnQueue) {
    record_hle_call("hle_nvnQueueFinalize");
    if !q.is_null() {
        std::ptr::write_bytes(q, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueFlush(_q: *mut NvnQueue) {
    record_hle_call("hle_nvnQueueFlush");
    // Real NVN kicks the GPU to start processing submitted command buffers.
    // Nothing is actually deferred on our side yet (no async GPU execution
    // model exists until the Metal-translation follow-up), so there's
    // nothing to flush -- a no-op is correct for the current execution
    // model, not a gap.
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueFinish(_q: *mut NvnQueue) {
    record_hle_call("hle_nvnQueueFinish");
    // Blocks until everything submitted so far has completed on the GPU.
    crate::gpu::backend().wait_serial(crate::gpu::backend().submitted_serial(), u64::MAX);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueGetError(q: *mut NvnQueue, _error_info: *mut ()) -> i32 {
    record_hle_call("hle_nvnQueueGetError");
    let _ = q;
    0 // NVN_QUEUE_GET_ERROR_RESULT_NONE
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueGetTotalCommandMemoryUsed(q: *mut NvnQueue) -> usize {
    record_hle_call("hle_nvnQueueGetTotalCommandMemoryUsed");
    if q.is_null() {
        0
    } else {
        (*q).command_mem_used
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueGetTotalControlMemoryUsed(q: *mut NvnQueue) -> usize {
    record_hle_call("hle_nvnQueueGetTotalControlMemoryUsed");
    if q.is_null() {
        0
    } else {
        (*q).control_mem_used
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueGetTotalComputeMemoryUsed(q: *mut NvnQueue) -> usize {
    record_hle_call("hle_nvnQueueGetTotalComputeMemoryUsed");
    if q.is_null() {
        0
    } else {
        (*q).compute_mem_used
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueResetMemoryUsageCounts(q: *mut NvnQueue) {
    record_hle_call("hle_nvnQueueResetMemoryUsageCounts");
    if !q.is_null() {
        (*q).command_mem_used = 0;
        (*q).control_mem_used = 0;
        (*q).compute_mem_used = 0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueuePresentTexture(
    _queue: *mut NvnQueue,
    _window: *mut NvnWindow,
    index: i32,
) {
    record_hle_call("hle_nvnQueuePresentTexture");
    crate::gpu::frame_breakdown::frame_end(crate::gpu::backend().presented_frames());
    // No waiting here: NVN queues the present and returns; the vsync / present-interval wait happens when the game
    // acquires the next window texture (see hle_nvnWindowAcquireTexture). TotK's frame pacer measures its update +
    // draw time (excluding that wait) to pick the present interval, so a wait here locked it at 20 fps.
    {
        static FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let f = FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if f == 1 || f % 600 == 0 {
            println!("[+] NVN present: frame {} (buffer index {})", f, index);
        }
        if f % 600 == 0 && crate::settings::get().log.log_cmd_histogram {
            dump_command_histogram(f);
        }
        // A dump run with plugin mods (lifted hosts; natively the Metal backend does this): the image of the code as the
        // mods patched it, then nothing more to do.
        #[cfg(not(nx_native))]
        if crate::guest::exec::aot::deferred_dump_frame() == Some(f) {
            match unsafe { crate::guest::exec::aot::dump_after_mods() } {
                Ok(()) => {
                    println!("[+] AOT: image written at frame {f}; exiting (--aot-dump)");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("[!!] AOT: {e}");
                    std::process::exit(1);
                }
            }
        }
        if (crate::host::display::window_up() || crate::host::display::headless())
            && !_window.is_null()
            && index >= 0
            && (index as usize) < 4
        {
            crate::capture::record::on_present((*_window).textures[index as usize], (*_window).crop);
            crate::gpu::backend().present((*_window).textures[index as usize], (*_window).crop);
        }
    }
    #[cfg(all(nx_runtime, windows))]
    crate::guest::exec::live_patch::presented();
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueWaitSync(_queue: *mut NvnQueue, _sync: usize) -> u8 {
    record_hle_call("hle_nvnQueueWaitSync");
    1 // a GPU-side wait: there is one Metal queue, which already runs command buffers in order
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueFenceSync(
    _queue: *mut NvnQueue,
    sync: *mut NvnSync,
    _condition: i32,
    _flags: i32,
) {
    record_hle_call("hle_nvnQueueFenceSync");
    // Signaled once all work submitted so far (submits execute during nvnQueueSubmitCommands) has completed.
    if !sync.is_null() {
        crate::nvn::api::sync::set_available_at(sync, std::time::Duration::ZERO);
        (*sync).serial = crate::gpu::backend().submitted_serial();
    }
}

pub(crate) unsafe extern "C" fn hle_nvnQueueSubmitCommands(_queue: *mut (), num: i32, handles: *const u64) {
    record_hle_call("hle_nvnQueueSubmitCommands");
    struct Encode(u64);
    impl Drop for Encode {
        fn drop(&mut self) {
            crate::gpu::frame_breakdown::add_encode_ns(crate::platform::clock::now_ns().saturating_sub(self.0));
        }
    }
    let _e = Encode(crate::platform::clock::now_ns());
    if handles.is_null() || num <= 0 {
        return;
    }
    let hs: Vec<u64> = (0..num as usize)
        .map(|i| std::ptr::read_unaligned(handles.add(i)))
        .collect();
    crate::capture::record::on_submit(&hs);
    crate::gpu::backend().submit(&hs);
    mark_command_lists_submitted(&hs);
}
