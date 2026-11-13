// Callees of the deferred view pass and the depth-of-field blur chain that record commands: the shared block / texture
// binds of a view's passes (main+0x972c48), the context's uniform buffer bind (main+0x972fc8), the view's compute pass
// (main+0x972b10), the copy of a storage block into the context's uniform buffer (main+0x9733fc) and the view's
// parameter block write (main+0x9735e0). Each checks its programs before it records anything; the only stop left is a
// program still being set up.

use super::draw::{Port, R};
use super::scene_passes::scene_blocks;
use super::vfx::{program_apply, program_block, program_storage_block, program_texture};

/// main+0x765148(scene, 0): the scene's per-frame block filled by the first user each frame.
const FILL_FRAME_BLOCK: usize = 0x765148;
/// The 16-byte name main+0x9733fc looks up (main+0x370444a).
const BLOCK_KEY: usize = 0x370444a;
/// Bytes at stride 4: how many floats of the view's parameters the block's layout takes (main+0x38414ec).
const FLOAT_COUNTS: usize = 0x38414ec;
/// nvnCommandBufferCopyBufferToBuffer's cell.
const PFN_COPY_BUFFER: usize = 0x466e200;
/// GOT cells of nvnMemoryPoolGetFlags / nvnMemoryPoolFlushMappedRange.
const POOL_GET_FLAGS: usize = 0x4613b40;
const POOL_FLUSH: usize = 0x4614018;
/// The stack functor's vtable (main+0x436e380); its invoke (slot 3) is a bare `ret` (main+0x27ce1a4).
const FUNCTOR_VTABLE: usize = 0x436e380;
const FUNCTOR_INVOKE: usize = 0x27ce1a4;

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

unsafe fn pending(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// main+0x972c48(this, w, view, prog, ctx): the program's shared blocks and textures of a view's pass: the view's record
/// block (this +0x18, 0xb90 each, +0xaf8), the scene's per-frame block (scene = ctx[6] +0x5a0; the first user each frame
/// fills it with main+0x765148), the scene block (+0x18 +0x390), the storage blocks this +0x20 / ctx[9] +0x1c0, the
/// textures 0..0x11 from the view's records (ctx[5] +0x1ec0 record +0x1670 / +0x2118, ctx[11] +0x1f0, ctx[10] +0x560)
/// and the context's own, then main+0x972fc8 with the program's index-3 entry (the entry 0 when it has fewer).
pub(super) unsafe fn ctx_blocks(p: &mut Port, this: usize, w: usize, view: u32, prog: usize, ctx: usize) -> R<()> {
    pending(p, prog)?;
    let (a, scene) = (p.m.r64(ctx + 0x28), p.m.r64(ctx + 0x30));
    let vi = clamp_index(p.m.r32(this + 0x10), view);
    let ri = p.m.r64(this + 0x18).wrapping_add(vi.wrapping_mul(0xb90));
    let sub = p.m.r64(a + 0x1ec0);
    let vj = clamp_index(p.m.r32(sub + 8), view);
    let rj = p.m.r64(sub + 0x10).wrapping_add(vj.wrapping_mul(0x21b0));
    let t = p.m.r64(rj + 0x2118);
    program_block(p, prog, w, 0, ri + 0xaf8, 0)?;
    let frame = p.m.r64(scene + 0x5a0);
    if p.m.r32(frame + 0xc8) == 0 {
        if p.m.predict() {
            return Err("scene frame block filled by the call");
        }
        p.m.fetch_or32(frame + 0xc8, 1);
        p.call(p.base + FILL_FRAME_BLOCK, &[scene as u64, 0]);
    } else {
        p.m.fetch_or32(frame + 0xc8, 1);
    }
    program_block(p, prog, w, 1, frame, 0)?;
    let off = if p.m.r32(scene + 0x10) > 2 { 0x860 } else { 0 };
    let s = p.m.r64(scene + 0x18).wrapping_add(off);
    p.m.w::<u8>(s + 0x421, p.m.r8(s + 0x421) | 1);
    program_block(p, prog, w, 2, s + 0x390, 0)?;
    program_storage_block(p, prog, w, 0, this + 0x20, 0)?;
    program_storage_block(p, prog, w, 1, p.m.r64(ctx + 0x48) + 0x1c0, 0)?;
    program_texture(p, prog, w, 0, rj + 0x1670)?;
    let e = p.m.r64(ctx + 0x58);
    let k = clamp_index(p.m.r32(e + 0x38), view);
    program_texture(p, prog, w, 4, p.m.r64(e + 0x40).wrapping_add(k.wrapping_mul(0x440)) + 0x1f0)?;
    program_texture(p, prog, w, 3, ri + 0x3e8)?;
    program_texture(p, prog, w, 5, ri + 0x5d0)?;
    program_texture(p, prog, w, 7, t + 0x208)?;
    program_texture(p, prog, w, 8, p.m.r64(scene + 0xd0))?;
    program_texture(p, prog, w, 0xb, p.m.r64(ctx))?;
    program_texture(p, prog, w, 0xc, p.m.r64(ctx + 8))?;
    program_texture(p, prog, w, 0xd, p.m.r64(ctx + 0x10))?;
    program_texture(p, prog, w, 0xe, p.m.r64(ctx + 0x18))?;
    program_texture(p, prog, w, 0x11, p.m.r64(ctx + 0x20))?;
    let e2 = p.m.r64(ctx + 0x50);
    let k2 = clamp_index(p.m.r32(e2 + 8), view);
    let t2 = p.m.r64(p.m.r64(e2 + 0x10).wrapping_add(k2.wrapping_mul(0x570)) + 0x560);
    program_texture(p, prog, w, 0x10, t2 + 0x1e8)?;
    let slots = p.m.r64(prog + 0x50) + if p.m.r32(prog + 0x48) > 3 { 0x30 } else { 0 };
    block_bind(p, scene, w, slots)
}

/// main+0x972fc8(ctx, w, slots): the uniform buffer of the context's object (+0x590; its descriptor +0x180: size +0x60,
/// buffers +0x40 by the object's +0x15 & 3, usable with bit 0 of +8) bound at the stages' slots packed in the word at
/// `slots` (bytes for stages 0, 1, 2 and 5; 0xff = none). Nothing without the object, descriptor or size.
pub(super) unsafe fn block_bind(p: &mut Port, ctx: usize, w: usize, slots: usize) -> R<()> {
    let obj = p.m.r64(ctx + 0x590);
    if obj == 0 {
        return Ok(());
    }
    let d = p.m.r64(obj + 0x180);
    if d == 0 {
        return Ok(());
    }
    let size = p.m.r64(d + 0x60);
    if size == 0 {
        return Ok(());
    }
    let bufs = p.m.r64(d + 0x40);
    let which = (p.m.r8(obj + 0x15) & 3) as usize;
    if bufs == 0 || p.m.r16(d + 8) & 1 == 0 {
        return Err("context uniform buffer missing (the game faults)");
    }
    let addr = p.gfx_buffer_address(bufs + which * 0x48);
    let slots = p.m.r32(slots);
    let cmd = p.m.r64(w + 0xb8);
    let size = size as u32 as u64;
    for (stage, sh) in [(0, 0), (1, 8), (2, 16), (5, 24)] {
        if (slots >> sh) & 0xff != 0xff {
            p.bind_uniform_buffer(cmd, stage, (slots >> sh) as i8 as i32, addr, size);
        }
    }
    Ok(())
}

/// main+0x972b10(this, w, view, scene), unless this +0x350 bit 2: the program [[this +0x370]+8]+0x20 applied with the
/// scene blocks of main+0x962f54, then a 1x1x1 compute dispatch.
pub(super) unsafe fn view_pre(p: &mut Port, this: usize, w: usize, view: u32, scene: usize) -> R<()> {
    if (p.m.r8(this + 0x350) >> 2) & 1 != 0 {
        return Ok(());
    }
    let prog = p.m.r64(p.m.r64(p.m.r64(this + 0x370) + 8) + 0x20);
    if p.m.r32(prog + 0x10) != 0 {
        return Err("agl program setup pending");
    }
    pending(p, prog)?;
    program_apply(p, prog, w)?;
    scene_blocks(p, this, w, view, prog, scene)?;
    let cmd = p.m.r64(w + 0xb8);
    p.dispatch(cmd, 1, 1, 1);
    Ok(())
}

/// main+0x9733fc(this, w, _, ctx, flag): the 0x160 bytes at the storage block this +0x1c0 (flag bit 0 clear) or +0x248
/// copied (nvnCommandBufferCopyBufferToBuffer) into the context's uniform buffer at the offset its layout gives the
/// 16-byte name main+0x370444a (a crit-bit lookup of the descriptor's trie, [[[d]+0x10]]+0x28; the offsets at [d]+0x60;
/// without the name the word before them, the first entry's offset is the block size - 0x160).
pub(super) unsafe fn view_post(p: &mut Port, this: usize, w: usize, ctx: usize, flag: u32) -> R<()> {
    let src = this + if flag & 1 == 0 { 0x1c0 } else { 0x248 };
    let obj = p.m.r64(ctx + 0x590);
    if obj == 0 {
        return Ok(());
    }
    let d = p.m.r64(obj + 0x180);
    if d == 0 {
        return Ok(());
    }
    let bufs = p.m.r64(d + 0x40);
    let which = (p.m.r8(obj + 0x15) & 3) as usize;
    if bufs == 0 || p.m.r8(d + 8) & 1 == 0 {
        return Err("context uniform buffer missing (the game faults)");
    }
    let t = p.m.r64(d);
    let offsets = p.m.r64(t + 0x60);
    let key = p.base + BLOCK_KEY;
    let mut found = None;
    let l1 = p.m.r64(t + 0x10);
    let l2 = if l1 != 0 { p.m.r64(l1) } else { 0 };
    let trie = if l2 != 0 { p.m.r64(l2 + 0x28) } else { 0 };
    if trie != 0 {
        let base = trie + 8;
        let mut prev = p.m.i32(base);
        let mut cur = base + p.m.r16(base + 4) as usize * 16;
        let mut v = p.m.i32(cur);
        while prev < v {
            let byte = v >> 3;
            let bit = if byte as u32 > 0xf { 0 } else { (p.m.r8(key + 0xf - byte as usize) >> (v & 7)) & 1 } as usize;
            prev = v;
            cur = base + p.m.r16(cur + 4 + bit * 2) as usize * 16;
            v = p.m.i32(cur);
        }
        let name = p.m.r64(cur + 8);
        if p.m.r16(name) == 16 && (0..16).all(|i| p.m.r8(name + 2 + i) == p.m.r8(key + i)) {
            found = Some(((cur - trie - 0x18) >> 4) as u32);
        }
    }
    let at = match found {
        Some(0) => p.m.r32(d + 0x60).wrapping_sub(0x160),
        Some(i) => p.m.r32(offsets.wrapping_add((i as i32 as isize * 4) as usize)).wrapping_add(4),
        None => p.m.r32(offsets.wrapping_sub(4)).wrapping_add(4),
    };
    if p.m.predict() {
        return Err("block copied by the call");
    }
    let dst = p.gfx_buffer_address(bufs + which * 0x48).wrapping_add(at as i32 as i64 as u64);
    let from = p.m.r64(src + 0x78) as u64 + 0x50;
    let cmd = p.m.r64(w + 0xb8);
    p.call(p.pfn(PFN_COPY_BUFFER), &[cmd as u64, from, dst, 0x160, 0]);
    Ok(())
}

/// main+0x9735e0(this, w, view, size, a4, a5): the view's parameter block (this +0x248, 0x80 bytes each; this +0x38
/// [+0x4160] +0xcc applied as the render state first): {width, height, 1/width, 1/height} of `size` as many floats as
/// the block's layout takes (table main+0x38414ec by its +0x46) written at its data + offset (+0x40), the mapped pool
/// range flushed when the pool is cached, and the stack functor with (w, block, a4, a5, 0, view) run (its invoke is
/// empty; with this +0x49 bit 7 the object [this +0xd8] runs it first through its own virtual).
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn view_last(p: &mut Port, this: usize, w: usize, view: u32, size: usize, a4: usize, a5: usize) -> R<()> {
    let blk = p.m.r64(this + 0x248).wrapping_add(clamp_index(p.m.r32(this + 0x240), view) << 7);
    let info = p.m.r64(p.m.r64(blk + 0x10));
    if info == 0 {
        return Err("view block layout missing (the game faults)");
    }
    let n = p.m.r8(p.base + FLOAT_COUNTS + 4 * p.m.r8(info + 0x46) as usize) as usize;
    if n == 0 || n > 4 {
        return Err("view block float count");
    }
    let hook = (p.m.r8(this + 0x49) >> 7) & 1 != 0;
    if hook && p.m.predict() {
        return Err("view block functor run by the call");
    }
    let g = p.m.r64(p.m.r64(this + 0x38) + 0x4160) + 0xcc;
    p.apply_graphics_context(g, w);
    let (fw, fh) = (p.m.f32(size), p.m.f32(size + 4));
    let f = [fw, fh, 1.0f32 / fw, 1.0f32 / fh];
    let data = p.m.r64(blk + 0x18);
    let dst = data + p.m.r32(info + 0x40) as usize;
    for k in 0..n {
        p.m.w::<u32>(dst + 4 * k, f[k].to_bits());
    }
    let count = p.m.r16(blk + 0x40) as u32;
    let shift = (p.m.r8(blk + 0x42) >> 4) as u32;
    let align_a = p.block_alignment(blk)? as u32;
    let align_b = p.block_alignment(blk)? as u32;
    let pool = p.m.r64(blk + 0x28);
    if pool != 0 {
        let flags = p.query(p.m.r64(p.m.r64(p.base + POOL_GET_FLAGS)), &[pool as u64]) as u32;
        if (flags >> 2) & 1 != 0 {
            let size = p.m.r32(blk + 0x38);
            let stride = size.wrapping_add(align_a).wrapping_sub(1) & align_b.wrapping_neg();
            let off = count.wrapping_mul(shift).wrapping_mul(stride).wrapping_add(p.m.r32(blk + 0x20));
            let start = off as u64 & 0xffff_ffc0;
            let len = ((size as u64 + off as u64 + 0x3f) & 0x3_ffff_ffc0) - start;
            p.call(p.m.r64(p.m.r64(p.base + POOL_FLUSH)), &[pool as u64, start, len]);
        }
    }
    let args: [u64; 6] = [w as u64, blk as u64, a4 as u64, a5 as u64, 0, view as u64];
    let mut functor: [u64; 2] = [(p.base + FUNCTOR_VTABLE) as u64, args.as_ptr() as u64];
    let fp = functor.as_mut_ptr() as usize;
    if hook {
        let obj = p.m.r64(this + 0xd8);
        p.call(p.m.r64(p.m.r64(obj) + 0x20), &[obj as u64, fp as u64]);
    }
    let target = p.m.r64(std::ptr::read_volatile(fp as *const u64) as usize + 0x18);
    if target != p.base + FUNCTOR_INVOKE {
        p.call(target, &[fp as u64]);
    }
    Ok(())
}
