// Executes NVN's GPU copy and clear commands (nvnCommandBufferCopy*, Clear*) with Metal blits.
//
// Metal textures are the source of truth once created (guest storage only seeds them, see `texture` in metal/mod.rs), so
// copies run on the Metal side. Blit encoders keep them ordered with the render passes around them in the command
// buffer, which a CPU-side replaceRegion would not. Buffers are the guest's own memory, wrapped zero-copy from the
// memory pools (metal/translated.rs `guest_buffer`).
//
// Not handled (logged once): partial-region texture clears, integer clears, depth/stencil copies, 3D sources whose
// storage we cannot upload, buffer textures.

use super::*;

use objc2::Message as _;
use objc2_foundation::NSRange;
use objc2_metal::MTLBlitCommandEncoder;

use crate::nvn::cmd::{Region, TexRef};

fn warn_once(key: &'static str, msg: String) {
    static SEEN: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut s = SEEN.lock().unwrap();
    if !s.contains(&key) {
        s.push(key);
        println!("[!] Gfx: {}", msg);
    }
}

/// Where in a Metal texture a copy lands, and how big one blit is.
struct Subresource {
    level: usize,
    /// Metal array slice for layer `i` of the region is `slice0 + i` (array / cube targets only).
    slice0: usize,
    origin: MTLOrigin,
    /// Size of one blit (depth 1 except for 3D textures).
    size: MTLSize,
    /// Number of array layers the region covers (1 for non-array targets).
    layers: usize,
    /// Bytes per row / per image in the linear buffer side.
    bytes_per_row: usize,
    bytes_per_image: usize,
}

unsafe fn geometry(
    t: &NvnTexture,
    tex: &Tex,
    r: &TexRef,
    region: &Region,
    row_stride: isize,
    image_stride: isize,
) -> Option<Subresource> {
    let info = crate::nvn::layout::format_info(t.format)?;
    let level = (r.level as usize).min(tex.mipmapLevelCount().saturating_sub(1));
    let (lw, lh) = (
        ((t.width as usize) >> level).max(1),
        ((t.height as usize) >> level).max(1),
    );
    // A zero-sized region means "the whole level" for callers that only fill in offsets.
    let w = if region.w > 0 { region.w as usize } else { lw };
    let h = if region.h > 0 { region.h as usize } else { lh };
    let d = region.d.max(1) as usize;
    let tt = tex.textureType();
    let arrayed = matches!(
        tt,
        MTLTextureType::Type1DArray
            | MTLTextureType::Type2DArray
            | MTLTextureType::TypeCube
            | MTLTextureType::TypeCubeArray
    );
    let is_3d = tt == MTLTextureType::Type3D;
    let (wb, hb) = (
        (w + info.block_w as usize - 1) / info.block_w as usize,
        (h + info.block_h as usize - 1) / info.block_h as usize,
    );
    let tight_row = wb * info.bytes as usize;
    let bytes_per_row = if row_stride > 0 {
        row_stride as usize
    } else {
        tight_row
    };
    let bytes_per_image = if image_stride > 0 {
        image_stride as usize
    } else {
        bytes_per_row * hb
    };
    Some(Subresource {
        level,
        slice0: if arrayed {
            r.layer as usize + region.z.max(0) as usize
        } else {
            0
        },
        origin: MTLOrigin {
            x: region.x.max(0) as usize,
            y: region.y.max(0) as usize,
            z: if is_3d { region.z.max(0) as usize } else { 0 },
        },
        size: MTLSize {
            width: w,
            height: h,
            depth: if is_3d { d } else { 1 },
        },
        layers: if arrayed { d } else { 1 },
        bytes_per_row,
        bytes_per_image,
    })
}

pub(super) fn is_depth_stencil(pf: MTLPixelFormat) -> bool {
    matches!(
        pf,
        MTLPixelFormat::Depth16Unorm
            | MTLPixelFormat::Depth32Float
            | MTLPixelFormat::Depth32Float_Stencil8
            | MTLPixelFormat::Stencil8
    )
}

impl Gfx {
    pub(super) unsafe fn exec_copy(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        cmd: &Cmd,
    ) {
        let shm = st.sh.clone();
        self.end_encoder(st);
        match cmd {
            Cmd::CopyBufferToBuffer { src, dst, size } => {
                if *size == 0 {
                    return;
                }
                super::note_gpu_guest_write(super::GuestWrite::Copy, *dst, *size as usize);
                // The caches' lock is never held while this thread waits on the issuer (its worker may need it).
                let pair = {
                    let mut g = shm.lock().unwrap();
                    (
                        self.guest_buffer_ro(&mut g, &mut st.cx, *src, *size),
                        self.guest_buffer(&mut g, &mut st.cx, *dst, *size),
                    )
                };
                let (Some((sb, so)), Some((db, d_off))) = pair else {
                    warn_once("b2b", format!("CopyBufferToBuffer 0x{:x} -> 0x{:x} ({} bytes): an address is outside every memory pool", src, dst, size));
                    return;
                };
                // Debug: --log-texcopy reports buffer copies whose destination overlaps a texture's storage.
                if crate::settings::get().log.log_texcopy {
                    let (d0, d1) = (*dst as usize, *dst as usize + *size as usize);
                    let hits: Vec<String> = crate::nvn::api::textures_near_storage(
                        d1.saturating_sub(1),
                        *size as usize + (64 << 20),
                    )
                    .into_iter()
                    .filter(|(_, _, t)| {
                        let len = crate::nvn::layout::compute_layout(&super::texture_params(t))
                            .total as usize;
                        (t.storage as usize) < d1 && t.storage as usize + len > d0
                    })
                    .map(|(a, _, t)| format!("{:#x}:{}x{} fmt {}", a, t.width, t.height, t.format))
                    .collect();
                    if !hits.is_empty() {
                        println!("[texcopy] f{} copy {:#x} -> {:#x} ({:#x} bytes) overlaps {} textures: {}", super::presented_frames() + 1, src, dst, size, hits.len(), hits.iter().take(4).cloned().collect::<Vec<_>>().join(" "));
                    }
                }
                // Queued in order (metal/issuer.rs, Op::Blit): no wait for the encode pipeline to drain.
                let size = *size as usize;
                st.issuer.push(super::issuer::Op::Blit {
                    cb: cb.retain(),
                    label: "copy buffer",
                    f: Box::new(move |blit| {
                        blit.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                            &sb, so, &db, d_off, size,
                        );
                    }),
                });
                self.note_encoder(st, "copy (blit)");
                super::mark_gpu_dirty(st, *dst, *dst + size as u64);
                crate::guest::dirty_track::release_for_gpu(*dst as usize, size);
            }
            Cmd::ClearBuffer { dst, size, value } => {
                if *size == 0 {
                    return;
                }
                super::mark_gpu_dirty(st, *dst, *dst + *size as u64);
                crate::guest::dirty_track::release_for_gpu(*dst as usize, *size as usize);
                let b = value.to_le_bytes();
                if b.iter().all(|&x| x == b[0]) {
                    super::note_gpu_guest_write(super::GuestWrite::Copy, *dst, *size as usize);
                    let r = {
                        let mut g = shm.lock().unwrap();
                        self.guest_buffer(&mut g, &mut st.cx, *dst, *size)
                    };
                    let Some((db, off)) = r else {
                        warn_once(
                            "clearbuf",
                            format!(
                                "ClearBuffer 0x{:x} ({} bytes) is outside every memory pool",
                                dst, size
                            ),
                        );
                        return;
                    };
                    let (size, v) = (*size, b[0]);
                    st.issuer.push(super::issuer::Op::Blit {
                        cb: cb.retain(),
                        label: "fill buffer",
                        f: Box::new(move |blit| {
                            blit.fillBuffer_range_value(&db, NSRange::new(off, size), v);
                        }),
                    });
                    self.note_encoder(st, "copy (blit)");
                } else if *dst > 0x10000 {
                    // A repeating 32-bit pattern has no blit equivalent; write it directly (not ordered against the GPU).
                    warn_once(
                        "clearbuf-pattern",
                        "ClearBuffer with a non-uniform 32-bit value is written on the CPU"
                            .to_string(),
                    );
                    for i in 0..(*size / 4) {
                        let a = *dst as usize + 4 * i;
                        (super::fence_guard::engine_write_ptr(a) as *mut u32)
                            .write_unaligned(*value);
                    }
                }
            }
            Cmd::CopyBufferToTexture {
                src,
                dst,
                region,
                row_stride,
                image_stride,
            } => {
                let dst = &resolved(dst);
                let Some(tex) = self.texture(&mut st.tc, dst.texture) else {
                    return;
                };
                st.tc.gpu_written.insert(super::storage_id(&tex)); // contents now differ from guest memory
                let Some((_, t)) = tex_info(dst.texture) else {
                    return;
                };
                let t = &t;
                if is_depth_stencil(tex.pixelFormat()) {
                    warn_once(
                        "b2t-depth",
                        "CopyBufferToTexture into a depth/stencil texture is not supported"
                            .to_string(),
                    );
                    return;
                }
                let Some(g) = geometry(t, &tex, dst, region, *row_stride, *image_stride) else {
                    return;
                };
                let total = g.bytes_per_image * g.size.depth.max(g.layers);
                let r = {
                    let mut g = shm.lock().unwrap();
                    self.guest_buffer_ro(&mut g, &mut st.cx, *src, total)
                };
                let Some((sb, so)) = r else {
                    warn_once("b2t-src", format!("CopyBufferToTexture source 0x{:x} ({} bytes) is outside every memory pool", src, total));
                    return;
                };
                st.issuer.push(super::issuer::Op::Blit { cb: cb.retain(), label: "copy buffer->texture", f: Box::new(move |blit| {
                    for i in 0..g.layers {
                        blit.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                            &sb, so + i * g.bytes_per_image, g.bytes_per_row, g.bytes_per_image, g.size, &tex, g.slice0 + i, g.level, g.origin,
                        );
                    }
                }) });
                self.note_encoder(st, "copy (blit)");
                st.stats.copies += 1;
            }
            Cmd::CopyTextureToBuffer {
                src,
                region,
                dst,
                row_stride,
                image_stride,
            } => {
                let src = &resolved(src);
                let Some(tex) = self.texture(&mut st.tc, src.texture) else {
                    return;
                };
                let Some((_, t)) = tex_info(src.texture) else {
                    return;
                };
                let t = &t;
                if is_depth_stencil(tex.pixelFormat()) {
                    warn_once(
                        "t2b-depth",
                        "CopyTextureToBuffer from a depth/stencil texture is not supported"
                            .to_string(),
                    );
                    return;
                }
                let Some(g) = geometry(t, &tex, src, region, *row_stride, *image_stride) else {
                    return;
                };
                let total = g.bytes_per_image * g.size.depth.max(g.layers);
                super::note_gpu_guest_write(super::GuestWrite::Copy, *dst, total);
                let r = {
                    let mut g = shm.lock().unwrap();
                    self.guest_buffer(&mut g, &mut st.cx, *dst, total)
                };
                let Some((db, d_off)) = r else {
                    warn_once("t2b-dst", format!("CopyTextureToBuffer destination 0x{:x} ({} bytes) is outside every memory pool", dst, total));
                    return;
                };
                st.issuer.push(super::issuer::Op::Blit { cb: cb.retain(), label: "copy texture->buffer", f: Box::new(move |blit| {
                    for i in 0..g.layers {
                        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                            &tex, g.slice0 + i, g.level, g.origin, g.size, &db, d_off + i * g.bytes_per_image, g.bytes_per_row, g.bytes_per_image,
                        );
                    }
                }) });
                self.note_encoder(st, "copy (blit)");
                st.stats.copies += 1;
            }
            Cmd::CopyTextureToTexture(p) => {
                let (src, src_region, dst, dst_region) =
                    (&resolved(&p.src), &p.src_region, &resolved(&p.dst), &p.dst_region);
                let (Some(stex), Some(dtex)) = (
                    self.texture(&mut st.tc, src.texture),
                    self.texture(&mut st.tc, dst.texture),
                ) else {
                    return;
                };
                st.tc.gpu_written.insert(super::storage_id(&dtex)); // contents now differ from guest memory
                let (Some((_, st_)), Some((_, dt_))) =
                    (tex_info(src.texture), tex_info(dst.texture))
                else {
                    return;
                };
                let (st_, dt_) = (&st_, &dt_);
                let (Some(sg), Some(dg)) = (
                    geometry(st_, &stex, src, src_region, 0, 0),
                    geometry(dt_, &dtex, dst, dst_region, 0, 0),
                ) else {
                    return;
                };
                if stex.pixelFormat() != dtex.pixelFormat() {
                    warn_once(
                        "t2t-format",
                        format!("CopyTextureToTexture between formats {:?} and {:?} needs a conversion pass (skipped)", stex.pixelFormat(), dtex.pixelFormat()),
                    );
                    return;
                }
                st.issuer.push(super::issuer::Op::Blit { cb: cb.retain(), label: "copy texture->texture", f: Box::new(move |blit| {
                    for i in 0..sg.layers.min(dg.layers) {
                        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                            &stex, sg.slice0 + i, sg.level, sg.origin, sg.size, &dtex, dg.slice0 + i, dg.level, dg.origin,
                        );
                    }
                }) });
                self.note_encoder(st, "copy (blit)");
                st.stats.copies += 1;
            }
            Cmd::ClearTexture(p) => {
                let (dst, region, color, mask) = (&resolved(&p.dst), &p.region, &p.color, p.mask);
                let Some(tex) = self.texture(&mut st.tc, dst.texture) else {
                    return;
                };
                st.tc.gpu_written.insert(super::storage_id(&tex)); // contents now differ from guest memory
                let Some((_, t)) = tex_info(dst.texture) else {
                    return;
                };
                let t = &t;
                let pf = tex.pixelFormat();
                let level = (dst.level as usize).min(tex.mipmapLevelCount().saturating_sub(1));
                let (lw, lh) = (
                    ((t.width as usize) >> level).max(1),
                    ((t.height as usize) >> level).max(1),
                );
                let full = (region.w <= 0 || region.w as usize >= lw)
                    && (region.h <= 0 || region.h as usize >= lh)
                    && region.x <= 0
                    && region.y <= 0;
                let float_color =
                    !is_depth_stencil(pf) && !matches!(tex.textureType(), MTLTextureType::Type3D);
                if is_depth_stencil(pf) {
                    warn_once("cleartex", format!("ClearTexture of a depth/stencil target ({:?}) is not supported", pf));
                    return;
                }
                if !full || !float_color || (mask & 0xF) != 0xF && mask != 0 {
                    self.clear_region(st, cb, &tex, dst, region, color, mask, level, (lw, lh));
                    return;
                }
                let pass = MTLRenderPassDescriptor::renderPassDescriptor();
                let att = pass.colorAttachments().objectAtIndexedSubscript(0);
                att.setTexture(Some(&tex));
                att.setLevel(level);
                att.setSlice(dst.layer as usize);
                att.setLoadAction(MTLLoadAction::Clear);
                att.setStoreAction(MTLStoreAction::Store);
                let f = |i: usize| f32::from_bits(color[i]) as f64;
                att.setClearColor(MTLClearColor {
                    red: f(0),
                    green: f(1),
                    blue: f(2),
                    alpha: f(3),
                });
                if super::tracing_frame() {
                    println!("[ftrace] CLEAR TEXTURE nvn {:#x} {}x{} {:?} level {} layer {} rgba=[{}, {}, {}, {}] mask {:#x}",
                        dst.texture, tex.width(), tex.height(), pf, level, dst.layer, f(0), f(1), f(2), f(3), mask);
                }
                st.issuer.push(super::issuer::Op::Begin {
                    cb: cb.retain(),
                    desc: pass,
                    label: "clear texture".to_string(),
                });
                st.issuer.push(super::issuer::Op::End);
                self.note_encoder(st, "clear texture");
            }
            _ => {}
        }
    }
}

/// A recorded texture reference with the object it names now (nvn::api::resolve_texture_ref).
fn resolved(r: &crate::nvn::cmd::TexRef) -> crate::nvn::cmd::TexRef {
    crate::nvn::cmd::TexRef { texture: crate::nvn::api::resolve_texture_ref(r.texture, r.gen), ..*r }
}

impl super::Gfx {
    /// ClearTexture of part of a level, some channels, or slices of a 3D texture: a triangle over the level, the region as
    /// scissor and the channel mask as write mask, per slice.
    #[allow(clippy::too_many_arguments)]
    fn clear_region(
        &self,
        st: &mut super::State,
        cb: &ProtocolObject<dyn objc2_metal::MTLCommandBuffer>,
        tex: &super::Tex,
        dst: &crate::nvn::cmd::TexRef,
        region: &crate::nvn::cmd::Region,
        color: &[u32; 4],
        mask: i32,
        level: usize,
        (lw, lh): (usize, usize),
    ) {
        use objc2_metal::MTLDevice;
        let pf = tex.pixelFormat();
        // NVNclearColorMask: R 1, G 2, B 4, A 8 (0 is treated as all channels, as the full-clear path does).
        let mask = if mask & 0xF == 0 { 0xF } else { (mask & 0xF) as u8 };
        let kind = super::translated::pf_kind(pf);
        let key = (pf.0 as usize, mask);
        if !st.clear_pipes.contains_key(&key) {
            let ty = ["float4", "uint4", "int4"][kind.clamp(0, 2) as usize];
            let src = format!(
                "#include <metal_stdlib>\nusing namespace metal;\n\
                 vertex float4 clear_vs(uint i [[vertex_id]]) {{ return float4(float((i << 1) & 2) * 2.0 - 1.0, float(i & 2) * 2.0 - 1.0, 0.0, 1.0); }}\n\
                 fragment {ty} clear_fs(constant uint4& c [[buffer(0)]]) {{ return as_type<{ty}>(c); }}\n"
            );
            let Ok(lib) = self.device.newLibraryWithSource_options_error(&NSString::from_str(&src), None) else {
                return;
            };
            let desc = objc2_metal::MTLRenderPipelineDescriptor::new();
            desc.setVertexFunction(lib.newFunctionWithName(&NSString::from_str("clear_vs")).as_deref());
            desc.setFragmentFunction(lib.newFunctionWithName(&NSString::from_str("clear_fs")).as_deref());
            let att = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
            att.setPixelFormat(pf);
            let mut wm = objc2_metal::MTLColorWriteMask::empty();
            for (bit, m) in [
                (1, objc2_metal::MTLColorWriteMask::Red),
                (2, objc2_metal::MTLColorWriteMask::Green),
                (4, objc2_metal::MTLColorWriteMask::Blue),
                (8, objc2_metal::MTLColorWriteMask::Alpha),
            ] {
                if mask & bit != 0 {
                    wm |= m;
                }
            }
            att.setWriteMask(wm);
            let Ok(p) = self.device.newRenderPipelineStateWithDescriptor_error(&desc) else {
                warn_once("cleartex-pipe", format!("ClearTexture: no clear pipeline for {:?}", pf));
                return;
            };
            st.clear_pipes.insert(key, p);
        }
        let pipe = st.clear_pipes[&key].clone();
        // Float formats take the clear color as floats, integer formats as raw values: either way the bits pass through.
        let bytes: Vec<u8> = color.iter().flat_map(|c| c.to_le_bytes()).collect();
        let Some(buf) = (unsafe {
            self.device.newBufferWithBytes_length_options(
                std::ptr::NonNull::new(bytes.as_ptr() as *mut libc::c_void).unwrap(),
                bytes.len(),
                objc2_metal::MTLResourceOptions::StorageModeShared,
            )
        }) else {
            return;
        };
        let x0 = region.x.max(0) as usize;
        let y0 = region.y.max(0) as usize;
        let x1 = if region.w <= 0 { lw } else { (region.x as i64 + region.w as i64).clamp(0, lw as i64) as usize };
        let y1 = if region.h <= 0 { lh } else { (region.y as i64 + region.h as i64).clamp(0, lh as i64) as usize };
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let is_3d = tex.textureType() == MTLTextureType::Type3D;
        let slices: Vec<usize> = if is_3d {
            let depth = (tex.depth() >> level).max(1);
            let z0 = region.z.max(0) as usize;
            let z1 = if region.d <= 0 { depth } else { (z0 + region.d as usize).min(depth) };
            (z0..z1).collect()
        } else {
            vec![dst.layer as usize]
        };
        for slice in slices {
            let pass = MTLRenderPassDescriptor::renderPassDescriptor();
            let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            att.setTexture(Some(tex));
            att.setLevel(level);
            if is_3d {
                att.setDepthPlane(slice);
            } else {
                att.setSlice(slice);
            }
            att.setLoadAction(MTLLoadAction::Load);
            att.setStoreAction(MTLStoreAction::Store);
            st.issuer.push(super::issuer::Op::Begin { cb: cb.retain(), desc: pass, label: "clear texture region".to_string() });
            st.issuer.push(super::issuer::Op::Pipeline(pipe.clone()));
            st.issuer.push(super::issuer::Op::Scissor(objc2_metal::MTLScissorRect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            }));
            st.issuer.push(super::issuer::Op::FBuf(buf.clone(), 0, 0));
            st.issuer.push(super::issuer::Op::Draw {
                prim: objc2_metal::MTLPrimitiveType::Triangle,
                start: 0,
                count: 3,
                instances: 1,
                base_instance: 0,
            });
            st.issuer.push(super::issuer::Op::End);
            self.note_encoder(st, "clear texture region");
        }
    }
}
