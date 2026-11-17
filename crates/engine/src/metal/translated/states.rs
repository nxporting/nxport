// Sampler and depth / stencil states built from NVN state, the default sampler and the sampler argument buffer
// (stages with more than 16 samplers).

use super::*;

impl Gfx {
    pub(crate) fn sampler_state(
        &self,
        sh: &mut SharedCaches,
        id: i32,
    ) -> Option<Retained<ProtocolObject<dyn MTLSamplerState>>> {
        // Per-thread memo by sampler id while no sampler was registered since (horizon::hle::samplers_gen): the pool lookup took
        // a global lock per texture binding. --no-sampler-memo / --replay-ab-sampler (side B): off.
        thread_local! {
            static MEMO: std::cell::RefCell<(u64, HashMap<i32, Retained<ProtocolObject<dyn MTLSamplerState>>>)> =
                std::cell::RefCell::new((0, HashMap::default()));
        }
        let gen = crate::nvn::api::samplers_gen();
        {
            if let Some(s) = MEMO.with(|m| {
                let m = m.borrow();
                if m.0 == gen {
                    m.1.get(&id).cloned()
                } else {
                    None
                }
            }) {
                return Some(s);
            }
        }
        let r = self.sampler_state_slow(sh, id);
        if let Some(x) = &r {
            MEMO.with(|m| {
                let mut m = m.borrow_mut();
                if m.0 != gen || m.1.len() > 1 << 14 {
                    m.1.clear();
                    m.0 = gen;
                }
                m.1.insert(id, x.clone());
            });
        }
        r
    }

    pub(crate) fn sampler_state_slow(
        &self,
        sh: &mut SharedCaches,
        id: i32,
    ) -> Option<Retained<ProtocolObject<dyn MTLSamplerState>>> {
        let desc = crate::nvn::api::lookup_pool_sampler(id)?;
        if let Some((d, s)) = sh.samplers.get(&id) {
            if *d == desc {
                return Some(s.clone());
            }
        }
        if let Some((_, s)) = sh.sampler_states.iter().find(|(d, _)| *d == desc) {
            let s = s.clone();
            sh.samplers.insert(id, (desc, s.clone()));
            return Some(s);
        }
        let sd = MTLSamplerDescriptor::new();
        let (min_linear, mip) = match desc.min_filter {
            0 => (false, MTLSamplerMipFilter::NotMipmapped),
            1 => (true, MTLSamplerMipFilter::NotMipmapped),
            2 => (false, MTLSamplerMipFilter::Nearest),
            3 => (true, MTLSamplerMipFilter::Nearest),
            4 => (false, MTLSamplerMipFilter::Linear),
            _ => (true, MTLSamplerMipFilter::Linear),
        };
        let mm = |linear: bool| {
            if linear {
                MTLSamplerMinMagFilter::Linear
            } else {
                MTLSamplerMinMagFilter::Nearest
            }
        };
        sd.setMinFilter(mm(min_linear));
        sd.setMagFilter(mm(desc.mag_filter != 0));
        sd.setMipFilter(mip);
        sd.setSAddressMode(address_mode(desc.wrap_s));
        sd.setTAddressMode(address_mode(desc.wrap_t));
        sd.setRAddressMode(address_mode(desc.wrap_r));
        sd.setLodMinClamp(desc.lod_min);
        // --debug-max-lod=N (diagnostic): clamp every sampler's maximum LOD (0 = base level only), to test mip contents.
        let max_lod = crate::settings::get()
            .debug
            .debug_max_lod
            .map_or(desc.lod_max, |m| desc.lod_max.min(m));
        sd.setLodMaxClamp(max_lod);
        sd.setMaxAnisotropy((desc.max_anisotropy.round() as usize).clamp(1, 16));
        if desc.compare_mode != 0 {
            sd.setCompareFunction(compare_func(desc.compare_func as u32));
        }
        let [r, g, b, a] = desc.border_color;
        sd.setBorderColor(if a < 0.5 {
            MTLSamplerBorderColor::TransparentBlack
        } else if r + g + b > 1.5 {
            MTLSamplerBorderColor::OpaqueWhite
        } else {
            MTLSamplerBorderColor::OpaqueBlack
        });
        sd.setSupportArgumentBuffers(true); // samplers 16+ are passed by handle (see bind_extra_samplers)
        let s = self.device.newSamplerStateWithDescriptor(&sd)?;
        sh.samplers.insert(id, (desc, s.clone()));
        sh.sampler_states.push((desc, s.clone()));
        Some(s)
    }

    pub(crate) fn depth_stencil_state(
        &self,
        sh: &mut SharedCaches,
        stencil: [[u8; 2]; 3],
        bits: u64,
        has_stencil: bool,
    ) -> Option<Retained<ProtocolObject<dyn MTLDepthStencilState>>> {
        // The stencil test only exists with a stencil attachment; its masks are dynamic state in NVN but part of the
        // state object in Metal, so they join the cache key.
        let stencil_on = has_stencil && (bits >> 6) & 1 != 0;
        let key: u128 = if stencil_on {
            bits as u128
                | 1 << 127
                | (u32::from_le_bytes([stencil[1][0], stencil[1][1], stencil[2][0], stencil[2][1]])
                    as u128)
                    << 64
        } else {
            (bits & !(1 << 6)) as u128
        };
        if let Some(d) = sh.ds_states.get(&key) {
            return Some(d.clone());
        }
        let test = bits & 1 != 0;
        let write = (bits >> 1) & 1 != 0;
        let func = ((bits >> 2) & 0xF) as u32;
        let desc = MTLDepthStencilDescriptor::new();
        // GL semantics (NVN follows them): with the depth test off the depth buffer is neither tested nor written.
        let cmp = compare_func(func);
        desc.setDepthCompareFunction(if test {
            cmp
        } else {
            MTLCompareFunction::Always
        });
        desc.setDepthWriteEnabled(test && write);
        if stencil_on {
            // NVNdepthStencilState bits (nvn/state.rs): front func 7..11, fail 11..15, depth fail 15..19, pass 19..23;
            // back at 23.. The stencil funcs share the depth compare enum (NEVER 1 .. ALWAYS 8).
            let op = |v: u64| match v {
                2 => objc2_metal::MTLStencilOperation::Zero,
                3 => objc2_metal::MTLStencilOperation::Replace,
                4 => objc2_metal::MTLStencilOperation::IncrementClamp,
                5 => objc2_metal::MTLStencilOperation::DecrementClamp,
                6 => objc2_metal::MTLStencilOperation::Invert,
                7 => objc2_metal::MTLStencilOperation::IncrementWrap,
                8 => objc2_metal::MTLStencilOperation::DecrementWrap,
                _ => objc2_metal::MTLStencilOperation::Keep,
            };
            for (face, base) in [(0usize, 7u32), (1, 23)] {
                let sd = objc2_metal::MTLStencilDescriptor::new();
                sd.setStencilCompareFunction(compare_func(((bits >> base) & 0xF) as u32));
                sd.setStencilFailureOperation(op((bits >> (base + 4)) & 0xF));
                sd.setDepthFailureOperation(op((bits >> (base + 8)) & 0xF));
                sd.setDepthStencilPassOperation(op((bits >> (base + 12)) & 0xF));
                sd.setReadMask(stencil[1][face] as u32);
                sd.setWriteMask(stencil[2][face] as u32);
                if face == 0 {
                    desc.setFrontFaceStencil(Some(&sd));
                } else {
                    desc.setBackFaceStencil(Some(&sd));
                }
            }
        }
        let d = self.device.newDepthStencilStateWithDescriptor(&desc)?;
        sh.ds_states.insert(key, d.clone());
        Some(d)
    }

    pub(crate) fn default_sampler(
        &self,
        sh: &mut SharedCaches,
    ) -> Option<Retained<ProtocolObject<dyn MTLSamplerState>>> {
        if let Some(s) = &sh.default_sampler {
            return Some(s.clone());
        }
        let sd = MTLSamplerDescriptor::new();
        sd.setMinFilter(MTLSamplerMinMagFilter::Linear);
        sd.setMagFilter(MTLSamplerMinMagFilter::Linear);
        sd.setSAddressMode(MTLSamplerAddressMode::ClampToEdge);
        sd.setTAddressMode(MTLSamplerAddressMode::ClampToEdge);
        sd.setSupportArgumentBuffers(true);
        let s = self.device.newSamplerStateWithDescriptor(&sd)?;
        sh.default_sampler = Some(s.clone());
        Some(s)
    }

    /// Samplers at index 16+ (beyond Metal's per-stage sampler table): an argument buffer of their GPU handles, laid out
    /// as metal/msl_specialize.rs's `spvExtraSamplers` (one MTLResourceID per slot).
    pub(crate) fn extra_sampler_buffer(
        &self,
        cx: &mut EncodeCtx,
        samplers: &[(usize, Retained<ProtocolObject<dyn MTLSamplerState>>)],
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        let n = samplers.iter().map(|(i, _)| i + 1).max()?;
        let mut ids = vec![0u64; n];
        for (i, s) in samplers {
            ids[*i] = unsafe { std::mem::transmute::<_, u64>(s.gpuResourceID()) };
            // MTLResourceID is one u64
        }
        let bytes = unsafe { std::slice::from_raw_parts(ids.as_ptr() as *const u8, ids.len() * 8) };
        cx.arena.alloc(&self.device, bytes)
    }
}
