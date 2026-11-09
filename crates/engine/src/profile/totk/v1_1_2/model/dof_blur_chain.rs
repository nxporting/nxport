// The depth-of-field blur chain, main+0x96b618(this, w, view, recs, scene, ctx, s7, s8): one full-screen triangle into a
// colour target copied from the view record, then (this +8 bit 3) a half-resolution blur pair into a temporary
// "Blur(r-half)" target and (bit 4) the four-draw reduce chain through the temporaries "Blur0(Reduce)" / "Blur1(Reduce)"
// back into the record's target. Every pass is drawn into a render buffer the game builds on its stack from texture
// states (texture_slots.rs): the objects are host buffers here, filled as the game's constructors and copies fill them.
// The temporary targets come from the dynamic allocator, which keeps pool state, and the first block bind is the game's
// own main+0x972fc8: the port is applied only (everything past the checks is after the commit point).

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, stream_draw_n, SHAPES, STREAMS};
use super::scene_passes::default_state;
use super::texture_slots::{alloc_target, barrier_if_slot, copy_target, level_size, new_buffer, new_target, release, release_sampler, set_extent, texture_assign, Host};
use super::texture_slots::{BUFFER_BYTES, COLOR_VTABLE, DEPTH_VTABLE, TARGET_BYTES};
use super::vfx::{program_apply, program_block, program_texture};
use super::{targets, viewport};

/// [[holder]]: the temporary target pool main+0x7f0354 gathers the frame's temporary targets with.
const TARGET_POOL: usize = 0x4634e70;
const GATHER: usize = 0x7f0354;
/// main+0x972fc8(ctx, w, slots): the context's uniform buffer (+0x590 object) bound at the program's slots of an index.
const BLOCK_BIND: usize = 0x972fc8;
/// "agl::TextureData", "agl::TextureSampler(from texture)".
const TEXTURE_NAME: usize = 0x37ccf1f;
const SAMPLER_NAME: usize = 0x37a908e;
/// "Blur(r-half)", "Blur0(Reduce)", "Blur1(Reduce)".
const NAME_HALF: usize = 0x376ed1b;
const NAME_REDUCE0: usize = 0x3766d59;
const NAME_REDUCE1: usize = 0x37d2174;

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

fn set_byte(g: &mut [u32; 0x20], o: usize, v: u8) {
    unsafe { *(g.as_mut_ptr() as *mut u8).add(o) = v }
}

/// The stack objects and values every stage shares.
struct Run {
    this: usize,
    w: usize,
    /// The view record (recs +0x18, 0xdd8 each).
    rec: usize,
    ctx: usize,
    scene: usize,
    s7: usize,
    s8: usize,
    /// The program set [[this+0x28]+8] (default program +0x10, variants +0x20, count +0x18).
    set: usize,
    pool: usize,
    /// The render buffer with the colour target `c` (+0x20) and the depth target `d` (+0x60).
    rb: usize,
    c: usize,
    d: usize,
    /// The ring block of (1 / width, 1 / height) the programs read at index 0.
    ring: usize,
    /// The full-screen triangle's stream (streams +0x488).
    stream: usize,
    /// The record's size (the record's region, truncated).
    width: u32,
    height: u32,
    /// The render state, changed from stage to stage.
    g: [u32; 0x20],
    vp: [u32; 10],
}

/// A texture with a sampler made from a texture state: the game's constructor (main+0x979454 or the inline copy) and
/// state assignment; the sampler and slot references are given back by `release`.
struct Sampled(Host);

impl Sampled {
    unsafe fn new(p: &mut Port) -> Sampled {
        let h = Host::new(p, 0x100);
        let x = h.addr();
        p.m.w::<i32>(x + 0x58, -1);
        p.m.w::<u8>(x + 0xa1, 1);
        p.m.w::<u64>(x + 0xa8, (p.base + TEXTURE_NAME) as u64);
        p.m.w::<u64>(x + 0xb0, (p.base + SAMPLER_NAME) as u64);
        p.m.w::<u64>(x + 0xb8, 0);
        p.m.w::<u64>(x + 0xc0, 0x3f80_0000_0000_0000);
        p.m.w::<u64>(x + 0xc8, 0x4170_0000_0000_0000);
        p.m.w::<u32>(x + 0xd0, 0);
        p.m.w::<u64>(x + 0xd4, 0x0101_0707_0702_0101);
        p.m.w::<u8>(x + 0xdc, 0);
        p.m.w::<u16>(x + 0xde, 0);
        p.m.w::<u64>(x + 0xe0, 0xff_0504_0302);
        p.m.w::<u8>(x + 0xe8, 0x20);
        p.m.w::<u16>(x + 0xea, 0xffff);
        Sampled(h)
    }

    fn addr(&self) -> usize {
        self.0.addr()
    }

    unsafe fn release(self, p: &mut Port) {
        release_sampler(p, self.addr() + 0xea);
        release(p, self.addr());
        self.0.release(p);
    }
}

/// The program of the set's variant table at `offset` when the table has at least `count` entries, else its first.
unsafe fn variant(p: &Port, set: usize, count: u32, offset: usize) -> usize {
    p.m.r64(set + 0x20) + if p.m.r32(set + 0x18) >= count { offset } else { 0 }
}

unsafe fn program_ready(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// main+0x96b618(this, w, view, recs, scene, ctx, s7, s8): the chain. With this +8 bits 0 and 1 not both set, or without
/// the view record's flag bit 3, nothing is drawn. The record is recs[view] (+0x18, 0xdd8 each, count +0x10).
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn blur_chain(p: &mut Port, this: usize, w: usize, view: u32, recs: usize, scene: usize, ctx: usize, s7: usize, s8: usize) -> R<()> {
    if (!p.m.r32(this + 8)) & 3 != 0 {
        return Ok(());
    }
    let rec = p.m.r64(recs + 0x18).wrapping_add(clamp_index(view, p.m.r32(recs + 0x10)).wrapping_mul(0xdd8));
    if (p.m.r8(rec) >> 3) & 1 == 0 {
        return Ok(());
    }
    // The checks before the commit point: the programs of every pass (setup pending), the buffer's unbind and the
    // context's.
    let set = p.m.r64(p.m.r64(this + 0x28) + 8);
    let flags = p.m.r32(this + 8);
    program_ready(p, p.m.r64(set + 0x10))?;
    if flags & 8 != 0 {
        program_ready(p, p.m.r64(set + 0x20))?;
        program_ready(p, variant(p, set, 2, 0x158))?;
    }
    if flags & 0x10 != 0 {
        program_ready(p, variant(p, set, 3, 0x2b0))?;
        program_ready(p, variant(p, set, 4, 0x408))?;
        program_ready(p, variant(p, set, 5, 0x560))?;
        program_ready(p, variant(p, set, 6, 0x6b8))?;
    }
    if p.m.r8(p.m.r64(p.base + targets::CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    let prev = p.m.r64(w + 0xf8);
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 && prev != 0 && p.vfn(prev, 0x28) != targets::RENDER_BUFFER_UNBIND {
        return Err("render buffer unbind");
    }

    // The record's region: its size and the reciprocals the programs read.
    let dx = p.m.f32(rec + 0x20) - p.m.f32(rec + 0x18);
    let dy = p.m.f32(rec + 0x24) - p.m.f32(rec + 0x1c);
    let data = [(1.0 / dx).to_bits(), (1.0 / dy).to_bits(), 0, 0];

    // The stack objects: the render buffer, the colour target and the depth target (constructed in that order).
    let rb = Host::new(p, BUFFER_BYTES);
    new_buffer(p, rb.addr());
    let c = Host::new(p, TARGET_BYTES);
    new_target(p, c.addr(), COLOR_VTABLE);
    let d = Host::new(p, TARGET_BYTES);
    new_target(p, d.addr(), DEPTH_VTABLE);
    p.m.w::<u64>(rb.addr() + 0x20, c.addr() as u64);
    p.m.w::<u64>(rb.addr() + 0x60, d.addr() as u64);
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    let mut run = Run {
        this,
        w,
        rec,
        ctx,
        scene,
        s7,
        s8,
        set,
        pool: p.m.r64(p.m.r64(p.base + TARGET_POOL)),
        rb: rb.addr(),
        c: c.addr(),
        d: d.addr(),
        ring: 0,
        stream: streams + 0x488,
        width: dx as u32,
        height: dy as u32,
        g: [0; 0x20],
        vp: [0; 10],
    };
    let r = if p.vfn(run.rb, 0x28) != targets::RENDER_BUFFER_UNBIND { Err("render buffer virtual +0x28") } else { chain(p, &mut run, data) };
    // The destructors: the depth target's slot reference first, then the colour target's.
    release(p, d.addr() + 8);
    release(p, c.addr() + 8);
    d.release(p);
    c.release(p);
    rb.release(p);
    r
}

unsafe fn chain(p: &mut Port, r: &mut Run, data: [u32; 4]) -> R<()> {
    p.m.commit();
    // commit
    r.ring = p.ring_block(data.as_ptr() as usize, 0x10);
    first_pass(p, r)?;
    if p.m.r32(r.this + 8) & 8 != 0 {
        half_blur(p, r)?;
    }
    if p.m.r32(r.this + 8) & 0x10 != 0 {
        reduce(p, r)?;
    }
    Ok(())
}

/// The record's buffer size and region from target `obj`.
unsafe fn extent(p: &mut Port, r: &Run, obj: usize) -> (f32, f32) {
    let (tw, th) = level_size(p, obj, false);
    set_extent(p, r.rb, tw, th)
}

unsafe fn set_viewport(p: &mut Port, r: &mut Run, fw: f32, fh: f32) {
    r.vp = viewport::full(p, fw, fh);
    viewport::apply(p, r.vp.as_ptr() as usize, r.w, r.rb);
}

unsafe fn apply_state(p: &mut Port, r: &Run) {
    p.apply_graphics_context(r.g.as_ptr() as usize, r.w);
}

/// The buffer bound, one draw of the triangle, the unbind and the barrier after the colour target.
unsafe fn draw_bound(p: &mut Port, r: &Run) -> R<()> {
    targets::bind(p, r.rb, r.w)?;
    stream_draw(p, r.stream, r.w);
    targets::unbind(p, r.w)?;
    barrier_if_slot(p, r.c + 0x60, r.w);
    Ok(())
}

/// The first pass: the full-screen triangle's vertex state, the scene's depth state (scene +0x13b8) as the depth target
/// and the record's +0x828 as the colour target, with the default program (set +0x10) reading the ring block, the
/// context's block 0x390 (and the game's block bind of the context), texture 1 = the record's +0x270.
unsafe fn first_pass(p: &mut Port, r: &mut Run) -> R<()> {
    let w = r.w;
    let rec = r.rec;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);
    let t = p.m.r64(r.scene + 0x13b8);
    copy_target(p, r.d, t)?;
    let (fw, fh) = extent(p, r, r.d);
    set_viewport(p, r, fw, fh);
    r.g = default_state(p);
    r.g[0] = (r.g[0] & 0xffff_0000) | 1;
    let p1 = p.m.r64(r.set + 0x10);
    program_apply(p, p1, w)?;
    program_block(p, p1, w, 0, r.ring, 0)?;
    let blocks = p.m.r64(r.ctx + 0x18);
    p.m.w::<u8>(blocks + 0x421, p.m.r8(blocks + 0x421) | 1);
    program_block(p, p1, w, 1, blocks + 0x390, 0)?;
    // main+0x972fc8 with the program's index-2 entry (the entry 0 when it has fewer).
    let slots = p.m.r64(p1 + 0x50) + if p.m.r32(p1 + 0x48) > 2 { 0x20 } else { 0 };
    super::view_callees::block_bind(p, r.ctx, w, slots)?;
    program_texture(p, p1, w, 1, rec + 0x270)?;
    copy_target(p, r.c, rec + 0x828)?;
    set_byte(&mut r.g, 0x64, 4);
    r.g[10] = 0x0101_0909;
    r.g[1] = 1;
    r.g[7] = 4;
    apply_state(p, r);
    draw_bound(p, r)
}

/// main+0x96b618's bit 3 pass: a temporary "Blur(r-half)" target (format 0x1d, the record's size), cleared to ones and
/// drawn with the first variant (the record's +0x828 in slot 0), then drawn again with the second variant (the
/// temporary as texture 2) back into the record's target; the temporary is gathered and its sampler given back.
unsafe fn half_blur(p: &mut Port, r: &mut Run) -> R<()> {
    let s0 = alloc_target(p, r.w, NAME_HALF, 0x1d, r.width, r.height)?;
    let t = Sampled::new(p);
    let res = half_blur_draws(p, r, s0, &t);
    t.release(p);
    res
}

unsafe fn half_blur_draws(p: &mut Port, r: &mut Run, s0: usize, t: &Sampled) -> R<()> {
    let (w, rec) = (r.w, r.rec);
    texture_assign(p, t.addr(), s0)?;
    copy_target(p, r.c, s0)?;
    // Colour 0 cleared to ones, the rest of the values ones as well (depth 1, stencil 0).
    let mut values = [0x3f80_0000_3f80_0000u64; 0x11];
    values[0x10] = 0x3f80_0000;
    targets::bind_clear(p, r.rb, w, r.vp.as_ptr() as usize, 1, values.as_ptr() as usize)?;
    let p2 = p.m.r64(r.set + 0x20);
    program_apply(p, p2, w)?;
    program_block(p, p2, w, 0, r.ring, 0)?;
    program_texture(p, p2, w, 0, rec + 0x828)?;
    r.g[1] = 0;
    r.g[7] = 0xf;
    apply_state(p, r);
    stream_draw_n(p, r.stream, w, 1);
    targets::unbind(p, w)?;
    barrier_if_slot(p, r.c + 0x60, w);
    let p3 = variant(p, r.set, 2, 0x158);
    program_apply(p, p3, w)?;
    program_block(p, p3, w, 0, r.ring, 0)?;
    program_texture(p, p3, w, 2, t.addr())?;
    program_texture(p, p3, w, 0, rec + 0x828)?;
    copy_target(p, r.c, rec + 0x828)?;
    r.g[11] = (r.g[11] & 0xffff_0000) | 0x401;
    r.g[1] = 1;
    r.g[10] = 0x0201_0202;
    r.g[7] = 7;
    apply_state(p, r);
    targets::bind(p, r.rb, w)?;
    stream_draw_n(p, r.stream, w, 1);
    targets::unbind(p, w)?;
    barrier_if_slot(p, r.c + 0x60, w);
    p.helper("depth of field gather", GATHER, &[r.pool as u64, s0 as u64])?;
    Ok(())
}

/// main+0x96b618's bit 4 chain: two half-size temporaries A ("Blur0(Reduce)") and B ("Blur1(Reduce)"), the depth target
/// from s8; the record's +0x828 reduced into A (variant 2), A into B (variant 3), B into A (variant 4) and A back into
/// the record's target with the depth state s7 (variant 5, blending state with this +8 bit 5); both temporaries gathered.
unsafe fn reduce(p: &mut Port, r: &mut Run) -> R<()> {
    let (hw, hh) = (r.width >> 1, r.height >> 1);
    let a = alloc_target(p, r.w, NAME_REDUCE0, 0x1d, hw, hh)?;
    let b = alloc_target(p, r.w, NAME_REDUCE1, 0x1d, hw, hh)?;
    let t = Sampled::new(p);
    let res = reduce_draws(p, r, a, b, &t);
    t.release(p);
    res?;
    p.helper("depth of field gather", GATHER, &[r.pool as u64, a as u64])?;
    p.helper("depth of field gather", GATHER, &[r.pool as u64, b as u64])?;
    Ok(())
}

unsafe fn reduce_draws(p: &mut Port, r: &mut Run, a: usize, b: usize, t: &Sampled) -> R<()> {
    let (w, rec) = (r.w, r.rec);
    copy_target(p, r.d, r.s8)?;
    r.g[0] &= 0xffff_0000;
    r.g[1] = 0;
    r.g[7] = 0xf;
    apply_state(p, r);
    // A is the first draw's target; the texture takes a reference to it too.
    copy_target(p, r.c, a)?;
    let (fw, fh) = extent(p, r, r.c);
    texture_assign(p, t.addr(), a)?;
    set_viewport(p, r, fw, fh);

    let p4 = variant(p, r.set, 3, 0x2b0);
    program_apply(p, p4, w)?;
    program_block(p, p4, w, 0, r.ring, 0)?;
    program_texture(p, p4, w, 2, rec + 0x828)?;
    draw_bound(p, r)?;

    // A sampled into B.
    let p5 = variant(p, r.set, 4, 0x408);
    program_apply(p, p5, w)?;
    program_block(p, p5, w, 0, r.ring, 0)?;
    program_texture(p, p5, w, 2, t.addr())?;
    copy_target(p, r.c, b)?;
    draw_bound(p, r)?;

    // B sampled into A.
    texture_assign(p, t.addr(), b)?;
    let p6 = variant(p, r.set, 5, 0x560);
    program_apply(p, p6, w)?;
    program_block(p, p6, w, 0, r.ring, 0)?;
    program_texture(p, p6, w, 2, t.addr())?;
    copy_target(p, r.c, a)?;
    draw_bound(p, r)?;

    // A sampled back into the record's target with the scene's depth state.
    texture_assign(p, t.addr(), a)?;
    let p7 = variant(p, r.set, 6, 0x6b8);
    program_apply(p, p7, w)?;
    program_block(p, p7, w, 0, r.ring, 0)?;
    program_texture(p, p7, w, 2, t.addr())?;
    copy_target(p, r.d, r.s7)?;
    copy_target(p, r.c, rec + 0x828)?;
    let (fw, fh) = extent(p, r, r.c);
    set_viewport(p, r, fw, fh);
    if (p.m.r8(r.this + 8) >> 5) & 1 != 0 {
        r.g[0] = (r.g[0] & 0xffff_0000) | 1;
        set_byte(&mut r.g, 0x64, 5);
    }
    r.g[7] = 7;
    apply_state(p, r);
    draw_bound(p, r)
}
