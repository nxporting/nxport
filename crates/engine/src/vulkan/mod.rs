// Vulkan backend for crate::gpu::Backend.
//
// Correctness first: each submit is one command buffer, recorded from the NVN command lists in order and waited for
// before the submit returns. Images stay in the GENERAL layout with full barriers between passes and other work.
// Guest memory pools are imported as buffers where the device allows it (VK_EXT_external_memory_host), so vertex,
// index and storage data are the guest's own pages, as on the Switch; uniform buffers are copied when bound (NVN updates
// them inline in the command stream). The Metal backend (crate::metal) is the reference this one is checked against
// with frame captures (crate::capture).

mod astc;
mod bindless;
mod async_pipe;
pub mod device;
mod exec;
pub mod format;
mod guard;
mod history;
mod inflight;
pub mod listed;
pub mod pipelines;
pub mod shader;
mod shadow;
mod vcache;
pub mod stage_cache;
pub mod state;
pub mod texture;
mod pass_dump;
mod window;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use ash::vk;

use crate::gpu::{Backend, GpuOnlyTexture, ShaderStage, Texels};

pub struct Vulkan {
    ctx: Mutex<Option<exec::Ctx>>,
    /// Programs set (set_program) and not yet taken into the context: (program, stages as Ctx::set_program takes them).
    programs: Mutex<Vec<(usize, Vec<(u64, u64, u64)>)>>,
    serial: AtomicU64,
    frames: AtomicU64,
    /// Start of the current 50-frame interval of the fps line.
    interval: Mutex<Option<std::time::Instant>>,
}

/// Time spent recording submits and waiting for them, since the last fps line.
static RECORD_NS: AtomicU64 = AtomicU64::new(0);
static WAIT_NS: AtomicU64 = AtomicU64::new(0);
/// GPU time of the submits (timestamps around each one's commands).
static GPU_NS: AtomicU64 = AtomicU64::new(0);
static LAST_GPU_NS: AtomicU64 = AtomicU64::new(0);
/// GPU time per pass of the last submit (when profiling passes): (ns, description).
static PASS_PROFILE: Mutex<Vec<(u64, String)>> = Mutex::new(Vec::new());
static LAST_RECORD_NS: AtomicU64 = AtomicU64::new(0);
static LAST_WAIT_NS: AtomicU64 = AtomicU64::new(0);
/// Submits, and blocking one-time submits (texture creation and uploads, reads) with their wait time.
static SUBMITS: AtomicU64 = AtomicU64::new(0);
static ONE_SHOTS: AtomicU64 = AtomicU64::new(0);
static ONE_SHOT_NS: AtomicU64 = AtomicU64::new(0);
/// Frames per second over the last fps line (the window title shows it).
static DISPLAY_FPS: AtomicU64 = AtomicU64::new(0);

pub fn display_fps() -> f32 {
    f64::from_bits(DISPLAY_FPS.load(Ordering::Relaxed)) as f32
}

/// Pipelines created; bytes of host memory filled for the GPU (uniform copies, data the GPU cannot read in place).
static PIPELINES: AtomicU64 = AtomicU64::new(0);
/// Pipelines created in all (the saved pipeline cache is written when it grew).
static PIPELINES_TOTAL: AtomicU64 = AtomicU64::new(0);
static ARENA_BYTES: AtomicU64 = AtomicU64::new(0);
/// This frame's work for --log-hitch: pipelines created, texture uploads, their bytes, ASTC decodes, shader stages translated,
/// recording ns.
static HITCH: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
pub(crate) const HITCH_PIPELINES: usize = 0;
pub(crate) const HITCH_UPLOADS: usize = 1;
pub(crate) const HITCH_UPLOAD_BYTES: usize = 2;
pub(crate) const HITCH_ASTC: usize = 3;
pub(crate) const HITCH_STAGES: usize = 4;
pub(crate) const HITCH_RECORD_NS: usize = 5;
pub(crate) fn hitch_add(what: usize, n: u64) {
    HITCH[what].fetch_add(n, Ordering::Relaxed);
}

/// Counts bytes written for the GPU outside the arena (repacked vertex streams).
pub(crate) fn note_copied(n: usize) {
    ARENA_BYTES.fetch_add(n as u64, Ordering::Relaxed);
}

/// Recording and GPU wait time not yet reported on an fps line, in ms (replays report them at the end).
pub fn timing() -> (f64, f64, f64, u64) {
    let ms = |a: &AtomicU64| a.swap(0, Ordering::Relaxed) as f64 / 1e6;
    (ms(&RECORD_NS), ms(&WAIT_NS), ms(&GPU_NS), texture::FLUSHES.swap(0, Ordering::Relaxed))
}

pub static VULKAN: Vulkan = Vulkan {
    ctx: Mutex::new(None),
    programs: Mutex::new(Vec::new()),
    serial: AtomicU64::new(0),
    frames: AtomicU64::new(0),
    interval: Mutex::new(None),
};

/// Creates the device. Before the guest submits anything.
pub fn init(validation: bool) -> Result<(), String> {
    let dev = device::Dev::new(validation)?;
    *VULKAN.ctx.lock().unwrap() = Some(exec::Ctx::new(dev)?);
    crate::gpu::pipeline_list::fetch();
    Ok(())
}

impl Vulkan {
    /// The programs set since the context last took them, in the order they were set (under the context's lock).
    fn take_programs(&self, c: &mut exec::Ctx) {
        let set = std::mem::take(&mut *self.programs.lock().unwrap());
        for (program, stages) in set {
            c.set_program(program, stages);
        }
    }
}

impl Backend for Vulkan {
    fn set_program(&self, program: usize, stages: &[ShaderStage]) {
        // Queued, and taken in by the next submit, which is when it took effect anyway: the game's threads set programs
        // in gameplay too (TotK on Great Sky Island, now and then on a ModuleSystemWorker), and waiting for the context
        // waited out the whole recording of a submit (~5 ms).
        let stages = stages.iter().map(|s| (s.data, s.control, unsafe { stage_hash(s.data, s.control) })).collect();
        self.programs.lock().unwrap().push((program, stages));
        if let Ok(mut g) = self.ctx.try_lock() {
            if let Some(c) = g.as_mut() {
                self.take_programs(c);
            }
        }
    }

    fn submit(&self, lists: &[u64]) {
        // The thread submitting (the game's Presentation Thread) and vk-record share one L3 cache from the first submit.
        static PINNED: std::sync::Once = std::sync::Once::new();
        PINNED.call_once(|| {
            let mask = unsafe { recording_affinity() };
            if mask != 0 {
                println!("[vulkan] recording threads on processors {mask:#x} (one L3 cache)");
            }
            REC_AFFINITY.store(mask, Ordering::Release);
            unsafe { pin_current_thread(mask) };
        });
        let guard_on = crate::gpu::report_guard::mode() != crate::settings::ReportGuard::Off;
        // Every list the submit reaches, fetched under one lock (a submit reaches ~3,000; one lock each was ~5% of
        // recording) for the executor too.
        // The map and stack of the submit before, kept (growing a fresh map to ~3,000 entries for every submit was ~1% of
        // recording).
        thread_local! {
            static SCRATCH: std::cell::RefCell<(Vec<u64>, rustc_hash::FxHashMap<u64, std::sync::Arc<crate::nvn::api::RecordedCommandList>>)> =
                std::cell::RefCell::new(Default::default());
        }
        let (mut stack, mut submit_lists) = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
        submit_lists.clear();
        let mut samples: Vec<usize> = Vec::new();
        // The report guard needs every list's report addresses before the submit; otherwise vk-decode fetches the lists the
        // roots call while their commands run (decode.rs), and only the roots are taken here.
        if guard_on {
            crate::nvn::api::walk_command_lists(lists, &mut stack, &mut submit_lists, |l| {
                let s = l.summary();
                samples.extend(s.2.iter().map(|&a| crate::nvn::api::translate_virtual(a as usize)));
            });
            crate::gpu::report_guard::begin_submit(&samples);
        } else {
            crate::nvn::api::pin_command_lists(lists, &mut submit_lists);
        }
        let mut g = self.ctx.lock().unwrap();
        let serial = self.serial.load(Ordering::Acquire) + 1;
        match g.as_mut() {
            Some(c) => unsafe {
                self.take_programs(c);
                guard::hold_hot();
                c.submit(lists, serial, &mut submit_lists, !guard_on);
                guard::release_hot();
            },
            None => inflight::watch(inflight::Done {
                serial,
                fence: vk::Fence::null(),
                occl_pool: vk::QueryPool::null(),
                queries: 0,
                occl: Vec::new(),
                writebacks: Vec::new(),
                timestamps: vk::QueryPool::null(),
                pass_labels: Vec::new(),
                rt_writebacks: Vec::new(),
            }),
        }
        // Ctx::submit handed back the map it emptied.
        submit_lists.clear();
        SCRATCH.with(|s| *s.borrow_mut() = (stack, submit_lists));
        self.serial.store(serial, Ordering::Release);
    }

    fn present(&self, texture: usize, crop: [i32; 4]) {
        let n = self.frames.load(Ordering::Acquire) + 1;
        if let Some(c) = self.ctx.lock().unwrap().as_mut() {
            c.write_back_rts();
            unsafe {
                c.present_window(texture, crop);
                c.keep_history(texture, crop, n);
            }
        }
        self.frames.fetch_add(1, Ordering::AcqRel);
        crate::gpu::report_guard::frame_end(n);
        async_pipe::ASYNC_WAITED_US.store(0, Ordering::Relaxed);
        log_hitch(n);
        if n % 50 == 0 {
            let now = std::time::Instant::now();
            let mut start = self.interval.lock().unwrap();
            if let Some(s) = *start {
                DISPLAY_FPS.store((50.0 / now.duration_since(s).as_secs_f64()).to_bits(), Ordering::Relaxed);
                let per_frame = |a: &AtomicU64| a.swap(0, Ordering::Relaxed) as f64 / 50.0 / 1e6;
                println!(
                    "[+] Gfx: present #{n} {:.1} fps, record {:.1} ms/frame, GPU wait {:.1} ms/frame (GPU {:.1}), {} overlap flushes, {:.1} submits, {:.1} one-shots ({:.1} ms), {:.1} pipelines created, {:.1} MB copied",
                    50.0 / now.duration_since(s).as_secs_f64(),
                    per_frame(&RECORD_NS),
                    per_frame(&WAIT_NS),
                    per_frame(&GPU_NS),
                    texture::FLUSHES.swap(0, Ordering::Relaxed),
                    SUBMITS.swap(0, Ordering::Relaxed) as f64 / 50.0,
                    ONE_SHOTS.swap(0, Ordering::Relaxed) as f64 / 50.0,
                    per_frame(&ONE_SHOT_NS),
                    PIPELINES.swap(0, Ordering::Relaxed) as f64 / 50.0,
                    ARENA_BYTES.swap(0, Ordering::Relaxed) as f64 / 50.0 / 1e6,
                );
            }
            *start = Some(now);
        }
        // --hle-history-at=N: the recent HLE calls and the guest threads' waits, as the Metal backend prints them.
        if crate::settings::get().log.hle_history_at == Some(n) {
            crate::horizon::hle::summarize_hle_call_history();
        }
        // --dump-frame=N,..: the presented image as /tmp/nxport_frame_N.ppm (as the Metal backend writes it).
        let dump = &crate::settings::get().dump;
        if dump.dump_frame.contains(&n) {
            let img = self.read_texture(texture, crate::nvn::api::texture_ref_gen(texture), 0, 0);
            match img {
                Some(t) => {
                    let path = format!("/tmp/nxport_frame_{n}.ppm");
                    let _ = std::fs::write(&path, ppm(&t));
                    println!("[+] Gfx: dumped {}x{} frame to {path}", t.width, t.height);
                }
                None => println!("[vulkan] frame {n}: the presented texture could not be read"),
            }
            if dump.exit_on_dump && dump.dump_frame.iter().max() == Some(&n) {
                println!("[+] Dump complete, exiting on request (--exit-on-dump)");
                std::process::exit(0);
            }
        }
    }

    fn submitted_serial(&self) -> u64 {
        self.serial.load(Ordering::Acquire)
    }

    fn wait_serial(&self, serial: u64, timeout_ns: u64) -> bool {
        inflight::wait(serial, timeout_ns)
    }

    fn wait_fence(&self, serial: u64, timeout_ns: u64) -> bool {
        // --early-fence: the frame loop's wait (guest-main) returns once the frame is queued, with the previous frame
        // complete (one frame overlaps) and the pages still-running work reads held by the write guard. Other threads
        // wait for GPU work whose results they read back.
        if !guard::enabled() || !on_guest_main() {
            return inflight::wait(serial, timeout_ns);
        }
        static PREV: AtomicU64 = AtomicU64::new(0);
        let prev = PREV.load(Ordering::Acquire);
        if prev < serial && !inflight::wait(prev, timeout_ns) {
            return false;
        }
        PREV.fetch_max(serial, Ordering::AcqRel);
        if inflight::completed() >= serial {
            guard::release_all();
            return true;
        }
        guard::protect(serial);
        true
    }

    fn early_fence_holds(&self, page: usize) -> bool {
        guard::holds(page)
    }

    fn access_fault(&self, f: &crate::gpu::Fault) -> bool {
        unsafe {
            crate::gpu::report_guard::fault(f.addr, f.pc, f.lr, f.write) || guard::fault(f.addr)
        }
    }

    fn describe_page(&self, addr: usize) -> String {
        let rep = crate::gpu::report_guard::describe(addr);
        if guard::holds(addr) {
            if rep.is_empty() {
                "held by the Vulkan write guard (GPU work in flight reads it)".into()
            } else {
                format!("{rep}; held by the Vulkan write guard (GPU work in flight reads it)")
            }
        } else {
            rep
        }
    }

    fn pages_reprotected(&self, addr: usize, len: usize) {
        unsafe {
            crate::gpu::report_guard::reprotect_held(addr, len);
            guard::reprotect_held(addr, len);
        }
    }

    fn before_host_write(&self, addr: usize, len: usize) {
        unsafe {
            #[cfg(not(nx_native))]
            crate::gpu::report_guard::before_host_write(addr, len);
            guard::before_host_write(addr, len)
        }
    }

    fn presented_frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }

    fn read_texture(&self, texture: usize, gen: u32, level: u32, layer: u32) -> Option<Texels> {
        let mut g = self.ctx.lock().unwrap();
        let c = g.as_mut()?;
        unsafe { c.read_texture(texture, gen, level, layer) }
    }

    fn last_submit_gpu_ns(&self) -> Option<u64> {
        Some(LAST_GPU_NS.load(Ordering::Relaxed))
    }

    fn profile_passes(&self, on: bool) -> Option<Vec<(u64, String)>> {
        if let Some(c) = self.ctx.lock().unwrap().as_mut() {
            c.profile_passes = on;
        }
        Some(std::mem::take(&mut *PASS_PROFILE.lock().unwrap()))
    }

    fn last_submit_cpu_ns(&self) -> Option<(u64, u64)> {
        Some((LAST_RECORD_NS.load(Ordering::Relaxed), LAST_WAIT_NS.load(Ordering::Relaxed)))
    }

    fn gpu_only_textures(&self, f: &mut dyn FnMut(GpuOnlyTexture)) -> usize {
        let mut g = self.ctx.lock().unwrap();
        let Some(c) = g.as_mut() else { return 0 };
        unsafe { c.gpu_only_textures(f) }
    }
}

/// The processors (mask) the recording threads run on, 0 until the first submit chose them, or for none in particular.
/// Recording hands most of what it works out to vk-record (record.rs): batches of commands, descriptors, uniform copies.
/// On a CPU with several L3 caches (Zen 2's core complexes) the two threads landed on different ones much of the time and
/// every handover was a cross-complex cache miss. On the first complex TotK on Great Sky Island went 56.7-61.9 -> 66.5-70.5
/// fps (Ryzen 7 3700X); on the second, 62.8-64.9 (the first L3 is taken, whatever the thread ran on before).
static REC_AFFINITY: AtomicU64 = AtomicU64::new(0);

/// The processors of each L3 cache of processor group 0, the first L3's (the lowest-numbered processor's) first (none:
/// unknown).
#[cfg(windows)]
unsafe fn l3_masks() -> Vec<u64> {
    extern "system" {
        fn GetLogicalProcessorInformationEx(relation: u32, buf: *mut u8, len: *mut u32) -> i32;
    }
    let mut len = 0u32;
    GetLogicalProcessorInformationEx(2, std::ptr::null_mut(), &mut len);
    let mut buf = vec![0u8; len as usize];
    // RelationCache: SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX records with a CACHE_RELATIONSHIP each.
    if len == 0 || GetLogicalProcessorInformationEx(2, buf.as_mut_ptr(), &mut len) == 0 {
        return Vec::new();
    }
    let (mut at, mut masks) = (0usize, Vec::new());
    while at + 56 <= len as usize {
        let r = &buf[at..];
        let size = u32::from_le_bytes(r[4..8].try_into().unwrap()) as usize;
        // Level, then the first GROUP_AFFINITY (mask, group) at 40.
        let (level, mask, group) = (r[8], u64::from_le_bytes(r[40..48].try_into().unwrap()), u16::from_le_bytes(r[48..50].try_into().unwrap()));
        if level == 3 && group == 0 && mask != 0 {
            masks.push(mask);
        }
        if size == 0 {
            break;
        }
        at += size;
    }
    masks.sort_by_key(|m| m.trailing_zeros());
    masks
}

#[cfg(not(windows))]
unsafe fn l3_masks() -> Vec<u64> {
    Vec::new()
}

/// The processors sharing the first L3 cache (0: one L3 for all of them, or unknown).
unsafe fn recording_affinity() -> u64 {
    match l3_masks()[..] {
        [first, _, ..] => first,
        _ => 0,
    }
}

/// The processors of the other L3 caches, where the game's threads run when Vulkan records on the first (0: anywhere).
/// They no longer take the recording threads' cores and cache, and the game's job threads (which hand work to each other
/// tens of thousands of times a second) share one: TotK on Great Sky Island 69.5-70.1 -> 72.0-73.3 fps, recording
/// 12.6-13.0 -> 12.0-12.3 ms a frame (Ryzen 7 3700X).
pub(crate) fn guest_affinity() -> u64 {
    static MASK: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *MASK.get_or_init(|| match unsafe { l3_masks() }[..] {
        [_, ref rest @ ..] if !rest.is_empty() => rest.iter().fold(0, |a, m| a | m),
        _ => 0,
    })
}

/// Keeps the calling thread on processors `mask` (0: leaves it as it is).
pub(crate) unsafe fn pin_current_thread(mask: u64) {
    #[cfg(windows)]
    {
        extern "system" {
            fn GetCurrentThread() -> *mut std::ffi::c_void;
            fn SetThreadAffinityMask(thread: *mut std::ffi::c_void, mask: usize) -> usize;
        }
        if mask != 0 {
            SetThreadAffinityMask(GetCurrentThread(), mask as usize);
        }
    }
    #[cfg(not(windows))]
    let _ = mask;
}

/// The recording threads' processors, once chosen (vk-record follows them).
pub(crate) fn rec_affinity() -> u64 {
    REC_AFFINITY.load(Ordering::Acquire)
}

fn on_guest_main() -> bool {
    thread_local! {
        static MAIN: bool = std::thread::current().name() == Some("guest-main");
    }
    MAIN.with(|m| *m)
}

fn log_hitch(frame: u64) {
    static HITCH_MS: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    let Some(thr) = *HITCH_MS.get_or_init(|| crate::settings::get().log.log_hitch) else {
        return;
    };
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let now = std::time::Instant::now();
    let prev = LAST.lock().unwrap().replace(now);
    let async_started = async_pipe::ASYNC_STARTED.swap(0, Ordering::Relaxed);
    let async_skips = async_pipe::ASYNC_SKIPS.swap(0, Ordering::Relaxed);
    let work: [u64; 6] = std::array::from_fn(|i| HITCH[i].swap(0, Ordering::Relaxed));
    if let Some(p) = prev {
        let ms = (now - p).as_secs_f64() * 1e3;
        if ms > thr {
            println!(
                "[hitch] frame {frame} {ms:.1} ms: recording {:.1} ms, {} pipelines created, {} stages translated, {} texture uploads ({:.1} MB), {} ASTC decodes",
                work[HITCH_RECORD_NS] as f64 / 1e6,
                work[HITCH_PIPELINES],
                work[HITCH_STAGES],
                work[HITCH_UPLOADS],
                work[HITCH_UPLOAD_BYTES] as f64 / 1e6,
                work[HITCH_ASTC]
            );
            if async_started + async_skips > 0 {
                println!("[hitch]   async pipelines: {async_started} compiles started, {async_skips} draws skipped while compiling");
            }
        }
    }
}

/// A content hash of a guest shader stage (its header and first code, and its control section): translations are cached
/// by address and this, as the game loads other shaders into memory it used before.
unsafe fn stage_hash(data: u64, control: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if data >= 0x10000 {
        std::slice::from_raw_parts(data as *const u8, 0x130).hash(&mut h);
    }
    if control >= 0x1_0000_0000 && *(control as *const u32) == 0x9876_1234 {
        std::slice::from_raw_parts(control as *const u8, 0x780).hash(&mut h);
    }
    h.finish()
}

/// A presented image as a binary PPM, rows as the screen shows them (bottom-up in memory for NVN's lower-left origin).
fn ppm(t: &Texels) -> Vec<u8> {
    let (w, h) = (t.width as usize, t.height as usize);
    let rgba = crate::capture::image::to_rgba8(t);
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    let flip = crate::nvn::api::window_origin_mode() == 0;
    for y in 0..h {
        let row = if flip { h - 1 - y } else { y };
        for px in rgba[row * w * 4..(row + 1) * w * 4].chunks_exact(4) {
            out.extend_from_slice(&px[..3]);
        }
    }
    out
}

/// Per-submit host memory for copies (uniform buffers, support and driver buffers, data the device cannot import).
pub(crate) struct Arena {
    /// Chunks this submit uses; chunks of completed submits, for reuse.
    chunks: Vec<device::HostBuffer>,
    free: Vec<device::HostBuffer>,
    /// The chunk being filled and how much of it is used.
    cur: usize,
    used: u64,
    align: u64,
    /// Chunks in host-cached memory: for what the CPU reads back (render-target write-backs). Other chunks are in
    /// host-visible memory the CPU writes and the GPU reads, which is write-combined and very slow for CPU reads.
    cached: bool,
}

impl Arena {
    const CHUNK: u64 = 64 << 20;

    unsafe fn alloc(&mut self, dev: &device::Dev, len: usize) -> Option<(vk::Buffer, u64, *mut u8)> {
        let len = (len as u64).max(16);
        loop {
            let off = self.used.next_multiple_of(self.align);
            if let Some(c) = self.chunks.get(self.cur) {
                if off + len <= c.size {
                    self.used = off + len;
                    return Some((c.buffer, off, c.ptr.add(off as usize)));
                }
                if self.cur + 1 < self.chunks.len() {
                    self.cur += 1;
                    self.used = 0;
                    continue;
                }
            }
            if let Some(i) = self.free.iter().position(|c| c.size >= len) {
                self.chunks.push(self.free.swap_remove(i));
                self.cur = self.chunks.len() - 1;
                self.used = 0;
                continue;
            }
            let usage = vk::BufferUsageFlags::UNIFORM_BUFFER
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::VERTEX_BUFFER
                | vk::BufferUsageFlags::INDEX_BUFFER
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::TRANSFER_DST;
            let chunk = if self.cached {
                dev.readback_buffer((16 << 20u64).max(len), usage).ok()?
            } else {
                dev.host_buffer(Self::CHUNK.max(len), usage).ok()?
            };
            std::ptr::write_bytes(chunk.ptr, 0, chunk.size as usize);
            self.chunks.push(chunk);
            self.cur = self.chunks.len() - 1;
            self.used = 0;
        }
    }

    /// A copy of `data`.
    unsafe fn copy(&mut self, dev: &device::Dev, data: &[u8]) -> Option<(vk::Buffer, u64)> {
        ARENA_BYTES.fetch_add(data.len() as u64, Ordering::Relaxed);
        let (b, off, p) = self.alloc(dev, data.len())?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        guard::patch(data.as_ptr() as usize, p, data.len());
        Some((b, off))
    }

    /// Room for a copy of `len` bytes that vk-record makes (exec::record::Op::Fill).
    unsafe fn reserve(&mut self, dev: &device::Dev, len: usize) -> Option<(vk::Buffer, u64, *mut u8)> {
        ARENA_BYTES.fetch_add(len as u64, Ordering::Relaxed);
        self.alloc(dev, len)
    }

    /// The chunks this submit used (they stay allocated until it completed: making and zeroing 64 MB chunks for every
    /// submit was a visible part of recording).
    fn take_used(&mut self) -> Vec<device::HostBuffer> {
        self.cur = 0;
        self.used = 0;
        std::mem::take(&mut self.chunks)
    }

    /// Chunks of a completed submit; oversized ones are freed.
    unsafe fn give_back(&mut self, dev: &device::Dev, chunks: Vec<device::HostBuffer>) {
        for c in chunks {
            if c.size > Self::CHUNK {
                dev.free_host_buffer(&c);
            } else {
                self.free.push(c);
            }
        }
    }
}

/// Guest memory pools imported as buffers, by page-aligned start.
#[derive(Default)]
pub(crate) struct Imports {
    by_start: BTreeMap<usize, (usize, vk::Buffer, vk::DeviceMemory)>,
    failed: std::collections::HashSet<(usize, usize)>,
    gen: u64,
    /// Imports dropped while a submit was being recorded: its command buffers may still use them.
    pub(super) retired: Vec<(vk::Buffer, vk::DeviceMemory, usize, usize)>,
}

impl Imports {
    /// The buffer holding guest address `addr` for `len` bytes and the offset of `addr` in it, if its pool is imported.
    unsafe fn find(&mut self, dev: &device::Dev, addr: usize, len: usize) -> Option<(vk::Buffer, u64)> {
        let gen = crate::nvn::api::pools_gen();
        if gen != self.gen {
            // Pools changed (registered, finalized, released): imports over memory released since must go. The others
            // stay: their pages are the same (this backend never unmaps freed address regions), and making every import
            // again at each change, which TotK's streaming makes many times a second, was a large part of recording.
            for (a, s, _) in crate::nvn::api::take_released_ranges() {
                let gone: Vec<usize> = self.by_start.range(..a + s).filter(|(&st, v)| st + v.0 > a).map(|(&st, _)| st).collect();
                for st in gone {
                    let (size, b, m) = self.by_start.remove(&st).unwrap();
                    self.retired.push((b, m, st, size));
                }
            }
            self.failed.clear();
            self.gen = gen;
        }
        if let Some((&s, &(size, b, _))) = self.by_start.range(..=addr).next_back() {
            if addr + len <= s + size {
                return Some((b, (addr - s) as u64));
            }
        }
        let (base, size) = crate::nvn::api::containing_pool(addr, len.max(1))?;
        let align = dev.host_memory.as_ref()?.1 as usize;
        let start = base / align * align;
        let end = (base + size).next_multiple_of(align);
        if self.failed.contains(&(start, end)) {
            return None;
        }
        // Before the pages are made writable, so that nothing protects them again in between.
        crate::guest::dirty_track::imported_add(start, end - start);
        // The driver maps imported pages with the protection they have now (NVIDIA on Windows): pages the write guard or
        // the write tracking hold read-only would stay read-only for the GPU, and its first write there loses the device.
        // They are made writable first, as a host write would: the guard waits for the GPU work reading them, and the
        // tracking counts its pages as written (their textures are checked again).
        #[cfg(windows)]
        {
            let read_only = crate::os::read_only_ranges(start, end - start);
            if !read_only.is_empty() {
                guard::before_host_write(start, end - start);
                for (a, b) in read_only {
                    for page in (a & !0x3FFF..b).step_by(0x4000) {
                        crate::guest::dirty_track::handle_fault(page.max(a));
                    }
                }
                // Still read-only (protected by something else): copies until the pools change, then tried again.
                if !crate::os::read_only_ranges(start, end - start).is_empty() {
                    crate::guest::dirty_track::imported_remove(start, end - start);
                    self.failed.insert((start, end));
                    return None;
                }
            }
        }
        match dev.import(start, end - start) {
            Some((b, m)) => {
                self.by_start.insert(start, (end - start, b, m));
                Some((b, (addr - start) as u64))
            }
            None => {
                crate::guest::dirty_track::imported_remove(start, end - start);
                self.failed.insert((start, end));
                None
            }
        }
    }
}
