// Decoded NVN command-buffer commands.
//
// NVN command-buffer methods are recorded when the game calls them but executed only when a
// command handle is submitted, possibly much later and from another thread. Arguments that are
// pointers marked NVN_NOESCAPE (render target lists, clear colours, state objects, inline
// uniform data, ...) are only valid during the call, and float arguments arrive in vector
// registers, so each command the executor understands is decoded here at record time into an
// owned `Cmd`. Everything else is still recorded uninterpreted by the generic recorder.
//
// GPU addresses (NVNbufferAddress) are host pointers in this implementation (see
// nvnMemoryPoolGetBufferAddress), so buffers are addressed by plain usize.

use std::os::raw::c_int;

use crate::nvn::api::record_typed;

/// A render target reference: NVNtexture address plus the view's level/layer (0 if no view).
#[derive(Clone, Copy, Debug, Default)]
pub struct RtRef {
    pub texture: usize,
    pub level: u16,
    pub layer: u16,
    /// The view's format (0 = the texture's own) and target (NVNtextureTarget; 0 = the texture's).
    pub view_format: u16,
    pub view_target: u8,
}

/// A texture subresource reference for copies: NVNtexture address plus the view's first level / layer.
#[derive(Clone, Copy, Debug, Default)]
pub struct TexRef {
    pub texture: usize,
    pub level: u16,
    pub layer: u16,
}

/// NVNcopyRegion: offsets and sizes in texels (layers for array targets via z / depth).
#[derive(Clone, Copy, Debug, Default)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub w: i32,
    pub h: i32,
    pub d: i32,
}

/// Bound vertex attribute states, inline (Cmd stays 72 bytes): the executor used to copy them out of a separate heap
/// allocation for every bind, a cache miss each.
#[derive(Clone, Copy, Default, Hash, PartialEq, Eq)]
pub struct AttribState {
    pub n: u8,
    pub v: [u32; 16],
}

impl AttribState {
    pub const MAX: usize = 16;
    pub fn as_slice(&self) -> &[u32] {
        &self.v[..self.n as usize]
    }
    pub fn get(&self, i: usize) -> Option<&u32> {
        self.as_slice().get(i)
    }
}

impl std::fmt::Debug for AttribState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:08x?}", self.as_slice())
    }
}

#[derive(Clone, Debug)]
pub enum Cmd {
    SetRenderTargets {
        colors: Vec<RtRef>,
        depth: Option<RtRef>,
    },
    ClearColor {
        index: i32,
        rgba: [f32; 4],
        mask: i32,
    },
    ClearDepthStencil {
        depth: f32,
        depth_mask: bool,
        stencil: i32,
        stencil_mask: i32,
    },
    SetViewport {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    SetScissor {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    },
    SetDepthRange {
        near: f32,
        far: f32,
    },
    /// nvnCommandBufferSetStencilRef / SetStencilValueMask / SetStencilMask: kind 0 ref, 1 read mask, 2 write mask.
    SetStencilValue {
        kind: u8,
        faces: i32,
        value: i32,
    },
    /// nvnCommandBufferSetDepthBounds: discard fragments whose stored depth is outside [min, max].
    SetDepthBounds {
        enable: bool,
        min: f32,
        max: f32,
    },
    /// Clamp depth instead of clipping at the near/far planes (shadow passes keep casters in front of the near plane).
    SetDepthClamp(bool),
    /// nvnCommandBufferSetPrimitiveRestart(enable, index).
    SetPrimitiveRestart {
        enable: bool,
        index: u32,
    },
    /// nvnCommandBufferSetPolygonOffsetClamp (applied where the polygon state enables offset).
    SetPolygonOffset {
        factor: f32,
        units: f32,
        clamp: f32,
    },
    SetBlendColor([f32; 4]),
    BindProgram {
        program: usize,
        stages: i32,
    },
    BindVertexBuffer {
        index: i32,
        addr: u64,
        size: usize,
    },
    BindVertexAttribState(AttribState),
    BindVertexStreamState(Vec<u64>),
    BindBlendState(u64),
    BindColorState(u32),
    BindChannelMaskState(u32),
    BindDepthStencilState(u64),
    BindPolygonState(u32),
    /// NVNmultisampleState bits (nvn/state.rs: enable[0] alpha-to-coverage[1] a2c dither[2] samples[3..8) ...).
    BindMultisampleState(u64),
    /// nvnCommandBufferBarrier: NVNbarrierBits (ORDER_* 0x1/0x2/0x4/0x8, INVALIDATE_TEXTURE 0x10, ...).
    Barrier(u32),
    BindTexture {
        stage: i32,
        index: i32,
        handle: u64,
    },
    /// nvnCommandBufferBindImage(s): storage image for (stage, image unit).
    BindImage {
        stage: i32,
        index: i32,
        handle: u64,
    },
    /// nvnCommandBufferBindTexturesDeferred: the handle array (guest address) is read when the list executes.
    BindTexturesDeferred {
        stage: i32,
        first: i32,
        count: i32,
        handles: u64,
    },
    /// nvnCommandBufferSetRenderEnable.
    SetRenderEnable(bool),
    /// nvnCommandBufferSetRenderEnableConditional: render iff the u64s at `counters` and `counters + 16` are equal
    /// (mode 0) or differ (mode 1), evaluated when the list executes.
    SetRenderEnableConditional {
        mode: i32,
        counters: u64,
    },
    /// nvnCommandBufferSetPointSize: rasterized point size when the vertex shader does not write one.
    SetPointSize(f32),
    /// nvnCommandBufferDispatchCompute with the bound compute program.
    DispatchCompute {
        x: i32,
        y: i32,
        z: i32,
    },
    BindUniformBuffer {
        stage: i32,
        index: i32,
        addr: u64,
        size: usize,
    },
    /// nvnCommandBufferBindStorageBuffer(s): shader storage buffer `index` of `stage`.
    BindStorageBuffer {
        stage: i32,
        index: i32,
        addr: u64,
        size: usize,
    },
    UpdateUniformBuffer {
        addr: u64,
        offset: isize,
        data: Vec<u8>,
    },
    DrawArrays {
        mode: i32,
        first: i32,
        count: i32,
        caller: u32,
    },
    DrawElements {
        mode: i32,
        index_type: i32,
        count: i32,
        index_addr: u64,
        base_vertex: i32,
        caller: u32,
    },
    CallCommands(smallvec::SmallVec<[u64; 2]>),
    /// Row / image strides are the command buffer's copy strides at record time (0 = densely packed).
    DrawArraysInstanced {
        mode: i32,
        first: i32,
        count: i32,
        base_instance: i32,
        instances: i32,
        caller: u32,
    },
    DrawElementsInstanced {
        mode: i32,
        index_type: i32,
        count: i32,
        index_addr: u64,
        base_vertex: i32,
        base_instance: i32,
        instances: i32,
        caller: u32,
    },
    /// nvnCommandBufferReportCounter: writes an NVNcounterData {counter, timestamp} to `addr`.
    ReportCounter {
        counter: i32,
        addr: u64,
    },
    /// nvnCommandBufferReportValue: same, with an application-provided counter value.
    ReportValue {
        value: u32,
        addr: u64,
    },
    /// nvnCommandBufferResetCounter: zero a counter (only SAMPLES_PASSED is tracked, see metal::Occlusion).
    ResetCounter {
        counter: i32,
    },
    CopyBufferToTexture {
        src: u64,
        dst: TexRef,
        region: Region,
        row_stride: isize,
        image_stride: isize,
    },
    CopyTextureToBuffer {
        src: TexRef,
        region: Region,
        dst: u64,
        row_stride: isize,
        image_stride: isize,
    },
    CopyTextureToTexture(Box<CopyTextureToTextureParams>),
    CopyBufferToBuffer {
        src: u64,
        dst: u64,
        size: usize,
    },
    ClearBuffer {
        dst: u64,
        size: usize,
        value: u32,
    },
    /// `color` holds the raw 32-bit words of the float / int / uint clear value.
    ClearTexture(Box<ClearTextureParams>),
}

#[derive(Clone, Debug)]
pub struct CopyTextureToTextureParams {
    pub src: TexRef,
    pub src_region: Region,
    pub dst: TexRef,
    pub dst_region: Region,
}

#[derive(Clone, Debug)]
pub struct ClearTextureParams {
    pub dst: TexRef,
    pub region: Region,
    pub color: [u32; 4],
    pub mask: i32,
}

/// The view struct (our 0x10-byte NvnTextureView): min_level u16, num_levels u16, min_layer u16, ...
unsafe fn view_level_layer(view: *const u8) -> (u16, u16) {
    if view.is_null() {
        (0, 0)
    } else {
        (*(view as *const u16), *(view.add(4) as *const u16))
    }
}

/// NVNtextureView format (offset 8) and target (offset 10); 0 when there is no view.
unsafe fn view_format_target(view: *const u8) -> (u16, u8) {
    if view.is_null() {
        (0, 0)
    } else {
        (*(view.add(8) as *const u16), *view.add(10))
    }
}

unsafe fn rt_list(tex: *const usize, views: *const usize, n: usize) -> Vec<RtRef> {
    (0..n)
        .map(|i| {
            let t = if tex.is_null() { 0 } else { *tex.add(i) };
            crate::nvn::api::capture_texture(t);
            let v = if views.is_null() { 0 } else { *views.add(i) };
            let (level, layer) = view_level_layer(v as *const u8);
            let (view_format, view_target) = view_format_target(v as *const u8);
            RtRef {
                texture: t,
                level,
                layer,
                view_format,
                view_target,
            }
        })
        .collect()
}

macro_rules! rec {
    ($name:literal, $cmd:expr, $val:expr) => {
        record_typed($name, $cmd as usize, $val)
    };
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetRenderTargets(
    cmd: usize,
    num: c_int,
    colors: *const usize,
    color_views: *const usize,
    depth: usize,
    depth_view: *const u8,
) {
    let lr: usize;
    std::arch::asm!("mov {}, x30", out(reg) lr, options(nomem, nostack));
    // --log-rt-callers: each distinct (caller, target count, which targets are null, depth null) once, with the
    // caller as a main-module offset (UltraCam diagnosis: it replaces the game's render targets).
    // --log-rt-caller-size=WxH: the guest call chain of the first 12 binds whose color target 0 has that size.
    if let (Some(size), false) = (
        crate::settings::get().log.log_rt_caller_size,
        colors.is_null() || num < 1,
    ) {
        let t0 = *colors;
        if t0 > 0x10000 {
            let t = &*(t0 as *const crate::nvn::api::NvnTexture);
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if size.is(t.width, t.height)
                && N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 12
            {
                let base = crate::cpu::crash_handler::get_guest_base();
                let mut fp: usize;
                std::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack));
                let mut chain = vec![format!("{:#x}", lr.wrapping_sub(base))];
                for _ in 0..16 {
                    if fp < 0x1000 || fp & 7 != 0 {
                        break;
                    }
                    let next = *(fp as *const usize);
                    chain.push(format!(
                        "{:#x}",
                        (*((fp + 8) as *const usize)).wrapping_sub(base)
                    ));
                    if next <= fp {
                        break;
                    }
                    fp = next;
                }
                let name = std::thread::current().name().unwrap_or("?").to_string();
                println!(
                    "[rt-caller-size] thread {} texture {:#x} storage {:#x} chain (base+): {}",
                    name,
                    t0,
                    t.storage,
                    chain.join(" <- ")
                );
            }
        }
    }
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *LOG.get_or_init(|| crate::settings::get().log.log_rt_callers) {
        static SEEN: std::sync::Mutex<Option<std::collections::HashSet<(usize, i32, u64, bool)>>> =
            std::sync::Mutex::new(None);
        let n = num.clamp(0, 8) as usize;
        let list: Vec<usize> = if colors.is_null() {
            Vec::new()
        } else {
            (0..n).map(|i| *colors.add(i)).collect()
        };
        let nulls = list
            .iter()
            .enumerate()
            .fold(0u64, |m, (i, &c)| if c == 0 { m | 1 << i } else { m });
        let key = (lr, num, nulls, depth == 0);
        let mut g = SEEN.lock().unwrap();
        if g.get_or_insert_with(Default::default).insert(key) {
            let base = crate::cpu::crash_handler::get_guest_base();
            println!(
                "[rt-caller] lr {:#x} (base+{:#x}) num {} colors@{:#x} {:x?} depth {:#x}",
                lr,
                lr.wrapping_sub(base),
                num,
                colors as usize,
                list,
                depth
            );
            // Guest call chain from the frame-pointer records (ours first, then the guest's).
            let mut fp: usize;
            std::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack));
            let mut chain = Vec::new();
            for _ in 0..14 {
                if fp < 0x1000 || fp & 7 != 0 {
                    break;
                }
                let next = *(fp as *const usize);
                let ret = *((fp + 8) as *const usize);
                chain.push(format!("{:#x}", ret.wrapping_sub(base)));
                if next <= fp {
                    break;
                }
                fp = next;
            }
            println!("[rt-caller]   chain (base+): {}", chain.join(" <- "));
        }
    }
    let (dl, dy) = view_level_layer(depth_view);
    crate::nvn::api::capture_texture(depth);
    rec!(
        "nvnCommandBufferSetRenderTargets",
        cmd,
        Cmd::SetRenderTargets {
            colors: rt_list(colors, color_views, num.max(0) as usize),
            depth: if depth != 0 {
                let (view_format, view_target) = view_format_target(depth_view);
                Some(RtRef {
                    texture: depth,
                    level: dl,
                    layer: dy,
                    view_format,
                    view_target,
                })
            } else {
                None
            },
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferClearColor(
    cmd: usize,
    index: c_int,
    color: *const f32,
    mask: c_int,
) {
    let rgba = if color.is_null() {
        [0.0; 4]
    } else {
        [*color, *color.add(1), *color.add(2), *color.add(3)]
    };
    rec!(
        "nvnCommandBufferClearColor",
        cmd,
        Cmd::ClearColor { index, rgba, mask }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferClearDepthStencil(
    cmd: usize,
    depth: f32,
    depth_mask: u8,
    stencil: c_int,
    stencil_mask: c_int,
) {
    rec!(
        "nvnCommandBufferClearDepthStencil",
        cmd,
        Cmd::ClearDepthStencil {
            depth,
            depth_mask: depth_mask != 0,
            stencil,
            stencil_mask
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetViewport(
    cmd: usize,
    x: c_int,
    y: c_int,
    w: c_int,
    h: c_int,
) {
    rec!(
        "nvnCommandBufferSetViewport",
        cmd,
        Cmd::SetViewport { x, y, w, h }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetViewports(
    cmd: usize,
    first: c_int,
    count: c_int,
    ranges: *const f32,
) {
    if first == 0 && count >= 1 && !ranges.is_null() {
        let x = (*ranges).round() as i32;
        let y = (*ranges.add(1)).round() as i32;
        let w = (*ranges.add(2)).round() as i32;
        let h = (*ranges.add(3)).round() as i32;
        rec!(
            "nvnCommandBufferSetViewports",
            cmd,
            Cmd::SetViewport { x, y, w, h }
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetScissor(
    cmd: usize,
    x: c_int,
    y: c_int,
    w: c_int,
    h: c_int,
) {
    rec!(
        "nvnCommandBufferSetScissor",
        cmd,
        Cmd::SetScissor { x, y, w, h }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetScissors(
    cmd: usize,
    first: c_int,
    count: c_int,
    rects: *const c_int,
) {
    if first == 0 && count >= 1 && !rects.is_null() {
        let x = *rects;
        let y = *rects.add(1);
        let w = *rects.add(2);
        let h = *rects.add(3);
        rec!(
            "nvnCommandBufferSetScissors",
            cmd,
            Cmd::SetScissor { x, y, w, h }
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetDepthBounds(cmd: usize, enable: u8, min: f32, max: f32) {
    rec!(
        "nvnCommandBufferSetDepthBounds",
        cmd,
        Cmd::SetDepthBounds {
            enable: enable != 0,
            min,
            max
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetStencilRef(cmd: usize, faces: i32, value: i32) {
    rec!(
        "nvnCommandBufferSetStencilRef",
        cmd,
        Cmd::SetStencilValue {
            kind: 0,
            faces,
            value
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetStencilValueMask(cmd: usize, faces: i32, value: i32) {
    rec!(
        "nvnCommandBufferSetStencilValueMask",
        cmd,
        Cmd::SetStencilValue {
            kind: 1,
            faces,
            value
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetStencilMask(cmd: usize, faces: i32, value: i32) {
    rec!(
        "nvnCommandBufferSetStencilMask",
        cmd,
        Cmd::SetStencilValue {
            kind: 2,
            faces,
            value
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetDepthRange(cmd: usize, near: f32, far: f32) {
    rec!(
        "nvnCommandBufferSetDepthRange",
        cmd,
        Cmd::SetDepthRange { near, far }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetDepthRanges(
    cmd: usize,
    first: c_int,
    count: c_int,
    ranges: *const f32,
) {
    if first == 0 && count >= 1 && !ranges.is_null() {
        let near = *ranges;
        let far = *ranges.add(1);
        rec!(
            "nvnCommandBufferSetDepthRanges",
            cmd,
            Cmd::SetDepthRange { near, far }
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetPrimitiveRestart(
    cmd: usize,
    enable: u8,
    index: c_int,
) {
    rec!(
        "nvnCommandBufferSetPrimitiveRestart",
        cmd,
        Cmd::SetPrimitiveRestart {
            enable: enable != 0,
            index: index as u32
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetDepthClamp(cmd: usize, enable: u8) {
    rec!(
        "nvnCommandBufferSetDepthClamp",
        cmd,
        Cmd::SetDepthClamp(enable != 0)
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetPolygonOffsetClamp(
    cmd: usize,
    factor: f32,
    units: f32,
    clamp: f32,
) {
    rec!(
        "nvnCommandBufferSetPolygonOffsetClamp",
        cmd,
        Cmd::SetPolygonOffset {
            factor,
            units,
            clamp
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetBlendColor(cmd: usize, color: *const f32) {
    let c = if color.is_null() {
        [0.0; 4]
    } else {
        [*color, *color.add(1), *color.add(2), *color.add(3)]
    };
    rec!("nvnCommandBufferSetBlendColor", cmd, Cmd::SetBlendColor(c));
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindProgram(
    cmd: usize,
    program: usize,
    stages: c_int,
) {
    rec!(
        "nvnCommandBufferBindProgram",
        cmd,
        Cmd::BindProgram { program, stages }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindVertexBuffer(
    cmd: usize,
    index: c_int,
    addr: u64,
    size: usize,
) {
    rec!(
        "nvnCommandBufferBindVertexBuffer",
        cmd,
        Cmd::BindVertexBuffer { index, addr, size }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindVertexAttribState(
    cmd: usize,
    n: c_int,
    attribs: *const u32,
) {
    // NVNvertexAttribState is 4 bytes; NVN has 16 vertex attributes (NVN_DEVICE_INFO_VERTEX_ATTRIBUTES).
    if n > AttribState::MAX as c_int {
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            println!("[!] nvnCommandBufferBindVertexAttribState with {} attributes: only the first {} are kept", n, AttribState::MAX);
        }
    }
    let mut v = AttribState::default();
    v.n = n.clamp(0, AttribState::MAX as c_int) as u8;
    for i in 0..v.n as usize {
        v.v[i] = std::ptr::read_unaligned(attribs.add(i));
    }
    rec!(
        "nvnCommandBufferBindVertexAttribState",
        cmd,
        Cmd::BindVertexAttribState(v)
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindVertexStreamState(
    cmd: usize,
    n: c_int,
    streams: *const u64,
) {
    // NVNvertexStreamState is 8 bytes.
    let v = (0..n.max(0) as usize)
        .map(|i| std::ptr::read_unaligned(streams.add(i)))
        .collect();
    rec!(
        "nvnCommandBufferBindVertexStreamState",
        cmd,
        Cmd::BindVertexStreamState(v)
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindBlendState(cmd: usize, s: *const u64) {
    rec!(
        "nvnCommandBufferBindBlendState",
        cmd,
        Cmd::BindBlendState(if s.is_null() {
            0
        } else {
            std::ptr::read_unaligned(s)
        })
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindColorState(cmd: usize, s: *const u32) {
    rec!(
        "nvnCommandBufferBindColorState",
        cmd,
        Cmd::BindColorState(if s.is_null() {
            0
        } else {
            std::ptr::read_unaligned(s)
        })
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindChannelMaskState(cmd: usize, s: *const u32) {
    rec!(
        "nvnCommandBufferBindChannelMaskState",
        cmd,
        Cmd::BindChannelMaskState(if s.is_null() {
            0
        } else {
            std::ptr::read_unaligned(s)
        })
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindDepthStencilState(cmd: usize, s: *const u64) {
    rec!(
        "nvnCommandBufferBindDepthStencilState",
        cmd,
        Cmd::BindDepthStencilState(if s.is_null() {
            0
        } else {
            std::ptr::read_unaligned(s)
        })
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindMultisampleState(cmd: usize, s: *const u64) {
    rec!(
        "nvnCommandBufferBindMultisampleState",
        cmd,
        Cmd::BindMultisampleState(if s.is_null() {
            0
        } else {
            std::ptr::read_unaligned(s)
        })
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_Barrier(cmd: usize, bits: c_int) {
    rec!("nvnCommandBufferBarrier", cmd, Cmd::Barrier(bits as u32));
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindPolygonState(cmd: usize, s: *const u32) {
    rec!(
        "nvnCommandBufferBindPolygonState",
        cmd,
        Cmd::BindPolygonState(if s.is_null() {
            0
        } else {
            std::ptr::read_unaligned(s)
        })
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindTexture(
    cmd: usize,
    stage: c_int,
    index: c_int,
    handle: u64,
) {
    rec!(
        "nvnCommandBufferBindTexture",
        cmd,
        Cmd::BindTexture {
            stage,
            index,
            handle
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_BindImage(cmd: usize, stage: i32, index: i32, handle: u64) {
    rec!(
        "nvnCommandBufferBindImage",
        cmd,
        Cmd::BindImage {
            stage,
            index,
            handle
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_BindImages(
    cmd: usize,
    stage: i32,
    first: i32,
    count: i32,
    handles: *const u64,
) {
    for i in 0..count.max(0) {
        let handle = if handles.is_null() {
            0
        } else {
            *handles.add(i as usize)
        };
        rec!(
            "nvnCommandBufferBindImages",
            cmd,
            Cmd::BindImage {
                stage,
                index: first + i,
                handle
            }
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_BindTextures(
    cmd: usize,
    stage: c_int,
    first: c_int,
    count: c_int,
    handles: *const u64,
) {
    for i in 0..count.max(0) {
        let handle = if handles.is_null() {
            0
        } else {
            std::ptr::read_unaligned(handles.add(i as usize))
        };
        rec!(
            "nvnCommandBufferBindTextures",
            cmd,
            Cmd::BindTexture {
                stage,
                index: first + i,
                handle
            }
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_BindTexturesDeferred(
    cmd: usize,
    stage: c_int,
    first: c_int,
    count: c_int,
    handles: *const u64,
) {
    rec!(
        "nvnCommandBufferBindTexturesDeferred",
        cmd,
        Cmd::BindTexturesDeferred {
            stage,
            first,
            count,
            handles: handles as u64
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetRenderEnable(cmd: usize, enable: u8) {
    rec!(
        "nvnCommandBufferSetRenderEnable",
        cmd,
        Cmd::SetRenderEnable(enable != 0)
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetRenderEnableConditional(
    cmd: usize,
    mode: c_int,
    counters: u64,
) {
    rec!(
        "nvnCommandBufferSetRenderEnableConditional",
        cmd,
        Cmd::SetRenderEnableConditional { mode, counters }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetPointSize(cmd: usize, size: f32) {
    rec!("nvnCommandBufferSetPointSize", cmd, Cmd::SetPointSize(size));
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_DispatchCompute(cmd: usize, x: c_int, y: c_int, z: c_int) {
    rec!(
        "nvnCommandBufferDispatchCompute",
        cmd,
        Cmd::DispatchCompute { x, y, z }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBindUniformBuffer(
    cmd: usize,
    stage: c_int,
    index: c_int,
    addr: u64,
    size: usize,
) {
    rec!(
        "nvnCommandBufferBindUniformBuffer",
        cmd,
        Cmd::BindUniformBuffer {
            stage,
            index,
            addr,
            size
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_BindUniformBuffers(
    cmd: usize,
    stage: c_int,
    first: c_int,
    count: c_int,
    ranges: *const u64,
) {
    for i in 0..count.max(0) {
        let (addr, size) = if ranges.is_null() {
            (0, 0)
        } else {
            (
                *ranges.add(i as usize * 2),
                *ranges.add(i as usize * 2 + 1) as usize,
            )
        };
        rec!(
            "nvnCommandBufferBindUniformBuffers",
            cmd,
            Cmd::BindUniformBuffer {
                stage,
                index: first + i,
                addr,
                size
            }
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_BindStorageBuffer(
    cmd: usize,
    stage: c_int,
    index: c_int,
    addr: u64,
    size: usize,
) {
    rec!(
        "nvnCommandBufferBindStorageBuffer",
        cmd,
        Cmd::BindStorageBuffer {
            stage,
            index,
            addr,
            size
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_BindStorageBuffers(
    cmd: usize,
    stage: c_int,
    first: c_int,
    count: c_int,
    ranges: *const u64,
) {
    for i in 0..count.max(0) {
        let (addr, size) = if ranges.is_null() {
            (0, 0)
        } else {
            (
                *ranges.add(i as usize * 2),
                *ranges.add(i as usize * 2 + 1) as usize,
            )
        };
        rec!(
            "nvnCommandBufferBindStorageBuffers",
            cmd,
            Cmd::BindStorageBuffer {
                stage,
                index: first + i,
                addr,
                size
            }
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferUpdateUniformBuffer(
    cmd: usize,
    addr: u64,
    _buffer_size: usize,
    offset: isize,
    size: usize,
    data: *const u8,
) {
    let d = if data.is_null() || size == 0 || size > 0x10000 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(data, size).to_vec()
    };
    rec!(
        "nvnCommandBufferUpdateUniformBuffer",
        cmd,
        Cmd::UpdateUniformBuffer {
            addr,
            offset,
            data: d
        }
    );
}

thread_local! {
    /// --log-drawables: the drawable object the game is drawing on this thread, and its draw function.
    static DRAWABLE: std::cell::Cell<(usize, usize, u32)> = const { std::cell::Cell::new((0, 0, 0)) };
    static DRAW_CTX: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// TotK 1.1.2 drawable probe (profile hook at main+0x12de360): called before each render-list entry's draw.
pub unsafe extern "C" fn note_drawable(obj: usize, func: usize, ctx: usize) {
    DRAWABLE.with(|d| d.set((obj, func, 0)));
    DRAW_CTX.with(|c| c.set(ctx));
    if obj != 0 {
        drawable_cmd_stats(func, None);
    }
}

/// --log-drawables: NVN commands recorded per drawable call, by draw function and command (every 600 frames).
pub fn drawable_cmd_stats(func: usize, cmd: Option<&'static str>) {
    struct S {
        calls: std::collections::HashMap<usize, u64>,
        cmds: std::collections::HashMap<(usize, &'static str), u64>,
        last: u64,
    }
    static M: std::sync::Mutex<Option<S>> = std::sync::Mutex::new(None);
    let mut g = M.lock().unwrap();
    let s = g.get_or_insert_with(|| S {
        calls: Default::default(),
        cmds: Default::default(),
        last: 0,
    });
    let base = crate::cpu::crash_handler::get_guest_base();
    let k = func.wrapping_sub(base);
    match cmd {
        None => *s.calls.entry(k).or_default() += 1,
        Some(c) => *s.cmds.entry((k, c)).or_default() += 1,
    }
    let frame = crate::gpu::backend().presented_frames();
    if frame >= s.last + 600 {
        s.last = frame;
        let mut funcs: Vec<_> = s.calls.iter().map(|(k, v)| (*v, *k)).collect();
        funcs.sort_unstable_by(|a, b| b.cmp(a));
        for (calls, f) in funcs.iter().take(4) {
            let mut c: Vec<_> = s
                .cmds
                .iter()
                .filter(|((ff, _), _)| ff == f)
                .map(|((_, n), v)| (*v, *n))
                .collect();
            c.sort_unstable_by(|a, b| b.cmp(a));
            let l: Vec<String> = c
                .iter()
                .take(14)
                .map(|(v, n)| {
                    format!(
                        "{} {:.2}",
                        n.trim_start_matches("nvnCommandBuffer"),
                        *v as f64 / *calls as f64
                    )
                })
                .collect();
            println!(
                "[drawable-cmds] main+{:#x}: {} calls; commands per call: {}",
                f,
                calls,
                l.join(", ")
            );
        }
        s.calls.clear();
        s.cmds.clear();
    }
}

/// The current drawable (object, draw function) of this thread (--log-drawables).
pub fn current_drawable() -> (usize, usize) {
    DRAWABLE.with(|d| {
        let (o, f, _) = d.get();
        (o, f)
    })
}

/// The current drawable and the number of draws it recorded before this one (its draw ordinal), counting this draw.
fn next_drawable_draw() -> (usize, usize, u32) {
    DRAWABLE.with(|d| {
        let (o, f, n) = d.get();
        d.set((o, f, n + 1));
        (o, f, n)
    })
}

/// --log-draw-callers=N: which game code records the draws. Every draw of frames N..N+60 is counted by its guest call
/// chain (the first five return addresses inside main, from the frame-pointer records; the first record is the HLE
/// trampoline's), with its index / vertex count; printed once after frame N+60, most draws first. For finding the render
/// systems worth patching in the game.
#[inline(always)]
unsafe fn note_draw_caller(count: c_int) -> u32 {
    note_draw_caller_at(count, 0)
}

unsafe fn note_draw_caller_at(count: c_int, index_addr: u64) -> u32 {
    // Measurement (--log-draw-items, TotK 1.1.2): the draw's item object instead of its caller. main+0x763718 takes
    // (renderer, item, ...), keeps the item at [x29 - 0x20] and calls the renderer's draw callback (return address
    // main+0x763798); most of the frame's draws come through it.
    static DRAWABLES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *DRAWABLES.get_or_init(|| {
        crate::settings::get().totk.diagnostics.log_drawables && crate::game::is_totk()
    }) {
        let (obj, func, nth) = next_drawable_draw();
        note_drawable_class(func, count);
        // Token: the object and the draw's index range (indexed draws: the sub-mesh range drawn, stable when culling
        // shifts the ordinal), else its ordinal within the object's draw call.
        let id = if index_addr != 0 {
            (index_addr as u32)
                ^ ((index_addr >> 32) as u32).rotate_left(16)
                ^ (count as u32).wrapping_mul(0x27D4_EB2F)
        } else {
            nth.wrapping_mul(0x85EB_CA6B)
        };
        let ctx = DRAW_CTX.with(|c| c.get());
        if obj != 0 {
            // Draws repeating an identity within the frame (the same mesh range drawn again by the same drawable in the
            // same pass), per draw function.
            static SEEN: std::sync::Mutex<
                Option<(
                    u64,
                    std::collections::HashSet<(usize, u64, i32, usize)>,
                    std::collections::HashMap<usize, (u64, u64)>,
                    u64,
                )>,
            > = std::sync::Mutex::new(None);
            let mut g = SEEN.lock().unwrap();
            let (f, set, per, last) =
                g.get_or_insert_with(|| (0, Default::default(), Default::default(), 0));
            let frame = crate::gpu::backend().presented_frames();
            if *f != frame {
                set.clear();
                *f = frame;
            }
            let base = crate::cpu::crash_handler::get_guest_base();
            let k = func.wrapping_sub(base);
            let e = per.entry(k).or_default();
            e.0 += 1;
            if !set.insert((obj, index_addr, count, ctx)) {
                e.1 += 1;
            }
            if frame >= *last + 600 {
                *last = frame;
                for (k, (n, r)) in per.iter() {
                    if *n > 600 {
                        println!("[drawables] main+{:#x}: {:.1}% of its draws repeat an identity within the frame", k, 100.0 * *r as f64 / *n as f64);
                    }
                }
                per.clear();
            }
        }
        return if obj != 0 {
            (((obj >> 3) as u32).wrapping_mul(0x9E37_79B1)
                ^ id
                ^ ((ctx >> 4) as u32).wrapping_mul(0x1656_67B1))
                | 1
        } else {
            0
        };
    }
    static ITEMS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ITEMS.get_or_init(|| {
        crate::settings::get().totk.diagnostics.log_draw_items && crate::game::is_totk()
    }) {
        let base = crate::cpu::crash_handler::get_guest_base();
        let mut fp: usize;
        std::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack));
        for _ in 0..16 {
            if fp < 0x1000 || fp & 7 != 0 {
                break;
            }
            let next = *(fp as *const usize);
            let ret = *((fp + 8) as *const usize);
            if ret == base + 0x763798 && next > 0x1000 && next & 7 == 0 {
                let item = *((next - 0x20) as *const usize);
                return ((item >> 3) as u32) | 1;
            }
            if next <= fp {
                break;
            }
            fp = next;
        }
        return 0;
    }
    static FROM: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    let Some(from) = *FROM.get_or_init(|| crate::settings::get().log.log_draw_callers) else {
        return 0;
    };
    let frame = crate::gpu::backend().presented_frames();
    if frame < from {
        return 0;
    }
    static MAP: std::sync::Mutex<Option<std::collections::HashMap<[u32; 8], (u64, u64)>>> =
        std::sync::Mutex::new(None);
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if frame >= from + 60 {
        if !DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let g = MAP.lock().unwrap();
            let mut rows: Vec<_> = g
                .as_ref()
                .map(|m| m.iter().map(|(k, v)| (*k, *v)).collect())
                .unwrap_or_default();
            rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
            let total: u64 = rows.iter().map(|r| r.1 .0).sum();
            println!(
                "[draw-callers] frames {}..{}: {} draws per frame, {} call chains",
                from,
                from + 60,
                total / 60,
                rows.len()
            );
            for (k, (n, idx)) in rows.iter() {
                let chain: Vec<String> = k
                    .iter()
                    .filter(|&&a| a != 0)
                    .map(|a| format!("{:#x}", a))
                    .collect();
                println!(
                    "[draw-callers] {:7.1} draws {:9.0} idx per frame  main+ {}",
                    *n as f64 / 60.0,
                    *idx as f64 / 60.0,
                    chain.join(" <- ")
                );
            }
            // Executor time per first return address (Gfx::run_list around each draw: planning on the Presentation Thread).
            let t = DRAW_TIME.lock().unwrap();
            let mut tr: Vec<_> = t
                .as_ref()
                .map(|m| m.iter().map(|(k, v)| (*k, *v)).collect())
                .unwrap_or_default();
            tr.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
            let tt: u64 = tr.iter().map(|r| r.1 .0).sum();
            println!(
                "[draw-time] executor time in draws: {:.2} ms per frame",
                tt as f64 / 60.0 / 1e6
            );
            for (k, (ns, n)) in tr.iter().take(40) {
                println!(
                    "[draw-time] {:6.3} ms {:6.1} draws per frame ({:5.2} us each)  main+{:#x}",
                    *ns as f64 / 60.0 / 1e6,
                    *n as f64 / 60.0,
                    *ns as f64 / *n.max(&1) as f64 / 1e3,
                    k
                );
            }
        }
        return 0;
    }
    let base = crate::cpu::crash_handler::get_guest_base();
    let mut fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack));
    let mut key = [0u32; 8];
    let mut n = 0;
    for _ in 0..32 {
        if fp < 0x1000 || fp & 7 != 0 || n == 8 {
            break;
        }
        let next = *(fp as *const usize);
        let ret = *((fp + 8) as *const usize);
        if ret >= base && ret < base + (256 << 20) {
            key[n] = (ret - base) as u32;
            n += 1;
        }
        if next <= fp {
            break;
        }
        fp = next;
    }
    let mut g = MAP.lock().unwrap();
    let e = g
        .get_or_insert_with(Default::default)
        .entry(key)
        .or_default();
    e.0 += 1;
    e.1 += count.max(0) as u64;
    key[0]
}

/// --log-drawables: draws per drawable class (its draw function, main+ offset), printed every 600 frames.
fn note_drawable_class(func: usize, count: c_int) {
    static M: std::sync::Mutex<Option<(std::collections::HashMap<usize, (u64, u64)>, u64)>> =
        std::sync::Mutex::new(None);
    let mut g = M.lock().unwrap();
    let (m, last) = g.get_or_insert_with(|| (Default::default(), 0));
    let base = crate::cpu::crash_handler::get_guest_base();
    let k = if func >= base && func < base + (256 << 20) {
        func - base
    } else {
        0
    };
    let e = m.entry(k).or_default();
    e.0 += 1;
    e.1 += count.max(0) as u64;
    let frame = crate::gpu::backend().presented_frames();
    if frame >= *last + 600 {
        let n = (frame - *last).max(1) as f64;
        *last = frame;
        let mut rows: Vec<_> = m.iter().map(|(k, v)| (*k, *v)).collect();
        rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        println!("[drawables] f{}: draws per frame by drawable draw function (0 = outside the render-list loop):", frame);
        for (k, (d, i)) in rows.iter().take(30) {
            println!(
                "[drawables]   {:7.1} draws {:9.0} idx  main+{:#x}",
                *d as f64 / n,
                *i as f64 / n,
                k
            );
        }
        m.clear();
    }
}

static DRAW_TIME: std::sync::Mutex<Option<std::collections::HashMap<u32, (u64, u64)>>> =
    std::sync::Mutex::new(None);

/// --log-draw-callers: executor time of one draw recorded by `caller` (0: not tagged).
pub fn note_draw_time(caller: u32, ns: u64) {
    if caller == 0 {
        return;
    }
    let mut g = DRAW_TIME.lock().unwrap();
    let e = g
        .get_or_insert_with(Default::default)
        .entry(caller)
        .or_default();
    e.0 += ns;
    e.1 += 1;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferDrawArrays(
    cmd: usize,
    mode: c_int,
    first: c_int,
    count: c_int,
) {
    let caller = note_draw_caller(count);
    rec!(
        "nvnCommandBufferDrawArrays",
        cmd,
        Cmd::DrawArrays {
            mode,
            first,
            count,
            caller
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferDrawElements(
    cmd: usize,
    mode: c_int,
    index_type: c_int,
    count: c_int,
    index_addr: u64,
) {
    let caller = note_draw_caller_at(count, index_addr);
    rec!(
        "nvnCommandBufferDrawElements",
        cmd,
        Cmd::DrawElements {
            mode,
            index_type,
            count,
            index_addr,
            base_vertex: 0,
            caller
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferDrawElementsBaseVertex(
    cmd: usize,
    mode: c_int,
    index_type: c_int,
    count: c_int,
    index_addr: u64,
    base_vertex: c_int,
) {
    let caller = note_draw_caller_at(count, index_addr);
    rec!(
        "nvnCommandBufferDrawElementsBaseVertex",
        cmd,
        Cmd::DrawElements {
            mode,
            index_type,
            count,
            index_addr,
            base_vertex,
            caller
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferCallCommands(
    cmd: usize,
    n: c_int,
    handles: *const u64,
) {
    let v: smallvec::SmallVec<[u64; 2]> = (0..n.max(0) as usize)
        .map(|i| std::ptr::read_unaligned(handles.add(i)))
        .collect();
    rec!("nvnCommandBufferCallCommands", cmd, Cmd::CallCommands(v));
}

#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_DrawArraysInstanced(
    cmd: usize,
    mode: c_int,
    first: c_int,
    count: c_int,
    base_instance: c_int,
    instances: c_int,
) {
    let caller = note_draw_caller(count * instances.max(1));
    rec!(
        "nvnCommandBufferDrawArraysInstanced",
        cmd,
        Cmd::DrawArraysInstanced {
            mode,
            first,
            count,
            base_instance,
            instances,
            caller
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_DrawElementsInstanced(
    cmd: usize,
    mode: c_int,
    index_type: c_int,
    count: c_int,
    index_addr: u64,
    base_vertex: c_int,
    base_instance: c_int,
    instances: c_int,
) {
    let caller = note_draw_caller_at(count * instances.max(1), index_addr);
    rec!(
        "nvnCommandBufferDrawElementsInstanced",
        cmd,
        Cmd::DrawElementsInstanced {
            mode,
            index_type,
            count,
            index_addr,
            base_vertex,
            base_instance,
            instances,
            caller
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_ResetCounter(cmd: usize, counter: c_int) {
    rec!(
        "nvnCommandBufferResetCounter",
        cmd,
        Cmd::ResetCounter { counter }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_ReportCounter(cmd: usize, counter: c_int, addr: u64) {
    rec!(
        "nvnCommandBufferReportCounter",
        cmd,
        Cmd::ReportCounter { counter, addr }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_ReportValue(cmd: usize, value: u32, addr: u64) {
    rec!(
        "nvnCommandBufferReportValue",
        cmd,
        Cmd::ReportValue { value, addr }
    );
}

// ---- copies ----

/// nvnCommandBufferSetCopyRowStride / SetCopyImageStride are per-command-buffer state that is NOT stored in the command
/// stream, so it is captured into each copy command as it is recorded.
static COPY_STRIDES: std::sync::Mutex<Vec<(usize, isize, isize)>> =
    std::sync::Mutex::new(Vec::new());

fn copy_strides(cmd: usize) -> (isize, isize) {
    COPY_STRIDES
        .lock()
        .unwrap()
        .iter()
        .find(|e| e.0 == cmd)
        .map(|e| (e.1, e.2))
        .unwrap_or((0, 0))
}

fn set_copy_stride(cmd: usize, row: Option<isize>, image: Option<isize>) {
    let mut v = COPY_STRIDES.lock().unwrap();
    if let Some(e) = v.iter_mut().find(|e| e.0 == cmd) {
        if let Some(r) = row {
            e.1 = r;
        }
        if let Some(i) = image {
            e.2 = i;
        }
    } else {
        v.push((cmd, row.unwrap_or(0), image.unwrap_or(0)));
    }
}

unsafe fn tex_ref(tex: usize, view: *const u8) -> TexRef {
    crate::nvn::api::capture_texture(tex);
    let (level, layer) = view_level_layer(view);
    // Debug: --log-texref logs the object's state as recorded (generation, size, storage).
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *LOG.get_or_init(|| crate::settings::get().log.log_texref) {
        if let Some((gen, t)) = crate::nvn::api::texture_snapshot(tex) {
            println!(
                "[texref] record tex={:#x} gen={} {}x{} fmt={} storage={:#x} level={} layer={}",
                tex, gen, t.width, t.height, t.format, t.storage, level, layer
            );
        }
    }
    TexRef {
        texture: tex,
        level,
        layer,
    }
}

unsafe fn region(r: *const i32) -> Region {
    if r.is_null() {
        return Region::default();
    }
    Region {
        x: *r,
        y: *r.add(1),
        z: *r.add(2),
        w: *r.add(3),
        h: *r.add(4),
        d: *r.add(5),
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetCopyRowStride(cmd: usize, stride: isize) {
    set_copy_stride(cmd, Some(stride), None);
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_SetCopyImageStride(cmd: usize, stride: isize) {
    set_copy_stride(cmd, None, Some(stride));
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_CopyBufferToTexture(
    cmd: usize,
    src: u64,
    dst: usize,
    dst_view: *const u8,
    dst_region: *const i32,
    _flags: c_int,
) {
    let (row_stride, image_stride) = copy_strides(cmd);
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *LOG.get_or_init(|| crate::settings::get().log.log_movie) {
        println!(
            "[movie] record CopyBufferToTexture cb={:#x} src={:#x} dst={:#x} region={:?}",
            cmd,
            src,
            dst,
            region(dst_region)
        );
    }
    rec!(
        "nvnCommandBufferCopyBufferToTexture",
        cmd,
        Cmd::CopyBufferToTexture {
            src,
            dst: tex_ref(dst, dst_view),
            region: region(dst_region),
            row_stride,
            image_stride,
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_CopyTextureToBuffer(
    cmd: usize,
    src: usize,
    src_view: *const u8,
    src_region: *const i32,
    dst: u64,
    _flags: c_int,
) {
    let (row_stride, image_stride) = copy_strides(cmd);
    rec!(
        "nvnCommandBufferCopyTextureToBuffer",
        cmd,
        Cmd::CopyTextureToBuffer {
            src: tex_ref(src, src_view),
            region: region(src_region),
            dst,
            row_stride,
            image_stride,
        }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_CopyTextureToTexture(
    cmd: usize,
    src: usize,
    src_view: *const u8,
    src_region: *const i32,
    dst: usize,
    dst_view: *const u8,
    dst_region: *const i32,
    _flags: c_int,
) {
    rec!(
        "nvnCommandBufferCopyTextureToTexture",
        cmd,
        Cmd::CopyTextureToTexture(Box::new(CopyTextureToTextureParams {
            src: tex_ref(src, src_view),
            src_region: region(src_region),
            dst: tex_ref(dst, dst_view),
            dst_region: region(dst_region),
        }))
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_CopyBufferToBuffer(
    cmd: usize,
    src: u64,
    dst: u64,
    size: usize,
    _flags: c_int,
) {
    rec!(
        "nvnCommandBufferCopyBufferToBuffer",
        cmd,
        Cmd::CopyBufferToBuffer { src, dst, size }
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_ClearBuffer(cmd: usize, dst: u64, size: usize, value: u32) {
    rec!(
        "nvnCommandBufferClearBuffer",
        cmd,
        Cmd::ClearBuffer { dst, size, value }
    );
}
unsafe fn clear_texture(
    cmd: usize,
    name: &'static str,
    dst: usize,
    view: *const u8,
    r: *const i32,
    color: *const u32,
    mask: c_int,
) {
    let c = if color.is_null() {
        [0; 4]
    } else {
        [*color, *color.add(1), *color.add(2), *color.add(3)]
    };
    record_typed(
        name,
        cmd,
        Cmd::ClearTexture(Box::new(ClearTextureParams {
            dst: tex_ref(dst, view),
            region: region(r),
            color: c,
            mask,
        })),
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_ClearTexture(
    cmd: usize,
    dst: usize,
    view: *const u8,
    r: *const i32,
    color: *const f32,
    mask: c_int,
) {
    clear_texture(
        cmd,
        "nvnCommandBufferClearTexture",
        dst,
        view,
        r,
        color as *const u32,
        mask,
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_ClearTexturei(
    cmd: usize,
    dst: usize,
    view: *const u8,
    r: *const i32,
    color: *const i32,
    mask: c_int,
) {
    clear_texture(
        cmd,
        "nvnCommandBufferClearTexturei",
        dst,
        view,
        r,
        color as *const u32,
        mask,
    );
}
#[no_mangle]
pub unsafe extern "C" fn hle_typed_cb_ClearTextureui(
    cmd: usize,
    dst: usize,
    view: *const u8,
    r: *const i32,
    color: *const u32,
    mask: c_int,
) {
    clear_texture(
        cmd,
        "nvnCommandBufferClearTextureui",
        dst,
        view,
        r,
        color,
        mask,
    );
}

/// Names (and addresses) of the typed recorders; used by the proc-address table.
pub fn lookup(name: &str) -> Option<usize> {
    macro_rules! f {
        ($f:ident($($a:tt),*)) => {
            Some(crate::hle!($f($($a),*)) as usize)
        };
    }
    match name {
        "nvnCommandBufferSetRenderTargets" => f!(hle_nvnCommandBufferSetRenderTargets(_, _, _, _, _, _)),
        "nvnCommandBufferClearColor" => f!(hle_nvnCommandBufferClearColor(_, _, _, _)),
        "nvnCommandBufferClearDepthStencil" => f!(hle_nvnCommandBufferClearDepthStencil(_, _, _, _, _)),
        "nvnCommandBufferSetViewport" => f!(hle_nvnCommandBufferSetViewport(_, _, _, _, _)),
        "nvnCommandBufferSetViewports" => f!(hle_nvnCommandBufferSetViewports(_, _, _, _)),
        "nvnCommandBufferSetScissor" => f!(hle_nvnCommandBufferSetScissor(_, _, _, _, _)),
        "nvnCommandBufferSetScissors" => f!(hle_nvnCommandBufferSetScissors(_, _, _, _)),
        "nvnCommandBufferSetDepthRange" => f!(hle_nvnCommandBufferSetDepthRange(_, _, _)),
        "nvnCommandBufferSetDepthRanges" => f!(hle_nvnCommandBufferSetDepthRanges(_, _, _, _)),
        "nvnCommandBufferSetDepthBounds" => f!(hle_typed_cb_SetDepthBounds(_, _, _, _)),
        "nvnCommandBufferSetStencilRef" => f!(hle_typed_cb_SetStencilRef(_, _, _)),
        "nvnCommandBufferSetStencilValueMask" => f!(hle_typed_cb_SetStencilValueMask(_, _, _)),
        "nvnCommandBufferSetStencilMask" => f!(hle_typed_cb_SetStencilMask(_, _, _)),
        "nvnCommandBufferBindImage" => f!(hle_typed_cb_BindImage(_, _, _, _)),
        "nvnCommandBufferBindImages" => f!(hle_typed_cb_BindImages(_, _, _, _, _)),
        "nvnCommandBufferSetDepthClamp" => f!(hle_nvnCommandBufferSetDepthClamp(_, _)),
        "nvnCommandBufferSetPrimitiveRestart" => f!(hle_nvnCommandBufferSetPrimitiveRestart(_, _, _)),
        "nvnCommandBufferSetPolygonOffsetClamp" => f!(hle_typed_cb_SetPolygonOffsetClamp(_, _, _, _)),
        "nvnCommandBufferSetBlendColor" => f!(hle_nvnCommandBufferSetBlendColor(_, _)),
        "nvnCommandBufferBindProgram" => f!(hle_nvnCommandBufferBindProgram(_, _, _)),
        "nvnCommandBufferBindVertexBuffer" => f!(hle_nvnCommandBufferBindVertexBuffer(_, _, _, _)),
        "nvnCommandBufferBindVertexAttribState" => f!(hle_nvnCommandBufferBindVertexAttribState(_, _, _)),
        "nvnCommandBufferBindVertexStreamState" => f!(hle_nvnCommandBufferBindVertexStreamState(_, _, _)),
        "nvnCommandBufferBindBlendState" => f!(hle_nvnCommandBufferBindBlendState(_, _)),
        "nvnCommandBufferBindColorState" => f!(hle_nvnCommandBufferBindColorState(_, _)),
        "nvnCommandBufferBindChannelMaskState" => f!(hle_nvnCommandBufferBindChannelMaskState(_, _)),
        "nvnCommandBufferBindDepthStencilState" => f!(hle_nvnCommandBufferBindDepthStencilState(_, _)),
        "nvnCommandBufferBindMultisampleState" => f!(hle_nvnCommandBufferBindMultisampleState(_, _)),
        "nvnCommandBufferBarrier" => f!(hle_typed_cb_Barrier(_, _)),
        "nvnCommandBufferBindPolygonState" => f!(hle_nvnCommandBufferBindPolygonState(_, _)),
        "nvnCommandBufferBindTexture" => f!(hle_nvnCommandBufferBindTexture(_, _, _, _)),
        "nvnCommandBufferBindTextures" => f!(hle_typed_cb_BindTextures(_, _, _, _, _)),
        "nvnCommandBufferBindTexturesDeferred" => f!(hle_typed_cb_BindTexturesDeferred(_, _, _, _, _)),
        "nvnCommandBufferSetRenderEnable" => f!(hle_typed_cb_SetRenderEnable(_, _)),
        "nvnCommandBufferDispatchCompute" => f!(hle_typed_cb_DispatchCompute(_, _, _, _)),
        "nvnCommandBufferSetPointSize" => f!(hle_typed_cb_SetPointSize(_, _)),
        "nvnCommandBufferSetRenderEnableConditional" => f!(hle_typed_cb_SetRenderEnableConditional(_, _, _)),
        "nvnCommandBufferBindUniformBuffer" => f!(hle_nvnCommandBufferBindUniformBuffer(_, _, _, _, _)),
        "nvnCommandBufferUpdateUniformBuffer" => f!(hle_nvnCommandBufferUpdateUniformBuffer(_, _, _, _, _, _)),
        "nvnCommandBufferBindUniformBuffers" => f!(hle_typed_cb_BindUniformBuffers(_, _, _, _, _)),
        "nvnCommandBufferBindStorageBuffer" => f!(hle_typed_cb_BindStorageBuffer(_, _, _, _, _)),
        "nvnCommandBufferBindStorageBuffers" => f!(hle_typed_cb_BindStorageBuffers(_, _, _, _, _)),
        "nvnCommandBufferDrawArrays" => f!(hle_nvnCommandBufferDrawArrays(_, _, _, _)),
        "nvnCommandBufferDrawElements" => f!(hle_nvnCommandBufferDrawElements(_, _, _, _, _)),
        "nvnCommandBufferDrawElementsBaseVertex" => f!(hle_nvnCommandBufferDrawElementsBaseVertex(_, _, _, _, _, _)),
        "nvnCommandBufferCallCommands" => f!(hle_nvnCommandBufferCallCommands(_, _, _)),
        "nvnCommandBufferDrawArraysInstanced" => f!(hle_typed_cb_DrawArraysInstanced(_, _, _, _, _, _)),
        "nvnCommandBufferDrawElementsInstanced" => f!(hle_typed_cb_DrawElementsInstanced(_, _, _, _, _, _, _, _)),
        "nvnCommandBufferResetCounter" => f!(hle_typed_cb_ResetCounter(_, _)),
        "nvnCommandBufferReportCounter" => f!(hle_typed_cb_ReportCounter(_, _, _)),
        "nvnCommandBufferReportValue" => f!(hle_typed_cb_ReportValue(_, _, _)),
        "nvnCommandBufferSetCopyRowStride" => f!(hle_typed_cb_SetCopyRowStride(_, _)),
        "nvnCommandBufferSetCopyImageStride" => f!(hle_typed_cb_SetCopyImageStride(_, _)),
        "nvnCommandBufferCopyBufferToTexture" => f!(hle_typed_cb_CopyBufferToTexture(_, _, _, _, _, _)),
        "nvnCommandBufferCopyTextureToBuffer" => f!(hle_typed_cb_CopyTextureToBuffer(_, _, _, _, _, _)),
        "nvnCommandBufferCopyTextureToTexture" => f!(hle_typed_cb_CopyTextureToTexture(_, _, _, _, _, _, _, _)),
        "nvnCommandBufferCopyBufferToBuffer" => f!(hle_typed_cb_CopyBufferToBuffer(_, _, _, _, _)),
        "nvnCommandBufferClearBuffer" => f!(hle_typed_cb_ClearBuffer(_, _, _, _)),
        "nvnCommandBufferClearTexture" => f!(hle_typed_cb_ClearTexture(_, _, _, _, _, _)),
        "nvnCommandBufferClearTexturei" => f!(hle_typed_cb_ClearTexturei(_, _, _, _, _, _)),
        "nvnCommandBufferClearTextureui" => f!(hle_typed_cb_ClearTextureui(_, _, _, _, _, _)),
        _ => None,
    }
}

const _: () = assert!(
    std::mem::size_of::<Cmd>() <= 72,
    "Cmd grew: keep large payloads out of line"
);
