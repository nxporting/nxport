// What the current render encoder has bound, so repeated bindings are skipped.

use super::*;

/// What the open render encoder has bound, so draws skip Objective-C calls for state that did not change (TotK's
/// consecutive draws share most of it; every draw used to set ~11 fixed-function states and every binding). Reset when
/// an encoder is created. Objects are compared by pointer: the encoder retains what it was given, so a pointer cannot
/// be reused for another object while it is bound.
pub(crate) struct EncCache {
    pub(crate) pipe: usize,
    pub(crate) viewport: [u64; 6],
    pub(crate) scissor: (usize, usize, usize, usize),
    pub(crate) cull: isize,
    pub(crate) clip: isize,
    pub(crate) winding: isize,
    pub(crate) fill: isize,
    pub(crate) bias: [u32; 3],
    pub(crate) ds: usize,
    pub(crate) stencil_ref: u32,
    pub(crate) blend: [u32; 4],
    pub(crate) vbuf: [(usize, usize); 32],
    pub(crate) fbuf: [(usize, usize); 32],
    pub(crate) vtex: [usize; 32],
    pub(crate) ftex: [usize; 32],
    pub(crate) vsamp: [usize; 32],
    pub(crate) fsamp: [usize; 32],
    /// Tiny-pass compute mode: bindings are recorded per stage for the kernels instead of being
    /// set on a render encoder, fixed-function state is ignored.
    pub(crate) rec: Option<TinyRec>,
    /// Argument-buffer draws (--arg-buffers): resource bindings are recorded for the draw's structs; fixed-function
    /// state is set as usual.
    pub(crate) bind_rec: Option<TinyRec>,
    /// Resources made resident for heap reads in this encoder (Op::Use), by pointer.
    pub(crate) heap_used: rustc_hash::FxHashSet<usize>,
}

/// Bindings encode_draw made for a draw that runs as compute: (object, offset / none, index) per stage.
#[derive(Default)]
pub(crate) struct TinyRec {
    pub(crate) vbufs: Vec<(Retained<ProtocolObject<dyn MTLBuffer>>, usize, usize)>,
    pub(crate) fbufs: Vec<(Retained<ProtocolObject<dyn MTLBuffer>>, usize, usize)>,
    pub(crate) vtex: Vec<(Tex, usize)>,
    pub(crate) ftex: Vec<(Tex, usize)>,
    pub(crate) vsamp: Vec<(Option<Retained<ProtocolObject<dyn MTLSamplerState>>>, usize)>,
    pub(crate) fsamp: Vec<(Option<Retained<ProtocolObject<dyn MTLSamplerState>>>, usize)>,
    /// Resources the stages read through descriptor heaps (made resident, not bound).
    pub(crate) uses: Vec<Retained<ProtocolObject<dyn objc2_metal::MTLResource>>>,
}

impl Default for EncCache {
    fn default() -> Self {
        EncCache {
            pipe: 0,
            viewport: [u64::MAX; 6],
            scissor: (usize::MAX, 0, 0, 0),
            cull: -1,
            clip: -1,
            winding: -1,
            fill: -1,
            bias: [u32::MAX; 3],
            ds: 0,
            stencil_ref: u32::MAX,
            blend: [u32::MAX; 4],
            vbuf: [(0, usize::MAX); 32],
            fbuf: [(0, usize::MAX); 32],
            vtex: [usize::MAX; 32],
            ftex: [usize::MAX; 32],
            vsamp: [usize::MAX; 32],
            fsamp: [usize::MAX; 32],
            rec: None,
            bind_rec: None,
            heap_used: Default::default(),
        }
    }
}

impl EncCache {
    /// Makes a resource read through the descriptor heaps resident (once per encoder; recorded for kernels).
    pub(in crate::metal) unsafe fn heap_use(
        &mut self,
        enc: &Enc,
        r: &ProtocolObject<dyn objc2_metal::MTLResource>,
    ) {
        if let Some(rec) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            rec.uses.push(r.retain());
            return;
        }
        if self.heap_used.insert(ptr_of(Some(r))) {
            enc.push(Op::Use(r.retain(), objc2_metal::MTLResourceUsage::Read));
        }
    }
    pub(in crate::metal) unsafe fn pipeline(
        &mut self,
        enc: &Enc,
        p: &ProtocolObject<dyn objc2_metal::MTLRenderPipelineState>,
    ) {
        if self.rec.is_some() {
            return;
        }
        let k = ptr_of(Some(p));
        if self.pipe != k {
            self.pipe = k;
            enc.push(Op::Pipeline(p.retain()));
        }
    }
    pub(in crate::metal) unsafe fn viewport(&mut self, enc: &Enc, v: MTLViewport) {
        if self.rec.is_some() {
            return;
        }
        let k = [
            v.originX.to_bits(),
            v.originY.to_bits(),
            v.width.to_bits(),
            v.height.to_bits(),
            v.znear.to_bits(),
            v.zfar.to_bits(),
        ];
        if self.viewport != k {
            self.viewport = k;
            enc.push(Op::Viewport(v));
        }
    }
    pub(in crate::metal) unsafe fn scissor(&mut self, enc: &Enc, r: MTLScissorRect) {
        if self.rec.is_some() {
            return;
        }
        let k = (r.x, r.y, r.width, r.height);
        if self.scissor != k {
            self.scissor = k;
            enc.push(Op::Scissor(r));
        }
    }
    pub(in crate::metal) unsafe fn cull(&mut self, enc: &Enc, m: MTLCullMode) {
        if self.rec.is_some() {
            return;
        }
        if self.cull != m.0 as isize {
            self.cull = m.0 as isize;
            enc.push(Op::Cull(m));
        }
    }
    pub(in crate::metal) unsafe fn clip(&mut self, enc: &Enc, m: objc2_metal::MTLDepthClipMode) {
        if self.rec.is_some() {
            return;
        }
        if self.clip != m.0 as isize {
            self.clip = m.0 as isize;
            enc.push(Op::Clip(m));
        }
    }
    pub(in crate::metal) unsafe fn winding(&mut self, enc: &Enc, w: MTLWinding) {
        if self.rec.is_some() {
            return;
        }
        if self.winding != w.0 as isize {
            self.winding = w.0 as isize;
            enc.push(Op::Winding(w));
        }
    }
    pub(in crate::metal) unsafe fn fill(&mut self, enc: &Enc, f: MTLTriangleFillMode) {
        if self.rec.is_some() {
            return;
        }
        if self.fill != f.0 as isize {
            self.fill = f.0 as isize;
            enc.push(Op::Fill(f));
        }
    }
    pub(in crate::metal) unsafe fn bias(&mut self, enc: &Enc, units: f32, factor: f32, clamp: f32) {
        if self.rec.is_some() {
            return;
        }
        let k = [units.to_bits(), factor.to_bits(), clamp.to_bits()];
        if self.bias != k {
            self.bias = k;
            enc.push(Op::Bias(units, factor, clamp));
        }
    }
    pub(in crate::metal) unsafe fn stencil_ref(&mut self, enc: &Enc, r: [u8; 2]) {
        if self.rec.is_some() {
            return;
        }
        let k = r[0] as u32 | (r[1] as u32) << 8;
        if self.stencil_ref != k {
            self.stencil_ref = k;
            enc.push(Op::StencilRef(r[0] as u32, r[1] as u32));
        }
    }
    pub(in crate::metal) unsafe fn depth_stencil(
        &mut self,
        enc: &Enc,
        d: &ProtocolObject<dyn MTLDepthStencilState>,
    ) {
        if self.rec.is_some() {
            return;
        }
        let k = ptr_of(Some(d));
        if self.ds != k {
            self.ds = k;
            enc.push(Op::DepthStencil(d.retain()));
        }
    }
    pub(in crate::metal) unsafe fn blend(&mut self, enc: &Enc, c: [f32; 4]) {
        if self.rec.is_some() {
            return;
        }
        let k = c.map(f32::to_bits);
        if self.blend != k {
            self.blend = k;
            enc.push(Op::Blend(c));
        }
    }
    // The same buffer at a new offset only moves the offset (setVertexBufferOffset: no retain here, no resource-list
    // work in the driver): uniform copies of consecutive draws sit in one arena chunk. --no-buf-offset-ops: off.
    pub(in crate::metal) unsafe fn vbuf(
        &mut self,
        enc: &Enc,
        b: &ProtocolObject<dyn MTLBuffer>,
        off: usize,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.vbufs.push((b.retain(), off, i));
            return;
        }
        let k = (ptr_of(Some(b)), off);
        if i < 32 && self.vbuf[i] == k {
            return;
        }
        let same_buf = i < 32 && self.vbuf[i].0 == k.0 && k.0 != 0;
        if i < 32 {
            self.vbuf[i] = k;
        }
        enc.push(if same_buf {
            Op::VBufOff(off, i)
        } else {
            Op::VBuf(b.retain(), off, i)
        });
    }
    pub(in crate::metal) unsafe fn fbuf(
        &mut self,
        enc: &Enc,
        b: &ProtocolObject<dyn MTLBuffer>,
        off: usize,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.fbufs.push((b.retain(), off, i));
            return;
        }
        let k = (ptr_of(Some(b)), off);
        if i < 32 && self.fbuf[i] == k {
            return;
        }
        let same_buf = i < 32 && self.fbuf[i].0 == k.0 && k.0 != 0;
        if i < 32 {
            self.fbuf[i] = k;
        }
        enc.push(if same_buf {
            Op::FBufOff(off, i)
        } else {
            Op::FBuf(b.retain(), off, i)
        });
    }
    pub(in crate::metal) unsafe fn vtex(
        &mut self,
        enc: &Enc,
        t: &ProtocolObject<dyn objc2_metal::MTLTexture>,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.vtex.push((t.retain(), i));
            return;
        }
        let k = ptr_of(Some(t));
        if i < 32 && self.vtex[i] == k {
            return;
        }
        if i < 32 {
            self.vtex[i] = k;
        }
        enc.push(Op::VTex(t.retain(), i));
    }
    pub(in crate::metal) unsafe fn ftex(
        &mut self,
        enc: &Enc,
        t: &ProtocolObject<dyn objc2_metal::MTLTexture>,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.ftex.push((t.retain(), i));
            return;
        }
        let k = ptr_of(Some(t));
        if i < 32 && self.ftex[i] == k {
            return;
        }
        if i < 32 {
            self.ftex[i] = k;
        }
        enc.push(Op::FTex(t.retain(), i));
    }
    pub(in crate::metal) unsafe fn vsamp(
        &mut self,
        enc: &Enc,
        s: Option<&ProtocolObject<dyn MTLSamplerState>>,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.vsamp.push((s.map(|x| x.retain()), i));
            return;
        }
        let k = ptr_of(s);
        if i < 32 && self.vsamp[i] == k {
            return;
        }
        if i < 32 {
            self.vsamp[i] = k;
        }
        enc.push(Op::VSamp(s.map(|x| x.retain()), i));
    }
    pub(in crate::metal) unsafe fn fsamp(
        &mut self,
        enc: &Enc,
        s: Option<&ProtocolObject<dyn MTLSamplerState>>,
        i: usize,
    ) {
        if let Some(r) = self.rec.as_mut().or(self.bind_rec.as_mut()) {
            r.fsamp.push((s.map(|x| x.retain()), i));
            return;
        }
        let k = ptr_of(s);
        if i < 32 && self.fsamp[i] == k {
            return;
        }
        if i < 32 {
            self.fsamp[i] = k;
        }
        enc.push(Op::FSamp(s.map(|x| x.retain()), i));
    }
}

pub(crate) type Enc = super::issuer::Sink;

pub(crate) fn ptr_of<T: ?Sized>(o: Option<&T>) -> usize {
    o.map_or(0, |r| r as *const T as *const () as usize)
}
