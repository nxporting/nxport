// Contents of a scaled texture (gpu::scale) brought down to the texture made at the game's size when its memory is
// demoted: each texel is the average of the k x k it covered. Queued by texture creation, run before the next draw or
// copy (as the depth twins' copies are).

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
"#;

fn pipeline(device: &Device, pf: MTLPixelFormat) -> Option<Retained<ProtocolObject<dyn MTLRenderPipelineState>>> {
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
    desc.setFragmentFunction(lib.newFunctionWithName(&NSString::from_str("ds_fs")).as_deref());
    unsafe { desc.colorAttachments().objectAtIndexedSubscript(0).setPixelFormat(pf) };
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
            let pf = dst.pixelFormat();
            if dst.textureType() != MTLTextureType::Type2D
                || src.textureType() != MTLTextureType::Type2D
                || copy::is_depth_stencil(pf)
                || !read_as_float(pf)
            {
                copy::warn_once("downscale", format!("scaled {:?} {:?} demoted without its contents", dst.textureType(), pf));
                continue;
            }
            let k = (src.width() / dst.width().max(1)).max(1) as u32;
            let Some(pipe) = pipeline(&self.device, pf) else { continue };
            for level in 0..dst.mipmapLevelCount().min(src.mipmapLevelCount()) {
                let pass = MTLRenderPassDescriptor::renderPassDescriptor();
                let att = pass.colorAttachments().objectAtIndexedSubscript(0);
                att.setTexture(Some(&dst));
                att.setLevel(level);
                att.setLoadAction(MTLLoadAction::DontCare);
                att.setStoreAction(MTLStoreAction::Store);
                let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) else { continue };
                enc.setRenderPipelineState(&pipe);
                enc.setFragmentTexture_atIndex(Some(&src), 0);
                let kl = [k, level as u32];
                enc.setFragmentBytes_length_atIndex(
                    std::ptr::NonNull::new(kl.as_ptr() as *mut std::ffi::c_void).unwrap(),
                    8,
                    0,
                );
                enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
                enc.endEncoding();
            }
            st.tc.mark_gpu_written(storage_id(&dst));
        }
    }
}
