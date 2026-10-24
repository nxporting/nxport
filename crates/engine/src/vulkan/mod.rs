// Vulkan backend for crate::gpu::Backend.
//
// Correctness first: each submit is one command buffer, recorded from the NVN command lists in order and waited for
// before the submit returns. Images stay in the GENERAL layout with full barriers between passes and other work.
// Guest memory pools are imported as buffers where the device allows it (VK_EXT_external_memory_host), so vertex,
// index and storage data are the guest's own pages, as on the Switch; uniform buffers are copied when bound (NVN updates
// them inline in the command stream). The Metal backend (crate::metal) is the reference this one is checked against
// with frame captures (crate::capture).

mod device;
mod exec;
mod format;
mod shader;
mod state;
mod texture;

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

/// Recording and GPU wait time not yet reported on an fps line, in ms (replays report them at the end).
pub fn timing() -> (f64, f64, u64) {
    let ms = |a: &AtomicU64| a.swap(0, Ordering::Relaxed) as f64 / 1e6;
    (ms(&RECORD_NS), ms(&WAIT_NS), texture::FLUSHES.swap(0, Ordering::Relaxed))
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
    Ok(())
}

impl Backend for Vulkan {
    fn set_program(&self, program: usize, stages: &[ShaderStage]) {
        if let Some(c) = self.ctx.lock().unwrap().as_mut() {
            c.programs.insert(program, stages.iter().map(|s| (s.data, s.control, unsafe { stage_hash(s.data, s.control) })).collect());
        }
    }

    fn submit(&self, lists: &[u64]) {
        if let Some(c) = self.ctx.lock().unwrap().as_mut() {
            unsafe { c.submit(lists) };
        }
        self.serial.fetch_add(1, Ordering::AcqRel);
    }

    fn present(&self, texture: usize, _crop: [i32; 4]) {
        if let Some(c) = self.ctx.lock().unwrap().as_mut() {
            unsafe { c.write_back_rts() };
        }
        let n = self.frames.fetch_add(1, Ordering::AcqRel) + 1;
        if n % 50 == 0 {
            let now = std::time::Instant::now();
            let mut start = self.interval.lock().unwrap();
            if let Some(s) = *start {
                let per_frame = |a: &AtomicU64| a.swap(0, Ordering::Relaxed) as f64 / 50.0 / 1e6;
                println!(
                    "[+] Gfx: present #{n} {:.1} fps, record {:.1} ms/frame, GPU wait {:.1} ms/frame, {} overlap flushes",
                    50.0 / now.duration_since(s).as_secs_f64(),
                    per_frame(&RECORD_NS),
                    per_frame(&WAIT_NS),
                    texture::FLUSHES.swap(0, Ordering::Relaxed)
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

    fn wait_serial(&self, _serial: u64, _timeout_ns: u64) -> bool {
        true
    }

    fn wait_fence(&self, _serial: u64, _timeout_ns: u64) -> bool {
        true
    }

    fn presented_frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }

    fn read_texture(&self, texture: usize, gen: u32, level: u32, layer: u32) -> Option<Texels> {
        let mut g = self.ctx.lock().unwrap();
        let c = g.as_mut()?;
        unsafe { c.read_texture(texture, gen, level, layer) }
    }

    fn gpu_only_textures(&self, f: &mut dyn FnMut(GpuOnlyTexture)) -> usize {
        let mut g = self.ctx.lock().unwrap();
        let Some(c) = g.as_mut() else { return 0 };
        unsafe { c.gpu_only_textures(f) }
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
    chunks: Vec<device::HostBuffer>,
    used: u64,
    align: u64,
}

impl Arena {
    const CHUNK: u64 = 64 << 20;

    unsafe fn alloc(&mut self, dev: &device::Dev, len: usize) -> Option<(vk::Buffer, u64, *mut u8)> {
        let len = (len as u64).max(16);
        let off = self.used.next_multiple_of(self.align);
        let fits = self.chunks.last().is_some_and(|c| off + len <= c.size);
        if !fits {
            let usage = vk::BufferUsageFlags::UNIFORM_BUFFER
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::VERTEX_BUFFER
                | vk::BufferUsageFlags::INDEX_BUFFER
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::TRANSFER_DST;
            let chunk = dev.host_buffer(Self::CHUNK.max(len), usage).ok()?;
            std::ptr::write_bytes(chunk.ptr, 0, chunk.size as usize);
            self.chunks.push(chunk);
            self.used = 0;
            return self.alloc(dev, len as usize);
        }
        let c = self.chunks.last().unwrap();
        self.used = off + len;
        Some((c.buffer, off, c.ptr.add(off as usize)))
    }

    /// A copy of `data`.
    unsafe fn copy(&mut self, dev: &device::Dev, data: &[u8]) -> Option<(vk::Buffer, u64)> {
        let (b, off, p) = self.alloc(dev, data.len())?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        Some((b, off))
    }

    /// Keeps the first chunk for the next submit.
    unsafe fn reset(&mut self, dev: &device::Dev) {
        while self.chunks.len() > 1 {
            let c = self.chunks.pop().unwrap();
            dev.free_host_buffer(&c);
        }
        self.used = 0;
    }
}

/// Guest memory pools imported as buffers, by page-aligned start.
#[derive(Default)]
pub(crate) struct Imports {
    by_start: BTreeMap<usize, (usize, vk::Buffer, vk::DeviceMemory)>,
    failed: std::collections::HashSet<(usize, usize)>,
    gen: u64,
    /// Imports dropped while a submit was being recorded: its command buffer may still use them.
    retired: Vec<(vk::Buffer, vk::DeviceMemory)>,
}

impl Imports {
    /// The buffer holding guest address `addr` for `len` bytes and the offset of `addr` in it, if its pool is imported.
    unsafe fn find(&mut self, dev: &device::Dev, addr: usize, len: usize) -> Option<(vk::Buffer, u64)> {
        let gen = crate::nvn::api::pools_gen();
        if gen != self.gen {
            // Pools changed (registered, finalized, released): imports over released memory must go.
            self.retired.extend(std::mem::take(&mut self.by_start).into_values().map(|(_, b, m)| (b, m)));
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

    /// Frees the imports dropped during the submit that just completed.
    unsafe fn release_retired(&mut self, dev: &device::Dev) {
        for (b, m) in self.retired.drain(..) {
            dev.device.destroy_buffer(b, None);
            dev.device.free_memory(m, None);
        }
    }
}
