// The volume mask chain, main+0xa4c1ec(this, w, view, src): for each layer record the mask target is allocated, then the
// source mask (src) is halved down to the view record's size through temporary depth targets, the shadow buffer is built
// the same way (per cascade slice), and every layer is drawn through its inline programs (or main+0xa55f9c) and the two
// quad copies. The temporary targets come from the dynamic allocator, which keeps pool state: the port is applied
// only (everything past the checks is after the commit point); predicting stops at the first allocation.

use super::cubemap::{set_uniform, texture_quad};
use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, SHAPES, STREAMS};
use super::scene_passes::default_state;
use super::texture_slots::{assign, barrier_if_slot, Host};
use super::vfx::{program_apply, program_texture};
use super::{targets, viewport};

/// [[holder]]: the program set holder ([[[h]+0x10]+0x600/0x610/0x60]+8 are the mask, layer and quad program sets).
const PROGRAM_HOLDER: usize = 0x4634e40;
/// [[holder]]: the temporary target pool.
const TARGET_POOL: usize = 0x4634e70;
/// main+0x7f0ad4(pool, w, &name, format, width, height, 1, x7, 0, 1, 0, 0): a temporary target (its state).
const ALLOC_TARGET: usize = 0x7f0ad4;
/// main+0xa519f4 (13 arguments) / main+0xa51ad4 (13 arguments, 8 stack slots read): the same as an array of slices.
const ALLOC_ARRAY: usize = 0xa519f4;
const ALLOC_THREAD_ARRAY: usize = 0xa51ad4;
/// agl::sdw::ShadowMask::setupOffscrBuffer(dst, src).
const SETUP_OFFSCREEN: usize = 0xff07a0;
/// The depth-of-field object's gather of a temporary target.
const GATHER: usize = 0x7f0354;
/// main+0xa50c00(this, view, tex a, tex b, 24-byte block, slice): the layer's target setup and its first draw state.
const MASK_PASS: usize = 0xa50c00;
/// main+0xa55f9c(this, w, view, layer): the layer pass without inline programs.
const LAYER_PASS: usize = 0xa55f9c;
/// main+0xa513d0(prog, w, block, count): the cascade block of a program.
const BLOCK_SET: usize = 0xa513d0;
/// main+0xa51614(this, view): the chain's finish.
const FINISH: usize = 0xa51614;
/// The frame orientation and the viewport type.
const ORIENTATION: usize = 0x46312d8;
const VIEWPORT_VTABLE: usize = 0x46323b0;
/// [var]: the texture quad's scale and offset.
const QUAD_SCALE: usize = 0x4634f50;
const QUAD_OFFSET: usize = 0x462cee0;
/// The thread index key and libc's getspecific / the fallback the game calls without a value.
const THREAD_KEY: usize = 0x462af70;
const GET_SPECIFIC: usize = 0x2b15be0;
/// Render buffer virtual +0x28: the wrapper's unbind (targets::unbind).
const RENDER_BUFFER_UNBIND: usize = 0xbe19ac;

const NAME_LAYER: usize = 0x377a4b8;
const NAME_DEPTH: usize = 0x378af64;
const NAME_SHADOW: usize = 0x3772387;
const NAME_TEMP_DEPTH: usize = 0x37a409a;
const NAME_TEMP_SHADOW: usize = 0x37a40b3;

/// The record's texture slot id for the pass barriers: after the mask / shadow chains, and after the layers.
const SLOT_CHAIN: usize = 0x5a8;
const SLOT_LAYER: usize = 0x4b8;

struct Ctx {
    this: usize,
    w: usize,
    view: u32,
    /// The view record (this +0x228, 0x728 each).
    v: usize,
    /// Its render buffer (+0x3e8).
    rb: usize,
}

/// `n`, or 0 when it is not below `count` (as the game's signed index).
fn clamp_index(n: u32, count: u32) -> usize {
    (if count <= n { 0 } else { n }) as i32 as isize as usize
}

unsafe fn programs(p: &Port, off: usize) -> usize {
    let h = p.m.r64(p.m.r64(p.base + PROGRAM_HOLDER));
    p.m.r64(p.m.r64(p.m.r64(h + 0x10) + off) + 8)
}

/// The set's program for the selector `sel` (the set's first program below 1).
unsafe fn pick(p: &Port, set: usize, sel: i32) -> usize {
    if sel < 1 {
        return p.m.r64(set + 0x10);
    }
    pick_at(p, set, sel as u32 - 1)
}

unsafe fn pick_at(p: &Port, set: usize, i: u32) -> usize {
    let i = if p.m.r32(set + 0x18) <= i { 0 } else { i };
    p.m.r64(set + 0x20) + i as usize * 0x158
}

unsafe fn ready(p: &Port, prog: usize) -> R<()> {
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    Ok(())
}

/// Width (at least 1) and height (2D class 3: the height, else 1, and at least the height) of a texture state.
unsafe fn tex_w(p: &Port, t: usize) -> u32 {
    (p.m.r16(t + 0x30) as u32).max(1)
}

unsafe fn tex_h(p: &Port, t: usize) -> u32 {
    let h = p.m.r16(t + 0x32) as u32;
    (if p.m.r16(t + 0x3a) == 3 { h } else { 1 }).max(h)
}

/// A default render state with the mask passes' changes: blend / write bits 0x101, no depth bit, +0x64 = 8.
unsafe fn state_a(p: &Port) -> [u32; 0x20] {
    let mut g = default_state(p);
    g[0] = (g[0] & 0xff00_0000) | 0x101;
    g[1] &= !1;
    g[0x19] = (g[0x19] & 0xffff_0000) | 8;
    g
}

/// A default render state with the layer passes' changes.
unsafe fn state_b(p: &Port) -> [u32; 0x20] {
    let mut g = default_state(p);
    g[0] &= 0xff00_0000;
    g[1] &= !1;
    g[0x19] = (g[0x19] & 0xffff_0000) | (g[0x19] & 0xff);
    g
}

/// The viewport of the record's size (+0x3f0, +0x3f4).
unsafe fn target_viewport(p: &Port, v: usize) -> [u32; 10] {
    viewport::full(p, p.m.f32(v + 0x3f0), p.m.f32(v + 0x3f4))
}

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

unsafe fn zero(p: &mut Port, s: &Host) {
    for o in [0, 8, 0x10] {
        p.m.w::<u64>(s.addr() + o, 0);
    }
}

unsafe fn pool(p: &Port) -> usize {
    p.m.r64(p.m.r64(p.base + TARGET_POOL))
}

/// A temporary target (main+0x7f0ad4 with 12 arguments, four on the stack).
unsafe fn alloc(p: &mut Port, c: &Ctx, name: usize, format: u32, width: u32, height: u32, x7: u64) -> R<usize> {
    let name: [u64; 2] = [(p.base + name) as u64, 0];
    let args = [pool(p) as u64, c.w as u64, name.as_ptr() as u64, format as u64, width as u64, height as u64, 1, x7, 0, 1, 0, 0];
    Ok(p.call_stack("volume mask target allocated by the call", ALLOC_TARGET, &args)? as usize)
}


/// pthread_getspecific of the thread index key: the slice array's thread slot (value - 1), none without a value.
unsafe fn thread_slot(p: &Port) -> Option<i32> {
    let key = p.m.r32(p.m.r64(p.base + THREAD_KEY)) as u64;
    match p.query(p.base + GET_SPECIFIC, &[key]) {
        0 => None,
        v => Some(v as i32 - 1),
    }
}

unsafe fn gather(p: &mut Port, target: usize) -> R<()> {
    let pl = pool(p);
    p.helper("depth of field gather by the call", GATHER, &[pl as u64, target as u64])?;
    Ok(())
}

unsafe fn mask_pass(p: &mut Port, c: &Ctx, scratch: &Host, a: usize, b: usize, slice: u32) -> R<()> {
    super::mask_callees::mask_pass(p, c.this, c.view, a as usize, b as usize, scratch.addr(), slice as u32)?;
    Ok(())
}

/// The unbind of the record's render buffer and the target barrier when its texture (id at `slot`) has a slot.
unsafe fn finish_pass(p: &mut Port, c: &Ctx, slot: usize) -> R<()> {
    targets::unbind(p, c.w)?;
    barrier_if_slot(p, c.v + slot, c.w);
    Ok(())
}

/// The layer program of the inline path (set +0x610): the cascade count and the record's selector pick it.
unsafe fn layer_program(p: &Port, c: &Ctx, rec: usize) -> usize {
    let set = programs(p, 0x610);
    let t = p.m.r64(set + 0x30);
    let a = p.m.r32(c.v + 0x1c8).wrapping_mul(p.m.r16(t + 0x20) as u32);
    let b = (p.m.r8(rec + 5) as u32).wrapping_mul(p.m.r16(t + if p.m.r32(set + 0x28) > 1 { 0x28 } else { 0 } + 0x20) as u32);
    pick(p, set, a.wrapping_add(b) as i32)
}

/// The mask program (set +0x600) of the first chain.
unsafe fn mask_program(p: &Port, c: &Ctx) -> usize {
    let set = programs(p, 0x600);
    let t = p.m.r64(set + 0x30);
    pick(p, set, (p.m.r8(c.this + 0x20c) as u32).wrapping_mul(p.m.r16(t + 0x20) as u32) as i32)
}

/// The shadow chain's program (set +0x600): the cascade selector plus the layer selector.
unsafe fn shadow_program(p: &Port, c: &Ctx) -> usize {
    let set = programs(p, 0x600);
    let t = p.m.r64(set + 0x30);
    let a = if p.m.r32(c.v + 0x1c8) != 0 { p.m.r16(t + if p.m.r32(set + 0x28) > 1 { 0x28 } else { 0 } + 0x20) as u32 } else { 0 };
    let b = (p.m.r8(c.this + 0x20d) as u32).wrapping_mul(p.m.r16(t + 0x20) as u32);
    pick(p, set, a.wrapping_add(b) as i32)
}

unsafe fn layer_record(p: &Port, this: usize, l: u32) -> usize {
    p.m.r64(this + 0x218) + clamp_index(l, p.m.r32(this + 0x210)) * 0x2c
}

/// main+0xa4c1ec(this, w, view, src): see the file head. With this +0x298 and the view's record enabled; `src` is the
/// state of the mask to reduce.
pub(super) unsafe fn volume_mask_chain(p: &mut Port, this: usize, w: usize, view: u32, src: usize) -> R<()> {
    if p.m.r8(this + 0x298) == 0 {
        return Ok(());
    }
    let v = p.m.r64(this + 0x228) + clamp_index(view, p.m.r32(this + 0x220)) * 0x728;
    if p.m.r8(v) == 0 {
        return Ok(());
    }
    let c = Ctx { this, w, view, v, rb: v + 0x3e8 };
    check(p, &c)?;
    let scratch = Host::new(p, 0x18);
    p.m.commit();
    // commit
    let r = run(p, &c, src, &scratch);
    scratch.release(p);
    r
}

/// Everything that can stop, before the first allocation: programs still being set up, the render buffer's unbind, the
/// slice arrays' thread slot.
unsafe fn check(p: &Port, c: &Ctx) -> R<()> {
    if p.vfn(c.rb, 0x28) != RENDER_BUFFER_UNBIND {
        return Err("render buffer virtual +0x28");
    }
    ready(p, mask_program(p, c))?;
    let shadow = p.m.r32(c.this + 0x208) != 0;
    if shadow {
        ready(p, shadow_program(p, c))?;
        if p.m.r32(c.v + 0x1c8) != 0 && thread_slot(p).is_none() {
            return Err("slice array thread slot missing");
        }
    }
    let quads = programs(p, 0x60);
    let n = p.m.r32(c.this + 0x210);
    for l in 0..(n as i32).max(0) as u32 {
        let rec = layer_record(p, c.this, l);
        if p.m.r8(rec) == 0 {
            continue;
        }
        if p.m.r8(c.this + 0x20e) != 0 {
            ready(p, layer_program(p, c, rec))?;
        }
        let kind = p.m.r8(rec + 6) as u32;
        ready(p, pick_at(p, quads, kind << 2 | 2))?;
        ready(p, pick_at(p, quads, kind << 2))?;
    }
    Ok(())
}

unsafe fn run(p: &mut Port, c: &Ctx, src: usize, scratch: &Host) -> R<()> {
    allocate_layers(p, c)?;
    if p.m.r32(c.this + 0x208) != 0 {
        allocate_shadow(p, c)?;
    }
    mask_chain(p, c, src, scratch)?;
    if p.m.r32(c.this + 0x208) != 0 {
        shadow_chain(p, c, scratch)?;
    }
    layers(p, c, scratch)?;
    // The per-layer debug names ("%d : mask", "%d : raw", "%d : depth") are only formatted into a stack string.
    if p.m.r8(c.this + 0x298) != 0 {
        let v = p.m.r64(c.this + 0x228) + clamp_index(c.view, p.m.r32(c.this + 0x220)) * 0x728;
        if p.m.r8(v) != 0 && p.m.r8(c.this + 0x244) != 0 {
            return Ok(());
        }
    }
    super::mask_callees::volume_mask_finish(p, c.this, c.view)?;
    Ok(())
}

/// The per-layer targets ("volume mask - layer", kept in the record +0x228 array and copied to its +0x218 states) and the
/// depth targets by level ("volume mask - depth", kept at +0x230, their 24-byte blocks at +0x260).
unsafe fn allocate_layers(p: &mut Port, c: &Ctx) -> R<()> {
    let (this, v) = (c.this, c.v);
    let n = p.m.i32(v + 0x220);
    let arr = p.m.r64(v + 0x228);
    for k in 0..n.max(0) as usize {
        p.m.w::<u64>(arr + k * 8, 0);
    }
    for o in (0..0xc0).step_by(8) {
        p.m.w::<u64>(v + 0x230 + o, 0);
    }
    let mut count = p.m.r32(this + 0x210);
    let mut l = 0u32;
    while (l as i32) < count as i32 {
        let rec = layer_record(p, this, l);
        if p.m.r8(rec) != 0 {
            let b = p.m.r8(rec + 4);
            let sh = (b & 0x1f) as u32;
            let width = (p.m.r32(v + 0x2c) >> sh).max(1);
            let height = (p.m.r32(v + 0x30) >> sh).max(1);
            let t = alloc(p, c, NAME_LAYER, p.m.r32(this + 0x23c), width, height, 0)?;
            let at = p.m.r64(v + 0x228) + clamp_index(l, p.m.r32(v + 0x220)) * 8;
            p.m.w::<u64>(at, t as u64);
            let state = p.m.r64(v + 0x218) + clamp_index(l, p.m.r32(v + 0x210)) * 0xf0;
            assign(p, state, t)?;
            p.m.fetch_or32(state + 0xe4, 1);
            let slot = v + 0x230 + b as usize * 8;
            if p.m.r64(slot) == 0 {
                let own = v + 0x260 + b as usize * 0x18;
                if p.m.r8(this + 0x20e) == 0 {
                    let d = alloc(p, c, NAME_DEPTH, 0x5a, width, height, own as u64)?;
                    p.m.w::<u64>(slot, d as u64);
                } else {
                    let d = alloc(p, c, NAME_DEPTH, 0x5a, width, height, 0)?;
                    p.m.w::<u64>(slot, d as u64);
                    p.m.w::<u32>(own, 0);
                    p.m.w::<u64>(own + 8, 0);
                    p.m.w::<u64>(own + 0x10, 0);
                }
            }
        }
        count = p.m.r32(this + 0x210);
        l += 1;
    }
    Ok(())
}

/// The shadow target ("volume mask - shadow", an array with the cascade count): the record +0x2f0, its offscreen buffer
/// (+0x2f8) set up from it and the record's flags.
unsafe fn allocate_shadow(p: &mut Port, c: &Ctx) -> R<()> {
    let (this, v) = (c.this, c.v);
    let sh = p.m.r32(this + 0x208) & 0x1f;
    let b0 = p.m.r64(v + 0xb0);
    let width = ((p.m.r16(b0 + 0x30) as u32) >> sh).max(1);
    let h = p.m.r16(b0 + 0x32) as u32;
    let height = (if p.m.r16(b0 + 0x3a) == 3 { h } else { 1 }).max(h >> sh);
    let cascades = p.m.r32(v + 0x1c8);
    let t = if cascades == 0 {
        alloc(p, c, NAME_SHADOW, 0x5a, width, height, 0)?
    } else {
        let name: [u64; 2] = [(p.base + NAME_SHADOW) as u64, 0];
        let args = [pool(p) as u64, c.w as u64, name.as_ptr() as u64, 0x5a, width as u64, height as u64, cascades as u64, 1, 0, 0, 1, 0, 0];
        p.call_stack("volume mask shadow array allocated by the call", ALLOC_ARRAY, &args)? as usize
    };
    p.m.w::<u64>(v + 0x2f0, t as u64);
    p.helper("shadow mask offscreen buffer by the call", SETUP_OFFSCREEN, &[(v + 0x2f8) as u64, t as u64])?;
    let b = p.m.r8(v + 0x3e0) | 4;
    let h16 = p.m.r16(v + 0x3d6) | 1;
    p.m.w::<u8>(v + 0x3e0, b);
    p.m.w::<u16>(v + 0x3d6, h16);
    p.m.fetch_or32(v + 0x3dc, 0x10);
    let x = p.m.r8(p.m.r64(v + 0xb0) + 0xdb);
    p.m.w::<u8>(v + 0x3e0, b);
    p.m.w::<u16>(v + 0x3d6, h16);
    p.m.w::<u8>(v + 0x3d3, x);
    p.m.fetch_or32(v + 0x3dc, 0x10);
    Ok(())
}

/// The record's state at +0x638 given the state `src` (the game's assignment), then its flag +0xe4 bit 0 (record +0x71c).
unsafe fn load_source(p: &mut Port, c: &Ctx, src: usize) -> R<()> {
    assign(p, c.v + 0x638, src)?;
    p.m.fetch_or32(c.v + 0x71c, 1);
    Ok(())
}

/// A slice's full-screen draw into the record's render buffer: the buffer bound (the viewport only for the first
/// slice when `first_only`), the slice index as uniform 0 when `index`, the full-screen triangle, the unbind.
unsafe fn slice_draw(p: &mut Port, c: &Ctx, prog: usize, slice: u32, with_viewport: bool) -> R<()> {
    let vp = target_viewport(p, c.v);
    targets::bind(p, c.rb, c.w)?;
    if with_viewport {
        viewport::apply(p, vp.as_ptr() as usize, c.w, c.rb);
    }
    set_uniform(p, prog, c.w, 0, &(slice as f32).to_le_bytes());
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    stream_draw(p, streams + 0x488, c.w);
    targets::unbind(p, c.w)
}

/// The mask reduction: the level of the view record's size nearest to `src` (halvings 1..5) drawn from it when there
/// is a target for it, then the levels above it down to the last one, each from the one before, through temporary
/// "volume mask - temp depth" targets where the record has none.
unsafe fn mask_chain(p: &mut Port, c: &Ctx, src: usize, scratch: &Host) -> R<()> {
    let (w, v) = (c.w, c.v);
    let prog = mask_program(p, c);
    let g = state_a(p);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    program_apply(p, prog, w)?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    shape_state(p, shapes + 0x10e8, w);

    let (sw, sh) = (tex_w(p, src), tex_h(p, src));
    let (tw, th) = (p.m.r32(v + 0x2c), p.m.r32(v + 0x30));
    // `skip`: the source is not at least twice the record's size (level 0, no first draw).
    let (skip, level) = if tw < sw << 1 || th < sh << 1 {
        (true, 0u32)
    } else {
        let mut lv = 1;
        if sw << 2 <= tw && sh << 2 <= th {
            lv = 2;
            if sw << 3 <= tw && sh << 3 <= th {
                lv = 3;
                if sw << 4 <= tw && sh << 4 <= th {
                    lv = 4;
                    if sh << 5 <= th && sw << 5 <= tw {
                        lv = 5;
                    }
                }
            }
        }
        (false, lv)
    };
    p.m.w::<u16>(v + 0x70c, 0);
    p.m.w::<u8>(v + 0x70e, 1);
    let mut levels = (p.m.r64(v + 0x230) != 0) as u32;
    for k in 1..6u32 {
        if p.m.r64(v + 0x230 + k as usize * 8) != 0 {
            levels = k + 1;
        }
    }
    p.m.fetch_or32(v + 0x71c, 4);
    if !skip && p.m.r64(v + 0x230 + level as usize * 8) != 0 {
        p.helper("shadow mask offscreen buffer by the call", SETUP_OFFSCREEN, &[(v + 0x638) as u64, src as u64])?;
        set_uniform(p, prog, w, 9, &f32s(&[1.0 / sw as f32, 1.0 / sh as f32]));
        program_texture(p, prog, w, 0, v + 0x638)?;
        for o in [0, 8, 0x10] {
            let x = p.m.r64(v + 0x260 + level as usize * 0x18 + o);
            p.m.w::<u64>(scratch.addr() + o, x as u64);
        }
        let target = p.m.r64(v + 0x230 + level as usize * 8);
        mask_pass(p, c, scratch, 0, target, 0)?;
        let vp = target_viewport(p, v);
        targets::bind(p, c.rb, w)?;
        viewport::apply(p, vp.as_ptr() as usize, w, c.rb);
        stream_draw(p, streams + 0x488, w);
        targets::unbind(p, w)?;
        barrier_if_slot(p, v + SLOT_CHAIN, w);
    }
    if level + 1 < levels {
        let mut s = src;
        let mut k = level + 1;
        let mut temp_prev = false;
        let mut last_new = false;
        while k != levels {
            let (cw, ch) = (tex_w(p, s), tex_h(p, s));
            let existing = p.m.r64(v + 0x230 + k as usize * 8);
            let target = if existing == 0 {
                alloc(p, c, NAME_TEMP_DEPTH, 0x5a, (cw >> 1).max(1), (ch >> 1).max(1), 0)?
            } else {
                existing
            };
            load_source(p, c, s)?;
            set_uniform(p, prog, w, 9, &f32s(&[1.0 / cw as f32, 1.0 / ch as f32]));
            program_texture(p, prog, w, 0, v + 0x638)?;
            if existing == 0 {
                zero(p, scratch);
            } else {
                for o in [0, 8, 0x10] {
                    let x = p.m.r64(v + 0x260 + k as usize * 0x18 + o);
                    p.m.w::<u64>(scratch.addr() + o, x as u64);
                }
            }
            mask_pass(p, c, scratch, 0, target, 0)?;
            let vp = target_viewport(p, v);
            targets::bind(p, c.rb, w)?;
            viewport::apply(p, vp.as_ptr() as usize, w, c.rb);
            stream_draw(p, streams + 0x488, w);
            if temp_prev {
                gather(p, s)?;
            }
            targets::unbind(p, w)?;
            barrier_if_slot(p, v + SLOT_CHAIN, w);
            k += 1;
            s = target;
            temp_prev = existing == 0;
            last_new = temp_prev;
        }
        if last_new {
            gather(p, s)?;
        }
    }
    Ok(())
}

/// The shadow reduction: the record's source (+0xb0) halved through "volume mask - temp shadow" targets (slice arrays
/// with cascades) while it is more than twice the shadow target's size, one full-screen draw per cascade slice from the
/// last one into the shadow target.
unsafe fn shadow_chain(p: &mut Port, c: &Ctx, scratch: &Host) -> R<()> {
    let (w, v) = (c.w, c.v);
    let prog = shadow_program(p, c);
    let g = state_a(p);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    program_apply(p, prog, w)?;
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    shape_state(p, shapes + 0x10e8, w);
    let target = p.m.r64(v + 0x2f0);
    let first = p.m.r64(v + 0xb0);
    let (tw, th) = (tex_w(p, target), tex_h(p, target));
    p.m.w::<u16>(v + 0x70c, 0);
    p.m.w::<u8>(v + 0x70e, 1);
    p.m.fetch_or32(v + 0x71c, 4);
    let cascades = p.m.r32(v + 0x1c8);
    let (mut cw, mut ch) = (tex_w(p, first), tex_h(p, first));
    let mut cur = first;
    let mut src = first;
    let mut iter = 0u32;
    if tw * 2 < cw && th * 2 < ch {
        loop {
            cur = if cascades == 0 {
                alloc(p, c, NAME_TEMP_SHADOW, 0x5a, cw >> 1, ch >> 1, 0)?
            } else {
                let slot = thread_slot(p).ok_or("slice array thread slot missing")?;
                let name: [u64; 2] = [(p.base + NAME_TEMP_SHADOW) as u64, 0];
                let pl = pool(p);
                let args = [
                    pl as u64,
                    c.w as u64,
                    (pl as isize + slot as isize * 0x280 + 0x10) as u64,
                    name.as_ptr() as u64,
                    0x5a,
                    4,
                    (cw >> 1) as u64,
                    (ch >> 1) as u64,
                    1,
                    0,
                    cascades as u64,
                    0,
                    1,
                    0,
                    0,
                    0,
                ];
                p.call_stack("volume mask slice array allocated by the call", ALLOC_THREAD_ARRAY, &args)? as usize
            };
            load_source(p, c, src)?;
            set_uniform(p, prog, w, 9, &f32s(&[1.0 / cw as f32, 1.0 / ch as f32]));
            program_texture(p, prog, w, 0, v + 0x638)?;
            for s in 0..cascades {
                zero(p, scratch);
                mask_pass(p, c, scratch, 0, cur, s)?;
                slice_draw(p, c, prog, s, s == 0)?;
            }
            if iter != 0 {
                gather(p, src)?;
            }
            barrier_if_slot(p, v + SLOT_CHAIN, w);
            iter += 1;
            cw = tex_w(p, cur);
            ch = tex_h(p, cur);
            if !(tw * 2 < cw) {
                break;
            }
            src = cur;
            if !(th * 2 < ch) {
                break;
            }
        }
    }
    p.helper("shadow mask offscreen buffer by the call", SETUP_OFFSCREEN, &[(v + 0x638) as u64, cur as u64])?;
    set_uniform(p, prog, w, 9, &f32s(&[1.0 / cw as f32, 1.0 / ch as f32]));
    program_texture(p, prog, w, 0, v + 0x638)?;
    for s in 0..cascades {
        zero(p, scratch);
        mask_pass(p, c, scratch, 0, target, s)?;
        slice_draw(p, c, prog, s, s == 0)?;
    }
    barrier_if_slot(p, v + SLOT_CHAIN, w);
    if iter != 0 {
        gather(p, cur)?;
    }
    Ok(())
}

unsafe fn layers(p: &mut Port, c: &Ctx, scratch: &Host) -> R<()> {
    let this = c.this;
    let mut count = p.m.r32(this + 0x210);
    let mut l = 0u32;
    while (l as i32) < count as i32 {
        let rec = layer_record(p, this, l);
        if p.m.r8(rec) != 0 {
            if p.m.r8(this + 0x20e) == 0 {
                super::mask_callees::layer_pass(p, this, c.w, c.view, l as u32)?;
            } else {
                layer_inline(p, c, scratch, l, rec)?;
            }
            layer_quads(p, c, scratch, l)?;
        }
        count = p.m.r32(this + 0x210);
        l += 1;
    }
    Ok(())
}

/// The cascades' matrices: the three rows at +0x1dc / +0x1ec / +0x1fc times each of the four rows of the 16 floats at
/// +0xb8 + 0x40 k (a fused product per lane, the fourth value joining lane 3).
unsafe fn cascade_matrices(p: &Port, v: usize, count: u32) -> Vec<f32> {
    let row = |a: usize| -> [f32; 4] { std::array::from_fn(|j| p.m.f32(a + j * 4)) };
    let m = [row(v + 0x1dc), row(v + 0x1ec), row(v + 0x1fc)];
    let mut out = vec![0f32; count as usize * 16];
    for k in 0..count as usize {
        for r in 0..4 {
            let q = row(v + k * 0x40 + 0xb8 + r * 0x10);
            for j in 0..4 {
                let t = m[0][j] * q[0];
                let t = m[1][j].mul_add(q[1], t);
                let t = m[2][j].mul_add(q[2], t);
                let z = if j == 3 { q[3] } else { 0.0 };
                out[k * 16 + r * 4 + j] = z + t;
            }
        }
    }
    out
}

/// The clip-to-texture matrix of the 16 floats at +0x34 (scale and bias 0.5, the zero terms kept for the game's rounding).
unsafe fn bias_matrix(p: &Port, v: usize) -> [f32; 16] {
    let e: [[f32; 4]; 4] = std::array::from_fn(|r| std::array::from_fn(|j| p.m.f32(v + 0x34 + r * 0x10 + j * 4)));
    let (z, h) = (0f32, 0.5f32);
    let mut out = [0f32; 16];
    for j in 0..4 {
        let mut v2 = e[0][j] * z;
        let mut v0 = e[0][j] * h;
        v0 = z.mul_add(e[1][j], v0);
        v0 = z.mul_add(e[2][j], v0);
        let mut v3 = v2;
        v2 = z.mul_add(e[1][j], v2);
        v3 = h.mul_add(e[1][j], v3);
        v3 = z.mul_add(e[2][j], v3);
        v0 = h.mul_add(e[3][j], v0);
        let mut v5 = v2;
        v2 = z.mul_add(e[2][j], v2);
        v3 = h.mul_add(e[3][j], v3);
        v5 = h.mul_add(e[2][j], v5);
        v5 = h.mul_add(e[3][j], v5);
        out[j] = v0;
        out[4 + j] = v3;
        out[8 + j] = v5;
        out[12 + j] = e[3][j] + v2;
    }
    out
}

/// The layer drawn with the inline programs (this +0x20e): the layer's temporary target set up (main+0xa50c00), the
/// record's buffer bound and cleared, the cascade matrices and the layer's values as uniforms of the layer program, the
/// mask level as texture 0 and the shadow source as texture 1, the full-screen triangle.
unsafe fn layer_inline(p: &mut Port, c: &Ctx, scratch: &Host, l: u32, rec: usize) -> R<()> {
    let (this, w, v, rb) = (c.this, c.w, c.v, c.rb);
    let prog = layer_program(p, c, rec);
    let layer_target = p.m.r64(p.m.r64(v + 0x228) + clamp_index(l, p.m.r32(v + 0x220)) * 8);
    zero(p, scratch);
    mask_pass(p, c, scratch, layer_target, 0, 0)?;
    let vp = target_viewport(p, v);
    let mut values = [0u32; 0x22];
    values[0x20] = 1f32.to_bits();
    targets::bind_clear(p, rb, w, vp.as_ptr() as usize, 1, values.as_ptr() as usize)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let g = state_b(p);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let source = if p.m.r32(this + 0x208) == 0 { p.m.r64(v + 0xb0) } else { v + 0x2f8 };
    let cascades = p.m.r32(v + 0x1c8);
    let mats = cascade_matrices(p, v, cascades);
    let bias = bias_matrix(p, v);
    program_apply(p, prog, w)?;
    set_uniform(p, prog, w, 11, &f32s(&bias));
    let block: Vec<u32> = (0..12).map(|k| p.m.r32(v + 0x1dc + k * 4)).collect();
    set_uniform(p, prog, w, 1, &u32s(&block));
    let a4 = p.m.f32(v + 0xa4);
    let a8 = p.m.f32(v + 0xa8);
    let n = (p.m.r16(rec + 2) as i32 - 1) as f32;
    let d3 = [1.0 / n, a4, f32::from_bits(p.m.r32(rec + 8)), 1.0 - a4 / a8];
    set_uniform(p, prog, w, 3, &f32s(&d3));
    set_uniform(p, prog, w, 4, &(p.m.r16(rec + 2) as u32).to_le_bytes());
    let (sw, sh) = (tex_w(p, source), tex_h(p, source));
    set_uniform(p, prog, w, 9, &f32s(&[1.0 / sw as f32, 1.0 / sh as f32]));
    let r14 = p.m.f32(rec + 0x14);
    set_uniform(p, prog, w, 8, &f32s(&[p.m.f32(rec + 0x18) - r14, r14]));
    set_uniform(p, prog, w, 10, &u32s(&[p.m.r32(v + 0x1d0), p.m.r32(v + 0x1cc)]));
    if cascades == 0 {
        // The game passes the first 0x40 bytes of its stack state (the render state default, as changed above).
        set_uniform(p, prog, w, 6, &u32s(&g[..16]));
    } else {
        set_uniform(p, prog, w, 6, &f32s(&mats));
        super::mask_callees::block_set(p, prog, w, v + 0x1b8, cascades as u32)?;
    }
    p.m.w::<u16>(v + 0x70c, 0x101);
    p.m.w::<u8>(v + 0x70e, 2);
    p.m.fetch_or32(v + 0x71c, 4);
    let level = p.m.r64(v + p.m.r8(rec + 4) as usize * 8 + 0x230);
    load_source(p, c, level)?;
    program_texture(p, prog, w, 1, source)?;
    program_texture(p, prog, w, 0, v + 0x638)?;
    let mut first = a4;
    if !(a4 > 0.0 && p.m.r8(rec + 1) != 0) {
        first = p.m.f32(rec + 0xc);
    }
    let mut span = p.m.f32(rec + 0x10) - first;
    let half = (p.m.f32(rec + 0x10) - p.m.f32(rec + 0xc)) * 0.5;
    if !(span > half) {
        span = half;
    }
    let d2 = [p.m.r32(v + 0x1d4), p.m.r32(v + 0x1d8), span.to_bits(), (first + f32::from_bits(0x3700_0000)).to_bits()];
    set_uniform(p, prog, w, 2, &u32s(&d2));
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    shape_state(p, shapes + 0x10e8, w);
    stream_draw(p, streams + 0x488, w);
    finish_pass(p, c, SLOT_LAYER)
}

/// The layer's two quad copies (programs of set +0x60 by the record's kind, byte +6): the layer target into a
/// temporary depth target ("volume mask - temp depth") sized like it, then that into the record's own target (this
/// +0x20 array of the view record), each followed by the target barrier; the gather of the temporary target.
unsafe fn layer_quads(p: &mut Port, c: &Ctx, scratch: &Host, l: u32) -> R<()> {
    let (this, w, v, rb) = (c.this, c.w, c.v, c.rb);
    let g = state_b(p);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let target = p.m.r64(p.m.r64(v + 0x228) + clamp_index(l, p.m.r32(v + 0x220)) * 8);
    let (tw, th) = (tex_w(p, target), tex_h(p, target));
    let temp = alloc(p, c, NAME_TEMP_DEPTH, p.m.r32(this + 0x23c), tw, th, 0)?;
    let vt = (p.pfn(VIEWPORT_VTABLE) + 0x10) as u64;
    let quad: [u32; 10] = [
        vt as u32,
        (vt >> 32) as u32,
        0f32.to_bits(),
        0f32.to_bits(),
        (tw as f32).to_bits(),
        (th as f32).to_bits(),
        p.m.i32(p.m.r64(p.base + ORIENTATION)) as u32,
        0f32.to_bits(),
        1f32.to_bits(),
        0,
    ];
    let kind = p.m.r8(layer_record(p, this, l) + 6) as u32;
    p.m.w::<u16>(v + 0x70c, 0x101);
    p.m.w::<u8>(v + 0x70e, 2);
    p.m.fetch_or32(v + 0x71c, 4);
    load_source(p, c, target)?;
    zero(p, scratch);
    mask_pass(p, c, scratch, temp, 0, 0)?;
    let vp = target_viewport(p, v);
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let quads = programs(p, 0x60);
    let prog = pick_at(p, quads, kind << 2 | 2);
    program_apply(p, prog, w)?;
    texture_quad(p, w, prog, v + 0x638, quad.as_ptr() as usize, p.pfn(QUAD_SCALE), p.pfn(QUAD_OFFSET))?;
    finish_pass(p, c, SLOT_LAYER)?;
    load_source(p, c, temp)?;
    zero(p, scratch);
    let out = p.m.r64(p.m.r64(v + 0x20) + clamp_index(l, p.m.r32(v + 0x18)) * 8);
    mask_pass(p, c, scratch, out, 0, 0)?;
    let vp = target_viewport(p, v);
    targets::bind(p, rb, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb);
    let prog = pick_at(p, quads, kind << 2);
    program_apply(p, prog, w)?;
    texture_quad(p, w, prog, v + 0x638, quad.as_ptr() as usize, p.pfn(QUAD_SCALE), p.pfn(QUAD_OFFSET))?;
    finish_pass(p, c, SLOT_LAYER)?;
    gather(p, temp)
}
