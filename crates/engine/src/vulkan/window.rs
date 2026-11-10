// Window output: each presented texture is blitted into an image of a swapchain on the display's CAMetalLayer
// (host/display.rs) or HWND (host/display_sdl.rs), cropped to what the game presents and flipped for NVN's lower-left
// window origin.
//
// On macOS the drawable takes the game's shape and the layer fits it into the window. A Win32 surface is the window's
// client area itself, so there the picture is fitted into the swapchain image here, with black bars around it: at the
// shape the game renders at (profile::rendered_aspect: a game drawing a 16:9 image of another shape, which follows the
// window's while the dynamic aspect ratio is on, so it fills the window), else at the shape it presents.

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
    /// The low-latency setting when it was made (fewer images).
    low_latency: bool,
    slots: Vec<Slot>,
    next: usize,
    /// Signaled when an image's blit is done, waited by its present: one per swapchain image (a semaphore per slot
    /// could be signaled again while the presentation engine still waited on it, which the slot's fence does not cover).
    done: Vec<vk::Semaphore>,
    /// The presented content size the drawable was last fitted to.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    content: (u32, u32),
    /// Swapchains a resize replaced, with the presents left before their last frames are certainly done (each present
    /// first waits for the slot it reuses): destroyed then, so a resize never waits for the frames in flight.
    retired: Vec<(vk::SwapchainKHR, usize)>,
    /// The window shape last reported to the game (profile::screen_aspect_changed); 0 while the dynamic aspect ratio is
    /// off, so turning it on reports the shape again.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    reported: f64,
}

struct Slot {
    cb: vk::CommandBuffer,
    fence: vk::Fence,
    acquired: vk::Semaphore,
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
                fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
            }
            let info = vk::Win32SurfaceCreateInfoKHR::default().hinstance(GetModuleHandleW(std::ptr::null()) as isize).hwnd(handle as isize);
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
            slots.push(Slot { cb, fence, acquired });
        }
        let mut w = Window {
            handle,
            surface,
            swapchain: vk::SwapchainKHR::null(),
            images: Vec::new(),
            extent: vk::Extent2D::default(),
            vsync: true,
            low_latency: false,
            slots,
            next: 0,
            done: Vec::new(),
            content: (0, 0),
            reported: 0.0,
            retired: Vec::new(),
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
        // Low latency: as few images as the surface allows (at least 2), so with FIFO at most one finished frame waits
        // for the screen, against up to two more with the usual count.
        let low_latency = crate::host::overlay::is_low_latency();
        let wanted = if low_latency { caps.min_image_count.max(2) } else { (caps.min_image_count + 1).max(SLOTS as u32) };
        let count = wanted.min(if caps.max_image_count > 0 { caps.max_image_count } else { u32::MAX });
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
            // Its images may still be presented: destroyed once every slot has been reused.
            self.retired.push((self.swapchain, SLOTS));
        }
        self.swapchain = new;
        self.images = s.swapchain.get_swapchain_images(new).ok()?;
        // Kept across swapchains (presents of the old one may still wait on theirs).
        while self.done.len() < self.images.len() {
            self.done.push(dev.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).ok()?);
        }
        println!(
            "[vulkan] swapchain {}x{}, {} images, {:?}",
            extent.width,
            extent.height,
            self.images.len(),
            mode
        );
        self.extent = extent;
        self.vsync = vsync;
        self.low_latency = low_latency;
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
        // The window was resized (or the VSync or low-latency setting changed).
        let caps = s.surface.get_physical_device_surface_capabilities(dev.pdev, self.surface);
        let resized = caps.is_ok_and(|c| c.current_extent.width != u32::MAX && c.current_extent != self.extent);
        let changed = self.vsync != crate::host::overlay::is_vsync() || self.low_latency != crate::host::overlay::is_low_latency();
        if (resized || changed) && self.recreate(dev).is_none() {
            return;
        }
        let (fence, sem) = (self.slots[self.next].fence, self.slots[self.next].acquired);
        if dev.device.wait_for_fences(&[fence], true, 1_000_000_000).is_err() {
            return;
        }
        // A swapchain may be destroyed only once its presents are done, which the slots' fences do not cover (they signal
        // when the copy into the image is done): MoltenVK's present completion then ran on the destroyed swapchain's
        // image (a crash entering the game, when the drawable takes the game's size). The device is waited for first:
        // presents of every earlier submit are then done. Rare: only after a resize or a VSync / low-latency change.
        if self.retired.iter().any(|&(_, left)| left == 1) {
            let _ = dev.device.device_wait_idle();
        }
        self.retired.retain_mut(|(chain, left)| {
            *left -= 1;
            if *left == 0 {
                s.swapchain.destroy_swapchain(*chain, None);
            }
            *left > 0
        });
        // The window may change size between the check above and the acquire (while it is dragged): made again once.
        let mut acquired = s.swapchain.acquire_next_image(self.swapchain, 1_000_000_000, sem, vk::Fence::null());
        if acquired == Err(vk::Result::ERROR_OUT_OF_DATE_KHR) {
            if self.recreate(dev).is_none() {
                return;
            }
            acquired = s.swapchain.acquire_next_image(self.swapchain, 1_000_000_000, sem, vk::Fence::null());
        }
        let Ok((index, _)) = acquired else { return };
        #[cfg(windows)]
        let slot_index = self.next;
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
        // The window's shape for a game that renders at it (macOS: host/display.rs follow_window_shape).
        #[cfg(not(target_os = "macos"))]
        if crate::host::live::dynamic_aspect() {
            let a = full_w as f64 / full_h as f64;
            if (a - self.reported).abs() >= 0.002 * a {
                self.reported = a;
                crate::profile::screen_aspect_changed(a);
            }
        } else {
            self.reported = 0.0;
        }
        // The picture's place in the image: all of it on macOS (the drawable has the picture's shape), else the largest
        // rectangle of the picture's shape, centred, with the rest cleared to black.
        #[cfg(target_os = "macos")]
        let (dx, dy, dw, dh) = (0, 0, full_w, full_h);
        #[cfg(not(target_os = "macos"))]
        let (dx, dy, dw, dh) = {
            let shape = crate::profile::rendered_aspect().unwrap_or(cw as f64 / ch as f64);
            let (dw, dh) = if full_w as f64 > full_h as f64 * shape {
                ((full_h as f64 * shape).round() as i32, full_h)
            } else {
                (full_w, (full_w as f64 / shape).round() as i32)
            };
            ((full_w - dw) / 2, (full_h - dh) / 2, dw.clamp(1, full_w), dh.clamp(1, full_h))
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
        #[cfg(windows)]
        let drawn_over = super::overlay::record(dev, slot.cb, dst, self.extent, slot_index, SLOTS);
        #[cfg(not(windows))]
        let drawn_over = false;
        let (old_layout, src_access, src_stage) = if drawn_over {
            (vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL, vk::AccessFlags::COLOR_ATTACHMENT_WRITE, vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        } else {
            (vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::AccessFlags::TRANSFER_WRITE, vk::PipelineStageFlags::TRANSFER)
        };
        let to_present = [vk::ImageMemoryBarrier::default()
            .image(dst)
            .old_layout(old_layout)
            .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .src_access_mask(src_access)
            .subresource_range(range)];
        d.cmd_pipeline_barrier(slot.cb, src_stage, vk::PipelineStageFlags::BOTTOM_OF_PIPE, vk::DependencyFlags::empty(), &[], &[], &to_present);
        let _ = d.end_command_buffer(slot.cb);
        let (cbs, waits, stages, signals) = ([slot.cb], [slot.acquired], [vk::PipelineStageFlags::TRANSFER], [self.done[index as usize]]);
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
