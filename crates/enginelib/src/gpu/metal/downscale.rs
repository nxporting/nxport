// Contents of a scaled texture (gpu::scale) brought down to the texture made at the game's size when its memory is
// demoted: each colour texel is the average of the k x k it covered; integer and depth texels take the first of them
// (an average would make values the game never wrote). Queued by texture creation, run before the next draw or copy (as
// the depth twins' copies are).

use super::*;
use objc2_metal::{MTLRenderCommandEncoder, MTLRenderPipelineDescriptor};

const MSL: &str = r#"
#include <metal_stdlib>
using namespace metal;
struct VOut { float4 pos [[position]]; };
vertex VOut ds_vs(uint vid [[vertex_id]]) {
    float2 p = float2((vid << 1) & 2, vid & 2);
    VOut o; o.pos = float4(p * 2.0 - 1.0, 0.0, 1.0); return o;
}
// kl: scale, source level
fragment float4 ds_fs(VOut in [[stage_in]], texture2d<float> t [[texture(0)]], constant uint2& kl [[buffer(0)]]) {
    uint2 c = uint2(in.pos.xy) * kl.x;
    float4 s = 0.0;
    for (uint y = 0; y < kl.x; y++)
        for (uint x = 0; x < kl.x; x++)
            s += t.read(c + uint2(x, y), kl.y);
    return s / float(kl.x * kl.x);
}
fragment uint4 ds_fs_u(VOut in [[stage_in]], texture2d<uint> t [[texture(0)]], constant uint2& kl [[buffer(0)]]) {
    return t.read(uint2(in.pos.xy) * kl.x, kl.y);
}
fragment int4 ds_fs_i(VOut in [[stage_in]], texture2d<int> t [[texture(0)]], constant uint2& kl [[buffer(0)]]) {
    return t.read(uint2(in.pos.xy) * kl.x, kl.y);
}
struct DOut { float d [[depth(any)]]; };
fragment DOut ds_fs_d(VOut in [[stage_in]], depth2d<float> t [[texture(0)]], constant uint2& kl [[buffer(0)]]) {
    DOut o; o.d = t.read(uint2(in.pos.xy) * kl.x, kl.y); return o;
}
"#;

/// How a format's texels are read and written.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Float,
    Uint,
    Sint,
    Depth,
}

fn kind(pf: MTLPixelFormat) -> Option<Kind> {
    use MTLPixelFormat as F;
    if read_as_float(pf) {
        return Some(Kind::Float);
    }
    match pf {
        F::R8Uint | F::RG8Uint | F::RGBA8Uint | F::R16Uint | F::RG16Uint | F::RGBA16Uint | F::R32Uint | F::RG32Uint
        | F::RGBA32Uint | F::RGB10A2Uint => Some(Kind::Uint),
        F::R8Sint | F::RG8Sint | F::RGBA8Sint | F::R16Sint | F::RG16Sint | F::RGBA16Sint | F::R32Sint | F::RG32Sint
        | F::RGBA32Sint => Some(Kind::Sint),
        // Depth only: a fragment shader cannot write stencil here.
        F::Depth16Unorm | F::Depth32Float => Some(Kind::Depth),
        _ => None,
    }
}

fn pipeline(device: &Device, pf: MTLPixelFormat, k: Kind) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
    static PIPES: Mutex<Option<HashMap<usize, Retained<ProtocolObject<dyn MTLRenderPipelineState>>>>> =
        Mutex::new(None);
    let mut g = PIPES.lock().unwrap();
    let m = g.get_or_insert_with(HashMap::default);
    if let Some(p) = m.get(&(pf.0 as usize)) {
        return Some(p.clone());
    }
    let lib = device.newLibraryWithSource_options_error(&NSString::from_str(MSL), None).ok()?;
    let desc = MTLRenderPipelineDescriptor::new();
    desc.setVertexFunction(lib.newFunctionWithName(&NSString::from_str("ds_vs")).as_deref());
    let fs = match k {
        Kind::Float => "ds_fs",
        Kind::Uint => "ds_fs_u",
        Kind::Sint => "ds_fs_i",
        Kind::Depth => "ds_fs_d",
    };
    desc.setFragmentFunction(lib.newFunctionWithName(&NSString::from_str(fs)).as_deref());
    if k == Kind::Depth {
        desc.setDepthAttachmentPixelFormat(pf);
    } else {
        unsafe { desc.colorAttachments().objectAtIndexedSubscript(0).setPixelFormat(pf) };
    }
    let p = device.newRenderPipelineStateWithDescriptor_error(&desc).ok()?;
    m.insert(pf.0 as usize, p.clone());
    Some(p)
}

/// Colour formats a float shader reads and writes (render-target formats; integer and depth ones need other shaders).
fn read_as_float(pf: MTLPixelFormat) -> bool {
    use MTLPixelFormat as F;
    matches!(
        pf,
        F::R8Unorm | F::RG8Unorm | F::RGBA8Unorm | F::RGBA8Unorm_sRGB | F::BGRA8Unorm | F::BGRA8Unorm_sRGB
            | F::R16Unorm | F::RG16Unorm | F::RGBA16Unorm | F::R16Float | F::RG16Float | F::RGBA16Float
            | F::R32Float | F::RG32Float | F::RGBA32Float | F::RGB10A2Unorm | F::RG11B10Float
    )
}

/// Depth test always passing, depth written: the scale-down's depth pass.
fn depth_write_always(device: &Device) -> Option<Retained<ProtocolObject<dyn objc2_metal::MTLDepthStencilState>>> {
    static S: Mutex<Option<Retained<ProtocolObject<dyn objc2_metal::MTLDepthStencilState>>>> = Mutex::new(None);
    let mut g = S.lock().unwrap();
    if g.is_none() {
        let d = objc2_metal::MTLDepthStencilDescriptor::new();
        d.setDepthCompareFunction(objc2_metal::MTLCompareFunction::Always);
        d.setDepthWriteEnabled(true);
        *g = device.newDepthStencilStateWithDescriptor(&d);
    }
    g.clone()
}

impl Gfx {
    /// Runs the queued scale-downs (TexCache::downscale_pending), between passes.
    pub(crate) unsafe fn run_downscales(&self, st: &mut State) {
        let pending = std::mem::take(&mut st.tc.downscale_pending);
        let Some(cb) = st.cur_cb.clone() else { return };
        if st.enc.is_some() {
            self.end_encoder_keep_clears(st);
        }
        st.issuer.sync(); // other command-buffer use: queued encoder ops first (metal/issuer.rs)
        for (src, dst) in pending {
            if !self.encode_downscale(&cb, &src, &dst) {
                continue;
            }
            st.tc.mark_gpu_written(storage_id(&dst));
        }
    }

    /// Encodes `src` (a scaled texture) averaged down into `dst` (the game's size), every level they share. False: the
    /// format or type has no downscale (warned once).
    pub(crate) unsafe fn encode_downscale(
        &self,
        cb: &ProtocolObject<dyn objc2_metal::MTLCommandBuffer>,
        src: &Tex,
        dst: &Tex,
    ) -> bool {
        let pf = dst.pixelFormat();
        let kd = kind(pf);
        if dst.textureType() != MTLTextureType::Type2D || src.textureType() != MTLTextureType::Type2D || kd.is_none() {
            copy::warn_once("downscale", format!("scaled {:?} {:?} demoted without its contents", dst.textureType(), pf));
            return false;
        }
        let kd = kd.unwrap();
        let k = (src.width() / dst.width().max(1)).max(1) as u32;
        let Some(pipe) = pipeline(&self.device, pf, kd) else { return false };
        for level in 0..dst.mipmapLevelCount().min(src.mipmapLevelCount()) {
            let pass = MTLRenderPassDescriptor::renderPassDescriptor();
            if kd == Kind::Depth {
                let att = pass.depthAttachment();
                att.setTexture(Some(dst));
                att.setLevel(level);
                att.setLoadAction(MTLLoadAction::DontCare);
                att.setStoreAction(MTLStoreAction::Store);
            } else {
                let att = pass.colorAttachments().objectAtIndexedSubscript(0);
                att.setTexture(Some(dst));
                att.setLevel(level);
                att.setLoadAction(MTLLoadAction::DontCare);
                att.setStoreAction(MTLStoreAction::Store);
            }
            let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) else { continue };
            enc.setRenderPipelineState(&pipe);
            if kd == Kind::Depth {
                if let Some(ds) = depth_write_always(&self.device) {
                    enc.setDepthStencilState(Some(&ds));
                }
            }
            enc.setFragmentTexture_atIndex(Some(src), 0);
            let kl = [k, level as u32];
            enc.setFragmentBytes_length_atIndex(
                std::ptr::NonNull::new(kl.as_ptr() as *mut std::ffi::c_void).unwrap(),
                8,
                0,
            );
            enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
            enc.endEncoding();
        }
        true
    }
}
