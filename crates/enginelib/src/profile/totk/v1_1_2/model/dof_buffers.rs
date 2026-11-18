// agl::pfx::DepthOfField's buffer pass, main+0xc4b934(this, w, ctx): the view's blur pyramid ("dof_blur_mipmap"), weight
// ("dof_blur_mipmap_weight") and depth ("dof_depth_mipmap") textures are taken from the temporary target pool and copied
// into the view's buffer record (this +0x5c0, 0x718 each), the depth pyramid pass (main+0xc594f8), the per-level blur
// passes into a render buffer built from the pyramid (record +0x1e0), then the full-screen pass of the effect into the
// view's render buffer (ctx +0x18) or the mode 2 mip copy (main+0xc59394), the temporaries given back and, for the
// settings with +0x78, the final pass (main+0x21f10e4).
// `ctx`: +0 view index, +4 pass mode (0 weight level, 1 level, 2 depth pyramid, 3 final blur, 4 final pass), +8 the view's
// buffer record, +0x10 the settings record, +0x18 the render buffer, +0x20 / +0x24 near / far, +0x28 / +0x2c size, +0x30
// reversed depth range, +0x31 reversed depth.
// The temporary targets come from the pool, which keeps state: the port is applied only (everything past the checks is
// after the commit point); predicting stops at the first allocation (every call with the effect on) and checks the final
// pass alone. Everything that can stop (programs still being set up, the render buffers' unbind, the target objects'
// setup virtual) is checked before the commit.

use super::cubemap::set_uniform;
use super::callees_misc::{mip_quad, mip_quad_check};
use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw_n};
use super::scene_passes::{default_state, fullscreen};
use super::targets::{self, CONTEXT_TYPE_GUARD, RENDER_BUFFER_UNBIND};
use super::texture_slots::{assign, barrier_if_slot, copy_target, new_target, release, texture_assign, Host, COLOR_VTABLE, TARGET_BYTES};
use super::viewport;
use super::vfx::{program_apply, program_texture, shadow_mask_setup};

/// [[holder]]: the temporary target pool; main+0x7f0ad4 allocates from it (12 arguments, four on the stack) and
/// main+0x7f0354(pool, target) gives a target back.
const TARGET_POOL: usize = 0x4634e70;
const ALLOC: usize = 0x7f0ad4;
const GATHER: usize = 0x7f0354;
/// [[holder]]: the program sets ([+0x10] the array).
const PROGRAM_HOLDER: usize = 0x4634e40;
/// libm stubs: the blur direction's two angle functions in the order the game calls them, and the level's rounding.
const LIBM_A: usize = 0x2b15760;
const LIBM_B: usize = 0x2b15770;
const LIBM_CEIL: usize = 0x2b15dc0;
/// main+0xdf51fc(w, texture, viewport, scale (two floats), offset, level in s0): the mode 2 mip copy.
/// [var]: the offset the mip copy takes (the quad offset of the frame).
const QUAD_OFFSET: usize = 0x462cee0;
/// Floats and the 16 constant bytes the passes read from the game's data.
const CONST_VECTOR: usize = 0x38417c8;
const MIN_CLAMP: usize = 0x380fc34;
const SCALE_K: usize = 0x380ec60;
const TENTH: usize = 0x380efb8;
const DEG_TO_RAD: usize = 0x380f068;
/// "dof_blur_mipmap", "dof_blur_mipmap_weight", "dof_depth_mipmap".
const NAME_BLUR: usize = 0x37d59de;
const NAME_WEIGHT: usize = 0x377a576;
const NAME_DEPTH: usize = 0x36de317;
/// Target object virtual +0x10 (colour / depth setup): both only write the object.
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;

fn fmax(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() { f32::NAN } else { a.max(b) }
}

fn fmin(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() { f32::NAN } else { a.min(b) }
}

fn set_byte(g: &mut [u32; 0x20], o: usize, v: u8) {
    unsafe { *(g.as_mut_ptr() as *mut u8).add(o) = v }
}

#[cfg(nx_native)]
unsafe fn libm(p: &Port, f: usize, x: f32) -> f32 {
    type F = unsafe extern "C" fn(f32) -> f32;
    std::mem::transmute::<usize, F>(p.base + f)(x)
}

#[cfg(not(nx_native))]
unsafe fn libm(_p: &Port, _f: usize, _x: f32) -> f32 {
    unimplemented!("guest calls with a float argument and result on a lifted host")
}

unsafe fn gf(p: &Port, a: usize) -> f32 {
    p.m.f32(p.base + a)
}

/// A side-effecting game function with a float argument (nothing while predicting or applying undoably).
unsafe fn call_f(p: &mut Port, name: &'static str, addr: usize, ints: &[u64], floats: &[f32]) -> R<u64> {
    if p.m.stops() {
        return Err(name);
    }
    p.flush();
    Ok(p.query_f(p.base + addr, ints, floats))
}

/// main+0xc58a64(rec): the blur is enabled (+0x118, pointer +0x548, +0x38) and its level (+0x238) is whole (or the
/// fractional flag +0x198 is set).
unsafe fn ready(p: &Port, r: usize) -> bool {
    if p.m.r8(r + 0x118) == 0 || p.m.r64(r + 0x548) == 0 || p.m.r8(r + 0x38) == 0 {
        return false;
    }
    let f = p.m.f32(r + 0x238);
    let t = f as i32;
    let negative_fraction = f < 0.0 && f != t as f32;
    t == negative_fraction as i32 || p.m.r8(r + 0x198) != 0
}

/// main+0xc57d28 / main+0xc58acc(rec) (the same code): the weight mode is on (+0x580) and the blur is not ready.
unsafe fn weight_mode(p: &Port, r: usize) -> bool {
    if p.m.r8(r + 0x580) == 0 {
        return false;
    }
    if p.m.r8(r + 0x118) == 0 || p.m.r64(r + 0x548) == 0 || p.m.r8(r + 0x38) == 0 {
        return true;
    }
    let f = p.m.f32(r + 0x238);
    let t = f as i32;
    let negative_fraction = f < 0.0 && f != t as f32;
    if t == negative_fraction as i32 {
        return false;
    }
    p.m.r8(r + 0x198) == 0
}

/// main+0x21f1290(rec): some blur is active.
unsafe fn blur_active(p: &Port, r: usize) -> bool {
    if p.m.r8(r + 0x18) == 0 && p.m.r8(r + 0x38) == 0 {
        return false;
    }
    if p.m.f32(r + 0x238) > 0.0 {
        return true;
    }
    if p.m.r8(r + 0x118) == 0 || p.m.r64(r + 0x548) == 0 || p.m.r8(r + 0x38) == 0 {
        return false;
    }
    let f = p.m.f32(r + 0x238);
    let t = f as i32;
    let negative_fraction = f < 0.0 && f != t as f32;
    if t == negative_fraction as i32 {
        return true;
    }
    p.m.r8(r + 0x198) != 0
}

/// Whether the depth pyramid pass runs for the settings record (+0x18, +0x58 and a level above 0).
unsafe fn depth_pass(p: &Port, r: usize) -> bool {
    p.m.r8(r + 0x18) != 0 && p.m.r8(r + 0x58) != 0 && p.m.f32(r + 0x238) > 0.0
}

/// Whether the final pass (main+0x21f10e4) runs after the body (the settings record's +0x78 and the conditions).
unsafe fn final_runs(p: &Port, r: usize) -> bool {
    p.m.r8(r + 0x78) != 0 && (p.m.r8(r + 0x98) == 0 || p.m.r8(r + 0xb8) != 0 || p.m.r32(r + 0x380) != 0 || p.m.f32(r + 0x238) <= 0.0)
}

/// A program's setup finished (+0x10, +0x14), so no pass below stops at it.
unsafe fn program_ready(p: &Port, prog: usize) -> R<()> {
    if prog == 0 {
        return Err("depth of field program missing");
    }
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// A render buffer's virtual +0x28 is the wrapper's unbind (`targets::unbind`).
unsafe fn unbind_ok(p: &Port, rb: usize) -> R<()> {
    if rb == 0 || p.m.r64(rb) == 0 || p.vfn(rb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    Ok(())
}

/// A target object's virtual +0x10 is one of the two setups that only write the object.
unsafe fn setup_ok(p: &Port, obj: usize) -> R<()> {
    if p.m.r64(obj) == 0 {
        return Err("target object setup");
    }
    let f = p.vfn(obj, 0x10);
    if f != SETUP_COLOR && f != SETUP_DEPTH {
        return Err("target object setup");
    }
    Ok(())
}

/// The context type is known and a render buffer still bound unbinds with the known virtual.
unsafe fn context_ok(p: &Port, w: usize) -> R<()> {
    if p.m.r8(p.m.r64(p.base + CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    let prev = p.m.r64(w + 0xf8);
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 && prev != 0 && p.vfn(prev, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer unbind");
    }
    Ok(())
}

/// The program set holder's set [[[[h]+0x10]+off]+8].
unsafe fn program_set(p: &Port, off: usize) -> usize {
    p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER)) + 0x10) + off) + 8)
}

/// The depth pyramid pass's first program: the set at +0x230, entry 0x20 (reversed range) or 0x10.
unsafe fn depth_program(p: &Port, u: bool) -> usize {
    p.m.r64(program_set(p, 0x230) + if u { 0x20 } else { 0x10 })
}

/// main+0xc583a8's program: the set at +0x28, entry 0x20.
unsafe fn tile_program(p: &Port) -> usize {
    p.m.r64(program_set(p, 0x28) + 0x20)
}

/// The program of the final blur pass by the effect's mode (this +0x8e0) and switches, entry `u` of the settings' table.
unsafe fn blur_program(p: &Port, this: usize, s: usize, u: usize) -> usize {
    let at = if p.m.r32(this + 0x8e0) == 1 {
        s + u * 8 + 0x4f8
    } else if p.m.r8(this + 0x148) == 0 || p.m.r64(this + 0x578) == 0 || p.m.r8(this + 0x68) == 0 {
        s + u * 8 + 0x508
    } else {
        s + u * 8 + 0x518
    };
    p.m.r64(at)
}

/// The settings' program of the weight level pass: the table entry 8 (or 0x10 with the reversed range).
unsafe fn weight_program(p: &Port, s: usize, u: bool) -> usize {
    p.m.r64(s + if u { 0x10 } else { 8 } + 0x4d8)
}

/// The program's uniform `k` set to four floats (the game's inlined update: the program must not be mid-setup).
unsafe fn uniform_f(p: &mut Port, prog: usize, w: usize, k: usize, v: [f32; 4]) -> R<()> {
    let mut b = [0u8; 16];
    for (i, x) in v.iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    uniform_bytes(p, prog, w, k, b)
}

unsafe fn uniform_bytes(p: &mut Port, prog: usize, w: usize, k: usize, b: [u8; 16]) -> R<()> {
    if p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    set_uniform(p, prog, w, k, &b);
    Ok(())
}

/// The program's uniform `k` set to 16 bytes at `at`.
unsafe fn uniform_mem(p: &mut Port, prog: usize, w: usize, k: usize, at: usize) -> R<()> {
    let b: [u8; 16] = std::array::from_fn(|i| p.m.r8(at + i));
    uniform_bytes(p, prog, w, k, b)
}

/// The constant vector at main+0x38417c8 as uniform `k`.
unsafe fn uniform_const(p: &mut Port, prog: usize, w: usize, k: usize) -> R<()> {
    uniform_mem(p, prog, w, k, p.base + CONST_VECTOR)
}

/// The texture's size as the game's passes read it: width (at least 1), height (the state's own, at least the 2D
/// class's) from the head at `t` (+0x30 width, +0x32 height, +0x3a class).
unsafe fn tex_size(p: &Port, t: usize) -> (u32, u32) {
    let w = (p.m.r16(t + 0x30) as u32).max(1);
    let h = p.m.r16(t + 0x32) as u32;
    let hk = if p.m.r16(t + 0x3a) == 3 { h } else { 1 };
    (w, hk.max(h))
}

/// main+0xc4f158(rb, level): the render buffer's size and region from its target (the depth target when it has one) at
/// the mip `level`; the targets' level (+0xce) is set (rebuilding their view) when it differs.
unsafe fn level_extent(p: &mut Port, rb: usize, level: u32) {
    let set_level = |p: &mut Port, t: usize| {
        if p.m.r8(t + 0xce) as u32 != level {
            p.m.fetch_or32(t + 0xc8, 1);
            p.m.w::<u8>(t + 0xce, level as u8);
        }
    };
    let colour = p.m.r64(rb + 0x20);
    if colour != 0 {
        set_level(p, colour);
    }
    let depth = p.m.r64(rb + 0x60);
    let obj = if depth != 0 {
        set_level(p, depth);
        depth
    } else {
        colour
    };
    if obj == 0 {
        for (o, v) in [(0x10usize, -1.0f32), (0x14, -1.0), (0x18, 0.0), (0x1c, 0.0), (8, -1.0), (0xc, -1.0)] {
            p.m.w::<f32>(rb + o, v);
        }
        return;
    }
    let sh = level & 31;
    let width = ((p.m.r16(obj + 0x38) as u32 >> sh) as i32).max(1);
    let h0 = p.m.r16(obj + 0x3a) as u32;
    let hk = if p.m.r16(obj + 0x42) == 3 { h0 } else { 1 } as i32;
    let hs = (h0 >> sh) as i32;
    let height = if hk > hs { hk } else { hs };
    let (fw, fh) = (width as f32, height as f32);
    p.m.w::<f32>(rb + 0x10, 0.0);
    p.m.w::<f32>(rb + 0x14, if height > 0 { 0.0 } else { fh });
    p.m.w::<f32>(rb + 0x18, fw);
    p.m.w::<f32>(rb + 0x1c, if height > 0 { fh } else { 0.0 });
    p.m.w::<f32>(rb + 8, fw);
    p.m.w::<f32>(rb + 0xc, fh);
}

/// main+0xc4f284(this, w, ctx): the full-screen draw of the pass by the pass mode (ctx +4): the pass's own shape
/// (this +0x8f8, 0x3e0 each, entry by the settings' +0x3c8 or, in mode 4 with the final conditions, +0x448) in the
/// modes 0 / 3 with the settings' +0x98 and in mode 4, else the full-screen triangle.
unsafe fn draw_pass(p: &mut Port, this: usize, w: usize, ctx: usize) {
    let mode = p.m.r32(ctx + 4);
    let s = p.m.r64(ctx + 0x10);
    let own = |p: &Port, at: usize| {
        let i = p.m.r32(at);
        this + 0x8f8 + (if i < 2 { i } else { 0 }) as usize * 0x3e0
    };
    if mode == 4 {
        let at = if p.m.r8(s + 0x78) != 0 && p.m.r8(s + 0x98) != 0 && p.m.r8(s + 0xb8) != 0 { s + 0x448 } else { s + 0x3c8 };
        let b = own(p, at);
        shape_state(p, b + 0x1a0, w);
        stream_draw_n(p, b + 0x380, w, 1);
    } else if (mode == 0 || mode == 3) && p.m.r8(s + 0x98) != 0 {
        let b = own(p, s + 0x3c8);
        shape_state(p, b + 0x1a0, w);
        stream_draw_n(p, b + 0x380, w, 1);
    } else {
        fullscreen(p, w);
    }
}

/// main+0xc583a8(w, c): the tile program (applied) with uniform 1 = (width, height, 1 / width, 1 / height) of the state
/// `c`, texture slot 0 = `c` and the full-screen triangle.
unsafe fn tile_draw(p: &mut Port, w: usize, c: usize) -> R<()> {
    let prog = tile_program(p);
    program_apply(p, prog, w)?;
    let (tw, th) = tex_size(p, c);
    let (fw, fh) = (tw as f32, th as f32);
    uniform_f(p, prog, w, 1, [fw, fh, 1.0 / fw, 1.0 / fh])?;
    program_texture(p, prog, w, 0, c)?;
    fullscreen(p, w);
    Ok(())
}

/// main+0xc59030(w, ctx, prog): the extra uniforms of the blur passes (9, 7, 10, 12, 11) by the pass mode: the clamped
/// ranges of the settings' record at q (+0x3b0, or +0x430 in mode 4 with the final conditions) and the pass size.
unsafe fn extra_pass(p: &mut Port, w: usize, ctx: usize, prog: usize) -> R<()> {
    let mode = p.m.r32(ctx + 4);
    let s = p.m.r64(ctx + 0x10);
    let (flag, q) = if mode == 3 {
        (blur_active(p, s), s + 0x3b0)
    } else if mode == 4 && p.m.r8(s + 0x78) != 0 && p.m.r8(s + 0x98) != 0 && p.m.r8(s + 0xb8) != 0 {
        (false, s + 0x430)
    } else {
        (mode == 0, s + 0x3b0)
    };
    let lo = gf(p, MIN_CLAMP);
    let mut s9 = fmax(p.m.f32(q + 0x58), lo);
    let mut s10 = fmax(p.m.f32(q + 0x5c), lo);
    let a = 1.0 - p.m.f32(q + 0x38);
    let b = fmin(a, 1.0);
    let s0 = if a < 0.0 { 0.0 } else { b };
    let s5 = 1.0 - s0;
    let (x, y) = (p.m.f32(q + 0x78).abs(), p.m.f32(q + 0x7c).abs());
    let s2 = if x > y { x } else { y };
    let s1 = p.m.f32(q + 0x3c) * s5;
    let k = gf(p, SCALE_K);
    let s1 = s0 + s1;
    let s3 = if s9 < s10 { s9 } else { s10 };
    let s4 = if flag { 0.0 } else { s0 };
    let s3 = 1.0 / s3;
    let s6 = s3 * k;
    let s0b = s6 + s2;
    uniform_f(p, prog, w, 9, [s4, s0, s1, s0b])?;
    let (mut zx, mut zy) = (1.0f32, 1.0f32);
    if p.m.r32(q + 0x18) == 0 {
        let (iw, ih) = (p.m.r32(ctx + 0x28) as i32, p.m.r32(ctx + 0x2c) as i32);
        let sum = iw.wrapping_mul(iw).wrapping_add(ih.wrapping_mul(ih));
        let d = (sum as f32).sqrt();
        zx = d / iw as f32;
        zy = d / ih as f32;
    }
    s9 *= zx;
    s10 *= zy;
    uniform_const(p, prog, w, 7)?;
    let s = p.m.r64(ctx + 0x10);
    let t = p.m.f32(s + 0x360);
    let t1 = fmin(t, 1.0);
    let t = if t < 0.0 { 0.0 } else { t1 };
    uniform_f(p, prog, w, 10, [s9, s10, t, 0.0])?;
    uniform_mem(p, prog, w, 12, s + 0x3a0)?;
    uniform_f(p, prog, w, 11, [p.m.f32(q + 0x78), p.m.f32(q + 0x7c), 0.0, 0.0])
}

/// A pass into render buffer `rb` at `level`: extent, viewport, bind, the pass's draw, unbind, barrier.
unsafe fn buffer_draw(p: &mut Port, this: usize, w: usize, ctx: usize, c: usize, level: u32) -> R<()> {
    let rb = c + 0x1e0;
    level_extent(p, rb, level);
    let vp = viewport::full(p, p.m.f32(rb + 8), p.m.f32(rb + 0xc));
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    draw_pass(p, this, w, ctx);
    targets::unbind(p, w)?;
    barrier_if_slot(p, p.m.r64(c + 0x200) + 0x60, w);
    Ok(())
}

/// main+0xc58c90(obj, src, level, layer) (inlined by the game where it builds the pyramid's render target): the
/// object's state := `src`, the setup flag 4, layer / level flags when they differ, then its virtual +0x10.
unsafe fn target_set(p: &mut Port, obj: usize, src: usize, level: u8, layer: u16) -> R<()> {
    assign(p, obj + 8, src)?;
    p.m.fetch_or32(obj + 0xc8, 4);
    if p.m.r16(obj + 0xcc) != layer {
        p.m.fetch_or32(obj + 0xc8, 2);
        p.m.w::<u16>(obj + 0xcc, layer);
    }
    if p.m.r8(obj + 0xce) != level {
        p.m.fetch_or32(obj + 0xc8, 1);
        p.m.w::<u8>(obj + 0xce, level);
    }
    let f = p.vfn(obj, 0x10);
    if f != SETUP_COLOR && f != SETUP_DEPTH {
        return Err("target object setup");
    }
    p.query(p.base + f, &[obj as u64]);
    Ok(())
}

/// The buffer record's render buffer (c +0x1e0) given the first target c +0x250 from the state `src` (and the second
/// target slots cleared).
unsafe fn buffer_target(p: &mut Port, c: usize, src: usize) -> R<()> {
    for o in (0x208..=0x240).step_by(8) {
        p.m.w::<u64>(c + o, 0);
    }
    p.m.w::<u64>(c + 0x200, (c + 0x250) as u64);
    target_set(p, c + 0x250, src, 0, 0)
}

/// A temporary target of the pool: main+0x7f0ad4(pool, w, &name, format, width, height, levels, 0, 0, 1, 0, 0).
unsafe fn alloc(p: &mut Port, w: usize, name: usize, format: u32, width: u32, height: u32, levels: u32) -> R<usize> {
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    let name: [u64; 2] = [(p.base + name) as u64, 0];
    let args = [pool as u64, w as u64, name.as_ptr() as u64, format as u64, width as u64, height as u64, levels as u64, 0, 0, 1, 0, 0];
    Ok(p.call_stack("depth of field buffer allocated by the call", ALLOC, &args)? as usize)
}

/// main+0x7f0354(pool, target): gives the target of the record's slot at `at` back (when set) and clears the slot.
unsafe fn gather(p: &mut Port, at: usize) -> R<()> {
    let t = p.m.r64(at);
    if t == 0 {
        return Ok(());
    }
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    p.helper("depth of field gather by the call", GATHER, &[pool as u64, t as u64])?;
    p.m.w::<u64>(at, 0);
    Ok(())
}

/// main+0xc4b934(this, w, ctx): the pass. Without the blur or depth effect on (the settings' record, this +0x5d0 entry
/// of the view) only the final pass can run.
pub(super) unsafe fn buffers(p: &mut Port, this: usize, w: usize, ctx: usize) -> R<()> {
    let n = p.m.r32(this + 0x18) & p.m.r32(ctx);
    let n = if p.m.r32(this + 0x5c8) <= n { 0 } else { n };
    let rec = p.m.r64(p.m.r64(this + 0x5d0).wrapping_add((n as i32 as isize as usize).wrapping_mul(8)));
    let f = p.m.f32(rec + 0x238);
    let none = p.m.r8(rec + 0x18) == 0 && p.m.r8(rec + 0x38) == 0;
    let body = if none { p.m.r8(rec + 0x98) != 0 && f > 0.0 } else { f > 0.0 || ready(p, rec) };
    if !body {
        return finish(p, this, w, ctx, rec);
    }
    if p.m.r8(this + 0x188) != 0 {
        return Err("depth of field offscreen path (this +0x188)");
    }
    check(p, this, w, ctx, rec)?;
    p.m.commit();
    // commit
    allocate(p, this, w, ctx, rec)?;
    p.m.w::<u32>(ctx + 4, 1);
    let c = p.m.r64(ctx + 8);
    let s = p.m.r64(ctx + 0x10);
    // The first render state (the level passes' own), the pyramid's render target, then the level passes.
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = (g[7] & 0xffff_fff0) | 7;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    buffer_target(p, c, c + 0x348)?;
    let r = Run { this, w, ctx, c, s, u: p.m.r8(ctx + 0x30) != 0, tile: ready(p, s), wt: p.m.r8(s + 0x580) != 0 };
    levels(p, &r)?;
    if p.m.r32(this + 0x8e0) == 2 {
        mip_copy(p, &r)?;
    } else {
        blur_pass(p, &r)?;
    }
    // The temporaries are given back.
    let v = p.m.r32(ctx);
    let v = if p.m.r32(this + 0x5b8) <= v { 0 } else { v };
    let cv = p.m.r64(this + 0x5c0).wrapping_add((v as i32 as isize as usize).wrapping_mul(0x718));
    gather(p, cv + 0x340)?;
    gather(p, cv + 0x438)?;
    gather(p, cv + 0x530)?;
    finish(p, this, w, ctx, rec)
}

/// The final block of the function: with the settings' +0x78 and the conditions of `final_runs` the game runs
/// main+0x21f10e4 with the pass mode 4.
unsafe fn finish(p: &mut Port, this: usize, w: usize, ctx: usize, rec: usize) -> R<()> {
    if !final_runs(p, rec) {
        return Ok(());
    }
    let s = p.m.r64(ctx + 0x10);
    if !p.m.committed() {
        context_ok(p, w)?;
        program_ready(p, p.m.r64(s + 0x528))?;
        unbind_ok(p, p.m.r64(ctx + 0x18))?;
    }
    p.m.commit();
    // commit
    p.m.w::<u32>(ctx + 4, 4);
    final_pass(p, this, w, ctx)
}

/// Everything that can stop, before the first call: the programs of every pass the conditions allow, the render
/// buffers' unbind, the pyramid target's setup virtual.
unsafe fn check(p: &Port, this: usize, w: usize, ctx: usize, rec: usize) -> R<()> {
    context_ok(p, w)?;
    mip_quad_check(p)?;
    let s = p.m.r64(ctx + 0x10);
    let c = p.m.r64(ctx + 8);
    let u = p.m.r8(ctx + 0x30) != 0;
    program_ready(p, p.m.r64(s + 0x4d8))?;
    if p.m.r8(s + 0x580) != 0 && !ready(p, s) {
        program_ready(p, weight_program(p, s, u))?;
    }
    if ready(p, s) {
        program_ready(p, tile_program(p))?;
    }
    unbind_ok(p, c + 0x1e0)?;
    setup_ok(p, c + 0x250)?;
    if depth_pass(p, rec) {
        program_ready(p, depth_program(p, p.m.r8(ctx + 0x30) == 1))?;
        program_ready(p, p.m.r64(s + 0x4f0))?;
    }
    unbind_ok(p, p.m.r64(ctx + 0x18))?;
    if p.m.r32(this + 0x8e0) != 2 {
        program_ready(p, blur_program(p, this, s, p.m.r8(ctx + 0x30) as usize))?;
    }
    if final_runs(p, rec) {
        program_ready(p, p.m.r64(s + 0x528))?;
    }
    Ok(())
}

/// The stage values the passes below share.
struct Run {
    this: usize,
    w: usize,
    ctx: usize,
    /// The view's buffer record and the settings record.
    c: usize,
    s: usize,
    /// ctx +0x30: the reversed depth range.
    u: bool,
    /// The settings' blur is ready (main+0xc58a64) and the weight mode (+0x580).
    tile: bool,
    wt: bool,
}

/// The allocation phase: the three textures taken from the pool and copied into the view's record (this +0x5c0), then
/// the depth pyramid pass.
unsafe fn allocate(p: &mut Port, this: usize, w: usize, ctx: usize, rec: usize) -> R<()> {
    let tex = p.m.r64(p.m.r64(ctx + 0x18) + 0x20);
    let v = p.m.r32(ctx);
    let v = if p.m.r32(this + 0x5b8) <= v { 0 } else { v };
    let cv = p.m.r64(this + 0x5c0).wrapping_add((v as i32 as isize as usize).wrapping_mul(0x718));
    let width = (p.m.r16(tex + 0x38) as u32).max(1);
    let h0 = p.m.r16(tex + 0x3a) as u32;
    let height = (if p.m.r16(tex + 0x42) == 3 { h0 } else { 1 }).max(h0);
    let format = p.m.r8(tex + 0xa8) as u32;
    let f = p.m.f32(rec + 0x238);
    let t = f as i32;
    let up = (0.0 <= f && f != t as f32) as i32;
    let (aw, ah, levels) = if ready(p, rec) {
        (width, height, t + up + 1)
    } else {
        let a = p.m.f32(rec + 0x53c) * width as f32;
        let a = a * p.m.f32(rec + 0x538);
        let b = p.m.f32(rec + 0x540) * height as f32;
        let b = p.m.f32(rec + 0x538) * b;
        (a as u32, b as u32, (t + up).max(1))
    };
    let blur = alloc(p, w, NAME_BLUR, format, aw, ah, levels as u32)?;
    p.m.w::<u64>(cv + 0x340, blur as u64);
    texture_assign(p, cv + 0x348, blur)?;
    if p.m.r8(rec + 0x580) != 0 && !ready(p, rec) {
        let weight = alloc(p, w, NAME_WEIGHT, 1, aw, ah, levels as u32)?;
        p.m.w::<u64>(cv + 0x438, weight as u64);
        texture_assign(p, cv + 0x440, weight)?;
    }
    if depth_pass(p, rec) {
        let fd = p.m.f32(rec + 0x258);
        let td = fd as i32;
        let l = td + (0.0 <= fd && fd != td as f32) as i32;
        let extra = if weight_mode(p, rec) && ((l + 1) as u32) < 3 { 2 } else { 1 };
        let depth = alloc(p, w, NAME_DEPTH, 1, width >> 1, height >> 1, (l + extra) as u32)?;
        p.m.w::<u64>(cv + 0x530, depth as u64);
        shadow_mask_setup(p, cv + 0x538, depth)?;
        p.m.w::<u32>(ctx + 4, 2);
        depth_pyramid(p, this, w, ctx)?;
    }
    Ok(())
}

/// main+0xc594f8(this, w, ctx): the depth pyramid. A render state, the record's render buffer given the depth state
/// (c +0x538) as its target, the program of the depth set (entry 0x10, 0x20 with the reversed range) drawn once at level
/// 0 with its uniforms 6, 7, 8, the scene texture 1, then the settings' program +0x4f0 at each further level with the
/// level's size, then the depth state's lod range.
unsafe fn depth_pyramid(p: &mut Port, this: usize, w: usize, ctx: usize) -> R<()> {
    let c = p.m.r64(ctx + 8);
    let s = p.m.r64(ctx + 0x10);
    let rb = c + 0x1e0;
    let depth = c + 0x538;
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    g[7] = (g[7] & 0xffff_fff0) | 1;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    buffer_target(p, c, depth)?;
    p.m.w::<u16>(c + 0x60c, 0x101);
    p.m.w::<u8>(c + 0x60e, 1);
    p.m.fetch_or32(c + 0x61c, 4);
    let prog = depth_program(p, p.m.r8(ctx + 0x30) == 1);
    program_apply(p, prog, w)?;
    let (near, far) = (p.m.f32(ctx + 0x20), p.m.f32(ctx + 0x24));
    let (c1, c2) = (p.m.f32(s + 0x1f8), p.m.f32(s + 0x218));
    let tenth = gf(p, TENTH);
    let u = p.m.r8(ctx + 0x30) != 0;
    let x = near / far;
    let ratio = 1.0 - x;
    let inv = 1.0 / near;
    let mx = if c1 > c2 { c1 } else { c2 };
    let mn = if c1 < c2 { c1 } else { c2 };
    let hi = if mx - mn < tenth { mx + tenth } else { mx };
    let ratio = ratio / near;
    let (mut b, mut span) = (hi, 1.0f32);
    if u {
        span = far - near;
        b = (hi - near) / span;
    }
    let d = mn - hi;
    let k = span / d;
    uniform_f(p, prog, w, 6, [inv, ratio, k, k * -b])?;
    uniform_const(p, prog, w, 7)?;
    program_texture(p, prog, w, 1, c + 0xf0)?;
    let half = 0.5f32;
    let (dw, dh) = tex_size(p, depth);
    let m = p.m.f32(this + 0x8e4);
    uniform_f(p, prog, w, 8, [(half / dw as f32) * m, m * (half / dh as f32), m * 0.0, m * 0.0])?;
    level_extent(p, rb, 0);
    let vp = viewport::full(p, p.m.f32(rb + 8), p.m.f32(rb + 0xc));
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    draw_pass(p, this, w, ctx);
    targets::unbind(p, w)?;
    barrier_if_slot(p, p.m.r64(c + 0x200) + 0x60, w);
    let prog2 = p.m.r64(p.m.r64(ctx + 0x10) + 0x4f0);
    program_apply(p, prog2, w)?;
    let n = p.m.r8(c + 0x571) as u32;
    for k in 1..n {
        p.m.w::<u32>(c + 0x608, 0);
        let l = (k - 1) as f32;
        p.m.w::<f32>(c + 0x600, l);
        p.m.w::<f32>(c + 0x604, l);
        p.m.fetch_or32(c + 0x61c, 8);
        program_texture(p, prog2, w, 1, depth)?;
        let sh = k & 31;
        let dw = ((p.m.r16(c + 0x568) as u32 >> sh) as i32).max(1);
        let h = p.m.r16(c + 0x56a) as u32;
        let hk = if p.m.r16(c + 0x572) == 3 { h } else { 1 } as i32;
        let hs = (h >> sh) as i32;
        let dh = if hk > hs { hk } else { hs };
        let m = p.m.f32(this + 0x8e4);
        uniform_f(p, prog2, w, 8, [(half / dw as f32) * m, m * (half / dh as f32), m * 0.0, m * 0.0])?;
        buffer_draw(p, this, w, ctx, c, k)?;
    }
    let lod = p.m.f32(p.m.r64(ctx + 0x10) + 0x258);
    p.m.w::<u32>(c + 0x608, 0);
    p.m.w::<f32>(c + 0x600, lod);
    p.m.w::<f32>(c + 0x604, lod);
    p.m.fetch_or32(c + 0x61c, 8);
    p.m.w::<u16>(c + 0x60c, 0x101);
    p.m.w::<u8>(c + 0x60e, 1);
    p.m.fetch_or32(c + 0x61c, 4);
    Ok(())
}

/// The per-level passes into the pyramid: the level count (c +0x381) levels of the blur program (settings +0x4d8, or
/// the weight program at level 0 with the weight mode), each into the record's render buffer at its mip level.
unsafe fn levels(p: &mut Port, r: &Run) -> R<()> {
    let tobj = Host::new(p, TARGET_BYTES);
    new_target(p, tobj.addr(), COLOR_VTABLE);
    let res = levels_run(p, r, tobj.addr());
    // The destructor: the object's slot reference given back.
    release(p, tobj.addr() + 8);
    tobj.release(p);
    res
}

unsafe fn levels_run(p: &mut Port, r: &Run, tobj: usize) -> R<()> {
    let (w, c, s) = (r.w, r.c, r.s);
    let mut prog = p.m.r64(s + 0x4d8);
    program_apply(p, prog, w)?;
    p.m.w::<u8>(c + 0x41e, 1);
    p.m.w::<u16>(c + 0x41c, 0x101);
    p.m.fetch_or32(c + 0x42c, 4);
    p.m.w::<u16>(c + 0x514, 0x101);
    p.m.w::<u8>(c + 0x516, 1);
    p.m.fetch_or32(c + 0x524, 4);
    let n = p.m.r8(c + 0x381) as u32;
    for k in 0..n {
        if k == 0 {
            first_level(p, r, tobj, &mut prog)?;
        } else {
            next_level(p, r, tobj, k, &mut prog)?;
        }
    }
    let last = if n == 0 { -1.0 } else { n as f32 + -1.0 };
    p.m.w::<u16>(c + 0x41c, 0x101);
    p.m.w::<u8>(c + 0x41e, 2);
    p.m.fetch_or32(c + 0x42c, 4);
    p.m.w::<u32>(c + 0x410, 0);
    p.m.w::<f32>(c + 0x414, last);
    p.m.w::<u32>(c + 0x418, 0);
    p.m.fetch_or32(c + 0x42c, 8);
    Ok(())
}

/// Level 0: with the blur ready the pyramid's level 0 is drawn by main+0xc583a8; else the program's input scale
/// uniform, the scene texture, the constant vector and the pass (the weight mode adds the weight target as the second
/// target, the weight program with the depth lod range and the ranges as uniform 6 and the input size as uniform 8).
unsafe fn first_level(p: &mut Port, r: &Run, tobj: usize, prog: &mut usize) -> R<()> {
    let (this, w, ctx, c, s) = (r.this, r.w, r.ctx, r.c, r.s);
    let rb = c + 0x1e0;
    if r.tile {
        level_extent(p, rb, 0);
        let vp = viewport::full(p, p.m.f32(rb + 8), p.m.f32(rb + 0xc));
        targets::bind(p, rb, w)?;
        viewport::apply(p, vp.as_ptr() as usize, w, rb);
        p.m.w::<u16>(c + 0xd4, 0x101);
        p.m.w::<u8>(c + 0xd6, 0);
        p.m.fetch_or32(c + 0xe4, 4);
        tile_draw(p, w, c)?;
        targets::unbind(p, w)?;
        barrier_if_slot(p, p.m.r64(c + 0x200) + 0x60, w);
        program_apply(p, *prog, w)?;
        return Ok(());
    }
    if !r.wt {
        let (tw, th) = tex_size(p, c);
        let m = p.m.f32(this + 0x8e4);
        let half = 0.5f32;
        uniform_f(p, *prog, w, 8, [m * (half / tw as f32), m * (half / th as f32), m * 0.0, m * 0.0])?;
        return level_tail(p, r, tobj, *prog, 0);
    }
    // The weight mode: the weight target is the buffer's second target.
    copy_target(p, tobj, p.m.r64(c + 0x438))?;
    p.m.w::<u64>(c + 0x208, tobj as u64);
    p.m.w::<u32>(ctx + 4, 0);
    *prog = weight_program(p, s, r.u);
    let prog = *prog;
    program_apply(p, prog, w)?;
    p.m.w::<u16>(c + 0xd4, 0);
    p.m.w::<u8>(c + 0xd6, 0);
    p.m.fetch_or32(c + 0xe4, 4);
    p.m.w::<u16>(c + 0x1c4, 0);
    p.m.w::<u8>(c + 0x1c6, 0);
    p.m.fetch_or32(c + 0x1d4, 4);
    program_texture(p, prog, w, 1, c + 0xf0)?;
    if depth_pass(p, s) {
        p.m.w::<u32>(c + 0x608, 0);
        let l = (p.m.r8(c + 0x571) as u32).wrapping_sub(1) as f32;
        p.m.w::<f32>(c + 0x600, l);
        p.m.w::<f32>(c + 0x604, l);
        p.m.fetch_or32(c + 0x61c, 8);
        program_texture(p, prog, w, 3, c + 0x538)?;
    }
    let (near, far) = (p.m.f32(ctx + 0x20), p.m.f32(ctx + 0x24));
    let one = 1.0f32;
    let a = near / far;
    let span = far - near;
    let a = one - a;
    let inv = one / near;
    let x = if r.u { span } else { inv };
    let b = a / near;
    let y = if r.u { near } else { b };
    let (a1, b1) = (p.m.f32(s + 0x1b8), p.m.f32(s + 0x1d8));
    let (c1, c2) = (p.m.f32(s + 0x1f8), p.m.f32(s + 0x218));
    let z = if a1 < b1 { a1 } else { b1 };
    let wv = if c1 > c2 { c1 } else { c2 };
    uniform_f(p, prog, w, 6, [x, y, z, wv])?;
    let (tw, th) = tex_size(p, c);
    let (fw, fh) = (tw as f32, th as f32);
    uniform_f(p, prog, w, 8, [one / fw, one / fh, fw, fh])?;
    if p.m.r8(s + 0x98) != 0 {
        extra_pass(p, w, ctx, prog)?;
    }
    level_tail(p, r, tobj, prog, 0)
}

/// The common part of the passes of level `k`: the input texture 0 (the record's own state), the constant vector, the
/// weight mode's level flag of the second target and the pass.
unsafe fn level_tail(p: &mut Port, r: &Run, tobj: usize, prog: usize, k: u32) -> R<()> {
    let (this, w, ctx, c) = (r.this, r.w, r.ctx, r.c);
    // Level 0 binds the record's own state as texture 0 here (the other levels bound theirs before).
    if k == 0 {
        program_texture(p, prog, w, 0, c)?;
    }
    uniform_const(p, prog, w, 7)?;
    if r.wt && !r.tile && p.m.r8(tobj + 0xce) as u32 != k {
        p.m.fetch_or32(tobj + 0xc8, 1);
        p.m.w::<u8>(tobj + 0xce, k as u8);
    }
    buffer_draw(p, this, w, ctx, c, k)
}

/// Level `k` > 0: the pyramid's previous level as texture 0 (and, with the weight mode, the weight's as texture 1), the
/// input scale at the level's size, the constant vector and the pass.
unsafe fn next_level(p: &mut Port, r: &Run, tobj: usize, k: u32, prog: &mut usize) -> R<()> {
    let (this, w, ctx, c, s) = (r.this, r.w, r.ctx, r.c, r.s);
    if weight_mode(p, s) && k == 1 {
        p.m.w::<u32>(ctx + 4, 1);
        *prog = p.m.r64(s + 0x4d8);
        program_apply(p, *prog, w)?;
        let lod = p.m.f32(p.m.r64(ctx + 0x10) + 0x258);
        p.m.w::<u32>(c + 0x608, 0);
        p.m.w::<f32>(c + 0x600, lod);
        p.m.w::<f32>(c + 0x604, lod);
        p.m.fetch_or32(c + 0x61c, 8);
    }
    let prog = *prog;
    let l = (k - 1) as f32;
    p.m.w::<u32>(c + 0x418, 0);
    p.m.w::<f32>(c + 0x410, l);
    p.m.w::<f32>(c + 0x414, l);
    p.m.fetch_or32(c + 0x42c, 8);
    program_texture(p, prog, w, 0, c + 0x348)?;
    let (num, sh) = if !weight_mode(p, s) {
        (0.5f32, k)
    } else {
        p.m.w::<f32>(c + 0x508, l);
        p.m.w::<f32>(c + 0x50c, l);
        p.m.w::<u32>(c + 0x510, 0);
        p.m.fetch_or32(c + 0x524, 8);
        program_texture(p, prog, w, 1, c + 0x440)?;
        (1.0f32, k - 1)
    };
    let sh = sh & 31;
    let iw = ((p.m.r16(c + 0x378) as u32 >> sh) as i32).max(1);
    let h = p.m.r16(c + 0x37a) as u32;
    let hk = if p.m.r16(c + 0x382) == 3 { h } else { 1 } as i32;
    let hs = (h >> sh) as i32;
    let ih = if hk > hs { hk } else { hs };
    let m = p.m.f32(this + 0x8e4);
    uniform_f(p, prog, w, 8, [(num / iw as f32) * m, (num / ih as f32) * m, m * 0.0, m * 0.0])?;
    level_tail(p, r, tobj, prog, k)
}

/// main+0xc59394(w, ctx): the mode 2 pass: the buffer bound with its viewport, a render state, the mip copy of the
/// pyramid (main+0xdf51fc at the level count - 1) and the unbind.
unsafe fn mip_copy(p: &mut Port, r: &Run) -> R<()> {
    let (w, ctx, c, s) = (r.w, r.ctx, r.c, r.s);
    let rb = p.m.r64(ctx + 0x18);
    let vp = viewport::full(p, p.m.f32(rb + 8), p.m.f32(rb + 0xc));
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    g[7] |= 0xf;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let scale = [2.0f32, 2.0f32];
    let level = p.m.f32(s + 0x238) + -1.0;
    let offset = p.m.r64(p.base + QUAD_OFFSET);
    mip_quad(p, w, c + 0x348, vp.as_ptr() as usize, scale.as_ptr() as usize, offset as usize, level.to_bits())?;
    targets::unbind(p, w)
}

/// The final blur pass: the pass mode 3. A render state by the settings' switches, the view's render buffer bound, the
/// effect's program by the mode with its uniforms 0, 8, 6, 7, 4, 5, 13 (and, with the tile blur, 1, 3 and 2), the
/// textures 2 (the pyramid), 3 (the depth pyramid), 1 (the scene) and 4, the extra uniforms with the settings' +0x98,
/// the draw and the unbind.
unsafe fn blur_pass(p: &mut Port, r: &Run) -> R<()> {
    let (this, w, ctx, c, s) = (r.this, r.w, r.ctx, r.c, r.s);
    p.m.w::<u32>(ctx + 4, 3);
    let (b14, b15) = if p.m.r8(s + 0x38) == 0 {
        if p.m.r8(s + 0x18) == 0 || p.m.r8(s + 0x58) != 0 { (false, false) } else { (false, p.m.r8(s + 0x98) == 0) }
    } else if p.m.r8(s + 0x18) == 0 {
        (p.m.r8(s + 0x98) == 0, false)
    } else {
        (false, false)
    };
    let mut g = default_state(p);
    if b14 {
        g[0] = (g[0] & 0xffff_0000) | 1;
        set_byte(&mut g, 0x64, 2);
    } else if b15 {
        g[0] = (g[0] & 0xffff_0000) | 1;
        set_byte(&mut g, 0x64, 5);
    } else {
        g[0] &= 0xffff_0000;
    }
    g[7] = (g[7] & 0xffff_fff0) | 7;
    let rb = p.m.r64(ctx + 0x18);
    let vp = viewport::full(p, p.m.f32(rb + 8), p.m.f32(rb + 0xc));
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let u = p.m.r8(ctx + 0x30) != 0;
    let prog = blur_program(p, this, s, p.m.r8(ctx + 0x30) as usize);
    program_apply(p, prog, w)?;
    let (near, far) = (p.m.f32(ctx + 0x20), p.m.f32(ctx + 0x24));
    let one = 1.0f32;
    let mut depth = p.m.f32(s + 0x278);
    let (iw, ih) = (p.m.r32(ctx + 0x28) as i32 as f32, p.m.r32(ctx + 0x2c) as i32 as f32);
    if u {
        depth = (depth - near) / (far - near);
    }
    let level = p.m.f32(s + 0x238);
    let rounded = libm(p, LIBM_CEIL, level);
    let strength = one - p.m.f32(s + 0x298);
    uniform_f(p, prog, w, 0, [level, rounded, depth, strength])?;
    let (fx, fy) = (one / iw, one / ih);
    let deg = gf(p, DEG_TO_RAD);
    let a = libm(p, LIBM_A, p.m.f32(this + 0x8ec) * deg);
    let scale = p.m.f32(this + 0x8e8);
    let a = a * scale;
    let bb = libm(p, LIBM_B, p.m.f32(this + 0x8ec) * deg);
    let bs = bb * scale;
    uniform_f(p, prog, w, 8, [fx * a, fy * bs, fx * -bs, fy * a])?;

    // The focus ranges (+0x1b8 / +0x1d8, and +0x1f8 / +0x218 with the second range on) as depth scale / bias.
    let tenth = gf(p, TENTH);
    let (ra, rb2) = (p.m.f32(s + 0x1b8), p.m.f32(s + 0x1d8));
    let lo = if ra < rb2 { ra } else { rb2 };
    let mut hi = if ra > rb2 { ra } else { rb2 };
    if hi - lo < tenth {
        hi += tenth;
    }
    let lo_off = lo - near;
    let (mut span, mut lo_n) = (one, lo);
    if u {
        span = far - near;
        lo_n = lo_off / span;
    }
    let on = p.m.r8(s + 0x18) != 0;
    let k0 = span / (hi - lo);
    let pd0_0 = k0 * -lo_n;
    let (mut p148_1, mut pd0_1) = (0.0f32, 0.0f32);
    let (c1, c2) = (p.m.f32(s + 0x1f8), p.m.f32(s + 0x218));
    let cmax = if c1 > c2 { c1 } else { c2 };
    if on {
        let cmin = if c1 < c2 { c1 } else { c2 };
        let hi2 = if cmax - cmin < tenth { cmax + tenth } else { cmax };
        let (hh, bnd) = if u {
            let hh = far - near;
            (hh, (hi2 - near) / hh)
        } else {
            (one, hi2)
        };
        p148_1 = hh / (cmin - hi2);
        pd0_1 = p148_1 * -bnd;
    }
    // The depth conversions: the clip depth (ctx +0x31 reversed: the reciprocal form) of the first range's ends.
    let (a0, a1);
    if p.m.r8(ctx + 0x31) == 0 {
        let rng = far - near;
        let t = lo_off / rng;
        a0 = t + t + -1.0;
        a1 = if on {
            let t2 = (cmax - near) / rng;
            t2 + t2 + -1.0
        } else {
            0.0
        };
    } else {
        let k = far * (near * (near + near));
        let nl = near * lo;
        let sum = far + near;
        let diff = far - near;
        a0 = (sum * nl - k) / (diff * nl);
        a1 = if on {
            let nm = near * cmax;
            (sum * nm - k) / (diff * nm)
        } else {
            0.0
        };
    }
    uniform_f(p, prog, w, 6, [one / near, (one - near / far) / near, a0, a1])?;
    uniform_const(p, prog, w, 7)?;
    // The second focus range's terms (+0xd8 on; +0x2b8 / +0x2bc, +0x2c0 / +0x2c4, +0xf8).
    let (mut v140, mut vc8) = ([0.0f32; 2], [0.0f32; 2]);
    if p.m.r8(s + 0xd8) != 0 {
        let (qa, qb) = (p.m.f32(s + 0x2b8), p.m.f32(s + 0x2bc));
        let mn = if qa < qb { qa } else { qb };
        let mut mx = if qa > qb { qa } else { qb };
        if mx - mn < tenth {
            mx += tenth;
        }
        let range = far - near;
        let (base, sc0) = if u { ((mn - near) / range, range) } else { (mn, one) };
        let sc1 = sc0 / (mx - mn);
        let (qc, qd) = (p.m.f32(s + 0x2c0), p.m.f32(s + 0x2c4));
        let hi3 = if qc > qd { qc } else { qd };
        let lo3 = if qc < qd { qc } else { qd };
        let sc2n = if u { range } else { one };
        let hb = if u { (hi3 - near) / range } else { hi3 };
        let top = if lo3 - hi3 < tenth { lo3 + tenth } else { lo3 };
        let sc2 = sc2n / (top - hi3);
        v140 = [sc1, sc2];
        vc8 = [sc1 * -base, sc2 * -hb];
        if p.m.r8(s + 0xf8) == 0 {
            v140[1] = one;
            vc8[1] = one;
        }
    }
    uniform_f(p, prog, w, 4, [k0, p148_1, v140[0], v140[1]])?;
    uniform_f(p, prog, w, 5, [pd0_0, pd0_1, vc8[0], vc8[1]])?;
    uniform_mem(p, prog, w, 13, s + 0x4c8)?;
    program_texture(p, prog, w, 2, c + 0x348)?;
    if depth_pass(p, s) {
        program_texture(p, prog, w, 3, c + 0x538)?;
    }
    program_texture(p, prog, w, 1, c + 0xf0)?;
    let tile = p.m.r8(s + 0x118) != 0 && p.m.r64(s + 0x548) != 0 && p.m.r8(s + 0x38) != 0;
    if tile {
        program_texture(p, prog, w, 4, this + 0x7f0)?;
        p.m.w::<f32>(s + 0x550, fx);
        p.m.w::<f32>(s + 0x554, fy);
        let hv = p.m.r32(ctx + 0x2c) as i32 as f32;
        p.m.w::<f32>(s + 0x558, one / (fx * hv));
        let keep = p.m.r32(s + 0x340);
        p.m.w::<u32>(s + 0x55c, keep);
        uniform_mem(p, prog, w, 1, s + 0x550)?;
        uniform_mem(p, prog, w, 3, s + 0x570)?;
    }
    if tile || p.m.r32(this + 0x5b4) == 1 {
        let mut b = [0u8; 16];
        for (i, o) in [0x560usize, 0x564, 0x568].iter().enumerate() {
            b[i * 4..i * 4 + 4].copy_from_slice(&p.m.r32(s + o).to_le_bytes());
        }
        b[12..16].copy_from_slice(&p.m.r32(this + 0x8f0).to_le_bytes());
        uniform_bytes(p, prog, w, 2, b)?;
    }
    if p.m.r8(s + 0x98) != 0 {
        extra_pass(p, w, ctx, prog)?;
    }
    draw_pass(p, this, w, ctx);
    targets::unbind(p, w)
}

/// main+0x21f10e4(this, w, ctx): the final pass (mode 4): a render state by the settings' +0x380, the render buffer
/// bound with its viewport, the settings' program +0x528 with the extra uniforms, the draw and the unbind.
unsafe fn final_pass(p: &mut Port, this: usize, w: usize, ctx: usize) -> R<()> {
    let s = p.m.r64(ctx + 0x10);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[7] = (g[7] & 0xffff_fff0) | 7;
    let mode = p.m.r32(s + 0x380);
    if mode <= 3 {
        let sh = mode * 8;
        let b0 = (0x0a01_0505u32 >> sh) as u8;
        let b1 = (0x0203_0206u32 >> sh) as u8;
        g[10] = u32::from_le_bytes([b0, b0, b1, b1]);
        g[11] = (g[11] & 0xffff_0000) | 0x101;
    }
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let rb = p.m.r64(ctx + 0x18);
    let vp = viewport::full(p, p.m.f32(rb + 8), p.m.f32(rb + 0xc));
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let prog = p.m.r64(s + 0x528);
    program_apply(p, prog, w)?;
    extra_pass(p, w, ctx, prog)?;
    draw_pass(p, this, w, ctx);
    targets::unbind(p, w)
}
