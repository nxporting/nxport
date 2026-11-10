// Textures: one Vulkan image per guest texture description (storage address and layout), filled from guest memory when
// it is made, kept in the GENERAL layout. Views per binding, samplers per pool sampler, readback in the guest's texel
// encoding.

use rustc_hash::FxHashMap;

use ash::vk;

use super::device::Dev;
use super::format;
use crate::gpu::Texels;
use crate::nvn::api::{NvnTexture, NvnTextureView, SamplerDesc};
use crate::nvn::layout::{self, TextureParams};

/// A texture as guest memory describes it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Desc {
    pub storage: usize,
    pub params: TextureParams,
}

impl Desc {
    /// The description of the image that holds this memory. A 4-byte colour texture's bytes are read through views in
    /// whatever format each texture has, as on the console: textures that differ only in that, or in flags that do not
    /// change where texels lie, are one image, whether or not they exist at the same time. Games re-create texture objects
    /// over the same memory every frame, and separate images would pass their contents through guest memory.
    pub fn canonical(&self) -> Desc {
        let mut d = *self;
        if format::is_four_byte_color(d.params.format) && d.params.samples <= 1 {
            const RGBA8: i32 = 37;
            const LAYOUT_FLAGS: i32 = layout::NVN_TEXTURE_FLAGS_LINEAR_BIT
                | layout::NVN_TEXTURE_FLAGS_LINEAR_RENDER_TARGET_BIT
                | layout::NVN_TEXTURE_FLAGS_MINIMAL_LAYOUT_BIT;
            d.params.format = RGBA8;
            d.params.flags &= LAYOUT_FLAGS;
        }
        d
    }

    pub fn of(t: &NvnTexture) -> Desc {
        Desc {
            storage: t.storage,
            params: TextureParams {
                target: t.target,
                width: t.width,
                height: t.height,
                depth: t.depth,
                levels: t.levels,
                format: t.format,
                flags: t.flags,
                samples: t.samples,
                stride: t.stride,
            },
        }
    }
}

pub struct Image {
    pub image: vk::Image,
    memory: vk::DeviceMemory,
    pub format: vk::Format,
    pub usage: vk::ImageUsageFlags,
    pub extent: vk::Extent3D,
    pub levels: u32,
    pub layers: u32,
    /// Made cube-compatible (a cube or cube-array texture).
    pub cube: bool,
    pub samples: vk::SampleCountFlags,
    /// GPU work rendered or wrote into it: guest memory no longer has its contents.
    pub gpu_written: bool,
    views: FxHashMap<ViewKey, vk::ImageView>,
    /// The submit it was last checked against guest memory in, the hash of the guest bytes uploaded then (for a
    /// GPU-written image: of the guest bytes when the GPU work completed), and the write-tracking token of those bytes
    /// (guest/dirty_track.rs; None: untracked, hash at every check).
    checked: u64,
    hash: u64,
    token: Option<u64>,
    /// The write-tracking token of the whole pages inside its memory: a change there is the CPU's, as those hold nothing else.
    interior_token: Option<u64>,
    /// A GPU-written image's hash and token were taken (`settle`).
    settled: bool,
    /// GPU work wrote it since its contents were last written to guest memory; when (a global sequence, ordering the
    /// writes of overlapping images) and in which submit.
    unflushed: bool,
    gpu_seq: u64,
    gpu_submit: u64,
    /// Bytes of guest memory its description covers; whether that memory's pool is CPU-writable, at a pool generation.
    guest_len: usize,
    /// Its guest address (for the per-chunk write record, `note_write`).
    storage: usize,
    cpu_writable: (u64, bool),
    /// Made for a render target and not loaded from guest memory yet (`target_image`).
    never_loaded: bool,
    /// The write sequence its contents are as of: its last GPU write, or the upload. A GPU write over the same memory with a
    /// later one has made them stale; the memory's bytes need not be looked at to know.
    content_seq: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ViewKey {
    ty: vk::ImageViewType,
    format: vk::Format,
    base_level: u32,
    levels: u32,
    base_layer: u32,
    layers: u32,
    swizzle: [u8; 4],
    aspect: vk::ImageAspectFlags,
}

#[derive(Default)]
pub struct Textures {
    pub images: FxHashMap<Desc, Image>,
    samplers: Vec<(SamplerDesc, vk::Sampler)>,
    pub warned: std::collections::HashSet<String>,
    /// Counts submits: textures are checked for CPU writes once per submit.
    pub serial: u64,
    /// Alias roots found (see `locate`), kept while no texture over their memory changes (`begin_submit`).
    roots: FxHashMap<Desc, (Desc, u32, u32)>,
    /// Depth images read from guest memory: colour textures sampled with depth comparison, as their depth twin
    /// (layout::depth_twin). Other depth images live on the GPU only.
    pub depth_twins: rustc_hash::FxHashSet<Desc>,
    /// The texture index change sequence the roots are valid at (begin_submit), and scratch for the changes.
    roots_seq: u64,
    root_changes: Vec<(usize, usize)>,
    /// Every image by its guest address, with the end of its guest bytes (the scans below ask for it of every neighbour).
    /// Each image in every chunk of guest memory its bytes cover (CHUNK_SHIFT), with the end of its bytes. Overlap queries
    /// visit the chunks of the range asked about only (a scan from the largest image's size before it visited every image
    /// within that distance, for each texture sampled).
    by_chunk: rustc_hash::FxHashMap<usize, Vec<(Desc, usize)>>,
    /// While a submit is recorded: image creation and uploads go into a command buffer queued ahead of the submit's next
    /// chunk (Ctx::submit_chunk), instead of blocking one-time submits; with the staging memory to free once it ran.
    batch: Option<Batch>,
    /// The ASTC decoder (devices without ASTC), made with the first ASTC upload.
    astc: Option<super::astc::Decoder>,
    /// Rebuilds images from the other images over their memory on the GPU (relayout.rs), made when first needed;
    /// `relayout_failed` if it could not be made (the CPU route is taken).
    relayout: Option<super::relayout::Relayout>,
    relayout_failed: bool,
    /// Its working buffer in device memory (the shader's many small accesses to host memory over the bus made each
    /// rebuild take milliseconds), with its size. Rebuilds of every submit use it in queue order, each behind a full
    /// barrier. Outgrown ones may still be in use by queued work: they are kept until `destroy` (their sizes double, so
    /// that is less than the current one).
    relayout_scratch: Option<(vk::Buffer, vk::DeviceMemory, u64)>,
    relayout_outgrown: Vec<(vk::Buffer, vk::DeviceMemory)>,
    /// Staging buffers of completed submits, for the next uploads (recycle_staging): making and freeing one for each
    /// upload was a steady part of recording (the driver's allocation calls).
    spare_staging: Vec<super::device::HostBuffer>,
    spare_bytes: u64,
}

/// Staging memory kept for reuse at most.
const SPARE_STAGING_MAX: u64 = 256 << 20;

struct Batch {
    pool: vk::CommandPool,
    cb: Option<vk::CommandBuffer>,
    staging: Vec<super::device::HostBuffer>,
}

fn image_type(target: i32) -> (vk::ImageType, vk::ImageViewType) {
    use layout::*;
    match target {
        NVN_TARGET_1D => (vk::ImageType::TYPE_1D, vk::ImageViewType::TYPE_1D),
        NVN_TARGET_1D_ARRAY => (vk::ImageType::TYPE_1D, vk::ImageViewType::TYPE_1D_ARRAY),
        NVN_TARGET_3D => (vk::ImageType::TYPE_3D, vk::ImageViewType::TYPE_3D),
        NVN_TARGET_2D_ARRAY | NVN_TARGET_2D_MULTISAMPLE_ARRAY => (vk::ImageType::TYPE_2D, vk::ImageViewType::TYPE_2D_ARRAY),
        NVN_TARGET_CUBEMAP => (vk::ImageType::TYPE_2D, vk::ImageViewType::CUBE),
        NVN_TARGET_CUBEMAP_ARRAY => (vk::ImageType::TYPE_2D, vk::ImageViewType::CUBE_ARRAY),
        _ => (vk::ImageType::TYPE_2D, vk::ImageViewType::TYPE_2D),
    }
}

/// The Vulkan format for an NVN format on this device: depth formats it lacks become the wider ones that hold them.
pub unsafe fn device_format(dev: &Dev, fmt: i32) -> Option<vk::Format> {
    let f = format::texture(fmt)?;
    let ok = |f: vk::Format| {
        let p = dev.instance.get_physical_device_format_properties(dev.pdev, f);
        !p.optimal_tiling_features.is_empty()
    };
    if ok(f) {
        return Some(f);
    }
    // Decoded on upload (vulkan/astc.rs); the decoder needs push descriptors.
    if super::astc::is_astc(fmt) && dev.push_ext.is_some() {
        return Some(super::astc::decoded_format(fmt));
    }
    let alt = match f {
        vk::Format::D24_UNORM_S8_UINT => vk::Format::D32_SFLOAT_S8_UINT,
        vk::Format::X8_D24_UNORM_PACK32 => vk::Format::D32_SFLOAT,
        _ => return None,
    };
    ok(alt).then_some(alt)
}

fn samples(n: i32) -> vk::SampleCountFlags {
    match n {
        2 => vk::SampleCountFlags::TYPE_2,
        4 => vk::SampleCountFlags::TYPE_4,
        8 => vk::SampleCountFlags::TYPE_8,
        16 => vk::SampleCountFlags::TYPE_16,
        _ => vk::SampleCountFlags::TYPE_1,
    }
}

/// A texture's memory size, measured the same way for every texture (declared size or layout, whichever is larger).
fn mem_size(t: &NvnTexture, l: &layout::TextureLayout) -> u64 {
    (t.size as u64).max(l.total)
}

/// A strict order among same-size textures over the same memory, so every alias of it picks the same root.
fn order(d: &Desc) -> (i32, i32, i32, i32, i32, i32, i64) {
    let p = &d.params;
    (p.levels, p.depth, p.target, p.format, p.flags, p.width, p.stride)
}

/// The submit being recorded (Image::mark_gpu_write) and a sequence ordering GPU writes.
static SUBMIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static GPU_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Images written to guest memory by flush_overlaps (reported with the fps line).
pub static FLUSHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// The size of the chunks of guest memory the image index is kept by (2^CHUNK_SHIFT bytes). A tuning number: it trades the
/// entries large images take against the images a query visits; the answers are the same.
const CHUNK_SHIFT: u32 = 16;

/// The latest GPU write sequence in each chunk of guest memory (CHUNK_SHIFT): an image's memory saw no write newer than its
/// contents when none of its chunks did, which overwritten_since answers without visiting the images there.
static CHUNK_WRITES: std::sync::Mutex<Option<rustc_hash::FxHashMap<usize, u64>>> = std::sync::Mutex::new(None);

fn note_write(storage: usize, len: usize, seq: u64) {
    let mut m = CHUNK_WRITES.lock().unwrap();
    let m = m.get_or_insert_with(Default::default);
    for c in storage >> CHUNK_SHIFT..=(storage + len.max(1) - 1) >> CHUNK_SHIFT {
        let e = m.entry(c).or_insert(0);
        *e = (*e).max(seq);
    }
}

/// The latest write sequence in any chunk of [start, end).
fn latest_write(start: usize, end: usize) -> u64 {
    let m = CHUNK_WRITES.lock().unwrap();
    let Some(m) = m.as_ref() else { return 0 };
    (start >> CHUNK_SHIFT..=(end.max(start + 1) - 1) >> CHUNK_SHIFT).filter_map(|c| m.get(&c)).copied().max().unwrap_or(0)
}

/// Images rebuilt from the images over their memory on the GPU (rebuild_on_gpu).
pub static REBUILDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Image {
    /// GPU work of the submit being recorded writes this image.
    pub fn mark_gpu_write(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        self.gpu_written = true;
        self.unflushed = true;
        self.gpu_seq = GPU_SEQ.fetch_add(1, Relaxed) + 1;
        self.content_seq = self.gpu_seq;
        self.gpu_submit = SUBMIT.load(Relaxed);
        note_write(self.storage, self.guest_len, self.gpu_seq);
    }
}

impl Textures {
    /// A submit starts recording.
    pub fn begin_submit(&mut self) {
        self.serial += 1;
        SUBMIT.store(self.serial, std::sync::atomic::Ordering::Relaxed);
        // Alias roots stay valid while no texture over their memory was indexed or unindexed.
        let mut changes = std::mem::take(&mut self.root_changes);
        changes.clear();
        let (seq, complete) = crate::nvn::api::index_changes_since(self.roots_seq, &mut changes);
        self.roots_seq = seq;
        if !complete || self.roots.len() > 1 << 15 {
            self.roots.clear();
        } else if !changes.is_empty() {
            changes.sort_unstable();
            // Merged, so a lookup is one binary search.
            let mut merged: Vec<(usize, usize)> = Vec::with_capacity(changes.len());
            for &(a, b) in &changes {
                match merged.last_mut() {
                    Some(m) if a <= m.1 => m.1 = m.1.max(b),
                    _ => merged.push((a, b)),
                }
            }
            self.roots.retain(|d, _| {
                let i = merged.partition_point(|m| m.1 <= d.storage);
                !(i < merged.len() && merged[i].0 <= d.storage)
            });
        }
        self.root_changes = changes;
    }

    /// Where texture `t` lives: the description of the image holding it and the level / layer it starts at there.
    /// Several texture objects over the same memory (a mip level or layer of a larger texture, a render target over a
    /// mip chain, another format of the same texels) share the image of the largest live texture containing them, so
    /// rendering through one is visible through the others, as on the GPU (the Metal backend's alias views).
    pub unsafe fn locate(&mut self, t: &NvnTexture) -> (Desc, u32, u32) {
        self.locate_desc(&Desc::of(t))
    }

    /// `locate` by the texture's description.
    pub unsafe fn locate_desc(&mut self, d: &Desc) -> (Desc, u32, u32) {
        let d = d.canonical();
        if let Some(&r) = self.roots.get(&d) {
            return r;
        }
        let mut r = (d, 0, 0);
        // Roots are strictly larger in (size, order), so the chain ends.
        for _ in 0..4 {
            let Some((root, level, layer)) = Self::alias_root(&r.0) else { break };
            r = (root, r.1 + level, r.2 + layer);
        }
        self.roots.insert(d, r);
        r
    }

    /// The largest live texture whose memory holds `d` at one of its mip levels / layers with `d`'s shape there and a
    /// view-compatible format.
    unsafe fn alias_root(d: &Desc) -> Option<(Desc, u32, u32)> {
        let p = &d.params;
        if d.storage <= 0x10000 || p.samples > 1 || p.width <= 0 || p.height <= 0 || p.target == layout::NVN_TARGET_BUFFER {
            return None;
        }
        let my_layout = layout::compute_layout(p);
        let my_size = my_layout.total;
        let my_layers = match p.target {
            layout::NVN_TARGET_3D => 1,
            layout::NVN_TARGET_CUBEMAP => 6,
            _ => my_layout.layers.max(1),
        };
        let layered = |target: i32| {
            use layout::*;
            matches!(target, NVN_TARGET_2D | NVN_TARGET_2D_ARRAY | NVN_TARGET_CUBEMAP | NVN_TARGET_CUBEMAP_ARRAY | NVN_TARGET_RECTANGLE)
        };
        let mut best: Option<(u64, Desc, u32, u32)> = None;
        for (_, _, c) in crate::nvn::api::textures_reaching(d.storage, crate::nvn::api::max_texture_span()) {
            let cd = Desc::of(&c).canonical();
            if cd == *d || c.samples > 1 || c.width <= 0 || c.height <= 0 || c.target == layout::NVN_TARGET_BUFFER {
                continue;
            }
            if !format::alias_compatible(p.format, c.format) {
                continue;
            }
            let lay = layout::compute_layout(&cd.params);
            let size = mem_size(&c, &lay);
            if (size, order(&cd)) <= (my_size, order(d)) || d.storage as u64 >= c.storage as u64 + size {
                continue;
            }
            if !(c.target == p.target || (layered(c.target) && layered(p.target))) {
                continue;
            }
            let rel = (d.storage - c.storage) as u64;
            let (slice, in_layer) = if lay.layer_stride > 0 { (rel / lay.layer_stride, rel % lay.layer_stride) } else { (0, rel) };
            let Some(level) = lay.level_offsets.iter().position(|&o| o == in_layer) else { continue };
            if ((c.width >> level).max(1), (c.height >> level).max(1)) != (p.width, p.height) {
                continue;
            }
            let c_layers = match c.target {
                layout::NVN_TARGET_3D => 1,
                layout::NVN_TARGET_CUBEMAP => 6,
                _ => lay.layers.max(1),
            };
            if level + p.levels.max(1) as usize > c.levels.max(1) as usize || slice as u32 + my_layers > c_layers {
                continue;
            }
            if best.as_ref().is_none_or(|b| (size, order(&cd)) > (b.0, order(&b.1))) {
                best = Some((size, cd, level as u32, slice as u32));
            }
        }
        best.map(|(_, cd, level, slice)| (cd, level, slice))
    }

    /// The image holding texture `t` (made and filled from guest memory the first time), and the level and layer `t`
    /// starts at in it.
    pub unsafe fn get(&mut self, dev: &Dev, t: &NvnTexture) -> Option<(&mut Image, u32, u32)> {
        let (d, level, layer) = self.locate(t);
        let img = self.get_desc(dev, &d)?;
        Some((img, level, layer))
    }

    /// The image of description `d`.
    pub unsafe fn get_desc(&mut self, dev: &Dev, d: &Desc) -> Option<&mut Image> {
        let d = *d;
        if !self.images.contains_key(&d) {
            let img = self.create(dev, &d)?;
            self.images.insert(d, img);
            self.index_image(&d);
            self.refresh(dev, &d, true);
        } else {
            let first = self.images.get_mut(&d).is_some_and(|i| std::mem::take(&mut i.never_loaded));
            self.refresh(dev, &d, first);
        }
        self.images.get_mut(&d)
    }

    /// The image a render target names, with its contents not brought up to date: a pass loads them when a draw could see
    /// them (`load_target`), and a clear of the whole image makes that unnecessary (`target_replaced`).
    pub unsafe fn target_image(&mut self, dev: &Dev, t: &NvnTexture) -> Option<(Desc, &mut Image, u32, u32)> {
        let (d, level, layer) = self.locate(t);
        if !self.images.contains_key(&d) {
            let mut img = self.create(dev, &d)?;
            img.never_loaded = true;
            self.images.insert(d, img);
            self.index_image(&d);
        }
        Some((d, self.images.get_mut(&d)?, level, layer))
    }

    /// A pass's first draw can see this target's old contents: they come up to date, then GPU work writes it.
    pub unsafe fn load_target(&mut self, dev: &Dev, d: &Desc) {
        if let Some(img) = self.get_desc(dev, d) {
            img.mark_gpu_write();
        }
    }

    /// A pass clears the whole image: nothing it held or that guest memory holds can be seen. Guest memory may have changed
    /// since its baseline, so it takes a new one when this submit completes (`settle`).
    pub fn target_replaced(&mut self, d: &Desc) {
        if let Some(img) = self.images.get_mut(d) {
            img.never_loaded = false;
            img.settled = false;
            img.mark_gpu_write();
        }
    }

    /// Uploads the texture again when the CPU wrote its guest memory since the last upload (the game fills textures
    /// after creating them: streamed assets, glyphs rasterized into the font atlas; and reuses memory for new contents).
    /// Once per submit; skipped while write tracking shows its pages untouched. An image GPU work wrote holds contents
    /// guest memory lacks: it is uploaded again only when the CPU changed the guest bytes since that work completed (the
    /// game reused the memory for a texture it fills itself), and from then on follows guest memory again.
    unsafe fn refresh(&mut self, dev: &Dev, d: &Desc, first: bool) {
        let serial = self.serial;
        let Some(img) = self.images.get_mut(d) else { return };
        if !first && (img.checked == serial || (img.gpu_written && !img.settled)) {
            return;
        }
        img.checked = serial;
        let gen = crate::nvn::api::pools_gen();
        if img.cpu_writable.0 != gen {
            img.cpu_writable = (gen, d.storage > 0x10000 && crate::nvn::api::memory_pool_cpu_writable(d.storage));
        }
        if !img.cpu_writable.1 {
            if first {
                self.flush_overlaps(dev, d);
                self.upload(dev, d);
            } else if self.overwritten_since(d) {
                // The CPU cannot reach this memory, but the GPU aliases it as on the console: other images' writes over
                // it since its contents are what it now holds.
                if self.rebuild_on_gpu(dev, d, true, true).is_some() {
                    if let Some(img) = self.images.get_mut(d) {
                        img.content_seq = GPU_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    }
                }
            }
            return;
        }
        let len = img.guest_len;
        if len == 0 {
            return;
        }
        // Two questions: did the CPU write this memory, and did the GPU write over it after these contents?
        let overwritten = self.overwritten_since(d);
        let changed = self.cpu_changed(d);
        if !first && !overwritten && !changed {
            return;
        }
        // GPU contents of other images over this memory reach it first: on the GPU where their layouts allow, else
        // through guest memory.
        // The CPU did not write it since its contents: they can be the base.
        let rebuilt = self.rebuild_on_gpu(dev, d, false, !first && !changed);
        // The CPU route ends in an upload, which leaves depth and multisampled images as they are: their read-backs
        // would change nothing in them.
        let uploads = self.images.get(d).is_some_and(|i| i.samples == vk::SampleCountFlags::TYPE_1 && !format::is_depth(i.format));
        if rebuilt.is_none() && uploads {
            self.flush_overlaps(dev, d);
        }
        let Some(img) = self.images.get_mut(d) else { return };
        Self::take_baseline(d, img);
        img.content_seq = GPU_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        img.gpu_written = false;
        img.settled = false;
        // Rebuilt from its own GPU contents too: guest memory still lacks them.
        img.unflushed = rebuilt == Some(true);
        if rebuilt.is_none() {
            self.upload(dev, d);
        }
    }

    /// The relayout working buffer, at least `len` bytes (made, or replaced by one twice as large, as needed).
    unsafe fn relayout_scratch(&mut self, dev: &Dev, len: u64) -> Option<vk::Buffer> {
        if let Some((b, _, size)) = self.relayout_scratch {
            if size >= len {
                return Some(b);
            }
        }
        let size = len.max(1 << 20).next_power_of_two();
        let usage = vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::STORAGE_BUFFER;
        let buffer = dev.device.create_buffer(&vk::BufferCreateInfo::default().size(size).usage(usage), None).ok()?;
        let req = dev.device.get_buffer_memory_requirements(buffer);
        let Ok(memory) = dev.alloc(req, vk::MemoryPropertyFlags::DEVICE_LOCAL) else {
            dev.device.destroy_buffer(buffer, None);
            return None;
        };
        if dev.device.bind_buffer_memory(buffer, memory, 0).is_err() {
            dev.device.destroy_buffer(buffer, None);
            dev.device.free_memory(memory, None);
            return None;
        }
        if let Some((b, m, _)) = self.relayout_scratch.replace((buffer, memory, size)) {
            self.relayout_outgrown.push((b, m));
        }
        Some(buffer)
    }

    /// refresh's route when GPU contents of other images lie over `d`'s memory: what flush_overlaps and an upload do,
    /// recorded in a command buffer instead of waiting for read-backs (relayout.rs). `d` is rebuilt from its guest bytes
    /// with those images' rows swizzled over them, oldest write first, and then deswizzled into its levels. Guest memory
    /// is left as it was; those images stay unflushed, as flush_overlaps leaves them, so later uploads still take their
    /// contents. `d`'s own GPU contents not in guest memory are among them, in their place in write order (where no
    /// later write covers them they are what its memory holds); Some(true) if so. `any_pool`: `d` lies in memory the CPU
    /// cannot reach (writers there count however the CPU route would treat them). `from_image`: the CPU has not written
    /// `d`'s memory since its contents (see the incremental route below). None, with nothing recorded, when no
    /// such image lies over it or a layout is one the shader does not move (pitch-linear, 3D, multisampled or depth `d`,
    /// decoded ASTC, blocks whose size does not divide 16, levels not 16-byte aligned): refresh then takes the CPU route.
    unsafe fn rebuild_on_gpu(&mut self, dev: &Dev, d: &Desc, any_pool: bool, from_image: bool) -> Option<bool> {
        if self.relayout_failed || d.storage <= 0x10000 {
            return None;
        }
        let total = layout::compute_layout(&d.params).total as usize;
        let (start, end) = (d.storage, d.storage + total);
        let mut group: Vec<(u64, Desc)> = self.unflushed_over(start, end);
        if !any_pool {
            group.retain(|(_, o)| {
                let keep = self.writer_kept(o);
                if !keep {
                    self.no_longer_unflushed(o);
                }
                keep
            });
        }
        if group.is_empty() {
            return None;
        }
        group.sort_by_key(|h| h.0);
        // From the image itself: its contents are what its memory held when they were made (a rebuild, or GPU work,
        // which refreshes it first), so the writers before them are in them already. Only the newer ones are applied, over
        // the image's own blocks under their bytes, and only those blocks are written back. Else from the guest bytes.
        let content = self.images.get(d)?.content_seq;
        let newer: Vec<(u64, Desc)> = group.iter().filter(|(s, o)| o != d && *s > content).copied().collect();
        let incremental = from_image && !newer.is_empty();
        let has_self = if incremental { self.images.get(d)?.unflushed } else { group.iter().any(|h| h.1 == *d) };
        let group = if incremental { newer } else { group };
        // Whether the shader moves this image's levels: block-linear 2D, single-sampled, the guest's own format, block
        // sizes dividing 16 bytes, starting 16-byte aligned.
        let movable = |o: &Desc, img: &Image| -> bool {
            let Some(info) = layout::format_info(o.params.format) else { return false };
            o.params.target != layout::NVN_TARGET_3D
                && img.extent.depth <= 1
                && img.samples == vk::SampleCountFlags::TYPE_1
                && 16 % info.bytes == 0
                && o.storage % 16 == 0
                && format::texture(o.params.format) == Some(img.format)
                && img.extent.width == o.params.width.max(1) as u32
                && img.extent.height == o.params.height.max(1) as u32
                && layout::block_linear_level(&o.params, 0).is_some()
        };
        let Some(dimg) = self.images.get(d) else { return None };
        // Depth images in D16 or D32F hold the guest's bytes as they are and copy to and from buffers (the depth aspect);
        // other depth formats do not.
        let depth_ok = !format::is_depth(dimg.format) || matches!(dimg.format, vk::Format::D16_UNORM | vk::Format::D32_SFLOAT);
        if total % 16 != 0 || !depth_ok || !movable(d, dimg) {
            return None;
        }
        // One buffer: the guest bytes, then each level's rows read out of the images over them, then `d`'s levels.
        let g_len = total as u32;
        let mut off = total as u64;
        let mut reads: Vec<(vk::Image, vk::BufferImageCopy)> = Vec::new();
        let mut moves: Vec<super::relayout::Params> = Vec::new();
        // The rows of `level` of an image as the shader moves them: (pitch of the rows in the buffer, Params), with the
        // copy region set to read or write them there.
        // `rect`: (first byte of a row, end byte, first texel row, texel rows), the end clamped to the level's rows.
        let rows_of = |o: &Desc, level: u32, rect: (u32, u32, u32, u32), at: usize, region: &mut vk::BufferImageCopy, width: u32, off: u64| {
            let info = layout::format_info(o.params.format)?;
            let (pitch, bh) = layout::block_linear_level(&o.params, level as usize)?;
            let (x0, x1, y, rows) = rect;
            let base = at as i64 - d.storage as i64;
            if base % 16 != 0 || y % info.block_h != 0 || x0 % 64 != 0 || i32::try_from(base).is_err() {
                return None;
            }
            let x1 = x1.min(width.div_ceil(info.block_w) * info.bytes);
            if x0 >= x1 {
                return None;
            }
            let row_bytes = x1 - x0;
            let lin_pitch = row_bytes.next_multiple_of(16);
            let tx = x0 / info.bytes * info.block_w;
            region.buffer_offset = off;
            region.buffer_row_length = lin_pitch / info.bytes * info.block_w;
            region.buffer_image_height = 0;
            region.image_offset.x = tx as i32;
            region.image_offset.y = y as i32;
            region.image_extent.width = (row_bytes / info.bytes * info.block_w).min(width - tx);
            region.image_extent.height = rows;
            let p = super::relayout::Params {
                lin_word: (off / 4) as u32,
                lin_pitch,
                row_bytes,
                rows: rows.div_ceil(info.block_h),
                y0: y / info.block_h,
                g_word: 0,
                g_base: base as i32,
                g_len,
                pitch: pitch as u32,
                bh: bh as u32,
                to_rows: 0,
                x0,
            };
            Some((lin_pitch as u64 * p.rows as u64, p))
        };
        // Incremental: per level and layer of `d`, the blocks the newer writers' bytes fall in (their bounding rectangle),
        // read from `d` first as the base the writers go over.
        let mut own: Vec<(u32, u32, usize, (u32, u32, u32, u32))> = Vec::new();
        if incremental {
            let lay = layout::compute_layout(&d.params);
            for layer in 0..dimg.layers {
                for level in 0..dimg.levels {
                    let at = d.storage + layer as usize * lay.layer_stride as usize + lay.level_offsets[level as usize] as usize;
                    let size = lay.level_sizes[level as usize] as usize;
                    let mut r: Option<(u32, u32, u32, u32)> = None;
                    for (_, o) in &group {
                        let o_end = o.storage + layout::compute_layout(&o.params).total as usize;
                        let (a, b) = (o.storage.max(at), o_end.min(at + size));
                        if a >= b {
                            continue;
                        }
                        let Some(q) = layout::rect_of_bytes(&d.params, level as usize, (a - at) as u64, (b - at) as u64) else { continue };
                        r = Some(match r {
                            None => q,
                            Some(p) => {
                                let (y0, y1) = (p.2.min(q.2), (p.2 + p.3).max(q.2 + q.3));
                                (p.0.min(q.0), p.1.max(q.1), y0, y1 - y0)
                            }
                        });
                    }
                    if let Some(r) = r {
                        own.push((layer, level, at, r));
                    }
                }
            }
            for &(layer, level, at, rect) in &own {
                let Some((mut region, width, _, _)) = Self::read_region(dimg, d.params.format, level, layer) else { return None };
                let Some((len, p)) = rows_of(d, level, rect, at, &mut region, width, off) else { return None };
                reads.push((dimg.image, region));
                moves.push(p);
                off += len;
            }
        }
        for (_, o) in &group {
            let Some(img) = self.images.get(o) else { continue };
            // flush_overlaps skips 3D images too.
            if o.params.target == layout::NVN_TARGET_3D {
                continue;
            }
            if !movable(o, img) {
                return None;
            }
            let lay = layout::compute_layout(&o.params);
            for layer in 0..img.layers {
                let slice = o.storage + layer as usize * lay.layer_stride as usize;
                for level in 0..img.levels {
                    let at = slice + lay.level_offsets[level as usize] as usize;
                    let (from, to) = (start.saturating_sub(at) as u64, end.saturating_sub(at) as u64);
                    let Some(rect) = layout::rect_of_bytes(&o.params, level as usize, from, to) else { continue };
                    let Some((mut region, width, _, _)) = Self::read_region(img, o.params.format, level, layer) else { continue };
                    let Some((len, p)) = rows_of(o, level, rect, at, &mut region, width, off) else { return None };
                    reads.push((img.image, region));
                    moves.push(p);
                    off += len;
                }
            }
        }
        if moves.is_empty() {
            return None;
        }
        let mut writes: Vec<vk::BufferImageCopy> = Vec::new();
        let mut unmoves: Vec<super::relayout::Params> = Vec::new();
        let lay = layout::compute_layout(&d.params);
        if incremental {
            for &(layer, level, at, rect) in &own {
                let Some((mut region, width, _, _)) = Self::read_region(dimg, d.params.format, level, layer) else { return None };
                let Some((len, mut p)) = rows_of(d, level, rect, at, &mut region, width, off) else { return None };
                p.to_rows = 1;
                writes.push(region);
                unmoves.push(p);
                off += len;
            }
        } else {
            for layer in 0..dimg.layers {
                for level in 0..dimg.levels {
                    let at = d.storage + layer as usize * lay.layer_stride as usize + lay.level_offsets[level as usize] as usize;
                    let Some((mut region, width, height, _)) = Self::read_region(dimg, d.params.format, level, layer) else { return None };
                    let Some((len, mut p)) = rows_of(d, level, (0, u32::MAX, 0, height), at, &mut region, width, off) else { return None };
                    p.to_rows = 1;
                    writes.push(region);
                    unmoves.push(p);
                    off += len;
                }
            }
        }
        let dimage = dimg.image;
        if self.relayout.is_none() {
            self.relayout = super::relayout::Relayout::new(dev);
            if self.relayout.is_none() {
                self.relayout_failed = true;
                println!("[vulkan] the relayout shader could not be made: overlapping images are read back through the CPU");
                return None;
            }
        }
        let Some(scratch) = self.relayout_scratch(dev, off) else { return None };
        // The guest bytes as the base (not needed incrementally).
        let buf = if incremental {
            None
        } else {
            let Ok(buf) = self.staging(dev, total) else { return None };
            std::ptr::copy_nonoverlapping(d.storage as *const u8, buf.ptr, total);
            Some(buf)
        };
        let rl = self.relayout.take().unwrap();
        let batched = self.batch.is_some();
        self.record(dev, |cb| {
            // Submits still in flight may read the old contents; the images read were written by earlier submits.
            dev.full_barrier(cb);
            dev.checkpoint(cb, "texture rebuilt from overlaps (W, H, format, guest address)", [
                d.params.width as u64,
                d.params.height as u64,
                d.params.format as u64,
                d.storage as u64,
            ]);
            if let Some(buf) = &buf {
                dev.device.cmd_copy_buffer(cb, buf.buffer, scratch, &[vk::BufferCopy::default().size(total as u64)]);
            }
            for (image, region) in &reads {
                dev.device.cmd_copy_image_to_buffer(cb, *image, vk::ImageLayout::GENERAL, scratch, &[*region]);
            }
            dev.full_barrier(cb);
            rl.bind(dev, cb, scratch);
            // In write order: a later write's bytes replace an earlier one's.
            for p in &moves {
                rl.dispatch(dev, cb, *p);
                dev.full_barrier(cb);
            }
            for p in &unmoves {
                rl.dispatch(dev, cb, *p);
            }
            dev.full_barrier(cb);
            dev.device.cmd_copy_buffer_to_image(cb, scratch, dimage, vk::ImageLayout::GENERAL, &writes);
            dev.full_barrier(cb);
        });
        self.relayout = Some(rl);
        FLUSHES.fetch_add(moves.len() as u64, std::sync::atomic::Ordering::Relaxed);
        REBUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(buf) = buf {
            match &mut self.batch {
                Some(b) if batched => b.staging.push(buf),
                // Recorded and run (one_shot waits).
                _ => self.recycle_staging(dev, buf),
            }
        }
        Some(has_self)
    }

    /// After a submit completed: GPU-written images not yet settled take the hash and write-tracking token of their guest
    /// bytes, against which later CPU writes are detected.
    pub unsafe fn settle(&mut self) {
        for (d, img) in self.images.iter_mut() {
            if img.gpu_written && !img.settled {
                Self::settle_one(d, img);
            }
        }
    }

    /// Before guest memory under `d` is read for an upload: GPU contents of other images over those bytes reach it, oldest
    /// write first, so the bytes are what the GPU's writes in their order left there. Only the rows holding those bytes are
    /// read back, in one wait; the images stay unflushed, as the rest of their memory is not written.
    ///
    /// Whether the CPU changed an image's bytes is decided before any write (it drops that image's GPU contents), and the
    /// images these writes touch take the bytes as their new baseline: a write-back is not a CPU write.
    unsafe fn flush_overlaps(&mut self, dev: &Dev, d: &Desc) {
        if d.storage <= 0x10000 {
            return;
        }
        let (start, end) = (d.storage, d.storage + layout::compute_layout(&d.params).total as usize);
        let mut group: Vec<(u64, Desc)> = self.unflushed_over(start, end).into_iter().filter(|h| h.1 != *d).collect();
        group.retain(|(_, o)| {
            let keep = self.writer_kept(o);
            if !keep {
                self.no_longer_unflushed(o);
            }
            keep
        });
        if group.is_empty() {
            return;
        }
        group.sort_by_key(|h| h.0);
        // (image, level, layer, the buffer offset, rows, where the rows go in guest memory)
        struct Part {
            d: Desc,
            level: u32,
            layer: u32,
            offset: u64,
            y: u32,
            width: u32,
            rows: u32,
            region: vk::BufferImageCopy,
        }
        let mut parts: Vec<Part> = Vec::new();
        let mut total = 0u64;
        let (mut low, mut high) = (usize::MAX, 0usize);
        for (_, o) in &group {
            let Some(img) = self.images.get(o) else { continue };
            let lay = layout::compute_layout(&o.params);
            if o.params.target == layout::NVN_TARGET_3D {
                continue;
            }
            for layer in 0..img.layers {
                let slice = o.storage + layer as usize * lay.layer_stride as usize;
                for level in 0..img.levels {
                    let at = slice + lay.level_offsets[level as usize] as usize;
                    let (from, to) = (start.saturating_sub(at) as u64, end.saturating_sub(at) as u64);
                    let Some((y, rows)) = layout::rows_of_bytes(&o.params, level as usize, from, to) else { continue };
                    let Some((mut region, width, _, _)) = Self::read_region(img, o.params.format, level, layer) else { continue };
                    let info = layout::format_info(o.params.format).unwrap();
                    region.buffer_offset = total;
                    region.image_offset.y = y as i32;
                    region.image_extent.height = rows;
                    let bytes = width.div_ceil(info.block_w) as u64 * rows.div_ceil(info.block_h) as u64 * info.bytes as u64;
                    parts.push(Part { d: *o, level, layer, offset: total, y, width, rows, region });
                    total += bytes;
                    low = low.min(at);
                    high = high.max(at + lay.level_sizes[level as usize] as usize);
                }
            }
        }
        if parts.is_empty() {
            return;
        }
        // The images these writes touch that the CPU had not changed.
        let others: Vec<Desc> = self.images_over(low, high).into_iter().filter(|o| self.guest_can_take(o) && !self.cpu_changed(o)).collect();
        let Ok(buf) = dev.host_buffer(total, vk::BufferUsageFlags::TRANSFER_DST) else { return };
        let done = dev.one_shot(|cb| {
            dev.full_barrier(cb);
            for p in &parts {
                let image = self.images[&p.d].image;
                dev.device.cmd_copy_image_to_buffer(cb, image, vk::ImageLayout::GENERAL, buf.buffer, &[p.region]);
            }
        });
        if done.is_ok() {
            for p in &parts {
                let lay = layout::compute_layout(&p.d.params);
                layout::write_texels_2d(
                    p.d.storage as *mut u8,
                    p.layer as usize * lay.layer_stride as usize,
                    &p.d.params,
                    p.level as usize,
                    0,
                    p.y as i32,
                    p.width as i32,
                    p.rows as i32,
                    buf.ptr.add(p.offset as usize),
                );
                FLUSHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        dev.free_host_buffer(&buf);
        for o in &others {
            if let Some(img) = self.images.get_mut(o) {
                Self::take_baseline(o, img);
            }
        }
    }

    fn index_image(&mut self, d: &Desc) {
        let len = layout::compute_layout(&d.params).total as usize;
        let end = d.storage + len.max(1);
        for c in d.storage >> CHUNK_SHIFT..=(end - 1) >> CHUNK_SHIFT {
            self.by_chunk.entry(c).or_default().push((*d, end));
        }
    }

    /// Calls `f` once for each image whose bytes overlap [start, end), with the end of its bytes: from the first chunk
    /// where the image and the range meet.
    fn for_each_over(&self, start: usize, end: usize, mut f: impl FnMut(&Desc, usize) -> bool) {
        if start >= end {
            return;
        }
        let first = start >> CHUNK_SHIFT;
        for c in first..=(end - 1) >> CHUNK_SHIFT {
            let Some(v) = self.by_chunk.get(&c) else { continue };
            for (o, o_end) in v {
                if *o_end <= start || o.storage >= end || (o.storage >> CHUNK_SHIFT).max(first) != c {
                    continue;
                }
                if !f(o, *o_end) {
                    return;
                }
            }
        }
    }

    /// The images over this range of guest memory.
    fn images_over(&self, start: usize, end: usize) -> Vec<Desc> {
        let mut over = Vec::new();
        self.for_each_over(start, end, |o, _| {
            over.push(*o);
            true
        });
        over
    }

    /// Whether GPU work of a completed submit wrote over image `d`'s memory after the contents it holds. Work of the
    /// submit being recorded cannot be read back yet.
    fn overwritten_since(&self, d: &Desc) -> bool {
        let Some(img) = self.images.get(d) else { return false };
        let end = d.storage + layout::compute_layout(&d.params).total as usize;
        // No chunk under it was written after its contents (its own writes set its contents' sequence): nothing to find.
        if latest_write(d.storage, end) <= img.content_seq {
            return false;
        }
        let mut found = false;
        self.for_each_over(d.storage, end, |o, _| {
            found = o != d && self.images.get(o).is_some_and(|i| i.gpu_seq > img.content_seq && i.gpu_submit != self.serial);
            !found
        });
        found
    }

    /// (write sequence, image) of each image with unflushed GPU contents from completed submits over this memory.
    fn unflushed_over(&self, start: usize, end: usize) -> Vec<(u64, Desc)> {
        let mut hits = Vec::new();
        self.for_each_over(start, end, |o, _| {
            if let Some(img) = self.images.get(o) {
                if img.unflushed && img.gpu_submit != self.serial {
                    hits.push((img.gpu_seq, *o));
                }
            }
            true
        });
        hits
    }

    fn no_longer_unflushed(&mut self, d: &Desc) {
        if let Some(img) = self.images.get_mut(d) {
            img.unflushed = false;
        }
    }

    /// Whether a writer's GPU contents over another image's memory still count: its memory is one the CPU can reach and
    /// the CPU has not written it since. A writer whose submit has not completed has no baseline yet (settle takes it), so
    /// the CPU cannot have overwritten what its work writes.
    unsafe fn writer_kept(&self, o: &Desc) -> bool {
        let unsettled = self.images.get(o).is_some_and(|i| i.gpu_written && !i.settled);
        self.guest_can_take(o) && (unsettled || !self.cpu_changed(o))
    }

    /// Whether image `d`'s guest memory is in a pool the CPU can reach, so what it holds can matter.
    fn guest_can_take(&self, d: &Desc) -> bool {
        let len = layout::compute_layout(&d.params).total as usize;
        crate::nvn::api::memory_pool_cpu_writable(d.storage) && crate::nvn::api::containing_pool(d.storage, len).is_some()
    }

    /// Whether the CPU changed image `d`'s bytes since its baseline. Pages at its ends may hold other data, whose writes
    /// also change its token: there the bytes decide.
    unsafe fn cpu_changed(&self, d: &Desc) -> bool {
        let Some(img) = self.images.get(d) else { return true };
        Self::cpu_changed_of(d, img)
    }

    unsafe fn cpu_changed_of(d: &Desc, img: &Image) -> bool {
        use crate::guest::dirty_track;
        let len = layout::compute_layout(&d.params).total as usize;
        if img.token.is_some() && dirty_track::generation(d.storage, len) == img.token {
            return false;
        }
        if img.token.is_some() {
            let (at, n) = interior_pages(d.storage, len);
            if n > 0 && dirty_track::generation(at, n) != img.interior_token {
                return true;
            }
        }
        baseline_hash(d.storage, len, img.token.is_some()) != img.hash
    }

    /// The bytes guest memory holds now become image `d`'s baseline for detecting CPU writes.
    unsafe fn take_baseline(d: &Desc, img: &mut Image) {
        use crate::guest::dirty_track;
        let len = layout::compute_layout(&d.params).total as usize;
        // The interior's generations are read before protecting, as protect reads the whole range's. Read after, a write
        // in between (TotK's font thread writes glyphs into its atlas all the time) had reopened a page: the token was
        // None, a later check's None matched it, and the hash below leaves the interior out, so no glyph written inside
        // the atlas from then on was uploaded (letters missing from menu text, 2026-11-08).
        let (at, n) = interior_pages(d.storage, len);
        let interior = if n > 0 { dirty_track::generations(at, n) } else { None };
        // Protect before hashing: writes during the hash change the token.
        img.token = if dirty_track::trackable(d.storage, len) { dirty_track::protect(d.storage, len) } else { None };
        if img.token.is_some() {
            img.interior_token = interior;
        }
        img.hash = baseline_hash(d.storage, len, img.token.is_some());
    }

    /// settle for one image (also after a write-back changed its guest bytes).
    pub unsafe fn resettle(&mut self, d: &Desc) {
        if let Some(img) = self.images.get_mut(d) {
            if img.gpu_written {
                Self::settle_one(d, img);
            }
        }
    }

    unsafe fn settle_one(d: &Desc, img: &mut Image) {
        img.settled = true;
        if d.storage <= 0x10000 || !crate::nvn::api::memory_pool_cpu_writable(d.storage) {
            return;
        }
        Self::take_baseline(d, img);
    }

    pub unsafe fn create(&mut self, dev: &Dev, d: &Desc) -> Option<Image> {
        let p = &d.params;
        let Some(fmt) = device_format(dev, p.format) else {
            if self.warned.insert(format!("fmt{}", p.format)) {
                println!("[vulkan] texture format {} not supported", p.format);
            }
            return None;
        };
        // A decoded ASTC texture: UNORM whatever its sampled format, so the decoder can write it as a storage image.
        let fmt = if super::astc::is_astc(p.format) && format::texture(p.format) != Some(fmt) {
            if p.target == layout::NVN_TARGET_3D {
                return None;
            }
            vk::Format::R8G8B8A8_UNORM
        } else {
            fmt
        };
        if p.target == layout::NVN_TARGET_BUFFER {
            if self.warned.insert("buffer".into()) {
                println!("[vulkan] buffer textures are not implemented");
            }
            return None;
        }
        let (ity, _) = image_type(p.target);
        let lay = layout::compute_layout(p);
        let layers = match p.target {
            layout::NVN_TARGET_3D => 1,
            layout::NVN_TARGET_CUBEMAP => 6,
            _ => lay.layers.max(1),
        };
        let extent = vk::Extent3D {
            width: p.width.max(1) as u32,
            height: if ity == vk::ImageType::TYPE_1D { 1 } else { p.height.max(1) as u32 },
            depth: if ity == vk::ImageType::TYPE_3D { p.depth.max(1) as u32 } else { 1 },
        };
        let props = dev.instance.get_physical_device_format_properties(dev.pdev, fmt).optimal_tiling_features;
        let mut usage = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST;
        if props.contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT) {
            usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
        }
        if props.contains(vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT) {
            usage |= vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT;
        }
        if props.contains(vk::FormatFeatureFlags::STORAGE_IMAGE) && p.samples <= 1 {
            usage |= vk::ImageUsageFlags::STORAGE;
        }
        let mut flags = vk::ImageCreateFlags::MUTABLE_FORMAT;
        // Viewed as any 4-byte colour format (format::alias_compatible), each with the usages it supports.
        if format::is_four_byte_color(p.format) && p.samples <= 1 {
            flags |= vk::ImageCreateFlags::EXTENDED_USAGE;
            usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::STORAGE;
        }
        if matches!(p.target, layout::NVN_TARGET_CUBEMAP | layout::NVN_TARGET_CUBEMAP_ARRAY) && layers % 6 == 0 {
            flags |= vk::ImageCreateFlags::CUBE_COMPATIBLE;
        }
        // 3D textures the GPU renders into: render passes attach one depth slice through a 2D view.
        if ity == vk::ImageType::TYPE_3D && usage.contains(vk::ImageUsageFlags::COLOR_ATTACHMENT) {
            flags |= vk::ImageCreateFlags::TYPE_2D_ARRAY_COMPATIBLE;
        }
        // 3D textures shaders sample as 2D (one slice): sampled_view_of.
        if ity == vk::ImageType::TYPE_3D && dev.view_2d_of_3d {
            flags |= vk::ImageCreateFlags::TYPE_2D_VIEW_COMPATIBLE_EXT;
        }
        let levels = (p.levels.max(1) as u32).min(32 - extent.width.max(extent.height).max(extent.depth).leading_zeros());
        let info = vk::ImageCreateInfo::default()
            .image_type(ity)
            .format(fmt)
            .extent(extent)
            .mip_levels(levels)
            .array_layers(layers)
            .samples(samples(p.samples))
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .flags(flags)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = match dev.device.create_image(&info, None) {
            Ok(i) => i,
            Err(e) => {
                println!("[vulkan] image {}x{}x{} fmt {} ({fmt:?}): {e}", extent.width, extent.height, layers, p.format);
                return None;
            }
        };
        let req = dev.device.get_image_memory_requirements(image);
        let memory = dev.alloc(req, vk::MemoryPropertyFlags::DEVICE_LOCAL).ok()?;
        dev.device.bind_image_memory(image, memory, 0).ok()?;
        let aspect = format::aspects(fmt);
        // Block-compressed images cannot be cleared (their uploads fill them).
        let compressed = layout::format_info(p.format).is_some_and(|i| i.block_w > 1);
        self.record(dev, |cb| {
            let b = [vk::ImageMemoryBarrier::default()
                .image(image)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::GENERAL)
                .dst_access_mask(vk::AccessFlags::MEMORY_WRITE | vk::AccessFlags::MEMORY_READ)
                .subresource_range(
                    vk::ImageSubresourceRange::default().aspect_mask(aspect).level_count(levels).layer_count(layers),
                )];
            dev.device.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &b,
            );
            // Defined contents for what the upload leaves (depth formats, multisample images): zeros, as guest memory
            // that was never written holds.
            let range = vk::ImageSubresourceRange::default().aspect_mask(aspect).level_count(levels).layer_count(layers);
            if format::is_depth(fmt) {
                let v = vk::ClearDepthStencilValue { depth: 0.0, stencil: 0 };
                dev.device.cmd_clear_depth_stencil_image(cb, image, vk::ImageLayout::GENERAL, &v, &[range]);
            } else if !compressed {
                let v = vk::ClearColorValue { uint32: [0; 4] };
                dev.device.cmd_clear_color_image(cb, image, vk::ImageLayout::GENERAL, &v, &[range]);
            }
            dev.full_barrier(cb);
        });
        crate::gpu::history::journal(|| {
            format!(
                "texture new {:x}: {}x{}x{} fmt {} lv {} flags {:#x} storage {:#x}",
                vk::Handle::as_raw(image),
                p.width,
                p.height,
                p.depth,
                p.format,
                p.levels,
                p.flags,
                d.storage
            )
        });
        Some(Image {
            image,
            memory,
            format: fmt,
            usage,
            extent,
            levels,
            layers,
            cube: flags.contains(vk::ImageCreateFlags::CUBE_COMPATIBLE),
            samples: samples(p.samples),
            gpu_written: false,
            views: FxHashMap::default(),
            checked: 0,
            hash: 0,
            token: None,
            settled: false,
            unflushed: false,
            gpu_seq: 0,
            gpu_submit: 0,
            guest_len: layout::compute_layout(&d.params).total as usize,
            storage: d.storage,
            cpu_writable: (u64::MAX, false),
            never_loaded: false,
            interior_token: None,
            content_seq: 0,
        })
    }

    /// Fills every level / layer from guest memory (block-linear or pitch-linear), as the Metal backend does.
    unsafe fn upload(&mut self, dev: &Dev, d: &Desc) {
        let Some(img) = self.images.get(d) else { return };
        let p = &d.params;
        crate::gpu::history::journal(|| {
            format!("upload {:x}: {}x{} fmt {} storage {:#x}", ash::vk::Handle::as_raw(img.image), p.width, p.height, p.format, d.storage)
        });
        if d.storage <= 0x10000 || img.samples != vk::SampleCountFlags::TYPE_1 || (format::is_depth(img.format) && !self.depth_twins.contains(d)) {
            return;
        }
        let aspect = format::aspects(img.format);
        let Some(info) = layout::format_info(p.format) else { return };
        let lay = layout::compute_layout(p);
        let linear = p.flags & 0x10 != 0 || p.flags & 0x40 != 0;
        let mut data: Vec<u8> = Vec::new();
        let mut regions = Vec::new();
        let is3d = p.target == layout::NVN_TARGET_3D;
        let levels = if linear { 1 } else { img.levels };
        let layers = if is3d { 1 } else { img.layers };
        for layer in 0..layers {
            let slice = layer as usize * lay.layer_stride as usize;
            for level in 0..levels {
                let lw = (img.extent.width >> level).max(1) as usize;
                let lh = (img.extent.height >> level).max(1) as usize;
                let ld = if is3d { (img.extent.depth >> level).max(1) as usize } else { 1 };
                let (wb, hb) = (lw.div_ceil(info.block_w as usize), lh.div_ceil(info.block_h as usize));
                let bpr = wb * info.bytes as usize;
                let off = data.len();
                data.resize(off + bpr * hb * ld, 0);
                let ok = if is3d {
                    layout::read_texels_3d(d.storage as *const u8, p, level as usize, data[off..].as_mut_ptr())
                } else if linear {
                    let pitch = if p.stride > 0 { p.stride as usize } else { bpr };
                    for r in 0..hb {
                        std::ptr::copy_nonoverlapping(
                            (d.storage + slice + r * pitch) as *const u8,
                            data[off + r * bpr..].as_mut_ptr(),
                            bpr,
                        );
                    }
                    true
                } else {
                    layout::read_texels_2d(
                        d.storage as *const u8,
                        slice,
                        p,
                        level as usize,
                        0,
                        0,
                        lw as i32,
                        lh as i32,
                        data[off..].as_mut_ptr(),
                    )
                };
                if !ok {
                    data.truncate(off);
                    continue;
                }
                regions.push(
                    vk::BufferImageCopy::default()
                        .buffer_offset(off as u64)
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(aspect)
                                .mip_level(level)
                                .base_array_layer(layer)
                                .layer_count(1),
                        )
                        .image_extent(vk::Extent3D { width: lw as u32, height: lh as u32, depth: ld as u32 }),
                );
                // Copies must start on a texel block boundary.
                data.resize(data.len().next_multiple_of(16), 0);
            }
        }
        if regions.is_empty() {
            return;
        }
        if super::astc::is_astc(p.format) && img.format == vk::Format::R8G8B8A8_UNORM {
            let (bw, bh) = (info.block_w as u32, info.block_h as u32);
            self.upload_astc(dev, d, &data, &regions, bw, bh);
            return;
        }
        let image = img.image;
        let Ok(staging) = self.staging(dev, data.len()) else { return };
        super::hitch_add(super::HITCH_UPLOADS, 1);
        super::hitch_add(super::HITCH_UPLOAD_BYTES, data.len() as u64);
        std::ptr::copy_nonoverlapping(data.as_ptr(), staging.ptr, data.len());
        let batched = self.batch.is_some();
        self.record(dev, |cb| {
            // Submits still in flight may read the old contents.
            dev.full_barrier(cb);
            dev.checkpoint(cb, "texture upload (W, H, format, guest address)", [p.width as u64, p.height as u64, p.format as u64, d.storage as u64]);
            dev.device.cmd_copy_buffer_to_image(cb, staging.buffer, image, vk::ImageLayout::GENERAL, &regions);
        });
        match &mut self.batch {
            Some(b) if batched => b.staging.push(staging),
            // Recorded and run (one_shot waits).
            _ => self.recycle_staging(dev, staging),
        }
    }

    /// An ASTC upload where the device has no ASTC: `data` holds each region's blocks, which the decoder writes into the
    /// image's levels and layers.
    unsafe fn upload_astc(&mut self, dev: &Dev, d: &Desc, data: &[u8], regions: &[vk::BufferImageCopy], bw: u32, bh: u32) {
        super::hitch_add(super::HITCH_ASTC, 1);
        if self.astc.is_none() {
            self.astc = super::astc::Decoder::new(dev);
            if self.astc.is_none() {
                if self.warned.insert("astc".into()) {
                    println!("[vulkan] the ASTC decoder could not be made: ASTC textures stay empty");
                }
                return;
            }
        }
        let Some(img) = self.images.get_mut(d) else { return };
        let srgb = super::astc::decoded_format(d.params.format) == vk::Format::R8G8B8A8_SRGB;
        let mut jobs = Vec::new();
        for r in regions {
            let s = &r.image_subresource;
            let unorm = vk::Format::R8G8B8A8_UNORM;
            let ty = vk::ImageViewType::TYPE_2D;
            let Some(view) = Self::view(dev, img, ty, unorm, s.mip_level, 1, s.base_array_layer, 1, [2, 3, 4, 5], false) else {
                continue;
            };
            let (w, h) = (r.image_extent.width, r.image_extent.height);
            let p = super::astc::Params {
                offset: (r.buffer_offset / 4) as u32,
                blocks_w: w.div_ceil(bw),
                width: w,
                height: h,
                bw,
                bh,
                srgb: srgb as u32,
            };
            jobs.push((view, p));
        }
        let Ok(staging) = self.staging(dev, data.len()) else { return };
        std::ptr::copy_nonoverlapping(data.as_ptr(), staging.ptr, data.len());
        let decoder = self.astc.take().unwrap();
        let batched = self.batch.is_some();
        self.record(dev, |cb| {
            // Submits still in flight may read the old contents.
            dev.full_barrier(cb);
            dev.checkpoint(cb, "ASTC decode (W, H, format, guest address)", [d.params.width as u64, d.params.height as u64, d.params.format as u64, d.storage as u64]);
            for (view, p) in &jobs {
                decoder.record(dev, cb, staging.buffer, *view, *p);
            }
            dev.full_barrier(cb);
        });
        self.astc = Some(decoder);
        match &mut self.batch {
            Some(b) if batched => b.staging.push(staging),
            // Recorded and run (one_shot waits).
            _ => self.recycle_staging(dev, staging),
        }
    }

    /// A staging buffer of at least `len` bytes: a spare one not much larger, else a new one (sizes rounded up to powers
    /// of two, so spares fit later uploads).
    unsafe fn staging(&mut self, dev: &Dev, len: usize) -> Result<super::device::HostBuffer, String> {
        let len = len as u64;
        let best = self.spare_staging.iter().enumerate().filter(|(_, b)| b.size >= len && b.size <= len.max(64 << 10) * 4).min_by_key(|(_, b)| b.size);
        if let Some((i, _)) = best {
            let b = self.spare_staging.swap_remove(i);
            self.spare_bytes -= b.size;
            return Ok(b);
        }
        dev.host_buffer(
            len.max(64 << 10).next_power_of_two(),
            vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::STORAGE_BUFFER,
        )
    }

    /// A staging buffer no submit uses any more: kept for later uploads, or freed past SPARE_STAGING_MAX.
    pub unsafe fn recycle_staging(&mut self, dev: &Dev, b: super::device::HostBuffer) {
        if self.spare_bytes + b.size > SPARE_STAGING_MAX {
            dev.free_host_buffer(&b);
            return;
        }
        self.spare_bytes += b.size;
        self.spare_staging.push(b);
    }

    /// Image creation and uploads of the submit being recorded go into command buffers from `pool` (`take_batch`).
    pub fn begin_batch(&mut self, pool: vk::CommandPool) {
        self.batch = Some(Batch { pool, cb: None, staging: Vec::new() });
    }

    /// The batch's commands recorded since the last take (ended), and the staging memory they read.
    pub unsafe fn take_batch(&mut self, dev: &Dev) -> (Option<vk::CommandBuffer>, Vec<super::device::HostBuffer>) {
        let Some(b) = self.batch.as_mut() else { return (None, Vec::new()) };
        let cb = b.cb.take();
        if let Some(cb) = cb {
            dev.full_barrier(cb);
            let _ = dev.device.end_command_buffer(cb);
        }
        (cb, std::mem::take(&mut b.staging))
    }

    /// After the submit: blocking one-time submits again (reads, presents, captures).
    pub fn end_batch(&mut self) {
        self.batch = None;
    }

    /// Records `f` into the batch when a submit is being recorded, else submits it alone and waits.
    /// A buffer copy, in the command buffer queued ahead of what is being recorded (shadow.rs: filling a slot from one
    /// a running submit writes).
    pub(super) unsafe fn record_copy(&mut self, dev: &Dev, src: vk::Buffer, src_off: u64, dst: vk::Buffer, dst_off: u64, n: u64) {
        self.record(dev, |cb| {
            dev.full_barrier(cb);
            dev.checkpoint(cb, "buffer copy (source offset, destination offset, bytes)", [src_off, dst_off, n, 0]);
            let region = vk::BufferCopy { src_offset: src_off, dst_offset: dst_off, size: n };
            dev.device.cmd_copy_buffer(cb, src, dst, &[region]);
            dev.full_barrier(cb);
        });
    }

    unsafe fn record(&mut self, dev: &Dev, f: impl FnOnce(vk::CommandBuffer)) {
        let Some(b) = self.batch.as_mut() else {
            let _ = dev.one_shot(f);
            return;
        };
        if b.cb.is_none() {
            let Ok(cbs) = dev.device.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(b.pool).command_buffer_count(1)) else {
                let _ = dev.one_shot(f);
                return;
            };
            let _ = dev.device.begin_command_buffer(cbs[0], &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT));
            // After whatever the queue ran before.
            dev.full_barrier(cbs[0]);
            b.cb = Some(cbs[0]);
        }
        f(b.cb.unwrap());
    }

    /// A view of `img` for sampling or rendering.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn view(
        dev: &Dev,
        img: &mut Image,
        ty: vk::ImageViewType,
        fmt: vk::Format,
        base_level: u32,
        levels: u32,
        base_layer: u32,
        layers: u32,
        swizzle: [u8; 4],
        depth_only: bool,
    ) -> Option<vk::ImageView> {
        let mut aspect = format::aspects(fmt);
        if depth_only && aspect.contains(vk::ImageAspectFlags::DEPTH) {
            aspect = vk::ImageAspectFlags::DEPTH;
        }
        let base_level = base_level.min(img.levels - 1);
        let levels = levels.clamp(1, img.levels - base_level);
        // 2D views of a 3D image name its depth slices (at the level) as layers.
        let slices = if img.extent.depth > 1 && ty != vk::ImageViewType::TYPE_3D {
            (img.extent.depth >> base_level).max(1)
        } else {
            img.layers
        };
        let base_layer = base_layer.min(slices - 1);
        let layers = layers.clamp(1, slices - base_layer);
        // A cube view needs a cube-compatible image and whole cubes (Metal aborts on others); a shader declaring a cube
        // where the game binds another texture reads nothing (a null descriptor).
        if matches!(ty, vk::ImageViewType::CUBE | vk::ImageViewType::CUBE_ARRAY) && (!img.cube || layers % 6 != 0) {
            return None;
        }
        let key = ViewKey { ty, format: fmt, base_level, levels, base_layer, layers, swizzle, aspect };
        if let Some(&v) = img.views.get(&key) {
            return Some(v);
        }
        let comp = |s: u8, identity: vk::ComponentSwizzle| match s {
            0 => vk::ComponentSwizzle::ZERO,
            1 => vk::ComponentSwizzle::ONE,
            2 => vk::ComponentSwizzle::R,
            3 => vk::ComponentSwizzle::G,
            4 => vk::ComponentSwizzle::B,
            5 => vk::ComponentSwizzle::A,
            _ => identity,
        };
        use vk::ComponentSwizzle as C;
        let props = dev.instance.get_physical_device_format_properties(dev.pdev, fmt).optimal_tiling_features;
        let mut view_usage = img.usage;
        if !props.contains(vk::FormatFeatureFlags::STORAGE_IMAGE) {
            view_usage &= !vk::ImageUsageFlags::STORAGE;
        }
        if !props.contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT) {
            view_usage &= !vk::ImageUsageFlags::COLOR_ATTACHMENT;
        }
        if !props.contains(vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT) {
            view_usage &= !vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT;
        }
        let mut usage_info = vk::ImageViewUsageCreateInfo::default().usage(view_usage);
        let create_info = vk::ImageViewCreateInfo::default()
            .push_next(&mut usage_info)
            .image(img.image)
            .view_type(ty)
            .format(fmt)
            .components(vk::ComponentMapping {
                r: comp(swizzle[0], C::R),
                g: comp(swizzle[1], C::G),
                b: comp(swizzle[2], C::B),
                a: comp(swizzle[3], C::A),
            })
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(aspect)
                    .base_mip_level(base_level)
                    .level_count(levels)
                    .base_array_layer(base_layer)
                    .layer_count(layers),
            );
        let v = dev.device.create_image_view(&create_info, None).ok()?;
        img.views.insert(key, v);
        Some(v)
    }

    /// A sampling view of texture `t` through the pool view `view` (levels, layers, format, swizzle).
    pub unsafe fn sampled_view(&mut self, dev: &Dev, t: &NvnTexture, view: Option<&NvnTextureView>, declared: vk::ImageViewType) -> Option<vk::ImageView> {
        let (img, level, layer) = self.get(dev, t)?;
        Self::sampled_view_of(dev, img, (level, layer), t, view, declared)
    }

    /// sampled_view on a given image of `t` (its render-target feedback snapshot), `t` starting at level / layer `at`.
    pub unsafe fn sampled_view_of(
        dev: &Dev,
        img: &mut Image,
        at: (u32, u32),
        t: &NvnTexture,
        view: Option<&NvnTextureView>,
        declared: vk::ImageViewType,
    ) -> Option<vk::ImageView> {
        let fmt_nvn = view.map(|v| v.format as i32).filter(|&f| f != 0 && format::view_compatible(t.format, f)).unwrap_or(t.format);
        let fmt = device_format(dev, fmt_nvn).filter(|f| format::aspects(*f) == format::aspects(img.format)).unwrap_or(img.format);
        let own_levels = (t.levels.max(1) as u32).min(img.levels.saturating_sub(at.0).max(1));
        let own_layers = match t.target {
            layout::NVN_TARGET_3D => 1,
            layout::NVN_TARGET_CUBEMAP => 6,
            _ => layout::layer_count(&Desc::of(t).params),
        }
        .min(img.layers.saturating_sub(at.1).max(1));
        let (base_level, levels, base_layer, layers) = match view {
            Some(v) => (
                v.min_level as u32,
                if v.num_levels == 0 { own_levels } else { v.num_levels as u32 },
                v.min_layer as u32,
                if v.num_layers == 0 { own_layers } else { v.num_layers as u32 },
            ),
            None => (0, own_levels, 0, own_layers),
        };
        let (base_level, base_layer) = (base_level + at.0, base_layer + at.1);
        // A view's swizzle replaces the texture's.
        let swizzle = match view {
            Some(v) if v.swizzle != [0; 4] => v.swizzle,
            _ if t.swizzle != [0; 4] => t.swizzle,
            _ => [2, 3, 4, 5],
        };
        let swizzle = format::packed_swizzle(fmt_nvn, swizzle);
        let layers = match declared {
            vk::ImageViewType::CUBE => 6,
            vk::ImageViewType::TYPE_2D | vk::ImageViewType::TYPE_1D | vk::ImageViewType::TYPE_3D => 1,
            _ => layers,
        };
        Self::view(dev, img, declared, fmt, base_level, levels, base_layer, layers, swizzle, true)
    }

    pub unsafe fn sampler(&mut self, dev: &Dev, d: &SamplerDesc) -> Option<vk::Sampler> {
        if let Some((_, s)) = self.samplers.iter().find(|(k, _)| k == d) {
            return Some(*s);
        }
        let (min, mip) = match d.min_filter {
            0 => (vk::Filter::NEAREST, None),
            1 => (vk::Filter::LINEAR, None),
            2 => (vk::Filter::NEAREST, Some(vk::SamplerMipmapMode::NEAREST)),
            3 => (vk::Filter::LINEAR, Some(vk::SamplerMipmapMode::NEAREST)),
            4 => (vk::Filter::NEAREST, Some(vk::SamplerMipmapMode::LINEAR)),
            _ => (vk::Filter::LINEAR, Some(vk::SamplerMipmapMode::LINEAR)),
        };
        let wrap = |w: i32| match w {
            1 => vk::SamplerAddressMode::REPEAT,
            2..=4 => vk::SamplerAddressMode::MIRROR_CLAMP_TO_EDGE,
            5 => vk::SamplerAddressMode::CLAMP_TO_BORDER,
            6 => vk::SamplerAddressMode::MIRRORED_REPEAT,
            _ => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        };
        let [r, g, b, a] = d.border_color;
        let border = if a < 0.5 {
            vk::BorderColor::FLOAT_TRANSPARENT_BLACK
        } else if r + g + b > 1.5 {
            vk::BorderColor::FLOAT_OPAQUE_WHITE
        } else {
            vk::BorderColor::FLOAT_OPAQUE_BLACK
        };
        let compare = |f: i8| match f {
            1 => vk::CompareOp::NEVER,
            2 => vk::CompareOp::LESS,
            3 => vk::CompareOp::EQUAL,
            4 => vk::CompareOp::LESS_OR_EQUAL,
            5 => vk::CompareOp::GREATER,
            6 => vk::CompareOp::NOT_EQUAL,
            7 => vk::CompareOp::GREATER_OR_EQUAL,
            _ => vk::CompareOp::ALWAYS,
        };
        // Whole sample counts, as the hardware takes them (the Metal backend rounds the same way).
        let aniso = d.max_anisotropy.round().clamp(1.0, 16.0);
        let info = vk::SamplerCreateInfo::default()
            .min_filter(min)
            .mag_filter(if d.mag_filter != 0 { vk::Filter::LINEAR } else { vk::Filter::NEAREST })
            .mipmap_mode(mip.unwrap_or(vk::SamplerMipmapMode::NEAREST))
            .address_mode_u(wrap(d.wrap_s))
            .address_mode_v(wrap(d.wrap_t))
            .address_mode_w(wrap(d.wrap_r))
            // No mip filter: the view's base level only, whatever the LOD clamp says (Metal's NotMipmapped; Ryujinx's
            // SamplerHolder sets 0 / 0.25 likewise). Clamping to lod_min sampled another level: Super Mario Odyssey's
            // mip chains (samplers clamped to the level being read, no mip filter) went wrong and the Cap Kingdom ground
            // came out as blocks.
            .min_lod(if mip.is_none() { 0.0 } else { d.lod_min })
            .max_lod(if mip.is_none() { 0.25 } else { d.lod_max })
            .mip_lod_bias(d.lod_bias)
            .anisotropy_enable(aniso > 1.0)
            .max_anisotropy(aniso)
            .compare_enable(d.compare_mode != 0)
            .compare_op(compare(d.compare_func))
            .border_color(border);
        let s = dev.device.create_sampler(&info, None).ok()?;
        self.samplers.push((*d, s));
        Some(s)
    }

    /// Level `level`, layer `layer` of `img` as the guest stores its texels (rows tightly packed). Depth formats the
    /// device widened and multisample images are not read.
    pub unsafe fn read(dev: &Dev, img: &Image, nvn_format: i32, level: u32, layer: u32) -> Option<Texels> {
        let (region, w, h, len) = Self::read_region(img, nvn_format, level, layer)?;
        let buf = dev.host_buffer(len, vk::BufferUsageFlags::TRANSFER_DST).ok()?;
        let r = dev.one_shot(|cb| {
            dev.full_barrier(cb);
            dev.device.cmd_copy_image_to_buffer(cb, img.image, vk::ImageLayout::GENERAL, buf.buffer, &[region]);
        });
        let data = r.ok().map(|_| std::slice::from_raw_parts(buf.ptr, len as usize).to_vec());
        dev.free_host_buffer(&buf);
        Some(Texels { width: w, height: h, format: nvn_format, data: data? })
    }

    /// The copy of `level` / `layer` of `img` into a buffer of texels as `nvn_format` lays them out: (region, width,
    /// height, bytes).
    pub fn read_region(img: &Image, nvn_format: i32, level: u32, layer: u32) -> Option<(vk::BufferImageCopy, u32, u32, u64)> {
        let info = layout::format_info(nvn_format)?;
        if level >= img.levels || img.samples != vk::SampleCountFlags::TYPE_1 {
            return None;
        }
        let is3d = img.extent.depth > 1;
        if (!is3d && layer >= img.layers) || (is3d && layer >= (img.extent.depth >> level).max(1)) {
            return None;
        }
        let aspect = match img.format {
            vk::Format::D16_UNORM | vk::Format::D32_SFLOAT if format::texture(nvn_format) == Some(img.format) => {
                vk::ImageAspectFlags::DEPTH
            }
            vk::Format::S8_UINT => vk::ImageAspectFlags::STENCIL,
            f if format::is_depth(f) => return None,
            _ => vk::ImageAspectFlags::COLOR,
        };
        let w = (img.extent.width >> level).max(1);
        let h = (img.extent.height >> level).max(1);
        let len = w.div_ceil(info.block_w) as u64 * h.div_ceil(info.block_h) as u64 * info.bytes as u64;
        let region = vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(aspect)
                    .mip_level(level)
                    .base_array_layer(if is3d { 0 } else { layer })
                    .layer_count(1),
            )
            .image_offset(vk::Offset3D { x: 0, y: 0, z: if is3d { layer as i32 } else { 0 } })
            .image_extent(vk::Extent3D { width: w, height: h, depth: 1 });
        Some((region, w, h, len))
    }

    pub unsafe fn destroy(&mut self, dev: &Dev) {
        for (_, img) in self.images.drain() {
            for (_, v) in img.views {
                dev.device.destroy_image_view(v, None);
            }
            dev.device.destroy_image(img.image, None);
            dev.device.free_memory(img.memory, None);
        }
        if let Some(a) = self.astc.take() {
            a.destroy(dev);
        }
        if let Some(r) = self.relayout.take() {
            r.destroy(dev);
        }
        for (b, m) in self.relayout_scratch.take().map(|(b, m, _)| (b, m)).into_iter().chain(self.relayout_outgrown.drain(..)) {
            dev.device.destroy_buffer(b, None);
            dev.device.free_memory(m, None);
        }
    }
}

/// The whole tracking pages inside [start, start + len): (address, bytes).
fn interior_pages(start: usize, len: usize) -> (usize, usize) {
    const PAGE: usize = 0x4000;
    let at = start.next_multiple_of(PAGE);
    let end = (start + len) / PAGE * PAGE;
    (at, end.saturating_sub(at))
}

/// What is hashed of an image's bytes for detecting CPU writes. With write tracking, only the bytes on pages it shares
/// with other data: the rest is known from the pages' generations. Without, all of them.
unsafe fn baseline_hash(start: usize, len: usize, tracked: bool) -> u64 {
    let (at, n) = if tracked { interior_pages(start, len) } else { (start, 0) };
    if n == 0 {
        return hash_bytes(std::slice::from_raw_parts(start as *const u8, len));
    }
    let head = hash_bytes(std::slice::from_raw_parts(start as *const u8, at - start));
    let tail_at = at + n;
    let tail = hash_bytes(std::slice::from_raw_parts(tail_at as *const u8, start + len - tail_at));
    head.rotate_left(1) ^ tail
}

/// A content hash of guest bytes (change detection): four independent lanes over 32-byte steps.
pub(super) fn hash_bytes(b: &[u8]) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut l = [0xcbf2_9ce4_8422_2325u64 ^ b.len() as u64, 1, 2, 3];
    let (body, rest) = b.split_at(b.len() / 32 * 32);
    for c in body.chunks_exact(32) {
        for (k, lane) in l.iter_mut().enumerate() {
            let w = u64::from_le_bytes(c[k * 8..k * 8 + 8].try_into().unwrap());
            *lane = (*lane ^ w).wrapping_mul(K).rotate_left(31);
        }
    }
    let mut h = l[0] ^ l[1].rotate_left(17) ^ l[2].rotate_left(34) ^ l[3].rotate_left(51);
    for &x in rest {
        h = (h ^ x as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod index_tests {
    use super::*;

    /// for_each_over finds exactly the images a scan of all of them finds, each once.
    #[test]
    fn chunk_index_finds_every_overlap_once() {
        let mut t = Textures::default();
        let mut all = Vec::new();
        let mut x = 0x1234_5678u64;
        let mut rnd = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        for _ in 0..400 {
            let w = 1 + rnd(2048) as i32;
            let h = 1 + rnd(512) as i32;
            let p = TextureParams { target: layout::NVN_TARGET_2D, width: w, height: h, depth: 1, levels: 1, format: 37, flags: 0, samples: 1, stride: 0 };
            let d = Desc { storage: 0x1000_0000 + (rnd(1 << 26) as usize & !0xff), params: p };
            t.index_image(&d);
            all.push((d, d.storage + layout::compute_layout(&p).total as usize));
        }
        for _ in 0..400 {
            let start = 0x1000_0000 + rnd(1 << 26) as usize;
            let end = start + 1 + rnd(1 << 22) as usize;
            let mut got = t.images_over(start, end);
            let mut want: Vec<Desc> = all.iter().filter(|(d, e)| *e > start && d.storage < end).map(|(d, _)| *d).collect();
            let key = |d: &Desc| (d.storage, d.params.width, d.params.height);
            got.sort_by_key(key);
            want.sort_by_key(key);
            assert_eq!(got, want, "range {start:#x}..{end:#x}");
        }
    }
}
