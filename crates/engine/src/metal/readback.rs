// Texture contents back from the GPU in the guest's texel encoding (gpu::Backend::read_texture / gpu_only_textures):
// frame captures and their replays compare and restore textures through it.

use super::*;
use crate::gpu::{GpuOnlyTexture, Texels};
use objc2_metal::{MTLBlitCommandEncoder, MTLBlitOption};

/// Waits until everything encoded so far, including work the issuer still queues, has completed on the GPU.
pub(super) fn drain(g: &Gfx) {
    let marker = {
        let st = g.state.lock().unwrap();
        let Some(m) = g.queue.commandBuffer() else {
            return;
        };
        st.issuer.commit(m.clone());
        st.issuer.sync();
        m
    };
    marker.waitUntilCompleted();
    while retire_oldest(g, true) {}
}

/// Level `level`, slice `layer` (a 3D texture's depth slice) of `tex`, as NVN format `format` stores its texels. Depth
/// formats are read where Metal keeps the guest's encoding (D16, D32F); multisample textures and depth formats Metal
/// widens are not.
unsafe fn read_level(g: &Gfx, tex: &Tex, level: usize, layer: usize, format: i32) -> Option<Texels> {
    let info = crate::nvn::layout::format_info(format)?;
    if level >= tex.mipmapLevelCount() {
        return None;
    }
    let tt = tex.textureType();
    if matches!(tt, MTLTextureType::Type2DMultisample | MTLTextureType::Type2DMultisampleArray) {
        return None;
    }
    let pf = tex.pixelFormat();
    let option = match (pf, format) {
        (MTLPixelFormat::Depth16Unorm, 50) | (MTLPixelFormat::Depth32Float, 52) => MTLBlitOption::empty(),
        (MTLPixelFormat::Stencil8, 49) => MTLBlitOption::empty(),
        _ if copy::is_depth_stencil(pf) => return None,
        _ => MTLBlitOption::empty(),
    };
    let (w, h) = ((tex.width() >> level).max(1), (tex.height() >> level).max(1));
    let (slice, z) = match tt {
        MTLTextureType::Type3D => (0, layer),
        _ => (layer, 0),
    };
    let slices = match tt {
        MTLTextureType::TypeCube => 6,
        MTLTextureType::TypeCubeArray => 6 * tex.arrayLength(),
        MTLTextureType::Type3D => (tex.depth() >> level).max(1),
        _ => tex.arrayLength().max(1),
    };
    if layer >= slices {
        return None;
    }
    let (bw, bh) = (
        w.div_ceil(info.block_w as usize),
        h.div_ceil(info.block_h as usize),
    );
    let row = bw * info.bytes as usize;
    let len = row * bh;
    let buf = g
        .device
        .newBufferWithLength_options(len.max(16), MTLResourceOptions::StorageModeShared)?;
    let cb = g.queue.commandBuffer()?;
    let blit = cb.blitCommandEncoder()?;
    blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage_options(
        tex,
        slice,
        level,
        MTLOrigin { x: 0, y: 0, z },
        MTLSize { width: w, height: h, depth: 1 },
        &buf,
        0,
        row,
        len,
        option,
    );
    blit.endEncoding();
    cb.commit();
    cb.waitUntilCompleted();
    let data = std::slice::from_raw_parts(buf.contents().as_ptr() as *const u8, len).to_vec();
    Some(Texels {
        width: w as u32,
        height: h as u32,
        format,
        data,
    })
}

pub(super) fn read_texture(texture: usize, gen: u32, level: u32, layer: u32) -> Option<Texels> {
    let g = GFX.get()?;
    autoreleasepool(|_| unsafe {
        drain(g);
        let addr = crate::nvn::api::resolve_texture_ref(texture, gen);
        let (_, t) = tex_info(addr)?;
        let tex = {
            let mut st = g.state.lock().unwrap();
            g.texture(&mut st.tc, addr)?
        };
        drain(g);
        read_level(g, &tex, level as usize, layer as usize, t.format)
    })
}

pub(super) fn gpu_only_textures(f: &mut dyn FnMut(GpuOnlyTexture)) -> usize {
    let Some(g) = GFX.get() else { return 0 };
    autoreleasepool(|_| unsafe {
        drain(g);
        let list: Vec<(TexDesc, Tex, u64)> = {
            let st = g.state.lock().unwrap();
            st.tc
                .tex_by_desc
                .iter()
                .filter(|(_, (t, _))| st.tc.gpu_written.contains(&storage_id(t)))
                .map(|(d, (t, _))| (*d, t.clone(), st.rt_history.get(&d.storage).copied().unwrap_or(0)))
                .collect()
        };
        let mut skipped = 0;
        for (d, tex, last) in list {
            let params = crate::nvn::layout::TextureParams {
                target: d.target,
                width: d.width,
                height: d.height,
                depth: d.depth,
                levels: d.levels,
                format: d.format,
                flags: d.flags,
                samples: d.samples,
                stride: d.stride,
            };
            let layers = match tex.textureType() {
                MTLTextureType::TypeCube => 6,
                MTLTextureType::TypeCubeArray => 6 * tex.arrayLength(),
                MTLTextureType::Type3D => tex.depth(),
                _ => tex.arrayLength().max(1),
            };
            let levels = (d.levels.max(1) as usize).min(tex.mipmapLevelCount());
            for level in 0..levels {
                let n = if tex.textureType() == MTLTextureType::Type3D {
                    (layers >> level).max(1)
                } else {
                    layers
                };
                for layer in 0..n {
                    match read_level(g, &tex, level, layer, d.format) {
                        Some(texels) => f(GpuOnlyTexture {
                            storage: d.storage,
                            params,
                            level: level as u32,
                            layer: layer as u32,
                            texels,
                            last_written: last,
                        }),
                        None => skipped += 1,
                    }
                }
            }
        }
        skipped
    })
}
