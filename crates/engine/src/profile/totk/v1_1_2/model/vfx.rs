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
    cache: usize,
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
        0x2e24c => repeated_draw(p, i).map(|_| true),
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
    let info = Info { g, em, a3, dp, cache, path: path as u8 };
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
    let info = Info { g: _g, em, a3: _a3, dp, cache: 0, path };
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
    bump(p, p.m.r64(a + 0x98) + core as i64 as usize * 0x828, size)
}

/// main+0x2ebb8: `size` bytes from bump allocator `r` (into the overlay): (CPU address, GPU address).
unsafe fn bump(p: &mut Port, r: usize, size: usize) -> Option<(usize, u64)> {
    bump_into(p, r, size, 0)
}

/// main+0x2ebb8 with its GPU address struct `out` (in game memory: written to the overlay) when nonzero.
unsafe fn bump_into(p: &mut Port, r: usize, size: usize, out: usize) -> Option<(usize, u64)> {
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
    if out != 0 {
        p.m.w::<u64>(out, gpu);
        p.m.w::<u64>(out + 8, p.m.r64(e + 0x190) as u64);
    }
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
    // The sort reads the view's depth row (+0x20..0x30) behind dp+0x10, which may be data the prediction wrote (the
    // core's view constants): a copy through the overlay.
    let v = p.m.r64(dp + 0x10);
    let view: Vec<u8> = (0..0x30).map(|i| p.m.r8(v + i)).collect();
    p.query_f(
        p.base + SORT_PARTICLES,
        &[em as u64, (&mut list) as *mut u64 as u64, (&mut n) as *mut u32 as u64, kind as u64, p.m.i32(dp + 0x20) as u32 as u64, view.as_ptr() as u64],
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

/// main+0x2e24c -> main+0x2e27c: the emitter drawn (main+0x8544) (int)([res+0x128]+0xc + 1) times, each with its own
/// 0xc0-byte uniform block from the params' bump allocator (+0x18) bound at the shader's vertex / fragment slots
/// +0x3d / +0x7f. The block's contents (per-repeat transforms) are the game's: the prediction checks the commands,
/// not the data.
unsafe fn repeated_draw(p: &mut Port, i: &Info) -> R<()> {
    if i.path == 3 {
        return Ok(());
    }
    let em = i.em;
    let res = p.m.r64(em + 0x40);
    let sh = p.m.r64(res + i.path as usize * 8 + 0x10);
    if sh == 0 || p.m.i32(i.dp + 0x24) != -1 {
        return Ok(());
    }
    let t = p.m.r64(res + 0x128);
    p.set_shader(i.g, p.m.r64(sh), 0x3f);
    let n = (p.m.f32(t + 0xc) + 1.0) as i32;
    let sys = p.m.r64(em + 0x50);
    for _ in 0..n.max(0) {
        let Some((_, gpu)) = bump(p, p.m.r64(i.dp + 0x18), 0xc0) else { return Ok(()) };
        let (v, f) = (p.m.i8(sh + 0x3d), p.m.i8(sh + 0x7f));
        if v != -1 {
            bind_uniform(p, i.g, v as i32, 0, gpu, 0xc0);
        }
        if f != -1 {
            bind_uniform(p, i.g, f as i32, 4, gpu, 0xc0);
        }
        draw_shaded(p, sys, i.g, em, sh, i.a3, i.dp, i.cache)?;
    }
    Ok(())
}

const VIEW_CONSTANTS: usize = 0xa426f8;
const DEPTH_KEY: usize = 0x167d0;
const SORT_SETS: usize = 0x296c0;
const TICK: usize = 0x2b15810;
const EXTRA_BLOCK_SLOTS: usize = 0x37ff704;
const DRAW_HOOK: usize = 0x74b610;
const VIEW_SCALE: usize = 0x463a288;
const PFN_BIND_TEXTURES_DEFERRED: usize = 0x466e018;

/// Copies `n` bytes into the overlay (and keeps them for the data check when `check`).
unsafe fn copy_into(p: &mut Port, dst: usize, src: &[u8], check: bool) {
    for (k, b) in src.iter().enumerate() {
        p.m.w::<u8>(dst + k, *b);
    }
    if check {
        BLOCKS.with(|bl| bl.borrow_mut().push((dst, src.to_vec())));
    }
}

/// main+0xa41a38(sys0, g, groups, n, mask, a3, flags): the effect system's draw for one view: the view constants
/// (main+0xa426f8, called) uploaded with the per-core extra blocks (main+0xa42240), the emitter sets of the groups
/// collected with their depth keys (main+0xa41e1c), sorted and drawn (main+0xa4203c).
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn draw_view(p: &mut Port, sys0: usize, g: usize, groups: usize, n: u32, mask: u32, a3: usize, flags: u32) -> R<()> {
    if (p.m.r8(sys0 + 0x378) >> 1) & 1 != 0 {
        return Ok(());
    }
    let c = core(p);
    let ci = c as i64 as usize;
    let timed = p.m.r8(sys0 + ci + 0x194c) == 0;
    if timed {
        p.m.w::<u32>(sys0 + ci * 4 + 0x1940, 2);
        let t = p.query(p.base + TICK, &[]);
        p.m.w::<u64>(sys0 + ci * 8 + 0x1928, t);
    }
    let s = p.m.r64(sys0 + 0x18);
    let r = p.m.r64(s + 0x98) + ci * 0x828;
    if c < p.m.i32(s + 0x2f38) {
        p.m.w::<u32>(r + 0x81c, 1u32.wrapping_shl(p.m.r32(a3 + 0xb8) & 31));
    }
    if p.m.i32(sys0 + 0x3b8) != -1 {
        return Err("vfx view path override");
    }
    let v = p.m.r32(a3 + 0xc0);
    if v != 0 && c < p.m.i32(s + 0x2f38) && (v & 0xff) < 3 {
        p.m.w::<u8>(r + 0x821, v as u8);
    }
    let cb = p.m.r64(sys0 + 0x380);
    let mut unknown = 0usize;
    if cb == p.base + VIEW_TEXTURES {
        unknown = view_textures(p, s, a3)?;
    } else if cb != 0 {
        return callback(cb, p.base);
    }
    let g8 = g + 8;
    if (p.m.r8(sys0 + 0x37a) >> 1) & 1 != 0 {
        return Err("vfx view constants (other path)");
    }
    // The view (3x4 at a3+0, transposed with a zero row), the projection (4x4 at a3+0x30, transposed), a vector
    // (a3+0x70, w from +0x78) and five scalars.
    let f = |o: usize| p.m.r32(a3 + o);
    let mut va = [0u32; 16];
    let mut vb = [0u32; 16];
    for k in 0..4 {
        for row in 0..3 {
            va[k * 4 + row] = f(row * 16 + k * 4);
        }
        for row in 0..4 {
            vb[k * 4 + row] = f(0x30 + row * 16 + k * 4);
        }
    }
    let vc = [f(0x70), f(0x74), f(0x78), 0];
    let mut out = vec![0u8; 0x300];
    let scale = p.m.f32(p.m.r64(p.base + VIEW_SCALE));
    p.query_f(
        p.base + VIEW_CONSTANTS,
        &[out.as_mut_ptr() as u64, va.as_ptr() as u64, vb.as_ptr() as u64, vc.as_ptr() as u64],
        &[p.m.f32(a3 + 0x84), p.m.f32(a3 + 0x88), p.m.f32(s + 0x4000), p.m.f32(a3 + 0x90), scale],
    );
    // main+0xa42240(s, core, g8, constants, 1).
    copy_into(p, r + 0x530, &out[..0x250], false);
    if let Some((blk, gpu)) = bump_into(p, r, 0x250, r + 0x780) {
        let q = |p: &Port, o: usize| p.m.r64(s + o).to_le_bytes();
        for (dst, src) in [(0x200, 0x7c), (0x208, 0x84), (0x210, 0x910), (0x218, 0x918), (0x220, 0x920), (0x228, 0x928), (0x230, 0x930), (0x238, 0x938), (0x240, 0x940), (0x248, 0x948)] {
            out[dst..dst + 8].copy_from_slice(&q(p, src));
        }
        copy_into(p, blk, &out[..0x250], true);
        for st in [0, 4, 5] {
            bind_uniform(p, g8, 5, st, gpu, 0x250);
        }
    }
    let extra = p.m.r64(s + 0x8c8) + ci * 0xc0;
    for k in 0..6usize {
        let src = p.m.r64(extra + k * 8);
        if src == 0 {
            continue;
        }
        let size = p.m.r64(extra + 0x30 + k * 8);
        if let Some((blk, gpu)) = bump_into(p, r, size, r + 0x790 + k * 0x10) {
            let bytes: Vec<u8> = (0..size).map(|i| p.m.r8(src + i)).collect();
            copy_into(p, blk, &bytes, src != unknown);
            let slot = p.m.r32(p.base + EXTRA_BLOCK_SLOTS + k * 4) as i32;
            for st in [0, 4, 5] {
                bind_uniform(p, g8, slot, st, gpu, size as u64);
            }
        }
    }
    let consts = out.as_ptr() as usize;
    for k in 0..n.min(i32::MAX as u32) as usize {
        collect(p, s, c, p.m.i32(groups + k * 4), mask, sys0 + 0x370, consts)?;
    }
    if flags & 1 != 0 {
        collect(p, s, c, 0x3f, mask, sys0 + 0x370, consts)?;
    }
    p.m.w::<u64>(s + 0x968, (p.base + DRAW_HOOK) as u64);
    draw_sets(p, s, c, g8, sys0 + 0x1998, a3)?;
    if c < p.m.i32(s + 0x2f38) {
        p.m.w::<u8>(r + 0x821, 0);
    }
    if timed {
        let t = p.query(p.base + TICK, &[]);
        let slot = sys0 + p.m.i32(sys0 + ci * 4 + 0x1940) as i64 as usize * 8 + 0x18d8;
        let start = p.m.r64(sys0 + ci * 8 + 0x1928) as u64;
        p.m.w::<u64>(slot, (t.wrapping_sub(start)).wrapping_add(p.m.r64(slot) as u64));
        p.m.w::<u32>(sys0 + ci * 4 + 0x1940, u32::MAX);
    }
    Ok(())
}

/// main+0xa41e1c(s, core, group, mask, filter): the group's emitter sets in the view (core's view bits +0x81c, the
/// mask, the filter, no owner +0x28) appended to the core's draw list with their sort keys: the system's key
/// callback (+0x990; TotK's main+0x26a7c00, distance from the camera, called with `consts`, a copy of the core's view
/// constants +0x530), else the depth along the view.
#[allow(clippy::too_many_arguments)]
unsafe fn collect(p: &mut Port, s: usize, c: i32, group: i32, mask: u32, filter: usize, consts: usize) -> R<()> {
    let mut set = p.m.r64((s as i64 + group as i64 * 8) as usize + 0xb8);
    if set == 0 {
        return Ok(());
    }
    let key_cb = p.m.r64(s + 0x990);
    if key_cb != 0 && key_cb != p.base + DISTANCE_KEY {
        return callback(key_cb, p.base);
    }
    let r = p.m.r64(s + 0x98) + c as i64 as usize * 0x828;
    while set != 0 {
        let next = p.m.r64(set);
        if p.m.r32(r + 0x81c) & p.m.r32(set + 0x34) != 0
            && p.m.r32(set + 0x30) & mask != 0
            && p.m.r64(filter) & p.m.r64(set + 0x20) != 0
            && p.m.r64(set + 0x28) as i64 == -1
        {
            let list = p.m.r64(r + 0x808);
            let n = p.m.i32(r + 0x814) as i64;
            let e = (list as i64 + n * 16) as usize;
            p.m.w::<u64>(e, set as u64);
            if key_cb != 0 {
                let key = p.query(key_cb, &[set as u64, consts as u64]) as u32;
                p.m.w::<u32>(e + 8, key);
                p.m.w::<i32>(r + 0x814, n as i32 + 1);
                set = next;
                continue;
            }
            let a = [p.m.f32(set + 0x130), p.m.f32(set + 0x134), p.m.f32(set + 0x138), p.m.f32(set + 0x13c)];
            let b = [p.m.f32(r + 0x550), p.m.f32(r + 0x554), p.m.f32(r + 0x558), 0.0];
            let m = [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]];
            let mut d = (m[0] + m[2]) + (m[1] + m[3]);
            d += p.m.f32(r + 0x55c);
            d = p.m.f32(p.m.r64(p.m.r64(set + 0x10)) + 0xa4) + d;
            d = p.m.f32(set + 0x260) + d;
            d = super::grid::fmin(d, 0.0);
            let key = p.query_f(p.base + DEPTH_KEY, &[], &[d]) as u32 | (p.m.r8(set + 0x66) as u32) << 24;
            p.m.w::<u32>(e + 8, key);
            p.m.w::<i32>(r + 0x814, n as i32 + 1);
        }
        set = next;
    }
    p.m.w::<u32>(r + 0x818, p.m.r32(r + 0x818) | mask);
    Ok(())
}

/// The core's view state block (s+0xa0, 0x30 bytes per core) refreshed from its per-core state.
unsafe fn view_state(p: &mut Port, s: usize, c: i32) -> usize {
    if c >= p.m.i32(s + 0x2f38) {
        return 0;
    }
    let r = p.m.r64(s + 0x98) + c as i64 as usize * 0x828;
    let st = p.m.r64(s + 0xa0) + c as i64 as usize * 0x30;
    p.m.w::<u32>(st + 0x28, p.m.r32(r + 0x81c));
    p.m.w::<u8>(st + 0x2c, p.m.r8(r + 0x821));
    p.m.w::<u32>(st + 0x24, p.m.r32(r + 0x810));
    p.m.w::<u64>(st, p.m.r64(r + 0x7f0) as u64);
    p.m.w::<u64>(st + 8, p.m.r64(r + 0x7f8) as u64);
    st
}

/// main+0xa4203c(s, core, g8, cache, 1, a3): the core's draw list sorted (main+0x296c0, run on a copy) and each set
/// drawn (main+0xa3a8); the list and the view's group bits cleared.
unsafe fn draw_sets(p: &mut Port, s: usize, c: i32, g8: usize, cache: usize, a3: usize) -> R<()> {
    let r = p.m.r64(s + 0x98) + c as i64 as usize * 0x828;
    let n = p.m.i32(r + 0x814);
    if n != 0 {
        let list = p.m.r64(r + 0x808);
        let mut v: Vec<[u64; 2]> = (0..n.max(0) as usize).map(|k| [p.m.r64(list + k * 16) as u64, p.m.r64(list + k * 16 + 8) as u64]).collect();
        let functor = [0u64; 2];
        let b = v.as_mut_ptr() as u64;
        p.query(p.base + SORT_SETS, &[b, b + v.len() as u64 * 16, functor.as_ptr() as u64]);
        for (k, e) in v.iter().enumerate() {
            p.m.w::<u64>(list + k * 16, e[0]);
            p.m.w::<u64>(list + k * 16 + 8, e[1]);
        }
        let st = view_state(p, s, c);
        let mut k = 0usize;
        while (k as i64) < p.m.i32(r + 0x814) as i64 {
            let set = p.m.r64(p.m.r64(r + 0x808) + k * 16);
            k += 1;
            draw_set(p, s, set, g8, p.m.r32(r + 0x818), a3, st, cache)?;
        }
        p.m.w::<i32>(r + 0x814, 0);
    }
    p.m.w::<u32>(r + 0x818, 0);
    Ok(())
}

/// main+0xa3a8: a set's emitters (from +8, next +8), each through main+0xa238.
#[allow(clippy::too_many_arguments)]
unsafe fn draw_set(p: &mut Port, s: usize, set: usize, g8: usize, prio: u32, a3: usize, st: usize, cache: usize) -> R<()> {
    if p.m.r8(p.m.r64(set + 0x10) + 0x23) == 0 && p.m.r8(set + 0x65) == 0x3f {
        return Ok(());
    }
    if p.m.f32(set + 0x4c).abs() <= 1.1920929e-07 {
        return Ok(());
    }
    let cull = p.m.r64(s + 0x988);
    if cull != 0 {
        if cull != p.base + SET_CULL {
            return callback(cull, p.base);
        }
        // TotK's set cull (bounds against the view): reads only, called with {set, a3}.
        let info = [set as u64, a3 as u64];
        if p.query(cull, &[info.as_ptr() as u64]) & 1 == 0 {
            return Ok(());
        }
    }
    let mut em = p.m.r64(set + 8);
    while em != 0 {
        let next = p.m.r64(em + 8);
        let children = (p.m.r8(em + 0x30) >> 4) & 1 != 0 && p.m.r8(p.m.r64(em + 0x40) + 0xb5) != 0;
        if children {
            return Err("vfx child emitters");
        }
        draw_candidate(p, s, set, g8, em, prio, a3, st, cache)?;
        em = next;
    }
    Ok(())
}

/// main+0x4108: whether the emitter is past its life.
unsafe fn expired(p: &Port, em: usize) -> bool {
    let f = p.m.r32(em + 0x30);
    if (f >> 19) & 1 == 0 {
        f & 2 == 0 || p.m.f32(em + 0xa8) + (p.m.i32(p.m.r64(em + 0x40) + 0xc0) as f32) < p.m.f32(em + 0x28) - p.m.f32(em + 0x2c)
    } else {
        f & 2 == 0
    }
}

/// main+0xa238: the emitter drawn through the system's draw hook (main+0x74b610) when it is live, in the view's bits,
/// its priority group, and has particles.
#[allow(clippy::too_many_arguments)]
unsafe fn draw_candidate(p: &mut Port, s: usize, set: usize, g8: usize, em: usize, prio: u32, a3: usize, st: usize, cache: usize) -> R<()> {
    let cnt = p.m.i32(em + 0x98);
    let skip = !p.m.r32(em + 0x30) & 0x2004 != 0
        || expired(p, em)
        || p.m.f32(em + 0x80) <= 0.0
        || (cnt < 1 && (cnt != 0 || (p.m.r8(p.m.r64(em + 0x40) + 0xb6) as u32).wrapping_sub(1) > 2));
    if p.m.r64(em + 0x50) == 0 || skip {
        return Ok(());
    }
    if p.m.r32(st + 0x28) & p.m.r32(em + 0x3c) == 0 || 1u32.wrapping_shl(p.m.r32(em + 0x38) & 31) & prio == 0 {
        return Ok(());
    }
    if p.m.r64(s + 0x978) != 0 {
        return callback(p.m.r64(s + 0x978), p.base);
    }
    let hook = p.m.r64(s + 0x968);
    if hook != p.base + DRAW_HOOK {
        return callback(hook, p.base);
    }
    // main+0x74b610 with {g8, s, 0, em, a3, st, cache, set+0x50, path st+0x2c, 1, 0}.
    let path = p.m.r8(st + 0x2c);
    let w = p.m.r64(a3 + 0xb0);
    let c = core(p);
    let st2 = view_state(p, s, c);
    let sh = if path == 3 { 0 } else { p.m.r64(p.m.r64(em + 0x40) + path as usize * 8 + 0x10) };
    if sh == 0 {
        return Err("vfx draw hook without shader");
    }
    p.set_shader(w + 8, p.m.r64(sh), 0);
    for (o, lo, hi, k) in [(0xd0usize, 0xe, 0x50, 0usize), (0xd4, 0xf, 0x51, 8)] {
        let idx = p.m.i32(a3 + o);
        if idx == -1 {
            continue;
        }
        let m = p.m.r8(sh + lo) as u32 | (p.m.r8(sh + hi) as u32) << 8 | 0xffff_0000;
        if bind_deferred(p, w, p.m.r64(a3 + 200), idx, m) {
            p.m.w::<u64>(st2 + k, u64::MAX);
        }
    }
    let _ = (set, g8);
    // main+0x4248(em, w+8, 1, a3, st2, s+0x78, s+0x7c, cache).
    let p7 = s + 0x7c;
    if p.m.r8(em + 0x32) & 1 != 0 && p.m.r8(p7 + 0x11) == 0 && p.m.r8(p7 + 0x10) != 0 {
        return Err("vfx emitter shadow path");
    }
    if p.m.r8(s + 0x78) != 0 {
        draw_emitter(p, p.m.r64(em + 0x50), w + 8, em, a3, st2, cache)?;
    }
    Ok(())
}

/// main+0x74fd08(w, table, index, mask): the deferred texture table entry bound to the mask's slots of stages 0, 1,
/// 2 and 5; whether the mask was not all 0xff.
unsafe fn bind_deferred(p: &mut Port, w: usize, table: usize, index: i32, mask: u32) -> bool {
    if mask == u32::MAX {
        return false;
    }
    let cmd = p.m.r64(w + 0xb8);
    let e = (table as i64 + index as i64 * 8) as u64;
    for (k, st) in [(0, 0u64), (1, 1), (2, 2), (3, 5)] {
        let b = (mask >> (k * 8)) & 0xff;
        if b != 0xff {
            p.m.w::<u8>(cmd + 0x10, 1);
            p.call(p.pfn(PFN_BIND_TEXTURES_DEFERRED), &[cmd as u64, st, b as u8 as i8 as i64 as u64, 1, e]);
        }
    }
    true
}

const VIEW_TEXTURES: usize = 0x74e8f0;
const DISTANCE_KEY: usize = 0x26a7c00;
const SET_CULL: usize = 0x1307170;
const RENDER_SCENE: usize = 0x462bf00;

/// main+0x74e8f0 (the system's per-view callback): the scene's effect parameter block (main+0x74eee8: 0x3e0 bytes from
/// the core's allocator registered as the core's extra block 1; its contents, main+0x74f158, are the game's: the
/// returned address is excluded from the data check) and the scene's effect textures, bound directly
/// (main+0x74ec98) or through the deferred texture table.
unsafe fn view_textures(p: &mut Port, s: usize, a3: usize) -> R<usize> {
    let gr = p.m.r64(p.m.r64(p.base + RENDER_GLOBALS));
    let s2 = p.m.r64(gr + 0x18);
    let c = core(p);
    let Some((blk, _)) = ring_alloc(p, s2, c, 0x3e0) else { return Err("vfx scene block allocation") };
    if (p.m.r8(gr + 0x37a) >> 1) & 1 != 0 {
        return Err("vfx scene block (other path)");
    }
    let e = p.m.r64(s2 + 0x8c8) + c as i64 as usize * 0xc0;
    p.m.w::<u64>(e + 8, blk as u64);
    p.m.w::<u64>(e + 0x38, 0x3e0);
    let sg = p.m.r64(p.m.r64(p.base + SCENE_GLOBALS));
    if p.m.i32(sg + 0x2ec) < 0 {
        return Err("vfx view adjust");
    }
    let rs = p.m.r64(p.m.r64(p.base + RENDER_SCENE));
    p.m.w::<u64>(rs + 0x64e74, 0);
    p.m.w::<u32>(rs + 0x64e7c, 0);
    let w = p.m.r64(a3 + 0xb0);
    let d8 = p.m.r64(a3 + 0xd8);
    let l2 = p.m.r64(d8 + 8);
    if l2 == 0 {
        return Ok(blk);
    }
    let l3 = p.m.r64(d8 + 0x100);
    if l3 != 0 {
        for (o, slot) in [(0xe0usize, 9), (0x110, -1), (0xf0, 0x13), (0x100, 0xd), (0x108, 0xe), (0xd8, 0x12)] {
            let t = p.m.r64(l3 + o);
            if t == 0 {
                continue;
            }
            if slot == -1 {
                // main+0x74efd4: the scene's uniform block object, element 0, at slot 4 of stages 0, 1, 2 and 5.
                p.bind_uniform_block(t, w, 0x0404_0404, 0)?;
                continue;
            }
            scene_texture(p, w, slot, t)?;
        }
    }
    let e0 = p.m.r64(a3 + 0xe0);
    if e0 == 0 || (p.m.r8(e0 + 0x41) >> 1) & 1 == 0 {
        for (slot, o) in [(0xc, 0x28usize), (7, 0x58), (6, 0xb8), (0xf, 0x80), (0x10, 0xa8), (0x11, 0x60), (0x14, 0x30)] {
            scene_texture(p, w, slot, p.m.r64(l2 + o))?;
        }
    } else {
        for (o, m) in [(0x199usize, 0xffff0c0cu32), (0x1f9, 0xffff0707), (0x2b9, 0xffff0606), (0x249, 0xffff0f0f), (0x299, 0xffff1010), (0x209, 0xffff1111), (0x1a9, 0xffff1414)] {
            bind_deferred(p, w, p.m.r64(a3 + 200), p.m.i8(l2 + o) as i32, m);
        }
    }
    Ok(blk)
}

/// main+0x74ec98(w, slot, tex): the texture (id +0x58, sampler +0xea) at `slot` of stages 0, 4 and 5, when it is ready
/// (a load the game finishes there is not predicted).
unsafe fn scene_texture(p: &mut Port, w: usize, slot: i32, tex: usize) -> R<()> {
    if p.m.r32(tex + 0xe4) != 0 {
        return Err("vfx texture still loading");
    }
    let id = [p.m.i32(tex + 0x58) as u32, 0];
    let smp = [p.m.r16(tex + 0xea) as i16 as i32 as u32, 0];
    for st in [0, 4, 5] {
        bind_texture(p, w + 8, slot, st, id.as_ptr() as usize, smp.as_ptr() as usize);
    }
    Ok(())
}

/// main+0xa414ac(obj, w, views, pass, path, _, over, mode): the effect pass of a scene view: when the pass's bit is in
/// the view's groups (or group 0x3f) of the system's tables, the draw context for the view built from the scene's
/// view (+0x18, 0x430 bytes: view rows, projection, camera, extents) and its target block, then main+0xa41a38.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn draw_pass(p: &mut Port, obj: usize, w: usize, views: usize, pass: u32, path: u32, over: usize, mode: u32) -> R<()> {
    let fl = p.m.r8(obj + 0x64a) as u32;
    if !fl & 3 != 0 {
        return Ok(());
    }
    let gr = p.m.r64(p.m.r64(p.base + RENDER_GLOBALS));
    if gr == 0 {
        return Ok(());
    }
    let l = p.m.r64(obj + 0x5f8);
    let s = p.m.r64(gr + 0x18);
    let bit = 1u32.wrapping_shl(pass & 31);
    let n = p.m.i32(l + 0xf8);
    let cnt = p.m.i32(s + 0x2f34);
    let tbl = p.m.r64(s + 0x90);
    let has = |p: &Port, o: usize| (0..cnt.max(0) as usize).any(|j| p.m.r32(tbl + o + j * 0x300) & bit != 0);
    let mut found = false;
    if n > 0 {
        if cnt < 1 {
            return Ok(());
        }
        found = (0..n as usize).any(|k| has(p, p.m.i32(l + k * 4 + 0x78) as i64 as usize * 4));
    }
    if !found && (cnt < 1 || !has(p, 0xfc)) {
        return Ok(());
    }
    let v = match mode {
        0 => {
            let mut v = if (fl >> 3) & 1 != 0 || pass == 8 { 1 } else { 0 };
            if pass == 0x17 {
                v = 2;
            }
            v
        }
        3 => 2,
        2 => 1,
        _ => 0,
    };
    let idx = p.m.r8(obj + 8) as u32;
    let vi = if p.m.r32(views + 8) > idx { idx } else { 0 } as usize;
    let vb = p.m.r64(views + 0x10) + vi * 0x21b0;
    let alt = p.m.r32(gr + 0x378) & 1 != 0;
    // The render context's own targets (+0x41 bit 1): its target table (+0x10, entry t at +0x20 + t*8; t from the
    // variant table main+0x386e1e8, else 0x13) and deferred texture indices.
    let own = over != 0 && (p.m.r8(over + 0x41) >> 1) & 1 != 0;
    let t = if alt { p.m.r64(p.base + VARIANT_TARGETS + v * 8) } else { 0x13 };
    let target = if own {
        p.m.r64(p.m.r64(over + 0x10) + t * 8 + 0x20)
    } else if alt {
        vb + v * 0x740 + 0x700
    } else {
        vb + 0x610
    };
    let c = core(p);
    let ci = c as i64 as usize;
    if c < p.m.i32(s + 0x2f38) {
        let r = p.m.r64(s + 0x98) + ci * 0x828;
        p.m.w::<i64>(r + 0x7f0, p.m.i32(vb + 0xcb8) as i64);
        p.m.w::<i64>(r + 0x7f8, p.m.i32(target + 0x58) as i64);
    }
    // The context (a3).
    let mut a3 = vec![0u8; 0x100];
    let put = |a3: &mut Vec<u8>, o: usize, b: &[u8]| a3[o..o + b.len()].copy_from_slice(b);
    let vw = p.m.r64(obj + 0x18) + if over != 0 { p.m.r8(over + 0x46) as usize } else { 0 } * 0x430;
    for k in 0..0x30usize {
        a3[k] = p.m.r8(vw + k);
    }
    for k in 0..0x40usize {
        a3[0x30 + k] = p.m.r8(vw + 0x160 + k);
    }
    let f = |o: usize| p.m.f32(vw + o);
    let (v0, v1, v2, v3) = (f(0), f(4), f(8), f(0xc));
    let (v4, v5, v6, v7) = (f(0x10), f(0x14), f(0x18), f(0x1c));
    let (v8, v9, v10, v11) = (f(0x20), f(0x24), f(0x28), f(0x2c));
    let cx = ((v3 * -v0) - v4 * v7) - v8 * v11;
    let cy = ((v3 * -v1) - v7 * v5) - v11 * v9;
    let cz = ((v3 * -v2) - v7 * v6) - v11 * v10;
    put(&mut a3, 0x70, &cx.to_le_bytes());
    put(&mut a3, 0x74, &cy.to_le_bytes());
    put(&mut a3, 0x78, &cz.to_le_bytes());
    let e = vb + if alt { v } else { 0 } * 0x740;
    put(&mut a3, 0x7c, &(p.m.f32(e + 0x1e0) - p.m.f32(e + 0x1d8)).to_le_bytes());
    put(&mut a3, 0x80, &(p.m.f32(e + 0x1e4) - p.m.f32(e + 0x1dc)).to_le_bytes());
    for (dst, src) in [(0x84, 0x1e0), (0x88, 0x1e4), (0x8c, 0x1ec), (0x90, 0x1e8), (0x94, 0x1f4), (0x98, 0x1fc), (0x9c, 0x200)] {
        put(&mut a3, dst, &p.m.r32(vw + src).to_le_bytes());
    }
    put(&mut a3, 0xa0, &((vw + 0x60) as u64).to_le_bytes());
    put(&mut a3, 0xa8, &u32::MAX.to_le_bytes());
    put(&mut a3, 0xb0, &(w as u64).to_le_bytes());
    put(&mut a3, 0xb8, &idx.to_le_bytes());
    put(&mut a3, 0xbc, &pass.to_le_bytes());
    put(&mut a3, 0xc0, &path.to_le_bytes());
    put(&mut a3, 0xd0, &u64::MAX.to_le_bytes());
    if own {
        let tt = p.m.r64(over + 0x10);
        put(&mut a3, 0xc8, &p.m.r64(over + 0xe0).to_le_bytes());
        put(&mut a3, 0xd0, &(p.m.i8(tt + 0x2a9) as i32).to_le_bytes());
        put(&mut a3, 0xd4, &(p.m.i8(tt + t * 0x10 + 0x189) as i32).to_le_bytes());
    }
    put(&mut a3, 0xd8, &(l as u64).to_le_bytes());
    put(&mut a3, 0xe0, &(over as u64).to_le_bytes());
    let groups = l + 0x78;
    draw_view(p, gr, w, groups, n as u32, bit, a3.as_ptr() as usize, (fl >> 2) & 1)
}

/// main+0x9c8d00(obj, pass, ctx): the gsys view pass's effect drawable: per render pass the scene view's effect
/// passes (main+0xa414ac with the render context as its override object). Not ported: the pass-3 callbacks
/// (obj+0x1a8 / +0x1b0), pass 5's shadow cascades (ctx+0xb0 2 / 3) and pass 6's cascade passes.
pub(super) unsafe fn forward(p: &mut Port, obj: usize, pass: u32, c: usize) -> R<()> {
    if p.m.r8(obj + 0x1a4) & 1 == 0 || pass > 7 {
        return Ok(());
    }
    let views = p.m.r64(p.m.r64(obj + 0x198) + 0x1ec0);
    let (w, o) = (p.m.r64(c + 0x20), p.m.r64(c + 0x10));
    let id = match pass {
        0 => 0,
        1 => 1,
        2 => 2,
        4 => 0x17,
        7 => 0x13,
        3 => {
            if p.m.r64(obj + 0x1a8) != 0 {
                return callback(p.m.r64(obj + 0x1a8), p.base);
            }
            draw_pass(p, o, w, views, 8, 0, c, 0)?;
            if p.m.r64(obj + 0x1b0) != 0 {
                return callback(p.m.r64(obj + 0x1b0), p.base);
            }
            return Ok(());
        }
        5 => {
            let b0 = p.m.i32(c + 0xb0);
            if (b0.wrapping_sub(2) as u32) < 2 {
                let bv = p.m.r8(c + 0x30) as u32;
                let vi = if p.m.r32(views + 8) > bv { bv } else { 0 } as usize;
                let l12 = p.m.r64(p.m.r64(views + 0x10) + vi * 0x21b0 + 0x2118);
                if p.m.r8(l12 + 0x4e8) & 0x1f == 0 && p.m.r8(l12 + 0xa60) & 0x1f == 0 {
                    return Err("vfx shadow systems");
                }
                let v = p.m.r64(o + 0x18);
                draw_cascade(p, o, w, 0xc, 3, v, v + 0x160, v + 0x60, u32::MAX)?;
                let v2 = p.m.r64(o + 0x18);
                return draw_cascade(p, o, w, 0xd, 3, v2, v2 + 0x160, v + 0x60, u32::MAX);
            }
            if b0 != 1 {
                return Ok(());
            }
            draw_pass(p, o, w, views, 0xc, 0, c, 0)?;
            draw_pass(p, o, w, views, 0xd, 0, c, 0)?;
            let gr = p.m.r64(p.m.r64(p.base + RENDER_GLOBALS));
            if (p.m.r32(gr + 0x378) >> 3) & 1 == 0 {
                return Ok(());
            }
            draw_pass(p, o, w, views, 0x14, 0, c, 0)?;
            0x15
        }
        _ => {
            let b0 = p.m.i32(c + 0xb0);
            let k = b0.wrapping_sub(9) as u32;
            let l6 = p.m.r64(obj + 0x198);
            if k >= 3 || p.m.r8(l6 + 0xce1) & 1 != 0 {
                return Ok(());
            }
            let cb = p.m.r64(obj + 0x1b8);
            if cb == p.base + CASCADE_DECALS {
                cascade_decals(p, w, p.m.r8(c + 0x30) as u32, k)?;
            } else if cb != 0 {
                return callback(cb, p.base);
            }
            let bv = p.m.r8(c + 0x30) as u32;
            let vi = if p.m.r32(l6 + 0xd20) > bv { bv } else { 0 } as usize;
            let l12 = p.m.r64(l6 + 0xd28) + vi * 0xb48;
            for pass in [0xc, 0x14, 0x11] {
                let ki = if p.m.r32(l12 + 0x360) > k { k } else { 0 } as usize;
                let e = p.m.r64(l12 + 0x368) + ki * 0x590;
                draw_cascade(p, o, w, pass, 4, e + 0x230, e + 0x260, 0, k)?;
            }
            return Ok(());
        }
    };
    draw_pass(p, o, w, views, id, 0, c, 0)
}

const CASCADE_TARGETS: usize = 0x4631740;
const VARIANT_TARGETS: usize = 0x386e1e8;
const GRAPHICS_CONTEXT_INIT: usize = 0x688a38;

/// main+0x9c9144(obj, w, pass, mode, view, proj, info, index): an effect pass for a shadow / cascade view: the same
/// group test as main+0xa414ac, the cascade target ids, a default sead::GraphicsContext (+0 0x101, +2 1) applied, then
/// main+0xa41a38 with the context built from the given view (3x4 rows), projection, the target's size and the
/// cascade's near / far values (info+0x180..).
#[allow(clippy::too_many_arguments)]
unsafe fn draw_cascade(p: &mut Port, obj: usize, w: usize, pass: u32, mode: u32, view: usize, proj: usize, info: usize, index: u32) -> R<()> {
    let fl = p.m.r8(obj + 0x64a) as u32;
    if !fl & 3 != 0 {
        return Ok(());
    }
    let gr = p.m.r64(p.m.r64(p.base + RENDER_GLOBALS));
    if gr == 0 {
        return Ok(());
    }
    let s = p.m.r64(gr + 0x18);
    let l = p.m.r64(obj + 0x5f8);
    let bit = 1u32.wrapping_shl(pass & 31);
    let n = p.m.i32(l + 0xf8);
    let cnt = p.m.i32(s + 0x2f34);
    let tbl = p.m.r64(s + 0x90);
    let has = |p: &Port, o: usize| (0..cnt.max(0) as usize).any(|j| p.m.r32(tbl + o + j * 0x300) & bit != 0);
    let mut found = false;
    if n > 0 {
        if cnt < 1 {
            return Ok(());
        }
        found = (0..n as usize).any(|k| has(p, p.m.i32(l + k * 4 + 0x78) as i64 as usize * 4));
    }
    if !found && (cnt < 1 || !has(p, 0xfc)) {
        return Ok(());
    }
    let ct = p.m.r64(p.m.r64(p.base + CASCADE_TARGETS));
    let c = core(p);
    if c < p.m.i32(s + 0x2f38) {
        let r = p.m.r64(s + 0x98) + c as i64 as usize * 0x828;
        p.m.w::<i64>(r + 0x7f0, p.m.i32(p.m.r64(ct + 0x40) + 0x58) as i64);
        p.m.w::<i64>(r + 0x7f8, p.m.i32(p.m.r64(ct + 0xd0) + 0x58) as i64);
    }
    let mut gc = vec![0u8; 0x100];
    p.query(p.base + GRAPHICS_CONTEXT_INIT, &[gc.as_mut_ptr() as u64]);
    gc[0..2].copy_from_slice(&0x101u16.to_le_bytes());
    gc[2] = 1;
    gc[4..8].copy_from_slice(&0u32.to_le_bytes());
    gc[0x1c..0x20].copy_from_slice(&0u32.to_le_bytes());
    p.apply_graphics_context(gc.as_ptr() as usize, w);
    let mut a3 = vec![0u8; 0x100];
    let put = |a3: &mut Vec<u8>, o: usize, b: &[u8]| a3[o..o + b.len()].copy_from_slice(b);
    for k in 0..0x30usize {
        a3[k] = p.m.r8(view + k);
    }
    for k in 0..0x40usize {
        a3[0x30 + k] = p.m.r8(proj + k);
    }
    let f = |o: usize| p.m.f32(view + o);
    let cx = ((-f(0)) * f(0xc) - f(0x1c) * f(0x10)) - f(0x2c) * f(0x20);
    let cy = ((-f(4)) * f(0xc) - f(0x1c) * f(0x14)) - f(0x2c) * f(0x24);
    let cz = ((-f(8)) * f(0xc) - f(0x1c) * f(0x18)) - f(0x2c) * f(0x28);
    put(&mut a3, 0x70, &cx.to_le_bytes());
    put(&mut a3, 0x74, &cy.to_le_bytes());
    put(&mut a3, 0x78, &cz.to_le_bytes());
    let t = p.m.r64(ct + 0xd0);
    let wd = p.m.r16(t + 0x30) as u32;
    let wd = if wd > 1 { wd } else { 1 };
    let (d3, ht) = (p.m.r16(t + 0x3a) as u32, p.m.r16(t + 0x32) as u32);
    let k = if d3 == 3 { ht } else { 1 };
    let ht = if k > ht { k } else { ht };
    put(&mut a3, 0x7c, &(wd as f32).to_le_bytes());
    put(&mut a3, 0x80, &(ht as f32).to_le_bytes());
    if info != 0 {
        put(&mut a3, 0x84, &p.m.r64(info + 0x180).to_le_bytes());
        put(&mut a3, 0x8c, &p.m.r32(info + 0x18c).to_le_bytes());
        put(&mut a3, 0x90, &p.m.r32(info + 0x188).to_le_bytes());
        put(&mut a3, 0x94, &p.m.r32(info + 0x194).to_le_bytes());
        put(&mut a3, 0x98, &p.m.r32(info + 0x19c).to_le_bytes());
        put(&mut a3, 0x9c, &p.m.r32(info + 0x1a0).to_le_bytes());
    } else {
        put(&mut a3, 0x84, &0.1f32.to_le_bytes());
        put(&mut a3, 0x88, &1.0f32.to_le_bytes());
        put(&mut a3, 0x8c, &1.0f32.to_le_bytes());
        put(&mut a3, 0x90, &1.0f32.to_le_bytes());
        put(&mut a3, 0x94, &1.0f32.to_le_bytes());
    }
    put(&mut a3, 0xa0, &(info as u64).to_le_bytes());
    put(&mut a3, 0xa8, &index.to_le_bytes());
    put(&mut a3, 0xb0, &(w as u64).to_le_bytes());
    put(&mut a3, 0xb8, &(p.m.r8(obj + 8) as u32).to_le_bytes());
    if mode == 4 || mode.wrapping_sub(1) <= 2 {
        let path = p.m.r32(gr + if mode == 4 { 0x3b4 } else { 0x3b0 });
        put(&mut a3, 0xbc, &pass.to_le_bytes());
        put(&mut a3, 0xc0, &path.to_le_bytes());
        put(&mut a3, 0xc4, &mode.to_le_bytes());
    } else {
        put(&mut a3, 0xbc, &0x20u32.to_le_bytes());
    }
    put(&mut a3, 0xd0, &u64::MAX.to_le_bytes());
    put(&mut a3, 0xd8, &(l as u64).to_le_bytes());
    draw_view(p, gr, w, l + 0x78, n as u32, bit, a3.as_ptr() as usize, (fl >> 2) & 1)
}

const PFN_BIND_PROGRAM: usize = 0x466dfb0;
const PFN_BUFFER_GET_ADDRESS: usize = 0x466d818;
const PFN_DRAW_ELEMENTS: usize = 0x466e088;
const PFN_DRAW_ELEMENTS_INSTANCED: usize = 0x466e0a0;
const PFN_BIND_VERTEX_BUFFER_: usize = 0x466dfb8;
const PROGRAM_BLOCK: usize = 0x4632818;
const DECAL_SYSTEM: usize = 0x462ade0;
const CASCADE_DECALS: usize = 0xa56740;
const DECAL_KINDS_GUARD: usize = 0x462c1a0;
const DECAL_KINDS: usize = 0x462c1a8;

/// main+0x7ef4b8: a shader program applied: when enabled (+0x20 bit 0) its program (+200, all stages) and, with
/// +0x18 set, the shared block (global +0x78, size +0x38) at the slots +0x1c..+0x1f of stages 0, 1, 2 and 5. A program
/// still being set up (+0x10) is not predicted.
unsafe fn program_apply(p: &mut Port, prog: usize, w: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 {
        return Err("agl program setup pending");
    }
    if p.m.r8(prog + 0x20) & 1 == 0 {
        return Ok(());
    }
    let cmd = p.m.r64(w + 0xb8) as u64;
    p.call(p.pfn(PFN_BIND_PROGRAM), &[cmd, (prog + 200) as u64, 0x3f]);
    if p.m.r32(prog + 0x18) == 0 {
        return Ok(());
    }
    let g = p.m.r64(p.m.r64(p.base + PROGRAM_BLOCK));
    for (k, st) in [(0usize, 0u64), (1, 1), (2, 2), (3, 5)] {
        let slot = p.m.i8(prog + 0x1c + k);
        if slot != -1 {
            p.m.w::<u8>(cmd as usize + 0x10, 1);
            p.call(p.pfn(PFN_BIND_UNIFORM_BUFFER), &[cmd, st, slot as i64 as u64, p.m.r64(g + 0x78) as u64, p.m.r64(g + 0x38) as u64]);
        }
    }
    Ok(())
}

/// main+0x96306c(prog, w, index, block, element): the uniform block's element at the program's slots for `index`
/// (+0x50 table, 16-byte entries, count +0x48); whether the program has them. A program still being set up (+0x14) is
/// not predicted.
unsafe fn program_block(p: &mut Port, prog: usize, w: usize, index: u32, block: usize, element: i32) -> R<bool> {
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let i = if p.m.r32(prog + 0x48) > index { index } else { 0 } as usize;
    let slots = p.m.r32(p.m.r64(prog + 0x50) + i * 16);
    p.bind_uniform_block(block, w, slots, element)
}

/// main+0x978ba4 / main+0xf216b4 / main+0x966e70: a shape's vertex states (+0x11c / +0x160), its vertex buffers (+0x108
/// count, +0x110 buffers: address, size +0x118) and its indexed draw (+0x180 relative: prim +0xc, type +8, count
/// +0x28, index buffer +0x30), `instances` times.
unsafe fn shape_draw(p: &mut Port, shape: usize, mesh: usize, w: usize, instances: u32) {
    let cmd = p.m.r64(w + 0xb8) as u64;
    p.call(p.pfn(PFN_BIND_VERTEX_ATTRIB_STATE), &[cmd, 0x10, (shape + 0x11c) as u64]);
    p.call(p.pfn(PFN_BIND_VERTEX_STREAM_STATE), &[cmd, 0x10, (shape + 0x160) as u64]);
    let bufs = p.m.r64(shape + 0x110);
    for i in 0..p.m.r32(shape + 0x108) as usize {
        let b = p.m.r64(bufs + i * 8);
        if b == 0 {
            continue;
        }
        let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[(b + 8) as u64]);
        p.m.w::<u8>(cmd as usize + 0x10, 1);
        p.call(p.pfn(PFN_BIND_VERTEX_BUFFER_), &[cmd, i as u64, addr, p.m.r32(b + 0x118) as u64]);
    }
    let count = p.m.i32(mesh + 0x28);
    if count == 0 {
        return;
    }
    let (prim, ty) = (p.m.r32(mesh + 0xc) as u64, p.m.r32(mesh + 8) as u64);
    let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[(mesh + 0x30) as u64]);
    p.m.w::<u8>(cmd as usize + 0x10, 1);
    if instances < 2 {
        p.call(p.pfn(PFN_DRAW_ELEMENTS), &[cmd, prim, ty, count as u32 as u64, addr]);
    } else {
        p.call(p.pfn(PFN_DRAW_ELEMENTS_INSTANCED), &[cmd, prim, ty, count as u32 as u64, addr, 0, 0, instances as u64]);
    }
}

/// main+0x8c8370: a double-buffered uniform block's current buffer (+0x42 high nibble) and CPU pointer (+0x18).
unsafe fn block_select(p: &mut Port, b: usize, sel: u32) {
    p.m.w::<u8>(b + 0x42, p.m.r8(b + 0x42) & 0xf | (sel << 4) as u8);
    let count = p.m.r16(b + 0x40) as u32;
    let size = p.m.r64(b + 0x38);
    let a1 = p.query(p.base + p.vfn(b, 0x20), &[b as u64]) as i32;
    let a2 = p.query(p.base + p.vfn(b, 0x20), &[b as u64]) as i32;
    let mut ptr = 0usize;
    let o = p.m.r64(b + 0x30);
    if o != 0 {
        let q = p.m.r64(o + 0x10);
        if q != 0 {
            let base = p.m.r64(q + 0x100);
            if base != 0 {
                let stride = ((size as i32).wrapping_add(a1).wrapping_sub(1) as u32) & (a2.wrapping_neg() as u32);
                ptr = base.wrapping_add((p.m.r32(b + 0x20)).wrapping_add((sel & 0xf).wrapping_mul(count).wrapping_mul(stride)) as usize);
            }
        }
    }
    p.m.w::<u64>(b + 0x18, ptr as u64);
}

/// main+0xa56740(w, views, obj, view, ctx, cascade, _): the cascade pass callback (decals projected per cascade): the
/// default graphics context, then for each decal of kind 4 enabled for the view, its block for the cascade selected
/// (its contents, the cascade's transforms, are the game's: not predicted), the decal program applied with the block,
/// its shape drawn.
#[allow(clippy::too_many_arguments)]
unsafe fn cascade_decals(p: &mut Port, w: usize, view: u32, cascade: u32) -> R<()> {
    let sys = p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.base + DECAL_SYSTEM)) + 0x128) + 0xb08);
    if sys == 0 || p.m.r32(sys + 8) & 0x101 != 1 {
        return Ok(());
    }
    let mut gc = vec![0u8; 0x100];
    p.query(p.base + GRAPHICS_CONTEXT_INIT, &[gc.as_mut_ptr() as u64]);
    gc[0..2].copy_from_slice(&0x101u16.to_le_bytes());
    gc[2] = 1;
    gc[4..8].copy_from_slice(&0u32.to_le_bytes());
    gc[0x1c..0x20].copy_from_slice(&0u32.to_le_bytes());
    p.apply_graphics_context(gc.as_ptr() as usize, w);
    let n = p.m.r32(sys + 0x2b0) as usize;
    let arr = p.m.r64(sys + 0x2b8);
    if n != 0 && p.m.r8(p.m.r64(p.base + DECAL_KINDS_GUARD)) & 1 == 0 {
        return Err("decal kinds (static init)");
    }
    let kinds = p.m.r64(p.base + DECAL_KINDS);
    for i in 0..n {
        let e = arr + i * 0x628;
        let k = p.m.r32(e + 8);
        let k = if p.m.r32(kinds) > k { k } else { 0 } as usize;
        if p.m.i32(p.m.r64(kinds + 8) + k * 4) != 4 || p.m.r8(e + 0xc) != 0 {
            continue;
        }
        let vi = if p.m.r32(e + 0x498) > view { view } else { 0 } as usize;
        if p.m.r8(p.m.r64(e + 0x4a0) + vi * 0x88 + 0x81) == 0 {
            continue;
        }
        let ci = if cascade < 3 { cascade as usize } else { 0 };
        let blk = if cascade < 3 { e + cascade as usize * 0x80 + 0x4a8 } else { e + 0x4a8 };
        let sel = 1u32.wrapping_sub((p.m.r8(e + ci * 0x80 + 0x4ea) >> 4) as u32);
        block_select(p, blk, sel);
        let prog = p.m.r64(p.m.r64(p.m.r64(sys + 0x2d0) + 8) + 0x10);
        program_apply(p, prog, w)?;
        program_block(p, prog, w, 0, blk, 0)?;
        shape_draw(p, e + 0x88, e + 0x268, w, 1);
    }
    Ok(())
}
