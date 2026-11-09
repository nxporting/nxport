// Passes of the scene renderer that work on a view's records of render targets: the view gather (main+0xd4cac4), the
// per-view target passes (main+0xbd8c40 with its helpers main+0xbd94b8 / main+0xbe25f4 / main+0xbe2528 / main+0xbe1950),
// the occlusion buffers (main+0xbdad04) and the noise gauge (main+0xdb6438). The last two allocate temporary targets
// from the dynamic allocator (game calls): they run only past the commit point, so they are checked in apply mode only.

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, stream_draw_n, SHAPES, STREAMS};
use super::scene_passes::default_state;
use super::texture_slots::{alloc_target, assign, barrier_if_slot, copy_target, level_size, set_extent};
use super::vfx::{program_apply, program_block, program_storage_block, program_texture};
use super::{targets, viewport};

/// [[holder]] +0x50c: the graphics settings.
const SETTINGS: usize = 0x462bbf0;
/// [var]: the 16-byte clear colour.
const CLEAR_COLOR: usize = 0x4634ea0;
/// [[var]] +0x38 count, +0x40 programs (0x968 into them: the set main+0xd4cac4 draws with).
const PROGRAM_SETS: usize = 0x4639f00;
/// [[var]]: the temporary target pool.
const TARGET_POOL: usize = 0x4634e70;
/// main+0x7f0354(pool, texture state): gives a temporary target back.
const RELEASE_TARGET: usize = 0x7f0354;
const RENDER_BUFFER_UNBIND: usize = 0xbe19ac;
/// The render buffer's virtual +0x48 (bind): agl::RenderBuffer's and the subclass's.
const RENDER_BUFFER_BIND: usize = 0x7ab980;
const RENDER_BUFFER_BIND_PLAIN: usize = 0x8974e8;
/// Target object virtual +0x10 the texture_slots copy accepts.
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;
/// Names of the temporary targets, and the noise gauge's size floor (0.1).
const OCCLUSION_COLOR_NAME: usize = 0x37a05d2;
const OCCLUSION_DEPTH_NAME: usize = 0x37a05b8;
const NOISE_GAUGE_NAME: usize = 0x37259e4;
const GAUGE_FLOOR: usize = 0x380efb8;
/// The noise gauge's program: [var] is the program object.
const GAUGE_PROGRAM: usize = 0x46b31e8;
/// main+0xa52b64(id, description, block, 0, tail, flag, 0): registers a texture state's slot.
const REGISTER_STATE: usize = 0xa52b64;

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn index(n: u32, count: u32) -> usize {
    (if count > n { n } else { 0 }) as i32 as isize as usize
}

/// Check (before the commit point) that a program is not still being set up.
unsafe fn program_ready(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// Check that copy_target accepts the target object's setup virtual.
unsafe fn setup_known(p: &Port, obj: usize) -> R<()> {
    match p.vfn(obj, 0x10) {
        SETUP_COLOR | SETUP_DEPTH => Ok(()),
        _ => Err("target object setup"),
    }
}

/// bind_clear's stop conditions that can be read in advance: a buffer still bound with an unknown unbind.
unsafe fn bind_free(p: &Port, w: usize) -> R<()> {
    let prev = p.m.r64(w + 0xf8);
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 && prev != 0 && p.vfn(prev, 0x28) != targets::RENDER_BUFFER_UNBIND {
        return Err("render buffer unbind");
    }
    Ok(())
}

/// Clear values of eight colour targets (`color` each) and depth 1.0 (+0x80), stencil 0.
fn clear_values(color: [u32; 4]) -> [u32; 0x22] {
    let mut v = [0u32; 0x22];
    for k in 0..8 {
        v[k * 4..k * 4 + 4].copy_from_slice(&color);
    }
    v[0x20] = 1f32.to_bits();
    v
}

/// main+0xd4cac4(this, w, view), with this +0x18 bit 2 and the view's record (+0x10, 0x21b0 each) +6 bit 1: the record's
/// buffer (+0x1788) unbound and, when its first target has storage, the barrier; the view callback (+0x140, with +0x160
/// bit 2: the game's, past the commit point); the record's buffer +0x1c8 (viewport +0x8e0) bound with no clears; a program of
/// the set at [[0x4639f00]+0x40]+0x968 (variant 3, or 4 with the record's +8 bit 11, from this +0x18 bit 3, else the
/// first), the shader of its table entry (+0x22) applied, the block +0x2120 and textures +0x18f0 / +0x700 / (+0x1d78 or
/// +0xe40) and the full-screen triangle (not with this +0x1d bit 1); unbound; the record's +0x228 put back, +0x17e8
/// cleared and the temporary target +0x18e8 given back by the game (main+0x7f0354).
pub(super) unsafe fn view_gather(p: &mut Port, this: usize, w: usize, view: u32) -> R<()> {
    if (p.m.r8(this + 0x18) >> 2) & 1 == 0 {
        return Ok(());
    }
    let rec = p.m.r64(this + 0x10).wrapping_add(index(view, p.m.r32(this + 8)).wrapping_mul(0x21b0));
    if (p.m.r8(rec + 6) >> 1) & 1 == 0 {
        return Ok(());
    }
    let rb = rec + 0x1788;
    if p.vfn(rb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    // Every program the pass may pick (the callback can change the record's flags), with its shader's: a stop past the
    // commit point would leave the record's +0x228 cleared.
    let holder = p.m.r64(p.m.r64(p.base + PROGRAM_SETS));
    if p.m.r32(holder + 0x38) < 8 {
        return Err("program set missing");
    }
    let list = p.m.r64(p.m.r64(holder + 0x40) + 0x968 + 8);
    let table = p.m.r64(this + 0x138);
    for u in [None, Some(3u32), Some(4)] {
        let prog = match u {
            None => p.m.r64(list + 0x10),
            Some(u) => p.m.r64(list + 0x20) + if p.m.r32(list + 0x18) <= u { 0 } else { u } as usize * 0x158,
        };
        program_ready(p, prog)?;
        let ti = p.m.r16(prog + 0x22) as u32;
        program_ready(p, p.m.r64(table + if p.m.r32(this + 0x130) <= ti { 0 } else { ti } as usize * 0x18 + 8))?;
    }
    targets::unbind(p, w)?;
    let first = p.m.r64(rec + 0x17a8);
    if first == 0 {
        return Err("no colour target");
    }
    barrier_if_slot(p, first + 0x60, w);
    if (p.m.r8(this + 0x160) >> 2) & 1 != 0 {
        match p.delegate_target(this + 0x140) {
            Some(None) => {}
            _ => {
                if p.m.predict() {
                    return Err("view callback");
                }
                p.m.commit();
                // commit
                let args: [u64; 5] = [w as u64, this as u64, view as u64 | 2 << 32, rb as u64, (rec + 0x19e0) as u64];
                p.call(p.m.r64(p.m.r64(this + 0x140)), &[(this + 0x140) as u64, args.as_ptr() as u64]);
            }
        }
    }
    let saved = p.m.r64(rec + 0x228);
    p.m.w::<u64>(rec + 0x228, 0);
    let c = p.pfn(CLEAR_COLOR);
    let lo = [p.m.r32(c), p.m.r32(c + 4), p.m.r32(c + 8), p.m.r32(c + 12)];
    let values = clear_values(lo);
    let vp = rec + 0x8e0;
    viewport::apply(p, vp, w, rec + 0x1c8);
    targets::bind_clear(p, rec + 0x1c8, w, vp, 0, values.as_ptr() as usize)?;
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = 7;
    let flags = p.m.r32(rec + 8);
    let prog = if (p.m.r32(this + 0x18) >> 3) & 1 == 0 {
        p.m.r64(list + 0x10)
    } else {
        let mut u = if flags & 0x800 != 0 { 4 } else { 3 };
        if p.m.r32(list + 0x18) <= u {
            u = 0;
        }
        p.m.r64(list + 0x20) + u as usize * 0x158
    };
    let mut ti = p.m.r16(prog + 0x22) as u32;
    if p.m.r32(this + 0x130) <= ti {
        ti = 0;
    }
    if (p.m.r32(this + 0x18) >> 13) & 1 == 0 {
        g[1] = 1;
        g[10] = 0x0505_0202;
    }
    p.apply_graphics_context(g.as_ptr() as usize, w);
    program_apply(p, p.m.r64(table + ti as usize * 0x18 + 8), w)?;
    program_block(p, prog, w, 0, rec + 0x2120, 0)?;
    program_texture(p, prog, w, 1, rec + 0x18f0)?;
    let rec2 = p.m.r64(this + 0x10).wrapping_add(index(view, p.m.r32(this + 8)).wrapping_mul(0x21b0));
    program_texture(p, prog, w, 0, rec2 + 0x700)?;
    program_texture(p, prog, w, 2, rec2 + if flags & 0x800 == 0 { 0xe40 } else { 0x1d78 })?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);
    if (p.m.r8(this + 0x1d) >> 1) & 1 == 0 {
        stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x488, w);
    }
    targets::unbind(p, w)?;
    // commit
    p.m.commit();
    p.m.w::<u64>(rec + 0x228, saved as u64);
    p.m.w::<u64>(rec + 0x17e8, 0);
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    p.cpu_helper(RELEASE_TARGET, &[pool as u64, p.m.r64(rec + 0x18e8) as u64]);
    Ok(())
}

/// The view record of main+0xbd8c40's family (this +0x5f0, 0x998 each, count +0x5e8).
unsafe fn target_record(p: &Port, this: usize, view: u32) -> usize {
    p.m.r64(this + 0x5f0).wrapping_add(index(view, p.m.r32(this + 0x5e8)).wrapping_mul(0x998))
}

/// main+0xbd94b8(this, w, view, src, layer, k, flags): with flags bit 0 the record's +0x398 (0x48 bytes) cleared; with
/// bit 1 the buffer +0x378 sized by the state `src` (width at least 1, height as the game picks) with its viewport
/// applied; the record's target object k (+0x3e8, 0xf0 each) given the state (texture_slots::copy_target), its layer
/// set to `layer` and level 0 (setup flags 2 / 1 when they change), the object entered in the buffer's target list
/// (+0x398 + 8k) and bit k set in +0x988.
#[allow(clippy::too_many_arguments)]
unsafe fn target_setup(p: &mut Port, this: usize, w: usize, view: u32, src: usize, layer: u32, k: usize, flags: u32) -> R<()> {
    let rec = target_record(p, this, view);
    if flags & 1 != 0 {
        for o in (0..0x48).step_by(8) {
            p.m.w::<u64>(rec + 0x398 + o, 0);
        }
    }
    if flags & 2 != 0 {
        let width = p.m.r16(src + 0x30).max(1);
        let h0 = p.m.r16(src + 0x32);
        let height = (if p.m.r16(src + 0x3a) == 3 { h0 } else { 1 }).max(h0);
        let (fw, fh) = set_extent(p, rec + 0x378, width, height);
        let vp = viewport::full(p, fw, fh);
        viewport::apply(p, vp.as_ptr() as usize, w, rec + 0x378);
    }
    let obj = rec + k * 0xf0 + 0x3e8;
    copy_target(p, obj, src)?;
    if p.m.r16(obj + 0xcc) as u32 != layer {
        p.m.fetch_or32(obj + 0xc8, 2);
        p.m.w::<u16>(obj + 0xcc, layer as u16);
    }
    if p.m.r8(obj + 0xce) != 0 {
        p.m.fetch_or32(obj + 0xc8, 1);
        p.m.w::<u8>(obj + 0xce, 0);
    }
    p.m.w::<u64>(rec + 0x398 + k * 8, obj as u64);
    p.m.w::<u8>(rec + 0x988, p.m.r8(rec + 0x988) | (1 << k));
    Ok(())
}

/// main+0xbe2528(this, w, view): the barriers for the record's pending targets (+0x988 bit 0 with a slot at +0x448; bit 1:
/// main+0xbe1950 on +0x4d8, a barrier when it has a slot), then the flags cleared.
unsafe fn target_barriers(p: &mut Port, this: usize, w: usize, view: u32) {
    let rec = target_record(p, this, view);
    if p.m.r8(rec + 0x988) & 1 != 0 {
        barrier_if_slot(p, rec + 0x448, w);
    }
    if p.m.r8(rec + 0x988) & 2 != 0 {
        barrier_if_slot(p, rec + 0x4d8 + 0x60, w);
    }
    p.m.w::<u8>(rec + 0x988, 0);
}

/// One draw of main+0xbd8c40: the buffer +0x378 bound (main+0xbe25f4; its viewport is not used), the program, the blocks,
/// the full-screen triangle, the unbind and the barriers.
unsafe fn target_draw(p: &mut Port, this: usize, w: usize, view: u32, prog: usize, rec0: usize, textures: &[(u32, usize)]) -> R<()> {
    program_block(p, prog, w, 0, this + 0x9b8, 0)?;
    program_block(p, prog, w, 1, rec0 + 0x2f0, 0)?;
    for &(slot, tex) in textures {
        program_texture(p, prog, w, slot, tex)?;
    }
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);
    stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x488, w);
    targets::unbind(p, w)?;
    target_barriers(p, this, w, view);
    Ok(())
}

/// main+0xbd8c40(this, w, view), with this +0xa58 bits 0-2 set, bit 16 clear and bits 5 / 6 not both clear, and the
/// view's record (+0x5f0) enabled (byte 0): a default render state (alpha test bits cleared), the record's target state
/// +8 set up (k = 0, flags 3), the buffer +0x378 bound, the program of the table entry (+0x2e0, 0x18 each) the flags
/// 20 / 24 select, blocks +0x9b8 / +0x2f0 and texture 4 (this +0x408), the triangle, unbind; with bit 19 two more passes
/// (states +0xf8 with textures 5 = +0x4f8 and 6 = +0x318 through the shader at +0x2e8; +0x1e8 with texture 6 through
/// the shader at +0x300).
pub(super) unsafe fn view_target_passes(p: &mut Port, this: usize, w: usize, view: u32) -> R<()> {
    let flags = p.m.r32(this + 0xa58);
    if !flags & 7 != 0 {
        return Ok(());
    }
    let rec0 = target_record(p, this, view);
    if p.m.r8(rec0) == 0 || (flags >> 16) & 1 != 0 || flags & 0x60 == 0 {
        return Ok(());
    }
    let mut g = default_state(p);
    g[0] &= 0xff00_0000;
    g[1] &= 0xffff_fffe;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    target_setup(p, this, w, view, rec0 + 8, 0, 0, 3)?;
    targets::bind(p, target_record(p, this, view) + 0x378, w)?;
    let set = p.m.r64(p.m.r64(p.m.r64(this + 0x1c8) + if p.m.r32(this + 0x1c0) < 7 { 0 } else { 0x30 }) + 8);
    let n = p.m.r32(set + 0x28);
    let list = p.m.r64(set + 0x30);
    let a = p.m.r16(list + (n > 1) as usize * 0x28 + 0x20) as u32;
    let b = p.m.r16(list + if n < 3 { 0 } else { 0x50 } + 0x20) as u32;
    let mut i = (a & if (flags >> 20) & 1 != 0 { u32::MAX } else { 0 }) + (b & if (flags >> 24) & 1 != 0 { u32::MAX } else { 0 });
    if p.m.r32(this + 0x2d8) <= i {
        i = 0;
    }
    let entry = p.m.r64(this + 0x2e0) + i as usize * 0x18;
    program_apply(p, p.m.r64(entry + 8), w)?;
    target_draw(p, this, w, view, p.m.r64(entry + 8), rec0, &[(4, this + 0x408)])?;
    if (flags >> 19) & 1 != 0 {
        target_setup(p, this, w, view, rec0 + 0xf8, 0, 0, 3)?;
        targets::bind(p, target_record(p, this, view) + 0x378, w)?;
        program_apply(p, p.m.r64(this + 0x2f0), w)?;
        target_draw(p, this, w, view, p.m.r64(this + 0x2f0), rec0, &[(5, this + 0x4f8), (6, this + 0x318)])?;
        target_setup(p, this, w, view, rec0 + 0x1e8, 0, 0, 3)?;
        targets::bind(p, target_record(p, this, view) + 0x378, w)?;
        program_apply(p, p.m.r64(this + 0x308), w)?;
        target_draw(p, this, w, view, p.m.r64(this + 0x308), rec0, &[(6, this + 0x318)])?;
    }
    Ok(())
}

/// main+0xbdad04(this, w, view, flags): the view's record (+0x10, 0x570 each): with this +0x18 bit 0 an
/// "OcclusionColorBuffer" target (format 0x1a) and always an "OcclusionDepthBuffer" (0x5a), each (this +0x30 times the
/// record's +0xf8 / +0x108) allocated by the game and copied into the record's objects (+0x360 / +0x450); the buffer +0x2f0
/// sized by the depth target at +0x350; the compute program [[[this+0x28]+8]+0x20] with blocks +0x1e0 / +0x260 / [+0x2e8]
/// +0x72a8 dispatched once, a barrier, then the buffer bound with clears (colour when the colour target exists, depth 1);
/// with flags bit 0 and this +0x18 bit 2 the program [[[this+0x20]+8]+0x10] with the record's textures (+0x558 count,
/// +0x560 entries of 0x2e8, +0xf0) and the same blocks, the shape at shapes +8 instanced over +0xf8 * +0x108.
pub(super) unsafe fn occlusion_buffers(p: &mut Port, this: usize, w: usize, view: u32, flags: u32) -> R<()> {
    let rec = p.m.r64(this + 0x10).wrapping_add(index(view, p.m.r32(this + 8)).wrapping_mul(0x570));
    let scale = p.m.i32(this + 0x30);
    let (n1, n2) = (scale.wrapping_mul(p.m.i32(rec + 0xf8)), scale.wrapping_mul(p.m.i32(rec + 0x108)));
    let colour = p.m.r8(this + 0x18) & 1 != 0;
    let prog = p.m.r64(p.m.r64(p.m.r64(this + 0x28) + 8) + 0x20);
    let second = flags & 1 != 0 && (p.m.r8(this + 0x18) >> 2) & 1 != 0;
    let prog2 = if second { p.m.r64(p.m.r64(p.m.r64(this + 0x20) + 8) + 0x10) } else { 0 };
    program_ready(p, prog)?;
    if second {
        program_ready(p, prog2)?;
    }
    if colour {
        setup_known(p, rec + 0x360)?;
    }
    setup_known(p, rec + 0x450)?;
    bind_free(p, w)?;
    // commit
    p.m.commit();
    let target = if colour {
        let t = alloc_target(p, w, OCCLUSION_COLOR_NAME, 0x1a, n1 as u32, n2 as u32)?;
        p.m.w::<u64>(rec + 0x540, t as u64);
        copy_target(p, rec + 0x360, t)?;
        rec + 0x360
    } else {
        0
    };
    p.m.w::<u64>(rec + 0x310, target as u64);
    let t = alloc_target(p, w, OCCLUSION_DEPTH_NAME, 0x5a, n1 as u32, n2 as u32)?;
    p.m.w::<u64>(rec + 0x548, t as u64);
    copy_target(p, rec + 0x450, t)?;
    let (lw, lh) = level_size(p, p.m.r64(rec + 0x350), false);
    let (fw, fh) = set_extent(p, rec + 0x2f0, lw, lh);
    program_apply(p, prog, w)?;
    program_block(p, prog, w, 0, rec + 0x1e0, 0)?;
    program_storage_block(p, prog, w, 0, p.m.r64(rec + 0x2e8) + 0x72a8, 0)?;
    program_storage_block(p, prog, w, 1, rec + 0x260, 0)?;
    let cmd = p.m.r64(w + 0xb8);
    p.dispatch(cmd, 1, 1, 1);
    let settings = p.m.r64(p.m.r64(p.base + SETTINGS));
    p.barrier(cmd, if (p.m.r8(settings + 0x50c) >> 1) & 1 == 0 { 0x11 } else { 0x111 });
    let has_colour = p.m.r64(rec + 0x540) != 0;
    let mut g = default_state(p);
    g[0] = (g[0] & 0xffff_0000) | 0x101;
    g[1] &= !1;
    g[7] = if has_colour { 0xf } else { 0 };
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, w, rec + 0x2f0);
    // Colour (0, 0, 0, 1) for every target, depth 1.
    let values = clear_values([0, 0, 0, 1f32.to_bits()]);
    targets::bind_clear(p, rec + 0x2f0, w, vp.as_ptr() as usize, has_colour as u32 | 0x100, values.as_ptr() as usize)?;
    if second {
        program_apply(p, prog2, w)?;
        // main+0x964f10(this, view, w, prog2).
        let r = p.m.r64(this + 0x10).wrapping_add(index(view, p.m.r32(this + 8)).wrapping_mul(0x570));
        let mut i = 0u32;
        while (i as i32) < p.m.i32(r + 0x558) {
            let count = p.m.r32(r + 0x558);
            let e = p.m.r64(r + 0x560) + (if count > i { i } else { 0 }) as usize * 0x2e8;
            program_texture(p, prog2, w, i, e + 0xf0)?;
            i += 1;
        }
        program_block(p, prog2, w, 0, r + 0x1e0, 0)?;
        program_storage_block(p, prog2, w, 0, p.m.r64(r + 0x2e8) + 0x72a8, 0)?;
        program_storage_block(p, prog2, w, 1, r + 0x260, 0)?;
        let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
        shape_state(p, shapes + 8, w);
        let n = p.m.r32(rec + 0x108).wrapping_mul(p.m.r32(rec + 0xf8));
        stream_draw_n(p, p.m.r64(p.m.r64(p.base + STREAMS)) + 0x728, w, n);
    }
    Ok(())
}

/// main+0xdb6438(this, w, scale in s0), when this +0x28 is empty: a "NoiseGauge" target of n = (min(1.5 * scale, 10), at
/// least the floor 0.1) * 64 texels (format 1) allocated by the game, its state's swizzle word (+0x50) set and its slot
/// registered (main+0xa52b64); the state copied to this +0x30 (+0x114 bit 0 set) and to the object +0x190; the buffer
/// +0x120 sized n x n (the viewport at +0x280 too), bound and cleared to zero; the program [0x46b31e8] with block +0x2b0
/// and the line shape (shapes at this +0x530, stream +0x4d0) drawn with line width 2 f and f; a barrier when the object
/// has a slot.
pub(super) unsafe fn noise_gauge(p: &mut Port, this: usize, w: usize, scale: f32) -> R<()> {
    if p.m.r64(this + 0x28) != 0 {
        return Ok(());
    }
    let floor = p.m.f32(p.base + GAUGE_FLOOR);
    let x = scale * 1.5;
    let f = if x < floor {
        floor
    } else if x.is_nan() {
        x
    } else {
        x.min(10.0)
    };
    let n = (f * 64.0) as i32;
    let prog = p.pfn(GAUGE_PROGRAM);
    let rb = this + 0x120;
    program_ready(p, prog)?;
    setup_known(p, this + 0x190)?;
    match p.vfn(rb, 0x48) {
        RENDER_BUFFER_BIND | RENDER_BUFFER_BIND_PLAIN => {}
        _ => return Err("render buffer bind (virtual)"),
    }
    bind_free(p, w)?;
    // commit
    p.m.commit();
    let t = alloc_target(p, w, NOISE_GAUGE_NAME, 1, n as u32, n as u32)?;
    p.m.w::<u64>(this + 0x28, t as u64);
    p.m.w::<u32>(t + 0x50, 0x0201_0101);
    let kind = p.m.r8(t + 0xa0);
    let flag = (p.m.r8(t + 0x54) >> 1) & 1 != 0 && kind > 0x11 && (kind as u32).wrapping_sub(0x2f) > 0x29;
    let desc: [u64; 3] = [p.m.r64(t) as u64, p.m.r64(t + 8) as u64, p.m.r64(t + 0x10) as u64];
    let tail = p.m.r64(t + 0xa8) as u64;
    p.helper("texture state registered by the call", REGISTER_STATE, &[(t + 0x58) as u64, desc.as_ptr() as u64, (t + 0x30) as u64, 0, tail, flag as u64, 0])?;
    assign(p, this + 0x30, t)?;
    p.m.fetch_or32(this + 0x114, 1);
    copy_target(p, this + 0x190, t)?;
    let size = n as f32;
    let lo = (n & (n >> 31)) as f32;
    let hi = (n & !(n >> 31)) as f32;
    p.m.w::<f32>(rb + 8, size);
    p.m.w::<f32>(rb + 0xc, size);
    p.m.w::<f32>(rb + 0x10, lo);
    p.m.w::<f32>(rb + 0x14, lo);
    p.m.w::<f32>(rb + 0x18, hi);
    p.m.w::<f32>(rb + 0x1c, hi);
    if p.m.r32(this + 0x298) <= 5 {
        let (x0, x1) = if n < 1 { (size, 0.0) } else { (0.0, size) };
        p.m.w::<f32>(this + 0x288, x0);
        p.m.w::<f32>(this + 0x28c, x0);
        p.m.w::<f32>(this + 0x290, x1);
        p.m.w::<f32>(this + 0x294, x1);
    }
    match p.vfn(rb, 0x48) {
        RENDER_BUFFER_BIND => targets::bind(p, rb, w)?,
        _ => targets::bind_plain(p, rb, w)?,
    }
    let vp = viewport::full(p, p.m.f32(this + 0x128), p.m.f32(this + 0x12c));
    let zero = [0u32; 4];
    targets::clear(p, rb, w, 0, 1, zero.as_ptr() as usize, 0, vp.as_ptr() as usize, 1.0)?;
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] |= 1;
    g[10] = 0x0202_0202;
    g[11] = (g[11] & 0xffff_0000) | 0x0101;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    program_apply(p, prog, w)?;
    program_block(p, prog, w, 0, this + 0x2b0, 0)?;
    shape_state(p, this + 0x530, w);
    let cmd = p.m.r64(w + 0xb8);
    p.line_width(cmd, f + f);
    stream_draw(p, this + 0x4d0, w);
    p.line_width(cmd, f);
    stream_draw(p, this + 0x4d0, w);
    barrier_if_slot(p, this + 0x1f0, w);
    Ok(())
}
