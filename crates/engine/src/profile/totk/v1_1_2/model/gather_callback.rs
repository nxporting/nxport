// The view callback of the scene's render buffer context (delegate at +0x140, called by the view gather main+0xd4cac4 with
// stage 2 and by the pass before it, main+0xd4d088, with stage 1): the delegate main+0xc70028 binds is the member function
// main+0xba85b0 of the object main+0xc6ffac was called with. Stage 2 is a compute pass per entry of the object's table
// (barrier, program, texture, blocks, a 1x1x1 dispatch); stage 1 a full-screen draw with the program set's variant.

use super::draw::{Port, R};
use super::scene_passes::{default_state, fullscreen};
use super::targets;
use super::vfx::{program_apply, program_block, program_storage_block, program_texture};

/// The member function main+0xc70028 binds into the delegate (adjustment 0): the callback ported here.
pub(super) const TARGET: usize = 0xba85b0;

/// `n` when `count > n` (unsigned), else 0, as a signed index.
fn index(n: u32, count: u32) -> usize {
    (if count > n { n } else { 0 }) as i32 as isize as usize
}

/// The `this` the delegate passes (its object plus the adjustment's arithmetic half).
pub(super) unsafe fn delegate_this(p: &Port, delegate: usize) -> usize {
    p.m.r64(delegate + 8).wrapping_add(((p.m.r64(delegate + 0x18) as i64) >> 1) as usize)
}

/// The delegate (a render buffer context's +0x140) called with the argument block the gather builds
/// [w, gather, view | stage << 32, render buffer, record + 0x19e0]: the port when it is the callback main+0xba85b0
/// (Ok(true)), else Ok(false) and nothing done (the caller runs the game's).
pub(super) unsafe fn delegate_call(p: &mut Port, delegate: usize, args: [u64; 5]) -> R<bool> {
    match p.delegate_target(delegate) {
        Some(Some(TARGET)) => {
            let this = delegate_this(p, delegate);
            view_callback(p, this, args)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// main+0xba85b0(this, args): stage (args +0x14) 1: the full-screen pass of main+0xba865c over the object at this +0xa08;
/// stage 2, with the object at this +0x9f8: a barrier and the compute passes of main+0xba8820; other stages nothing.
pub(super) unsafe fn view_callback(p: &mut Port, this: usize, args: [u64; 5]) -> R<()> {
    let (w, gather) = (args[0] as usize, args[1] as usize);
    let view = args[2] as u32;
    let stage = (args[2] >> 32) as u32;
    match stage {
        1 => {
            let obj = p.m.r64(this + 0xa08);
            fullscreen_pass(p, obj, w, view, gather)
        }
        2 => {
            let obj = p.m.r64(this + 0x9f8);
            if obj == 0 {
                return Ok(());
            }
            // INVALIDATE_TEXTURE (and with settings +0x50c bit 1 INVALIDATE_SHADER) | ORDER_FRAGMENTS
            let bits = (targets::barrier_bits(p) & 0x110) | 1;
            let cmd = p.m.r64(w + 0xb8);
            p.barrier(cmd, bits);
            compute_passes(p, obj, w, gather)
        }
        _ => Ok(()),
    }
}

/// main+0xba8820(obj, w, gather): for each of the object's entries (count +8, 0x108 bytes each at +0x10) and the gather's
/// record of the same index (0x21b0 each at +0x10, count +8): the record's render buffer (+0x1788) barriers, the program
/// [[[obj +0x18] +8] +0x10] applied with the record's texture +0x18f0 at slot 0, the entry's block +0x88 and storage block
/// (index 0), and a 1x1x1 compute dispatch.
unsafe fn compute_passes(p: &mut Port, obj: usize, w: usize, gather: usize) -> R<()> {
    let count = p.m.r32(obj + 8) as i32;
    let mut i = 0u32;
    while (i as i32) < count {
        let rec = p.m.r64(gather + 0x10).wrapping_add(index(i, p.m.r32(gather + 8)).wrapping_mul(0x21b0));
        targets::barrier_targets(p, rec + 0x1788, w);
        let entries = p.m.r64(obj + 0x10);
        let entry = entries.wrapping_add(index(i, p.m.r32(obj + 8)).wrapping_mul(0x108));
        let prog = p.m.r64(p.m.r64(p.m.r64(obj + 0x18) + 8) + 0x10);
        program_apply(p, prog, w)?;
        let rec = p.m.r64(gather + 0x10).wrapping_add(index(i, p.m.r32(gather + 8)).wrapping_mul(0x21b0));
        program_texture(p, prog, w, 0, rec + 0x18f0)?;
        program_block(p, prog, w, 0, entry + 0x88, 0)?;
        program_storage_block(p, prog, w, 0, entry, 0)?;
        // the program's finish flag (+0x10) was zero for program_apply: nothing to wait for
        let cmd = p.m.r64(w + 0xb8);
        p.dispatch(cmd, 1, 1, 1);
        i += 1;
    }
    Ok(())
}

/// main+0xba865c(obj, w, view, {0, gather}): unless obj +8 has bits 21 and 22 both set, a full-screen draw with the
/// program of the set [[obj +0xa8] +8] (its first, or variant (count * 4 - 1) of the second list), the shader table's
/// entry applied, the textures of the gather's record (+0xf30 at 0) and obj's record for the view (0xb90 each at +0x18,
/// count +0x10: +0x3e8 at 3, +0x5d0 at 5, +0x7b8 at 6), the blocks +0xaf8 (0), [+0xb78] (2) and [+0xb88] (1), and the
/// render state (blend into target 0, no depth) of the full-screen triangle.
unsafe fn fullscreen_pass(p: &mut Port, obj: usize, w: usize, view: u32, gather: usize) -> R<()> {
    if (!p.m.r32(obj + 8)) & 0x60_0000 == 0 {
        return Ok(());
    }
    let b = p.m.r64(obj + 0x18).wrapping_add(index(view, p.m.r32(obj + 0x10)).wrapping_mul(0xb90));
    let (block2, block1) = (p.m.r64(b + 0xb78), p.m.r64(b + 0xb88));
    let set = p.m.r64(p.m.r64(obj + 0xa8) + 8);
    let n = p.m.r16(p.m.r64(set + 0x30) + 0x20) as u32;
    let prog = if n == 0 {
        p.m.r64(set + 0x10)
    } else {
        let top = (n << 2).wrapping_sub(1);
        let i = if p.m.r32(set + 0x18) > top { top } else { 0 } as usize;
        p.m.r64(set + 0x20) + i * 0x158
    };
    program_apply(p, prog, w)?;
    let rec = p.m.r64(gather + 0x10).wrapping_add(index(view, p.m.r32(gather + 8)).wrapping_mul(0x21b0));
    program_texture(p, prog, w, 0, rec + 0xf30)?;
    program_texture(p, prog, w, 3, b + 0x3e8)?;
    program_texture(p, prog, w, 5, b + 0x5d0)?;
    program_texture(p, prog, w, 6, b + 0x7b8)?;
    program_block(p, prog, w, 0, b + 0xaf8, 0)?;
    program_block(p, prog, w, 2, block2, 0)?;
    program_block(p, prog, w, 1, block1, 0)?;
    let mut g = default_state(p);
    g[0] = (g[0] & 0xffff_0000) | 1;
    g[1] = 1;
    g[7] = 0xf;
    g[10] = 0x0606_0105;
    g[0x19] = (g[0x19] & !0xff) | 6;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    // main+0x979918
    fullscreen(p, w);
    Ok(())
}
