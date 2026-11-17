// One frame's render passes, target by target: when a capture is asked for (gpu::history), the next frame's color
// and depth targets are copied at the end of every pass and written to captures/passes_<frame>/ once the frame is
// done: <pass>_<w>x<h>_<format>_c<slot>.ppm (RGB) and .a.pgm (alpha), <pass>_<w>x<h>_<format>_depth.pgm (16-bit,
// stretched over the depths drawn: neither 0 nor 1), and passes.txt. Shows which pass a glitch first appears in.
// R32_SFLOAT targets (linear depths) are written raw instead: <pass>_..._c<slot>.f32, little-endian floats row by row.
// Integer and multisampled targets are left out (a blit cannot convert them).

use ash::vk;

use super::device::{Dev, HostBuffer};

/// Bytes of copies kept of one frame at most (a frame has a few hundred passes, copied at full size).
const MAX_BYTES: u64 = 3 << 30;

struct Item {
    name: String,
    size: (u32, u32),
    /// The RGBA8 image a colour target is blitted into; a depth target is copied straight to `buf`.
    image: Option<(vk::Image, vk::DeviceMemory)>,
    /// A depth copy's format (2 bytes a texel for D16, else 4); None for colour.
    depth: Option<vk::Format>,
    /// An R32_SFLOAT target copied straight to `buf` (written raw).
    float: bool,
    buf: HostBuffer,
}

pub(super) struct PassDump {
    /// The frame whose passes are copied; recording until its present.
    frame: u64,
    recording: bool,
    passes: u32,
    items: Vec<Item>,
    bytes: u64,
    index: String,
}

impl PassDump {
    pub(super) fn new(frame: u64) -> PassDump {
        println!("[+] Pass dump: copying every pass's color targets of frame {frame}");
        PassDump { frame, recording: true, passes: 0, items: Vec::new(), bytes: 0, index: String::new() }
    }

    pub(super) fn recording(&self) -> bool {
        self.recording
    }

    /// The copies of the targets of a pass that just ended, to record after it: `targets` (image, format, slot) and
    /// `depth` (image, format) in the GENERAL layout, `label` its line in passes.txt.
    pub(super) unsafe fn pass(
        &mut self,
        dev: &Dev,
        extent: (u32, u32),
        targets: &[(vk::Image, vk::Format, usize)],
        depth: Option<(vk::Image, vk::Format)>,
        label: String,
    ) -> Vec<Box<dyn FnOnce(&Dev, vk::CommandBuffer)>> {
        let mut cmds: Vec<Box<dyn FnOnce(&Dev, vk::CommandBuffer)>> = Vec::new();
        let n = self.passes;
        self.passes += 1;
        self.index.push_str(&format!("{n:03} {label}\n"));
        let (w, h) = (extent.0.max(1), extent.1.max(1));
        let bytes = w as u64 * h as u64 * 4;
        for &(src, format, slot) in targets {
            if self.bytes + bytes > MAX_BYTES {
                break;
            }
            let name = format!("{n:03}_{w}x{h}_{format:?}_c{slot}");
            if format == vk::Format::R32_SFLOAT {
                let Ok(buf) = dev.readback_buffer(bytes, vk::BufferUsageFlags::TRANSFER_DST) else { break };
                self.bytes += bytes;
                let dst_buf = buf.buffer;
                self.items.push(Item { name, size: (w, h), image: None, depth: None, float: true, buf });
                cmds.push(Box::new(move |d: &Dev, cb: vk::CommandBuffer| {
                    let layers = vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1);
                    let copy = vk::BufferImageCopy::default().image_subresource(layers).image_extent(vk::Extent3D { width: w, height: h, depth: 1 });
                    d.device.cmd_copy_image_to_buffer(cb, src, vk::ImageLayout::GENERAL, dst_buf, &[copy]);
                }));
                continue;
            }
            let Some((image, memory, buf)) = scratch(dev, (w, h)) else { break };
            self.bytes += bytes;
            self.items.push(Item { name, size: (w, h), image: Some((image, memory)), depth: None, float: false, buf });
            let dst_buf = self.items.last().unwrap().buf.buffer;
            let (sw, sh) = (w as i32, h as i32);
            cmds.push(Box::new(move |d: &Dev, cb: vk::CommandBuffer| {
                let d = &d.device;
                let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
                let to_dst = [vk::ImageMemoryBarrier::default()
                    .image(image)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .subresource_range(range)];
                d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &to_dst);
                let layers = vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1);
                let region = vk::ImageBlit::default()
                    .src_subresource(layers)
                    .src_offsets([vk::Offset3D::default(), vk::Offset3D { x: sw, y: sh, z: 1 }])
                    .dst_subresource(layers)
                    .dst_offsets([vk::Offset3D::default(), vk::Offset3D { x: w as i32, y: h as i32, z: 1 }]);
                d.cmd_blit_image(cb, src, vk::ImageLayout::GENERAL, image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region], vk::Filter::LINEAR);
                let to_src = [vk::ImageMemoryBarrier::default()
                    .image(image)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                    .subresource_range(range)];
                d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &to_src);
                let copy = vk::BufferImageCopy::default().image_subresource(layers).image_extent(vk::Extent3D { width: w, height: h, depth: 1 });
                d.cmd_copy_image_to_buffer(cb, image, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, dst_buf, &[copy]);
            }));
        }
        if let Some((src, format)) = depth.filter(|_| self.bytes + bytes <= MAX_BYTES) {
            let Ok(buf) = dev.readback_buffer(bytes, vk::BufferUsageFlags::TRANSFER_DST) else { return cmds };
            self.bytes += bytes;
            let dst_buf = buf.buffer;
            self.items.push(Item { name: format!("{n:03}_{w}x{h}_{format:?}_depth"), size: (w, h), image: None, depth: Some(format), float: false, buf });
            cmds.push(Box::new(move |d: &Dev, cb: vk::CommandBuffer| {
                let layers = vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::DEPTH).layer_count(1);
                let copy = vk::BufferImageCopy::default().image_subresource(layers).image_extent(vk::Extent3D { width: w, height: h, depth: 1 });
                d.device.cmd_copy_image_to_buffer(cb, src, vk::ImageLayout::GENERAL, dst_buf, &[copy]);
            }));
        }
        cmds
    }

    /// The frame was presented: no more passes are copied.
    pub(super) fn presented(&mut self, frame: u64) {
        if frame >= self.frame {
            self.recording = false;
        }
    }

    /// Once the frame's work is done (the caller waited for the device): copies the targets out, frees them, and writes
    /// them on a thread of their own.
    pub(super) unsafe fn write(self, dev: &Dev) {
        let dir = crate::profile::data_subdir("captures")
            .unwrap_or_else(|| std::env::temp_dir().join("nxport"))
            .join(format!("passes_{}", self.frame));
        let mut out = Vec::new();
        for it in self.items {
            let texel = if it.depth == Some(vk::Format::D16_UNORM) { 2 } else { 4 };
            let bytes = std::slice::from_raw_parts(it.buf.ptr, (it.size.0 * it.size.1 * texel) as usize).to_vec();
            out.push((it.name, it.size, it.depth, it.float, bytes));
            dev.free_host_buffer(&it.buf);
            if let Some((image, memory)) = it.image {
                dev.device.destroy_image(image, None);
                dev.device.free_memory(memory, None);
            }
        }
        let (index, passes) = (self.index, self.passes);
        let _ = std::thread::Builder::new().name("pass-dump-write".into()).spawn(move || {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                return eprintln!("[!] Pass dump: {}: {e}", dir.display());
            }
            for (name, (w, h), depth, float, data) in &out {
                if *float {
                    let _ = std::fs::write(dir.join(format!("{name}.f32")), data);
                    continue;
                }
                if let Some(format) = depth {
                    let _ = std::fs::write(dir.join(format!("{name}.pgm")), depth_pgm(*format, (*w, *h), data));
                    continue;
                }
                let rgba = data;
                let mut rgb = format!("P6\n{w} {h}\n255\n").into_bytes();
                rgb.extend(rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
                let _ = std::fs::write(dir.join(format!("{name}.ppm")), rgb);
                let mut a = format!("P5\n{w} {h}\n255\n").into_bytes();
                a.extend(rgba.chunks_exact(4).map(|p| p[3]));
                let _ = std::fs::write(dir.join(format!("{name}.a.pgm")), a);
            }
            let _ = std::fs::write(dir.join("passes.txt"), index);
            println!("[+] Pass dump: {passes} passes, {} targets written to {}", out.len(), dir.display());
        });
    }
}

/// A depth copy as a 16-bit PGM, its drawn depths (neither 0 nor 1) stretched over the whole range.
fn depth_pgm(format: vk::Format, (w, h): (u32, u32), data: &[u8]) -> Vec<u8> {
    let values: Vec<f32> = match format {
        vk::Format::D16_UNORM => data.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]]) as f32 / 65535.0).collect(),
        vk::Format::D24_UNORM_S8_UINT | vk::Format::X8_D24_UNORM_PACK32 => {
            data.chunks_exact(4).map(|b| (u32::from_le_bytes([b[0], b[1], b[2], b[3]]) & 0xff_ffff) as f32 / 16777215.0).collect()
        }
        _ => data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
    };
    let drawn = values.iter().copied().filter(|v| *v > 0.0 && *v < 1.0);
    let (lo, hi) = drawn.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let scale = if hi > lo { 65535.0 / (hi - lo) } else { 0.0 };
    let mut out = format!("P5\n{w} {h}\n65535\n").into_bytes();
    out.extend(values.iter().flat_map(|&v| {
        let q = if v <= 0.0 { 0.0 } else if v >= 1.0 { 65535.0 } else { ((v - lo) * scale).clamp(0.0, 65535.0) };
        (q as u16).to_be_bytes()
    }));
    out
}

/// An RGBA8 image of `size` to scale a target into, and a buffer to copy it to.
unsafe fn scratch(dev: &Dev, size: (u32, u32)) -> Option<(vk::Image, vk::DeviceMemory, HostBuffer)> {
    let d = &dev.device;
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(vk::Format::R8G8B8A8_UNORM)
        .extent(vk::Extent3D { width: size.0, height: size.1, depth: 1 })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC);
    let image = d.create_image(&info, None).ok()?;
    let memory = dev.alloc(d.get_image_memory_requirements(image), vk::MemoryPropertyFlags::DEVICE_LOCAL).ok()?;
    d.bind_image_memory(image, memory, 0).ok()?;
    let buf = dev.readback_buffer(size.0 as u64 * size.1 as u64 * 4, vk::BufferUsageFlags::TRANSFER_DST).ok()?;
    Some((image, memory, buf))
}
