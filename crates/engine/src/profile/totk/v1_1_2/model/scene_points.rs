// Two small render units of the scene (main+0xbde1b8, main+0xbde7f0) and the material bind's remaining paths
// (main+0x7b42f0: the model's own blocks, the unit's extra shapes).

use super::cubemap::set_uniform;
use super::draw::{Port, R};
use super::scene_passes::{default_state, fullscreen};
use super::targets;
use super::texture_slots::{copy_target, release, release_sampler, texture_assign, Host};
use super::vfx::{program_apply, program_block, program_storage_block, program_texture, shadow_mask_setup};
use super::viewport;

const LOCK: usize = 0x2b15370;
const UNLOCK: usize = 0x2b15380;
/// agl::TextureData's constructor (0xf0 bytes).
const TEXTURE_CTOR: usize = 0xbf2180;
/// Pointer to the game's nvnCommandBufferDrawArrays pointer.
const GOT_DRAW_ARRAYS: usize = 0x4613b80;
/// The render target's view setup virtuals (+0x18: one level / every level).
const VIEW_SETUP: usize = 0x2a2f550;
const VIEW_SETUP_ALL_LEVELS: usize = 0x2a2f89c;
const PFN_BUFFER_GET_ADDRESS: usize = 0x466d818;
/// main+0xc7a7cc(texture, 0, size): flushes the texture's CPU-written memory.
const TEXTURE_FLUSH: usize = 0xc7a7cc;

/// A constructed agl::TextureData on a host object (main+0xbf2180).
unsafe fn texture_new(p: &mut Port) -> Host {
    let o = Host::new(p, 0x100);
    p.query(p.base + TEXTURE_CTOR, &[o.addr() as u64]);
    o
}

/// main+0xbde1b8(this, w, ctx, view): the view's record (+0x10, 0x150 each; view 0 when out of range) with its entries
/// (count +0x130, list +0x138) each drawn as one point: the program chosen by the textures the context offers (ctx[0]
/// the record's own target when +0x14c bit 0, ctx[1] / ctx[2] shadow mask sources with bits 1 / 2) bound at slots 0 /
/// 1 / 2 from a texture object made on the stack, a 0x20-byte ring block (the entry's values and the context's
/// reciprocal size), the record's storage block (+0x80, element from the entry), under the object's mutex (+0x28).
/// Applying only: the stack texture is finished by the game's own call.
pub(super) unsafe fn mask_points(p: &mut Port, this: usize, w: usize, ctx: usize, view: u32) -> R<()> {
    let idx = if p.m.r32(this + 8) > view { view } else { 0 } as i32 as isize as usize;
    let rec = p.m.r64(this + 0x10).wrapping_add(idx.wrapping_mul(0x150));
    if p.m.i32(rec + 0x130) < 1 {
        return Ok(());
    }
    let flags = p.m.r8(rec + 0x14c);
    let c0 = p.m.r64(ctx);
    let own = c0 != 0 && flags & 1 != 0;
    let second = p.m.r64(ctx + 8) != 0 && (flags >> 1) & 1 != 0;
    let third = p.m.r64(ctx + 0x10) != 0 && (flags >> 2) & 1 != 0;
    let mut k = 0u32;
    if own {
        k = if p.m.r16(c0 + 0x3a) == 2 { 8 } else { 4 };
    }
    if second {
        k |= 2;
    }
    if third {
        k |= 1;
    }
    let progs = p.m.r64(p.m.r64(this + 0x18) + 8);
    let prog = if k == 0 {
        p.m.r64(progs + 0x10)
    } else {
        let i = if p.m.r32(progs + 0x18) > k - 1 { k - 1 } else { 0 };
        p.m.r64(progs + 0x20) + i as usize * 0x158
    };
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    if p.m.predict() && (own || second || third) {
        return Err("mask texture finished by the call");
    }
    p.m.commit();
    if !p.m.predict() {
        p.query(p.base + LOCK, &[(this + 0x28) as u64]);
    }
    let t = texture_new(p);
    let r = mask_points_in(p, t.addr(), w, ctx, rec, prog, [own, second, third]);
    t.release(p);
    if !p.m.predict() {
        p.query(p.base + UNLOCK, &[(this + 0x28) as u64]);
    }
    r
}

unsafe fn mask_points_in(p: &mut Port, t: usize, w: usize, ctx: usize, rec: usize, prog: usize, bound: [bool; 3]) -> R<()> {
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = 0;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    program_apply(p, prog, w)?;
    // The stack texture: its flags, then the context's own texture copied in.
    let f = p.m.r8(ctx + 0x20);
    p.m.w::<u8>(t + 0xd4, f);
    p.m.w::<u8>(t + 0xd5, f);
    p.m.w::<u8>(t + 0xd6, 1);
    p.m.fetch_or32(t + 0xe4, 4);
    if p.m.r8(ctx + 0x21) != 0 {
        p.m.w::<u8>(t + 0xd7, 1);
        p.m.w::<u8>(t + 0xd8, 1);
        p.m.w::<u8>(t + 0xd9, 1);
        p.m.fetch_or32(t + 0xe4, 2);
    }
    if bound[0] {
        texture_assign(p, t, p.m.r64(ctx))?;
        program_texture(p, prog, w, 0, t)?;
    }
    if bound[1] {
        shadow_mask_setup(p, t, p.m.r64(ctx + 8))?;
        program_texture(p, prog, w, 1, t)?;
    }
    if bound[2] {
        shadow_mask_setup(p, t, p.m.r64(ctx + 0x10))?;
        program_texture(p, prog, w, 2, t)?;
    }
    let sx = 1.0 / p.m.i32(ctx + 0x18) as f32;
    let sy = 1.0 / p.m.i32(ctx + 0x1c) as f32;
    let list = p.m.r64(rec + 0x138);
    let draw = p.m.r64(p.m.r64(p.base + GOT_DRAW_ARRAYS));
    for i in 0..p.m.i32(rec + 0x130).max(0) as usize {
        let e = p.m.r64(list + i * 8);
        // The entry's 16 bytes at +0xc, the reciprocal size; the last 8 bytes of the 0x20 are the game's stack.
        let mut d = [0u32; 8];
        d[0] = p.m.r32(e + 0xc);
        d[1] = p.m.r32(e + 0x10);
        d[2] = p.m.r32(e + 0x14);
        d[4] = sx.to_bits();
        d[5] = sy.to_bits();
        let block = p.ring_block_known(d.as_ptr() as usize, 0x18, 0x20);
        program_block(p, prog, w, 0, block, 0)?;
        let element = p.m.r8(e + p.m.r8(rec + 0x14d) as usize + 0x35) as i32;
        program_storage_block(p, prog, w, 0, rec + 0x80, element)?;
        let cmd = p.m.r64(w + 0xb8);
        p.draw_arrays(draw, cmd, 0, 0, 1, 0, 0);
    }
    release_sampler(p, t + 0xea);
    release(p, t);
    Ok(())
}

/// main+0xbde7f0(this, w): the texture state at +0x100 copied into the render target object at +0x568 (render buffer
/// +0x4f8, square of side +0x2e0), a full-screen triangle drawn into it with the program at [[[[+8]]+8]+0x10] sampling
/// the texture at +0x10 (uniform 0 = (1, 1, 0, 0), uniform 1 = 0), the buffer unbound and, with +0x86d bit 0, the
/// texture's CPU-written memory flushed (main+0xc7a7cc).
pub(super) unsafe fn target_blit(p: &mut Port, this: usize, w: usize) -> R<()> {
    let prog = p.m.r64(p.m.r64(p.m.r64(p.m.r64(this + 8)) + 8) + 0x10);
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    let obj = this + 0x568;
    if !matches!(p.vfn(obj, 0x18), VIEW_SETUP | VIEW_SETUP_ALL_LEVELS) {
        return Err("render target setup");
    }
    if p.m.r8(p.m.r64(p.base + targets::CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    // A buffer still bound is unbound by the bind (its virtual +0x28; one saved buffer at +0x100 is not handled).
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 {
        let prev = p.m.r64(w + 0xf8);
        if prev != 0 && (p.vfn(prev, 0x28) != targets::RENDER_BUFFER_UNBIND || p.m.r64(w + 0x100) != 0) {
            return Err("render buffer still bound");
        }
    }
    if p.m.predict() && p.m.r8(this + 0x86d) & 1 != 0 {
        return Err("texture memory flushed by the call");
    }
    let had = p.m.i32(this + 0x5c8) != -1;
    let size = p.m.r32(this + 0x2e0) as f32;
    // The texture state copied into the target object (slot references moved), the buffer's size and its target.
    copy_target(p, obj, this + 0x100)?;
    p.m.w::<u64>(this + 0x508, 0);
    for o in [0x510usize, 0x514, 0x500, 0x504] {
        p.m.w::<f32>(this + o, size);
    }
    for o in (0x520..=0x558).step_by(8) {
        p.m.w::<u64>(this + o, 0);
    }
    p.m.w::<u64>(this + 0x518, obj as u64);
    let vp = viewport::full(p, size, size);
    p.m.commit();

    let cmd = p.m.r64(w + 0xb8);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    if had {
        let bits = targets::barrier_bits(p);
        p.barrier(cmd, bits);
    }
    let rb = this + 0x4f8;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    targets::bind(p, rb, w)?;
    program_apply(p, prog, w)?;
    program_texture(p, prog, w, 0, this + 0x10)?;
    let ones: Vec<u8> = [1.0f32, 1.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    set_uniform(p, prog, w, 0, &ones);
    set_uniform(p, prog, w, 1, &0u32.to_le_bytes());
    fullscreen(p, w);
    targets::unbind(p, w)?;
    if p.m.i32(this + 0x5c8) != -1 {
        let bits = targets::barrier_bits(p);
        p.barrier(cmd, bits);
    }
    if p.m.r8(this + 0x86d) & 1 != 0 {
        p.helper("texture memory flushed by the call", TEXTURE_FLUSH, &[(this + 0x100) as u64, 0, p.m.r64(this + 0x140) as u64])?;
        p.m.w::<u32>(this + 0x86c, p.m.r32(this + 0x86c) & 0xffff_feff);
    }
    Ok(())
}

/// A uniform buffer at the stage slots packed in `slots` (stages 0, 1, 2 and 5).
unsafe fn bind_slots(p: &mut Port, cmd: usize, slots: u32, addr: u64, size: u64) {
    for (stage, sh) in [(0, 0), (1, 8), (2, 16), (5, 24)] {
        let s = (slots >> sh) & 0xff;
        if s != 0xff {
            p.bind_uniform_buffer(cmd, stage, s as i8 as i32, addr, size);
        }
    }
}

/// main+0x221f550(o, w, &addr, slots, 0, size) after the address of element `b` of block object `o` is worked out
/// (main+0x766ef4's body per block): `b` -1 selects the object's own current element (+0x42 high nibble).
unsafe fn bind_object(p: &mut Port, o: usize, cmd: usize, slots: u32, b: i32) -> R<()> {
    let n = (p.m.r8(o + 0x42) & 0xf) as i32;
    // SDIV by zero gives 0 (the remainder is then b).
    let sel = if b != -1 { if n == 0 { b } else { b.wrapping_rem(n) } } else { (p.m.r8(o + 0x42) >> 4) as i32 };
    let count = p.m.r16(o + 0x40) as i64;
    let size = p.m.r32(o + 0x38);
    let al = p.block_alignment(o)?;
    let al2 = p.block_alignment(o)?;
    let stride = ((size as i32).wrapping_add(al).wrapping_sub(1) & al2.wrapping_neg()) as u32 as u64;
    let addr = (p.m.r64(o + 0x78) as u64).wrapping_add(stride.wrapping_mul((sel as i64 * count) as u64));
    for (stage, sh) in [(0, 0), (1, 8), (2, 16), (5, 24)] {
        let s = (slots >> sh) & 0xff;
        if s != 0xff {
            p.bind_block(o, cmd, stage, s as i8 as i32, addr, size)?;
        }
    }
    Ok(())
}

/// main+0x766ef4(model + 0x70, w, mat, b): the model's four block objects (+0x78, +0x80, +0x88, +0x90) at the material
/// record's slots (+0x2c, +0x30, +0x34, +0x38), element `b`. Replaces the stop in material::bind for model +0xd8 bits
/// 0..3 (`mat` = [entry + 8]).
pub(super) unsafe fn material_blocks(p: &mut Port, model: usize, w: usize, mat: usize, b: i32) -> R<()> {
    let rec = p.m.r64(mat);
    for (k, so) in [0x2cusize, 0x30, 0x34, 0x38].into_iter().enumerate() {
        let slots = p.m.r32(rec + so);
        if slots == u32::MAX {
            continue;
        }
        let o = p.m.r64(model + 0x78 + k * 8);
        if o == 0 {
            continue;
        }
        let cmd = p.m.r64(w + 0xb8);
        bind_object(p, o, cmd, slots, b)?;
    }
    Ok(())
}

/// The unit's extra shape list ([d+0x68]+0xb00, linked through +8, each node's item +0x50 the model's block table
/// index): per node the unit mesh's shape ([[d+0x68]+8]+0x18) drawn whole (main+0x758644), then the table entry's buffer
/// (element b, or +0xf * a + b with +0xd) bound at the material's slots (+0x28), 0x100 bytes (main+0x7b576c).
/// Replaces `extra_shapes` in material.rs (`mat` = [entry + 8]).
pub(super) unsafe fn extra_shapes(p: &mut Port, d: usize, w: usize, mat: usize, a: i32, b: i32) -> R<()> {
    let mut node = p.m.r64(p.m.r64(d + 0x68) + 0xb00);
    while node != 0 {
        let sh = p.m.r64(p.m.r64(p.m.r64(d + 0x68) + 8) + 0x18);
        let idx = p.m.r16(p.m.r64(node) + 0x50) as usize;
        let table = p.m.r64(p.m.r64(d + 8) + 0x178);
        p.draw_shape(sh, w + 8, 0, p.m.r16(sh + 0x34) as u32, 1, 0);
        let t = table + idx * 0x70;
        let k = if p.m.r8(t + 0xd) != 0 {
            b.wrapping_add((p.m.r8(t + 0xf) as i32).wrapping_mul(a)) as i64
        } else {
            b as i64
        };
        let bufs = p.m.r64(t + 0x20);
        let e = if bufs != 0 { bufs.wrapping_add((k as usize).wrapping_mul(0x48)) } else { 0 };
        let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[p.m.r64(e + 8) as u64]);
        let cmd = p.m.r64(w + 0xb8);
        bind_slots(p, cmd, p.m.r32(p.m.r64(mat) + 0x28), addr, 0x100);
        node = p.m.r64(node + 8);
    }
    Ok(())
}
