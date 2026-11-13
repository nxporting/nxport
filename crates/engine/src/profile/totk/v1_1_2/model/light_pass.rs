// TotK's deferred lighting pass, main+0x966f7c(this, w, view, scene, records, tex, shadows, storage): per view, the
// lights drawn into the miasma density target (a texture of the dynamic allocator), the fog's texture set, then one
// full-screen (or grid-instanced) pass that samples it. The game builds its render buffers, targets and texture copies
// as stack objects; they are host objects here (byte layout as the game's, only the fields anything reads).
// Applying only: the density texture comes from the dynamic allocator, past the commit point.

use super::draw::{Port, R};
use super::texture_slots::{alloc_target, copy_target, new_buffer, new_target, release, release_sampler, set_extent, texture_assign, Host};
use super::texture_slots::{COLOR_VTABLE, DEPTH_VTABLE};
use super::lights::{light_program, light_shape, shape_state, stream_draw, stream_draw_n, SHAPES, STREAMS};
use super::targets;
use super::vfx::{program_apply, program_block, program_storage_block, program_texture, shadow_mask_setup};
use super::viewport;

/// sead::GraphicsContext's constructor (0x74 bytes of default state).
const GRAPHICS_CONTEXT_INIT: usize = 0x688a38;
/// agl::TextureData's constructor (0xf0 bytes).
const TEXTURE_CTOR: usize = 0xbf2180;
/// Texture copy used for the shadow mask sources (main+0x8c3b14(dst, src texture)).
const TEXTURE_COPY: usize = 0x8c3b14;
/// Pointers to: the graphics settings (+0x50c), the allocator's owner (the gather's object), the fog holder (+0xd0: the fog object).
const HOLDER: usize = 0x462bbf0;
const SCENE_HOLDER: usize = 0x4634e70;
const FOG_HOLDER: usize = 0x462ada0;
const GATHER: usize = 0x7f0354;
/// "agl::TextureData" and "MiasmaDensity".
const TEXTURE_NAME: usize = 0x37ccf1f;
const DENSITY_NAME: usize = 0x37daacc;
const GOT_DRAW_ARRAYS: usize = 0x4613b80;
/// The light entry tree of an owner (+0x130), an entry's id and its scale.
const ENTRY_TREE: usize = 0x96a970;
const ENTRY_ID: usize = 0x96a918;
const ENTRY_SCALE: usize = 0x96aa20;

unsafe fn holder(p: &Port) -> usize {
    p.m.r64(p.m.r64(p.base + HOLDER))
}


/// A host object of 0x100 bytes the port owns.
fn host(p: &mut Port) -> Host {
    Host::new(p, 0x100)
}

/// A constructed agl::TextureData (main+0xbf2180).
unsafe fn texture_new(p: &mut Port) -> Host {
    let o = host(p);
    p.query(p.base + TEXTURE_CTOR, &[o.addr() as u64]);
    o
}

/// The constructor the game inlines where it makes a texture to be filled by main+0x8c3b14 / 0xff07a0.
unsafe fn texture_blank(p: &mut Port) -> Host {
    let o = host(p);
    p.m.w::<u32>(o.addr() + 0x58, u32::MAX);
    p.m.w::<u16>(o.addr() + 0xa1, 1);
    p.m.w::<u64>(o.addr() + 0xa8, (p.base + TEXTURE_NAME) as u64);
    o
}

/// A colour or depth target object given the state at `src`.
unsafe fn target(p: &mut Port, vtable: usize, src: usize) -> R<Host> {
    let o = Host::new(p, super::texture_slots::TARGET_BYTES);
    new_target(p, o.addr(), vtable);
    copy_target(p, o.addr(), src)?;
    Ok(o)
}

/// A render buffer of `w` x `h`.
unsafe fn buffer(p: &mut Port, w: u16, h: u16) -> Host {
    let o = Host::new(p, super::texture_slots::BUFFER_BYTES);
    new_buffer(p, o.addr());
    set_extent(p, o.addr(), w, h);
    o
}

unsafe fn default_context(p: &Port) -> [u32; 0x20] {
    let mut g = [0u32; 0x20];
    p.query(p.base + GRAPHICS_CONTEXT_INIT, &[g.as_mut_ptr() as u64]);
    g
}

fn byte(g: &mut [u32; 0x20], o: usize, v: u8) {
    unsafe { *(g.as_mut_ptr() as *mut u8).add(o) = v }
}

/// The entry (deepest tree level first, as the game's iterator) whose own id is `id`.
unsafe fn entry_with_id(p: &mut Port, root: usize, id: u32) -> usize {
    if root == 0 {
        return 0;
    }
    let mut levels = 0;
    let mut n = root;
    loop {
        n = p.m.r64(n + 0x18);
        levels += 1;
        if n == 0 {
            break;
        }
    }
    for level in (0..levels).rev() {
        let mut node = root;
        for _ in 0..level {
            node = p.m.r64(node + 0x18);
            if node == 0 {
                return 0;
            }
        }
        for i in 0..p.m.r32(node + 8) as usize {
            let e = p.m.r64(p.m.r64(node + 0x10) + i * 8);
            if p.m.r32(p.query(p.base + ENTRY_ID, &[e as u64]) as usize) == id {
                return e;
            }
        }
    }
    0
}

/// The record's light list (count +o, lights +o+8), each drawn by light_shape.
unsafe fn light_list(p: &mut Port, ctx: usize, rec: usize, o: usize, view: u32) -> R<()> {
    let n = p.m.i32(rec + o);
    if n < 0 {
        return Err("negative light count");
    }
    let list = p.m.r64(rec + o + 8);
    for k in 0..n as usize {
        light_shape(p, ctx, p.m.r64(list + k * 8), view)?;
    }
    Ok(())
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

/// main+0x966f7c(this, w, view, scene, records, tex, shadows, storage).
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn light_pass(p: &mut Port, this: usize, w: usize, view: u32, scene: usize, records: usize, tex: usize, shadows: usize, storage: usize) -> R<()> {
    let mut o = Objs(Vec::new());
    let r = light_pass_in(p, &mut o, this, w, view, scene, records, tex, shadows, storage);
    for h in o.0 {
        h.release(p);
    }
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn light_pass_in(p: &mut Port, o: &mut Objs, this: usize, w: usize, view: u32, scene: usize, records: usize, tex: usize, shadows: usize, storage: usize) -> R<()> {
    let flags = p.m.r32(this + 0x14c);
    if flags & 3 != 3 {
        return Ok(());
    }
    let views = p.m.r64(this + 0xc0);
    let vi = if p.m.r32(this + 0xb8) <= view { 0 } else { view } as i32 as isize as usize;
    let rec = views.wrapping_add(vi.wrapping_mul(0x1a0));
    // Shadow-mask lights: the fog object's value (vtable +0x130) is positive, +0x14c bit 2, the view's bit 1.
    let fog = p.m.r64(p.m.r64(p.m.r64(p.base + FOG_HOLDER)) + 0xd0);
    let shadowed = fog != 0
        && p.query_ret_f(p.base + p.vfn(fog, 0x130), &[fog as u64]) > 0.0
        && (flags >> 2) & 1 != 0
        && (p.m.r8(rec + 0x19d) >> 1) & 1 != 0;
    if !shadowed && p.m.i32(rec + 0x138) < 1 && p.m.i32(rec + 0x148) < 1 && p.m.i32(rec + 0x178) < 1 {
        return Ok(());
    }
    let ri = if p.m.r32(records + 8) <= view { 0 } else { view } as i32 as isize as usize;
    let r5 = p.m.r64(records + 0x10).wrapping_add(ri.wrapping_mul(0x21b0));
    let fogrec = p.m.r64(r5 + 0x2118);
    if (p.m.r8(fogrec + 0x4e8) >> 3) & 1 == 0 {
        return Ok(());
    }
    // The view record's sizes (two sets, the second for the alternate target set).
    let u = |p: &Port, o: usize| p.m.r16(r5 + o);
    let a = u(p, 0x450).max(1);
    let b0 = u(p, 0x452);
    let b = (if u(p, 0x45a) != 3 { 1 } else { b0 }).max(b0);
    let c = u(p, 0xb90).max(1);
    let d0 = u(p, 0xb92);
    let d = (if u(p, 0xb9a) != 3 { 1 } else { d0 }).max(d0);
    let alt = (flags >> 4) & 1 != 0 && (p.m.r8(rec + 0x19d) >> 2) & 1 != 0;
    let flag = alt as u8;
    let (src, (aw, ah), extra) = if alt { (r5 + 0xb60, (c, d), 0xf30) } else { (r5 + 0x420, (a, b), 0x700) };
    let mut density = r5 + extra;
    // The programs this run applies: none may be pending past the commit point.
    let shadow_prog = {
        let mut i = (flag as u32) << 3 | 0x12;
        let progs = p.m.r64(p.m.r64(this + 0x20) + 8);
        if p.m.r32(progs + 0x18) <= i {
            i = 0;
        }
        p.m.r64(progs + 0x20) + i as usize * 0x158
    };
    let mut k = if flag != 0 && flags & 0x20 != 0 { 4 } else { 0 };
    k |= (flags >> 2) & 2;
    k |= p.m.r8(rec + 0x19d) as u32 & 1;
    let progs = p.m.r64(p.m.r64(this + 0x28) + 8);
    let main_prog = if k == 0 {
        p.m.r64(progs + 0x10)
    } else {
        let i = if p.m.r32(progs + 0x18) <= k - 1 { 0 } else { k - 1 };
        p.m.r64(progs + 0x20) + i as usize * 0x158
    };
    let post = p.m.r64(this + 0x30);
    let used = [shadowed.then_some(shadow_prog), Some(main_prog), Some(post)];
    for prog in used.into_iter().flatten() {
        if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
            return Err("agl program setup pending");
        }
    }
    let t1 = o.keep(texture_new(p));
    texture_assign(p, t1, src)?;
    p.m.commit();
    // commit
    // The density target: allocated by the game's allocator, copied into a texture of ours.
    let t2src = alloc_target(p, w, DENSITY_NAME, 0x1d, aw as u32, ah as u32)?;
    let t2 = o.keep(texture_new(p));
    texture_assign(p, t2, t2src)?;
    // First buffer: the density target (colour 0) and the view's own target (depth slot), cleared.
    let rb1 = o.keep(buffer(p, aw, ah));
    let t3 = target(p, COLOR_VTABLE, t2src)?;
    let t3 = o.keep(t3);
    let t4 = target(p, DEPTH_VTABLE, src)?;
    let t4 = o.keep(t4);
    p.m.w::<u64>(rb1 + 0x20, t3 as u64);
    p.m.w::<u64>(rb1 + 0x60, t4 as u64);
    let vp = viewport::full(p, aw as f32, ah as f32);
    targets::bind(p, rb1, w)?;
    viewport::apply(p, vp.as_ptr() as usize, w, rb1);
    let black = [0u32; 4];
    targets::clear(p, rb1, w, 0, 1, black.as_ptr() as usize, 0, vp.as_ptr() as usize, 0.0)?;
    let mut g = default_context(p);
    g[0] &= 0xffff_0000;
    g[1] &= !1;
    g[7] = 3;
    for (k, v) in [2u8, 2, 2, 2, 5, 5].into_iter().enumerate() {
        byte(&mut g, 0x28 + k, v);
    }
    p.apply_graphics_context(g.as_ptr() as usize, w);
    // The light program's context (as lights.rs light_program reads it) and the lights' (light_shape).
    let wcell = o.keep(host(p));
    p.m.w::<u64>(wcell, w as u64);
    let viewcell = o.keep(host(p));
    p.m.w::<u32>(viewcell, view);
    let flagcell = o.keep(host(p));
    p.m.w::<u8>(flagcell, flag);
    let densitycell = o.keep(host(p));
    p.m.w::<u64>(densitycell, density as u64);
    let lc = o.keep(host(p));
    for (k, v) in [wcell, t1, densitycell, shadows, this, viewcell, scene, storage].into_iter().enumerate() {
        p.m.w::<u64>(lc + k * 8, v as u64);
    }
    let sc = o.keep(host(p));
    for (k, v) in [flagcell, this, wcell, lc, rec].into_iter().enumerate() {
        p.m.w::<u64>(sc + k * 8, v as u64);
    }
    let shapes = p.m.r64(p.m.r64(p.base + SHAPES));
    let streams = p.m.r64(p.m.r64(p.base + STREAMS));
    if shadowed {
        let prog = shadow_prog;
        light_program(p, lc, prog)?;
        // The two shadow mask sources (this +8, +0x10) as textures at slots 2 and 3.
        for (field, slot) in [(8usize, 2u32), (0x10, 3)] {
            let x = o.keep(texture_blank(p));
            let y = o.keep(texture_new(p));
            p.cpu_helper(TEXTURE_COPY, &[x as u64, p.m.r64(this + field) as u64]);
            shadow_mask_setup(p, y, x)?;
            program_texture(p, prog, w, slot, y)?;
            release_sampler(p, y + 0xea);
            release(p, y);
            release(p, x);
        }
        // The fog entry's scale (entry id 0xc), zero without one, in a 16-byte ring block.
        let tree = p.query(p.base + ENTRY_TREE, &[p.m.r64(this + 0x130) as u64]) as usize;
        let mut record = [0u32; 4];
        let e = entry_with_id(p, tree, 0xc);
        if e != 0 {
            record[0] = p.m.r32(p.query(p.base + ENTRY_SCALE, &[e as u64]) as usize);
        }
        let block = p.ring_block(record.as_ptr() as usize, 0x10);
        program_block(p, prog, w, 1, block, 0)?;
        shape_state(p, shapes + 0x10e8, w);
        stream_draw(p, streams + 0x488, w);
    }
    // The lights: stencil-style volumes then shaded ones (five lists of the view record).
    g[0] = (g[0] & 0xffff_0000) | 1;
    g[1] |= 1;
    byte(&mut g, 0x64, 7);
    byte(&mut g, 0x65, 1);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    light_list(p, sc, rec, 0x138, 0)?;
    byte(&mut g, 0x64, 4);
    byte(&mut g, 0x65, 2);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    light_list(p, sc, rec, 0x148, 0)?;
    byte(&mut g, 0x64, 7);
    byte(&mut g, 0x65, 1);
    byte(&mut g, 0x2c, 4);
    byte(&mut g, 0x2d, 4);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    light_list(p, sc, rec, 0x158, 1)?;
    byte(&mut g, 0x64, 4);
    byte(&mut g, 0x65, 2);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    light_list(p, sc, rec, 0x168, 1)?;
    byte(&mut g, 0x64, 7);
    byte(&mut g, 0x65, 1);
    byte(&mut g, 0x2c, 5);
    byte(&mut g, 0x2d, 5);
    p.apply_graphics_context(g.as_ptr() as usize, w);
    light_list(p, sc, rec, 0x178, 0)?;
    targets::barrier_targets(p, rb1, w);
    targets::unbind(p, w)?;
    release(p, t4 + 8);
    release(p, t3 + 8);
    // The view's colour copy again (from its first target set), the density target's own pointer now the plain one.
    texture_assign(p, t1, r5 + 0x420)?;
    density = r5 + 0x700;
    p.m.w::<u64>(densitycell, density as u64);
    let block = storage + 0x1c0;
    match p.vfn(block, 0x10) {
        // nn::gfx's invalidation for shader reads (main+0x2a2bff8: barrier 0x28).
        0x2a2bff8 => p.barrier(p.m.r64(w + 0xb8), 0x28),
        f => {
            p.call(p.base + f, &[block as u64, w as u64, 0]);
        }
    }
    // Second buffer: up to three targets from the fog record (+0x538, +0x518, +0x510) with their write masks.
    let rb2 = o.keep(buffer(p, a, b));
    let mut mask = 0u32;
    let mut made = Vec::new();
    for (k, (field, bits)) in [(0x538usize, 7u32), (0x518, 0xf0), (0x510, 0x500)].into_iter().enumerate() {
        let s = p.m.r64(fogrec + field);
        if s == 0 {
            continue;
        }
        let t = target(p, COLOR_VTABLE, s + 8)?;
        let t = o.keep(t);
        p.m.w::<u64>(rb2 + 0x20 + k * 8, t as u64);
        mask |= bits;
        made.push(t);
    }
    let vp2 = viewport::full(p, a as f32, b as f32);
    targets::bind(p, rb2, w)?;
    viewport::apply(p, vp2.as_ptr() as usize, w, rb2);
    let mut g = default_context(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = mask;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    let prog = main_prog;
    program_apply(p, prog, w)?;
    let n = p.m.r32(shadows + 8);
    let sh = p.m.r64(shadows + 0x10);
    program_texture(p, prog, w, 0, t1)?;
    program_texture(p, prog, w, 1, density)?;
    program_texture(p, prog, w, 2, r5 + 0xe40)?;
    program_texture(p, prog, w, 3, sh + 0xb8)?;
    program_texture(p, prog, w, 4, sh + if n > 1 { 0x1c0 } else { 0 } + 0xb8)?;
    program_texture(p, prog, w, 5, sh + if n > 2 { 0x380 } else { 0 } + 0xb8)?;
    program_texture(p, prog, w, 6, sh + if n > 4 { 0x700 } else { 0 } + 0xb8)?;
    program_texture(p, prog, w, 7, t2)?;
    program_texture(p, prog, w, 8, tex)?;
    program_texture(p, prog, w, 9, fogrec + 0x18)?;
    program_texture(p, prog, w, 10, fogrec + 0x300)?;
    program_texture(p, prog, w, 11, fogrec + 0x208)?;
    program_block(p, prog, w, 0, rec, 0)?;
    program_block(p, prog, w, 1, this + 0x38, 0)?;
    let s = p.m.r64(scene + 0x18);
    p.m.w::<u8>(s + 0x421, p.m.r8(s + 0x421) | 1);
    program_block(p, prog, w, 2, p.m.r64(scene + 0x18) + 0x390, 0)?;
    program_storage_block(p, prog, w, 0, block, 0)?;
    let grid = p.m.i32(r5 + 0x21ac).wrapping_mul(p.m.i32(r5 + 0x21a8));
    if (flags >> 3) & 1 == 0 {
        shape_state(p, shapes + 0x10e8, w);
        stream_draw(p, streams + 0x488, w);
    } else {
        shape_state(p, shapes + 0xd28, w);
        stream_draw_n(p, streams + 0x1e8, w, grid as u32);
    }
    let cmd = p.m.r64(w + 0xb8);
    let h = holder(p);
    p.barrier(cmd, if (p.m.r8(h + 0x50c) >> 1) & 1 == 0 { 0x12 } else { 0x112 });
    // The second buffer unbound (its vtable +0x28), its targets released (third, second, first).
    targets::unbind(p, w)?;
    for t in made.iter().rev() {
        release(p, t + 8);
    }
    let mut g = default_context(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = 0;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    program_apply(p, post, w)?;
    program_storage_block(p, post, w, 0, block, 0)?;
    program_storage_block(p, post, w, 1, rec + 0x80, 0)?;
    if grid != 0 {
        let f = p.m.r64(p.m.r64(p.base + GOT_DRAW_ARRAYS));
        p.draw_arrays(f, cmd, 0, 0, grid, 0, 0);
    }
    p.barrier(cmd, 0x21);
    let scenes = p.m.r64(p.m.r64(p.base + SCENE_HOLDER));
    p.call(p.base + GATHER, &[scenes as u64, t2src as u64]);
    release_sampler(p, t2 + 0xea);
    release(p, t2);
    release_sampler(p, t1 + 0xea);
    release(p, t1);
    Ok(())
}
