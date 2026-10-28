// main+0x688c68: draws one shape of a model drawable (TotK's model renderer). Ported from the game's code; offsets are
// the game's. Float expressions follow the machine code's evaluation order (no fused operations; sums associated as
// the compiler emitted them), so culling decisions match bit for bit.
//
// d = drawable, c = draw context (the game caches the state it last applied there and emits only changes),
// shape = shape index, flags = pass flags (bit 7: occlusion data at c+0x120).

use super::mem::Mem;

/// A helper the port does not implement yet (verify mode counts calls that reach one).
pub(super) type R<T> = Result<T, &'static str>;

// The game's NVN entry point variables (main+): filled through nvnDeviceGetProcAddress.
const PFN_DEVICE_GET_TEXTURE_HANDLE: usize = 0x466d428;
const PFN_BUFFER_GET_ADDRESS: usize = 0x466d818;
const PFN_CALL_COMMANDS: usize = 0x466df60;
const PFN_BIND_UNIFORM_BUFFER: usize = 0x466dfc8;
const PFN_BIND_TEXTURE: usize = 0x466e008;
const PFN_BIND_TEXTURES_DEFERRED: usize = 0x466e018;
const PFN_DRAW_ELEMENTS_INSTANCED: usize = 0x466e0a0;
const PFN_BIND_PROGRAM: usize = 0x466dfb0;
const PFN_BLEND_STATE_SET_DEFAULTS: usize = 0x466dc58;
const PFN_COLOR_STATE_SET_DEFAULTS: usize = 0x466dcd0;
const PFN_COLOR_STATE_SET_BLEND_ENABLE: usize = 0x466dcd8;
const PFN_BLEND_STATE_SET_BLEND_EQUATION: usize = 0x466dc70;
const PFN_BLEND_STATE_SET_BLEND_FUNC: usize = 0x466dc68;
const PFN_BLEND_STATE_SET_BLEND_TARGET: usize = 0x466dc60;
const PFN_BIND_BLEND_STATE: usize = 0x466df70;
const PFN_COLOR_STATE_SET_LOGIC_OP: usize = 0x466dce0;
const PFN_BIND_COLOR_STATE: usize = 0x466df80;
const PFN_SET_BLEND_COLOR: usize = 0x466e190;
const PFN_CHANNEL_MASK_STATE_SET_CHANNEL_MASK: usize = 0x466dd10;
const PFN_BIND_CHANNEL_MASK_STATE: usize = 0x466df78;
const PFN_DEPTH_STENCIL_SET_DEPTH_TEST_ENABLE: usize = 0x466de28;
const PFN_DEPTH_STENCIL_SET_DEPTH_WRITE_ENABLE: usize = 0x466de30;
const PFN_DEPTH_STENCIL_SET_DEPTH_FUNC: usize = 0x466de38;
const PFN_DEPTH_STENCIL_SET_STENCIL_TEST_ENABLE: usize = 0x466de40;
const PFN_DEPTH_STENCIL_SET_STENCIL_FUNC: usize = 0x466de48;
const PFN_DEPTH_STENCIL_SET_STENCIL_OP: usize = 0x466de50;
const PFN_BIND_DEPTH_STENCIL_STATE: usize = 0x466df98;
const PFN_SET_STENCIL_VALUE_MASK: usize = 0x466e178;
const PFN_SET_STENCIL_MASK: usize = 0x466e180;
const PFN_SET_STENCIL_REF: usize = 0x466e188;
const PFN_POLYGON_STATE_SET_CULL_FACE: usize = 0x466dde0;
const PFN_POLYGON_STATE_SET_FRONT_FACE: usize = 0x466dde8;
const PFN_POLYGON_STATE_SET_POLYGON_MODE: usize = 0x466ddf0;
const PFN_POLYGON_STATE_SET_POLYGON_OFFSET_ENABLES: usize = 0x466ddf8;
const PFN_BIND_POLYGON_STATE: usize = 0x466df90;
const PFN_SET_POLYGON_OFFSET_CLAMP: usize = 0x466e1a8;
const PFN_SET_DEPTH_CLAMP: usize = 0x466e1c8;

// Data (main+)
const DEFAULT_MATERIAL_SLOT: usize = 0x4639fb0; // pointer used when the drawable has no material for the pass
const DEVICE_HOLDER: usize = 0x462bbe8; // pointer to the object holding the NVNdevice at +0x30
const NO_VERTEX_STATE: usize = 0x386de44; // three u32 used when the shape has no vertex state
const SHADER_SLOT_OFFSETS: usize = 0x46138a0; // pointer to the offsets of a program set's vertex / geometry / ... shaders
const TICK_HOLDER: usize = 0x462ab50; // pointer to the tick frequency the texture wait's sleep step derives from
const SLEEP: usize = 0x1051cb8; // the sleep the texture wait loops on (symmatch named it agl::RenderTargetColor::invalidateGPUCache)
const PRIMITIVE_TABLE: usize = 0x37ff9b8; // nn::gfx primitive topology -> NVN draw mode
const INDEX_TYPE_TABLE: usize = 0x37ffa0c; // nn::gfx index format -> NVN index type

/// Virtual functions the port met but does not implement: (vtable slot, main+ offset).
pub(super) static UNKNOWN_VFNS: std::sync::Mutex<Vec<(usize, usize)>> = std::sync::Mutex::new(Vec::new());

pub(super) struct Port {
    pub(super) m: Mem,
    pub(super) base: usize,
    /// --native-model-draws on Metal: a render unit's list call and the index ranges drawn after it are recorded as
    /// one Cmd::ModelDraw.
    pub(super) native: bool,
    /// The unit call waiting for its draws: (NVNcommandBuffer, list handle, ranges). Recorded before anything else is.
    pending: std::cell::RefCell<Option<(usize, u64, smallvec::SmallVec<[crate::nvn::cmd::ModelRange; 2]>)>>,
}

impl Port {
    unsafe fn pfn(&self, var: usize) -> usize {
        self.m.r64(self.base + var)
    }

    pub(super) fn new(m: Mem, base: usize, native: bool) -> Self {
        Port { m, base, native, pending: Default::default() }
    }

    /// Records the pending unit draw (anything recorded after it must come after it in the stream).
    pub(super) unsafe fn flush(&self) {
        if let Some((cmd, unit, ranges)) = self.pending.borrow_mut().take() {
            crate::nvn::api::record_typed(
                "nvnCommandBufferModelDraw",
                cmd,
                crate::nvn::cmd::Cmd::ModelDraw(Box::new(crate::nvn::cmd::ModelDraw { unit, ranges })),
            );
        }
    }

    /// A call into the game or an NVN entry point that may record commands.
    unsafe fn call(&self, f: usize, args: &[u64]) -> u64 {
        self.flush();
        crate::guest::call(f, args)
    }

    /// An NVN entry point that records nothing (addresses, handles).
    unsafe fn query(&self, f: usize, args: &[u64]) -> u64 {
        crate::guest::call(f, args)
    }

    /// A game helper the port does not implement: predicting stops there; applying calls the game's function.
    unsafe fn helper(&self, name: &'static str, addr: usize, args: &[u64]) -> R<u64> {
        if self.m.predict() {
            return Err(name);
        }
        Ok(self.call(self.base + addr, args))
    }

    unsafe fn buffer_address(&self, nvn_buffer: usize) -> u64 {
        self.query(self.pfn(PFN_BUFFER_GET_ADDRESS), &[nvn_buffer as u64])
    }

    unsafe fn bind_uniform_buffer(&mut self, cmd: usize, stage: i32, slot: i32, addr: u64, size: u64) {
        self.m.w::<u8>(cmd + 0x10, 1);
        self.call(self.pfn(PFN_BIND_UNIFORM_BUFFER), &[cmd as u64, stage as u64, slot as u64, addr, size]);
    }

    unsafe fn call_commands(&mut self, cmd: usize, handles: usize) {
        self.m.w::<u8>(cmd + 0x10, 1);
        self.call(self.pfn(PFN_CALL_COMMANDS), &[cmd as u64, 1, handles as u64]);
    }

    /// Calls a render unit's material list (its draws follow).
    unsafe fn call_unit(&mut self, cmd: usize, handles: usize) {
        if !self.native {
            return self.call_commands(cmd, handles);
        }
        self.m.w::<u8>(cmd + 0x10, 1);
        self.flush();
        *self.pending.borrow_mut() = Some((cmd, self.m.r64(handles) as u64, Default::default()));
    }

    /// The main+ offset of `obj`'s virtual function at vtable slot `slot`.
    unsafe fn vfn(&self, obj: usize, slot: usize) -> usize {
        self.m.r64(self.m.r64(obj) + slot).wrapping_sub(self.base)
    }

    fn unknown_vfn(&self, slot: usize, f: usize) -> &'static str {
        let mut u = UNKNOWN_VFNS.lock().unwrap();
        if !u.contains(&(slot, f)) {
            u.push((slot, f));
        }
        "uniform block virtual function"
    }

    /// Uniform block object, virtual +0x20: the block alignment.
    unsafe fn block_alignment(&mut self, obj: usize) -> R<i32> {
        match self.vfn(obj, 0x20) {
            0x2a30390 => Ok(0x100),
            f if self.m.predict() => Err(self.unknown_vfn(0x20, f)),
            f => Ok(self.call(self.base + f, &[obj as u64]) as i32),
        }
    }

    /// Uniform block object, virtual +0x48: binds `addr` (`size`) to `slot` of `stage`.
    unsafe fn bind_block(&mut self, obj: usize, cmd: usize, stage: i32, slot: i32, addr: u64, size: u32) -> R<()> {
        match self.vfn(obj, 0x48) {
            // nvnPrivate0 is 0 under the HLE: always through the NVN entry point.
            0x2a303b4 => {
                self.bind_uniform_buffer(cmd, stage, slot, addr, size as u64);
                Ok(())
            }
            f if self.m.predict() => Err(self.unknown_vfn(0x48, f)),
            f => {
                self.call(self.base + f, &[obj as u64, cmd as u64, stage as u64, slot as u64, addr, size as u64]);
                Ok(())
            }
        }
    }

    /// sub_887400: binds element `index` of uniform block object `obj` to the stages / slots packed in `slots`.
    unsafe fn bind_uniform_block(&mut self, obj: usize, w: usize, slots: u32, index: i32) -> R<bool> {
        let shift = self.m.r8(obj + 0x42) >> 4;
        let mut addr = self.m.r64(obj + 0x78) as u64;
        let count = self.m.r16(obj + 0x40) as u32;
        let size = self.m.r64(obj + 0x38);
        let a = self.block_alignment(obj)?;
        let b = self.block_alignment(obj)?;
        if slots == 0xffff_ffff {
            return Ok(false);
        }
        let size32 = self.m.r32(obj + 0x38);
        let cmd = self.m.r64(w + 0xb8);
        let stride = ((size as i32).wrapping_add(a).wrapping_sub(1) as u32 & (b.wrapping_neg() as u32)) as u64;
        addr = addr.wrapping_add(stride.wrapping_mul((index as u32).wrapping_add(count.wrapping_mul(shift as u32)) as i32 as i64 as u64));
        for (stage, sh) in [(0, 0), (1, 8), (2, 16)] {
            if (!(slots >> sh)) & 0xff != 0 {
                self.bind_block(obj, cmd, stage, (slots >> sh) as i8 as i32, addr, size32)?;
            }
        }
        if (slots >> 24) & 0xff != 0xff {
            self.bind_block(obj, cmd, 5, (slots >> 24) as i8 as i32, addr, size32)?;
        }
        Ok(true)
    }

    /// nn::gfx's shader stage bits -> NVNshaderStageBits (main+0x307d8).
    fn nvn_stage_bits(b: u32) -> u32 {
        let mut r = b & 1;
        r |= ((b >> 1) & 1) << 3;
        r |= ((b >> 2) & 1) << 4;
        r |= (b >> 1) & 4;
        r |= (b >> 3) & 2;
        r | (b & 0x20)
    }

    /// nn::gfx::CommandBuffer::SetShader (main+0x74b81c) through the wrapper `w8` (= the context's wrapper + 8).
    unsafe fn set_shader(&mut self, w8: usize, shader: usize, stages: u32) {
        let cmd = self.m.r64(w8 + 0xb0);
        let (program, bits) = if shader == 0 {
            (0, Self::nvn_stage_bits(stages))
        } else if self.m.r8(shader + 1) & 1 == 0 {
            (self.m.r64(shader + 8), self.m.r32(shader + 4))
        } else {
            (self.m.r64(shader + 8), Self::nvn_stage_bits(stages))
        };
        self.call(self.pfn(PFN_BIND_PROGRAM), &[cmd as u64, program as u64, bits as u64]);
    }

    /// sub_764efc: binds the program the shape state's flags select (+0x34 bits 2 / 4 / 3) from its program set.
    unsafe fn bind_shape_program(&mut self, st: usize, w8: usize) {
        let f = self.m.r16(st + 0x34);
        let k = if (f >> 2) & 1 != 0 {
            0
        } else if (f >> 4) & 1 != 0 {
            1
        } else if (f >> 3) & 1 != 0 {
            2
        } else {
            return;
        };
        let off = self.m.r64(self.m.r64(self.base + SHADER_SLOT_OFFSETS) + k * 8);
        let shader = self.m.r64(self.m.r64(self.m.r64(st + 0x20) + off) + 0x68);
        self.set_shader(w8, shader, 0x3f);
    }

    /// sead::GraphicsContext::apply (main+0x74d980): `g` is the context's render state, `w` the command buffer wrapper.
    unsafe fn apply_graphics_context(&mut self, g: usize, w: usize) {
        let cmd = self.m.r64(w + 0xb8) as u64;
        let mut blend = [0u64; 8];
        let mut color = [0u64; 8];
        let mut mask = [0u64; 8];
        let mut ds = [0u64; 8];
        let mut poly = [0u64; 8];
        let (bp, cp, mp, dp, pp) = (
            blend.as_mut_ptr() as u64,
            color.as_mut_ptr() as u64,
            mask.as_mut_ptr() as u64,
            ds.as_mut_ptr() as u64,
            poly.as_mut_ptr() as u64,
        );
        let f = |s: &Self, v: usize| s.pfn(v);
        self.call(f(self, PFN_BLEND_STATE_SET_DEFAULTS), &[bp]);
        self.call(f(self, PFN_COLOR_STATE_SET_DEFAULTS), &[cp]);
        let targets = self.m.r32(g + 4);
        for i in 0..8u32 {
            if (targets >> i) & 1 == 0 {
                continue;
            }
            let t = g + i as usize * 6;
            let b = |o: usize| self.m.r8(t + o) as u64;
            let (a0, a2, a1, a3, e0, e1) = (b(0x28), b(0x2a), b(0x29), b(0x2b), b(0x2c), b(0x2d));
            self.call(f(self, PFN_COLOR_STATE_SET_BLEND_ENABLE), &[cp, i as u64, 1]);
            self.call(f(self, PFN_BLEND_STATE_SET_BLEND_TARGET), &[bp, i as u64]);
            self.call(f(self, PFN_BLEND_STATE_SET_BLEND_FUNC), &[bp, a0, a2, a1, a3]);
            self.call(f(self, PFN_BLEND_STATE_SET_BLEND_EQUATION), &[bp, e0, e1]);
            self.call(f(self, PFN_BIND_BLEND_STATE), &[cmd, bp]);
        }
        self.call(f(self, PFN_COLOR_STATE_SET_LOGIC_OP), &[cp, 3]);
        self.call(f(self, PFN_BIND_COLOR_STATE), &[cmd, cp]);
        let bc = [self.m.f32(g + 8), self.m.f32(g + 12), self.m.f32(g + 16), self.m.f32(g + 20)];
        self.call(f(self, PFN_SET_BLEND_COLOR), &[cmd, bc.as_ptr() as u64]);
        let cm = self.m.r32(g + 0x1c);
        for i in 0..8u32 {
            let v = (cm >> (i * 4)) as u64;
            self.call(f(self, PFN_CHANNEL_MASK_STATE_SET_CHANNEL_MASK), &[mp, i as u64, v & 1, (v >> 1) & 1, (v >> 2) & 1, (v >> 3) & 1]);
        }
        self.call(f(self, PFN_BIND_CHANNEL_MASK_STATE), &[cmd, mp]);
        let b = |o: usize| self.m.r8(g + o) as u64;
        let stencil = b(3);
        self.call(f(self, PFN_DEPTH_STENCIL_SET_DEPTH_TEST_ENABLE), &[dp, b(0)]);
        self.call(f(self, PFN_DEPTH_STENCIL_SET_DEPTH_WRITE_ENABLE), &[dp, b(1)]);
        self.call(f(self, PFN_DEPTH_STENCIL_SET_DEPTH_FUNC), &[dp, b(0x64)]);
        self.call(f(self, PFN_DEPTH_STENCIL_SET_STENCIL_TEST_ENABLE), &[dp, stencil]);
        self.call(f(self, PFN_DEPTH_STENCIL_SET_STENCIL_FUNC), &[dp, 3, b(0x6a)]);
        self.call(f(self, PFN_DEPTH_STENCIL_SET_STENCIL_OP), &[dp, 3, b(0x67), b(0x68), b(0x69)]);
        self.call(f(self, PFN_BIND_DEPTH_STENCIL_STATE), &[cmd, dp]);
        if stencil != 0 {
            self.call(f(self, PFN_SET_STENCIL_VALUE_MASK), &[cmd, 3, self.m.r32(g + 0x24) as u64]);
            self.call(f(self, PFN_SET_STENCIL_MASK), &[cmd, 3, 0xff]);
            self.call(f(self, PFN_SET_STENCIL_REF), &[cmd, 3, self.m.r32(g + 0x20) as u64]);
        }
        self.call(f(self, PFN_POLYGON_STATE_SET_CULL_FACE), &[pp, b(0x65)]);
        self.call(f(self, PFN_POLYGON_STATE_SET_FRONT_FACE), &[pp, 1]);
        self.call(f(self, PFN_POLYGON_STATE_SET_POLYGON_MODE), &[pp, b(0x6b)]);
        self.call(f(self, PFN_POLYGON_STATE_SET_POLYGON_OFFSET_ENABLES), &[pp, ((b(0x6d) & 1).wrapping_neg()) & 7]);
        self.call(f(self, PFN_BIND_POLYGON_STATE), &[cmd, pp]);
        let clamp: unsafe extern "C" fn(u64, f32, f32, f32) = std::mem::transmute(self.pfn(PFN_SET_POLYGON_OFFSET_CLAMP));
        self.flush();
        clamp(cmd, self.m.f32(g + 0x58), self.m.f32(g + 0x5c), self.m.f32(g + 0x60));
        self.call(f(self, PFN_SET_DEPTH_CLAMP), &[cmd, b(0x6e)]);
        let _ = (&mut blend, &mut color, &mut mask, &mut ds, &mut poly);
    }

    /// sub_68b1d8: the GPU address of an nn::gfx buffer (its NVNbuffer at +8).
    unsafe fn gfx_buffer_address(&self, gfx_buffer: usize) -> u64 {
        self.buffer_address(self.m.r64(gfx_buffer + 8))
    }

    /// sub_68b364: an indexed draw through the command buffer wrapper `w` (NVNcommandBuffer* at +0xb0).
    #[allow(clippy::too_many_arguments)]
    unsafe fn draw_indexed(&mut self, w: usize, prim: u32, itype: u32, addr: u64, count: i32, base_vertex: i32, instances: i32, base_instance: i32) {
        let cmd = self.m.r64(w + 0xb0);
        let mode = self.m.r32(self.base + PRIMITIVE_TABLE + prim as usize * 4);
        let ty = self.m.r32(self.base + INDEX_TYPE_TABLE + itype as usize * 4);
        if let Some((pcmd, _, ranges)) = self.pending.borrow_mut().as_mut() {
            if *pcmd == cmd {
                ranges.push(crate::nvn::cmd::ModelRange {
                    mode: mode as i32,
                    index_type: ty as i32,
                    count,
                    index_addr: addr,
                    base_vertex,
                    base_instance,
                    instances,
                });
                return;
            }
        }
        self.call(
            self.pfn(PFN_DRAW_ELEMENTS_INSTANCED),
            &[cmd as u64, mode as u64, ty as u64, count as u64, addr, base_vertex as u64, base_instance as u64, instances as u64],
        );
    }

    /// The index count of sub-mesh range [first, last] of shape record `sh` and its index buffer address.
    unsafe fn range(&self, sh: usize, first: usize, last: usize) -> (u64, i32) {
        let ranges = self.m.r64(sh);
        let end = ranges + last * 8;
        let start = self.m.r32(ranges + first * 8) as u64;
        let shift = self.m.r32(sh + 0x28) & 63;
        let count = self.m.r32(end + 4).wrapping_add(((self.m.r32(end) as u64).wrapping_sub(start) as i64 >> shift) as u32);
        (self.gfx_buffer_address(self.m.r64(sh + 0x10)).wrapping_add(start), count as i32)
    }

    unsafe fn draw_range(&mut self, w: usize, sh: usize, first: usize, last: usize) -> i32 {
        let (addr, count) = self.range(sh, first, last);
        let (prim, itype, bv) = (self.m.r32(sh + 0x24), self.m.r32(sh + 0x28), self.m.i32(sh + 0x30));
        self.draw_indexed(w, prim, itype, addr, count, bv, 1, 1);
        count / 3
    }

    /// sub_758644: draws `count` sub-meshes of shape record `sh` from `first` as `instances` instances.
    unsafe fn draw_shape(&mut self, sh: usize, w: usize, first: u32, count: u32, instances: i32, base_instance: i32) {
        let last = (first as i32 + count as i32 - 1) as isize as usize;
        let (addr, n) = self.range(sh, first as usize, last);
        let (prim, itype, bv) = (self.m.r32(sh + 0x24), self.m.r32(sh + 0x28), self.m.i32(sh + 0x30));
        self.draw_indexed(w, prim, itype, addr, n, bv, instances, base_instance);
    }

    /// sub_68b20c: is the box (min xyz at +0, max xyz at +0x10) inside the frustum `f` (optional AABB at +0 / +0x10
    /// when +0xe4 is set, then +0xe0 planes of 0x20 bytes from +0x20: normal xyzw, distance at +0x10).
    unsafe fn in_frustum(&self, f: usize, b: usize) -> bool {
        let m = &self.m;
        let (min, max) = ([m.f32(b), m.f32(b + 4), m.f32(b + 8)], [m.f32(b + 0x10), m.f32(b + 0x14), m.f32(b + 0x18)]);
        if m.r32(f + 0xe4) != 0 {
            let (fmin, fmax) = ([m.f32(f), m.f32(f + 4), m.f32(f + 8)], [m.f32(f + 0x10), m.f32(f + 0x14), m.f32(f + 0x18)]);
            for k in 0..3 {
                if fmin[k] > max[k] || min[k] > fmax[k] {
                    return false;
                }
            }
        }
        let n = m.i32(f + 0xe0);
        for i in 0..n.max(0) as usize {
            let p = f + 0x20 + i * 0x20;
            let (nx, ny, nz, nw) = (m.f32(p), m.f32(p + 4), m.f32(p + 8), m.f32(p + 12));
            let px = if nx < 0.0 { max[0] } else { min[0] };
            let py = if ny < 0.0 { max[1] } else { min[1] };
            let pz = if nz < 0.0 { max[2] } else { min[2] };
            let s = (nz * pz + nx * px) + (nw * 0.0 + ny * py) + m.f32(p + 0x10);
            if s > 0.0 {
                return false;
            }
        }
        true
    }

    /// The occlusion test of sub_688c68 against the depth pyramid `o` (TotK's hierarchical depth): true when the box is
    /// hidden. The caller has checked that (o[0] & 0x440) == 0x440.
    unsafe fn occluded(&self, o: usize, b: usize) -> bool {
        let m = &self.m;
        let a = |x: f32| if x < 0.0 { -x } else { x };
        let (x0, y0, z0, x1, y1, z1) = (m.f32(b), m.f32(b + 4), m.f32(b + 8), m.f32(b + 0x10), m.f32(b + 0x14), m.f32(b + 0x18));
        let (cx, cy, cz) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5, (z0 + z1) * 0.5);
        let (ex, ey, ez) = ((x1 - x0) * 0.5, (y1 - y0) * 0.5, (z1 - z0) * 0.5);
        let p = |k: usize| m.f32(o + k * 4);
        // Depth plane (o[0x1b..0x1e]).
        let d = p(0x1e) + ((cx * p(0x1b) + cy * p(0x1c)) + cz * p(0x1d));
        let r = (a(ex * p(0x1b)) + a(ey * p(0x1c))) + a(ez * p(0x1d));
        let near = -d - r;
        if p(0x58) > near {
            return false;
        }
        let inv_far = 1.0 / (r - d);
        let inv_near = 1.0 / near;
        // Screen x (o[0x13..0x16]) and y (o[0x17..0x1a]).
        let x = p(0x16) + ((cx * p(0x13) + cy * p(0x14)) + cz * p(0x15));
        let rx = (a(ex * p(0x13)) + a(ey * p(0x14))) + a(ez * p(0x15));
        let y = p(0x1a) + ((cx * p(0x17) + cy * p(0x18)) + cz * p(0x19));
        let ry = (a(ex * p(0x17)) + a(ey * p(0x18))) + a(ez * p(0x19));
        let lt = |u: f32, v: f32| if u < v { u } else { v };
        let gt = |u: f32, v: f32| if u > v { u } else { v };
        let x_min = lt(inv_far * (x - rx), inv_near * (x - rx));
        let x_max = gt(inv_far * (x + rx), inv_near * (x + rx));
        let y_min = lt(inv_far * (y - ry), inv_near * (y - ry));
        let y_max = gt(inv_far * (y + ry), inv_near * (y + ry));
        let w = m.i32(o + 0xc4);
        let h = m.i32(o + 0xc8);
        let (hw, hh) = (w as f32 * 0.5, h as f32 * 0.5);
        let (sx, ox) = (p(0x1f) * hw, hw * (1.0 - p(0x21)));
        let (sy, oy) = (p(0x24) * hh, hh * (1.0 - p(0x25)));
        let clamp0 = |v: i32| v & !(v >> 31);
        let col0 = clamp0((x_min * sx + ox) as i32);
        let col1 = ((x_max * sx + ox) as i32).min(w - 1);
        let row_lo = clamp0((y_min * sy + oy) as i32); // uVar47
        let row0 = clamp0(h + !((y_max * sy + oy) as i32)); // uVar43
        let row1 = h + !row_lo; // iVar52
        let mut bits = (!(m.r32(o) >> 1)) & 1;
        let decide = |bits: u32| bits == 1;
        // Coarse levels (top down): any tile nearer than the box's nearest depth makes it visible; tiles that cannot
        // decide (flags or far depth) send the test one level finer.
        let top = m.i8(o + 0x19a);
        if top >= 0 {
            let tiles = m.r64(o + 0x178);
            let mut lvl = top as u32;
            loop {
                let s = (lvl + 1) & 31;
                let mut r = (row0 as u32) >> s;
                let r_end = row1 >> s;
                let li = if m.r32(o + 0x180) <= lvl { 0 } else { lvl };
                if r_end < r as i32 {
                    return decide(bits);
                }
                let c0 = (col0 as u32) >> s;
                let c_end = col1 >> s;
                if c_end < c0 as i32 {
                    return decide(bits);
                }
                let info = m.r64(o + 0x188) + li as usize * 6;
                let width = c_end - c0 as i32 + 1;
                let pitch = m.r16(info) as u32;
                let mut i = c0.wrapping_add(r.wrapping_mul(pitch)).wrapping_add(m.r16(info + 4) as u32);
                let mut left = width;
                let mut row_start = i;
                loop {
                    let k = if m.r32(o + 0x170) <= i { 0 } else { i };
                    let e = tiles + (k as i32 as isize as usize).wrapping_mul(0xc);
                    if near < m.f32(e) {
                        return false;
                    }
                    if m.r8(e + 8) & 3 != 0 || near <= m.f32(e + 4) {
                        break;
                    }
                    left -= 1;
                    i = i.wrapping_add(1);
                    if left == 0 {
                        let last = r as i32 == r_end;
                        r = r.wrapping_add(1);
                        i = row_start.wrapping_add(pitch);
                        left = width;
                        row_start = i;
                        if last {
                            return decide(bits);
                        }
                    }
                }
                if lvl as i32 <= 0 {
                    break;
                }
                lvl -= 1;
            }
        }
        let mut seen = 0u32;
        if row0 <= row1 && col0 <= col1 {
            let texels = m.r64(o + 0x128);
            let width = col1 - col0 + 1;
            let mut row = row0 as u32;
            let mut row_start = (col0 as u32).wrapping_add((w as u32).wrapping_mul(row0 as u32));
            let mut i = row_start;
            loop {
                let mut left = width;
                loop {
                    let k = if m.r32(o + 0x120) <= i { 0 } else { i };
                    let e = texels.wrapping_add((k as i32 as isize as usize).wrapping_mul(8));
                    if near < m.f32(e) {
                        return false;
                    }
                    left -= 1;
                    i = i.wrapping_add(1);
                    seen |= m.r8(e + 7) as u32;
                    if left == 0 {
                        break;
                    }
                }
                row = row.wrapping_add(1);
                row_start = row_start.wrapping_add(w as u32);
                i = row_start;
                if row == (h - row_lo) as u32 {
                    break;
                }
            }
            seen <<= 1;
        }
        bits |= seen & 2 | seen & 4;
        decide(bits)
    }
}

/// Port of main+0x688c68.
pub(super) unsafe fn draw(p: &mut Port, d: usize, c: usize, shape: u32, flags: u32) -> R<()> {
    let base = p.base;
    if ((p.m.r8(d + 0x1c) >> 4) & 7) as i32 <= shape as i32 {
        return Ok(());
    }
    let w = p.m.r64(c + 0x20); // command buffer wrapper (NVNcommandBuffer* at +0xb8)
    let mut pass = p.m.i8(c + 0x45);
    if pass == -1 {
        pass = p.m.i8(d + p.m.r8(c + 0x31) as usize + 0x20);
    }
    let mat_index_at = (d as isize + pass as isize + 0x2f) as usize;
    let mat_index = p.m.i8(mat_index_at) as i64;
    let mut mat = p.m.r64(base + DEFAULT_MATERIAL_SLOT);
    if mat_index != -1 {
        mat = p.m.r64(p.m.r64(d + 0x50) + mat_index as usize * 8);
    }
    let shape_state_holder = p.m.r64(mat);
    if shape_state_holder == 0 || p.m.r64(shape_state_holder) == 0 {
        return Ok(());
    }
    let ss = p.m.r64(shape_state_holder + 8); // plVar29
    let mut b31 = p.m.r8(d + 0x46);
    let mut u55 = b31 as u32;
    let mut b45 = p.m.r8(c + 0x3d);
    let mut u60 = p.m.r32(c + 0x40);
    let cur_ss = p.m.r64(c + 0x110);
    let pipe_index = p.m.i8(c + 0x48);
    let b18 = if pipe_index == -1 {
        true
    } else {
        let pipes = p.m.r64(d + 0x58);
        if pipes != 0 {
            let pipe = (pipes as isize + pipe_index as isize * 200) as usize;
            if p.m.r8(pipe) != 0 {
                p.helper("nn::gfx::CommandBuffer::SetPipeline", 0x21fe6c8, &[pipe as u64, w as u64])?;
            }
        }
        pipes == 0
    };

    // Render state cache.
    #[derive(Clone, Copy)]
    enum L {
        D2c,
        A9a10,
        D30,
        D34,
        Cdc,
    }
    let mut at = if b45 == b31 {
        b31 = 0;
        L::D2c
    } else {
        p.m.w::<u8>(c + 0x3d, b31);
        if (p.m.r8(c + 0x41) >> 3) & 1 != 0 {
            u55 |= 3;
        }
        u55 |= p.m.r8(c + 0x32) as u32;
        b31 = p.m.r8(c + 0x195);
        let u48 = u55 & 4;
        let set = (u48 != 0) as u32;
        if (b31 & 1) as u32 == u48 >> 2 && set == ((b31 & 2) >> 1) as u32 && set == ((b31 & 4) >> 2) as u32 {
            b31 = 0;
        } else {
            p.m.w::<u8>(c + 0x195, (b31 & 0xfe) | (u48 >> 2) as u8);
            b31 = 1;
        }
        let mut c46 = 0u8;
        match u55 & 3 {
            0 => c46 = 3,
            1 => c46 = if u55 & 8 == 0 { 2 } else { 1 },
            2 => c46 = if u55 & 8 != 0 { 2 } else { 1 },
            _ => {}
        }
        if c46 != p.m.r8(c + 0x18d) {
            b31 = 1;
            p.m.w::<u8>(c + 0x18d, c46);
        }
        let m = (u55 >> 4) & 3;
        let c46 = if m != 1 { 2 } else { 1 };
        let c12 = if m != 2 { c46 } else { 0 };
        if c12 == p.m.r8(c + 0x193) && c12 == p.m.r8(c + 0x194) {
            L::D2c
        } else {
            b31 = 1;
            p.m.w::<u8>(c + 0x193, c12);
            p.m.w::<u8>(c + 0x194, c12);
            if u60 & 1 != 0 {
                L::A9a10
            } else {
                L::D30
            }
        }
    };
    loop {
        match at {
            L::D2c => at = if u60 & 1 == 0 { L::D30 } else { L::A9a10 },
            L::A9a10 => {
                let rs = p.m.r64(d + 0x70);
                if p.m.r32(c + 0xa8) == p.m.r32(rs + 0x170) {
                    at = L::D30;
                    continue;
                }
                p.m.w::<u32>(c + 0xa8, p.m.r32(rs + 0x170));
                let mut u48 = p.m.r8(c + 0x44) as u32;
                u55 = p.m.r32(c);
                b45 = p.m.r8(rs + 0x179) & 7;
                u60 = p.m.r32(c + 0x40);
                if b45 != 5 && b45 != 2 {
                    at = L::D30;
                    continue;
                }
                if p.m.r32(rs + 0xf4) == 0 {
                    u48 = 0;
                    b45 = 0;
                } else {
                    if p.m.f32(c + 0x130) == p.m.f32(rs + 0xf8)
                        && p.m.f32(c + 0x134) == p.m.f32(rs + 0xfc)
                        && p.m.f32(c + 0x138) == p.m.f32(rs + 0x100)
                        && p.m.f32(c + 0x13c) == p.m.f32(rs + 0x104)
                    {
                        b45 = 0;
                    } else {
                        let hi = p.m.r64(rs + 0x100);
                        p.m.w::<usize>(c + 0x130, p.m.r64(rs + 0xf8));
                        p.m.w::<usize>(c + 0x138, hi);
                        b45 = 1;
                    }
                    if (u55 >> 0xf) & 1 == 0 {
                        for k in (0..0x30).step_by(6) {
                            let s = rs + 0x11d + k;
                            let t = c + 0x150 + k;
                            let v = [p.m.r8(s - 5), p.m.r8(s - 4), p.m.r8(s - 3), p.m.r8(s - 2), p.m.r8(s - 1), p.m.r8(s)];
                            if (0..6).any(|j| v[j] != p.m.r8(t + j)) {
                                p.m.w::<u8>(t, v[0]);
                                p.m.w::<u8>(t + 2, v[2]);
                                p.m.w::<u8>(t + 1, v[1]);
                                p.m.w::<u8>(t + 3, v[3]);
                                p.m.w::<u16>(t + 4, p.m.r16(s - 1));
                                b45 = 1;
                            }
                        }
                    }
                }
                if p.m.r32(c + 0x12c) != u48 {
                    b45 = 1;
                    p.m.w::<u32>(c + 0x12c, u48);
                }
                if ((u60 as u16) >> 3) & 1 == 0 || p.m.r8(rs + 0x179) & 7 != 5 {
                    at = L::D34;
                    continue;
                }
                let f1 = p.m.r8(rs + 0xf1);
                if (p.m.r8(c + 0x128) != 0) != (p.m.r8(rs + 0xf0) != 0) || (p.m.r8(c + 0x129) != 0) != (f1 != 0) {
                    b45 = 1;
                    p.m.w::<u8>(c + 0x128, p.m.r8(rs + 0xf0));
                    p.m.w::<u8>(c + 0x129, f1);
                }
                if p.m.r8(c + 0x18c) == p.m.r8(rs + 0x154) {
                    at = L::D34;
                    continue;
                }
                p.m.w::<u8>(c + 0x18c, p.m.r8(rs + 0x154));
                at = L::Cdc;
            }
            L::D30 => {
                b45 = 0;
                at = L::D34;
            }
            L::D34 => {
                if b31 | b45 != 0 {
                    u55 = p.m.r32(c);
                    at = L::Cdc;
                } else {
                    break;
                }
            }
            L::Cdc => {
                if (u55 >> 0x10) & 1 == 0 {
                    p.apply_graphics_context(c + 0x128, p.m.r64(c + 0x20));
                }
                break;
            }
        }
    }

    // Shape state: vertex / uniform slots and the texture table.
    if cur_ss != ss {
        if b18 {
            p.bind_shape_program(p.m.r64(shape_state_holder), p.m.r64(c + 0x20) + 8);
        }
        let mut u10 = p.m.r16(c + 0x40);
        p.m.w::<u32>(c + 0x108, 0);
        p.m.w::<u32>(c + 0x10c, 0);
        p.m.w::<usize>(c + 0x110, ss);
        let mut u11 = 0u16;
        let changed_a;
        let no_vs_bit = (u10 >> 0xe) & 1 == 0;
        if no_vs_bit || {
            u11 = p.m.r16(ss + 0x18);
            (u11 >> 6) & 1 == 0
        } {
            let old = p.m.r32(c + 0x88);
            let new = p.m.r32(p.m.r64(ss) + 8);
            if old != new {
                p.m.w::<u32>(c + 0x88, new);
            }
            changed_a = old != new;
            let (p64, p49, p33);
            if (p.m.r8(ss + 0x18) >> 6) & 1 == 0 {
                p.m.w::<u16>(c + 0x40, u10 & 0xbfff);
                let l = p.m.r64(ss);
                (p64, p49, p33) = (l + 0x10, l + 0x14, l + 0x18);
            } else {
                (p64, p49, p33) = (base + NO_VERTEX_STATE, base + NO_VERTEX_STATE + 4, base + NO_VERTEX_STATE + 8);
                p.m.w::<u16>(c + 0x40, u10 | 0x4000);
            }
            if old != new {
                let a = p.m.r32(p64);
                let cc = p.m.r32(p33);
                let u48 = p.m.r32(p49);
                if a != p.m.r32(c + 0x5c) {
                    p.m.w::<u32>(c + 0x5c, a);
                    if p.m.r8(c + 0x43) & 1 == 0 {
                        p.bind_uniform_block(p.m.r64(c + 0x28), p.m.r64(c + 0x20), a, 0)?;
                    }
                }
                if u48 != p.m.r32(c + 0x60) {
                    p.m.w::<u32>(c + 0x60, u48);
                    if (p.m.r8(c + 0x43) >> 1) & 1 == 0 {
                        let l32 = p.m.r64(p.m.r64(c + 0x10) + 0x590);
                        if l32 != 0 {
                            let l38 = p.m.r64(l32 + 0x180);
                            if l38 != 0 {
                                let size = p.m.r64(l38 + 0x60) as u64;
                                if size != 0 {
                                    let l56 = p.m.r64(c + 0x20);
                                    let mut l34 = 0usize;
                                    if p.m.r64(l38 + 0x40) != 0 && p.m.r16(l38 + 8) & 1 != 0 {
                                        l34 = p.m.r64(l38 + 0x40) + (p.m.r8(l32 + 0x15) & 3) as usize * 0x48;
                                    }
                                    let addr = p.buffer_address(p.m.r64(l34 + 8));
                                    let cmd = p.m.r64(l56 + 0xb8);
                                    let size = size & 0xffff_ffff;
                                    for (stage, k) in [(0, 0), (1, 8), (2, 16)] {
                                        if (!(u48 >> k)) & 0xff != 0 {
                                            p.bind_uniform_buffer(cmd, stage, (u48 >> k) as i8 as i32, addr, size);
                                        }
                                    }
                                    if u48 >> 24 != 0xff {
                                        p.bind_uniform_buffer(cmd, 5, (u48 >> 24) as i8 as i32, addr, size);
                                    }
                                }
                            }
                        }
                    }
                }
                if cc != p.m.r32(c + 0x64) {
                    p.m.w::<u32>(c + 0x64, cc);
                    p.m.w::<u8>(c + 0x42, 0xff);
                }
            }
            u10 = p.m.r16(c + 0x40);
            u11 = p.m.r16(ss + 0x18);
        } else {
            changed_a = false;
        }
        let changed_b = if (u10 as i16) < 0 && (u11 >> 7) & 1 != 0 {
            false
        } else {
            let old = p.m.r32(c + 0x98);
            p.m.w::<u16>(c + 0x40, (u10 & 0x7fff) | ((u11 >> 7) << 0xf));
            let new = p.m.r32(p.m.r64(ss + 8) + 8);
            if old != new {
                p.m.w::<u32>(c + 0x98, new);
            }
            old != new
        };
        if changed_a | changed_b {
            p.m.w::<u16>(c + 0xc6, 0);
        }
        let tt = p.m.r64(ss + 0x10);
        if (p.m.r8(c + 0x43) >> 2) & 1 == 0 && p.m.r32(c + 0x78) != p.m.r32(tt + 8) {
            p.m.w::<u32>(c + 0x78, p.m.r32(tt + 8));
            let mask = p.m.r32(tt + 0x10);
            if mask & 0x3fff_ffff != 0 {
                for k in 0..0x1e_usize {
                    if (mask >> (k & 0x1f)) & 1 == 0 {
                        continue;
                    }
                    let slots = p.m.r32(tt + k * 4 + 0x14);
                    if slots == 0xffff_ffff {
                        continue;
                    }
                    if (p.m.r8(c + 0x41) >> 1) & 1 == 0 {
                        let tex = p.m.r64(p.m.r64(c + 0x18) + k * 8);
                        let ready = tex + 0xe4;
                        if p.m.r32(ready) != 0 {
                            let old = p.m.fetch_or32(ready, 0x8000_0000);
                            if old & 0x7fff_ffff == 0 {
                                p.m.w::<u32>(ready, 0);
                            } else if (old as i32) < 0 {
                                // Another thread is making it ready: wait (the game sleeps in steps), then clear.
                                if p.m.predict() {
                                    return Err("texture ready wait");
                                }
                                let step = (p.m.r64(p.m.r64(base + TICK_HOLDER)) as i64 / 1_000_000) as u64;
                                while p.m.i32(ready) < 0 {
                                    p.call(base + SLEEP, &[step]);
                                }
                                (*(ready as *const std::sync::atomic::AtomicU32)).store(0, std::sync::atomic::Ordering::Release);
                            } else {
                                p.helper("texture ready wait", 0x7ff1fc, &[tex as u64])?;
                                (*(ready as *const std::sync::atomic::AtomicU32)).store(0, std::sync::atomic::Ordering::Release);
                            }
                        }
                        let (tid, sid) = (p.m.i32(tex + 0x58), p.m.r16(tex + 0xea) as i16 as i32);
                        if tid == -1 || sid == -1 {
                            continue;
                        }
                        let dev = p.m.r64(p.m.r64(p.m.r64(base + DEVICE_HOLDER)) + 0x30);
                        let handle = p.query(p.pfn(PFN_DEVICE_GET_TEXTURE_HANDLE), &[dev as u64, tid as u64, sid as u64]);
                        let cmd = p.m.r64(p.m.r64(c + 0x20) + 0xb8);
                        for (stage, sh) in [(0u64, 0), (1, 8), (2, 16), (5, 24)] {
                            if (slots >> sh) & 0xff != 0xff {
                                p.call(p.pfn(PFN_BIND_TEXTURE), &[cmd as u64, stage, ((slots >> sh) as i8) as i64 as u64, handle]);
                            }
                        }
                    } else {
                        let cmd = p.m.r64(p.m.r64(c + 0x20) + 0xb8);
                        let sel = p.m.i8(p.m.r64(c + 0x10) + k * 0x10 + 0x189) as isize;
                        let handles = (p.m.r64(c + 0xe0) as isize + sel * 8) as usize;
                        for (stage, sh) in [(0u64, 0), (1, 8), (2, 16), (5, 24)] {
                            if (slots >> sh) & 0xff != 0xff {
                                p.m.w::<u8>(cmd + 0x10, 1);
                                p.call(
                                    p.pfn(PFN_BIND_TEXTURES_DEFERRED),
                                    &[cmd as u64, stage, ((slots >> sh) as i8) as i64 as u64, 1, handles as u64],
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let mask = p.m.r16(c + 0xc4);
    if mask != 0 {
        let m = mask & !p.m.r16(d + 0x14);
        if m & !p.m.r16(c + 0xc6) != 0 {
            let scene = p.m.r64(c + 0x10);
            p.helper("sub_766cf8", 0x766cf8, &[(scene + 0x470) as u64, w as u64, p.m.r64(c + 0x110) as u64, 0xffff_ffff])?;
        }
        p.m.w::<u16>(c + 0xc6, m);
    }

    // Per-drawable uniform block (the drawable's +0x49 selects a record of the scene's block table).
    let sel = p.m.i8(d + 0x49);
    if sel != p.m.i8(c + 0x42) && p.m.r32(c + 0x64) != 0xffff_ffff {
        let scene = p.m.r64(c + 0x10);
        if p.m.r64(scene + 0x530) != 0 {
            let records = p.m.r32(scene + 0x598);
            let pick = |v: i8| {
                let v = if v == -1 { 0 } else { v as i32 as u32 };
                if records <= v { 0 } else { v }
            };
            let rec = p.m.r64(scene + 0x5a0) + pick(sel) as usize * 0xd0;
            let mut k = (p.m.r8(rec + 0x42) >> 4) as u32;
            if p.m.r32(rec + 0x80) <= k {
                k = 0;
            }
            p.m.w::<usize>(c + 0xe8, p.m.r64(p.m.r64(rec + 0x88) + k as usize * 8));
            let obj = p.m.r64(scene + 0x5a0) + pick(p.m.i8(d + 0x49)) as usize * 0xd0;
            if p.m.fetch_or32(obj + 200, 1) == 0 {
                p.helper("sub_765148 (scene uniform block init)", 0x765148, &[scene as u64, p.m.i8(d + 0x49) as i32 as u32 as u64])?;
            }
            let slots = p.m.r32(c + 0x64);
            let shift = (p.m.r8(obj + 0x42) >> 4) as u64;
            let count = p.m.r16(obj + 0x40) as u64;
            let size = p.m.r64(obj + 0x38);
            let a = p.block_alignment(obj)?;
            let b = p.block_alignment(obj)?;
            if slots != 0xffff_ffff {
                let size32 = p.m.r32(obj + 0x38);
                let stride = ((size as i32).wrapping_add(a).wrapping_sub(1) as u32 & (b.wrapping_neg() as u32)) as u64;
                let addr = (p.m.r64(obj + 0x78) as u64).wrapping_add(count.wrapping_mul(shift).wrapping_mul(stride));
                let cmd = p.m.r64(p.m.r64(c + 0x20) + 0xb8);
                for (stage, sh) in [(0, 0), (1, 8), (2, 16)] {
                    if (!(slots >> sh)) & 0xff != 0 {
                        p.bind_block(obj, cmd, stage, (slots >> sh) as i8 as i32, addr, size32)?;
                    }
                }
                if slots >> 24 != 0xff {
                    p.bind_block(obj, cmd, 5, (slots >> 24) as i8 as i32, addr, size32)?;
                }
            }
            p.m.w::<u8>(c + 0x42, p.m.r8(d + 0x49));
        }
    }

    // Material.
    let u59 = (p.m.r8(p.m.r64(d + 8) + 0x15) & 3) as usize;
    let mat_rec = mat + u59 * 0x58 + 0x58;
    if (p.m.r16(c + 0x40) >> 5) & 1 == 0 || (p.m.r8(mat_rec) >> 1) & 1 == 0 {
        let mut pl58 = p.m.r64(base + DEFAULT_MATERIAL_SLOT);
        if mat_index != -1 {
            pl58 = p.m.r64(p.m.r64(d + 0x50) + mat_index as usize * 8);
        }
        if (p.m.r16(c + 0x40) >> 6) & 1 == 0 || (p.m.r8(d + 0x48) >> 2) & 1 == 0 {
            p.helper("sub_764f4c", 0x764f4c, &[p.m.r64(d + 0x68) as u64, w as u64, pass as i32 as u32 as u64])?;
        } else {
            let rec = p.m.r64(p.m.r64(pl58) + 0x10);
            if p.m.r8(rec + 0x50) & 1 != 0 {
                let cmd = p.m.r64(p.m.r64(c + 0x20) + 0xb8);
                p.call_unit(cmd, rec + 0x48);
            }
        }
        let shader_set = p.m.r64((p.m.r64(d + 0x68) as isize + pass as isize * 0x60 + 0x40) as usize);
        p.helper(
            "sub_7b42f0 (material bind)",
            0x7b42f0,
            &[d as u64, w as u64, p.m.r64(mat) as u64, shader_set as u64, p.m.i8(c + 0x30) as i64 as u64, u59 as u64],
        )?;
    } else {
        if p.m.r8(mat_rec) & 1 != 0 {
            let cmd = p.m.r64(p.m.r64(c + 0x20) + 0xb8);
            p.call_unit(cmd, mat + u59 * 0x58 + 0x50);
        }
        if p.m.r8(c + 0x30) != 0 && (p.m.r8(d + 0x47) >> 2) & 1 != 0 {
            let table = p.m.r64(p.m.r64(d + 8) + 0x178);
            let rec = table + p.m.r16(d + 0x16) as usize * 0x70;
            let blocks = p.m.r64(rec + 0x20);
            let mut k = u59;
            if p.m.r8(rec + 0xd) != 0 {
                k = p.m.r8(rec + 0xf) as usize * p.m.r8(c + 0x30) as usize + u59;
            }
            let block = if blocks != 0 { blocks + k * 0x48 } else { 0 };
            let slot = p.m.r32(p.m.r64(p.m.r64(c + 0x110)) + 0x28);
            p.helper("sub_7b576c", 0x7b576c, &[w as u64, slot as u64, block as u64, 0x100, u59 as u64])?;
        }
    }

    let mut b31 = p.m.r8(d + 0x47);
    if ((flags & b31 as u32) >> 3) & 1 != 0 {
        let r = p.helper("sub_68b3fc", 0x68b3fc, &[p.m.r64(d + 0x80) as u64, w as u64, p.m.i8(c + 0x30) as i64 as u64, 0])?;
        if r & 1 == 0 {
            return Ok(());
        }
        b31 = p.m.r8(d + 0x47);
    }
    if (b31 >> 5) & 1 != 0 {
        p.helper("sub_68b500", 0x68b500, &[d as u64, c as u64])?;
    }
    b31 = p.m.r8(d + 0x47);
    let model = p.m.r64(d + 0x68);
    let f = b31 as u32 & flags;
    let tris: i32;
    if f & 0x51 == 0 {
        let mut instances = p.m.r16(d + 0x4a);
        let s = shape as i32 as isize as usize;
        let list_rec = p.m.r64(model + 0x8c8) + s * 0x268;
        let mut direct = true;
        if instances < 2 {
            instances = 1;
            if p.m.r8(list_rec + 0x50) & 1 != 0 && p.m.i8(c + 0x40) < 0 {
                let cmd = p.m.r64(w + 0xb8);
                p.call_commands(cmd, list_rec + 0x48);
                direct = false;
            }
        }
        if direct {
            let sh = p.m.r64(p.m.r64(model + 8) + 0x18) + s * 0x38;
            p.draw_shape(sh, w + 8, 0, p.m.r16(sh + 0x34) as u32, instances as i32, 0);
        }
        tris = p.m.i32(list_rec + 0x260);
    } else {
        let sf = f as u8 as i8 as i32 as u32;
        tris = if (sf >> 4) & 1 != 0 {
            p.helper("sub_68b474", 0x68b474, &[p.m.r64(d + 0x80) as u64, w as u64, p.m.i8(c + 0x30) as i64 as u64])?;
            p.m.i32(p.m.r64(model + 0x8c8) + shape as i32 as isize as usize * 0x268 + 0x260)
        } else if f & 0x41 != 0 {
            draw_culled(p, d, c, shape, sf, w)?
        } else {
            0
        };
    }
    p.m.w::<u32>(c + 0x34, p.m.r32(c + 0x34).wrapping_add(tris as u32));
    Ok(())
}

/// The culled path: sub-meshes tested against the frustum (and the depth pyramid with flags bit 7), consecutive
/// visible ones drawn as one range; or a precomputed LOD list.
unsafe fn draw_culled(p: &mut Port, d: usize, c: usize, shape: u32, sf: u32, w: usize) -> R<i32> {
    let mesh = p.m.r64(p.m.r64(d + 0x68) + 8);
    let lod = p.m.r64(d + 0x78);
    let pass = p.m.r8(c + 0x30) as usize;
    if (sf >> 1) & 1 != 0 && lod != 0 && p.m.i8(lod + pass * 0x18 + 0x10) as i32 == shape as i32 {
        let list_at = lod + pass * 0x18;
        let mut list = p.m.r64(list_at);
        if p.m.r64(list_at + 8) != 0 {
            let n = p.m.r16(p.m.r64(mesh + 0x18) + shape as i32 as isize as usize * 0x38 + 0x34) as i32;
            let a = p.m.i8(mesh + 0x5b) as i32;
            let v = if a > 1 { n } else { (if n + 1 < 0 { n + 2 } else { n + 1 }) >> 1 } + 1;
            if (v as i64 * 6) as u64 <= p.m.r32(c + 0x1a8) as u64 {
                p.helper("sub_221f100", 0x221f100, &[p.m.r64(c + 0x1a0) as u64, list as u64, p.m.r64(list_at + 8) as u64])?;
                list = p.m.r64(c + 0x1a0);
            }
        }
        let mut n = p.m.r16(list + 2) as u32;
        if n == 0 {
            return Ok(0);
        }
        let mut tris = 0i32;
        let mut e = list + 8;
        loop {
            let sh = p.m.r64(mesh + 0x18) + p.m.r16(e - 4) as usize * 0x38;
            let first = p.m.r16(e - 8) as usize;
            tris = tris.wrapping_add(p.draw_range(w + 8, sh, first, first + n as usize - 1));
            n = p.m.r16(e) as u32;
            e += 6;
            if n == 0 {
                return Ok(tris);
            }
        }
    }
    let shapes = p.m.r64(mesh + 0x18);
    let sh = shapes + shape as i32 as isize as usize * 0x38;
    let count = p.m.r16(sh + 0x34) as i32;
    let mut boxes = p.m.r64(p.m.r64(p.m.r64(d + 8) + 0x178) + p.m.r16(d + 0x16) as usize * 0x70 + 0x40);
    for k in 0..shape as i32 {
        boxes += p.m.r16(shapes + 0x34 + k as usize * 0x38) as usize * 0x20;
    }
    if count == 0 {
        return Ok(0);
    }
    let occl = if (sf as i32) < 0 { p.m.r64(c + 0x120) } else { 0 };
    let lod_min = p.m.r32(c + 0x38);
    let frustum = p.m.r64(p.m.r64(c + 0x10) + 0x18) + p.m.r8(c + 0x46) as usize * 0x430 + 0x2a0;
    let ranges = p.m.r64(sh);
    let bx = |i: i32| boxes + i as isize as usize * 0x20;
    let mut tris = 0i32;
    let mut next = 0i32;
    'outer: loop {
        // The next sub-mesh inside the frustum (and not occluded).
        let i = loop {
            let i = next;
            next = i + 1;
            if p.in_frustum(frustum, bx(i)) {
                if occl == 0 {
                    break i;
                }
                let lod_i = p.m.r32(ranges + i as usize * 8 + 4);
                if lod_i < lod_min || (!p.m.r32(occl)) & 0x440 != 0 || !p.occluded(occl, bx(i)) {
                    break i;
                }
            }
            if next >= count {
                break 'outer;
            }
        };
        // Extend the run while the following sub-meshes stay visible.
        let mut end = count;
        let mut j = next;
        while j < count {
            if !p.in_frustum(frustum, bx(j)) {
                end = j;
                break;
            }
            if occl != 0
                && lod_min <= p.m.r32(ranges + j as usize * 8 + 4)
                && (!p.m.r32(occl)) & 0x440 == 0
                && p.occluded(occl, bx(j))
            {
                end = j;
                break;
            }
            j += 1;
        }
        tris = tris.wrapping_add(p.draw_range(w + 8, sh, i as usize, (end - 1) as usize));
        next = end + 1;
        if next >= count {
            break;
        }
    }
    Ok(tris)
}
