// agl::pfx::FilterAA::FXAA, main+0xe97ed4(this, w, view, ctx): the anti-aliasing filter of the final image. ctx: [0] the
// render buffer drawn into, [1] its viewport, [2] the source texture state, [3] the shader of the plain copy, [4] the
// shader of the copy back, [5] the filter's shader, [10] / [12] optional textures (with this +0x988 the reprojection
// variant), [14] an optional luma texture state.
//
// With the effect off (this +0xae0 not above 0) the source is copied to ctx[0] with the quad of main+0xe97b10. Else the
// view's record (this +0xb90, 0xb78 each) takes the source state (+0x258) and the luma state (+0x348: the default
// texture, ctx[14], or a "luma" target the pass draws first); the filter's uniforms, the two textures and the
// full-screen triangle are drawn into ctx[0] -- or, when this +0xb7a is set and the source is not the buffer's size, into
// a "temp_target" of the source's size that is then copied to ctx[0] through ctx[4]'s shader. The temporaries are given
// back at the end.
// Everything that can stop the port (programs still being set up, the render buffers' unbind and setup virtuals,
// locked targets, the context type) is checked before the commit point. The temporary targets come from the pool, which
// keeps state, so the port is applied only past the first allocation; predicting stops at the texture the pass binds
// (its finish flag) or the allocation.

use super::cubemap::{set_uniform, texture_quad};
use super::draw::{Port, R};
use super::scene_passes::fullscreen;
use super::targets::{self, CONTEXT_TYPE_GUARD, RENDER_BUFFER_UNBIND};
use super::texture_slots::{alloc_target, barrier_if_slot, copy_target, new_buffer, new_target, release, release_sampler, set_extent, texture_assign, Host, COLOR_VTABLE};
use super::ui_callees::draw_texture;
use super::vfx::{program_apply, program_texture, shadow_mask_setup};
use super::viewport;

/// [var]: the quad's scale and offset of main+0xe97b10's caller (the values the frame's copies use).
const QUAD_SCALE: usize = 0x4634f50;
const QUAD_OFFSET: usize = 0x462cee0;
/// [[var]]: the temporary target pool; main+0x7f0ad4 allocates from it, main+0x7f0354(pool, target) gives one back.
const TARGET_POOL: usize = 0x4634e70;
const ALLOC: usize = 0x7f0ad4;
const GATHER: usize = 0x7f0354;
/// main+0xf817bc(target object, state): the luma target's texture given to the record's target object.
const SSAO_ALLOC: usize = 0xf817bc;
/// [[[var]]+0x18]: the default texture state.
const DEFAULT_TEXTURES: usize = 0x4631740;
/// The pure 4x4 inverse (out, in).
const INVERSE_4X4: usize = 0x93abb8;
/// [var]: the sead viewport's vtable and the frame's orientation.
const VIEWPORT_VTABLE: usize = 0x46323b0;
const ORIENTATION: usize = 0x46312d8;
/// "temp_target", "luma", "agl::TextureData", "agl::TextureSampler(from texture)".
const NAME_TEMP: usize = 0x3741d8b;
const NAME_LUMA: usize = 0x37de6df;
const TEXTURE_NAME: usize = 0x37ccf1f;
const SAMPLER_NAME: usize = 0x37a908e;
/// Floats the pass reads from the game's data: 0.15, 0.1, the two three-entry tables, and the reprojection's constant.
const K_FIFTEEN: usize = 0x380e98c;
const K_TENTH: usize = 0x380efb8;
const TABLE_A: usize = 0x38417d8;
const TABLE_B: usize = 0x38417e4;
const K_SQRT2: usize = 0x380f6b0;
/// Target object virtual +0x10 (colour / depth setup, which only write the object) and +0x18 (the view rebuild).
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;
const VIEW_SETUP: usize = 0x2a2f550;
const VIEW_SETUP_ALL_LEVELS: usize = 0x2a2f89c;

/// The temporary target group's layout (the game's stack object): the texture state pointer, the colour target object
/// (+8), the render buffer (+0xf8) and the viewport (+0x168).
const T_TARGET: usize = 8;
const T_STATE: usize = 0x10;
const T_RB: usize = 0xf8;
const T_VP: usize = 0x168;
const T_BYTES: usize = 0x190;

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

unsafe fn gf(p: &Port, a: usize) -> f32 {
    p.m.f32(p.base + a)
}

fn words(v: [f32; 4]) -> [u32; 4] {
    v.map(f32::to_bits)
}

fn bytes(v: [u32; 4]) -> [u8; 16] {
    let mut b = [0u8; 16];
    for (k, x) in v.iter().enumerate() {
        b[k * 4..k * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    b
}

/// A program still being set up (+0x10 / +0x14).
unsafe fn program_ready(p: &Port, prog: usize) -> R<()> {
    if prog == 0 {
        return Err("fxaa program missing");
    }
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// The context's type is known and a buffer still bound unbinds with the known virtual.
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

/// A render buffer's virtual +0x28 is the wrapper's unbind.
unsafe fn unbind_ok(p: &Port, rb: usize) -> R<()> {
    if rb == 0 || p.m.r64(rb) == 0 || p.vfn(rb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    Ok(())
}

/// A target object's view rebuild (virtual +0x18) is one of the two the bind knows.
unsafe fn view_ok(p: &Port, t: usize) -> R<()> {
    if p.m.r64(t) == 0 {
        return Err("render target setup");
    }
    match p.vfn(t, 0x18) {
        VIEW_SETUP | VIEW_SETUP_ALL_LEVELS => Ok(()),
        _ => Err("render target setup"),
    }
}

/// What targets::bind can stop at in the buffer's targets: a lock held by another thread, an unknown view rebuild.
unsafe fn bind_ok(p: &Port, rb: usize) -> R<()> {
    for k in 0..9 {
        let t = p.m.r64(rb + 0x20 + k * 8);
        if t == 0 || p.m.i32(t + 0x60) == -1 {
            continue;
        }
        let f = p.m.r32(t + 0xc8);
        if f == 0 {
            continue;
        }
        if (f as i32) < 0 {
            return Err("render target locked");
        }
        if f & 0x7fff_ffff != 0 {
            view_ok(p, t)?;
        }
    }
    Ok(())
}

/// The pool the temporary targets are taken from and given back to.
unsafe fn pool(p: &Port) -> usize {
    p.m.r64(p.m.r64(p.base + TARGET_POOL))
}

/// main+0x7f0354(pool, target).
unsafe fn gather(p: &mut Port, target: usize) -> R<()> {
    let pl = pool(p);
    p.helper("fxaa gather by the call", GATHER, &[pl as u64, target as u64])?;
    Ok(())
}

/// A stack texture with a sampler made from a texture state (main+0x979454): the constructor's constants and the state
/// assigned.
struct Sampled(Host);

impl Sampled {
    unsafe fn new(p: &mut Port, src: usize) -> R<Sampled> {
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
        let s = Sampled(h);
        texture_assign(p, x, src)?;
        Ok(s)
    }

    fn addr(&self) -> usize {
        self.0.addr()
    }

    /// The sampler id and the slot reference given back, the buffer freed.
    unsafe fn release(self, p: &mut Port) {
        release_sampler(p, self.addr() + 0xea);
        release(p, self.addr());
        self.0.release(p);
    }
}

/// The temporary target group the game builds on its stack (main+0x21f35b0 and the stores after it): the colour target
/// object, the render buffer of the source's size with that target, the viewport over it.
struct Temp(Host);

impl Temp {
    unsafe fn new(p: &mut Port, tw: u16, th: u16) -> Temp {
        let h = Host::new(p, T_BYTES);
        let t = h.addr();
        new_target(p, t + T_TARGET, COLOR_VTABLE);
        new_buffer(p, t + T_RB);
        p.m.w::<u64>(t + T_RB + 0x20, (t + T_TARGET) as u64);
        let (fw, fh) = set_extent(p, t + T_RB, tw, th);
        // The viewport: the region from the origin (the height's side first when the height is 0), the frame's
        // orientation, depth range 0..1.
        let empty = th == 0;
        let vp = t + T_VP;
        p.m.w::<u64>(vp, (p.m.r64(p.base + VIEWPORT_VTABLE) + 0x10) as u64);
        p.m.w::<f32>(vp + 8, 0.0);
        p.m.w::<f32>(vp + 0xc, if empty { fh } else { 0.0 });
        p.m.w::<f32>(vp + 0x10, fw);
        p.m.w::<f32>(vp + 0x14, if empty { 0.0 } else { fh });
        p.m.w::<u32>(vp + 0x18, p.m.r32(p.m.r64(p.base + ORIENTATION)));
        p.m.w::<f32>(vp + 0x1c, 0.0);
        p.m.w::<f32>(vp + 0x20, 1.0);
        Temp(h)
    }

    fn addr(&self) -> usize {
        self.0.addr()
    }

    fn rb(&self) -> usize {
        self.addr() + T_RB
    }

    fn vp(&self) -> usize {
        self.addr() + T_VP
    }
}

/// The uniforms 1 and 2 of a texture size (w, h): (1 / w, 1 / h, 0.5 / w, 0.5 / h) and (0.5 / w, 0.5 / h, 0.15 / w,
/// 0.15 / h), as the game's code computes them.
unsafe fn size_uniforms(p: &Port, w: f32, h: f32) -> ([u32; 4], [u32; 4]) {
    let inv_w = 1.0 / w;
    let inv_h = 1.0 / h;
    let half_w = inv_w * 0.5;
    let half_h = inv_h * 0.5;
    let k = gf(p, K_FIFTEEN);
    (words([inv_w, inv_h, half_w, half_h]), words([half_w, half_h, inv_w * k, inv_h * k]))
}

/// The filter's uniforms 3, 4 and 5 (the pass of the luma target uses them too).
struct Params {
    u3: [u32; 4],
    u4: [u32; 4],
    u5: [u32; 4],
}

unsafe fn params(p: &Port, this: usize) -> Params {
    let u3 = [p.m.r32(this + 0xad0), p.m.r32(this + 0xad4), p.m.r32(this + 0xad8), 0];
    let i = p.m.r32(this + 0x8c8);
    let (mut a, mut b) = if i < 3 { (gf(p, TABLE_A + i as usize * 4), gf(p, TABLE_B + i as usize * 4)) } else { (1.0, 1.0) };
    if p.m.r32(this + 0x8e8) > 4 {
        a *= 1.5;
        b *= 1.5;
    }
    let u4 = [a.to_bits(), b.to_bits(), 0x3f80_0000, 0x3f80_0000];
    let d2 = 1.0 / (p.m.f32(this + 0x948) * 8.0);
    let d3 = p.m.f32(this + 0x968) * gf(p, K_TENTH);
    let u5 = [p.m.r32(this + 0x908), p.m.r32(this + 0x928), d2.to_bits(), d3.to_bits()];
    Params { u3, u4, u5 }
}

/// main+0x21f3690(this, w, tex, size, mode, 0, u4, u5): the luma pass: the program of the holder at this +0x5d8 (0x18
/// each; the second with mode above 4) with uniforms 1 to 5 and 0xb = 0, the source in slot 0, the full-screen triangle.
unsafe fn luma_pass(p: &mut Port, this: usize, w: usize, tex: usize, size: [f32; 2], mode: u32, pr: &Params) -> R<()> {
    let holder = this + (mode > 4) as usize * 0x18 + 0x5d8;
    let prog = p.m.r64(holder + 8);
    program_apply(p, prog, w)?;
    let (u1, u2) = size_uniforms(p, size[0], size[1]);
    set_uniform(p, prog, w, 1, &bytes(u1));
    set_uniform(p, prog, w, 2, &bytes(u2));
    set_uniform(p, prog, w, 3, &bytes(pr.u3));
    set_uniform(p, prog, w, 4, &bytes(pr.u4));
    set_uniform(p, prog, w, 5, &bytes(pr.u5));
    set_uniform(p, prog, w, 0xb, &0f32.to_le_bytes());
    program_texture(p, prog, w, 0, tex)?;
    fullscreen(p, w);
    Ok(())
}

/// The 3x4 product main+0xe99e94(out, a, b): row r of `a` (xyz) times the first three rows of `b`, fused as the vector
/// code does, plus (0, 0, 0, a[r][3]).
fn affine(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut o = [0f32; 16];
    for r in 0..4 {
        for l in 0..4 {
            let mut x = b[l] * a[r * 4];
            x = b[4 + l].mul_add(a[r * 4 + 1], x);
            x = b[8 + l].mul_add(a[r * 4 + 2], x);
            o[r * 4 + l] = if l == 3 { a[r * 4 + 3] + x } else { 0.0 + x };
        }
    }
    o
}

/// The 4x4 product main+0xe99e38(out, a, b).
fn product(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut o = [0f32; 16];
    for r in 0..4 {
        for l in 0..4 {
            let mut x = b[l] * a[r * 4];
            x = b[4 + l].mul_add(a[r * 4 + 1], x);
            x = b[8 + l].mul_add(a[r * 4 + 2], x);
            x = b[12 + l].mul_add(a[r * 4 + 3], x);
            o[r * 4 + l] = x;
        }
    }
    o
}

unsafe fn matrix(p: &Port, at: usize) -> [f32; 16] {
    std::array::from_fn(|k| p.m.f32(at + k * 4))
}

fn matrix_bytes(m: &[f32; 16]) -> [u8; 64] {
    let mut b = [0u8; 64];
    for (k, x) in m.iter().enumerate() {
        b[k * 4..k * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    b
}

/// The source's size as the game reads it: the width (at least 1) and the height (the 2D class's own, at least the
/// state's) from the head (+0x30, +0x32, class +0x3a).
unsafe fn tex_dims(p: &Port, t: usize) -> (u32, u32) {
    let w = (p.m.r16(t + 0x30) as u32).max(1);
    let h = p.m.r16(t + 0x32) as u32;
    let hk = if p.m.r16(t + 0x3a) == 3 { h } else { 1 };
    (w, hk.max(h))
}

/// The copy of the source to ctx[0]: main+0xe97ad8 (the buffer bound, its viewport) and main+0xe97b10 (the shader of
/// ctx[3] applied, the texture quad), then the buffer's unbind.
unsafe fn plain_copy(p: &mut Port, w: usize, ctx: usize) -> R<()> {
    let rb = p.m.r64(ctx);
    let prog = p.m.r64(p.m.r64(ctx + 0x18) + 8);
    context_ok(p, w)?;
    program_ready(p, prog)?;
    unbind_ok(p, rb)?;
    bind_ok(p, rb)?;
    p.m.commit();
    // commit
    targets::bind(p, rb, w)?;
    viewport::apply(p, p.m.r64(ctx + 8), w, rb);
    program_apply(p, prog, w)?;
    let (scale, offset) = (p.m.r64(p.base + QUAD_SCALE), p.m.r64(p.base + QUAD_OFFSET));
    texture_quad(p, w, prog, p.m.r64(ctx + 0x10), p.m.r64(ctx + 8), scale, offset)?;
    targets::unbind(p, w)
}

/// main+0xe97ed4(this, w, view, ctx).
pub(super) unsafe fn fxaa(p: &mut Port, this: usize, w: usize, view: u32, ctx: usize) -> R<()> {
    if p.m.f32(this + 0xae0) <= 0.0 {
        return plain_copy(p, w, ctx);
    }
    let rb0 = p.m.r64(ctx);
    let vp0 = p.m.r64(ctx + 8);
    let tex = p.m.r64(ctx + 0x10);
    if rb0 == 0 || vp0 == 0 || tex == 0 {
        return Err("fxaa context missing");
    }
    let rec = p.m.r64(this + 0xb90).wrapping_add(clamp_index(view, p.m.r32(this + 0xb88)).wrapping_mul(0xb78));
    let (ctx10, ctx12) = (p.m.r64(ctx + 0x50), p.m.r64(ctx + 0x60));
    let reproject = ctx10 != 0 && ctx12 != 0 && p.m.r8(this + 0x988) != 0;
    let (tw, th) = tex_dims(p, tex);
    // The size the pass draws at: the viewport's extent in the buffer's units (swapped for orientations 1 and 2),
    // truncated.
    let f = |a: usize| p.m.f32(a);
    let (dx, dy) = (f(vp0 + 0x10) - f(vp0 + 8), f(vp0 + 0x14) - f(vp0 + 0xc));
    let (ex, ey) = if p.m.r32(vp0 + 0x18).wrapping_sub(1) <= 1 { (dy, dx) } else { (dx, dy) };
    let fx = (ex / f(rb0 + 8)) * (f(rb0 + 0x18) - f(rb0 + 0x10));
    let fy = (ey / f(rb0 + 0xc)) * (f(rb0 + 0x1c) - f(rb0 + 0x14));
    let (mut lw, mut lh) = ((fx as u32) as f32, (fy as u32) as f32);
    let aa8 = p.m.r32(this + 0xaa8);
    let depth0 = p.m.r64(rb0 + 0x60) != 0;
    let temp = p.m.r8(this + 0xb7a) != 0
        && !((fx as u32) == ((tw as f32) as i32 as u32) && (fy as u32) == ((th as f32) as i32 as u32))
        && !(aa8 != 7 && depth0);
    let temp = if temp {
        lw = tw as f32;
        lh = th as f32;
        Some(Temp::new(p, tw as u16, th as u16))
    } else {
        None
    };
    let r = run(p, this, w, ctx, rec, reproject, (lw, lh), temp.as_ref());
    if let Some(t) = temp {
        t.0.release(p);
    }
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn run(p: &mut Port, this: usize, w: usize, ctx: usize, rec: usize, reproject: bool, size: (f32, f32), temp: Option<&Temp>) -> R<()> {
    let (lw, lh) = size;
    let rb0 = p.m.r64(ctx);
    let vp0 = p.m.r64(ctx + 8);
    let tex = p.m.r64(ctx + 0x10);
    let prog = p.m.r64(p.m.r64(ctx + 0x28) + 8);
    let aa8 = p.m.r32(this + 0xaa8);
    let luma_src = p.m.r64(ctx + 0x70);
    let luma_new = p.m.r8(this + 0xb7b) != 0 && luma_src == 0;
    // The checks before the commit point.
    context_ok(p, w)?;
    program_ready(p, prog)?;
    unbind_ok(p, rb0)?;
    bind_ok(p, rb0)?;
    if let Some(t) = temp {
        unbind_ok(p, t.rb())?;
        view_ok(p, t.addr() + T_TARGET)?;
        let f = p.vfn(t.addr() + T_TARGET, 0x10);
        if f != SETUP_COLOR && f != SETUP_DEPTH {
            return Err("target object setup");
        }
        let ctx4 = p.m.r64(ctx + 0x20);
        if ctx4 == 0 || p.m.r64(ctx4 + 8) == 0 || p.m.r64(rb0 + 0x20) == 0 {
            return Err("fxaa copy back missing");
        }
        program_ready(p, p.m.r64(ctx4 + 8))?;
    }
    if luma_new {
        let mode = p.m.r32(this + 0x8e8);
        program_ready(p, p.m.r64(this + (mode > 4) as usize * 0x18 + 0x5e0))?;
        view_ok(p, rec + 0x78)?;
        if p.m.predict() {
            return Err("fxaa luma target allocated by the call");
        }
    }
    if temp.is_some() && p.m.predict() {
        return Err("fxaa temporary target allocated by the call");
    }
    p.m.commit();
    // commit

    // The temporary target: allocated at the source's size and given to the colour target object.
    if let Some(t) = temp {
        let format = p.m.r8(p.m.r64(rb0 + 0x20) + 0xa8) as u32;
        let st = alloc_target(p, w, NAME_TEMP, format, size.0 as u32, size.1 as u32)?;
        p.m.w::<u64>(t.addr(), st as u64);
        copy_target(p, t.addr() + T_TARGET, st)?;
    }
    let (rb_dst, vp_dst) = match temp {
        Some(t) => (t.rb(), t.vp()),
        None => (rb0, vp0),
    };
    // The values the draw uses (the game computes them before the record's copies).
    let (u1, u2) = size_uniforms(p, lw, lh);
    let pr = params(p, this);
    let u6 = 1.0 - p.m.f32(this + 0xae0);
    // The record's source state, then its luma state.
    texture_assign(p, rec + 0x258, tex)?;
    let mut luma = 0usize;
    if p.m.r8(this + 0xb7b) == 0 {
        let d = p.m.r64(p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES)) + 0x18);
        texture_assign(p, rec + 0x348, d)?;
    } else {
        let mut src = luma_src;
        if luma_new {
            src = luma_target(p, this, w, rec, (lw, lh), &pr)?;
            luma = src;
        }
        shadow_mask_setup(p, rec + 0x348, src)?;
    }
    // The buffer drawn into.
    targets::bind(p, rb_dst, w)?;
    viewport::apply(p, vp_dst, w, rb_dst);
    let mut g = [0u32; 0x20];
    let mut u11 = 0u32;
    if aa8 != 7 && p.m.r64(rb_dst + 0x60) != 0 {
        if aa8 < 7 {
            p.m.w::<u8>(rec + 0x95c, aa8 as u8 + 1);
        }
        p.m.w::<u16>(rec + 0x8f8, 1);
        for (k, v) in g.iter_mut().enumerate().take(0x1d) {
            *v = p.m.r32(rec + 0x8f8 + k * 4);
        }
        p.apply_graphics_context(g.as_ptr() as usize, w);
        u11 = p.m.r32(this + 0xac8);
    }
    program_apply(p, prog, w)?;
    let u7 = if p.m.r8(this + 0xb79) != 0 { 0x3ee8_ba2e } else { 0x3f80_0000 };
    if reproject {
        reprojected(p, this, w, rec, prog, (ctx10(p, ctx), ctx12(p, ctx)), [u1, u2], &pr, u7, u11, u6)?;
    } else {
        set_uniform(p, prog, w, 1, &bytes(u1));
        set_uniform(p, prog, w, 2, &bytes(u2));
        set_uniform(p, prog, w, 3, &bytes(pr.u3));
        set_uniform(p, prog, w, 4, &bytes(pr.u4));
        set_uniform(p, prog, w, 5, &bytes(pr.u5));
        set_uniform(p, prog, w, 6, &u6.to_le_bytes());
        set_uniform(p, prog, w, 7, &u7.to_le_bytes());
        set_uniform(p, prog, w, 0xb, &u11.to_le_bytes());
        program_texture(p, prog, w, 0, rec + 0x258)?;
        program_texture(p, prog, w, 1, rec + 0x348)?;
        fullscreen(p, w);
    }
    // The buffer unbound (its virtual +0x28), the luma target given back.
    targets::unbind(p, w)?;
    if luma != 0 {
        gather(p, luma)?;
    }
    if let Some(t) = temp {
        copy_back(p, w, ctx, t)?;
    }
    Ok(())
}

unsafe fn ctx10(p: &Port, ctx: usize) -> usize {
    p.m.r64(ctx + 0x50)
}

unsafe fn ctx12(p: &Port, ctx: usize) -> usize {
    p.m.r64(ctx + 0x60)
}

/// The luma target (this +0xb7b without a luma state in ctx[14]): a "luma" target (format 1, the pass's size, this
/// +0xb78) from the pool given to the record's target object (+0x78, main+0xf817bc), the record's render buffer (+8) of
/// that size bound and drawn by main+0x21f3690 from the record's source state, then unbound and barriered. Returns the
/// target's state.
unsafe fn luma_target(p: &mut Port, this: usize, w: usize, rec: usize, size: (f32, f32), pr: &Params) -> R<usize> {
    let (lw, lh) = size;
    let name: [u64; 2] = [(p.base + NAME_LUMA) as u64, 0];
    let args = [pool(p) as u64, w as u64, name.as_ptr() as u64, 1, lw as u32 as u64, lh as u32 as u64, 1, 0, p.m.r8(this + 0xb78) as u64, 1, 0, 0];
    let st = p.call_stack("fxaa luma target allocated by the call", ALLOC, &args)? as usize;
    let obj = rec + 0x78;
    p.helper("fxaa luma texture by the call", SSAO_ALLOC, &[obj as u64, st as u64])?;
    for o in (0x28..0x70).step_by(8) {
        p.m.w::<u64>(rec + o, 0);
    }
    // The buffer's size and region (the extents' sides by sign).
    let span = |v: f32| if v <= 0.0 { (v, 0.0) } else { (0.0, v) };
    let ((x1, x2), (y1, y2)) = (span(lw), span(lh));
    p.m.w::<f32>(rec + 0x10, lw);
    p.m.w::<f32>(rec + 0x14, lh);
    p.m.w::<f32>(rec + 0x18, x1);
    p.m.w::<f32>(rec + 0x1c, y1);
    p.m.w::<f32>(rec + 0x20, x2);
    p.m.w::<f32>(rec + 0x24, y2);
    p.m.w::<u64>(rec + 0x28, obj as u64);
    let vp = viewport::full(p, lw, lh);
    targets::bind(p, rec + 8, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rec + 8);
    luma_pass(p, this, w, rec + 0x258, [lw, lh], p.m.r32(this + 0x8e8), pr)?;
    targets::unbind(p, w)?;
    barrier_if_slot(p, obj + 0x60, w);
    Ok(st)
}

/// The filter's uniforms and textures with the reprojection (ctx[10] / ctx[12], this +0x988): the record's two matrix
/// pairs (+0x818 / +0x858 and +0x888 / +0x8c8) combined and inverted, the (0.5, -0.5) device matrix, the size of
/// ctx[12] and the filter's scalars.
#[allow(clippy::too_many_arguments)]
unsafe fn reprojected(p: &mut Port, this: usize, w: usize, rec: usize, prog: usize, tex: (usize, usize), u12: [[u32; 4]; 2], pr: &Params, u7: u32, u11: u32, u6: f32) -> R<()> {
    let (t10, t12) = tex;
    let k = gf(p, K_SQRT2) / p.m.f32(this + 0x9a8);
    let (tw, th) = tex_dims(p, t12);
    let (fw, fh) = (tw as f32, th as f32);
    let u0 = words([fw, fh, k * fw, k * fh]);
    let m1 = affine(&matrix(p, rec + 0x818), &matrix(p, rec + 0x858));
    let mut inv = [0f32; 16];
    p.query(p.base + INVERSE_4X4, &[inv.as_mut_ptr() as u64, m1.as_ptr() as u64]);
    let device: [f32; 16] = [0.5, 0.0, 0.0, 0.5, 0.0, -0.5, 0.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    let m2 = affine(&matrix(p, rec + 0x888), &matrix(p, rec + 0x8c8));
    let n = product(&device, &m2);
    let f = product(&n, &inv);
    set_uniform(p, prog, w, 1, &bytes(u12[0]));
    set_uniform(p, prog, w, 2, &bytes(u12[1]));
    set_uniform(p, prog, w, 3, &bytes(pr.u3));
    set_uniform(p, prog, w, 4, &bytes(pr.u4));
    set_uniform(p, prog, w, 5, &bytes(pr.u5));
    set_uniform(p, prog, w, 7, &u7.to_le_bytes());
    set_uniform(p, prog, w, 8, &matrix_bytes(&f));
    set_uniform(p, prog, w, 0, &bytes(u0));
    set_uniform(p, prog, w, 9, &matrix_bytes(&inv)[..16]);
    set_uniform(p, prog, w, 10, &matrix_bytes(&device)[..16]);
    set_uniform(p, prog, w, 0xb, &u11.to_le_bytes());
    set_uniform(p, prog, w, 6, &u6.to_le_bytes());
    program_texture(p, prog, w, 0, rec + 0x258)?;
    program_texture(p, prog, w, 1, rec + 0x348)?;
    program_texture(p, prog, w, 4, t12)?;
    program_texture(p, prog, w, 3, t10)?;
    fullscreen(p, w);
    Ok(())
}

/// The temporary target copied to ctx[0]: a stack texture made from the target's state (main+0x979454), ctx[0] bound
/// with its viewport (main+0xe97ad8), the copy shader of ctx[4] applied and the texture drawn (main+0xe9a108), the
/// buffer unbound, the target given back; the temporary's slot reference, the stack texture's sampler and slot reference
/// released.
unsafe fn copy_back(p: &mut Port, w: usize, ctx: usize, t: &Temp) -> R<()> {
    let st = p.m.r64(t.addr());
    let x = Sampled::new(p, st)?;
    let rb0 = p.m.r64(ctx);
    targets::bind(p, rb0, w)?;
    viewport::apply(p, p.m.r64(ctx + 8), w, rb0);
    let prog = p.m.r64(p.m.r64(ctx + 0x20) + 8);
    program_apply(p, prog, w)?;
    draw_texture(p, w, prog, x.addr())?;
    targets::unbind(p, w)?;
    gather(p, st)?;
    release(p, t.addr() + T_STATE);
    x.release(p);
    Ok(())
}
