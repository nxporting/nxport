// Textures: one Vulkan image per guest texture description (storage address and layout), filled from guest memory when
// it is made, kept in the GENERAL layout. Views per binding, samplers per pool sampler, readback in the guest's texel
// encoding.

use std::collections::HashMap;

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
    pub extent: vk::Extent3D,
    pub levels: u32,
    pub layers: u32,
    pub samples: vk::SampleCountFlags,
    /// GPU work rendered or wrote into it: guest memory no longer has its contents.
    pub gpu_written: bool,
    views: HashMap<ViewKey, vk::ImageView>,
    /// The submit it was last checked against guest memory in, the hash of the guest bytes uploaded then (for a
    /// GPU-written image: of the guest bytes when the GPU work completed), and the write-tracking token of those bytes
    /// (guest/dirty_track.rs; None: untracked, hash at every check).
    checked: u64,
    hash: u64,
    token: Option<u64>,
    /// A GPU-written image's hash and token were taken (`settle`).
    settled: bool,
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
    pub images: HashMap<Desc, Image>,
    samplers: Vec<(SamplerDesc, vk::Sampler)>,
    pub warned: std::collections::HashSet<String>,
    /// Counts submits: textures are checked for CPU writes once per submit.
    pub serial: u64,
    /// Alias roots found in this frame (see `locate`; textures are re-initialized between frames).
    roots: HashMap<Desc, (Desc, u32, u32)>,
    roots_frame: u64,
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

impl Textures {
    /// Where texture `t` lives: the description of the image holding it and the level / layer it starts at there.
    /// Several texture objects over the same memory (a mip level or layer of a larger texture, a render target over a
    /// mip chain, another format of the same texels) share the image of the largest live texture containing them, so
    /// rendering through one is visible through the others, as on the GPU (the Metal backend's alias views).
    pub unsafe fn locate(&mut self, t: &NvnTexture) -> (Desc, u32, u32) {
        let d = Desc::of(t);
        let frame = super::VULKAN.frames.load(std::sync::atomic::Ordering::Relaxed);
        if self.roots_frame != frame {
            self.roots.clear();
            self.roots_frame = frame;
        }
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
        for (_, _, c) in crate::nvn::api::textures_near_storage(d.storage, crate::nvn::api::max_texture_span()) {
            // Most candidates end below this texture: their declared size says so without a layout.
            if c.size > 0 && c.storage + c.size <= d.storage {
                continue;
            }
            let cd = Desc::of(&c);
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
            self.refresh(dev, &d, true);
        } else {
            self.refresh(dev, &d, false);
        }
        self.images.get_mut(&d)
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
        if d.storage <= 0x10000 || !crate::nvn::api::memory_pool_cpu_writable(d.storage) {
            if first {
                self.upload(dev, d);
            }
            return;
        }
        let len = layout::compute_layout(&d.params).total as usize;
        if len == 0 {
            return;
        }
        use crate::guest::dirty_track;
        if !first && img.token.is_some() && dirty_track::generation(d.storage, len) == img.token {
            return;
        }
        // Protect before hashing: writes during the hash change the token.
        img.token = if dirty_track::trackable(d.storage, len) { dirty_track::protect(d.storage, len) } else { None };
        let h = hash_bytes(std::slice::from_raw_parts(d.storage as *const u8, len));
        if !first && h == img.hash {
            return;
        }
        img.hash = h;
        img.gpu_written = false;
        img.settled = false;
        self.upload(dev, d);
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
        let len = layout::compute_layout(&d.params).total as usize;
        use crate::guest::dirty_track;
        img.token = if dirty_track::trackable(d.storage, len) { dirty_track::protect(d.storage, len) } else { None };
        img.hash = hash_bytes(std::slice::from_raw_parts(d.storage as *const u8, len));
    }

    pub unsafe fn create(&mut self, dev: &Dev, d: &Desc) -> Option<Image> {
        let p = &d.params;
        let Some(fmt) = device_format(dev, p.format) else {
            if self.warned.insert(format!("fmt{}", p.format)) {
                println!("[vulkan] texture format {} not supported", p.format);
            }
            return None;
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
        if matches!(p.target, layout::NVN_TARGET_CUBEMAP | layout::NVN_TARGET_CUBEMAP_ARRAY) && layers % 6 == 0 {
            flags |= vk::ImageCreateFlags::CUBE_COMPATIBLE;
        }
        // 3D textures the GPU renders into: render passes attach one depth slice through a 2D view.
        if ity == vk::ImageType::TYPE_3D && usage.contains(vk::ImageUsageFlags::COLOR_ATTACHMENT) {
            flags |= vk::ImageCreateFlags::TYPE_2D_ARRAY_COMPATIBLE;
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
        let _ = dev.one_shot(|cb| {
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
            } else {
                let v = vk::ClearColorValue { uint32: [0; 4] };
                dev.device.cmd_clear_color_image(cb, image, vk::ImageLayout::GENERAL, &v, &[range]);
            }
            dev.full_barrier(cb);
        });
        Some(Image {
            image,
            memory,
            format: fmt,
            extent,
            levels,
            layers,
            samples: samples(p.samples),
            gpu_written: false,
            views: HashMap::new(),
            checked: 0,
            hash: 0,
            token: None,
            settled: false,
        })
    }

    /// Fills every level / layer from guest memory (block-linear or pitch-linear), as the Metal backend does.
    unsafe fn upload(&mut self, dev: &Dev, d: &Desc) {
        let Some(img) = self.images.get(d) else { return };
        let p = &d.params;
        if d.storage <= 0x10000 || img.samples != vk::SampleCountFlags::TYPE_1 || format::is_depth(img.format) {
            return;
        }
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
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
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
        let Ok(staging) = dev.host_buffer(data.len() as u64, vk::BufferUsageFlags::TRANSFER_SRC) else { return };
        std::ptr::copy_nonoverlapping(data.as_ptr(), staging.ptr, data.len());
        let image = img.image;
        let _ = dev.one_shot(|cb| {
            dev.device.cmd_copy_buffer_to_image(cb, staging.buffer, image, vk::ImageLayout::GENERAL, &regions);
        });
        dev.free_host_buffer(&staging);
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
        let v = dev
            .device
            .create_image_view(
                &vk::ImageViewCreateInfo::default()
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
                    ),
                None,
            )
            .ok()?;
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
            _ => layout::compute_layout(&Desc::of(t).params).layers.max(1),
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
            .min_lod(d.lod_min)
            // No mip filter: the base level only (GL's non-mipmapped minification).
            .max_lod(if mip.is_none() { d.lod_min.max(0.0) + 0.25 } else { d.lod_max })
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
        let buf = dev.host_buffer(len, vk::BufferUsageFlags::TRANSFER_DST).ok()?;
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
        let r = dev.one_shot(|cb| {
            dev.full_barrier(cb);
            dev.device.cmd_copy_image_to_buffer(cb, img.image, vk::ImageLayout::GENERAL, buf.buffer, &[region]);
        });
        let data = r.ok().map(|_| std::slice::from_raw_parts(buf.ptr, len as usize).to_vec());
        dev.free_host_buffer(&buf);
        Some(Texels { width: w, height: h, format: nvn_format, data: data? })
    }

    pub unsafe fn destroy(&mut self, dev: &Dev) {
        for (_, img) in self.images.drain() {
            for (_, v) in img.views {
                dev.device.destroy_image_view(v, None);
            }
            dev.device.destroy_image(img.image, None);
            dev.device.free_memory(img.memory, None);
        }
    }
}

fn hash_bytes(b: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ b.len() as u64;
    let (words, rest) = b.split_at(b.len() / 8 * 8);
    for w in words.chunks_exact(8) {
        h = (h ^ u64::from_le_bytes(w.try_into().unwrap())).wrapping_mul(0x0100_0000_01b3).rotate_left(29);
    }
    for &x in rest {
        h = (h ^ x as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h
}
