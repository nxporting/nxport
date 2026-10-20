// Dear ImGui draw data on Metal. Renders into any colour texture of one pixel format, so a window of its own and an
// overlay on the game's presented frame share it. Textures (the font atlas pages) are created and updated as Dear
// ImGui requests them. Shaders are compiled from source at startup by the system's Metal compiler.

use std::collections::HashMap;

use dear_imgui_rs as imgui;
use imgui::render::{
    DrawCmd, PendingFrame, SnapshotTextureId, SynchronousRendererConsumer, TextureOp,
};
use imgui::{BackendFlags, TextureFormat, TextureId};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::Message;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBlendFactor, MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder, MTLDevice,
    MTLIndexType, MTLLibrary, MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType,
    MTLRegion, MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLRenderPipelineDescriptor,
    MTLRenderPipelineState, MTLResourceOptions, MTLSamplerDescriptor, MTLSamplerMinMagFilter,
    MTLSamplerState, MTLScissorRect, MTLSize, MTLStoreAction, MTLTexture, MTLTextureDescriptor,
    MTLTextureUsage, MTLViewport,
};

const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct Vert { packed_float2 pos; packed_float2 uv; uchar4 col; };
struct Out { float4 pos [[position]]; float2 uv; float4 col; };

vertex Out ui_vs(const device Vert* v [[buffer(0)]], constant float4& xf [[buffer(1)]], uint vid [[vertex_id]]) {
    Out o;
    o.pos = float4(float2(v[vid].pos) * xf.xy + xf.zw, 0.0, 1.0);
    o.uv = float2(v[vid].uv);
    o.col = float4(v[vid].col) / 255.0;
    return o;
}

fragment float4 ui_fs(Out in [[stage_in]], texture2d<float> tex [[texture(0)]], sampler s [[sampler(0)]]) {
    return in.col * tex.sample(s, in.uv);
}
"#;

// ui_vs reads DrawVert as {packed_float2 pos, packed_float2 uv, uchar4 col}: 20 bytes, no padding.
const _: () = assert!(std::mem::size_of::<imgui::render::DrawVert>() == 20);

type Texture = Retained<ProtocolObject<dyn MTLTexture>>;

pub struct Renderer {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    pipeline: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    linear: Retained<ProtocolObject<dyn MTLSamplerState>>,
    nearest: Retained<ProtocolObject<dyn MTLSamplerState>>,
    consumer: SynchronousRendererConsumer,
    /// Dear ImGui's textures by its snapshot id, with the id draw commands name them by.
    textures: HashMap<SnapshotTextureId, (TextureId, Texture)>,
    by_id: HashMap<u64, Texture>,
    next_id: u64,
}

impl Renderer {
    /// A renderer for targets of `format`, registered with `ctx` as its texture consumer.
    pub fn new(
        device: &ProtocolObject<dyn MTLDevice>,
        ctx: &mut imgui::Context,
        format: MTLPixelFormat,
    ) -> Result<Self, String> {
        let lib = device
            .newLibraryWithSource_options_error(&NSString::from_str(MSL), None)
            .map_err(|e| format!("imgui shaders: {}", e.localizedDescription()))?;
        let vs = lib
            .newFunctionWithName(&NSString::from_str("ui_vs"))
            .ok_or("ui_vs missing")?;
        let fs = lib
            .newFunctionWithName(&NSString::from_str("ui_fs"))
            .ok_or("ui_fs missing")?;
        let desc = MTLRenderPipelineDescriptor::new();
        desc.setVertexFunction(Some(&vs));
        desc.setFragmentFunction(Some(&fs));
        unsafe {
            let ca = desc.colorAttachments().objectAtIndexedSubscript(0);
            ca.setPixelFormat(format);
            ca.setBlendingEnabled(true);
            ca.setSourceRGBBlendFactor(MTLBlendFactor::SourceAlpha);
            ca.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            ca.setSourceAlphaBlendFactor(MTLBlendFactor::One);
            ca.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        }
        let pipeline = device
            .newRenderPipelineStateWithDescriptor_error(&desc)
            .map_err(|e| format!("imgui pipeline: {}", e.localizedDescription()))?;
        let sampler = |f: MTLSamplerMinMagFilter| {
            let sd = MTLSamplerDescriptor::new();
            sd.setMinFilter(f);
            sd.setMagFilter(f);
            device
                .newSamplerStateWithDescriptor(&sd)
                .ok_or("imgui sampler")
        };
        let (linear, nearest) = (
            sampler(MTLSamplerMinMagFilter::Linear)?,
            sampler(MTLSamplerMinMagFilter::Nearest)?,
        );

        ctx.set_renderer_name(Some("nxport-metal"))
            .map_err(|e| e.to_string())?;
        let mut flags = ctx.io().backend_flags();
        flags.insert(BackendFlags::RENDERER_HAS_TEXTURES);
        flags.insert(BackendFlags::RENDERER_HAS_VTX_OFFSET);
        ctx.io_mut().set_backend_flags(flags);
        let consumer = ctx
            .create_synchronous_renderer_consumer()
            .map_err(|e| e.to_string())?;
        Ok(Renderer {
            device: device.retain(),
            pipeline,
            linear,
            nearest,
            consumer,
            textures: HashMap::new(),
            by_id: HashMap::new(),
            next_id: 1,
        })
    }

    pub fn consumer(&self) -> &SynchronousRendererConsumer {
        &self.consumer
    }

    /// Applies the frame's texture requests, then encodes its draw data into `target`, cleared to `clear` first or
    /// drawn over its contents with None.
    pub fn render(
        &mut self,
        frame: PendingFrame<'_>,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        target: &ProtocolObject<dyn MTLTexture>,
        clear: Option<[f64; 4]>,
    ) -> Result<(), String> {
        let mut feedback = Vec::with_capacity(frame.texture_requests().len());
        for request in frame.texture_requests() {
            let fb = match request.operation() {
                TextureOp::Create {
                    format,
                    width,
                    height,
                    row_pitch,
                    pixels,
                } => {
                    let tex = self.new_texture(*width as usize, *height as usize)?;
                    upload(
                        &tex,
                        *format,
                        0,
                        0,
                        *width as usize,
                        *height as usize,
                        *row_pitch,
                        pixels,
                    );
                    let id = TextureId::new(self.next_id);
                    self.next_id += 1;
                    self.by_id.insert(id.id(), tex.clone());
                    if let Some((old, _)) = self.textures.insert(request.texture(), (id, tex)) {
                        self.by_id.remove(&old.id());
                    }
                    request.uploaded(id).map_err(|e| e.to_string())?
                }
                TextureOp::Update { format, rects, .. } => {
                    match self.textures.get(&request.texture()) {
                        Some((id, tex)) => {
                            for r in rects {
                                let rc = r.rect;
                                upload(
                                    tex,
                                    *format,
                                    rc.x as usize,
                                    rc.y as usize,
                                    rc.w as usize,
                                    rc.h as usize,
                                    r.row_pitch,
                                    &r.data,
                                );
                            }
                            request.uploaded(*id).map_err(|e| e.to_string())?
                        }
                        None => request.retry(),
                    }
                }
                TextureOp::Destroy => {
                    if let Some((id, _)) = self.textures.remove(&request.texture()) {
                        self.by_id.remove(&id.id());
                    }
                    request.destroyed().map_err(|e| e.to_string())?
                }
            };
            feedback.push(fb);
        }
        let frame = frame
            .reconcile_texture_feedback(feedback)
            .map_err(|e| e.to_string())?;
        self.encode(frame.draw_data(), cb, target, clear);
        Ok(())
    }

    fn encode(
        &self,
        draw: &imgui::render::DrawData,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        target: &ProtocolObject<dyn MTLTexture>,
        clear: Option<[f64; 4]>,
    ) {
        let (fb_w, fb_h) = (target.width() as f32, target.height() as f32);
        let pass = MTLRenderPassDescriptor::renderPassDescriptor();
        unsafe {
            let att = pass.colorAttachments().objectAtIndexedSubscript(0);
            att.setTexture(Some(target));
            match clear {
                Some([r, g, b, a]) => {
                    att.setLoadAction(MTLLoadAction::Clear);
                    att.setClearColor(MTLClearColor {
                        red: r,
                        green: g,
                        blue: b,
                        alpha: a,
                    });
                }
                None => att.setLoadAction(MTLLoadAction::Load),
            }
            att.setStoreAction(MTLStoreAction::Store);
        }
        let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) else {
            return;
        };
        let [dw, dh] = draw.display_size();
        if dw > 0.0 && dh > 0.0 && draw.total_idx_count() > 0 {
            let [ox, oy] = draw.display_pos();
            let [sx, sy] = draw.framebuffer_scale();
            // Display coordinates to clip space.
            let xf: [f32; 4] = [
                2.0 / dw,
                -2.0 / dh,
                -1.0 - ox * 2.0 / dw,
                1.0 + oy * 2.0 / dh,
            ];
            let setup = |enc: &ProtocolObject<dyn MTLRenderCommandEncoder>| unsafe {
                enc.setRenderPipelineState(&self.pipeline);
                enc.setViewport(MTLViewport {
                    originX: 0.0,
                    originY: 0.0,
                    width: fb_w as f64,
                    height: fb_h as f64,
                    znear: 0.0,
                    zfar: 1.0,
                });
                enc.setVertexBytes_length_atIndex(std::ptr::NonNull::from(&xf).cast(), 16, 1);
                enc.setFragmentSamplerState_atIndex(Some(&self.linear), 0);
            };
            setup(&enc);
            for list in draw.draw_lists() {
                let (vtx, idx) = (list.vtx_buffer(), list.idx_buffer());
                let (Some(vb), Some(ib)) = (self.buffer(vtx), self.buffer(idx)) else {
                    continue;
                };
                unsafe { enc.setVertexBuffer_offset_atIndex(Some(&vb), 0, 0) };
                for cmd in list.commands() {
                    let (count, p) = match cmd {
                        DrawCmd::Elements { count, cmd_params } => (count, cmd_params),
                        DrawCmd::ResetRenderState => {
                            setup(&enc);
                            unsafe { enc.setVertexBuffer_offset_atIndex(Some(&vb), 0, 0) };
                            continue;
                        }
                        DrawCmd::SetSamplerLinear | DrawCmd::SetSamplerFromTexture => {
                            unsafe { enc.setFragmentSamplerState_atIndex(Some(&self.linear), 0) };
                            continue;
                        }
                        DrawCmd::SetSamplerNearest => {
                            unsafe { enc.setFragmentSamplerState_atIndex(Some(&self.nearest), 0) };
                            continue;
                        }
                        DrawCmd::RawCallback(_) => continue,
                    };
                    let Some(tex) = self.by_id.get(&p.texture_id.id()) else {
                        continue;
                    };
                    let [x0, y0, x1, y1] = p.clip_rect;
                    let (x0, y0) = (((x0 - ox) * sx).max(0.0), ((y0 - oy) * sy).max(0.0));
                    let (x1, y1) = (((x1 - ox) * sx).min(fb_w), ((y1 - oy) * sy).min(fb_h));
                    if x1 <= x0 || y1 <= y0 {
                        continue;
                    }
                    enc.setScissorRect(MTLScissorRect {
                        x: x0 as usize,
                        y: y0 as usize,
                        width: (x1 - x0) as usize,
                        height: (y1 - y0) as usize,
                    });
                    unsafe {
                        enc.setFragmentTexture_atIndex(Some(tex), 0);
                        enc.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount_baseVertex_baseInstance(
                            MTLPrimitiveType::Triangle,
                            count,
                            MTLIndexType::UInt16,
                            &ib,
                            p.idx_offset * std::mem::size_of::<imgui::render::DrawIdx>(),
                            1,
                            p.vtx_offset as isize,
                            0,
                        );
                    }
                }
            }
        }
        enc.endEncoding();
    }

    fn new_texture(&self, w: usize, h: usize) -> Result<Texture, String> {
        let td = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::RGBA8Unorm,
                w.max(1),
                h.max(1),
                false,
            )
        };
        td.setUsage(MTLTextureUsage::ShaderRead);
        self.device
            .newTextureWithDescriptor(&td)
            .ok_or_else(|| "imgui texture".to_string())
    }

    fn buffer<T>(&self, data: &[T]) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        let len = std::mem::size_of_val(data);
        if len == 0 {
            return None;
        }
        unsafe {
            self.device.newBufferWithBytes_length_options(
                std::ptr::NonNull::new(data.as_ptr() as *mut libc::c_void)?,
                len,
                MTLResourceOptions::StorageModeShared,
            )
        }
    }
}

/// Writes a w x h region at (x, y). Alpha8 data becomes white with that alpha, so one shader serves both formats.
fn upload(
    tex: &Texture,
    format: TextureFormat,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    row_pitch: usize,
    data: &[u8],
) {
    if w == 0 || h == 0 {
        return;
    }
    let rgba: Vec<u8> = match format {
        TextureFormat::RGBA32 => (0..h)
            .flat_map(|r| data[r * row_pitch..r * row_pitch + w * 4].iter().copied())
            .collect(),
        TextureFormat::Alpha8 => (0..h)
            .flat_map(|r| {
                data[r * row_pitch..r * row_pitch + w]
                    .iter()
                    .flat_map(|&a| [255, 255, 255, a])
            })
            .collect(),
    };
    unsafe {
        tex.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
            MTLRegion {
                origin: MTLOrigin { x, y, z: 0 },
                size: MTLSize {
                    width: w,
                    height: h,
                    depth: 1,
                },
            },
            0,
            std::ptr::NonNull::new(rgba.as_ptr() as *mut libc::c_void).unwrap(),
            w * 4,
        );
    }
}
