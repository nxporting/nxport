// TotK's per-view deferred buffer pass, main+0x96e6fc(this, w, view, ctx): the view's temporary targets from the
// dynamic allocator (Normal, SkyFog, Shadow, Light, QuatNormal, QuatNLD(SSAO)), the view's uniform block, the render
// buffer with the accumulation targets, the optional blur chain, and the stack textures / targets the game builds
// for them as host objects here (byte layout as the game's, only the fields anything reads).
// Applying only: the allocations are past the commit point. Everything that can stop the port is checked before it.

use super::draw::{Port, R};
use super::scene_passes::{default_state, fullscreen, view_fullscreen};
use super::texture_slots::{alloc_target, assign, copy_target, level_size, new_buffer, new_target, release, release_sampler, set_extent, texture_assign, Host};
use super::texture_slots::{BUFFER_BYTES, COLOR_VTABLE, DEPTH_VTABLE, TARGET_BYTES};
use super::vfx::{draw_pass, program_apply, program_block, program_texture, shadow_mask_setup};
use super::{targets, viewport};

/// [[var]]: the temporary target pool; main+0x7f0354(pool, target) gives one back.
const TARGET_POOL: usize = 0x4634e70;
const GATHER: usize = 0x7f0354;
/// [[var]] +0x50c bit 1: graphics settings (barrier bits).
const SETTINGS: usize = 0x462bbf0;
/// [var]: the 16-byte clear colour.
const CLEAR_COLOR: usize = 0x4634ea0;
/// [[[var]]+0x18]: the default texture state.
const DEFAULT_TEXTURES: usize = 0x4631740;
/// [var] a key and main+0x2b15be0(key) / main+0x2b15c10(): the index of the thread's light target pool entry.
const LIGHT_KEY: usize = 0x462af70;
const THREAD_VALUE: usize = 0x2b15be0;
const THREAD_FALLBACK: usize = 0x2b15c10;
/// main+0xa51ad4(pool, w, pool entry, name, 0x1a, 4, width, height, 1, 0, 2, 0, 1, _, 0, 0): the light target.
const ALLOC_LIGHT: usize = 0xa51ad4;

/// Target-object / texture helpers the game calls.
const SCENE_BLOCKS: usize = 0x972c48;
const LIGHTING: usize = 0x973b40;
const VIEW_PRE: usize = 0x972b10;
const VIEW_POST: usize = 0x9733fc;
const VIEW_LAST: usize = 0x9735e0;

/// Virtual functions the port stands in for.
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;
const BLOCK_NOOP: usize = 0x2a303a8;

/// Pool flag query / flush (GOT cells of nvnMemoryPoolGetFlags / nvnMemoryPoolFlushMappedRange).
const POOL_GET_FLAGS: usize = 0x4613b40;
const POOL_FLUSH: usize = 0x4614018;

/// Names of the temporary targets and of the stack textures.
const NORMAL_NAME: usize = 0x37d6eaa;
const SKYFOG_NAME: usize = 0x37b14bb;
const SHADOW_NAME: usize = 0x37b14c2;
const LIGHT_NAME: usize = 0x3766cf0;
const QUAT_NORMAL_NAME: usize = 0x3766cf6;
const QUAT_NLD_NAME: usize = 0x370432e;
const SHADOW_BLUR_NAME: usize = 0x36da8bd;
const SSR_BLUR_NAME: usize = 0x37b931f;
const TEXTURE_NAME: usize = 0x37ccf1f;
/// "agl::TextureSampler" (the texture constructor main+0xbf2180) and "agl::TextureSampler(from texture)" (the inline
/// copies of the blur chain).
const SAMPLER_NAME: usize = 0x378aff3;
const SAMPLER_FROM_TEXTURE_NAME: usize = 0x37a908e;

const ONE: [u32; 4] = [0x3f80_0000; 4];
const ZERO: [u32; 4] = [0; 4];
const BLACK: [u32; 4] = [0, 0, 0, 0x3f80_0000];

/// The view's fields the stages share.
#[derive(Clone, Copy)]
struct C {
    this: usize,
    w: usize,
    view: u32,
    ctx: usize,
    /// ctx[5] +0x1ec0: the scene (its view records at +0x10, 0x21b0 each).
    scene: usize,
    /// This's record of the view (this +0x18, 0xb90 each).
    r: usize,
    /// The scene's record of the view.
    v: usize,
    /// [v +0x2118]: the record's texture set.
    t: usize,
    /// v +0x1670 / +0x1490: texture states.
    vstate: usize,
    vdepth: usize,
    width: u32,
    height: u32,
    f8: u32,
    f9: u8,
    f10: u8,
    pool: usize,
}

/// The host objects of the pass.
struct Objects {
    /// Render buffers: the Normal pass, the QuatNormal pass, the accumulation buffer.
    rb1: Host,
    rb2: Host,
    rb3: Host,
    /// Colour targets built from QuatNormal, QuatNLD and Light; the depth target.
    t1: Host,
    t2: Host,
    t3: Host,
    td: Host,
    /// Stack textures bound to the passes (QuatNormal, QuatNLD).
    sa: Host,
    sb: Host,
    /// The blur chain's colour targets and sampled textures.
    bt1: Host,
    bt2: Host,
    q1: Host,
    q2: Host,
}

impl Objects {
    unsafe fn new(p: &mut Port) -> Objects {
        let buffer = |p: &mut Port| {
            let h = Host::new(p, BUFFER_BYTES);
            new_buffer(p, h.addr());
            h
        };
        let (rb1, rb2, rb3) = (buffer(p), buffer(p), buffer(p));
        let target = |p: &mut Port, vtable: usize| {
            let h = Host::new(p, TARGET_BYTES);
            new_target(p, h.addr(), vtable);
            h
        };
        let (t1, t2, t3) = (target(p, COLOR_VTABLE), target(p, COLOR_VTABLE), target(p, COLOR_VTABLE));
        let td = target(p, DEPTH_VTABLE);
        let (bt1, bt2) = (target(p, COLOR_VTABLE), target(p, COLOR_VTABLE));
        let (sa, sb, q1, q2) = (Host::new(p, 0x100), Host::new(p, 0x100), Host::new(p, 0x100), Host::new(p, 0x100));
        Objects { rb1, rb2, rb3, t1, t2, t3, td, sa, sb, bt1, bt2, q1, q2 }
    }

    unsafe fn free(self, p: &mut Port) {
        for h in [self.rb1, self.rb2, self.rb3, self.t1, self.t2, self.t3, self.td, self.sa, self.sb, self.bt1, self.bt2, self.q1, self.q2] {
            h.release(p);
        }
    }
}

/// A program of a program set (+0x10 the first, +0x20 the table of 0x158-byte entries, count +0x18) by the variant
/// index (0 = the first, else entry index - 1, out of range = the first entry).
unsafe fn variant(p: &Port, set: usize, idx: u32) -> usize {
    if idx == 0 {
        return p.m.r64(set + 0x10);
    }
    let mut i = idx - 1;
    if p.m.r32(set + 0x18) <= i {
        i = 0;
    }
    p.m.r64(set + 0x20) + i as usize * 0x158
}

unsafe fn program_ready(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// copy_target accepts the target object's setup virtual.
unsafe fn setup_known(p: &Port, obj: usize) -> R<()> {
    match p.vfn(obj, 0x10) {
        SETUP_COLOR | SETUP_DEPTH => Ok(()),
        _ => Err("target object setup"),
    }
}

unsafe fn unbind_known(p: &Port, rb: usize) -> R<()> {
    if p.vfn(rb, 0x28) != targets::RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    Ok(())
}

/// The clear values of a buffer bind: eight colours and depth 1.0 (stencil 0).
fn values(colors: [[u32; 4]; 8]) -> [u32; 0x22] {
    let mut v = [0u32; 0x22];
    for (k, c) in colors.iter().enumerate() {
        v[k * 4..k * 4 + 4].copy_from_slice(c);
    }
    v[0x20] = 1f32.to_bits();
    v
}

unsafe fn clear_color(p: &Port) -> [u32; 4] {
    let c = p.pfn(CLEAR_COLOR);
    std::array::from_fn(|k| p.m.r32(c + k * 4))
}

/// Graphics settings +0x50c bit 1.
unsafe fn settings_bit(p: &Port) -> bool {
    (p.m.r8(p.m.r64(p.m.r64(p.base + SETTINGS)) + 0x50c) >> 1) & 1 != 0
}

/// The texture object constants the game's constructor (main+0xbf2180) and the inline copies write; the state is
/// assigned after.
unsafe fn sampler_constants(p: &mut Port, s: usize, name: usize) {
    p.m.w::<u32>(s + 0x58, 0xffff_ffff);
    p.m.w::<u16>(s + 0xa1, 1);
    p.m.w::<u64>(s + 0xa8, (p.base + TEXTURE_NAME) as u64);
    p.m.w::<u64>(s + 0xb0, (p.base + name) as u64);
    p.m.w::<u64>(s + 0xb8, 0);
    p.m.w::<u64>(s + 0xc0, 0x3f80_0000_0000_0000);
    p.m.w::<u64>(s + 0xc8, 0x4170_0000_0000_0000);
    p.m.w::<u32>(s + 0xd0, 0);
    p.m.w::<u64>(s + 0xd4, 0x0101_0707_0702_0101);
    p.m.w::<u8>(s + 0xdc, 0);
    p.m.w::<u16>(s + 0xde, 0);
    p.m.w::<u64>(s + 0xe0, 0xff_0504_0302);
    p.m.w::<u8>(s + 0xe8, 0x20);
    p.m.w::<u16>(s + 0xea, 0xffff);
}

/// main+0xbf2180(s): the constants, and with the default texture holder its state assigned.
unsafe fn sampler_default(p: &mut Port, s: usize) -> R<()> {
    sampler_constants(p, s, SAMPLER_NAME);
    let holder = p.m.r64(p.m.r64(p.base + DEFAULT_TEXTURES));
    if holder != 0 {
        texture_assign(p, s, p.m.r64(holder + 0x18))?;
    }
    Ok(())
}

/// The stack texture's settings the pass changes after its constructor: +0xd4 (2 bytes) / +0xd6 cleared, the finish
/// flag +0xe4 bit 2.
unsafe fn sampler_flags(p: &mut Port, s: usize) {
    p.m.w::<u16>(s + 0xd4, 0);
    p.m.w::<u8>(s + 0xd6, 0);
    p.m.fetch_or32(s + 0xe4, 4);
}

/// A stack texture `s` made from state `src` the way the blur chain inlines it.
unsafe fn sampled_copy(p: &mut Port, s: usize, src: usize) -> R<()> {
    sampler_constants(p, s, SAMPLER_FROM_TEXTURE_NAME);
    texture_assign(p, s, src)
}

/// main+0x96e6fc(this, w, view, ctx). Applying only.
pub(super) unsafe fn deferred_view_pass(p: &mut Port, this: usize, w: usize, view: u32, ctx: usize) -> R<()> {
    let scene = p.m.r64(p.m.r64(ctx + 0x28) + 0x1ec0);
    let recs = p.m.r64(this + 0x18);
    let rv = p.m.r64(scene + 0x10);
    let i28 = (if p.m.r32(this + 0x10) > view { view } else { 0 }) as i32 as isize as usize;
    let i34 = (if p.m.r32(scene + 8) > view { view } else { 0 }) as i32 as isize as usize;
    let r = recs.wrapping_add(i28.wrapping_mul(0xb90));
    let v = rv.wrapping_add(i34.wrapping_mul(0x21b0));
    let h = p.m.r16(v + 0x16a2);
    let c = C {
        this,
        w,
        view,
        ctx,
        scene,
        r,
        v,
        t: p.m.r64(v + 0x2118),
        vstate: v + 0x1670,
        vdepth: v + 0x1490,
        width: p.m.r16(v + 0x16a0).max(1) as u32,
        height: (if p.m.r16(v + 0x16aa) != 3 { 1 } else { h }).max(h) as u32,
        f8: p.m.r32(this + 8),
        f9: p.m.r8(this + 9),
        f10: p.m.r8(this + 10),
        pool: p.m.r64(p.m.r64(p.base + TARGET_POOL)),
    };
    let plan = check(p, &c)?;
    let o = Objects::new(p);
    let r = match check_objects(p, &o) {
        Ok(()) => stages(p, &c, &o, &plan),
        Err(e) => Err(e),
    };
    o.free(p);
    r
}

/// What the checks worked out for the stages.
struct Plan {
    /// The programs of the buffer passes (set +8 of this +0xb0): the main pass, the blur passes, the last pass.
    set2: usize,
    a: u32,
    b: u32,
    c: u32,
    /// The view's uniform block ring: the address of this frame's copy and the alignment of its elements.
    ring: usize,
    nibble: u32,
    align: (i32, i32),
}

/// Everything that can stop the port, read before the commit point: a buffer still bound with an unknown unbind,
/// programs still being set up, unknown virtual functions, the uniform block's memory.
unsafe fn check(p: &mut Port, c: &C) -> R<Plan> {
    let prev = p.m.r64(c.w + 0xf8);
    if (p.m.r8(c.w + 0x108) >> 1) & 1 != 0 && prev != 0 && p.vfn(prev, 0x28) != targets::RENDER_BUFFER_UNBIND {
        return Err("render buffer unbind");
    }
    if p.m.r8(p.m.r64(p.base + targets::CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    let set1 = p.m.r64(p.m.r64(c.this + 0xa8) + 8);
    let idx1 = p.m.r16(p.m.r64(set1 + 0x30) + 0x20) as u32;
    if c.f9 & 0x20 != 0 {
        program_ready(p, variant(p, set1, idx1))?;
        setup_known(p, c.r + 0x8a8)?;
    }
    if c.f9 & 0x40 != 0 {
        program_ready(p, variant(p, set1, idx1 * 2))?;
    }
    for k in 0..4 {
        if k < 3 || p.m.r64(c.r + 0x10 + k * 0x1e8) != 0 {
            setup_known(p, c.r + 0x108 + k * 0x1e8)?;
        }
    }
    let ps = p.m.r64(c.this + 0xb0);
    let set2 = p.m.r64(ps + 8);
    let table = p.m.r64(set2 + 0x30);
    let a = if (c.f8 >> 9) & 1 != 0 { p.m.r16(table + ((p.m.r32(set2 + 0x28) > 1) as usize) * 0x28 + 0x20) as u32 } else { 0 };
    let b = if (c.f8 >> 0x19) & 1 != 0 { p.m.r16(table + (if p.m.r32(set2 + 0x28) < 3 { 0 } else { 0x50 }) + 0x20) as u32 } else { 0 };
    let cc = p.m.r16(table + 0x20) as u32;
    let base = a + b;
    program_ready(p, variant(p, set2, base))?;
    if c.f8 & 2 != 0 {
        program_ready(p, variant(p, set2, base + cc * 2))?;
        program_ready(p, variant(p, set2, base + cc * 3))?;
        let sub = p.m.r64(c.ctx + 0x48);
        program_ready(p, p.m.r64(p.m.r64(p.m.r64(sub + 0x368) + 8) + 0x10))?;
    }
    if (c.f8 >> 0xb) & 1 != 0 {
        program_ready(p, variant(p, set2, base + cc))?;
    }
    // The uniform block of the view's record: the alignment virtuals and the ring address of the flipped half.
    let blk = c.r + 0xaf8;
    let align = (p.block_alignment(blk)?, p.block_alignment(blk)?);
    if p.vfn(blk, 0x38) != BLOCK_NOOP {
        return Err("uniform block virtual function");
    }
    let nibble = 1u32.wrapping_sub((p.m.r8(c.r + 0xb3a) >> 4) as u32);
    let mem = p.m.r64(c.r + 0xb28);
    let mut ring = 0usize;
    if mem != 0 {
        let a1 = p.m.r64(mem + 0x10);
        if a1 != 0 {
            let a2 = p.m.r64(a1 + 0x100);
            if a2 != 0 {
                let stride = p.m.r32(c.r + 0xb30).wrapping_add(align.0 as u32).wrapping_sub(1) & (align.1 as u32).wrapping_neg();
                let off = (nibble & 0xf).wrapping_mul(p.m.r16(c.r + 0xb38) as u32).wrapping_mul(stride).wrapping_add(p.m.r32(c.r + 0xb18));
                ring = a2.wrapping_add(off as usize);
            }
        }
    }
    if ring == 0 || p.m.r64(c.r + 0xb08) == 0 || p.m.r64(p.m.r64(c.r + 0xb08)) == 0 {
        return Err("view block memory missing");
    }
    Ok(Plan { set2, a, b, c: cc, ring, nibble, align })
}

unsafe fn check_objects(p: &mut Port, o: &Objects) -> R<()> {
    for rb in [&o.rb1, &o.rb2, &o.rb3] {
        unbind_known(p, rb.addr())?;
    }
    for t in [&o.t1, &o.t2, &o.t3, &o.td, &o.bt1, &o.bt2] {
        setup_known(p, t.addr())?;
    }
    Ok(())
}

unsafe fn stages(p: &mut Port, c: &C, o: &Objects, plan: &Plan) -> R<()> {
    if p.m.predict() {
        return Err("deferred buffer pass allocates targets by the call");
    }
    // commit
    p.m.commit();
    normal_buffer(p, c, o)?;
    // The stack textures of the passes.
    sampler_default(p, o.sa.addr())?;
    sampler_default(p, o.sb.addr())?;
    sampler_flags(p, o.sa.addr());
    if (c.f10 >> 4) & 1 == 0 {
        sampler_flags(p, o.sb.addr());
    }
    let t = temporaries(p, c, o)?;
    view_block(p, c, plan);
    entries(p, c)?;
    accumulate(p, c, o, plan, &t)?;
    finish(p, c, o, &t)
}

/// The temporary targets of the view.
struct Temps {
    quat_normal: usize,
    quat_nld: usize,
    light: usize,
}

/// With this +0x9 bit 5 the "Normal" target (format 0x1d, size of the scene record's +0xd80) kept in the view's record
/// (+0x7b0), its state copied to the target object +0x8a8 and the texture +0x7b8, and drawn as a full-screen pass of
/// the program set [this+0xa8]+8 into a buffer of it (clear colour in all eight, depth 1) with the texture +0x110 of
/// the view's set in slot 6; without it the offscreen buffer setup of +0x7b8 from there.
unsafe fn normal_buffer(p: &mut Port, c: &C, o: &Objects) -> R<()> {
    if c.f9 & 0x20 == 0 {
        return shadow_mask_setup(p, c.r + 0x7b8, c.t + 0x110);
    }
    let w1 = p.m.r16(c.v + 0xd80).max(1) as u32;
    let h = p.m.r16(c.v + 0xd82);
    let h1 = (if p.m.r16(c.v + 0xd8a) != 3 { 1 } else { h }).max(h) as u32;
    let obj = alloc_target(p, c.w, NORMAL_NAME, 0x1d, w1, h1)?;
    p.m.w::<u64>(c.r + 0x7b0, obj as u64);
    let tobj = c.r + 0x8a8;
    copy_target(p, tobj, obj)?;
    assign(p, c.r + 0x7b8, obj)?;
    p.m.fetch_or32(c.r + 0x89c, 1);

    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    p.apply_graphics_context(g.as_ptr() as usize, c.w);
    let rb = o.rb1.addr();
    p.m.w::<u64>(rb + 0x20, tobj as u64);
    let (tw, th) = level_size(p, tobj, false);
    let (fw, fh) = set_extent(p, rb, tw, th);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, c.w, rb);
    let cc = clear_color(p);
    let vals = values([cc; 8]);
    targets::bind_clear(p, rb, c.w, vp.as_ptr() as usize, 0, vals.as_ptr() as usize)?;
    let set1 = p.m.r64(p.m.r64(c.this + 0xa8) + 8);
    let prog = variant(p, set1, p.m.r16(p.m.r64(set1 + 0x30) + 0x20) as u32);
    program_apply(p, prog, c.w)?;
    program_texture(p, prog, c.w, 6, c.t + 0x110)?;
    fullscreen(p, c.w);
    targets::unbind(p, c.w)
}

/// The temporaries SkyFog (+0x3e0), Shadow (+0x10), Light (+0x1f8, per-thread entry of the pool), QuatNormal and
/// QuatNLD(SSAO) (format of the scene record's +0x1710); the QuatNormal colour target object; then with this +0x9
/// bit 6 the QuatNormal pass, else the offscreen setups of the two stack textures.
unsafe fn temporaries(p: &mut Port, c: &C, o: &Objects) -> R<Temps> {
    let sky = alloc_target(p, c.w, SKYFOG_NAME, 0x1d, c.width, c.height)?;
    p.m.w::<u64>(c.r + 0x3e0, sky as u64);
    let shadow = alloc_target(p, c.w, SHADOW_NAME, 0x1d, c.width, c.height)?;
    p.m.w::<u64>(c.r + 0x10, shadow as u64);
    let key = p.m.r32(p.m.r64(p.base + LIGHT_KEY)) as u64;
    let tls = p.query(p.base + THREAD_VALUE, &[key]);
    let slot = if tls == 0 { p.query(p.base + THREAD_FALLBACK, &[]) as u32 } else { (tls as u32).wrapping_sub(1) };
    let entry = c.pool.wrapping_add((slot as i32 as isize as usize).wrapping_mul(0x280)).wrapping_add(0x10);
    let name: [u64; 2] = [(p.base + LIGHT_NAME) as u64, 0];
    let args: [u64; 16] = [c.pool as u64, c.w as u64, entry as u64, name.as_ptr() as u64, 0x1a, 4, c.width as u64, c.height as u64, 1, 0, 2, 0, 1, 0, 0, 0];
    let light = p.call_stack("light target allocation", ALLOC_LIGHT, &args)? as usize;
    p.m.w::<u64>(c.r + 0x1f8, light as u64);
    let quat_normal = alloc_target(p, c.w, QUAT_NORMAL_NAME, 0x1d, c.width, c.height)?;
    let quat_nld = alloc_target(p, c.w, QUAT_NLD_NAME, p.m.r8(c.v + 0x1710) as u32, c.width, c.height)?;

    let t1 = o.t1.addr();
    copy_target(p, t1, quat_normal)?;
    if c.f9 & 0x40 == 0 {
        shadow_mask_setup(p, o.sb.addr(), c.vstate)?;
        shadow_mask_setup(p, o.sa.addr(), c.r + 0x7b8)?;
    } else {
        copy_target(p, o.t2.addr(), quat_nld)?;
        quat_pass(p, c, o)?;
        release(p, o.t2.addr() + 8);
        texture_assign(p, o.sb.addr(), quat_nld)?;
        texture_assign(p, o.sa.addr(), quat_normal)?;
    }
    Ok(Temps { quat_normal, quat_nld, light })
}

/// The pass into QuatNormal and QuatNLD (this +0x9 bit 6): the program the set's second variant picks, the scene block
/// ctx[6] (+0x860 from three views on) +0x390, textures 6 = the record's Normal texture (+0x7b8) and 0 = the scene
/// record's state, full-screen; colour mask 0xff, targets cleared to (0, 0, 0, 1).
unsafe fn quat_pass(p: &mut Port, c: &C, o: &Objects) -> R<()> {
    let rb = o.rb2.addr();
    let (t1, t2) = (o.t1.addr(), o.t2.addr());
    p.m.w::<u64>(rb + 0x20, t1 as u64);
    p.m.w::<u64>(rb + 0x28, t2 as u64);
    let (tw, th) = level_size(p, t1, true);
    let (fw, fh) = set_extent(p, rb, tw, th);
    let vp = viewport::full(p, fw, fh);
    viewport::apply(p, vp.as_ptr() as usize, c.w, rb);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = 0xff;
    p.apply_graphics_context(g.as_ptr() as usize, c.w);
    let vals = values([BLACK; 8]);
    targets::bind_clear(p, rb, c.w, vp.as_ptr() as usize, 0, vals.as_ptr() as usize)?;
    let set1 = p.m.r64(p.m.r64(c.this + 0xa8) + 8);
    let prog = variant(p, set1, p.m.r16(p.m.r64(set1 + 0x30) + 0x20) as u32 * 2);
    program_apply(p, prog, c.w)?;
    let sc = p.m.r64(c.ctx + 0x30);
    let off = if p.m.r32(sc + 0x10) < 3 { 0 } else { 0x860 };
    let blk = p.m.r64(sc + 0x18) + off;
    p.m.w::<u8>(blk + 0x421, p.m.r8(blk + 0x421) | 1);
    program_block(p, prog, c.w, 2, blk + 0x390, 0)?;
    program_texture(p, prog, c.w, 6, c.r + 0x7b8)?;
    program_texture(p, prog, c.w, 0, c.vstate)?;
    fullscreen(p, c.w);
    targets::unbind(p, c.w)
}

/// The view's uniform block (record +0xaf8, memory +0xb28, ring half in the high nibble of +0xb3a): the half flipped,
/// the element's address stored (+0xb10), ten words written (1.0, this +0xbc, 800, 500, the flag +0x15 as 1.0 / 0, 1.0,
/// 1 - this +0xcc, this +0xd0, two values this +0xc0 / +0xc4 / +0xc8 pick by this +0x8 bits 10 and 24), and the range
/// flushed when the pool is mapped.
unsafe fn view_block(p: &mut Port, c: &C, plan: &Plan) {
    let r = c.r;
    let old = p.m.r8(r + 0xb3a);
    p.m.w::<u8>(r + 0xb3a, (old & 0xf) | (((plan.nibble & 0xf) as u8) << 4));
    p.m.w::<u64>(r + 0xb10, plan.ring as u64);
    let one = 0x3f80_0000u32;
    let dst = plan.ring + p.m.r32(p.m.r64(p.m.r64(r + 0xb08)) + 4) as usize;
    let f8 = c.f8;
    let f = |p: &Port, o: usize| p.m.r32(c.this + o);
    let (s8, s10) = if f8 & 0x100_0000 != 0 { (f(p, 0xc4), f(p, 0xc0)) } else { (f(p, 0xc8), f(p, 0xc8)) };
    let one_minus = (1.0f32 - p.m.f32(c.this + 0xcc)).to_bits();
    let flag = if (f8 >> 0x15) & 1 != 0 { one } else { 0 };
    let bit10 = f8 & 0x400 != 0;
    let words: [u32; 10] = [one, f(p, 0xbc), 800f32.to_bits(), 500f32.to_bits(), flag, one, one_minus, f(p, 0xd0), if bit10 { s10 } else { one }, if bit10 { s8 } else { one }];
    for (k, v) in words.iter().enumerate() {
        p.m.w::<u32>(dst + k * 4, *v);
    }
    p.m.w::<u64>(dst + 0x28, 0);
    // The range flush of the mapped pool.
    let pool = p.m.r64(r + 0xb20);
    if pool != 0 {
        let flags = p.query(p.m.r64(p.m.r64(p.base + POOL_GET_FLAGS)), &[pool as u64]) as u32;
        if (flags >> 2) & 1 != 0 {
            let size = p.m.r32(r + 0xb30);
            let stride = size.wrapping_add(plan.align.0 as u32).wrapping_sub(1) & (plan.align.1 as u32).wrapping_neg();
            let off = (p.m.r16(r + 0xb38) as u32).wrapping_mul(plan.nibble & 0xf).wrapping_mul(stride).wrapping_add(p.m.r32(r + 0xb18));
            let start = off as u64 & 0xffff_ffc0;
            let len = ((size as u64 + off as u64 + 0x3f) & 0x3_ffff_ffc0) - start;
            p.call(p.m.r64(p.m.r64(p.base + POOL_FLUSH)), &[pool as u64, start, len]);
        }
    }
}

/// The view's record entries 0..3 (+0x10 + k * 0x1e8: the temporary target kept there): its state copied to the
/// texture at +0x18 (finish flag +0xe4 bit 0) and to the target object at +0x108 (setup flags).
unsafe fn entries(p: &mut Port, c: &C) -> R<()> {
    for k in 0..4usize {
        let src = p.m.r64(c.r + 0x10 + k * 0x1e8);
        if src == 0 {
            continue;
        }
        texture_assign(p, c.r + 0x18 + k * 0x1e8, src)?;
        copy_target(p, c.r + 0x108 + k * 0x1e8, src)?;
    }
    Ok(())
}

/// The accumulation buffer: colour targets 0..2 the record's entries (+0x108, +0x2f0, +0x4d8), 4 = the Light target
/// (layer 1), 5 = QuatNormal, depth = the scene's depth state (+0x1490); the passes into it.
unsafe fn accumulate(p: &mut Port, c: &C, o: &Objects, plan: &Plan, t: &Temps) -> R<()> {
    let rb = o.rb3.addr();
    let (t1, t3, td) = (o.t1.addr(), o.t3.addr(), o.td.addr());
    let entry: [usize; 3] = [c.r + 0x108, c.r + 0x2f0, c.r + 0x4d8];
    let sets = |p: &mut Port| {
        for (k, e) in entry.iter().enumerate() {
            p.m.w::<u64>(rb + 0x20 + k * 8, *e as u64);
        }
        p.m.w::<u64>(rb + 0x40, t3 as u64);
        p.m.w::<u64>(rb + 0x48, t1 as u64);
        p.m.w::<u64>(rb + 0x60, td as u64);
    };
    let cc = plan.c;
    let base = plan.a + plan.b;
    let bit0 = (c.f10 & 1) as u32;

    let mut g = default_state(p);
    if bit0 != 0 {
        g[0x19] = (g[0x19] & !0xff) | 6;
    }
    g[0] = (g[0] & 0xffff_0000) | bit0;
    g[1] = 0;
    copy_target(p, td, c.vdepth)?;
    let (dw, dh) = level_size(p, td, true);
    let (fw, fh) = set_extent(p, rb, dw, dh);
    g[7] = 0xfff;
    copy_target(p, t3, t.light)?;
    if p.m.r16(t3 + 0xcc) != 1 {
        p.m.fetch_or32(t3 + 0xc8, 2);
        p.m.w::<u16>(t3 + 0xcc, 1);
    }
    sets(p);
    g[7] = (g[7] & 0xff00_ffff) | if (c.f8 >> 15) & 1 != 0 { 0xf0_0000 } else { 0 } | 0xf_0000;
    let vp = viewport::full(p, fw, fh);
    let vpp = vp.as_ptr() as usize;
    viewport::apply(p, vpp, c.w, rb);
    let cc16 = clear_color(p);
    let vals = values([ONE, ZERO, ONE, cc16, ZERO, cc16, cc16, cc16]);
    let mask = if bit0 == 0 { 0x12 } else { 0x17 };
    p.apply_graphics_context(g.as_ptr() as usize, c.w);
    targets::bind_clear(p, rb, c.w, vpp, mask, vals.as_ptr() as usize)?;
    let prog = variant(p, plan.set2, base);
    program_apply(p, prog, c.w)?;
    super::view_callees::ctx_blocks(p, c.this, c.w, c.view, prog, c.ctx)?;
    program_texture(p, prog, c.w, 6, o.sa.addr())?;
    program_texture(p, prog, c.w, 1, o.sb.addr())?;
    fullscreen(p, c.w);
    targets::unbind(p, c.w)?;
    if t.quat_nld != 0 {
        p.helper("depth of field gather", GATHER, &[c.pool as u64, t.quat_nld as u64])?;
    }
    let args: [u64; 5] = [p.m.r64(c.ctx + 0x28) as u64, p.m.r64(c.ctx + 0x30) as u64, p.m.r64(c.ctx + 0x48) as u64, p.m.r64(c.ctx + 0x50) as u64, c.this as u64];
    p.helper("lighting pass main+0x973b40", LIGHTING, &[p.m.r64(c.ctx + 0x58) as u64, c.w as u64, c.view as u64, args.as_ptr() as u64])?;
    super::view_callees::view_pre(p, p.m.r64(c.ctx + 0x48), c.w, c.view, p.m.r64(c.ctx + 0x30))?;
    if c.f8 & 2 != 0 {
        blur(p, c, o, plan, &mut g, vpp, sets)?;
    }
    let bits = if settings_bit(p) { 0x112 } else { 0x12 };
    let cmd = p.m.r64(c.w + 0xb8);
    p.barrier(cmd, bits);
    if (c.f8 >> 0xb) & 1 != 0 {
        g[0] &= 0xffff_0000;
        g[7] = 0xf0;
        p.apply_graphics_context(g.as_ptr() as usize, c.w);
        let vals = values([cc16; 8]);
        targets::bind_clear(p, rb, c.w, vpp, 0, vals.as_ptr() as usize)?;
        let prog = variant(p, plan.set2, base + cc);
        program_apply(p, prog, c.w)?;
        super::view_callees::ctx_blocks(p, c.this, c.w, c.view, prog, c.ctx)?;
        program_texture(p, prog, c.w, 6, o.sa.addr())?;
        // main+0x979918: the full-screen triangle.
        fullscreen(p, c.w);
        targets::unbind(p, c.w)?;
    }
    if (c.f8 >> 0xc) & 1 != 0 {
        p.m.w::<u64>(rb + 0x20, entry[1] as u64);
        targets::bind(p, rb, c.w)?;
        let size: [f32; 2] = [c.width as f32, c.height as f32];
        super::view_callees::view_last(p, p.m.r64(c.ctx + 0x40), c.w, c.view, size.as_ptr() as usize, o.sa.addr(), c.vstate)?;
        targets::unbind(p, c.w)?;
    }
    Ok(())
}

/// The blur chain (this +0x8 bit 1): ShadowBlur and SSRBlur targets from the allocator, drawn into slots 0 and 4 of the
/// buffer from the textures of the view's entries 0 / 1 (clears by this +0x8 bit 16), the view's full-screen pass, the
/// post pass main+0x9733fc, the blurred targets as stack textures 9 / 10 of the combine pass, and the effect pass 0x1a
/// into the buffer without depth.
#[allow(clippy::too_many_arguments)]
unsafe fn blur(p: &mut Port, c: &C, o: &Objects, plan: &Plan, g: &mut [u32; 0x20], vpp: usize, sets: impl Fn(&mut Port)) -> R<()> {
    let rb = o.rb3.addr();
    let (bt1, bt2, q1, q2) = (o.bt1.addr(), o.bt2.addr(), o.q1.addr(), o.q2.addr());
    let shadow_blur = alloc_target(p, c.w, SHADOW_BLUR_NAME, 0x1d, c.width, c.height)?;
    let ssr_blur = alloc_target(p, c.w, SSR_BLUR_NAME, 0x1a, c.width, c.height)?;
    let bits = if settings_bit(p) { 0x112 } else { 0x12 };
    let cmd = p.m.r64(c.w + 0xb8);
    p.barrier(cmd, bits);
    copy_target(p, bt1, shadow_blur)?;
    copy_target(p, bt2, ssr_blur)?;
    g[1] = 0;
    g[7] = 0xf_000f;
    p.m.w::<u64>(rb + 0x20, bt1 as u64);
    p.m.w::<u64>(rb + 0x40, bt2 as u64);
    p.apply_graphics_context(g.as_ptr() as usize, c.w);
    let cc16 = clear_color(p);
    let vals = values([ONE, ZERO, cc16, cc16, cc16, cc16, cc16, cc16]);
    viewport::apply(p, vpp, c.w, rb);
    let mask = if (p.m.r32(c.this + 8) as i32) << 15 >> 31 != 0 { 0x11 } else { 0 };
    targets::bind_clear(p, rb, c.w, vpp, mask, vals.as_ptr() as usize)?;
    let base = plan.a + plan.b;
    let prog = variant(p, plan.set2, base + plan.c * 2);
    program_apply(p, prog, c.w)?;
    program_texture(p, prog, c.w, 9, c.r + 0x18)?;
    program_texture(p, prog, c.w, 10, c.r + 0x200)?;
    fullscreen(p, c.w);
    targets::unbind(p, c.w)?;
    release(p, bt2 + 8);
    release(p, bt1 + 8);
    let sub = p.m.r64(c.ctx + 0x48);
    let scene = p.m.r64(c.ctx + 0x30);
    view_fullscreen(p, sub, c.w, c.view, scene)?;
    let bits = if settings_bit(p) { 0x111 } else { 0x11 };
    let cmd = p.m.r64(c.w + 0xb8);
    p.barrier(cmd, bits);
    super::view_callees::view_post(p, sub, c.w, scene, 0)?;

    g[0] = (g[0] & 0xffff_0000) | ((c.f8 >> 0x11) & 1);
    g[7] = if c.f8 & 0x200 != 0 { 0xf_06ff } else { 0x6ff };
    sets(p);
    p.apply_graphics_context(g.as_ptr() as usize, c.w);
    sampled_copy(p, q1, shadow_blur)?;
    sampled_copy(p, q2, ssr_blur)?;
    viewport::apply(p, vpp, c.w, rb);
    targets::bind(p, rb, c.w)?;
    let prog = variant(p, plan.set2, base + plan.c * 3);
    program_apply(p, prog, c.w)?;
    super::view_callees::ctx_blocks(p, c.this, c.w, c.view, prog, c.ctx)?;
    program_texture(p, prog, c.w, 9, q1)?;
    program_texture(p, prog, c.w, 10, q2)?;
    program_texture(p, prog, c.w, 6, o.sa.addr())?;
    fullscreen(p, c.w);
    targets::unbind(p, c.w)?;
    release_sampler(p, q2 + 0xea);
    release(p, q2);
    release_sampler(p, q1 + 0xea);
    release(p, q1);
    // The effect pass without a depth target.
    p.m.w::<u64>(rb + 0x60, 0);
    targets::bind(p, rb, c.w)?;
    draw_pass(p, p.m.r64(c.ctx + 0x30), c.w, c.scene, 0x1a, 0, 0, 3)?;
    targets::unbind(p, c.w)?;
    p.m.w::<u64>(rb + 0x60, o.td.addr() as u64);
    p.helper("depth of field gather", GATHER, &[c.pool as u64, shadow_blur as u64])?;
    p.helper("depth of field gather", GATHER, &[c.pool as u64, ssr_blur as u64])?;
    Ok(())
}

/// The stack objects destroyed in the reverse order of their construction: the Light target object, the depth target,
/// the QuatNormal gather, its target object, the two stack textures' samplers and slots.
unsafe fn finish(p: &mut Port, c: &C, o: &Objects, t: &Temps) -> R<()> {
    release(p, o.t3.addr() + 8);
    release(p, o.td.addr() + 8);
    p.helper("depth of field gather", GATHER, &[c.pool as u64, t.quat_normal as u64])?;
    release(p, o.t1.addr() + 8);
    release_sampler(p, o.sb.addr() + 0xea);
    release(p, o.sb.addr());
    release_sampler(p, o.sa.addr() + 0xea);
    release(p, o.sa.addr());
    Ok(())
}
