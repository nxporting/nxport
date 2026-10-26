// Vulkan backend for crate::gpu::Backend.
//
// Correctness first: each submit is one command buffer, recorded from the NVN command lists in order and waited for
// before the submit returns. Images stay in the GENERAL layout with full barriers between passes and other work.
// Guest memory pools are imported as buffers where the device allows it (VK_EXT_external_memory_host), so vertex,
// index and storage data are the guest's own pages, as on the Switch; uniform buffers are copied when bound (NVN updates
// them inline in the command stream). The Metal backend (crate::metal) is the reference this one is checked against
// with frame captures (crate::capture).

mod astc;
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
mod window;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use ash::vk;

use crate::gpu::{Backend, GpuOnlyTexture, ShaderStage, Texels};

pub struct Vulkan {
    ctx: Mutex<Option<exec::Ctx>>,
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

impl Backend for Vulkan {
    fn set_program(&self, program: usize, stages: &[ShaderStage]) {
        if let Some(c) = self.ctx.lock().unwrap().as_mut() {
            c.programs.insert(program, stages.iter().map(|s| (s.data, s.control, unsafe { stage_hash(s.data, s.control) })).collect());
        }
    }

    fn submit(&self, lists: &[u64]) {
        let guard_on = crate::gpu::report_guard::mode() != crate::settings::ReportGuard::Off;
        if guard_on {
            let mut stack = Vec::new();
            let mut submit_lists = rustc_hash::FxHashMap::default();
            let mut samples: Vec<usize> = Vec::new();
            crate::nvn::api::walk_command_lists(lists, &mut stack, &mut submit_lists, |l| {
                let s = l.summary();
                samples.extend(
                    s.2.iter()
                        .map(|&a| crate::nvn::api::translate_virtual(a as usize)),
                );
            });
            crate::gpu::report_guard::begin_submit(&samples);
        }
        let mut g = self.ctx.lock().unwrap();
        let serial = self.serial.load(Ordering::Acquire) + 1;
        match g.as_mut() {
            Some(c) => unsafe {
                guard::hold_hot();
                c.submit(lists, serial);
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
        unsafe { guard::before_host_write(addr, len) }
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
    if let Some(p) = prev {
        let ms = (now - p).as_secs_f64() * 1e3;
        if ms > thr {
            println!("[hitch] frame {frame} {ms:.1} ms");
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
            let chunk = dev.host_buffer(Self::CHUNK.max(len), usage).ok()?;
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
    pub(super) retired: Vec<(vk::Buffer, vk::DeviceMemory)>,
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
                    let (_, b, m) = self.by_start.remove(&st).unwrap();
                    self.retired.push((b, m));
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
                self.failed.insert((start, end));
                None
            }
        }
    }
}
