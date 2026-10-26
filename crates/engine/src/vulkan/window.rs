// Window output: each presented texture is blitted into an image of a swapchain on the display's CAMetalLayer
// (host/display.rs) or HWND (host/display_sdl.rs), cropped to what the game presents and flipped for NVN's lower-left
// window origin.
//
// On macOS the drawable takes the game's shape and the layer fits it into the window. A Win32 surface is the window's
// client area itself, so there the picture is fitted into the swapchain image here, with black bars around it.

use ash::vk;

use super::device::Dev;

/// Swapchain images in flight at most.
const SLOTS: usize = 3;

pub(super) struct Window {
    /// The CAMetalLayer (Apple) or HWND (Windows) the surface is on.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    handle: usize,
    surface: vk::SurfaceKHR,
    swapchain: vk::SwapchainKHR,
    images: Vec<vk::Image>,
    extent: vk::Extent2D,
    /// Whether the swapchain waits for vertical blank (FIFO): the VSync setting when it was made.
    vsync: bool,
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
    /// A window on `handle` (the CAMetalLayer on Apple hosts, the HWND on Windows), if the device can present to it.
    pub(super) unsafe fn new(dev: &Dev, handle: usize) -> Option<Window> {
        let Some(s) = dev.surfaces.as_ref() else {
            println!("[vulkan] no surface extensions");
            return None;
        };
        #[cfg(target_vendor = "apple")]
        let surface = {
            let info = vk::MetalSurfaceCreateInfoEXT::default().layer(handle as *const vk::CAMetalLayer);
            s.metal.create_metal_surface(&info, None).ok()?
        };
        #[cfg(windows)]
        let surface = {
            extern "system" {
                fn GetModuleHandleW(name: *const u16) -> isize;
            }
            let info = vk::Win32SurfaceCreateInfoKHR::default().hinstance(GetModuleHandleW(std::ptr::null())).hwnd(handle as isize);
            s.win32.create_win32_surface(&info, None).ok()?
        };
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
            handle,
            surface,
            swapchain: vk::SwapchainKHR::null(),
            images: Vec::new(),
            extent: vk::Extent2D::default(),
            vsync: true,
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
        // VSync off: the newest frame replaces a waiting one (MAILBOX), else frames show at once and may tear
        // (IMMEDIATE). FIFO is the only mode every implementation has.
        let vsync = crate::host::overlay::is_vsync();
        let modes = s.surface.get_physical_device_surface_present_modes(dev.pdev, self.surface).unwrap_or_default();
        let mode = [vk::PresentModeKHR::MAILBOX, vk::PresentModeKHR::IMMEDIATE]
            .into_iter()
            .find(|m| !vsync && modes.contains(m))
            .unwrap_or(vk::PresentModeKHR::FIFO);
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
            .present_mode(mode)
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
        self.vsync = vsync;
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
            let l = &*(self.handle as *const objc2_quartz_core::CAMetalLayer);
            crate::host::display::content_size_changed(l, cw as f64, ch as f64);
        }
        // The window was resized (or the VSync setting changed).
        let caps = s.surface.get_physical_device_surface_capabilities(dev.pdev, self.surface);
        let resized = caps.is_ok_and(|c| c.current_extent.width != u32::MAX && c.current_extent != self.extent);
        if (resized || self.vsync != crate::host::overlay::is_vsync()) && self.recreate(dev).is_none() {
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
        let (full_w, full_h) = (self.extent.width as i32, self.extent.height as i32);
        // The picture's place in the image: all of it on macOS (the drawable has the picture's shape), else the largest
        // rectangle of the picture's shape, centred, with the rest cleared to black.
        #[cfg(target_os = "macos")]
        let (dx, dy, dw, dh) = (0, 0, full_w, full_h);
        #[cfg(not(target_os = "macos"))]
        let (dx, dy, dw, dh) = {
            let (dw, dh) = if full_w as i64 * ch as i64 > full_h as i64 * cw as i64 {
                ((full_h as i64 * cw as i64 / ch as i64) as i32, full_h)
            } else {
                (full_w, (full_w as i64 * ch as i64 / cw as i64) as i32)
            };
            ((full_w - dw) / 2, (full_h - dh) / 2, dw.max(1), dh.max(1))
        };
        if (dw, dh) != (full_w, full_h) {
            let black = vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] };
            d.cmd_clear_color_image(slot.cb, dst, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &black, &[range]);
            let cleared = [vk::ImageMemoryBarrier::default()
                .image(dst)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .subresource_range(range)];
            d.cmd_pipeline_barrier(slot.cb, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &cleared);
        }
        // NVN's LOWER_LEFT window origin keeps the bottom row first in memory: flipped on the way out.
        let (y0, y1) = if crate::nvn::api::window_origin_mode() == 0 { (cy + ch, cy) } else { (cy, cy + ch) };
        let region = vk::ImageBlit::default()
            .src_subresource(
                vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).mip_level(level).base_array_layer(layer).layer_count(1),
            )
            .src_offsets([vk::Offset3D { x: cx, y: y0, z: 0 }, vk::Offset3D { x: cx + cw, y: y1, z: 1 }])
            .dst_subresource(vk::ImageSubresourceLayers::default().aspect_mask(vk::ImageAspectFlags::COLOR).layer_count(1))
            .dst_offsets([vk::Offset3D { x: dx, y: dy, z: 0 }, vk::Offset3D { x: dx + dw, y: dy + dh, z: 1 }]);
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
        match s.swapchain.queue_present(dev.queue, &present) {
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                let _ = self.recreate(dev);
            }
            Err(vk::Result::ERROR_DEVICE_LOST) => super::device::report_lost("presenting to the window"),
            _ => {}
        }
    }
}
