// Frame history for this backend (gpu::history): each presented image is blitted at half resolution (linear filter,
// the bytes a window shows) into a scratch image and copied into the next of N host buffers, in its own submit after
// the frame's work; a capture waits for those copies and writes them out.

use ash::vk;

use super::device::{Dev, HostBuffer};

/// Command buffers in flight for history copies.
const CBS: usize = 4;

struct Slot {
    buf: Option<HostBuffer>,
    frame: u64,
    size: (u32, u32),
}

pub(super) struct History {
    slots: Vec<Slot>,
    next: usize,
    cbs: Vec<(vk::CommandBuffer, vk::Fence)>,
    next_cb: usize,
    scratch: Option<(vk::Image, vk::DeviceMemory, (u32, u32))>,
}

impl History {
    pub(super) unsafe fn new(dev: &Dev, n: usize) -> Option<History> {
        let mut cbs = Vec::new();
        for _ in 0..CBS {
            let cb = dev
                .device
                .allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(dev.cmd_pool).command_buffer_count(1))
                .ok()?[0];
            let fence = dev.device.create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None).ok()?;
            cbs.push((cb, fence));
        }
        Some(History { slots: (0..n).map(|_| Slot { buf: None, frame: 0, size: (0, 0) }).collect(), next: 0, cbs, next_cb: 0, scratch: None })
    }

    unsafe fn scratch(&mut self, dev: &Dev, size: (u32, u32)) -> Option<vk::Image> {
        if let Some((img, mem, s)) = self.scratch {
            if s == size {
                return Some(img);
            }
            self.wait_all(dev);
            dev.device.destroy_image(img, None);
            dev.device.free_memory(mem, None);
            self.scratch = None;
        }
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_UNORM)
            .extent(vk::Extent3D { width: size.0, height: size.1, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC);
        let img = dev.device.create_image(&info, None).ok()?;
        let mem = dev.alloc(dev.device.get_image_memory_requirements(img), vk::MemoryPropertyFlags::DEVICE_LOCAL).ok()?;
        dev.device.bind_image_memory(img, mem, 0).ok()?;
        self.scratch = Some((img, mem, size));
        Some(img)
    }

    unsafe fn wait_all(&self, dev: &Dev) {
        let fences: Vec<vk::Fence> = self.cbs.iter().map(|c| c.1).collect();
        let _ = dev.device.wait_for_fences(&fences, true, 5_000_000_000);
    }

    /// Keeps presented frame `frame`: `src` (GENERAL layout) at `level` / `layer`, region `crop` (x, y, w, h).
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn keep(&mut self, dev: &Dev, frame: u64, src: vk::Image, level: u32, layer: u32, crop: (i32, i32, i32, i32), lower_left: bool) {
        let (cx, cy, cw, ch) = crop;
        if cw <= 0 || ch <= 0 {
            return;
        }
        let size = (((cw / 2).max(1)) as u32, ((ch / 2).max(1)) as u32);
        let Some(scratch) = self.scratch(dev, size) else { return };
        let bytes = size.0 as u64 * size.1 as u64 * 4;
        if self.slots[self.next].buf.as_ref().is_some_and(|b| b.size < bytes) {
            self.wait_all(dev);
            let slot = &mut self.slots[self.next];
            dev.free_host_buffer(slot.buf.as_ref().unwrap());
            slot.buf = None;
        }
        let slot = &mut self.slots[self.next];
        if slot.buf.is_none() {
            match dev.host_buffer(bytes, vk::BufferUsageFlags::TRANSFER_DST) {
                Ok(b) => slot.buf = Some(b),
                Err(_) => return,
            }
        }
        let (cb, fence) = self.cbs[self.next_cb];
        if dev.device.wait_for_fences(&[fence], true, 1_000_000_000).is_err() {
            return;
        }
        let _ = dev.device.reset_fences(&[fence]);
        let d = &dev.device;
        let _ = d.reset_command_buffer(cb, vk::CommandBufferResetFlags::empty());
        let _ = d.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT));
        dev.full_barrier(cb);
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let to_dst = [vk::ImageMemoryBarrier::default()
            .image(scratch)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .subresource_range(range)];
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &to_dst);
        let (y0, y1) = if lower_left { (cy + ch, cy) } else { (cy, cy + ch) };
        let region = vk::ImageBlit::default()
            .src_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).mip_level(level).base_array_layer(layer).layer_count(1))
            .src_offsets([vk::Offset3D { x: cx, y: y0, z: 0 }, vk::Offset3D { x: cx + cw, y: y1, z: 1 }])
            .dst_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1))
            .dst_offsets([vk::Offset3D { x: 0, y: 0, z: 0 }, vk::Offset3D { x: size.0 as i32, y: size.1 as i32, z: 1 }]);
        d.cmd_blit_image(cb, src, vk::ImageLayout::GENERAL, scratch, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region], vk::Filter::LINEAR);
        let to_src = [vk::ImageMemoryBarrier::default()
            .image(scratch)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
            .subresource_range(range)];
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &to_src);
        let copy = vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1))
            .image_extent(vk::Extent3D { width: size.0, height: size.1, depth: 1 });
        d.cmd_copy_image_to_buffer(cb, scratch, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, slot.buf.as_ref().unwrap().buffer, &[copy]);
        let host = [vk::BufferMemoryBarrier::default()
            .buffer(slot.buf.as_ref().unwrap().buffer)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::HOST_READ)
            .size(vk::WHOLE_SIZE)];
        d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::HOST, vk::DependencyFlags::empty(), &[], &host, &[]);
        let _ = d.end_command_buffer(cb);
        let cbs = [cb];
        if d.queue_submit(dev.queue, &[vk::SubmitInfo::default().command_buffers(&cbs)], fence).is_err() {
            return;
        }
        slot.frame = frame;
        slot.size = size;
        self.next = (self.next + 1) % self.slots.len();
        self.next_cb = (self.next_cb + 1) % CBS;
    }

    /// Writes the kept frames and the journal to the capture directory of frame `pn`.
    pub(super) unsafe fn write(&self, dev: &Dev, pn: u64) {
        self.wait_all(dev);
        let dir = crate::gpu::history::dir(pn);
        let mut frames = Vec::new();
        for s in &self.slots {
            let Some(b) = s.buf.as_ref().filter(|_| s.frame > 0) else { continue };
            let (w, h) = (s.size.0 as usize, s.size.1 as usize);
            let rgba = std::slice::from_raw_parts(b.ptr, w * h * 4);
            let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
            frames.push((s.frame, w, h, rgb));
        }
        crate::gpu::history::write_frames(&dir, &mut frames);
        crate::gpu::history::write_journal(&dir);
    }
}
