// Bindless textures (graphics.vk_bindless_textures; sass2spv::accessor::Host::bindless_textures): graphics stages sample
// through one global descriptor set of sampled-image arrays (one per view type) and a sampler array, indexed by a word
// per texture in the push constants. Views and samplers get an index the first time a draw names them and keep it (the
// backend never destroys either while it runs), so a descriptor in use by submitted work is never rewritten. Draws then
// resolve a texture handle to its word through a cache that lives across submits (draw.rs bindless_word), not to
// descriptors written per draw.

use ash::vk;

use super::device::Dev;

/// The global set, made with the device.
pub struct Global {
    pub set_layout: vk::DescriptorSetLayout,
    pub set: vk::DescriptorSet,
    _pool: vk::DescriptorPool,
    /// Entries of each image array and of the sampler array.
    pub images: u32,
    pub samplers: u32,
}

/// Image arrays: one per VkImageViewType value (1D .. cube array).
pub const VIEW_TYPES: u32 = 7;

/// The global set where the device has what it needs (descriptor indexing with update-after-bind and partially bound
/// arrays, null descriptors), within its limits.
pub unsafe fn create(device: &ash::Device, ip: &vk::PhysicalDeviceDescriptorIndexingProperties) -> Option<Global> {
    // Room for the per-draw set's own textures and samplers beside them.
    let per_stage = ip.max_per_stage_descriptor_update_after_bind_sampled_images.min(ip.max_descriptor_set_update_after_bind_sampled_images);
    let images = (per_stage.saturating_sub(4096) / VIEW_TYPES).min(1 << 16);
    let samplers = ip
        .max_per_stage_descriptor_update_after_bind_samplers
        .min(ip.max_descriptor_set_update_after_bind_samplers)
        .saturating_sub(256)
        .min(4096);
    if images < 4096 || samplers < 1024 {
        return None;
    }
    let flags = vk::DescriptorBindingFlags::PARTIALLY_BOUND | vk::DescriptorBindingFlags::UPDATE_AFTER_BIND | vk::DescriptorBindingFlags::UPDATE_UNUSED_WHILE_PENDING;
    let mut lb: Vec<vk::DescriptorSetLayoutBinding> = (0..VIEW_TYPES)
        .map(|b| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(b)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(images)
                .stage_flags(vk::ShaderStageFlags::ALL_GRAPHICS)
        })
        .collect();
    lb.push(
        vk::DescriptorSetLayoutBinding::default()
            .binding(sass2spv::accessor::BINDLESS_SAMPLERS as u32)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .descriptor_count(samplers)
            .stage_flags(vk::ShaderStageFlags::ALL_GRAPHICS),
    );
    let bf = vec![flags; lb.len()];
    let mut fi = vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&bf);
    let set_layout = device
        .create_descriptor_set_layout(
            &vk::DescriptorSetLayoutCreateInfo::default()
                .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
                .bindings(&lb)
                .push_next(&mut fi),
            None,
        )
        .ok()?;
    let sizes = [
        vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: images * VIEW_TYPES },
        vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: samplers },
    ];
    let pool = device
        .create_descriptor_pool(
            &vk::DescriptorPoolCreateInfo::default().flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND).max_sets(1).pool_sizes(&sizes),
            None,
        )
        .ok()?;
    let layouts = [set_layout];
    let set = device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(pool).set_layouts(&layouts)).ok()?[0];
    Some(Global { set_layout, set, _pool: pool, images, samplers })
}

/// The indices given out (Presentation thread): views by view type, samplers. Index 0 of each image array is a null
/// descriptor (an unresolved texture reads zeros, as through the per-draw path), sampler 0 the default sampler.
pub struct Slots {
    views: rustc_hash::FxHashMap<vk::ImageView, u32>,
    next: [u32; VIEW_TYPES as usize],
    samplers: rustc_hash::FxHashMap<vk::Sampler, u32>,
    next_sampler: u32,
    pub warned: bool,
    /// Entries given out and not yet written: (binding, index, view, sampler), written together before the next queue
    /// submission (flush; update-after-bind lets recorded commands name them first). One write per new view at a scene
    /// change was a hitch of its own.
    pending: Vec<(u32, u32, vk::ImageView, vk::Sampler)>,
}

impl Slots {
    pub unsafe fn new(dev: &Dev, default_sampler: vk::Sampler) -> Slots {
        let mut s = Slots { views: Default::default(), next: [1; VIEW_TYPES as usize], samplers: Default::default(), next_sampler: 0, warned: false, pending: Vec::new() };
        if let Some(g) = &dev.bindless {
            for t in 0..VIEW_TYPES {
                write(dev, g, t, 0, vk::ImageView::null(), vk::Sampler::null());
            }
        }
        s.sampler(dev, default_sampler);
        s
    }

    /// Writes the entries given out since the last flush (before work recorded with them is submitted).
    pub unsafe fn flush(&mut self, dev: &Dev) {
        let Some(g) = &dev.bindless else { return };
        if self.pending.is_empty() {
            return;
        }
        let infos: Vec<[vk::DescriptorImageInfo; 1]> = self
            .pending
            .iter()
            .map(|&(_, _, view, sampler)| [vk::DescriptorImageInfo { sampler, image_view: view, image_layout: vk::ImageLayout::GENERAL }])
            .collect();
        let writes: Vec<vk::WriteDescriptorSet> = self
            .pending
            .iter()
            .zip(&infos)
            .map(|(&(b, i, _, _), info)| {
                let ty = if b < VIEW_TYPES { vk::DescriptorType::SAMPLED_IMAGE } else { vk::DescriptorType::SAMPLER };
                vk::WriteDescriptorSet::default().dst_set(g.set).dst_binding(b).dst_array_element(i).descriptor_type(ty).image_info(info)
            })
            .collect();
        dev.device.update_descriptor_sets(&writes, &[]);
        self.pending.clear();
    }

    /// The packed word for sampling `view` (of type `ty`) with `sampler`: the view's index (low 20 bits), the sampler's
    /// (high 12). Written into the global set the first time.
    pub unsafe fn word(&mut self, dev: &Dev, view: vk::ImageView, ty: vk::ImageViewType, sampler: vk::Sampler) -> u32 {
        self.view(dev, view, ty) | self.sampler(dev, sampler) << 20
    }

    unsafe fn view(&mut self, dev: &Dev, view: vk::ImageView, ty: vk::ImageViewType) -> u32 {
        let Some(g) = &dev.bindless else { return 0 };
        let t = ty.as_raw() as u32;
        if view == vk::ImageView::null() || t >= VIEW_TYPES {
            return 0;
        }
        if let Some(&i) = self.views.get(&view) {
            return i;
        }
        let i = self.next[t as usize];
        if i >= g.images {
            self.warned = true;
            return 0;
        }
        self.next[t as usize] += 1;
        self.pending.push((t, i, view, vk::Sampler::null()));
        self.views.insert(view, i);
        i
    }

    unsafe fn sampler(&mut self, dev: &Dev, sampler: vk::Sampler) -> u32 {
        let Some(g) = &dev.bindless else { return 0 };
        if let Some(&i) = self.samplers.get(&sampler) {
            return i;
        }
        let i = self.next_sampler;
        if i >= g.samplers {
            self.warned = true;
            return 0;
        }
        self.next_sampler += 1;
        self.pending.push((sass2spv::accessor::BINDLESS_SAMPLERS as u32, i, vk::ImageView::null(), sampler));
        self.samplers.insert(sampler, i);
        i
    }
}

/// Writes entry `i` of binding `b`: an image (b < VIEW_TYPES) or the sampler.
unsafe fn write(dev: &Dev, g: &Global, b: u32, i: u32, view: vk::ImageView, sampler: vk::Sampler) {
    let info = [vk::DescriptorImageInfo { sampler, image_view: view, image_layout: vk::ImageLayout::GENERAL }];
    let ty = if b < VIEW_TYPES { vk::DescriptorType::SAMPLED_IMAGE } else { vk::DescriptorType::SAMPLER };
    let w = vk::WriteDescriptorSet::default().dst_set(g.set).dst_binding(b).dst_array_element(i).descriptor_type(ty).image_info(&info);
    dev.device.update_descriptor_sets(&[w], &[]);
}
