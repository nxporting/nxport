// Window output: each presented texture is blitted into an image of a swapchain on the display's CAMetalLayer
// (host/display.rs), cropped to what the game presents and flipped for NVN's lower-left window origin.

use ash::vk;

use super::device::Dev;

/// Swapchain images in flight at most.
const SLOTS: usize = 3;

pub(super) struct Window {
    layer: usize,
    surface: vk::SurfaceKHR,
    swapchain: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    extent: vk::Extent2D,
    slots: Vec<Slot>,
    next: usize,
    /// The presented content size the drawable was last fitted to.
    content: (u32, u32),
}

struct Slot {
    cb: vk::CommandBuffer,
    fence: vk::Fence,
    acquired: vk::Semaphore,
    done: vk::Semaphore,
}

impl Window {
    /// A window on the CAMetalLayer at `layer`, if the device can present to it.
    pub(super) unsafe fn new(dev: &Dev, layer: usize) -> Option<Window> {
        let Some(s) = dev.surfaces.as_ref() else {
            println!("[vulkan] no surface extensions");
            return None;
        };
        let info = vk::MetalSurfaceCreateInfoEXT::default().layer(layer as *const vk::CAMetalLayer);
        let surface = s.metal.create_metal_surface(&info, None).ok()?;
        if !s.surface.get_physical_device_surface_support(dev.pdev, dev.family, surface).unwrap_or(false) {
            s.surface.destroy_surface(surface, None);
            return None;
        }
        let mut slots = Vec::new();
        for _ in 0..SLOTS {
            let cb = dev
                .device
                .allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(dev.cmd_pool).command_buffer_count(1))
                .ok()?[0];
            let fence = dev.device.create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None).ok()?;
            let acquired = dev.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).ok()?;
            let done = dev.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).ok()?;
            slots.push(Slot { cb, fence, acquired, done });
        }
        let mut w = Window {
            layer,
            surface,
            swapchain: vk::SwapchainKHR::null(),
            images: Vec::new(),
            extent: vk::Extent2D::default(),
            slots,
            next: 0,
            content: (0, 0),
        };
        w.recreate(dev)?;
        println!("[vulkan] presenting to the window ({}x{})", w.extent.width, w.extent.height);
        Some(w)
    }

    /// (Re)makes the swapchain at the drawable's current size.
    unsafe fn recreate(&mut self, dev: &Dev) -> Option<()> {
        let s = dev.surfaces.as_ref()?;
        let caps = s.surface.get_physical_device_surface_capabilities(dev.pdev, self.surface).ok()?;
        let extent = if caps.current_extent.width != u32::MAX {
            caps.current_extent
        } else {
            vk::Extent2D { width: 1920, height: 1080 }
        };
        if extent.width == 0 || extent.height == 0 {
            return None;
        }
        let count = (caps.min_image_count + 1).max(SLOTS as u32).min(if caps.max_image_count > 0 { caps.max_image_count } else { u32::MAX });
        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(self.surface)
            .min_image_count(count)
            .image_format(vk::Format::B8G8R8A8_UNORM)
            .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(vk::PresentModeKHR::FIFO)
            .clipped(true)
            .old_swapchain(self.swapchain);
        let new = s.swapchain.create_swapchain(&info, None).ok()?;
        if self.swapchain != vk::SwapchainKHR::null() {
            // Its images may still be presented: wait for the slots first.
            let fences: Vec<vk::Fence> = self.slots.iter().map(|s| s.fence).collect();
            let _ = dev.device.wait_for_fences(&fences, true, 1_000_000_000);
            s.swapchain.destroy_swapchain(self.swapchain, None);
        }
        self.swapchain = new;
        self.images = s.swapchain.get_swapchain_images(new).ok()?;
        self.extent = extent;
        Some(())
    }

    /// Shows `src` (`level` / `layer` of the game's image in the GENERAL layout), cropped to `crop` (x, y, w, h; 0 =
    /// whole) of its `size` there.
    pub(super) unsafe fn present(&mut self, dev: &Dev, src: vk::Image, level: u32, layer: u32, size: (u32, u32), crop: [i32; 4]) {
        let Some(s) = dev.surfaces.as_ref() else { return };
        let (cx, cy, cw, ch) = if crop[2] > 0 && crop[3] > 0 {
            (crop[0], crop[1], crop[2], crop[3])
        } else {
            (0, 0, size.0 as i32, size.1 as i32)
        };
        // The drawable follows the shape the game presents (mods render other aspect ratios).
        #[cfg(target_os = "macos")]
        if self.content != (cw as u32, ch as u32) {
            self.content = (cw as u32, ch as u32);
            let l = &*(self.layer as *const objc2_quartz_core::CAMetalLayer);
            crate::host::display::content_size_changed(l, cw as f64, ch as f64);
        }
        let caps = s.surface.get_physical_device_surface_capabilities(dev.pdev, self.surface);
        if caps.is_ok_and(|c| c.current_extent.width != u32::MAX && c.current_extent != self.extent) && self.recreate(dev).is_none() {
            return;
        }
        let slot = &self.slots[self.next];
        if dev.device.wait_for_fences(&[slot.fence], true, 1_000_000_000).is_err() {
            return;
        }
        let index = match s.swapchain.acquire_next_image(self.swapchain, 1_000_000_000, slot.acquired, vk::Fence::null()) {
            Ok((i, _)) => i,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                let _ = self.recreate(dev);
                return;
            }
            Err(_) => return,
        };
        let slot = &self.slots[self.next];
        self.next = (self.next + 1) % SLOTS;
        let _ = dev.device.reset_fences(&[slot.fence]);
        let dst = self.images[index as usize];
        let d = &dev.device;
        let _ = d.reset_command_buffer(slot.cb, vk::CommandBufferResetFlags::empty());
        let _ = d.begin_command_buffer(slot.cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT));
        // After the frame's rendering.
        dev.full_barrier(slot.cb);
        let range = vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let to_dst = [vk::ImageMemoryBarrier::default()
            .image(dst)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .subresource_range(range)];
        d.cmd_pipeline_barrier(slot.cb, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &to_dst);
        // NVN's LOWER_LEFT window origin keeps the bottom row first in memory: flipped on the way out.
        let (y0, y1) = if crate::nvn::api::window_origin_mode() == 0 { (cy + ch, cy) } else { (cy, cy + ch) };
        let region = vk::ImageBlit::default()
            .src_subresource(
                vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).mip_level(level).base_array_layer(layer).layer_count(1),
            )
            .src_offsets([vk::Offset3D { x: cx, y: y0, z: 0 }, vk::Offset3D { x: cx + cw, y: y1, z: 1 }])
            .dst_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1))
            .dst_offsets([
                vk::Offset3D { x: 0, y: 0, z: 0 },
                vk::Offset3D { x: self.extent.width as i32, y: self.extent.height as i32, z: 1 },
            ]);
        d.cmd_blit_image(slot.cb, src, vk::ImageLayout::GENERAL, dst, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[region], vk::Filter::LINEAR);
        let to_present = [vk::ImageMemoryBarrier::default()
            .image(dst)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .subresource_range(range)];
        d.cmd_pipeline_barrier(slot.cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::DependencyFlags::empty(), &[], &[], &to_present);
        let _ = d.end_command_buffer(slot.cb);
        let (cbs, waits, stages, signals) = ([slot.cb], [slot.acquired], [vk::PipelineStageFlags::TRANSFER], [slot.done]);
        let submit = vk::SubmitInfo::default().command_buffers(&cbs).wait_semaphores(&waits).wait_dst_stage_mask(&stages).signal_semaphores(&signals);
        if d.queue_submit(dev.queue, &[submit], slot.fence).is_err() {
            return;
        }
        let (chains, indices) = ([self.swapchain], [index]);
        let present = vk::PresentInfoKHR::default().wait_semaphores(&signals).swapchains(&chains).image_indices(&indices);
        if let Err(vk::Result::ERROR_OUT_OF_DATE_KHR) = s.swapchain.queue_present(dev.queue, &present) {
            let _ = self.recreate(dev);
        }
    }
}
