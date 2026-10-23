// The Vulkan instance and device, memory, and one-shot command buffers.

use std::ffi::CStr;

use ash::vk;

pub struct Dev {
    pub _entry: ash::Entry,
    pub instance: ash::Instance,
    pub pdev: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub family: u32,
    pub mem: vk::PhysicalDeviceMemoryProperties,
    pub limits: vk::PhysicalDeviceLimits,
    pub name: String,
    /// VK_EXT_external_memory_host: guest memory pools are GPU buffers in place.
    pub host_memory: Option<(ash::ext::external_memory_host::Device, u64)>,
    /// VK_EXT_depth_clip_control: NVN's -1..1 depth range without shader changes.
    pub depth_clip_control: bool,
    pub validation: bool,
    pub cmd_pool: vk::CommandPool,
    /// What shader translation may use on this device.
    pub shader_host: sass2spv::accessor::Host,
}

fn has(exts: &[vk::ExtensionProperties], name: &CStr) -> bool {
    exts.iter().any(|e| e.extension_name_as_c_str().ok() == Some(name))
}

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _ty: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) && !data.is_null() {
        let msg = (*data).message_as_c_str().map(|m| m.to_string_lossy().into_owned()).unwrap_or_default();
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 200 {
            println!("[vulkan] validation: {msg}");
        }
    }
    vk::FALSE
}

impl Dev {
    pub fn new(validation: bool) -> Result<Dev, String> {
        unsafe {
            // The system's loader, else where macOS package managers put it.
            let entry = match ash::Entry::load() {
                Ok(e) => e,
                Err(first) => ["/opt/homebrew/lib/libvulkan.1.dylib", "/usr/local/lib/libvulkan.1.dylib"]
                    .iter()
                    .find_map(|p| ash::Entry::load_from(p).ok())
                    .ok_or_else(|| format!("no Vulkan loader: {first}"))?,
            };
            let inst_exts = entry.enumerate_instance_extension_properties(None).map_err(|e| e.to_string())?;
            let mut exts: Vec<*const i8> = Vec::new();
            let mut flags = vk::InstanceCreateFlags::empty();
            if has(&inst_exts, ash::khr::portability_enumeration::NAME) {
                exts.push(ash::khr::portability_enumeration::NAME.as_ptr());
                flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
            }
            let validation = validation && has(&inst_exts, ash::ext::debug_utils::NAME);
            let layer = c"VK_LAYER_KHRONOS_validation";
            let layers_available = entry.enumerate_instance_layer_properties().unwrap_or_default();
            let validation = validation
                && layers_available.iter().any(|l| l.layer_name_as_c_str().ok() == Some(layer));
            let mut layers = Vec::new();
            if validation {
                exts.push(ash::ext::debug_utils::NAME.as_ptr());
                layers.push(layer.as_ptr());
            }
            let app = vk::ApplicationInfo::default().application_name(c"nxport").api_version(vk::API_VERSION_1_3);
            let instance = entry
                .create_instance(
                    &vk::InstanceCreateInfo::default()
                        .application_info(&app)
                        .enabled_extension_names(&exts)
                        .enabled_layer_names(&layers)
                        .flags(flags),
                    None,
                )
                .map_err(|e| format!("vkCreateInstance: {e}"))?;
            if validation {
                let du = ash::ext::debug_utils::Instance::new(&entry, &instance);
                let _ = du.create_debug_utils_messenger(
                    &vk::DebugUtilsMessengerCreateInfoEXT::default()
                        .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING)
                        .message_type(vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION | vk::DebugUtilsMessageTypeFlagsEXT::GENERAL)
                        .pfn_user_callback(Some(debug_callback)),
                    None,
                );
            }
            let pdevs = instance.enumerate_physical_devices().map_err(|e| e.to_string())?;
            // A discrete GPU first, then integrated, then anything.
            let pdev = *pdevs
                .iter()
                .max_by_key(|&&p| match instance.get_physical_device_properties(p).device_type {
                    vk::PhysicalDeviceType::DISCRETE_GPU => 3,
                    vk::PhysicalDeviceType::INTEGRATED_GPU => 2,
                    _ => 1,
                })
                .ok_or("no Vulkan device")?;
            let props = instance.get_physical_device_properties(pdev);
            let name = props.device_name_as_c_str().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let family = instance
                .get_physical_device_queue_family_properties(pdev)
                .iter()
                .position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE))
                .ok_or("no graphics queue")? as u32;
            let dev_exts = instance.enumerate_device_extension_properties(pdev).map_err(|e| e.to_string())?;
            let mut dexts: Vec<*const i8> = Vec::new();
            let portability = c"VK_KHR_portability_subset";
            if has(&dev_exts, portability) {
                dexts.push(portability.as_ptr());
            }
            let host_memory = has(&dev_exts, ash::ext::external_memory_host::NAME);
            if host_memory {
                dexts.push(ash::ext::external_memory_host::NAME.as_ptr());
            }
            let depth_clip_control = has(&dev_exts, ash::ext::depth_clip_control::NAME);
            if depth_clip_control {
                dexts.push(ash::ext::depth_clip_control::NAME.as_ptr());
            }
            let robustness2 = has(&dev_exts, ash::ext::robustness2::NAME);
            if robustness2 {
                dexts.push(ash::ext::robustness2::NAME.as_ptr());
            }
            let custom_border = has(&dev_exts, ash::ext::custom_border_color::NAME);
            if custom_border {
                dexts.push(ash::ext::custom_border_color::NAME.as_ptr());
            }
            let prio = [1.0f32];
            let qinfo = [vk::DeviceQueueCreateInfo::default().queue_family_index(family).queue_priorities(&prio)];
            // Every feature only where the device has it.
            let mut q13 = vk::PhysicalDeviceVulkan13Features::default();
            let mut q12 = vk::PhysicalDeviceVulkan12Features::default();
            let mut q11 = vk::PhysicalDeviceVulkan11Features::default();
            let mut qr2 = vk::PhysicalDeviceRobustness2FeaturesEXT::default();
            let mut qcb = vk::PhysicalDeviceCustomBorderColorFeaturesEXT::default();
            let mut q2 = vk::PhysicalDeviceFeatures2::default().push_next(&mut q13).push_next(&mut q12).push_next(&mut q11);
            if robustness2 {
                q2 = q2.push_next(&mut qr2);
            }
            if custom_border {
                q2 = q2.push_next(&mut qcb);
            }
            instance.get_physical_device_features2(pdev, &mut q2);
            let supported = q2.features;
            if q13.dynamic_rendering == 0 {
                return Err(format!("{name}: no dynamic rendering"));
            }
            let features = vk::PhysicalDeviceFeatures {
                robust_buffer_access: supported.robust_buffer_access,
                independent_blend: supported.independent_blend,
                depth_clamp: supported.depth_clamp,
                depth_bias_clamp: supported.depth_bias_clamp,
                fill_mode_non_solid: supported.fill_mode_non_solid,
                sampler_anisotropy: supported.sampler_anisotropy,
                texture_compression_bc: supported.texture_compression_bc,
                texture_compression_astc_ldr: supported.texture_compression_astc_ldr,
                shader_int16: supported.shader_int16,
                fragment_stores_and_atomics: supported.fragment_stores_and_atomics,
                vertex_pipeline_stores_and_atomics: supported.vertex_pipeline_stores_and_atomics,
                shader_storage_image_write_without_format: supported.shader_storage_image_write_without_format,
                shader_storage_image_read_without_format: supported.shader_storage_image_read_without_format,
                image_cube_array: supported.image_cube_array,
                multi_viewport: supported.multi_viewport,
                dual_src_blend: supported.dual_src_blend,
                logic_op: supported.logic_op,
                shader_clip_distance: supported.shader_clip_distance,
                shader_float64: supported.shader_float64,
                shader_int64: supported.shader_int64,
                geometry_shader: supported.geometry_shader,
                tessellation_shader: supported.tessellation_shader,
                sample_rate_shading: supported.sample_rate_shading,
                shader_image_gather_extended: supported.shader_image_gather_extended,
                ..Default::default()
            };
            let mut f11 = vk::PhysicalDeviceVulkan11Features::default().shader_draw_parameters(q11.shader_draw_parameters != 0);
            // Scaled vertex formats (*_UI2F / *_I2F) are fetched as integers and converted in the vertex stage: not every
            // device has the USCALED formats.
            let shader_host = sass2spv::accessor::Host {
                depth_clip_control: false,
                shader_float64: supported.shader_float64 != 0,
                fragment_shader_interlock: false,
                scaled_vertex_formats: false,
                image_gather_extended: supported.shader_image_gather_extended != 0,
                draw_parameters: q11.shader_draw_parameters != 0,
                stage_set: None,
            };
            let mut f13 = vk::PhysicalDeviceVulkan13Features::default()
                .dynamic_rendering(true)
                .synchronization2(q13.synchronization2 != 0)
                .shader_demote_to_helper_invocation(q13.shader_demote_to_helper_invocation != 0);
            let mut f12 = vk::PhysicalDeviceVulkan12Features::default()
                .timeline_semaphore(q12.timeline_semaphore != 0)
                .sampler_mirror_clamp_to_edge(q12.sampler_mirror_clamp_to_edge != 0)
                .scalar_block_layout(q12.scalar_block_layout != 0)
                .shader_float16(q12.shader_float16 != 0)
                .shader_int8(q12.shader_int8 != 0)
                .storage_buffer8_bit_access(q12.storage_buffer8_bit_access != 0)
                .uniform_and_storage_buffer8_bit_access(q12.uniform_and_storage_buffer8_bit_access != 0);
            let mut r2 = vk::PhysicalDeviceRobustness2FeaturesEXT::default()
                .robust_buffer_access2(qr2.robust_buffer_access2 != 0)
                .null_descriptor(qr2.null_descriptor != 0);
            let mut dcc = vk::PhysicalDeviceDepthClipControlFeaturesEXT::default().depth_clip_control(true);
            let mut cbc = vk::PhysicalDeviceCustomBorderColorFeaturesEXT::default()
                .custom_border_colors(qcb.custom_border_colors != 0)
                .custom_border_color_without_format(qcb.custom_border_color_without_format != 0);
            let mut info = vk::DeviceCreateInfo::default()
                .queue_create_infos(&qinfo)
                .enabled_extension_names(&dexts)
                .enabled_features(&features)
                .push_next(&mut f13)
                .push_next(&mut f12)
                .push_next(&mut f11);
            if robustness2 {
                info = info.push_next(&mut r2);
            }
            if depth_clip_control {
                info = info.push_next(&mut dcc);
            }
            if custom_border {
                info = info.push_next(&mut cbc);
            }
            let device = instance.create_device(pdev, &info, None).map_err(|e| format!("vkCreateDevice: {e}"))?;
            let queue = device.get_device_queue(family, 0);
            let mem = instance.get_physical_device_memory_properties(pdev);
            let host_memory = host_memory.then(|| {
                let mut hp = vk::PhysicalDeviceExternalMemoryHostPropertiesEXT::default();
                let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut hp);
                instance.get_physical_device_properties2(pdev, &mut p2);
                let align = hp.min_imported_host_pointer_alignment;
                (ash::ext::external_memory_host::Device::new(&instance, &device), align)
            });
            let cmd_pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(|e| e.to_string())?;
            println!(
                "[vulkan] {} (Vulkan {}.{}), host memory import {}, depth clip control {}, robustness2 {}, validation {}",
                name,
                vk::api_version_major(props.api_version),
                vk::api_version_minor(props.api_version),
                host_memory.as_ref().map_or("no".to_string(), |h| format!("yes (alignment {:#x})", h.1)),
                depth_clip_control,
                robustness2,
                validation
            );
            Ok(Dev {
                _entry: entry,
                instance,
                pdev,
                device,
                queue,
                family,
                mem,
                limits: props.limits,
                name,
                host_memory,
                depth_clip_control,
                validation,
                shader_host: sass2spv::accessor::Host { depth_clip_control, ..shader_host },
                cmd_pool,
            })
        }
    }

    pub fn memory_type(&self, bits: u32, want: vk::MemoryPropertyFlags) -> Option<u32> {
        (0..self.mem.memory_type_count).find(|&i| {
            bits & (1 << i) != 0 && self.mem.memory_types[i as usize].property_flags.contains(want)
        })
    }

    pub unsafe fn alloc(&self, req: vk::MemoryRequirements, want: vk::MemoryPropertyFlags) -> Result<vk::DeviceMemory, String> {
        let ty = self
            .memory_type(req.memory_type_bits, want)
            .or_else(|| self.memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::empty()))
            .ok_or("no memory type")?;
        self.device
            .allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty), None)
            .map_err(|e| format!("vkAllocateMemory ({} bytes): {e}", req.size))
    }

    /// A host-visible, coherent buffer (staging, readback, uniform arenas), mapped.
    pub unsafe fn host_buffer(&self, size: u64, usage: vk::BufferUsageFlags) -> Result<HostBuffer, String> {
        let buffer = self
            .device
            .create_buffer(&vk::BufferCreateInfo::default().size(size.max(16)).usage(usage), None)
            .map_err(|e| e.to_string())?;
        let req = self.device.get_buffer_memory_requirements(buffer);
        let memory = self.alloc(req, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)?;
        self.device.bind_buffer_memory(buffer, memory, 0).map_err(|e| e.to_string())?;
        let ptr = self
            .device
            .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
            .map_err(|e| e.to_string())? as *mut u8;
        Ok(HostBuffer { buffer, memory, ptr, size })
    }

    pub unsafe fn free_host_buffer(&self, b: &HostBuffer) {
        self.device.destroy_buffer(b.buffer, None);
        self.device.free_memory(b.memory, None);
    }

    /// Guest memory [addr, addr + size) as a buffer over the same pages; None if the host cannot import it.
    pub unsafe fn import(&self, addr: usize, size: usize) -> Option<(vk::Buffer, vk::DeviceMemory)> {
        let (ext, align) = self.host_memory.as_ref()?;
        let align = *align as usize;
        if addr % align != 0 || size % align != 0 || size == 0 {
            return None;
        }
        let ht = vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT;
        let mut props = vk::MemoryHostPointerPropertiesEXT::default();
        let r = (ext.fp().get_memory_host_pointer_properties_ext)(ext.device(), ht, addr as *const _, &mut props);
        if r != vk::Result::SUCCESS {
            return None;
        }
        let mut ext_info = vk::ExternalMemoryBufferCreateInfo::default().handle_types(ht);
        let usage = vk::BufferUsageFlags::VERTEX_BUFFER
            | vk::BufferUsageFlags::INDEX_BUFFER
            | vk::BufferUsageFlags::UNIFORM_BUFFER
            | vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::INDIRECT_BUFFER;
        let buffer = self
            .device
            .create_buffer(&vk::BufferCreateInfo::default().size(size as u64).usage(usage).push_next(&mut ext_info), None)
            .ok()?;
        let req = self.device.get_buffer_memory_requirements(buffer);
        let bits = req.memory_type_bits & props.memory_type_bits;
        let Some(ty) = self.memory_type(bits, vk::MemoryPropertyFlags::HOST_VISIBLE) else {
            self.device.destroy_buffer(buffer, None);
            return None;
        };
        let mut import = vk::ImportMemoryHostPointerInfoEXT::default().handle_type(ht).host_pointer(addr as *mut _);
        let memory = match self.device.allocate_memory(
            &vk::MemoryAllocateInfo::default().allocation_size(size as u64).memory_type_index(ty).push_next(&mut import),
            None,
        ) {
            Ok(m) => m,
            Err(_) => {
                self.device.destroy_buffer(buffer, None);
                return None;
            }
        };
        if self.device.bind_buffer_memory(buffer, memory, 0).is_err() {
            self.device.destroy_buffer(buffer, None);
            self.device.free_memory(memory, None);
            return None;
        }
        Some((buffer, memory))
    }

    /// Records with `f` into a new command buffer, submits it and waits.
    pub unsafe fn one_shot(&self, f: impl FnOnce(vk::CommandBuffer)) -> Result<(), String> {
        let cb = self
            .device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default().command_pool(self.cmd_pool).command_buffer_count(1),
            )
            .map_err(|e| e.to_string())?[0];
        self.device
            .begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))
            .map_err(|e| e.to_string())?;
        f(cb);
        self.device.end_command_buffer(cb).map_err(|e| e.to_string())?;
        let r = self.submit_wait(cb);
        self.device.free_command_buffers(self.cmd_pool, &[cb]);
        r
    }

    pub unsafe fn submit_wait(&self, cb: vk::CommandBuffer) -> Result<(), String> {
        let fence = self.device.create_fence(&vk::FenceCreateInfo::default(), None).map_err(|e| e.to_string())?;
        let cbs = [cb];
        let r = self
            .device
            .queue_submit(self.queue, &[vk::SubmitInfo::default().command_buffers(&cbs)], fence)
            .map_err(|e| format!("vkQueueSubmit: {e}"))
            .and_then(|_| self.device.wait_for_fences(&[fence], true, u64::MAX).map_err(|e| format!("GPU: {e}")));
        self.device.destroy_fence(fence, None);
        r
    }

    /// A barrier between everything before and everything after (all stages, all memory).
    pub unsafe fn full_barrier(&self, cb: vk::CommandBuffer) {
        let mb = [vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE | vk::AccessFlags::MEMORY_READ)
            .dst_access_mask(vk::AccessFlags::MEMORY_WRITE | vk::AccessFlags::MEMORY_READ)];
        self.device.cmd_pipeline_barrier(
            cb,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::DependencyFlags::empty(),
            &mb,
            &[],
            &[],
        );
    }
}

pub struct HostBuffer {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub ptr: *mut u8,
    pub size: u64,
}

unsafe impl Send for HostBuffer {}
