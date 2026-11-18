// Passes of the scene renderer that draw into a render buffer the game builds on its stack from texture states of its
// records (texture_slots.rs): the objects are host buffers here, filled as the game's constructors and copies fill them.

use super::callees_misc::{half_res_pass, half_res_pass_check};
use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, stream_draw_n, SHAPES, STREAMS};
use super::scene_passes::{default_state, fullscreen, scene_blocks};
use super::texture_slots::{acquire, alloc_target, assign, barrier_if_slot, copy_target, level_size, new_buffer, new_target, release, release_sampler, set_extent, Host};
use super::texture_slots::{BUFFER_BYTES, COLOR_VTABLE, DEPTH_VTABLE, TARGET_BYTES};
use super::vfx::{draw_pass, program_apply, program_block, program_storage_block, program_texture};
use super::{cubemap, targets, viewport};

/// [[holder]] +0x50c: the graphics settings.
const SETTINGS: usize = 0x462bbf0;
/// [var]: the 16-byte clear colour of main+0x9618d0's targets.
const CLEAR_COLOR: usize = 0x4634ea0;
/// The pure 3x4 inverse (out, in).
const INVERSE_3X4: usize = 0x93a4c4;
/// The program set holder: [[[[holder]+0x10]+0x1b0]+8]+0x10 is the program of main+0x959a14's first pass.
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[holder]]: the depth-of-field object main+0x7f0354 gathers the frame's temporary targets with.
const TARGET_POOL: usize = 0x4634e70;
const GATHER: usize = 0x7f0354;
/// "Reprojection", "DepthBuffer".
const REPROJECTION_NAME: usize = 0x3746b91;
const DEPTH_BUFFER_NAME: usize = 0x36fbafe;
/// [[[holder]]+0x18]: the default texture.
const DEFAULT_TEXTURES: usize = 0x4631740;
/// "CorrectDepth".
const CORRECT_DEPTH_NAME: usize = 0x374e8ec;
/// "volume_mask".
const VOLUME_MASK_NAME: usize = 0x37360cb;
/// "agl::TextureSampler(from texture)".
const SAMPLER_NAME: usize = 0x37a908e;
/// Render buffer virtual +0x28: the wrapper's unbind (targets::unbind).
const RENDER_BUFFER_UNBIND: usize = 0xbe19ac;

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

/// The stack objects of a pass: a render buffer and `n` target objects.
struct Objects {
    rb: Host,
    targets: Vec<Host>,
}

impl Objects {
    unsafe fn new(p: &mut Port, vtables: &[usize]) -> Objects {
        let rb = Host::new(p, BUFFER_BYTES);
        new_buffer(p, rb.addr());
        let mut targets = Vec::new();
        for &v in vtables {
            let t = Host::new(p, TARGET_BYTES);
            new_target(p, t.addr(), v);
            targets.push(t);
        }
        Objects { rb, targets }
    }

    fn rb(&self) -> usize {
        self.rb.addr()
    }

    fn target(&self, k: usize) -> usize {
        self.targets[k].addr()
    }

    /// The objects' slot references released, the last constructed first, and the buffers given back.
    unsafe fn release(self, p: &mut Port) {
        for t in self.targets.iter().rev() {
            release(p, t.addr() + 8);
        }
        for t in self.targets {
            t.release(p);
        }
        self.rb.release(p);
    }
}

/// main+0x9618d0(this, w, view, scene, ctx, flags): the view record's (ctx +0x10, 0x21b0 each) grid of quads
/// (+0x21a8 * +0x21ac instances) drawn twice through a buffer of a colour and a depth target copied from the record.
/// With this +0x352 bit 2 and flags bit 0 clear: a barrier, targets +0xf30 / +0xd50, program 9 of [+0x368]+8 with the
/// scene blocks and texture 0 = +0xf30, no blending; otherwise the game's main+0xa548e0(ctx, w, view, 8, 0, 0, 0)
/// instead. Then with the bit: program 10 with the same bindings, targets +0x1670 / +0x1490, blending into all four
/// channels; without it main+0xa548e0(ctx, w, view, 0x14, 0, 1, 1).
pub(super) unsafe fn scene_pass(p: &mut Port, this: usize, w: usize, view: u32, scene: usize, ctx: usize, flags: u32) -> R<()> {
    let o = Objects::new(p, &[COLOR_VTABLE, DEPTH_VTABLE]);
    let r = scene_stages(p, this, w, view, scene, ctx, flags, &o);
    o.release(p);
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn scene_stages(p: &mut Port, this: usize, w: usize, view: u32, scene: usize, ctx: usize, flags: u32, o: &Objects) -> R<()> {
    let rec = p.m.r64(ctx + 0x10).wrapping_add(clamp_index(view, p.m.r32(ctx + 8)).wrapping_mul(0x21b0));
    let (rb, a, b) = (o.rb(), o.target(0), o.target(1));
    p.m.w::<u64>(rb + 0x20, a as u64);
    p.m.w::<u64>(rb + 0x60, b as u64);
    let mut g = default_state(p);
    g[0] = (g[0] & 0xffff_0000) | 0x101;
    g[1] = 0;
    g[0x19] = (g[0x19] & 0xffff_ff00) | 8;
    let enabled = (p.m.r8(this + 0x352) >> 2) & 1 != 0;
    if !enabled || flags & 1 != 0 {
        half_res_pass_check(p, ctx, w, view, 8, 0, 0, 0)?;
    }
    if !enabled {
        half_res_pass_check(p, ctx, w, view, 0x14, 0, 1, 1)?;
    }
    let programs = p.m.r64(p.m.r64(this + 0x368) + 8);
    if !enabled || flags & 1 != 0 {
        half_res_pass(p, ctx, w, view, 8, 0, 0, 0)?;
    } else {
        let settings = p.m.r64(p.m.r64(p.base + SETTINGS));
        let cmd = p.m.r64(w + 0xb8);
        p.barrier(cmd, if (p.m.r8(settings + 0x50c) >> 1) & 1 == 0 { 0x12 } else { 0x112 });
        let prog = p.m.r64(programs + 0x20) + if p.m.r32(programs + 0x18) < 10 { 0 } else { 0xc18 };
        copy_target(p, a, rec + 0xf30)?;
        copy_target(p, b, rec + 0xd50)?;
        g[7] = 0;
        bind_targets(p, w, rb, b, &g)?;
        program_apply(p, prog, w)?;
        scene_blocks(p, this, w, view, prog, scene)?;
        program_texture(p, prog, w, 0, rec + 0xf30)?;
        instanced_quad(p, w, rec);
        targets::unbind(p, w)?;
    }
    if !enabled {
        half_res_pass(p, ctx, w, view, 0x14, 0, 1, 1)?;
        return Ok(());
    }
    let prog = p.m.r64(programs + 0x20) + if p.m.r32(programs + 0x18) < 0xb { 0 } else { 0xd70 };
    program_apply(p, prog, w)?;
    scene_blocks(p, this, w, view, prog, scene)?;
    program_texture(p, prog, w, 0, rec + 0xf30)?;
    copy_target(p, a, rec + 0x1670)?;
    copy_target(p, b, rec + 0x1490)?;
    g[7] = 0xf;
    bind_targets(p, w, rb, b, &g)?;
    instanced_quad(p, w, rec);
    targets::unbind(p, w)
}

/// The buffer sized by its depth target `d`, the viewport of that size, the render state, the bind (main+0x8660b4
/// without clears; its values eight copies of the clear colour, depth 1).
unsafe fn bind_targets(p: &mut Port, w: usize, rb: usize, d: usize, g: &[u32; 0x20]) -> R<()> {
    let (dw, dh) = level_size(p, d, false);
    let (fw, fh) = set_extent(p, rb, dw, dh);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let c = p.pfn(CLEAR_COLOR);
    let (lo, hi) = (p.m.r64(c) as u64, p.m.r64(c + 8) as u64);
    let mut values = [0u64; 0x11];
    for k in 0..8 {
        values[k * 2] = lo;
        values[k * 2 + 1] = hi;
    }
    values[0x10] = 1f32.to_bits() as u64;
    targets::bind_clear(p, rb, w, vp.as_ptr() as usize, 0, values.as_ptr() as usize)
}

/// The quad shape (shapes +0xd28) drawn over the record's grid (stream +0x1e8).
unsafe fn instanced_quad(p: &mut Port, w: usize, rec: usize) {
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0xd28, w);
    let n = p.m.i32(rec + 0x21ac).wrapping_mul(p.m.i32(rec + 0x21a8)) as u32;
    stream_draw_n(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x1e8, w, n);
}

/// main+0x95bef4(this, [w, view, _, grid]): with this +0x2f0 bit 0, the view's entries (this +0x10, 0xc0 each; count
/// +0xb0) drawn into a buffer of the grid record's (+0x10, 0x21b0 each) textures [+0x2118] +0x518 / +0x528 and depth
/// +0x420: program [[this+0xc8]+8]+0x20 with eight textures and the entries' block, colour 0 mask 7, then per visible
/// entry (+0xd4 bit 1, bit 2 only with this +0x2f0 bit 4) a 0xd0-byte ring block of its words with the view matrix
/// times its 3x4 inverse and the shape +0x968 / stream +0x1648; the buffer's unbind.
pub(super) unsafe fn entry_volumes(p: &mut Port, this: usize, args: usize) -> R<()> {
    if p.m.r8(this + 0x2f0) & 1 == 0 {
        return Ok(());
    }
    let w = p.m.r64(args);
    let view = p.m.r32(args + 8);
    let grid = p.m.r64(args + 0x18);
    let entries = p.m.r64(this + 0x10).wrapping_add(clamp_index(view, p.m.r32(this + 8)).wrapping_mul(0xc0));
    if p.m.r32(entries + 0xb0) == 0 {
        return Ok(());
    }
    let rec = p.m.r64(grid + 0x10).wrapping_add(clamp_index(view, p.m.r32(grid + 8)).wrapping_mul(0x21b0));
    let t = p.m.r64(rec + 0x2118);
    let (ta, tb) = (p.m.r64(t + 0x518), p.m.r64(t + 0x528));
    if ta == 0 || tb == 0 {
        return Ok(());
    }
    let o = Objects::new(p, &[COLOR_VTABLE, COLOR_VTABLE, DEPTH_VTABLE]);
    let r = entry_volumes_draw(p, this, w, entries, rec, t, [ta, tb], &o);
    o.release(p);
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn entry_volumes_draw(p: &mut Port, this: usize, w: usize, entries: usize, rec: usize, t: usize, [ta, tb]: [usize; 2], o: &Objects) -> R<()> {
    // The buffer's size from the record's depth state (+0x450 width, +0x452 height, +0x45a class).
    let dw = p.m.r16(rec + 0x450).max(1);
    let h = p.m.r16(rec + 0x452);
    let dh = if p.m.r16(rec + 0x45a) != 3 { 1 } else { h }.max(h);
    let rb = o.rb();
    let (fw, fh) = set_extent(p, rb, dw, dh);
    let (a, b, c) = (o.target(0), o.target(1), o.target(2));
    p.m.w::<u64>(rb + 0x20, a as u64);
    p.m.w::<u64>(rb + 0x28, b as u64);
    p.m.w::<u64>(rb + 0x60, c as u64);
    copy_target(p, a, ta + 8)?;
    copy_target(p, b, tb + 8)?;
    copy_target(p, c, rec + 0x420)?;
    let vp = viewport::full(p, fw, fh);
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    let prog = p.m.r64(p.m.r64(p.m.r64(this + 0xc8) + 8) + 0x20);
    program_apply(p, prog, w)?;
    program_texture(p, prog, w, 0, rec + 0x610)?;
    program_texture(p, prog, w, 1, t + 0x208)?;
    program_texture(p, prog, w, 2, t + 0x18)?;
    program_texture(p, prog, w, 3, t + 0x110)?;
    program_texture(p, prog, w, 4, this + 0xd0)?;
    program_texture(p, prog, w, 5, this + 0x1c0)?;
    let probes = p.m.r64(this + 0x2c8);
    program_texture(p, prog, w, 6, probes + 0xb8)?;
    program_texture(p, prog, w, 7, probes + ((p.m.r32(this + 0x2c0) > 1) as usize) * 0x1c8 + 0xb8)?;
    program_block(p, prog, w, 0, entries, 0)?;
    let mut g = default_state(p);
    g[0] = (g[0] & 0xffff_0000) | 1;
    g[1] = 0;
    g[7] = 0xf7;
    g[0x19] = (g[0x19] & 0xffff_0000) | 0x107;
    p.apply_graphics_context(g.as_ptr() as usize, w);

    let count = p.m.i32(entries + 0xb0);
    let list = p.m.r64(entries + 0xb8);
    // The view's 3x4 matrix rows (entries +0x80, +0x90, +0xa0).
    let m: [[f32; 4]; 3] = std::array::from_fn(|r| std::array::from_fn(|j| p.m.f32(entries + 0x80 + r * 0x10 + j * 4)));
    for i in 0..count.max(0) as usize {
        let e = p.m.r64(list + i * 8);
        let flags = p.m.r8(e + 0xd4);
        if (flags >> 1) & 1 == 0 || !((p.m.r8(this + 0x2f0) >> 4) & 1 != 0 || (flags >> 2) & 1 == 0) {
            continue;
        }
        let mut inv = [0f32; 12];
        p.query(p.base + INVERSE_3X4, &[inv.as_mut_ptr() as u64, (e + 8) as u64]);
        let mut data = [0u32; 0x34];
        for k in 0..12 {
            data[k] = p.m.r32(e + 8 + k * 4);
        }
        // View matrix times the inverse: fmul, then two fmla per lane; the inverse's fourth column joins lane 3.
        for r in 0..3 {
            for j in 0..4 {
                let acc = m[0][j] * inv[r * 4];
                let acc = m[1][j].mul_add(inv[r * 4 + 1], acc);
                let acc = m[2][j].mul_add(inv[r * 4 + 2], acc);
                let extra = if j == 3 { inv[r * 4 + 3] } else { 0.0 };
                data[0xc + r * 4 + j] = (extra + acc).to_bits();
            }
        }
        for k in 0x18..0x34 {
            data[k] = p.m.r32(e + k * 4);
        }
        let block = p.ring_block(data.as_ptr() as usize, 0xd0);
        program_block(p, prog, w, 1, block, 0)?;
        shape_state(p, shapes + 0x968, w);
        stream_draw(p, streams + 0x1648, w);
    }
    if p.vfn(rb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    targets::unbind(p, w)
}

/// main+0x959a14(this, w, view, tex, desc, ctx): with +0x30 bit 0, the view record's target (+0x40, 0x440 each, +0x100)
/// drawn with the program [[[[holder]+0x10]+0x1b0]+8]+0x10 (uniform 8 = ones, 0x1a = 1.0; colour mask +0x1c = 4) as
/// the full-screen triangle; ctx's per-view slot (+0x20, 0x9e0 each, +0x5c0) cleared; with a description `desc`, `tex`
/// drawn with [[[this+0x50]+8]+0x20] (+0x1c = 8) into a target copied from it.
pub(super) unsafe fn view_copy(p: &mut Port, this: usize, w: usize, view: u32, tex: usize, desc: usize, ctx: usize) -> R<()> {
    if p.m.r8(this + 0x30) & 1 != 0 {
        let target = p.m.r64(this + 0x40).wrapping_add(clamp_index(view, p.m.r32(this + 0x38)).wrapping_mul(0x440)) + 0x100;
        let o = Objects::new(p, &[]);
        let r = view_copy_first(p, w, target, o.rb());
        o.release(p);
        r?;
    }
    let c = clamp_index(view, p.m.r32(ctx + 0x18));
    p.m.w::<u64>(p.m.r64(ctx + 0x20).wrapping_add(c.wrapping_mul(0x9e0)) + 0x5c0, 0);
    if desc == 0 {
        return Ok(());
    }
    let o = Objects::new(p, &[COLOR_VTABLE]);
    let r = view_copy_second(p, this, w, tex, desc, &o);
    o.release(p);
    r
}

unsafe fn view_copy_first(p: &mut Port, w: usize, target: usize, rb: usize) -> R<()> {
    let (tw, th) = level_size(p, target, false);
    let (fw, fh) = set_extent(p, rb, tw, th);
    p.m.w::<u64>(rb + 0x20, target as u64);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = 4;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    targets::bind(p, rb, w)?;
    let h = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    let prog = p.m.r64(p.m.r64(p.m.r64(p.m.r64(h + 0x10) + 0x1b0) + 8) + 0x10);
    program_apply(p, prog, w)?;
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let ones: Vec<u8> = [1f32; 4].iter().flat_map(|f| f.to_le_bytes()).collect();
    cubemap::set_uniform(p, prog, w, 8, &ones);
    cubemap::set_uniform(p, prog, w, 0x1a, &1f32.to_le_bytes());
    fullscreen(p, w);
    targets::unbind(p, w)
}

unsafe fn view_copy_second(p: &mut Port, this: usize, w: usize, tex: usize, desc: usize, o: &Objects) -> R<()> {
    let (rb, t) = (o.rb(), o.target(0));
    copy_target(p, t, desc)?;
    let (tw, th) = level_size(p, t, true);
    let (fw, fh) = set_extent(p, rb, tw, th);
    p.m.w::<u64>(rb + 0x20, t as u64);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = 8;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let prog = p.m.r64(p.m.r64(p.m.r64(this + 0x50) + 8) + 0x20);
    program_apply(p, prog, w)?;
    targets::bind(p, rb, w)?;
    program_texture(p, prog, w, 0, tex)?;
    fullscreen(p, w);
    targets::unbind(p, w)
}

/// main+0x9635b8(this, w, view): the reprojection pass of the view record (this +0x10, 0x570 each) with n = +0x558
/// textures (entries +0x560, 0x2e8 each): per entry a temporary "Reprojection" target (format +0xa0, size +0x550 /
/// +0x554) kept in the entry (+0x1e0, its state at +0x1e8, flag +0x2cc bit 0) and copied into a colour target object
/// (blending by the entry's +0x2e0 bit 0); a "DepthBuffer" target (format 0x5a) as the depth object; the render state, the
/// viewport of the depth size, the program the set ([this+0x20]+8) picks by n, the textures and blocks of main+0x964f10,
/// the shape +8, the buffer bound with every target cleared (zero, depth 1), with this +0x19 bit 1 the stream +0x728 over
/// +0xf8 * +0x108 instances; the unbind, the target barriers and the gather (main+0x7f0354) of the depth target.
/// Applying only: the allocations are past the commit point.
pub(super) unsafe fn reprojection(p: &mut Port, x: usize, w: usize, view: u32) -> R<()> {
    let rec = p.m.r64(x + 0x10).wrapping_add(clamp_index(view, p.m.r32(x + 8)).wrapping_mul(0x570));
    let n = p.m.r32(rec + 0x558);
    if n == 0 {
        return Ok(());
    }
    if n > 8 {
        return Err("more than eight reprojection textures");
    }
    // The program the texture count picks: the set's table (+0x30) of variant sizes (+0x20, the second table 0x28 on).
    let set = p.m.r64(p.m.r64(x + 0x20) + 8);
    let table = p.m.r64(set + 0x30);
    let step = if p.m.r32(set + 0x28) > 1 { 0x28 } else { 0 };
    let sel = (p.m.r16(table + 0x20) as u32).wrapping_add(n.wrapping_mul(p.m.r16(table + step + 0x20) as u32)) as i32;
    let prog = if sel < 1 {
        p.m.r64(set + 0x10)
    } else {
        let i = if p.m.r32(set + 0x18) <= (sel - 1) as u32 { 0 } else { (sel - 1) as usize };
        p.m.r64(set + 0x20) + i * 0x158
    };
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let mut vt = vec![COLOR_VTABLE; n as usize];
    vt.push(DEPTH_VTABLE);
    let o = Objects::new(p, &vt);
    let r = reprojection_pass(p, x, w, view, rec, n as usize, prog, &o);
    o.release(p);
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn reprojection_pass(p: &mut Port, x: usize, w: usize, view: u32, rec: usize, n: usize, prog: usize, o: &Objects) -> R<()> {
    let (width, height) = (p.m.r32(rec + 0x550), p.m.r32(rec + 0x554));
    let entries = p.m.r64(rec + 0x560);
    let rb = o.rb();
    p.m.commit();
    // commit
    let mut blend = 0u32;
    let mut present = 0u32;
    for k in 0..n {
        let entry = entries + k * 0x2e8;
        let t = alloc_target(p, w, REPROJECTION_NAME, p.m.r8(entry + 0xa0) as u32, width, height)?;
        p.m.w::<u64>(entry + 0x1e0, t as u64);
        assign(p, entry + 0x1e8, t)?;
        p.m.fetch_or32(entry + 0x2cc, 1);
        let obj = o.target(k);
        copy_target(p, obj, t)?;
        p.m.w::<u64>(rb + 0x20 + k * 8, obj as u64);
        if p.m.r8(entry + 0x2e0) & 1 != 0 {
            blend |= 0xf << ((k as u32 & 7) << 2);
        }
        present |= 1 << k;
    }
    let mask = (present & 0xff) | 0x100;
    let depth = alloc_target(p, w, DEPTH_BUFFER_NAME, 0x5a, width, height)?;
    let d = o.target(n);
    copy_target(p, d, depth)?;
    p.m.w::<u64>(rb + 0x60, d as u64);
    let (dw, dh) = level_size(p, d, true);
    let (fw, fh) = set_extent(p, rb, dw, dh);
    let mut g = default_state(p);
    g[0] = (g[0] & 0xffff_0000) | 0x101;
    g[1] = 0;
    g[7] = blend;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let mut values = [0u32; 0x22];
    values[0x20] = 1f32.to_bits();
    program_apply(p, prog, w)?;
    pass_bindings(p, x, view, w, prog)?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 8, w);
    targets::bind_clear(p, rb, w, vp.as_ptr() as usize, mask, values.as_ptr() as usize)?;
    if (p.m.r8(x + 0x19) >> 1) & 1 != 0 {
        let instances = p.m.i32(rec + 0x108).wrapping_mul(p.m.i32(rec + 0xf8)) as u32;
        stream_draw_n(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x728, w, instances);
    }
    targets::unbind(p, w)?;
    targets::barrier_targets(p, rb, w);
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    p.helper("depth of field gather", GATHER, &[pool as u64, depth as u64])?;
    Ok(())
}

/// main+0x964f10(this, view, w, prog): the view record's textures 0..n (entries +0xf0), its block 0 (+0x1e0) and
/// storage blocks 0 ([+0x2e8] +0x72a8) and 1 (+0x260).
unsafe fn pass_bindings(p: &mut Port, x: usize, view: u32, w: usize, prog: usize) -> R<()> {
    let rec = p.m.r64(x + 0x10).wrapping_add(clamp_index(view, p.m.r32(x + 8)).wrapping_mul(0x570));
    let mut k = 0u32;
    while k < p.m.r32(rec + 0x558) {
        program_texture(p, prog, w, k, p.m.r64(rec + 0x560) + k as usize * 0x2e8 + 0xf0)?;
        k += 1;
    }
    program_block(p, prog, w, 0, rec + 0x1e0, 0)?;
    program_storage_block(p, prog, w, 0, p.m.r64(rec + 0x2e8) + 0x72a8, 0)?;
    program_storage_block(p, prog, w, 1, rec + 0x260, 0)?;
    Ok(())
}

/// main+0x95ad64(this, [w, ctx, view record, texture, clear flag]): the volume mask. A temporary "volume_mask" target
/// (format 0x1d, sized by the largest of three textures) kept in this's per-view slot (+0x30, 0xf8 each; its state
/// copied to +8 at the end, +0xec bit 0). Pass 1: the full-screen triangle of [this+8] into it with the textures (the
/// default one, or the record's when its switches +0x218 and validity allow it), the given texture and a 16-byte
/// ring block of this +0x20. With the first switch and a valid record, pass 2: the record's own target cleared
/// (the flag as mask) and the view's effect pass 0x18 (main+0xa414ac) drawn into it. Each pass ends with the unbind and
/// a barrier when the target has a slot. Applying only: the allocation is past the commit point.
pub(super) unsafe fn volume_mask(p: &mut Port, this: usize, args: usize) -> R<()> {
    let w = p.m.r64(args);
    let a = p.m.r64(args + 8);
    let b = p.m.r64(args + 16);
    let c = p.m.r64(args + 24);
    let flag = p.m.r8(args + 32);
    let view = p.m.r8(b + 8) as u32;
    let default = p.m.r64(p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES)) + 0x18);
    let rec = p.m.r64(a + 0x1e78);
    if rec == 0 {
        return Err("volume mask record missing");
    }
    let count = p.m.r32(rec + 0x220);
    let entry = |p: &Port| p.m.r64(rec + 0x228) + clamp_index(view, count) * 0x728;
    let valid = p.m.r8(p.m.r64(a + 0x1df8) + 0x730) != 0 && p.m.r8(rec + 0x298) != 0 && p.m.r8(entry(p)) != 0;
    let switches = p.m.r64(rec + 0x218);
    let first = p.m.r8(switches) != 0;
    let second = p.m.r8(switches + ((p.m.r32(rec + 0x210) > 1) as usize) * 0x2c) != 0;
    let second_texture = |p: &Port| {
        let r = entry(p);
        p.m.r64(r + 0x10) + ((p.m.r32(r + 8) > 1) as usize) * 0xf0
    };
    let (mut tex0, mut tex1) = (default, default);
    let used = if first { valid } else { second && valid };
    if used && flag == 0 {
        if first {
            tex0 = p.m.r64(entry(p) + 0x10);
        }
        if second {
            tex1 = second_texture(p);
        }
    }
    let again = used && first;
    // The target's size: the largest of the three textures'.
    let width = |p: &Port, t: usize, min: bool| {
        let v = p.m.r16(t + 0x30);
        if min { v.max(1) } else { v }
    };
    let height = |p: &Port, t: usize| {
        let h = p.m.r16(t + 0x32);
        if p.m.r16(t + 0x3a) != 3 { 1 } else { h }.max(h)
    };
    let tw = width(p, tex0, true).max(width(p, tex1, false)).max(width(p, c, true));
    let th = height(p, tex0).max(height(p, tex1)).max(height(p, c));
    let prog = p.m.r64(this + 8);
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let o = Objects::new(p, &[COLOR_VTABLE]);
    p.m.commit();
    // commit
    let r = volume_mask_passes(p, this, [w, a, b, c], [tex0, tex1], [tw, th], rec, again, flag, view, &o);
    o.release(p);
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn volume_mask_passes(p: &mut Port, this: usize, [w, a, b, c]: [usize; 4], [tex0, tex1]: [usize; 2], [tw, th]: [u16; 2], rec: usize, again: bool, flag: u8, view: u32, o: &Objects) -> R<()> {
    let t = alloc_target(p, w, VOLUME_MASK_NAME, 0x1d, tw as u32, th as u32)?;
    let elem = p.m.r64(this + 0x30).wrapping_add(clamp_index(view, p.m.r32(this + 0x28)).wrapping_mul(0xf8));
    p.m.w::<u64>(elem, t as u64);
    let (rb, obj) = (o.rb(), o.target(0));
    p.m.w::<u64>(rb + 0x20, obj as u64);
    copy_target(p, obj, t)?;
    let (ow, oh) = level_size(p, obj, true);
    let (fw, fh) = set_extent(p, rb, ow, oh);
    let vp = viewport::full(p, fw, fh);
    targets::bind(p, rb, w)?;
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let prog = p.m.r64(this + 8);
    program_apply(p, prog, w)?;
    let block: [u32; 4] = [p.m.r32(this + 0x20), 0, 0, 0];
    let ring = p.ring_block(block.as_ptr() as usize, 0x10);
    program_block(p, prog, w, 0, ring, 0)?;
    program_texture(p, prog, w, 0, tex0)?;
    program_texture(p, prog, w, 1, tex1)?;
    program_texture(p, prog, w, 2, c)?;
    fullscreen(p, w);
    targets::unbind(p, w)?;
    barrier_if_slot(p, obj + 0x60, w);
    if again {
        let count = p.m.r32(rec + 0x220);
        let r = p.m.r64(rec + 0x228) + clamp_index(view, count) * 0x728;
        copy_target(p, obj, p.m.r64(r + 0x10))?;
        let (ow, oh) = level_size(p, obj, false);
        let (fw, fh) = set_extent(p, rb, ow, oh);
        let vp = viewport::full(p, fw, fh);
        viewport::apply(p, vp.as_ptr() as usize, w, rb);
        // Clear values: colours (0, 0, 0, 1), depth 1, stencil 0.
        let mut values = [0u32; 0x22];
        for k in 0..8 {
            values[k * 4 + 3] = 1f32.to_bits();
        }
        values[0x20] = 1f32.to_bits();
        targets::bind_clear(p, rb, w, vp.as_ptr() as usize, flag as i8 as i32 as u32, values.as_ptr() as usize)?;
        draw_pass(p, b, w, p.m.r64(a + 0x1ec0), 0x18, 0, 0, 0)?;
        targets::unbind(p, w)?;
        barrier_if_slot(p, obj + 0x60, w);
    }
    release(p, obj + 8);
    let src = p.m.r64(elem);
    assign(p, elem + 8, src)?;
    p.m.fetch_or32(elem + 0xec, 1);
    Ok(())
}

/// main+0x957820(this, w, view, flags, tex, desc, depth): a pass into a colour target copied from `desc` and a depth
/// target main+0x9580c4 copies from `depth`: render state (blend and write bits from flags bit 2, colour mask by bit 1,
/// +0x64 = 8), the buffer bound without clears, the program the flags' variant counts select from [[this+0x28]+8], the
/// view's record (+0x18, 0x7400 each) as block 0 and storage block 0 (+0x7328), `tex` at slot 0, the full-screen
/// triangle.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn view_target_draw(p: &mut Port, this: usize, w: usize, view: u32, flags: u32, tex: usize, desc: usize, depth: usize) -> R<()> {
    let o = Objects::new(p, &[COLOR_VTABLE, DEPTH_VTABLE]);
    let r = view_target_pass(p, this, w, view, flags, tex, desc, depth, &o);
    o.release(p);
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn view_target_pass(p: &mut Port, this: usize, w: usize, view: u32, flags: u32, tex: usize, desc: usize, depth: usize, o: &Objects) -> R<()> {
    let mut g = default_state(p);
    let b = g.as_mut_ptr() as *mut u8;
    *b = ((flags >> 2) & 1) as u8;
    g[1] = 0;
    *b.add(1) = 1;
    *b.add(0x64) = 8;
    g[7] = if (flags >> 1) & 1 != 0 { 0xf } else { 0 };
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let (rb, c, d) = (o.rb(), o.target(0), o.target(1));
    copy_target(p, c, desc)?;
    copy_target(p, d, depth)?;
    let (cw, ch) = level_size(p, c, true);
    let (fw, fh) = set_extent(p, rb, cw, ch);
    p.m.w::<u64>(rb + 0x20, c as u64);
    p.m.w::<u64>(rb + 0x60, d as u64);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let cc = p.pfn(CLEAR_COLOR);
    let (lo, hi) = (p.m.r64(cc) as u64, p.m.r64(cc + 8) as u64);
    let mut values = [0u64; 0x11];
    for k in 0..8 {
        values[k * 2] = lo;
        values[k * 2 + 1] = hi;
    }
    values[0x10] = 1f32.to_bits() as u64;
    targets::bind_clear(p, rb, w, vp.as_ptr() as usize, 0, values.as_ptr() as usize)?;
    // The program: the variant counts of the sets the flags select, one-based; none = the first program.
    let set = p.m.r64(p.m.r64(this + 0x28) + 8);
    let lv = p.m.r64(set + 0x30);
    let n = p.m.r32(set + 0x28);
    let base = if flags & 0x10 == 0 {
        let x = p.m.r16(lv + 0x20) as u32;
        if flags & 8 == 0 { x << 1 } else { x }
    } else {
        0
    };
    let a = if flags & 1 != 0 { 0 } else { p.m.r16(lv + if n > 1 { 0x28 } else { 0 } + 0x20) as u32 };
    let cnt = if flags & 4 != 0 { p.m.r16(lv + if n < 3 { 0 } else { 0x50 } + 0x20) as u32 } else { 0 };
    let dd = if flags & 2 != 0 { p.m.r16(lv + if n < 4 { 0 } else { 0x78 } + 0x20) as u32 } else { 0 };
    let sel = a.wrapping_add(base).wrapping_add(cnt).wrapping_add(dd);
    let prog = if sel == 0 {
        p.m.r64(set + 0x10)
    } else {
        let i = if p.m.r32(set + 0x18) <= sel - 1 { 0 } else { sel - 1 };
        p.m.r64(set + 0x20) + i as usize * 0x158
    };
    program_apply(p, prog, w)?;
    let rec = p.m.r64(this + 0x18).wrapping_add(clamp_index(view, p.m.r32(this + 0x10)).wrapping_mul(0x7400));
    program_block(p, prog, w, 0, rec, 0)?;
    program_storage_block(p, prog, w, 0, rec + 0x7328, 0)?;
    program_texture(p, prog, w, 0, tex)?;
    fullscreen(p, w);
    targets::unbind(p, w)
}

/// main+0xe01cb8(this, w, _, ctx), with +0x352 bit 1: the bound render buffer (its virtual +0x28 the known unbind)
/// unbound; a colour target copied from ctx[1] (layer ctx +0x18) bound alone and cleared to white; with ctx +0x1c,
/// ctx[0] drawn through [[[this+0x368]+8]+0x20] as a texture with a sampler made from it and the scene block 2 (ctx[2]
/// +0x18, 0x430 each, +0x390); then the target unbound and the first buffer bound again. Applying only: the stack
/// texture is finished by the game.
pub(super) unsafe fn target_blit(p: &mut Port, this: usize, w: usize, ctx: usize) -> R<()> {
    if (p.m.r8(this + 0x352) >> 1) & 1 == 0 {
        return Ok(());
    }
    let cur = p.m.r64(w + 0xf8);
    if cur == 0 {
        return Err("no render buffer bound");
    }
    if p.vfn(cur, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    let o = Objects::new(p, &[COLOR_VTABLE]);
    let r = target_blit_pass(p, this, w, ctx, cur, &o);
    o.release(p);
    r
}

unsafe fn target_blit_pass(p: &mut Port, this: usize, w: usize, ctx: usize, cur: usize, o: &Objects) -> R<()> {
    targets::unbind(p, w)?;
    let layer = p.m.r32(ctx + 0x18);
    let (rb, t) = (o.rb(), o.target(0));
    p.m.w::<u16>(t + 0xcc, layer as u16);
    copy_target(p, t, p.m.r64(ctx + 8))?;
    let (tw, th) = level_size(p, t, true);
    let (fw, fh) = set_extent(p, rb, tw, th);
    p.m.w::<u64>(rb + 0x20, t as u64);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    // Clear values: every colour 1.0, depth 1.0, stencil 0.
    let mut values = [0u32; 0x22];
    values[..0x21].fill(1f32.to_bits());
    targets::bind_clear(p, rb, w, vp.as_ptr() as usize, 1, values.as_ptr() as usize)?;
    if p.m.r8(ctx + 0x1c) != 0 {
        let prog = p.m.r64(p.m.r64(p.m.r64(this + 0x368) + 8) + 0x20);
        program_apply(p, prog, w)?;
        let u = Host::new(p, 0x100);
        let r = sampled_draw(p, w, ctx, layer, prog, u.addr());
        u.release(p);
        r?;
    }
    targets::unbind(p, w)?;
    targets::bind(p, cur, w)
}

/// ctx[0] as a stack texture `u` with a sampler made from it (the game's inline constructor and copy), at slot 0 of
/// `prog`, the scene block 2 and the full-screen triangle; the sampler and slot references released after.
unsafe fn sampled_draw(p: &mut Port, w: usize, ctx: usize, layer: u32, prog: usize, u: usize) -> R<()> {
    let src = p.m.r64(ctx);
    for o in (0..0x58).step_by(8) {
        p.m.w::<u64>(u + o, p.m.r64(src + o) as u64);
    }
    p.m.w::<u32>(u + 0x58, p.m.r32(src + 0x58));
    for o in (0x60..0xb0).step_by(8) {
        p.m.w::<u64>(u + o, p.m.r64(src + o) as u64);
    }
    p.m.w::<u64>(u + 0xb0, (p.base + SAMPLER_NAME) as u64);
    p.m.w::<u64>(u + 0xc0, 0x3f80_0000_0000_0000);
    p.m.w::<u64>(u + 0xc8, 0x4170_0000_0000_0000);
    p.m.w::<u64>(u + 0xd4, 0x0101_0707_0700_0000);
    p.m.w::<u64>(u + 0xe0, 0xff_0504_0302);
    p.m.w::<u8>(u + 0xe8, 0x20);
    p.m.w::<u16>(u + 0xea, 0xffff);
    acquire(p, u)?;
    program_texture(p, prog, w, 0, u)?;
    let ctx2 = p.m.r64(ctx + 0x10);
    let rec = p.m.r64(ctx2 + 0x18).wrapping_add(clamp_index(layer, p.m.r32(ctx2 + 0x10)).wrapping_mul(0x430));
    p.m.w::<u8>(rec + 0x421, p.m.r8(rec + 0x421) | 1);
    program_block(p, prog, w, 2, rec + 0x390, 0)?;
    fullscreen(p, w);
    release_sampler(p, u + 0xea);
    release(p, u);
    Ok(())
}

/// main+0x9590f8(this, w, view, [scene, grid, storage]), with +0x350 bit 11: a "CorrectDepth" temporary target (format 0x1d,
/// the view record's (grid +0x10, 0x21b0 each) +0x1670 size, halved with bit 14; +0x7f0's with bit 15) drawn with
/// program 2 (3 with bit 12) of [+0x368]+8: the scene blocks, storage block 1 ([storage +0x18] +0x7328, 0x7400 each),
/// textures [+0x2118]+0x208 and the size's source, over the record's grid, after a barrier; cleared with bit 12, which
/// also opens its colour mask. The target is given back before the unbind. Applying only: the allocation is past the
/// commit point.
pub(super) unsafe fn correct_depth(p: &mut Port, this: usize, w: usize, view: u32, ctx: usize) -> R<()> {
    let flags = p.m.r32(this + 0x350);
    if (flags >> 11) & 1 == 0 {
        return Ok(());
    }
    let grid = p.m.r64(ctx + 8);
    let rec = p.m.r64(grid + 0x10).wrapping_add(clamp_index(view, p.m.r32(grid + 8)).wrapping_mul(0x21b0));
    let cleared = (flags >> 12) & 1;
    let programs = p.m.r64(p.m.r64(this + 0x368) + 8);
    let index = if cleared != 0 { 3u32 } else { 2 };
    let index = if p.m.r32(programs + 0x18) <= index { 0 } else { index };
    let prog = p.m.r64(programs + 0x20) + index as usize * 0x158;
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let o = Objects::new(p, &[COLOR_VTABLE]);
    let r = correct_depth_pass(p, this, w, view, ctx, rec, flags, prog, &o);
    o.release(p);
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn correct_depth_pass(p: &mut Port, this: usize, w: usize, view: u32, ctx: usize, rec: usize, flags: u32, prog: usize, o: &Objects) -> R<()> {
    let size = |p: &Port, s: usize| {
        let h = p.m.r16(s + 0x32) as u32;
        ((p.m.r16(s + 0x30) as u32).max(1), if p.m.r16(s + 0x3a) != 3 { 1 } else { h }.max(h))
    };
    let cleared = (flags >> 12) & 1;
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = if cleared != 0 { 0xf } else { 0 };
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let (mut source, (mut tw, mut th)) = (rec + 0x1670, size(p, rec + 0x1670));
    if (flags >> 14) & 1 != 0 {
        tw >>= 1;
        th >>= 1;
    } else if (flags >> 15) & 1 != 0 {
        source = rec + 0x7f0;
        (tw, th) = size(p, source);
    }
    program_apply(p, prog, w)?;
    scene_blocks(p, this, w, view, prog, p.m.r64(ctx))?;
    let storage = p.m.r64(ctx + 0x10);
    let s = clamp_index(view, p.m.r32(storage + 0x10)).wrapping_mul(0x7400);
    program_storage_block(p, prog, w, 1, p.m.r64(storage + 0x18).wrapping_add(s) + 0x7328, 0)?;
    program_texture(p, prog, w, 0, p.m.r64(rec + 0x2118) + 0x208)?;
    program_texture(p, prog, w, 1, source)?;
    p.m.commit();
    // commit
    let t = alloc_target(p, w, CORRECT_DEPTH_NAME, 0x1d, tw, th)?;
    let (rb, c) = (o.rb(), o.target(0));
    copy_target(p, c, t)?;
    let (cw, ch) = level_size(p, c, true);
    let (fw, fh) = set_extent(p, rb, cw, ch);
    p.m.w::<u64>(rb + 0x20, c as u64);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let settings = p.m.r64(p.m.r64(p.base + SETTINGS));
    let cmd = p.m.r64(w + 0xb8);
    p.barrier(cmd, if (p.m.r8(settings + 0x50c) >> 1) & 1 == 0 { 0x11 } else { 0x111 });
    let cc = p.pfn(CLEAR_COLOR);
    let (lo, hi) = (p.m.r64(cc) as u64, p.m.r64(cc + 8) as u64);
    let mut values = [0u64; 0x11];
    for k in 0..8 {
        values[k * 2] = lo;
        values[k * 2 + 1] = hi;
    }
    values[0x10] = 1f32.to_bits() as u64;
    targets::bind_clear(p, rb, w, vp.as_ptr() as usize, cleared, values.as_ptr() as usize)?;
    instanced_quad(p, w, rec);
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    p.helper("depth of field gather", GATHER, &[pool as u64, t as u64])?;
    targets::unbind(p, w)
}
