// NVN's copy and clear commands (nvnCommandBufferCopy*, ClearBuffer, ClearTexture) as Vulkan transfers, in stream
// order with everything around them.

use super::*;
use crate::nvn::cmd::{Region, TexRef};

/// Where in an image a copy lands, and the buffer side's layout.
struct Geometry {
    level: u32,
    base_layer: u32,
    layers: u32,
    offset: vk::Offset3D,
    extent: vk::Extent3D,
    /// Buffer row length and image height in texels (0: tightly packed).
    row_texels: u32,
    image_rows: u32,
    bytes_per_image: usize,
    aspect: vk::ImageAspectFlags,
}

/// `at`: the level / layer texture `t` starts at in `img` (an alias of a larger texture).
fn geometry(t: &NvnTexture, img: &texture::Image, at: (u32, u32), r: &TexRef, region: &Region, row_stride: isize, image_stride: isize) -> Option<Geometry> {
    let info = crate::nvn::layout::format_info(t.format)?;
    let level = (r.level as u32 + at.0).min(img.levels - 1);
    let (lw, lh) = ((img.extent.width >> level).max(1), (img.extent.height >> level).max(1));
    // A zero-sized region means the whole level.
    let w = if region.w > 0 { region.w as u32 } else { lw };
    let h = if region.h > 0 { region.h as u32 } else { lh };
    let d = region.d.max(1) as u32;
    let is3d = img.extent.depth > 1;
    let (wb, hb) = (w.div_ceil(info.block_w), h.div_ceil(info.block_h));
    let tight = wb as usize * info.bytes as usize;
    let row = if row_stride > 0 { row_stride as usize } else { tight };
    let bytes_per_image = if image_stride > 0 { image_stride as usize } else { row * hb as usize };
    if row % info.bytes as usize != 0 {
        return None;
    }
    let row_texels = (row / info.bytes as usize) as u32 * info.block_w;
    let image_rows = (bytes_per_image / row) as u32 * info.block_h;
    Some(Geometry {
        level,
        base_layer: if is3d { 0 } else { at.1 + r.layer as u32 + region.z.max(0) as u32 },
        layers: if is3d { 1 } else { d },
        offset: vk::Offset3D { x: region.x.max(0), y: region.y.max(0), z: if is3d { region.z.max(0) } else { 0 } },
        extent: vk::Extent3D { width: w, height: h, depth: if is3d { d } else { 1 } },
        row_texels,
        image_rows,
        bytes_per_image,
        aspect: format::aspects(img.format),
    })
}

impl Ctx {
    /// The image a recorded texture reference names: the texture, its image's description, image and where the
    /// texture starts in it.
    unsafe fn tex_ref(&mut self, r: &TexRef) -> Option<(NvnTexture, texture::Desc, vk::Image, (u32, u32))> {
        let addr = crate::nvn::api::resolve_texture_ref(r.texture, r.gen);
        let (_, t) = crate::nvn::api::texture_snapshot(addr)?;
        let (d, level, layer) = self.tex.locate(&t);
        let img = self.tex.get_desc(&self.dev, &d)?;
        Some((t, d, img.image, (level, layer)))
    }

    /// Guest memory [addr, addr + len) as a buffer range: in place when its pool is imported, else a copy (written back
    /// after the submit when the GPU writes it).
    pub(super) unsafe fn guest_range(&mut self, addr: u64, len: usize, write: bool) -> Option<(vk::Buffer, u64)> {
        let a = crate::nvn::api::translate_virtual(addr as usize);
        if a < 0x10000 || len == 0 {
            return None;
        }
        if write {
            self.gpu_dirty.add(a, a + len);
            crate::guest::dirty_track::release_for_gpu(a, len);
            crate::vulkan::guard::note_write(a, len);
        }
        if let Some(x) = self.imports.find(&self.dev, a, len).filter(|_| write || !crate::vulkan::guard::hot(a, len)) {
            if !write {
                crate::vulkan::guard::note_read(a, len);
            }
            return Some(x);
        }
        let (buf, off, p) = self.arena.alloc(&self.dev, len)?;
        std::ptr::copy_nonoverlapping(a as *const u8, p, len);
        if write {
            self.writebacks.push((a, p, len));
        }
        Some((buf, off))
    }

    pub(super) unsafe fn exec_copy(&mut self, c: &Cmd) {
        self.end_pass();
        match c {
            Cmd::CopyBufferToBuffer { src, dst, size } => {
                let (Some((sb, so)), Some((db, d_off))) = (self.guest_range(*src, *size, false), self.guest_range(*dst, *size, true)) else {
                    return;
                };
                self.dev.device.cmd_copy_buffer(self.cb, sb, db, &[vk::BufferCopy { src_offset: so, dst_offset: d_off, size: *size as u64 }]);
            }
            Cmd::ClearBuffer { dst, size, value } => {
                let Some((db, off)) = self.guest_range(*dst, *size, true) else { return };
                if off % 4 != 0 || size % 4 != 0 {
                    self.warn("ClearBuffer of an unaligned range");
                    return;
                }
                self.dev.device.cmd_fill_buffer(self.cb, db, off, *size as u64, *value);
            }
            Cmd::CopyBufferToTexture { src, dst, region, row_stride, image_stride } => {
                let Some((t, d, image, at)) = self.tex_ref(dst) else { return };
                let img = self.tex.images.get_mut(&d).unwrap();
                img.mark_gpu_write();
                let Some(g) = geometry(&t, img, at, dst, region, *row_stride, *image_stride) else {
                    self.warn("CopyBufferToTexture with a row stride that is not whole texels");
                    return;
                };
                let total = g.bytes_per_image * g.layers.max(g.extent.depth) as usize;
                let Some((sb, so)) = self.guest_range(*src, total, false) else { return };
                let region = vk::BufferImageCopy::default()
                    .buffer_offset(so)
                    .buffer_row_length(g.row_texels)
                    .buffer_image_height(g.image_rows)
                    .image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(g.aspect).mip_level(g.level).base_array_layer(g.base_layer).layer_count(g.layers))
                    .image_offset(g.offset)
                    .image_extent(g.extent);
                self.dev.device.cmd_copy_buffer_to_image(self.cb, sb, image, vk::ImageLayout::GENERAL, &[region]);
            }
            Cmd::CopyTextureToBuffer { src, region, dst, row_stride, image_stride } => {
                let Some((t, d, image, at)) = self.tex_ref(src) else { return };
                let img = &self.tex.images[&d];
                let Some(g) = geometry(&t, img, at, src, region, *row_stride, *image_stride) else { return };
                let total = g.bytes_per_image * g.layers.max(g.extent.depth) as usize;
                let Some((db, d_off)) = self.guest_range(*dst, total, true) else { return };
                let region = vk::BufferImageCopy::default()
                    .buffer_offset(d_off)
                    .buffer_row_length(g.row_texels)
                    .buffer_image_height(g.image_rows)
                    .image_subresource(vk::ImageSubresourceLayers::default().aspect_mask(g.aspect).mip_level(g.level).base_array_layer(g.base_layer).layer_count(g.layers))
                    .image_offset(g.offset)
                    .image_extent(g.extent);
                self.dev.device.cmd_copy_image_to_buffer(self.cb, image, vk::ImageLayout::GENERAL, db, &[region]);
            }
            Cmd::CopyTextureToTexture(p) => {
                let (Some((st_, sd, si, sat)), Some((dt, dd, di, dat))) = (self.tex_ref(&p.src), self.tex_ref(&p.dst)) else { return };
                let simg = &self.tex.images[&sd];
                let (sfmt, saspect) = (simg.format, format::aspects(simg.format));
                let Some(sg) = geometry(&st_, simg, sat, &p.src, &p.src_region, 0, 0) else { return };
                let dimg = self.tex.images.get_mut(&dd).unwrap();
                dimg.mark_gpu_write();
                let dfmt = dimg.format;
                let Some(dg) = geometry(&dt, dimg, dat, &p.dst, &p.dst_region, 0, 0) else { return };
                let sub = |g: &Geometry, aspect| {
                    vk::ImageSubresourceLayers::default().aspect_mask(aspect).mip_level(g.level).base_array_layer(g.base_layer).layer_count(g.layers.min(sg.layers))
                };
                if sg.extent == dg.extent && (sfmt == dfmt || format::aspects(dfmt) == saspect) {
                    let r = vk::ImageCopy::default()
                        .src_subresource(sub(&sg, saspect))
                        .src_offset(sg.offset)
                        .dst_subresource(sub(&dg, format::aspects(dfmt)))
                        .dst_offset(dg.offset)
                        .extent(sg.extent);
                    self.dev.device.cmd_copy_image(self.cb, si, vk::ImageLayout::GENERAL, di, vk::ImageLayout::GENERAL, &[r]);
                } else {
                    // Different sizes: a scaled copy.
                    let corners = |g: &Geometry| {
                        [g.offset, vk::Offset3D { x: g.offset.x + g.extent.width as i32, y: g.offset.y + g.extent.height as i32, z: g.offset.z + g.extent.depth as i32 }]
                    };
                    let r = vk::ImageBlit::default()
                        .src_subresource(sub(&sg, saspect))
                        .src_offsets(corners(&sg))
                        .dst_subresource(sub(&dg, format::aspects(dfmt)))
                        .dst_offsets(corners(&dg));
                    let filter = if format::is_depth(sfmt) { vk::Filter::NEAREST } else { vk::Filter::LINEAR };
                    self.dev.device.cmd_blit_image(self.cb, si, vk::ImageLayout::GENERAL, di, vk::ImageLayout::GENERAL, &[r], filter);
                }
            }
            Cmd::ClearTexture(p) => {
                let Some((t, d, image, at)) = self.tex_ref(&p.dst) else { return };
                let img = self.tex.images.get_mut(&d).unwrap();
                img.mark_gpu_write();
                let (fmt, levels, layers, extent) = (img.format, img.levels, img.layers, img.extent);
                let Some(g) = geometry(&t, img, at, &p.dst, &p.region, 0, 0) else { return };
                let level_extent = ((extent.width >> g.level).max(1), (extent.height >> g.level).max(1));
                let whole = g.offset.x == 0 && g.offset.y == 0 && (g.extent.width, g.extent.height) == level_extent;
                if !whole || p.mask & 0xF != 0xF {
                    self.warn("partial texture clears are cleared whole");
                }
                let range = vk::ImageSubresourceRange::default()
                    .aspect_mask(format::aspects(fmt))
                    .base_mip_level(g.level)
                    .level_count(1.min(levels))
                    .base_array_layer(g.base_layer.min(layers - 1))
                    .layer_count(g.layers.min(layers - g.base_layer.min(layers - 1)));
                if format::is_depth(fmt) {
                    let v = vk::ClearDepthStencilValue { depth: f32::from_bits(p.color[0]), stencil: p.color[1] };
                    self.dev.device.cmd_clear_depth_stencil_image(self.cb, image, vk::ImageLayout::GENERAL, &v, &[range]);
                } else {
                    // The raw words of the float / int / uint value.
                    let v = vk::ClearColorValue { uint32: p.color };
                    self.dev.device.cmd_clear_color_image(self.cb, image, vk::ImageLayout::GENERAL, &v, &[range]);
                }
            }
            _ => {}
        }
        self.dev.full_barrier(self.cb);
    }
}
