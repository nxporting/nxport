// Synthetic captures: short NVN command sequences recorded through the frontend (nvn::api) as a game would record them,
// with hand-encoded Maxwell shaders (maxwell.rs) that go through sass2spv like the games'. Each scenario writes a capture
// that replay.rs runs; their references live in the repository (crates/cli/tests/gpu/<name>/).
//
// Everything a scenario makes lives in one reserved region at a fixed address, so its capture maps at the same place in
// every run. Nothing runs on a GPU here: the null backend is installed and the commands are only recorded.

use std::path::Path;

use super::maxwell::{self as mx, attr, POSITION};
use crate::nvn::api as nvn;
use crate::nvn::cmd as cmd;
use crate::nvn::state;

pub const NAMES: &[&str] = &["clear", "triangle", "blend", "depth", "scissor", "texture", "render_to_texture", "formats"];

const BASE: usize = 0x6_0000_0000;
const OBJECTS: usize = 0x100_0000;
const POOL: usize = 0x300_0000;
const COMMAND_MEMORY: usize = 0x10_0000;

// NVN values (NVNformat, NVNdrawPrimitive, ...).
const RGBA8: i32 = 0x25;
const RGBA16F: i32 = 0x29;
const RGBA32F: i32 = 0x2E;
const R32F: i32 = 0x0A;
const RG16F: i32 = 0x11;
const RGB10A2: i32 = 0x3D;
const R11G11B10F: i32 = 0x3F;
const DEPTH32F: i32 = 0x34;
const DEPTH24_STENCIL8: i32 = 0x35;
const TRIANGLES: i32 = 4;
const TARGET_2D: i32 = 1;
const TEXTURE_COMPRESSIBLE: i32 = 0x8;
const STAGE_FRAGMENT: i32 = 1;
const STAGES_VERTEX_FRAGMENT: i32 = 0x3;
const BLEND_ONE: i32 = 2;
const BLEND_SRC_ALPHA: i32 = 5;
const BLEND_ONE_MINUS_SRC_ALPHA: i32 = 6;
const BLEND_ADD: i32 = 1;
const DEPTH_LESS: i32 = 2;

struct Env {
    next: usize,
    pool_next: usize,
    device: usize,
    pool: usize,
    cmd: usize,
    texture_pool: usize,
    sampler_pool: usize,
    next_texture_id: i32,
    handles: Vec<u64>,
}

impl Env {
    unsafe fn new() -> Result<Env, String> {
        crate::os::reserve(crate::os::At::Exactly(BASE), OBJECTS + POOL)
            .map_err(|e| format!("scenario memory at {BASE:#x}: {e}"))?;
        crate::os::commit(BASE, OBJECTS + POOL, crate::os::Prot::ReadWrite).map_err(|e| e.to_string())?;
        let mut e = Env {
            next: BASE,
            pool_next: 0,
            device: 0,
            pool: 0,
            cmd: 0,
            texture_pool: 0,
            sampler_pool: 0,
            next_texture_id: 16,
            handles: Vec::new(),
        };
        e.device = e.alloc(0x100);
        let builder = e.alloc(0x100) as *mut nvn::NvnMemoryPoolBuilder;
        nvn::hle_nvnMemoryPoolBuilderSetDefaults(builder);
        nvn::hle_nvnMemoryPoolBuilderSetDevice(builder, e.device);
        nvn::hle_nvnMemoryPoolBuilderSetStorage(builder, BASE + OBJECTS, POOL);
        // CPU_UNCACHED | GPU_CACHED | SHADER_CODE
        nvn::hle_nvnMemoryPoolBuilderSetFlags(builder, 0x2 | 0x20 | 0x40);
        e.pool = e.alloc(0x100);
        if nvn::hle_nvnMemoryPoolInitialize(e.pool as *mut nvn::NvnMemoryPool, builder) == 0 {
            return Err("memory pool".into());
        }
        e.cmd = e.alloc(0x100);
        nvn::hle_nvnCommandBufferInitialize(e.cmd as *mut nvn::NvnCommandBuffer, e.device);
        let (off, _) = e.pool_alloc(COMMAND_MEMORY, 0x1000);
        nvn::hle_nvnCommandBufferAddCommandMemory(
            e.cmd as *mut nvn::NvnCommandBuffer,
            e.pool as *const nvn::NvnMemoryPool,
            off as i64,
            COMMAND_MEMORY,
        );
        let control = e.alloc(0x10000);
        nvn::hle_nvnCommandBufferAddControlMemory(e.cmd as *mut nvn::NvnCommandBuffer, control as *mut _, 0x10000);
        let (off, _) = e.pool_alloc(0x8000, 0x20);
        e.texture_pool = e.alloc(0x100);
        nvn::hle_nvnTexturePoolInitialize(e.texture_pool as *mut nvn::NvnTexturePool, e.pool, off, 1024);
        let (off, _) = e.pool_alloc(0x8000, 0x20);
        e.sampler_pool = e.alloc(0x100);
        nvn::hle_nvnSamplerPoolInitialize(e.sampler_pool as *mut nvn::NvnSamplerPool, e.pool, off, 1024);
        Ok(e)
    }

    /// Zeroed object memory (CPU side: NVN objects, state, builders).
    fn alloc(&mut self, size: usize) -> usize {
        let a = (self.next + 0xFF) & !0xFF;
        assert!(a + size <= BASE + OBJECTS, "scenario object memory");
        self.next = a + size;
        a
    }

    /// Pool memory: (offset in the pool, address).
    fn pool_alloc(&mut self, size: usize, align: usize) -> (usize, usize) {
        let off = (self.pool_next + align - 1) / align * align;
        assert!(off + size <= POOL, "scenario pool memory");
        self.pool_next = off + size;
        (off, BASE + OBJECTS + off)
    }

    /// Pool memory holding `bytes`: its address.
    unsafe fn data(&mut self, bytes: &[u8], align: usize) -> usize {
        let (_, a) = self.pool_alloc(bytes.len(), align);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), a as *mut u8, bytes.len());
        a
    }

    unsafe fn texture(&mut self, w: i32, h: i32, format: i32, flags: i32) -> usize {
        let b = self.alloc(0x100) as *mut nvn::NvnTextureBuilder;
        nvn::hle_nvnTextureBuilderSetDefaults(b);
        nvn::hle_nvnTextureBuilderSetDevice(b, self.device);
        nvn::hle_nvnTextureBuilderSetTarget(b, TARGET_2D);
        nvn::hle_nvnTextureBuilderSetWidth(b, w);
        nvn::hle_nvnTextureBuilderSetHeight(b, h);
        nvn::hle_nvnTextureBuilderSetFormat(b, format);
        nvn::hle_nvnTextureBuilderSetFlags(b, flags);
        let size = nvn::hle_nvnTextureBuilderGetStorageSize(b);
        let align = nvn::hle_nvnTextureBuilderGetStorageAlignment(b).max(0x200);
        let (off, _) = self.pool_alloc(size, align);
        nvn::hle_nvnTextureBuilderSetStorage(b, self.pool, off as i64);
        let t = self.alloc(0x100);
        assert!(nvn::hle_nvnTextureInitialize(t as *mut nvn::NvnTexture, b) != 0, "texture");
        t
    }

    /// A texture registered in the texture pool with a nearest-filtering sampler: its handle.
    unsafe fn sampled(&mut self, tex: usize) -> u64 {
        let id = self.next_texture_id;
        self.next_texture_id += 1;
        nvn::hle_nvnTexturePoolRegisterTexture(
            self.texture_pool as *const nvn::NvnTexturePool,
            id,
            tex as *const nvn::NvnTexture,
            std::ptr::null(),
        );
        let sb = self.alloc(0x100) as *mut nvn::NvnSamplerBuilder;
        nvn::hle_nvnSamplerBuilderSetDefaults(sb);
        nvn::hle_nvnSamplerBuilderSetDevice(sb, self.device);
        nvn::hle_nvnSamplerBuilderSetMinMagFilter(sb, 0, 0);
        nvn::hle_nvnSamplerBuilderSetWrapMode(sb, 7, 7, 7);
        let s = self.alloc(0x100);
        nvn::hle_nvnSamplerInitialize(s as *mut nvn::NvnSampler, sb);
        nvn::hle_nvnSamplerPoolRegisterSampler(self.sampler_pool as *const nvn::NvnSamplerPool, id, s as *const nvn::NvnSampler);
        state::hle_nvnDeviceGetTextureHandle(self.device as *const u8, id, id)
    }

    /// Writes level 0 of a 2D texture from tightly packed texels, in its guest layout.
    unsafe fn fill(&mut self, tex: usize, texels: &[u8]) {
        let t = &*(tex as *const nvn::NvnTexture);
        let p = crate::nvn::layout::TextureParams {
            target: t.target,
            width: t.width,
            height: t.height,
            depth: t.depth,
            levels: t.levels,
            format: t.format,
            flags: t.flags,
            samples: t.samples,
            stride: t.stride,
        };
        crate::nvn::layout::write_texels_2d(t.storage as *mut u8, 0, &p, 0, 0, 0, t.width, t.height, texels.as_ptr());
    }

    unsafe fn program(&mut self, vertex: &[u8], fragment: &[u8]) -> usize {
        let v = self.data(vertex, 0x100);
        let f = self.data(fragment, 0x100);
        let stages = self.alloc(0x40) as *mut u64;
        *stages = v as u64;
        *stages.add(1) = 0;
        *stages.add(2) = f as u64;
        *stages.add(3) = 0;
        let p = self.alloc(0x100);
        nvn::hle_nvnProgramInitialize(p as *mut nvn::NvnProgram, self.device);
        assert!(nvn::hle_nvnProgramSetShaders(p as *mut nvn::NvnProgram, 2, stages) != 0, "program");
        p
    }

    fn begin(&mut self) {
        unsafe { nvn::hle_nvnCommandBufferBeginRecording(self.cmd as *mut nvn::NvnCommandBuffer) };
    }

    fn end(&mut self) {
        let h = unsafe { nvn::hle_nvnCommandBufferEndRecording(self.cmd as *mut nvn::NvnCommandBuffer) };
        self.handles.push(h);
    }

    unsafe fn targets(&mut self, colors: &[usize], depth: usize) {
        let list = self.alloc(8 * colors.len().max(1)) as *mut usize;
        for (i, &c) in colors.iter().enumerate() {
            *list.add(i) = c;
        }
        cmd::hle_nvnCommandBufferSetRenderTargets(self.cmd, colors.len() as i32, list, std::ptr::null(), depth, std::ptr::null());
    }

    unsafe fn clear(&mut self, index: i32, rgba: [f32; 4]) {
        cmd::hle_nvnCommandBufferClearColor(self.cmd, index, rgba.as_ptr(), 0xF);
    }

    unsafe fn viewport(&mut self, x: i32, y: i32, w: i32, h: i32) {
        cmd::hle_nvnCommandBufferSetViewport(self.cmd, x, y, w, h);
        cmd::hle_nvnCommandBufferSetScissor(self.cmd, x, y, w, h);
    }

    /// Default blend, color, channel mask, depth / stencil, polygon and multisample state.
    unsafe fn default_state(&mut self) {
        let blend = self.alloc(0x40) as *mut state::BlendState;
        state::hle_nvnBlendStateSetDefaults(blend);
        cmd::hle_nvnCommandBufferBindBlendState(self.cmd, blend as *const u64);
        self.color_state(false);
        let mask = self.alloc(0x40) as *mut state::ChannelMaskState;
        state::hle_nvnChannelMaskStateSetDefaults(mask);
        cmd::hle_nvnCommandBufferBindChannelMaskState(self.cmd, mask as *const u32);
        self.depth_state(false, DEPTH_LESS);
        let poly = self.alloc(0x40) as *mut state::PolygonState;
        state::hle_nvnPolygonStateSetDefaults(poly);
        cmd::hle_nvnCommandBufferBindPolygonState(self.cmd, poly as *const u32);
        let ms = self.alloc(0x40) as *mut state::MultisampleState;
        state::hle_nvnMultisampleStateSetDefaults(ms);
        cmd::hle_nvnCommandBufferBindMultisampleState(self.cmd, ms as *const u64);
    }

    unsafe fn color_state(&mut self, blend: bool) {
        let s = self.alloc(0x40) as *mut state::ColorState;
        state::hle_nvnColorStateSetDefaults(s);
        state::hle_nvnColorStateSetBlendEnable(s, 0, blend as u8);
        cmd::hle_nvnCommandBufferBindColorState(self.cmd, s as *const u32);
    }

    unsafe fn blend(&mut self, src: i32, dst: i32) {
        let s = self.alloc(0x40) as *mut state::BlendState;
        state::hle_nvnBlendStateSetDefaults(s);
        state::hle_nvnBlendStateSetBlendTarget(s, 0);
        state::hle_nvnBlendStateSetBlendFunc(s, src, dst, src, dst);
        state::hle_nvnBlendStateSetBlendEquation(s, BLEND_ADD, BLEND_ADD);
        cmd::hle_nvnCommandBufferBindBlendState(self.cmd, s as *const u64);
        self.color_state(true);
    }

    unsafe fn depth_state(&mut self, test: bool, func: i32) {
        let s = self.alloc(0x40) as *mut state::DepthStencilState;
        state::hle_nvnDepthStencilStateSetDefaults(s);
        state::hle_nvnDepthStencilStateSetDepthTestEnable(s, test as u8);
        state::hle_nvnDepthStencilStateSetDepthWriteEnable(s, test as u8);
        state::hle_nvnDepthStencilStateSetDepthFunc(s, func);
        cmd::hle_nvnCommandBufferBindDepthStencilState(self.cmd, s as *const u64);
    }

    /// Vertices of two float4 attributes (position, then a varying), interleaved: binds them and the matching
    /// attribute and stream state.
    unsafe fn vertices(&mut self, v: &[[f32; 8]]) {
        let bytes: Vec<u8> = v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let addr = self.data(&bytes, 0x100);
        let attribs = self.alloc(0x40) as *mut state::VertexAttribState;
        for i in 0..2 {
            let a = attribs.add(i);
            state::hle_nvnVertexAttribStateSetDefaults(a);
            state::hle_nvnVertexAttribStateSetFormat(a, RGBA32F, i as isize * 16);
            state::hle_nvnVertexAttribStateSetStreamIndex(a, 0);
        }
        cmd::hle_nvnCommandBufferBindVertexAttribState(self.cmd, 2, attribs as *const u32);
        let stream = self.alloc(0x40) as *mut state::VertexStreamState;
        state::hle_nvnVertexStreamStateSetDefaults(stream);
        state::hle_nvnVertexStreamStateSetStride(stream, 32);
        cmd::hle_nvnCommandBufferBindVertexStreamState(self.cmd, 1, stream as *const u64);
        cmd::hle_nvnCommandBufferBindVertexBuffer(self.cmd, 0, addr as u64, bytes.len());
    }

    unsafe fn draw(&mut self, program: usize, v: &[[f32; 8]]) {
        cmd::hle_nvnCommandBufferBindProgram(self.cmd, program, STAGES_VERTEX_FRAGMENT);
        self.vertices(v);
        cmd::hle_nvnCommandBufferDrawArrays(self.cmd, TRIANGLES, 0, v.len() as i32);
    }
}

/// Position (x, y, z, 1) and a varying.
fn vtx(x: f32, y: f32, z: f32, v: [f32; 4]) -> [f32; 8] {
    [x, y, z, 1.0, v[0], v[1], v[2], v[3]]
}

/// Two triangles covering [x0, x1] x [y0, y1] in clip space at depth z, with one varying for every corner.
fn quad(x0: f32, y0: f32, x1: f32, y1: f32, z: f32, v: [f32; 4]) -> Vec<[f32; 8]> {
    vec![
        vtx(x0, y0, z, v),
        vtx(x1, y0, z, v),
        vtx(x0, y1, z, v),
        vtx(x1, y0, z, v),
        vtx(x1, y1, z, v),
        vtx(x0, y1, z, v),
    ]
}

/// A quad whose varying is its texture coordinate (0..1 over the quad).
fn uv_quad(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 8]> {
    let c = |x: f32, y: f32, u: f32, v: f32| vtx(x, y, 0.0, [u, v, 0.0, 1.0]);
    vec![c(x0, y0, 0.0, 0.0), c(x1, y0, 1.0, 0.0), c(x0, y1, 0.0, 1.0), c(x1, y0, 1.0, 0.0), c(x1, y1, 1.0, 1.0), c(x0, y1, 0.0, 1.0)]
}

/// Vertex shader: position from attribute 0, varying 0 from attribute 1.
fn passthrough_vs() -> Vec<u8> {
    mx::vertex(&[mx::ald(0, attr(0), 4), mx::ald(4, attr(1), 4), mx::ast(POSITION, 0, 4), mx::ast(attr(0), 4, 4), mx::exit()])
}

/// Fragment shader: color target 0 = varying 0.
fn varying_fs() -> Vec<u8> {
    mx::fragment(
        &[mx::ipa(0, attr(0)), mx::ipa(1, attr(0) + 4), mx::ipa(2, attr(0) + 8), mx::ipa(3, attr(0) + 12), mx::exit()],
        &[(0, mx::IMAP_PERSPECTIVE)],
        0xF,
    )
}

/// Fragment shader: color target 0 = the texture bound at fragment slot 0, sampled at varying 0's xy.
fn texture_fs() -> Vec<u8> {
    mx::fragment(&[mx::ipa(4, attr(0)), mx::ipa(5, attr(0) + 4), mx::texs_2d(0, 2, 4, 5, 8), mx::exit()], &[(0, mx::IMAP_PERSPECTIVE)], 0xF)
}

unsafe fn build(name: &str, e: &mut Env) -> Result<(usize, Vec<(usize, usize)>), String> {
    let size = 64;
    Ok(match name {
        // Clears of two targets, of two formats.
        "clear" => {
            let a = e.texture(size, size, RGBA8, TEXTURE_COMPRESSIBLE);
            let b = e.texture(size, size, RGBA16F, TEXTURE_COMPRESSIBLE);
            e.begin();
            e.targets(&[a], 0);
            e.clear(0, [0.25, 0.5, 0.75, 1.0]);
            e.targets(&[b], 0);
            e.clear(0, [2.0, -1.0, 0.5, 0.25]);
            e.end();
            (a, vec![])
        }
        // One triangle with a color per vertex: rasterization and interpolation.
        "triangle" => {
            let t = e.texture(size, size, RGBA8, TEXTURE_COMPRESSIBLE);
            let p = e.program(&passthrough_vs(), &varying_fs());
            e.begin();
            e.targets(&[t], 0);
            e.viewport(0, 0, size, size);
            e.default_state();
            e.clear(0, [0.0, 0.0, 0.0, 1.0]);
            e.draw(
                p,
                &[
                    vtx(-0.9, -0.8, 0.0, [1.0, 0.0, 0.0, 1.0]),
                    vtx(0.85, -0.6, 0.0, [0.0, 1.0, 0.0, 1.0]),
                    vtx(-0.2, 0.9, 0.0, [0.0, 0.0, 1.0, 1.0]),
                ],
            );
            e.end();
            (t, vec![])
        }
        // Alpha blending, then additive blending, over a cleared background.
        "blend" => {
            let t = e.texture(size, size, RGBA8, TEXTURE_COMPRESSIBLE);
            let p = e.program(&passthrough_vs(), &varying_fs());
            e.begin();
            e.targets(&[t], 0);
            e.viewport(0, 0, size, size);
            e.default_state();
            e.clear(0, [0.0, 0.0, 0.5, 1.0]);
            e.blend(BLEND_SRC_ALPHA, BLEND_ONE_MINUS_SRC_ALPHA);
            e.draw(p, &quad(-0.8, -0.8, 0.4, 0.4, 0.0, [1.0, 0.5, 0.0, 0.5]));
            e.blend(BLEND_ONE, BLEND_ONE);
            e.draw(p, &quad(-0.4, -0.4, 0.8, 0.8, 0.0, [0.25, 0.25, 0.25, 1.0]));
            e.end();
            (t, vec![])
        }
        // Depth test LESS with two depth formats: the far quad drawn after the near one loses where they overlap.
        "depth" => {
            let p = e.program(&passthrough_vs(), &varying_fs());
            let mut first = 0;
            for (i, fmt) in [DEPTH32F, DEPTH24_STENCIL8].into_iter().enumerate() {
                let t = e.texture(size, size, RGBA8, TEXTURE_COMPRESSIBLE);
                let d = e.texture(size, size, fmt, TEXTURE_COMPRESSIBLE);
                if i == 0 {
                    first = t;
                }
                e.begin();
                e.targets(&[t], d);
                e.viewport(0, 0, size, size);
                e.default_state();
                e.clear(0, [0.0, 0.0, 0.0, 1.0]);
                cmd::hle_nvnCommandBufferClearDepthStencil(e.cmd, 1.0, 1, 0, 0xFF);
                e.depth_state(true, DEPTH_LESS);
                e.draw(p, &quad(-0.7, -0.7, 0.3, 0.3, 0.2, [1.0, 0.0, 0.0, 1.0]));
                e.draw(p, &quad(-0.3, -0.3, 0.7, 0.7, 0.6, [0.0, 1.0, 0.0, 1.0]));
                e.draw(p, &quad(-0.5, 0.1, 0.5, 0.9, -0.2, [0.0, 0.0, 1.0, 1.0]));
                e.end();
            }
            (first, vec![])
        }
        // A viewport that is not the whole target and a scissor inside it.
        "scissor" => {
            let t = e.texture(size, size, RGBA8, TEXTURE_COMPRESSIBLE);
            let p = e.program(&passthrough_vs(), &varying_fs());
            e.begin();
            e.targets(&[t], 0);
            e.default_state();
            e.viewport(0, 0, size, size);
            e.clear(0, [0.1, 0.1, 0.1, 1.0]);
            cmd::hle_nvnCommandBufferSetViewport(e.cmd, 8, 16, 40, 32);
            cmd::hle_nvnCommandBufferSetScissor(e.cmd, 12, 4, 24, 52);
            e.draw(p, &quad(-1.0, -1.0, 1.0, 1.0, 0.0, [1.0, 1.0, 0.0, 1.0]));
            e.end();
            (t, vec![])
        }
        // A texture sampled with nearest filtering.
        "texture" => {
            let src = e.texture(8, 8, RGBA8, 0);
            let texels: Vec<u8> = (0..64u32)
                .flat_map(|i| [(i % 8 * 32) as u8, (i / 8 * 32) as u8, if (i % 8 + i / 8) % 2 == 0 { 255 } else { 0 }, 255])
                .collect();
            e.fill(src, &texels);
            let handle = e.sampled(src);
            let t = e.texture(size, size, RGBA8, TEXTURE_COMPRESSIBLE);
            let p = e.program(&passthrough_vs(), &texture_fs());
            e.begin();
            e.targets(&[t], 0);
            e.viewport(0, 0, size, size);
            e.default_state();
            e.clear(0, [0.0, 0.0, 0.0, 1.0]);
            cmd::hle_nvnCommandBufferBindTexture(e.cmd, STAGE_FRAGMENT, 0, handle);
            e.draw(p, &uv_quad(-0.75, -0.75, 0.75, 0.75));
            e.end();
            (t, vec![])
        }
        // Rendering into a texture, then sampling it in a second pass.
        "render_to_texture" => {
            let mid = e.texture(32, 32, RGBA8, TEXTURE_COMPRESSIBLE);
            let handle = e.sampled(mid);
            let t = e.texture(size, size, RGBA8, TEXTURE_COMPRESSIBLE);
            let color = e.program(&passthrough_vs(), &varying_fs());
            let tex = e.program(&passthrough_vs(), &texture_fs());
            e.begin();
            e.targets(&[mid], 0);
            e.viewport(0, 0, 32, 32);
            e.default_state();
            e.clear(0, [0.2, 0.0, 0.2, 1.0]);
            e.draw(
                color,
                &[
                    vtx(-1.0, -1.0, 0.0, [1.0, 1.0, 0.0, 1.0]),
                    vtx(1.0, -1.0, 0.0, [0.0, 1.0, 1.0, 1.0]),
                    vtx(0.0, 1.0, 0.0, [1.0, 0.0, 1.0, 1.0]),
                ],
            );
            e.targets(&[t], 0);
            e.viewport(0, 0, size, size);
            e.clear(0, [0.0, 0.0, 0.0, 1.0]);
            cmd::hle_typed_cb_Barrier(e.cmd, 0x1 | 0x10);
            cmd::hle_nvnCommandBufferBindTexture(e.cmd, STAGE_FRAGMENT, 0, handle);
            e.draw(tex, &uv_quad(-1.0, -1.0, 0.5, 0.5));
            e.end();
            (t, vec![])
        }
        // The same draw into color targets of several formats (values beyond [0, 1] for the float ones).
        "formats" => {
            let p = e.program(&passthrough_vs(), &varying_fs());
            let mut first = 0;
            for fmt in [RGBA16F, R32F, RG16F, RGB10A2, R11G11B10F, RGBA32F] {
                let t = e.texture(size, size, fmt, TEXTURE_COMPRESSIBLE);
                if first == 0 {
                    first = t;
                }
                e.begin();
                e.targets(&[t], 0);
                e.viewport(0, 0, size, size);
                e.default_state();
                e.clear(0, [0.5, 0.25, 0.125, 1.0]);
                e.draw(
                    p,
                    &[
                        vtx(-0.9, -0.9, 0.0, [3.0, 0.0, 0.25, 1.0]),
                        vtx(0.9, -0.9, 0.0, [0.0, 1.5, 0.0, 0.5]),
                        vtx(0.0, 0.9, 0.0, [0.125, 0.0, 6.0, 0.0]),
                    ],
                );
                e.end();
            }
            (first, vec![])
        }
        _ => return Err(format!("no scenario {name} (scenarios: {})", NAMES.join(", "))),
    })
}

/// Records scenario `name` and writes it as a capture to `dir`. Once per process (it reserves its memory at a fixed
/// address); the null backend and settings must be installed.
pub fn write(name: &str, dir: &Path) -> Result<(), String> {
    let mut e = unsafe { Env::new()? };
    let (present, _) = unsafe { build(name, &mut e)? };
    let extra = [(BASE as u64, (BASE + OBJECTS) as u64)];
    let submits: Vec<Vec<u64>> = e.handles.iter().map(|&h| vec![h]).collect();
    let summary = super::record::write_capture(dir, 1, &submits, present, [0; 4], &extra)?;
    println!("[gpu-scenario] {name} written to {}: {summary}", dir.display());
    Ok(())
}
