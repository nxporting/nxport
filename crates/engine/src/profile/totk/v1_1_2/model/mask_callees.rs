// The callees of the volume mask chain (main+0xa4c1ec) and the post chain (main+0xc4f7c8) that issue NVN commands
// themselves: the layer's target setup (main+0xa50c00), the layer pass without inline programs (main+0xa55f9c), the
// cascade block uniform (main+0xa513d0), the volume mask finish (main+0xa51614), the bloom composite (main+0xc57e74)
// and the post chain's finish (main+0xc57d98). They run past their callers' commit point: what can stop them is
// checked by the `*_check` functions (callers add them to their own checks), everything else is applied.

use super::cubemap::set_uniform;
use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw_n, SHAPES, STREAMS};
use super::scene_passes::default_state;
use super::targets::{self, RENDER_BUFFER_UNBIND};
use super::texture_slots::{barrier_if_slot, copy_target};
use super::viewport;
use super::vfx::{program_apply, program_texture};

/// [[holder]]: the program set holder ([[[h]+0x10]+0x608]+8 is the layer pass's program set).
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[holder]]: the temporary target pool; main+0x7f0354(pool, target) gives one back.
const TARGET_POOL: usize = 0x4634e70;
const GATHER: usize = 0x7f0354;
/// The frame orientation.
const ORIENTATION: usize = 0x46312d8;
/// Target object virtual +0x10: colour and depth (the only ones texture_slots::copy_target runs).
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;
/// The import behind main+0x2b153e0: powf(x, y).
const POWF: usize = 0x2b153e0;
/// main+0x21eb800(w, render buffer, 2, floats, 32-byte table, s0, s1): the bloom composite body.
const BLOOM_BODY: usize = 0x21eb800;
/// The 32 bytes the bloom composite passes along.
const BLOOM_TABLE: usize = 0x37f9108;

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

fn at(base: usize, index: usize, size: usize) -> usize {
    base.wrapping_add(index.wrapping_mul(size))
}

unsafe fn pool(p: &Port) -> usize {
    p.m.r64(p.m.r64(p.base + TARGET_POOL))
}

unsafe fn gather(p: &mut Port, target: usize) -> R<()> {
    let pl = pool(p);
    p.helper("depth of field gather by the call", GATHER, &[pl as u64, target as u64])?;
    Ok(())
}

unsafe fn tex_w(p: &Port, t: usize) -> u32 {
    (p.m.r16(t + 0x30) as u32).max(1)
}

unsafe fn tex_h(p: &Port, t: usize) -> u32 {
    let h = p.m.r16(t + 0x32) as u32;
    (if p.m.r16(t + 0x3a) == 3 { h } else { 1 }).max(h)
}

fn span(v: f32) -> (f32, f32) {
    if v > 0.0 {
        (0.0, v)
    } else {
        (v, 0.0)
    }
}

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

unsafe fn bytes(p: &Port, a: usize, n: usize) -> Vec<u8> {
    (0..n).map(|i| p.m.r8(a + i)).collect()
}

unsafe fn ready(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

#[cfg(nx_native)]
unsafe fn guest_powf(p: &Port, x: f32, y: f32) -> f32 {
    let f: unsafe extern "C" fn(f32, f32) -> f32 = std::mem::transmute(p.base + POWF);
    f(x, y)
}

#[cfg(not(nx_native))]
unsafe fn guest_powf(_p: &Port, x: f32, y: f32) -> f32 {
    let _ = POWF;
    x.powf(y)
}

/// A side-effecting game function taking two floats too (s0, s1; integers in x0..): stops like `helper`.
#[cfg(nx_native)]
unsafe fn call_floats(p: &Port, name: &'static str, addr: usize, ints: &[u64], floats: [f32; 2]) -> R<u64> {
    if p.m.stops() {
        return Err(name);
    }
    p.flush();
    type F = unsafe extern "C" fn(u64, u64, u64, u64, u64, u64, u64, u64, f32, f32) -> u64;
    let mut x = [0u64; 8];
    x[..ints.len()].copy_from_slice(ints);
    let f: F = std::mem::transmute(p.base + addr);
    Ok(f(x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7], floats[0], floats[1]))
}

#[cfg(not(nx_native))]
unsafe fn call_floats(_p: &Port, name: &'static str, _addr: usize, _ints: &[u64], _floats: [f32; 2]) -> R<u64> {
    Err(name)
}

/// The set's program for the selector `sel` (the set's first program below 1).
unsafe fn pick(p: &Port, set: usize, sel: i32) -> usize {
    if sel < 1 {
        return p.m.r64(set + 0x10);
    }
    let i = sel as u32 - 1;
    let i = if p.m.r32(set + 0x18) <= i { 0 } else { i };
    p.m.r64(set + 0x20) + i as usize * 0x158
}

/// A program uniform (the program must have been checked ready).
unsafe fn uniform(p: &mut Port, prog: usize, w: usize, k: usize, data: &[u8]) {
    set_uniform(p, prog, w, k, data);
}

/// main+0xa513d0(prog, w, block, count): uniform 7 of the program set to `count` floats of `block` (each as the first
/// word of a 16-byte entry; one float alone as a single word). A program still being set up (+0x14) stops.
pub(super) unsafe fn block_set(p: &mut Port, prog: usize, w: usize, block: usize, count: u32) -> R<()> {
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    if count == 0 {
        return Ok(());
    }
    let data: Vec<u8> = if count == 1 {
        p.m.r32(block).to_le_bytes().to_vec()
    } else {
        let mut d = Vec::with_capacity(count as usize * 16);
        for k in 0..count as usize {
            d.extend_from_slice(&p.m.r32(block + k * 4).to_le_bytes());
            d.extend_from_slice(&[0u8; 12]);
        }
        d
    };
    uniform(p, prog, w, 7, &data);
    Ok(())
}

/// The view record of the volume mask (this +0x228 array, 0x728 each).
unsafe fn record(p: &Port, this: usize, view: u32) -> usize {
    at(p.m.r64(this + 0x228), clamp_index(view, p.m.r32(this + 0x220)), 0x728)
}

unsafe fn target_object_known(p: &Port, obj: usize) -> bool {
    if p.m.r64(obj) == 0 {
        return false;
    }
    let f = p.vfn(obj, 0x10);
    f == SETUP_COLOR || f == SETUP_DEPTH
}

/// What can stop main+0xa50c00: the two target objects' setup functions (the ones given a texture: `a`, `b` nonzero).
pub(super) unsafe fn mask_pass_check(p: &Port, this: usize, view: u32, a: bool, b: bool) -> R<()> {
    let v = record(p, this, view);
    if (a && !target_object_known(p, v + 0x458)) || (b && !target_object_known(p, v + 0x548)) {
        return Err("volume mask target object setup");
    }
    Ok(())
}

/// main+0xa50c00(this, view, a, b, block, slice): the view record's (+0x228 array, 0x728 each) target setup: its
/// slots +0x408..+0x450 cleared; `a` copied into the target object at +0x458 (the slot +0x408 pointing at it), `b`
/// into the object at +0x548 with the 24-byte `block` (width, height, two words) kept at +0x620 and `slice` as its
/// layer (+0x614), the slot +0x448 pointing at it; the sizes of the last texture given (0 without) and their spans
/// stored at +0x3f0..+0x404.
pub(super) unsafe fn mask_pass(p: &mut Port, this: usize, view: u32, a: usize, b: usize, block: usize, slice: u32) -> R<()> {
    mask_pass_check(p, this, view, a != 0, b != 0)?;
    let v = record(p, this, view);
    for o in (0..0x48).step_by(8) {
        p.m.w::<u64>(v + 0x408 + o, 0);
    }
    let (mut fw, mut fh) = (0f32, 0f32);
    if a != 0 {
        let (tw, th) = (tex_w(p, a), tex_h(p, a));
        copy_target(p, v + 0x458, a)?;
        p.m.w::<u64>(v + 0x408, (v + 0x458) as u64);
        fw = tw as f32;
        fh = th as f32;
    }
    if b != 0 {
        let (tw, th) = (tex_w(p, b), tex_h(p, b));
        copy_target(p, v + 0x548, b)?;
        fw = tw as f32;
        fh = th as f32;
        let (bw, bh) = (p.m.r32(block), p.m.r32(block + 4));
        let (bx, by) = (p.m.r64(block + 8), p.m.r64(block + 0x10));
        if bx != p.m.r64(v + 0x628) || bw != p.m.r32(v + 0x620) || bh != p.m.r32(v + 0x624) {
            p.m.w::<u32>(v + 0x620, bw);
            p.m.w::<u64>(v + 0x628, bx as u64);
            p.m.w::<u32>(v + 0x624, bh);
            p.m.w::<u64>(v + 0x630, by as u64);
            p.m.fetch_or32(v + 0x610, 8);
        }
        if p.m.r16(v + 0x614) as u32 != slice {
            p.m.fetch_or32(v + 0x610, 2);
            p.m.w::<u16>(v + 0x614, slice as u16);
        }
        p.m.w::<u64>(v + 0x448, (v + 0x548) as u64);
    }
    let (x0, x1) = span(fw);
    let (y0, y1) = span(fh);
    p.m.w::<f32>(v + 0x3f8, x0);
    p.m.w::<f32>(v + 0x3f0, fw);
    p.m.w::<f32>(v + 0x3f4, fh);
    p.m.w::<f32>(v + 0x400, x1);
    p.m.w::<f32>(v + 0x3fc, y0);
    p.m.w::<f32>(v + 0x404, y1);
    Ok(())
}

/// The record's viewport of its size (+0x3f0, +0x3f4); orientations outside 1..5 keep the constructor's defaults.
unsafe fn target_viewport(p: &Port, v: usize) -> [u32; 10] {
    let mut vp = viewport::full(p, p.m.f32(v + 0x3f0), p.m.f32(v + 0x3f4));
    let o = p.m.i32(p.m.r64(p.base + ORIENTATION));
    if !(1..=5).contains(&o) {
        vp[2] = (-f32::MAX).to_bits();
        vp[3] = f32::MAX.to_bits();
        vp[4] = (-f32::MAX).to_bits();
        vp[5] = (-f32::MAX).to_bits();
    }
    vp
}

/// The layer pass's program: the cascade count and the layer's selector pick it from the set +0x608.
unsafe fn layer_program(p: &Port, v: usize, lrec: usize) -> usize {
    let h = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    let set = p.m.r64(p.m.r64(p.m.r64(h + 0x10) + 0x608) + 8);
    let t = p.m.r64(set + 0x30);
    let a = p.m.r32(v + 0x1c8).wrapping_mul(p.m.r16(t + 0x20) as u32);
    let b = (p.m.r8(lrec + 5) as u32).wrapping_mul(p.m.r16(t + if p.m.r32(set + 0x28) > 1 { 0x28 } else { 0 } + 0x20) as u32);
    pick(p, set, a.wrapping_add(b) as i32)
}

unsafe fn layer_record(p: &Port, this: usize, layer: u32) -> usize {
    at(p.m.r64(this + 0x218), clamp_index(layer, p.m.r32(this + 0x210)), 0x2c)
}

/// What can stop main+0xa55f9c: its program still being set up, the record's render buffer's unbind, the target
/// objects' setup functions.
pub(super) unsafe fn layer_pass_check(p: &Port, this: usize, view: u32, layer: u32) -> R<()> {
    let v = record(p, this, view);
    ready(p, layer_program(p, v, layer_record(p, this, layer)))?;
    if p.vfn(v + 0x3e8, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    mask_pass_check(p, this, view, true, true)
}

/// main+0xa55f9c(this, w, view, layer): layer `layer` (this +0x218 array, 0x2c each) of the view record: the layer's
/// target set up (main+0xa50c00 with the record's target, depth level and block of the layer's level), the record's
/// buffer bound and cleared (colour 0 to (0, 0, 0, 1), depth 1), the render state, the program of the set +0x608 with
/// the record's matrices and values as uniforms and the shadow source as texture 1, then the full-screen triangle
/// drawn once per slice step (instances positioned by the layer's span, exponent and count) while the position stays
/// above the record's near value (+0xa4); the buffer unbound and the barrier after it.
pub(super) unsafe fn layer_pass(p: &mut Port, this: usize, w: usize, view: u32, layer: u32) -> R<()> {
    layer_pass_check(p, this, view, layer)?;
    let v = record(p, this, view);
    let lrec = layer_record(p, this, layer);
    let prog = layer_program(p, v, lrec);
    let rb = v + 0x3e8;
    let level = p.m.r8(lrec + 4) as usize;
    let a = p.m.r64(at(p.m.r64(v + 0x228), clamp_index(layer, p.m.r32(v + 0x220)), 8));
    let b = p.m.r64(v + 0x230 + level * 8);
    let own = v + 0x260 + level * 0x18;
    let block: [u64; 3] = [
        p.m.r32(own) as u64 | (p.m.r32(own + 4) as u64) << 32,
        p.m.r64(own + 8) as u64,
        p.m.r64(own + 0x10) as u64,
    ];
    mask_pass(p, this, view, a, b, block.as_ptr() as usize, 0)?;
    let vp = target_viewport(p, v);
    let mut values = [0u32; 0x22];
    for k in 0..8 {
        values[k * 4 + 3] = 1f32.to_bits();
    }
    values[0x20] = 1f32.to_bits();
    targets::bind_clear(p, rb, w, vp.as_ptr() as usize, 1, values.as_ptr() as usize)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let mut g = default_state(p);
    g[0] = (g[0] & 0xff00_0000) | 1;
    g[1] |= 1;
    g[10] = 0x0202_0505;
    g[0x19] = (g[0x19] & 0xffff_0000) | 2;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let shadow = p.m.r32(this + 0x208);
    let own_source = p.m.r64(v + 0xb0);
    program_apply(p, prog, w)?;
    uniform(p, prog, w, 11, &bytes(p, v + 0x34, 0x40));
    uniform(p, prog, w, 1, &bytes(p, v + 0x1dc, 0x30));
    let source = if shadow == 0 { own_source } else { v + 0x2f8 };
    let (a4, a8) = (p.m.f32(v + 0xa4), p.m.f32(v + 0xa8));
    let count = p.m.r16(lrec + 2) as u32;
    let n = (count as i32).wrapping_sub(1) as f32;
    let d3 = [1.0 / n, a4, a8, 1.0 - a4 / a8];
    uniform(p, prog, w, 3, &f32s(&d3));
    uniform(p, prog, w, 4, &count.to_le_bytes());
    let (sw, sh) = (tex_w(p, source), tex_h(p, source));
    uniform(p, prog, w, 9, &f32s(&[1.0 / sw as f32, 1.0 / sh as f32]));
    let r14 = p.m.f32(lrec + 0x14);
    uniform(p, prog, w, 8, &f32s(&[p.m.f32(lrec + 0x18) - r14, r14]));
    uniform(p, prog, w, 10, &u32s(&[p.m.r32(v + 0x1d0), p.m.r32(v + 0x1cc)]));
    let cascades = p.m.r32(v + 0x1c8);
    if cascades == 0 {
        uniform(p, prog, w, 6, &bytes(p, v + 0xb8, 0x40));
    } else {
        uniform(p, prog, w, 6, &bytes(p, v + 0xb8, cascades as usize * 0x40));
        block_set(p, prog, w, v + 0x1b8, cascades)?;
    }
    program_texture(p, prog, w, 1, source)?;
    let first = if a4 > 0.0 && p.m.r8(lrec + 1) != 0 { a4 } else { p.m.f32(lrec + 0xc) };
    let (r0c, r10) = (p.m.f32(lrec + 0xc), p.m.f32(lrec + 0x10));
    let half = (r10 - r0c) * 0.5;
    let wide = r10 - first;
    let range = if wide > half { wide } else { half };
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);
    let e = p.m.f32(lrec + 8);
    let step = |p: &Port, n: f32, d: f32| -> f32 {
        if e == 1.0 {
            n / d
        } else {
            guest_powf(p, n / d, e)
        }
    };
    if count != 0 {
        let top = count - 1;
        let mut s0 = top as f32;
        let mut pos = first + range * step(p, s0, top as i32 as f32);
        if !(p.m.f32(v + 0xa4) >= pos) {
            let mut left = count.wrapping_sub(2);
            loop {
                let s1 = pos + f32::from_bits(0x3700_0000);
                let (d4, d8) = (p.m.f32(v + 0x1d4), p.m.f32(v + 0x1d8));
                uniform(p, prog, w, 2, &f32s(&[s1 * d4, s1 * d8, s1, s0]));
                let streams = p.m.r64(p.m.r64(p.base + STREAMS));
                stream_draw_n(p, streams + 0x488, w, 1);
                if left == u32::MAX {
                    break;
                }
                s0 = left as f32;
                left = left.wrapping_sub(1);
                let denom = (p.m.r16(lrec + 2) as u32).wrapping_sub(1) as i32 as f32;
                pos = first + range * step(p, s0, denom);
                if p.m.f32(v + 0xa4) >= pos {
                    break;
                }
            }
        }
    }
    if p.vfn(rb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    targets::unbind(p, w)?;
    barrier_if_slot(p, v + 0x4b8, w);
    Ok(())
}

/// main+0xa51614(this, view): the view record's temporary targets given back: per layer its target (+0x228 array of
/// the record) and its depth level target (+0x230 by the layer's level, with that level's 24-byte block at +0x260
/// cleared), then the shadow target (+0x2f0).
pub(super) unsafe fn volume_mask_finish(p: &mut Port, this: usize, view: u32) -> R<()> {
    let v = record(p, this, view);
    let mut i = 0u32;
    while (i as i32) < p.m.r32(this + 0x210) as i32 {
        let arr = p.m.r64(v + 0x228);
        let slot = at(arr, clamp_index(i, p.m.r32(v + 0x220)), 8);
        let t = p.m.r64(slot);
        if t != 0 {
            gather(p, t)?;
            let slot = at(p.m.r64(v + 0x228), clamp_index(i, p.m.r32(v + 0x220)), 8);
            p.m.w::<u64>(slot, 0);
        }
        let level = p.m.r8(at(p.m.r64(this + 0x218), i as usize, 0x2c) + 4) as usize;
        let d = p.m.r64(v + 0x230 + level * 8);
        if d != 0 {
            gather(p, d)?;
            let level = p.m.r8(at(p.m.r64(this + 0x218), i as usize, 0x2c) + 4) as usize;
            p.m.w::<u64>(v + 0x230 + level * 8, 0);
            let own = v + 0x260 + p.m.r8(at(p.m.r64(this + 0x218), i as usize, 0x2c) + 4) as usize * 0x18;
            p.m.w::<u32>(own, 0);
            p.m.w::<u64>(own + 8, 0);
            p.m.w::<u64>(own + 0x10, 0);
        }
        i += 1;
    }
    let t = p.m.r64(v + 0x2f0);
    if t != 0 {
        gather(p, t)?;
        p.m.w::<u64>(v + 0x2f0, 0);
    }
    Ok(())
}

/// gsys::RenderBufferContext::releaseAABuffer(state) (main+0xc57dd4): the five temporary targets of the state given
/// back, in order.
unsafe fn release_aa(p: &mut Port, state: usize) -> R<()> {
    for i in 0..5 {
        let t = p.m.r64(state + i * 8);
        if t != 0 {
            gather(p, t)?;
            p.m.w::<u64>(state + i * 8, 0);
        }
    }
    Ok(())
}

/// main+0xc57d98(this, view): the frame record's (+0x5c8 array, 0x860 each) two target sets (+0x430, +0x638) given back.
pub(super) unsafe fn post_finish(p: &mut Port, this: usize, view: u32) -> R<()> {
    let f = at(p.m.r64(this + 0x5c8), clamp_index(view, p.m.r32(this + 0x5c0)), 0x860);
    release_aa(p, f + 0x430)?;
    release_aa(p, f + 0x638)
}

/// main+0xc57e74(this, w, view, k, render buffer): the bloom composite of level `k` of the view record (this +0x5d8
/// array, indexed by view & this +0x18), when its entry (0xa0 each, flag +0x168) is set: the game's composite body
/// (main+0x21eb800) called with the render buffer, two floats of the entry (+0x188, +0x1a8), a 32-byte table and the
/// frame record's two floats (+0x858, +0x85c). The body emits NVN commands itself and is left to the game.
pub(super) unsafe fn bloom_pass(p: &mut Port, this: usize, w: usize, view: u32, k: u32, rbuf: usize) -> R<()> {
    let vi = clamp_index(view, p.m.r32(this + 0x5c0));
    let m = p.m.r32(this + 0x18) & view;
    let rec_view = p.m.r64(at(p.m.r64(this + 0x5d8), clamp_index(m, p.m.r32(this + 0x5d0)), 8));
    let rec = rec_view.wrapping_add((k as usize).wrapping_mul(0xa0));
    if p.m.r8(rec + 0x168) == 0 {
        return Ok(());
    }
    let frame = at(p.m.r64(this + 0x5c8), vi, 0x860);
    let floats: [u32; 2] = [p.m.r32(rec + 0x188), p.m.r32(rec + 0x1a8)];
    let table: [u64; 4] = std::array::from_fn(|i| p.m.r64(p.base + BLOOM_TABLE + i * 8) as u64);
    let s = [p.m.f32(frame + 0x858), p.m.f32(frame + 0x85c)];
    call_floats(p, "bloom composite by the call", BLOOM_BODY, &[w as u64, rbuf as u64, 2, floats.as_ptr() as u64, table.as_ptr() as u64], s)?;
    Ok(())
}
