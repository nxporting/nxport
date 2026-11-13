// TotK's lighting pass main+0x973b40(this, w, view, args), called by main+0x96e6fc once per view: the view's texture
// copied into a colour target and drawn into with the inner-mask model passes and the light volumes (the second state
// of the lights' groups), then, with this +0x30 bit 2, the blurred mask: the view texture drawn through a program into
// itself, a quarter-size record (a texture of the cache main+0x9773e8) filled from it, and +0x70 rounds of a horizontal
// pass into a temporary target and a vertical one back, the result kept in the view record (+0x1f0). The game builds
// its render buffers, targets and texture copies as stack objects; they are host objects here (byte layout as the
// game's, only the fields anything reads). The first game call past the commit point is the downsample draw
// (main+0xc267dc / main+0xa675b0); the temporary target allocation, the frame block fill and the target hand-back follow.

use super::draw::{Port, R};
use super::lights::{draw as lights_draw, shape_state, stream_draw, SHAPES, STREAMS};
use super::scene_passes::default_state;
use super::targets;
use super::texture_slots::{alloc_target, assign, barrier_if_slot, copy_target, level_size, new_buffer, new_target, release, release_sampler, set_extent, texture_assign, Host};
use super::texture_slots::{BUFFER_BYTES, COLOR_VTABLE, DEPTH_VTABLE, TARGET_BYTES};
use super::vfx::{program_apply, program_block, program_storage_block, program_texture, shape_draw};
use super::{render_list, viewport};

/// [[holder]]: the texture slot table's holder (sampler table +0x718 mutex, +0x738 count, +0x740 entries, +0x758 first id).
const HOLDER: usize = 0x462bbf0;
const LOCK: usize = 0x2b15370;
const UNLOCK: usize = 0x2b15380;
/// [var]: the 16-byte clear colour, and the values pointer main+0x8660b4 is given for its unused clears.
const CLEAR_COLOR: usize = 0x4634ea0;
const BIND_VALUES: usize = 0x4634f28;
/// The argument block's vtable of the model passes, and the names of the two inner-mask passes.
const PASS_ARG_VTABLE: usize = 0x4634f18;
const SUB_NAME: usize = 0x3704318;
const ADD_NAME: usize = 0x3746a6e;
/// "InnerMask(Blur0)", the second variable of the downsample calls.
const BLUR_NAME: usize = 0x37b92ff;
const DOWNSAMPLE_DATA: usize = 0x462cee0;
/// main+0xc267dc(w, texture, viewport, level, data; scale in s0) / main+0xa675b0(w, texture, viewport, scale pair, data):
/// the downsample draws (both emit NVN themselves).
const DOWNSAMPLE_SMALL: usize = 0xc267dc;
const DOWNSAMPLE_LARGE: usize = 0xa675b0;
/// The scene's per-frame block fill (main+0x765148(scene, 0)), the target hand-back (main+0x7f0354(pool, state)) and
/// the pool holder.
const FILL_FRAME_BLOCK: usize = 0x765148;
const GATHER: usize = 0x7f0354;
const TARGET_POOL: usize = 0x4634e70;

fn byte(g: &mut [u32; 0x20], o: usize, v: u8) {
    unsafe { *(g.as_mut_ptr() as *mut u8).add(o) = v }
}

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

/// The host objects of one run, given back at its end.
struct Objs(Vec<Host>);

impl Objs {
    fn keep(&mut self, h: Host) -> usize {
        let a = h.addr();
        self.0.push(h);
        a
    }
}

/// A target object of `vtable` given the state at `src`.
unsafe fn target(p: &mut Port, o: &mut Objs, vtable: usize, src: usize) -> R<usize> {
    let h = Host::new(p, TARGET_BYTES);
    new_target(p, h.addr(), vtable);
    let a = o.keep(h);
    copy_target(p, a, src)?;
    Ok(a)
}

/// A program set's program still being set up would stop the port inside a draw.
unsafe fn ready(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// A sampler id's use count raised (the table under its mutex), the inverse of texture_slots::release_sampler.
unsafe fn acquire_sampler(p: &mut Port, a: usize) {
    let s = p.m.r16(a) as i16;
    if s == -1 {
        return;
    }
    let h = p.m.r64(p.m.r64(p.base + HOLDER));
    if !p.m.predict() {
        p.query(p.base + LOCK, &[(h + 0x718) as u64]);
    }
    let mut i = (s as i32 as u32).wrapping_sub(p.m.r16(h + 0x758) as u32);
    if p.m.r32(h + 0x738) <= i {
        i = 0;
    }
    let e = p.m.r64(h + 0x740).wrapping_add((i as i32 as isize as usize).wrapping_mul(0x80));
    p.m.w::<u16>(e + 0x76, p.m.r16(e + 0x76).wrapping_add(1));
    if !p.m.predict() {
        p.query(p.base + UNLOCK, &[(h + 0x718) as u64]);
    }
}

/// main+0x9773e8(owner, view, index, width, height, format): the cached texture record of the view (owner +0x10, 0x570
/// each, count +8): its entry `index` (+0x560, 0x2e8 each, count +0x558) when it already has the size (the request
/// limited by +0x2d8 / +0x2dc) and format; else the game reallocates it.
unsafe fn mask_record(p: &Port, owner: usize, view: u32, index: u32, width: u32, height: u32, format: u8) -> R<usize> {
    let v = if p.m.r32(owner + 8) > view { view } else { 0 } as i32 as isize as usize;
    let rec = p.m.r64(owner + 0x10).wrapping_add(v.wrapping_mul(0x570));
    let count = p.m.r32(rec + 0x558);
    let i = if count > index { index } else { 0 } as i32 as isize as usize;
    let e = p.m.r64(rec + 0x560).wrapping_add(i.wrapping_mul(0x2e8));
    let (lw, lh) = (p.m.i32(e + 0x2d8), p.m.i32(e + 0x2dc));
    let (w, h) = ((width as i32).min(lw), (height as i32).min(lh));
    let cw = (p.m.r16(e + 0x30) as u32).max(1);
    let h0 = p.m.r16(e + 0x32) as u32;
    let ch = (if p.m.r16(e + 0x3a) == 3 { h0 } else { 1 }).max(h0);
    if cw != w as u32 || ch != h as u32 || p.m.r8(e + 0xa0) != format {
        return Err("inner mask texture reallocated by the game");
    }
    Ok(e)
}

/// The texture of the view's record (+0x3e8 of its 0xb90-byte entry) with its sampler, as the game's inline copy
/// constructor makes it on its stack.
unsafe fn texture_copy(p: &mut Port, o: &mut Objs, src: usize) -> R<usize> {
    let h = Host::new(p, 0x100);
    let t = o.keep(h);
    p.m.w::<i32>(t + 0x58, -1);
    assign(p, t, src)?;
    for off in (0xb0..0xe4).step_by(4) {
        p.m.w::<u32>(t + off, p.m.r32(src + off));
    }
    p.m.w::<u32>(t + 0xe4, 0x7fff_ffff);
    p.m.w::<u8>(t + 0xe8, p.m.r8(src + 0xe8));
    p.m.w::<u16>(t + 0xea, p.m.r16(src + 0xea));
    acquire_sampler(p, t + 0xea);
    Ok(t)
}

/// main+0x973b40(this, w, view, args).
pub(super) unsafe fn lighting(p: &mut Port, this: usize, w: usize, view: u32, args: usize) -> R<()> {
    let mut o = Objs(Vec::new());
    let r = lighting_in(p, &mut o, this, w, view, args);
    for h in o.0 {
        h.release(p);
    }
    r
}

unsafe fn lighting_in(p: &mut Port, o: &mut Objs, this: usize, w: usize, view: u32, args: usize) -> R<()> {
    let flags = p.m.r32(this + 0x30);
    let rec = p.m.r64(this + 0x40).wrapping_add(clamp_index(view, p.m.r32(this + 0x38)).wrapping_mul(0x440));
    let a1 = p.m.r64(args + 8);
    // The inner-mask renderer's pass is on (+0x2c count, +0x16 bit 6).
    let renderer = p.m.r64(a1 + 0x600);
    let inner = renderer != 0 && p.m.i32(renderer + 0x2c) > 0 && p.m.r8(renderer + 0x16) & 0x40 != 0;
    let lights = if p.m.i32(rec + 0x420) < 1 && p.m.i32(rec + 0x430) < 1 { false } else { (flags >> 5) & 1 != 0 };
    let grid_set = p.m.r64(p.m.r64(args) + 0x1ec0);
    let grid = p.m.r64(grid_set + 0x10).wrapping_add(clamp_index(view, p.m.r32(grid_set + 8)).wrapping_mul(0x21b0));
    let depth_src = grid + 0x12a0;
    let texs = p.m.r64(args + 0x20);
    let tex_rec = p.m.r64(texs + 0x18).wrapping_add(clamp_index(view, p.m.r32(texs + 0x10)).wrapping_mul(0xb90));
    // The view texture state of the record (head +0x3e8; its sampler section ends at +0x4d8).
    let view_tex = tex_rec + 0x3e8;
    let blur = flags & 4 != 0;
    let rounds = p.m.r8(this + 0x70) as u32;

    // Everything the run could stop at, before anything is written.
    if p.m.r8(p.m.r64(p.base + targets::CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    let light_prog = if lights { p.m.r64(p.m.r64(p.m.r64(this + 0x68) + 8) + 0x10) } else { 0 };
    if lights {
        ready(p, light_prog)?;
    }
    let sets = p.m.r64(p.m.r64(this + 0x50) + 8);
    let (set_base, set_count) = (p.m.r64(sets + 0x20), p.m.r32(sets + 0x18));
    let first_prog = set_base + (set_count > 1) as usize * 0x158;
    let x_prog = set_base + if set_count < 3 { 0 } else { 0x2b0 };
    let y_prog = |idx: u32| set_base + (if set_count <= idx { 0 } else { idx }) as usize * 0x158;
    let (qw, qh);
    if blur {
        let w0 = p.m.r16(view_tex + 0x30);
        let h0 = p.m.r16(view_tex + 0x32);
        let full_h = (if p.m.r16(view_tex + 0x3a) == 3 { h0 } else { 1 }).max(h0);
        qw = ((w0 >> 2) as u32).max(1);
        qh = ((full_h >> 2) as u32).max(1);
        mask_record(p, p.m.r64(args + 0x18), view, 1, qw, qh, 0x1d)?;
        ready(p, first_prog)?;
        if rounds != 0 {
            ready(p, x_prog)?;
            ready(p, y_prog(3))?;
            ready(p, y_prog(4))?;
            let frame = p.m.r64(a1 + 0x5a0);
            if p.m.predict() && p.m.r32(frame + 0xc8) == 0 {
                return Err("scene frame block filled by the call");
            }
        }
    } else {
        qw = 0;
        qh = 0;
    }

    // The view texture as a colour target and the grid record's depth target, the buffer over them.
    let t1 = target(p, o, COLOR_VTABLE, view_tex)?;
    let d = target(p, o, DEPTH_VTABLE, depth_src)?;
    let rb1 = {
        let h = Host::new(p, BUFFER_BYTES);
        new_buffer(p, h.addr());
        o.keep(h)
    };
    // Width not stored back, the height is.
    let level = (p.m.r8(t1 + 0xce) & 0x1f) as u32;
    let aw = ((p.m.r16(t1 + 0x38) as u32 >> level) as u16).max(1);
    let h0 = p.m.r16(t1 + 0x3a);
    let ah = (if p.m.r16(t1 + 0x42) != 3 { 1 } else { h0 }).max((h0 as u32 >> level) as u16);
    p.m.w::<u16>(t1 + 0x3a, ah);
    set_extent(p, rb1, aw, ah);
    p.m.w::<u64>(rb1 + 0x20, t1 as u64);
    p.m.w::<u64>(rb1 + 0x60, d as u64);
    let vp = viewport::full(p, aw as f32, ah as f32);
    viewport::apply(p, vp.as_ptr() as usize, w, rb1);
    let values = p.pfn(BIND_VALUES);
    targets::bind_clear(p, rb1, w, vp.as_ptr() as usize, 0, values)?;

    // Inner mask: the model passes of the renderer, subtracting then adding.
    if inner && (flags >> 4) & 1 != 0 {
        for (kind, id, name) in [(0x88_0000_0000u64, 0x11u32, SUB_NAME), (0x0004_0088_0000_0000, 0x12, ADD_NAME)] {
            let a = Host::new(p, 0x90);
            let ad = a.addr();
            let vt = p.pfn(PASS_ARG_VTABLE) as u64 + 0x10;
            p.m.w::<u64>(ad, 0x1_0040_0000);
            p.m.w::<u64>(ad + 8, kind);
            p.m.w::<u64>(ad + 0x10, 0xffff);
            p.m.w::<u32>(ad + 0x1c, 1);
            p.m.w::<u32>(ad + 0x20, 1);
            p.m.w::<u32>(ad + 0x24, 1);
            p.m.w::<u32>(ad + 0x2c, 0xffff_ffff);
            p.m.w::<u32>(ad + 0x30, id);
            for off in [0x38usize, 0x3c, 0x40, 0x44] {
                p.m.w::<f32>(ad + off, 1.0);
            }
            p.m.w::<u32>(ad + 0x4c, 0xffff_ffff);
            p.m.w::<u64>(ad + 0x50, vt);
            p.m.w::<u64>(ad + 0x70, (p.base + name) as u64);
            let r = render_list::cached_model_pass(p, p.m.r64(a1 + 0x600), w, ad, a1, 0, p.m.r64(a1 + 0x5e8) as u64);
            a.release(p);
            r?;
        }
    }

    // The light volumes: two passes of the lights' own draw, then the groups' lists.
    if lights {
        let mut g = default_state(p);
        g[0] = (g[0] & 0xffff_0000) | 1;
        g[1] = 1;
        g[7] = 2;
        for k in 0..4 {
            byte(&mut g, 0x28 + k, 2);
        }
        byte(&mut g, 0x2c, 3);
        byte(&mut g, 0x2d, 3);
        byte(&mut g, 0x64, 5);
        byte(&mut g, 0x65, 1);
        p.apply_graphics_context(g.as_ptr() as usize, w);
        lights_draw(p, this, w, view, args)?;
        let mut g = default_state(p);
        g[0] = (g[0] & 0xffff_0000) | 1;
        g[1] = 1;
        g[7] = 2;
        for k in 0..4 {
            byte(&mut g, 0x28 + k, 2);
        }
        byte(&mut g, 0x64, 5);
        p.apply_graphics_context(g.as_ptr() as usize, w);
        lights_draw(p, this, w, view, args)?;
        let mut g = default_state(p);
        g[0] = (g[0] & 0xffff_0000) | 1;
        g[1] = 1;
        g[7] = 2;
        for k in 0..4 {
            byte(&mut g, 0x28 + k, 2);
        }
        byte(&mut g, 0x2c, 3);
        byte(&mut g, 0x2d, 3);
        byte(&mut g, 0x64, 4);
        p.apply_graphics_context(g.as_ptr() as usize, w);
        light_groups(p, this, w, view, args, light_prog)?;
    }
    targets::unbind(p, w)?;
    barrier_if_slot(p, t1 + 0x60, w);
    release(p, d + 8);
    release(p, t1 + 8);
    if !blur {
        return Ok(());
    }
    blurred_mask(p, o, this, w, view, a1, args, rec, view_tex, [qw, qh], rounds, first_prog, x_prog, set_base, set_count)
}

/// The inline copy of main+0x9777c4's group part for the stencil-style pass: the lights program with the scene block 2,
/// then per group not seen by the view (+0x68 bit clear) the lists at +0x10 (per record: shape +8), +0x20 (shape +8),
/// +0x30 (shape +0x968) and +0x40 (shape +0x5a8), every draw using the index stream +0x728.
unsafe fn light_groups(p: &mut Port, this: usize, w: usize, view: u32, args: usize, prog: usize) -> R<()> {
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    program_apply(p, prog, w)?;
    let scene = p.m.r64(p.m.r64(args + 8) + 0x18);
    p.m.w::<u8>(scene + 0x421, p.m.r8(scene + 0x421) | 1);
    program_block(p, prog, w, 2, scene + 0x390, 0)?;
    let rec = p.m.r64(this + 0x40).wrapping_add(clamp_index(view, p.m.r32(this + 0x38)).wrapping_mul(0x440));
    let groups = p.m.r64(rec + 0x438);
    for k in 0..p.m.i32(rec + 0x430).max(0) as usize {
        let g = p.m.r64(groups + k * 8);
        if (p.m.r32(g + 0x68) >> (view & 31)) & 1 != 0 {
            continue;
        }
        for (list, shape) in [(0x10usize, 8usize), (0x20, 8), (0x30, 0x968), (0x40, 0x5a8)] {
            if list != 0x10 {
                shape_state(p, shapes + shape, w);
            }
            let records = p.m.r64(g + list + 8);
            for j in 0..p.m.r32(g + list) as usize {
                let block = p.ring_block(records + j * 0x30, 0x30);
                program_block(p, prog, w, 1, block, 0)?;
                if list == 0x10 {
                    shape_draw(p, shapes + shape, streams + 0x728, w, 1);
                } else {
                    stream_draw(p, streams + 0x728, w);
                }
            }
        }
    }
    Ok(())
}

/// The blurred mask (this +0x30 bit 2).
#[allow(clippy::too_many_arguments)]
unsafe fn blurred_mask(
    p: &mut Port,
    o: &mut Objs,
    this: usize,
    w: usize,
    view: u32,
    a1: usize,
    args: usize,
    rec: usize,
    view_tex: usize,
    [qw, qh]: [u32; 2],
    rounds: u32,
    first_prog: usize,
    x_prog: usize,
    set_base: usize,
    set_count: u32,
) -> R<()> {
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    let r = mask_record(p, p.m.r64(args + 0x18), view, 1, qw, qh, 0x1d)?;
    // The record's size and the one-over-size block (this +0xf8 last).
    let rw = (p.m.r16(r + 0x30)).max(1) as u32;
    let rh0 = p.m.r16(r + 0x32);
    let rh = (if p.m.r16(r + 0x3a) == 3 { rh0 } else { 1 }).max(rh0) as u32;
    let full_w = (p.m.r16(view_tex + 0x30)).max(1) as u32;
    let mut data = [0u32; 4];
    data[0] = (1.0 / rw as f32).to_bits();
    data[1] = (1.0 / rh as f32).to_bits();
    data[3] = p.m.r32(this + 0xf8);
    let blk = p.ring_block(data.as_ptr() as usize, 0x10);
    let ratio = full_w as f32 / rw as f32;

    // The view texture again as a colour target: drawn into through the first program, sampling itself.
    let d2 = target(p, o, COLOR_VTABLE, view_tex)?;
    let rb2 = {
        let h = Host::new(p, BUFFER_BYTES);
        new_buffer(p, h.addr());
        o.keep(h)
    };
    let (dw, dh) = level_size(p, d2, true);
    let (fw, fh) = set_extent(p, rb2, dw, dh);
    p.m.w::<u64>(rb2 + 0x20, d2 as u64);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rb2);
    shape_state(p, shapes + 0x10e8, w);
    program_apply(p, first_prog, w)?;
    program_block(p, first_prog, w, 2, blk, 0)?;
    program_storage_block(p, first_prog, w, 0, p.m.r64(args + 0x10) + 0x1c0, 0)?;
    program_texture(p, first_prog, w, 2, view_tex)?;
    g[7] = 7;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let cc = p.pfn(CLEAR_COLOR);
    let (lo, hi) = (p.m.r64(cc) as u64, p.m.r64(cc + 8) as u64);
    let mut values = [0u64; 0x11];
    for k in 0..8 {
        values[k * 2] = lo;
        values[k * 2 + 1] = hi;
    }
    values[0x10] = 1f32.to_bits() as u64;
    targets::bind_clear(p, rb2, w, vp.as_ptr() as usize, 0, values.as_ptr() as usize)?;
    stream_draw(p, streams + 0x488, w);
    targets::unbind(p, w)?;
    barrier_if_slot(p, d2 + 0x60, w);

    // The target now the record, drawn by the downsample call.
    copy_target(p, d2, r)?;
    let (dw, dh) = level_size(p, d2, false);
    let (fw, fh) = set_extent(p, rb2, dw, dh);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rb2);
    g[7] = 0xf;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    targets::bind(p, rb2, w)?;
    p.m.commit();
    // commit
    let data_ptr = p.pfn(DOWNSAMPLE_DATA) as u64;
    if ratio > 4.0 {
        let s = 1.0 / ratio;
        let pair = [s.to_bits(), s.to_bits()];
        p.helper("downsample draw", DOWNSAMPLE_LARGE, &[w as u64, view_tex as u64, vp.as_ptr() as u64, pair.as_ptr() as u64, data_ptr])?;
    } else {
        // main+0xc267dc(w, texture, viewport, level, data) with the scale 1.0 in s0.
        p.flush();
        p.query_f(p.base + DOWNSAMPLE_SMALL, &[w as u64, view_tex as u64, vp.as_ptr() as u64, 2, data_ptr], &[1.0]);
    }
    targets::unbind(p, w)?;
    barrier_if_slot(p, d2 + 0x60, w);

    // A temporary target of the record's size for the horizontal passes.
    let r2 = alloc_target(p, w, BLUR_NAME, 0x1d, rw, rh)?;
    shape_state(p, shapes + 0x10e8, w);
    let tex = texture_copy(p, o, view_tex)?;
    let ctx_values = (p.m.r8(this + 0x71) as u32, p.m.r8(this + 0x72) as u32);
    for k in 0..rounds {
        let sel: u32 = if ctx_values.0 <= k { 9 } else { 0xb };
        g[7] = (sel & 0xffff_fff8) | (sel & 3) | (((k < ctx_values.1) as u32) << 2);
        p.apply_graphics_context(g.as_ptr() as usize, w);
        // Horizontal: the target's contents into the temporary one.
        program_apply(p, x_prog, w)?;
        program_block(p, x_prog, w, 2, blk, 0)?;
        texture_assign(p, tex, d2 + 8)?;
        program_texture(p, x_prog, w, 2, tex)?;
        copy_target(p, d2, r2)?;
        let (dw, dh) = level_size(p, d2, false);
        set_extent(p, rb2, dw, dh);
        targets::bind(p, rb2, w)?;
        stream_draw(p, streams + 0x488, w);
        targets::unbind(p, w)?;
        barrier_if_slot(p, d2 + 0x60, w);
        // Vertical: back into the record (the last round's program differs).
        let idx = if k as i32 == ctx_values.1 as i32 - 1 { 4 } else { 3 };
        let prog = set_base + (if set_count <= idx { 0 } else { idx }) as usize * 0x158;
        program_apply(p, prog, w)?;
        program_block(p, prog, w, 2, blk, 0)?;
        let frame = p.m.r64(a1 + 0x5a0);
        if p.m.r32(frame + 0xc8) == 0 {
            p.m.fetch_or32(frame + 0xc8, 1);
            p.call(p.base + FILL_FRAME_BLOCK, &[a1 as u64, 0]);
        } else {
            p.m.fetch_or32(frame + 0xc8, 1);
        }
        program_block(p, prog, w, 0, frame, 0)?;
        program_storage_block(p, prog, w, 0, p.m.r64(args + 0x10) + 0x1c0, 0)?;
        texture_assign(p, tex, d2 + 8)?;
        program_texture(p, prog, w, 2, tex)?;
        copy_target(p, d2, r)?;
        let (dw, dh) = level_size(p, d2, false);
        set_extent(p, rb2, dw, dh);
        targets::bind(p, rb2, w)?;
        stream_draw(p, streams + 0x488, w);
        targets::unbind(p, w)?;
        barrier_if_slot(p, d2 + 0x60, w);
    }
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    p.call(p.base + GATHER, &[pool as u64, r2 as u64]);
    release_sampler(p, tex + 0xea);
    release(p, tex);
    // The record kept in the view's (+0x1f0).
    assign(p, rec + 0x1f0, r)?;
    p.m.fetch_or32(rec + 0x2d4, 1);
    release(p, d2 + 8);
    Ok(())
}
