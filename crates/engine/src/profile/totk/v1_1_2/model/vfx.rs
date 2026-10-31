// The effect system's emitter draw (main+0x843c, reached from the gsys view pass main+0x9c8d00 through main+0x4248):
// the emitter's shader, render state, textures, uniform blocks, vertex streams and custom attributes, then one
// instanced draw of its particles. Emitter callbacks (the game's custom effect shaders) are left to the game.
// nn::gfx command buffer wrappers are called through the GOT slots the game's own wrappers use.

use super::draw::{Port, R};

/// GOT slots of the NVN entry points (each holds the address of the pfn variable).
const PFN_BIND_BLEND_STATE: usize = 0x466df70;
const PFN_BIND_CHANNEL_MASK_STATE: usize = 0x466df78;
const PFN_BIND_COLOR_STATE: usize = 0x466df80;
const PFN_SET_BLEND_COLOR: usize = 0x466e190;
const PFN_DEVICE_GET_TEXTURE_HANDLE: usize = 0x466d428;
const PFN_BIND_TEXTURE: usize = 0x466e008;
const PFN_BIND_UNIFORM_BUFFER: usize = 0x466dfc8;
const PFN_BIND_STORAGE_BUFFER: usize = 0x466dff0;
const PFN_BIND_VERTEX_ATTRIB_STATE: usize = 0x466dfa0;
const PFN_BIND_VERTEX_STREAM_STATE: usize = 0x466dfa8;
const GOT_DRAW_ARRAYS_INSTANCED: usize = 0x4613b88;
const GOT_BIND_DEPTH_STENCIL_STATE: usize = 0x4613bb8;
const GOT_SET_STENCIL_VALUE_MASK: usize = 0x4613bc0;
const GOT_SET_STENCIL_MASK: usize = 0x4613bc8;
const GOT_SET_STENCIL_REF: usize = 0x4613bd0;
const GOT_SET_DEPTH_BOUNDS: usize = 0x4613bd8;
const GOT_BIND_POLYGON_STATE: usize = 0x4613810;
const GOT_SET_POLYGON_OFFSET_CLAMP: usize = 0x4613818;
const GOT_BIND_MULTISAMPLE_STATE: usize = 0x4613c00;
const GOT_SET_SAMPLE_MASK: usize = 0x4613c08;
const GOT_SET_DEPTH_CLAMP: usize = 0x4613c10;
const GOT_SET_RASTERIZER_DISCARD: usize = 0x4613c18;
const GOT_SET_CONSERVATIVE_RASTER: usize = 0x4613c20;
const GOT_BIND_VERTEX_BUFFER: usize = 0x4613c30;
/// nn::gfx shader stage -> NVN stage, primitive topology -> NVN draw mode (u32 tables).
const STAGE_TABLE: usize = 0x37ff9f4;
const TOPOLOGY_TABLE: usize = 0x37ff9b8;
const GLOBAL_TEXTURES: usize = 0x4613830;

pub(super) static OTHER: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

/// A callback the port does not implement: counted by address, predicting stops.
fn callback(f: usize, base: usize) -> R<()> {
    let off = f.wrapping_sub(base);
    let mut o = OTHER.lock().unwrap();
    match o.iter_mut().find(|e| e.0 == off) {
        Some(e) => e.1 += 1,
        None => o.push((off, 1)),
    }
    Err("vfx emitter callback")
}

/// What a callback gets (the caller's stack block): g, emitter, a3, params, then the draw path byte (+0x20) and
/// whether it is 3 (+0x21).
struct Info {
    g: usize,
    em: usize,
    a3: usize,
    dp: usize,
    path: u8,
}

/// An emitter callback at absolute address `f`: Some(result) when ported.
unsafe fn run_callback(p: &mut Port, f: usize, i: &Info) -> R<bool> {
    match f.wrapping_sub(p.base) {
        // Returns true.
        0x26a6eb0 => Ok(true),
        0x7c4bf0 => Ok(custom_block(p, i)),
        0x7c4440 => render_state_basic2(p, i, false),
        0xd61360 => render_state_basic2(p, i, true),
        0x780f80 => {
            render_state_blend(p, i);
            Ok(true)
        }
        // The params' +0x4d byte.
        0x1f73c => Ok(p.m.r8(i.dp + 0x4d) & 1 != 0),
        _ => callback(f, p.base).map(|_| false),
    }
}

/// main+0x7c4bf0: the resource's custom shader uniform block (+0x3b8 set; [+0x40]+0x188 + [+0x3f0], [+0x3f8] bytes) to
/// the shader's vertex / fragment slots (+0x3e / +0x80).
unsafe fn custom_block(p: &mut Port, i: &Info) -> bool {
    let res = p.m.r64(i.em + 0x40);
    let sh = if i.path == 3 { 0 } else { p.m.r64(res + i.path as usize * 8 + 0x10) };
    if p.m.r64(res + 0x3b8) == 0 {
        return false;
    }
    let addr = p.m.r64(p.m.r64(res + 0x40) + 0x188).wrapping_add(p.m.r32(res + 0x3f0) as usize) as u64;
    let size = p.m.r32(res + 0x3f8) as u64;
    let (v, f) = (p.m.i8(sh + 0x3e), p.m.i8(sh + 0x80));
    if v != -1 {
        bind_uniform(p, i.g, v as i32, 0, addr, size);
    }
    if f != -1 {
        bind_uniform(p, i.g, f as i32, 4, addr, size);
    }
    true
}

pub(super) fn report() -> String {
    let mut g = std::mem::take(&mut *OTHER.lock().unwrap());
    g.sort_by(|a, b| b.1.cmp(&a.1));
    g.iter().map(|(f, n)| format!("main+{f:#x} {n}")).collect::<Vec<_>>().join(", ")
}

unsafe fn got(p: &Port, slot: usize) -> usize {
    p.m.r64(p.m.r64(p.base + slot))
}

unsafe fn stage(p: &Port, s: u32) -> u64 {
    p.m.r32(p.base + STAGE_TABLE + s as usize * 4) as u64
}

/// main+0x781064: nn::gfx SetBlendState (`bs`: per-target blend states at [bs] (+0x25 of them), color +8, channel
/// mask +0xc, blend color +0x10).
unsafe fn set_blend_state(p: &mut Port, g: usize, bs: usize) {
    let cmd = p.m.r64(g + 0xb0) as u64;
    let arr = p.m.r64(bs);
    for i in 0..p.m.r8(bs + 0x25) as usize {
        p.call(p.pfn(PFN_BIND_BLEND_STATE), &[cmd, (arr + i * 8) as u64]);
    }
    p.call(p.pfn(PFN_BIND_CHANNEL_MASK_STATE), &[cmd, (bs + 0xc) as u64]);
    p.call(p.pfn(PFN_BIND_COLOR_STATE), &[cmd, (bs + 8) as u64]);
    p.call(p.pfn(PFN_SET_BLEND_COLOR), &[cmd, (bs + 0x10) as u64]);
}

/// main+0x78112c: nn::gfx SetDepthStencilState.
unsafe fn set_depth_stencil_state(p: &mut Port, g: usize, ds: usize) {
    let cmd = p.m.r64(g + 0xb0) as u64;
    p.call(got(p, GOT_BIND_DEPTH_STENCIL_STATE), &[cmd, ds as u64]);
    p.call(got(p, GOT_SET_STENCIL_VALUE_MASK), &[cmd, 3, p.m.r32(ds + 8) as u64]);
    p.call(got(p, GOT_SET_STENCIL_MASK), &[cmd, 3, p.m.r32(ds + 0xc) as u64]);
    p.call(got(p, GOT_SET_STENCIL_REF), &[cmd, 2, p.m.r32(ds + 0x10) as u64]);
    p.call(got(p, GOT_SET_STENCIL_REF), &[cmd, 1, p.m.r32(ds + 0x14) as u64]);
    if (p.m.r8(ds + 0x1a) >> 3) & 1 == 0 {
        let f: unsafe extern "C" fn(u64, u32, f32, f32) = std::mem::transmute(got(p, GOT_SET_DEPTH_BOUNDS));
        p.flush();
        f(cmd, 0, 0.0, 1.0);
    }
}

/// main+0x2f8ec: nn::gfx SetRasterizerState.
unsafe fn set_rasterizer_state(p: &mut Port, g: usize, rs: usize) {
    let cmd = p.m.r64(g + 0xb0) as u64;
    p.call(got(p, GOT_BIND_POLYGON_STATE), &[cmd, rs as u64]);
    let f: unsafe extern "C" fn(u64, f32, f32, f32) = std::mem::transmute(got(p, GOT_SET_POLYGON_OFFSET_CLAMP));
    p.flush();
    f(cmd, p.m.f32(rs + 0x28), p.m.f32(rs + 0x20), p.m.f32(rs + 0x24));
    p.call(got(p, GOT_BIND_MULTISAMPLE_STATE), &[cmd, (rs + 4) as u64]);
    let b = p.m.r8(rs + 0x2d) as u64;
    if b & 1 != 0 {
        p.call(got(p, GOT_SET_SAMPLE_MASK), &[cmd, p.m.r32(rs + 0x1c) as u64]);
    }
    let b = p.m.r8(rs + 0x2d) as u64;
    p.call(got(p, GOT_SET_DEPTH_CLAMP), &[cmd, !(b >> 1) & 1]);
    p.call(got(p, GOT_SET_RASTERIZER_DISCARD), &[cmd, !(p.m.r8(rs + 0x2d) as u64 >> 2) & 1]);
    if (p.m.r8(g + 1) >> 1) & 1 != 0 {
        p.call(got(p, GOT_SET_CONSERVATIVE_RASTER), &[cmd, (p.m.r8(rs + 0x2d) as u64 >> 3) & 1]);
    }
}

/// main+0x74ee70: the texture (descriptor slot *tex) with the sampler (slot *smp) bound to `slot` of `st`.
unsafe fn bind_texture(p: &mut Port, g: usize, slot: i32, st: u32, tex: usize, smp: usize) {
    let dev = p.m.r64(p.m.r64(g + 8)) as u64;
    let pfn = p.pfn(PFN_DEVICE_GET_TEXTURE_HANDLE);
    let handle = p.query(pfn, &[dev, p.m.r32(tex) as u64, p.m.r32(smp) as u64]);
    let cmd = p.m.r64(g + 0xb0) as u64;
    p.call(p.pfn(PFN_BIND_TEXTURE), &[cmd, stage(p, st), slot as u32 as u64, handle]);
}

/// main+0x30274: a uniform block (GPU address *addr) of `size` bytes bound to `slot` of `st`.
unsafe fn bind_uniform(p: &mut Port, g: usize, slot: i32, st: u32, addr: u64, size: u64) {
    let cmd = p.m.r64(g + 0xb0) as u64;
    p.call(p.pfn(PFN_BIND_UNIFORM_BUFFER), &[cmd, stage(p, st), slot as u32 as u64, addr, size]);
}

/// main+0x302e0: a storage buffer bound to `slot` of `st`.
unsafe fn bind_storage(p: &mut Port, g: usize, slot: i32, st: u32, addr: u64, size: u64) {
    let cmd = p.m.r64(g + 0xb0) as u64;
    p.call(p.pfn(PFN_BIND_STORAGE_BUFFER), &[cmd, stage(p, st), slot as u32 as u64, addr, size]);
}

/// main+0x7650a4: vertex buffer `index` at `addr`, `size` bytes.
unsafe fn bind_vertex_buffer(p: &mut Port, g: usize, index: i32, addr: u64, size: u64) {
    let cmd = p.m.r64(g + 0xb0) as u64;
    p.call(got(p, GOT_BIND_VERTEX_BUFFER), &[cmd, index as u32 as u64, addr, size]);
}

/// main+0x2f804(g, topology, count, first, instances, base instance).
unsafe fn draw_arrays(p: &mut Port, g: usize, topology: u32, count: u32, first: u32, instances: u32, base: u32) {
    let cmd = p.m.r64(g + 0xb0) as u64;
    let mode = p.m.r32(p.base + TOPOLOGY_TABLE + topology as usize * 4) as u64;
    p.call(got(p, GOT_DRAW_ARRAYS_INSTANCED), &[cmd, mode, first as u64, count as u64, base as u64, instances as u64]);
}

/// main+0x7650f4: vertex attribute and stream states.
unsafe fn bind_vertex_state(p: &mut Port, g: usize, s: usize) {
    let cmd = p.m.r64(g + 0xb0) as u64;
    p.call(p.pfn(PFN_BIND_VERTEX_ATTRIB_STATE), &[cmd, p.m.r32(s + 8) as u64, p.m.r64(s + 0x18) as u64]);
    p.call(p.pfn(PFN_BIND_VERTEX_STREAM_STATE), &[cmd, p.m.r32(s + 4) as u64, p.m.r64(s + 0x10) as u64]);
}

/// main+0x843c(sys, g, emitter, a3, params, cache): returns whether the emitter was drawn (sets its +0x30 bit 7).
pub(super) unsafe fn draw_emitter(p: &mut Port, sys: usize, g: usize, em: usize, a3: usize, dp: usize, cache: usize) -> R<bool> {
    let res = p.m.r64(em + 0x40);
    if p.m.r8(res + 0xb9) == 0 && p.m.r8(em + 0x8a) == 0x3f {
        return Ok(false);
    }
    // main+0x1854: the emitter's four callback sets (+0x370..), their +0x28 draw callback.
    let path = p.m.r8(dp + 0x2c) as usize;
    let info = Info { g, em, a3, dp, path: path as u8 };
    for k in 0..4 {
        let set = p.m.r64(em + 0x370 + k * 8);
        if set != 0 && p.m.r64(set + 0x28) != 0 && run_callback(p, p.m.r64(set + 0x28), &info)? {
            p.m.w::<u32>(em + 0x30, p.m.r32(em + 0x30) | 0x80);
            return Ok(true);
        }
    }
    if path == 3 {
        return Ok(false);
    }
    let sh = p.m.r64(res + 0x10 + path * 8);
    if sh == 0 || p.m.i32(dp + 0x24) != -1 {
        return Ok(false);
    }
    p.set_shader(g, p.m.r64(sh), 0x3f);
    if draw_shaded(p, sys, g, em, sh, a3, dp, cache)? {
        p.m.w::<u32>(em + 0x30, p.m.r32(em + 0x30) | 0x80);
        return Ok(true);
    }
    Ok(false)
}

/// main+0x8544.
#[allow(clippy::too_many_arguments)]
unsafe fn draw_shaded(p: &mut Port, sys: usize, g: usize, em: usize, sh: usize, a3: usize, dp: usize, cache: usize) -> R<bool> {
    let res = p.m.r64(em + 0x40);
    if p.m.i32(dp + 0x24) == -1 {
        set_blend_state(p, g, res + 0x1e8);
        set_depth_stencil_state(p, g, res + 0x210);
        set_rasterizer_state(p, g, res + 0x22c);
    }
    bind_textures(p, sys, g, em, sh, dp, cache);
    bind_blocks(p, g, em, sh);
    if !callbacks(p, em, g, p.m.r8(dp + 0x2c), a3, dp)? {
        return Ok(false);
    }
    let n = (p.m.i32(em + 0x98) as i64) << 4;
    let pool = p.m.r64(em + 0x658);
    let gpu = |p: &Port, cpu: usize| (cpu.wrapping_sub(p.m.r64(pool + 0x1a0)).wrapping_add(p.m.r64(pool + 8))) as u64;
    let bufs = p.m.r64(em + 0x70);
    let res = p.m.r64(em + 0x40);
    bind_vertex_buffer(p, g, p.m.i8(res + 0xa8) as i32, gpu(p, p.m.r64(bufs)), n as u64);
    bind_vertex_buffer(p, g, p.m.i8(res + 0xa9) as i32, gpu(p, p.m.r64(bufs + 8)), n as u64);
    for (o, b) in [(0xaa, 0x10), (0xac, 0x20), (0xab, 0x18), (0xad, 0x28), (0xae, 0x30), (0xaf, 0x38), (0xb0, 0x40), (0xb1, 0x48), (0xb2, 0x50), (0xb4, 0x58)] {
        let res = p.m.r64(em + 0x40);
        let i = p.m.i8(res + o);
        if i != -1 {
            let pool = p.m.r64(em + 0x658);
            let a = p.m.r64(p.m.r64(em + 0x70) + b).wrapping_sub(p.m.r64(pool + 0x1a0)).wrapping_add(p.m.r64(pool + 8));
            bind_vertex_buffer(p, g, i as i32, a as u64, n as u64);
        }
    }
    // Custom attributes 0..3: storage buffers from the emitter's attribute slots (+0x4d0, 0x40 apart).
    let layer = p.m.r8(em + 0x630) as usize;
    for k in 0..4usize {
        let slot = p.m.i8(sh + 0x46 + k);
        if slot == -1 {
            continue;
        }
        let a = em + 0x4d0 + k * 0x40;
        if p.m.r8(a) == 0 {
            return Err("vfx custom attribute not initialized");
        }
        let size = p.m.r64(a + 0x38) as u64;
        let addr = attribute_address(p, a, layer, p.m.r64(em + 0x658))?;
        bind_storage(p, g, slot as i32, 0, addr, size);
    }
    let prim = p.m.r64(res + 8);
    if prim != 0 {
        if p.m.r64(prim).wrapping_sub(p.base) != PRIMITIVE_VTABLE {
            return callback(p.m.r64(prim), p.base).map(|_| false);
        }
        // The primitive's vertex buffers (vtable +0x30 count, +0x20 address, +0x18 size).
        let d = p.m.r64(prim + 0x18);
        for i in 0..p.m.r8(d + 0x4d) as usize {
            let addr = p.gfx_buffer_address(p.m.r64(p.m.r64(d + 0x28) + i * 8));
            let size = p.m.r32(p.m.r64(d + 0x30) + i * 16) as u64;
            bind_vertex_buffer(p, g, i as i32, addr, size);
        }
    }
    if p.m.i32(dp + 0x24) == -1 {
        bind_vertex_state(p, g, p.m.r64(em + 0x40) + p.m.r8(dp + 0x2c) as usize * 0x20 + 200);
    }
    let lod = p.m.r8(em + 0xac) as i32;
    if p.m.r8(p.m.r64(em + 0x48) + 0xca1) != 0 {
        sorted(p, g, em, prim, dp, lod)?;
        return Ok(true);
    }
    let count = p.m.i32(em + 0x98) as u32;
    if (p.m.r8(em + 0x31) >> 4) & 1 == 0 {
        if prim != 0 {
            draw_primitive(p, prim, g, count, 0, lod);
        } else {
            draw_arrays(p, g, 4, 4, 0, count, 0);
        }
        return Ok(true);
    }
    // Ring of particles: two draws when the live range wraps.
    let cap = p.m.i32(em + 0xa0);
    let start = p.m.i32(em + 0x9c);
    let mut end = p.m.i32(em + 0xa4).wrapping_add(cap).wrapping_sub(1);
    let q = if cap != 0 { end.wrapping_div(cap) } else { 0 };
    end = end.wrapping_sub(q.wrapping_mul(cap));
    if end < start {
        if prim != 0 {
            draw_primitive(p, prim, g, end.wrapping_add(1) as u32, 0, lod);
            draw_primitive(p, prim, g, cap.wrapping_sub(start) as u32, start as u32, lod);
            return Ok(true);
        }
        draw_arrays(p, g, 4, 4, 0, end.wrapping_add(1) as u32, 0);
        draw_arrays(p, g, 4, 4, 0, cap.wrapping_sub(start) as u32, start as u32);
    } else if prim != 0 {
        draw_primitive(p, prim, g, end.wrapping_sub(start).wrapping_add(1) as u32, start as u32, lod);
    } else {
        draw_arrays(p, g, 4, 4, 0, end.wrapping_sub(start).wrapping_add(1) as u32, start as u32);
    }
    Ok(true)
}

/// The effect primitive class (vtable main+0x41a72b0).
const PRIMITIVE_VTABLE: usize = 0x41a72b0;

/// main+0x1f78c (primitive vtable +0x60): the LOD mesh `lod` (clamped to the last of [+0x20]+0x5b; meshes at
/// [+0x20]+0x18, 0x38 bytes) drawn `instances` times from `base` through main+0x758644.
unsafe fn draw_primitive(p: &mut Port, prim: usize, g: usize, instances: u32, base: u32, lod: i32) {
    let m = p.m.r64(prim + 0x20);
    let n = p.m.r8(m + 0x5b) as i32;
    let meshes = p.m.r64(m + 0x18);
    if meshes == 0 {
        return;
    }
    let k = if n > lod { lod } else { n - 1 };
    let mesh = (meshes as i64 + k as i64 * 0x38) as usize;
    let parts = p.m.r16(mesh + 0x34) as usize;
    let (addr, count) = p.range(mesh, 0, parts.wrapping_sub(1));
    let (pr, it, bv) = (p.m.r32(mesh + 0x24), p.m.r32(mesh + 0x28), p.m.i32(mesh + 0x30));
    p.draw_indexed(g, pr, it, addr, count, bv, instances as i32, base as i32);
}

/// main+0x7604: a custom attribute's GPU address (kinds 0, and 1 / 2 per layer).
unsafe fn attribute_address(p: &Port, a: usize, layer: usize, pool: usize) -> R<u64> {
    let kind = p.m.r8(a + 1);
    let e = match kind {
        0 => a,
        1 | 2 => a + layer * 8,
        _ => return Err("vfx custom attribute kind"),
    };
    Ok(p.m.r64(e + 0x18).wrapping_sub(p.m.r64(pool + 0x1a0)).wrapping_add(p.m.r64(pool + 8)) as u64)
}

/// main+0x7734: the resource's six textures (+0x48.., samplers +0x78..) to the shader's vertex / fragment slots, the
/// params' two textures and the global texture, through the binding cache (`cache`: six ints and an enable byte).
unsafe fn bind_textures(p: &mut Port, sys: usize, g: usize, em: usize, sh: usize, dp: usize, cache: usize) {
    for k in 0..6usize {
        let res = p.m.r64(em + 0x40);
        let t = p.m.r64(res + 0x48 + k * 8);
        if t == 0 {
            continue;
        }
        let tex = t + 0x90;
        if p.m.r64(tex) as i64 == -1 {
            continue;
        }
        let smp = p.m.r64(res + 0x78 + k * 8) + 0x18;
        let (vs, fs) = (p.m.i8(sh + 8 + k), p.m.i8(sh + 0x4a + k));
        if vs != -1 {
            bind_texture(p, g, vs as i32, 0, tex, smp);
        }
        if fs != -1 {
            bind_texture(p, g, fs as i32, 4, tex, smp);
        }
    }
    let use_cache = p.m.r8(cache + 0x18) != 0;
    let mut cached = |p: &mut Port, slot: i8, st: u32, ci: usize, tex: usize, smp: usize| {
        if slot == -1 {
            return;
        }
        if use_cache && p.m.i32(cache + ci * 4) == slot as i32 {
            return;
        }
        bind_texture(p, g, slot as i32, st, tex, smp);
        p.m.w::<i32>(cache + ci * 4, slot as i32);
    };
    let s1 = p.m.r64(sys + 8) + 0x18;
    for (k, o) in [(0usize, 0usize), (1, 8)] {
        let tex = dp + o;
        if p.m.r64(tex) as i64 == -1 {
            continue;
        }
        let (vs, fs) = (p.m.i8(sh + 0xe + k), p.m.i8(sh + 0x50 + k));
        cached(p, vs, 0, k * 2, tex, s1);
        cached(p, fs, 4, k * 2 + 1, tex, s1);
    }
    let gt = p.m.r64(p.base + GLOBAL_TEXTURES) + 0x260;
    let s0 = p.m.r64(sys) + 0x18;
    let (vs, fs) = (p.m.i8(sh + 0x10), p.m.i8(sh + 0x52));
    cached(p, vs, 0, 4, gt, s0);
    cached(p, fs, 4, 5, gt, s0);
}

/// main+0x7c1c: the emitter's three uniform blocks (view, resource, emitter) to the shader's vertex / fragment slots.
unsafe fn bind_blocks(p: &mut Port, g: usize, em: usize, sh: usize) {
    let pool = p.m.r64(em + 0x650);
    let a = p.m.r64(em + p.m.r8(em + 0x88) as usize * 8 + 0x638).wrapping_sub(p.m.r64(pool + 0x1a0)).wrapping_add(p.m.r64(pool + 0x188));
    let res = p.m.r64(em + 0x40);
    let pool_r = p.m.r64(res + 0x40);
    let b = p.m.r64(pool_r + 0x188).wrapping_add(p.m.r32(res + 1000) as usize);
    let c = p.m.r64(pool_r + 0x188).wrapping_add(p.m.r32(res + 0x3f4) as usize);
    for (addr, vs, fs, size) in [(a, 0x3b, 0x7d, 0x160u64), (b, 0x3a, 0x7c, 0xca0), (c, 0x3c, 0x7e, 0x160)] {
        let (v, f) = (p.m.i8(sh + vs), p.m.i8(sh + fs));
        if v != -1 {
            bind_uniform(p, g, v as i32, 0, addr as u64, size);
        }
        if f != -1 {
            bind_uniform(p, g, f as i32, 4, addr as u64, size);
        }
    }
}

/// main+0x48f0: the emitter's render callbacks; returns whether to draw.
unsafe fn callbacks(p: &mut Port, em: usize, _g: usize, path: u8, _a3: usize, dp: usize) -> R<bool> {
    let sys_cb = p.m.r64(p.m.r64(em) + 0x68);
    if sys_cb != 0 {
        let v = if path == 3 { 0 } else { p.m.r64(p.m.r64(em + 0x40) + path as usize * 8 + 0x10) };
        let e = p.m.r64(sys_cb + 0x98) + p.m.i32(dp + 0x20) as i64 as usize * 0x828;
        p.m.w::<u64>(e + 0x800, v as u64);
        p.m.w::<u8>(e + 0x820, 10);
    }
    let info = Info { g: _g, em, a3: _a3, dp, path };
    for o in [0x370usize, 0x380] {
        let set = p.m.r64(em + o);
        if set != 0 && p.m.r64(set + 0x50) != 0 && !run_callback(p, p.m.r64(set + 0x50), &info)? {
            return Ok(false);
        }
    }
    let set = p.m.r64(em + 0x388);
    if set != 0 && p.m.r64(set + 0x50) != 0 {
        if !run_callback(p, p.m.r64(set + 0x50), &info)? {
            return Ok(false);
        }
    } else if p.m.r64(p.m.r64(em + 0x40) + 0x3b8) != 0 {
        custom_block(p, &info);
    }
    let bit = 1u32.wrapping_shl(p.m.r32(em + 0x38) & 31);
    let t = p.m.r64(em + 0x390);
    if let Some(k) = (0..8usize).find(|&k| p.m.r32(t + k * 4) & bit != 0) {
        let f = p.m.r64(t + 0x20 + k * 8);
        if f != 0 {
            run_callback(p, f, &info)?;
        }
    }
    if sys_cb != 0 {
        let e = p.m.r64(sys_cb + 0x98) + p.m.i32(dp + 0x20) as i64 as usize * 0x828;
        p.m.w::<u64>(e + 0x800, 0);
        p.m.w::<u8>(e + 0x820, 10);
    }
    Ok(true)
}

const TLS_CORE_SLOT: usize = 0x462af70;
const GET_TLS_VALUE: usize = 0x2b15be0;
const CURRENT_CORE: usize = 0x2b15c10;
const SCENE_GLOBALS: usize = 0x462bef8;
const LIST_AT: usize = 0xec6c4c;
const PFN_SET_CHANNEL_MASK: usize = 0x466dd10;
const PFN_BIND_CHANNEL_MASK_STATE_: usize = 0x466df78;

thread_local! {
    /// Uniform blocks the prediction filled (CPU address, bytes), checked against the game's after its call.
    pub(super) static BLOCKS: std::cell::RefCell<Vec<(usize, Vec<u8>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The predicted uniform blocks against the game's: None when equal.
pub(super) unsafe fn check_blocks() -> Option<String> {
    let v = BLOCKS.with(|b| std::mem::take(&mut *b.borrow_mut()));
    for (a, bytes) in v {
        let real = std::slice::from_raw_parts(a as *const u8, bytes.len());
        if real != bytes.as_slice() {
            let i = (0..bytes.len()).find(|&i| real[i] != bytes[i]).unwrap_or(0);
            return Some(format!("vfx uniform block {a:#x}: byte {i:#x} port {:#x} game {:#x}", bytes[i], real[i]));
        }
    }
    None
}

pub(super) fn discard_blocks() {
    BLOCKS.with(|b| b.borrow_mut().clear());
}

/// The current thread's core index (TLS value - 1, else the current core).
unsafe fn core(p: &Port) -> i32 {
    let v = p.query(p.base + GET_TLS_VALUE, &[p.m.r32(p.m.r64(p.base + TLS_CORE_SLOT)) as u64]);
    if v == 0 {
        p.query(p.base + CURRENT_CORE, &[]) as i32
    } else {
        (v as i32).wrapping_sub(1)
    }
}

/// main+0x68b66c / main+0x2ebb8: `size` bytes from the core's bump allocator of `a` (into the overlay): (CPU address,
/// GPU address).
unsafe fn ring_alloc(p: &mut Port, a: usize, core: i32, size: usize) -> Option<(usize, u64)> {
    if core >= p.m.i32(a + 0x2f38) {
        return None;
    }
    let r = p.m.r64(a + 0x98) + core as i64 as usize * 0x828;
    if p.m.r8(r + 0x52d) == 0 || size == 0 {
        return None;
    }
    let i = p.m.i32(r + 0x528) as i64;
    let e = (r as i64 + i * 0x1a8) as usize;
    let align = p.m.r64(e + 0x168);
    let step = (align.wrapping_add(size - 1)) & align.wrapping_neg();
    let off = p.m.r64(e + 0x198);
    let end = off.wrapping_add(step);
    if end > p.m.r64(e + 0x170) {
        return None;
    }
    p.m.w::<u64>(e + 0x198, end as u64);
    let base = p.m.r64(e + 0x1a0);
    if base == 0 {
        return None;
    }
    let cpu = base.wrapping_add(off);
    let gpu = cpu.wrapping_sub(base).wrapping_add(p.m.r64(e + 0x188)) as u64;
    let stat = (r as i64 + i * 8) as usize + 0x508;
    p.m.w::<u64>(stat, p.m.r64(stat).wrapping_add(step) as u64);
    Some((cpu, gpu))
}

/// main+0x7c4cb0: a texture of the effect system's per-core shader (slot `k`: vertex +0x11, fragment +0x53), when the
/// texture is ready (a pending load is finished by the game: not predicted).
unsafe fn effect_texture(p: &mut Port, w: usize, sys_cb: usize, k: usize, tex: usize) -> R<()> {
    if tex == 0 {
        return Ok(());
    }
    if p.m.r32(tex + 0xe4) != 0 {
        return Err("vfx texture still loading");
    }
    let c = core(p);
    let sh = p.m.r64(p.m.r64(sys_cb + 0x98) + c as i64 as usize * 0x828 + 0x800);
    if sh == 0 {
        return Ok(());
    }
    let e = sh + k;
    let id = [p.m.i32(tex + 0x58) as u32, 0];
    let smp = [p.m.r16(tex + 0xea) as i16 as i32 as u32, 0];
    let (v, f) = (p.m.i8(e + 0x11), p.m.i8(e + 0x53));
    if v != -1 {
        bind_texture(p, w + 8, v as i32, 0, id.as_ptr() as usize, smp.as_ptr() as usize);
    }
    if f != -1 {
        bind_texture(p, w + 8, f as i32, 4, id.as_ptr() as usize, smp.as_ptr() as usize);
    }
    Ok(())
}

/// main+0x7c4440 (TotK's CallbackRenderStateSet_Basic2): the custom block, effect textures, a 0xd0-byte block of
/// per-emitter parameters from the core's allocator bound at the shader's third block slots, the emitter's
/// previous position updated. `alt`: main+0xd61360, the same without the slot 8 texture, with word 0x27 set from
/// the source's +0xb bit 5 and its own channel mask.
unsafe fn render_state_basic2(p: &mut Port, i: &Info, alt: bool) -> R<bool> {
    if i.path == 3 {
        return Ok(true);
    }
    let em = i.em;
    let ud = p.m.r64(em + 0x3a0);
    if ud == 0 || ud == 0xdead_beef {
        return Err("vfx render state without user data");
    }
    let sys_cb = p.m.r64(p.m.r64(em) + 0x68);
    let a3 = i.a3;
    let res = p.m.r64(em + 0x40);
    let block_sh = p.m.r64(res + 0x10);
    custom_block(p, i);
    let d8 = p.m.r64(a3 + 0xd8);
    let l14 = p.m.r64(d8 + 0x100);
    let fl = p.m.r64(em + 0x48);
    let u13 = p.m.r64(fl + 0xe78) as u64;
    let u19 = p.m.r64(fl + 0xe80) as u64;
    let w = p.m.r64(a3 + 0xb0);
    if u13 & 1 != 0 {
        let mut k = 0usize;
        let cs = p.m.r64(res + 0x3b8);
        if cs != 0 {
            k = (p.m.f32(cs + 0x28) as u32).min(7) as usize;
        }
        let t = p.m.r64(d8 + k * 8 + 0x18);
        if t != 0 {
            effect_texture(p, w, sys_cb, 0x18, t)?;
        }
    }
    let c = core(p);
    if let Some((blk, gpu)) = ring_alloc(p, sys_cb, c, 0xd0) {
        let mut b = [0u32; 0x34];
        let wf = |b: &mut [u32; 0x34], k: usize, f: f32| b[k] = f.to_bits();
        let globals = p.m.r64(p.m.r64(p.base + SCENE_GLOBALS));
        if (p.m.i32(fl + 0xcb4).wrapping_sub(0x11) as u32) < 2 {
            wf(&mut b, 0, 1.0);
        }
        if (u13 >> 14) & 1 != 0 {
            let (mut x, mut z, mut len);
            let src = p.m.r64(p.m.r64(em) + 0x98);
            if src == 0 {
                x = p.m.f32(globals + 0x418);
                z = p.m.f32(globals + 0x420);
                len = p.m.f32(globals + 0x414);
            } else {
                let o = p.m.r64(src);
                let v = if o != 0 { o + 0x38 } else { 0 };
                x = p.m.f32(v);
                let y = p.m.f32(v + 4);
                z = p.m.f32(v + 8);
                let sq = x * x + y * y + z * z;
                let n = sq.sqrt();
                len = n;
                if n <= 9.094947e-14 {
                    x = 0.0;
                    z = 0.0;
                } else if n == f32::INFINITY {
                    let k = 3.1298223e-20f32;
                    x *= k;
                    z *= k;
                    let yk = y * k;
                    let n2 = (x * x + yk * yk + z * z).sqrt();
                    x *= 1.0 / n2;
                    z *= 1.0 / n2;
                } else {
                    x *= 1.0 / n;
                    z *= 1.0 / n;
                }
            }
            if 10.0 < len {
                len = 10.0;
            }
            b[2] = p.m.r32(ud);
            b[3] = p.m.r32(ud + 4);
            wf(&mut b, 4, x);
            b[5] = 0;
            wf(&mut b, 6, z);
            wf(&mut b, 7, len * 0.2);
        }
        if l14 != 0 {
            if (u13 >> 15) & 1 != 0 && p.m.r64(l14 + 0xd8) != 0 {
                effect_texture(p, w, sys_cb, 2, p.m.r64(l14 + 0xd8))?;
            }
            if u13 & 0x600000 != 0 && p.m.r64(l14 + 0xe0) != 0 {
                effect_texture(p, w, sys_cb, 3, p.m.r64(l14 + 0xe0))?;
            }
            if !alt && (u19 >> 32) & 1 != 0 && p.m.r64(l14 + 0xf8) != 0 {
                effect_texture(p, w, sys_cb, 8, p.m.r64(l14 + 0xf8))?;
            }
            if (u13 & 0xc0000 != 0 || u19 & 0x4_0000_0000 != 0) && p.m.r64(l14 + 0xe8) != 0 {
                let l6 = p.m.r64(d8 + 8);
                if l6 != 0 {
                    let e0 = p.m.r64(a3 + 0xe0);
                    if e0 == 0 || (p.m.r8(e0 + 0x41) >> 1) & 1 == 0 {
                        effect_texture(p, w, sys_cb, 3, p.m.r64(l6 + 0xd8))?;
                    } else {
                        return Err("vfx deferred textures");
                    }
                }
            }
        }
        let q = p.m.r64(ud + 0x10);
        if (u13 >> 0x38) & 1 != 0 && q != 0 {
            b[8] = p.m.r32(q + 0xc);
            b[9] = p.m.r32(q + 0x10);
            b[10] = p.m.r32(q + 0x14);
            b[0xb] = 0;
            wf(&mut b, 0xc, p.m.f32(q) - p.m.f32(q + 0xc));
            wf(&mut b, 0xd, p.m.f32(q + 4) - p.m.f32(q + 0x10));
            wf(&mut b, 0xe, p.m.f32(q + 8) - p.m.f32(q + 0x14));
        }
        if (u13 >> 24) & 1 != 0 {
            b[0xb] = p.m.r32(globals + 0x310);
        }
        if (u13 >> 0x33) & 1 != 0 && l14 != 0 {
            for k in 0..8 {
                b[0x10 + k] = p.m.r32(l14 + 0x180 + k * 4);
            }
            wf(&mut b, 0x11, if p.m.r8(ud + 0x24) != 0 { 1.0 } else { 0.0 });
        }
        if u13 & 0x600000000000000 != 0 || u19 & 0x4000 != 0 {
            b[0x19] = 0;
            b[0x1a] = 0;
            b[0x1b] = 0;
            b[0x18] = p.m.r32(ud + 8);
        }
        let (px, py, pz) = (p.m.f32(em + 0x170), p.m.f32(em + 0x174), p.m.f32(em + 0x178));
        let dx = px - p.m.f32(ud + 0x18);
        let dy = py - p.m.f32(ud + 0x1c);
        let dz = pz - p.m.f32(ud + 0x20);
        b[0x27] = 0;
        wf(&mut b, 0x26, dz);
        let still = dz == 0.0 && dy == 0.0 && dx == 0.0;
        wf(&mut b, 0x24, dx);
        wf(&mut b, 0x25, if still { 1e-5 } else { dy });
        p.m.w::<f32>(ud + 0x20, pz);
        p.m.w::<u64>(ud + 0x18, p.m.r64(em + 0x170) as u64);
        let m = p.m.r64(p.m.r64(em) + 0x98);
        if m != 0 {
            if alt && (p.m.r8(m + 0xb) >> 5) & 1 == 0 {
                wf(&mut b, 0x27, 1.0);
            }
            for k in 0..8 {
                b[0x1c + k] = p.m.r32(m + 0x20 + k * 4);
            }
            if u19 & 1 != 0 {
                let v = p.m.f32(m + 0x1c) + f32::from_bits(b[0xf]);
                wf(&mut b, 0xf, v);
            }
            let bones = p.m.r64(m + 0x48);
            if (u19 >> 0x16) & 1 != 0 && bones != 0 {
                for k in 0..3usize {
                    let r = p.query(p.base + LIST_AT, &[(bones + 0x180) as u64, k as u64]) as usize;
                    let e = if r != 0 { (r as i64 - p.m.i32(bones + 0x194) as i64) as usize } else { 0 };
                    for (j, o) in [0x1c, 0x2c, 0x3c, 0x50].iter().enumerate() {
                        b[0x28 + k * 4 + j] = p.m.r32(e + o);
                    }
                }
            }
        }
        for (k, v) in b.iter().enumerate() {
            p.m.w::<u32>(blk + k * 4, *v);
        }
        let bytes: Vec<u8> = b.iter().flat_map(|v| v.to_le_bytes()).collect();
        BLOCKS.with(|bl| bl.borrow_mut().push((blk, bytes)));
        // main+0x746c30: the block to the shader's vertex / fragment block slots 2 (+0x41 / +0x83).
        let (v, f) = (p.m.i8(block_sh + 0x41), p.m.i8(block_sh + 0x83));
        if v != -1 {
            bind_uniform(p, w + 8, v as i32, 0, gpu, 0xd0);
        }
        if f != -1 {
            bind_uniform(p, w + 8, f as i32, 4, gpu, 0xd0);
        }
    }
    if alt {
        channel_mask(p, w + 8, if p.m.i32(a3 + 0xbc) == 0x19 { 0xf0f0f1 } else { 0xf0f0ff });
        if p.m.i32(a3 + 0xc4) == 4 {
            return Err("vfx effect pass 4 state");
        }
    } else if p.m.i32(a3 + 0xbc) == 0x18 {
        channel_mask(p, w + 8, 0xf0f0fd);
    }
    Ok(true)
}

/// main+0xd61a60: a channel mask state from 4 bits per target, bound.
unsafe fn channel_mask(p: &mut Port, g: usize, mask: u32) {
    let mut st = [0u64; 4];
    let sp = st.as_mut_ptr() as u64;
    for t in 0..8u64 {
        let m = mask >> (t * 4);
        p.query(p.pfn(PFN_SET_CHANNEL_MASK), &[sp, t, (m & 1) as u64, ((m >> 1) & 1) as u64, ((m >> 2) & 1) as u64, ((m >> 3) & 1) as u64]);
    }
    let cmd = p.m.r64(g + 0xb0) as u64;
    p.call(p.pfn(PFN_BIND_CHANNEL_MASK_STATE_), &[cmd, sp]);
    let _ = &mut st;
}

const RENDER_GLOBALS: usize = 0x462c4a8;

/// main+0x780f80: the blend state the emitter's +0x48 mode (+0xdee) selects (0 / 1 / 4: render globals +0x288 /
/// +0x2b0 / +0x2d8), the scene's depth-stencil state when its flags ask for it, and for emitter kind 0x1a the scene's
/// blend state +0x9eb8 (main+0x781118).
unsafe fn render_state_blend(p: &mut Port, i: &Info) {
    let w8 = p.m.r64(i.a3 + 0xb0) + 8;
    let rg = p.m.r64(p.m.r64(p.base + RENDER_GLOBALS));
    match p.m.r8(p.m.r64(i.em + 0x48) + 0xdee) {
        0 => set_blend_state(p, w8, rg + 0x288),
        1 => set_blend_state(p, w8, rg + 0x2b0),
        4 => set_blend_state(p, w8, rg + 0x2d8),
        _ => {}
    }
    let sg = p.m.r64(p.m.r64(p.base + SCENE_GLOBALS));
    if (p.m.r8(sg + 0x2ec) >> 2) & 1 != 0 && (p.m.r8(sg + 0x2f1) >> 4) & 1 != 0 {
        set_depth_stencil_state(p, w8, sg + 0x9e9c);
    }
    if p.m.i32(i.em + 0x38) == 0x1a {
        let sg = p.m.r64(p.m.r64(p.base + SCENE_GLOBALS));
        set_blend_state(p, w8, sg + 0x9eb8);
    }
}

const SORT_PARTICLES: usize = 0x43f4;

/// main+0x8d30: particles drawn one by one in the order main+0x43f4 sorts them into the emitter's per-core sort
/// buffer (called: deterministic, it writes only that buffer, which the game's own call rewrites the same), the live
/// ones (age at most the life, life positive, nonzero) only.
unsafe fn sorted(p: &mut Port, g: usize, em: usize, prim: usize, dp: usize, lod: i32) -> R<()> {
    let kind = p.m.r8(p.m.r64(em + 0x48) + 0xca1);
    if kind == 4 {
        return Err("vfx custom particle sort");
    }
    let mut list = 0u64;
    let mut n = 0u32;
    let t = p.m.f32(em + 0x28) - p.m.f32(em + 0x2c);
    p.query_f(
        p.base + SORT_PARTICLES,
        &[em as u64, (&mut list) as *mut u64 as u64, (&mut n) as *mut u32 as u64, kind as u64, p.m.i32(dp + 0x20) as u32 as u64, p.m.r64(dp + 0x10) as u64],
        &[t],
    );
    for k in 0..n as i32 as i64 {
        if k < 0 {
            break;
        }
        let idx = p.m.i32(list as usize + k as usize * 8 + 4);
        let pt = (p.m.r64(em + 0x68) as i64 + idx as i64 * 0x20) as usize;
        let life = p.m.f32(pt + 0xc);
        if !(p.m.f32(em + 0x28) - p.m.f32(pt + 8) <= life) || p.m.r32(pt) == 0 || !(life > 0.0) {
            continue;
        }
        if prim != 0 {
            draw_primitive(p, prim, g, 1, idx as u32, lod);
        } else {
            draw_arrays(p, g, 4, 4, 0, 1, idx as u32);
        }
    }
    Ok(())
}
