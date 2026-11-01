// TotK's render-list loop (main+0x12de2a0) and its per-entry render state (main+0x12de37c), ported. The loop walks a
// gsys render queue: for each entry whose flags match the pass mask it updates the render state cache of the draw
// context (gsys::ModelRenderCache) and calls the drawable's virtual draw (vtable +0x60). Model units
// (main+0x688c68) draw through the model port; other drawables call the game's function.

use super::draw::{self, Port, R};

const MODEL_DRAW: usize = 0x688c68;
const TERRAIN_DRAW: usize = 0x2a47078;
const DISPATCH_DRAW: usize = 0x763718;
const FORWARD_DRAW: usize = 0x2a3f77c;
const STATE_HOOK: usize = 0x27d0468;
pub(super) const PFN_BARRIER: usize = 0x466e290;
/// Holders of global game objects (pointers to the pointer).
const GRAPHICS_FLAGS_HOLDER: usize = 0x462bbf0;
pub(super) const SCENE_HOLDER: usize = 0x462bc08;

/// Draws of drawables the port does not draw: (main+ draw function, count).
static DRAWABLES: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

fn note_drawable(f: usize) {
    let mut g = DRAWABLES.lock().unwrap();
    match g.iter_mut().find(|x| x.0 == f) {
        Some(x) => x.1 += 1,
        None => g.push((f, 1)),
    }
}

/// The drawables the port left to the game since the last report, most frequent first.
pub(super) fn drawables_report() -> String {
    let mut g = std::mem::take(&mut *DRAWABLES.lock().unwrap());
    g.sort_by(|a, b| b.1.cmp(&a.1));
    g.iter()
        .take(12)
        .map(|(f, n)| match f >> 40 {
            1 => format!("forwarded main+{:#x} {n}", f & ((1 << 40) - 1)),
            2 => format!("state hook main+{:#x} {n}", f & ((1 << 40) - 1)),
            _ => format!("main+{f:#x} {n}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// sub_12de2a0(list, count, ctx, mask, _, flags).
pub(super) unsafe fn draw_list(p: &mut Port, list: usize, count: u32, c: usize, mask: u32, flags: u32) -> R<()> {
    draw_list_of(p, list, count, c, mask, None, flags)
}

/// The list loop; with `only` (main+0xe46da0) just the entries whose +0xc is that model index.
unsafe fn draw_list_of(p: &mut Port, list: usize, count: u32, c: usize, mask: u32, only: Option<u32>, flags: u32) -> R<()> {
    for i in 0..(count as i32).max(0) as usize {
        let e = p.m.r64(list + i * 8);
        if p.m.r32(e + 8) & mask == 0 {
            continue;
        }
        if only.is_some_and(|m| p.m.r16(e + 0xc) as u32 != m) {
            continue;
        }
        entry_state(p, c, e)?;
        let d = p.m.r64(e + 0x10);
        let sub = ((p.m.r16(e + 0xe) >> 6) & 7) as u32;
        let f = p.vfn(d, 0x60);
        if f == MODEL_DRAW {
            draw::draw(p, d, c, sub, flags)?;
        } else if f == TERRAIN_DRAW {
            super::terrain::draw(p, d, c, sub, flags)?;
        } else if f == DISPATCH_DRAW {
            super::dispatch::draw(p, d, c, sub, flags)?;
        } else if f == FORWARD_DRAW {
            // main+0x2a3f77c: forwards to its object's virtual function +0x2f8 (object, index at +0x18, context).
            let obj = p.m.r64(d + 8);
            let target = p.vfn(obj, 0x2f8);
            forwarded(p, target, obj, p.m.r16(d + 0x18) as u32, c)?;
        } else {
            note_drawable(f);
            p.helper("drawable draw", f, &[d as u64, c as u64, sub as u64, flags as u64])?;
        }
    }
    Ok(())
}

/// The forwarding drawable's target `f`(obj, index, ctx).
unsafe fn forwarded(p: &mut Port, f: usize, obj: usize, index: u32, c: usize) -> R<()> {
    match f {
        0x27e04b8 | 0x26ae888 => {}
        0x9c8d00 => super::vfx::forward(p, obj, index, c)?,
        0xed7a84 => super::vfx::decal_forward(p, obj, index, c)?,
        // A barrier before the first four views (ORDER_FRAGMENTS | INVALIDATE_TEXTURE, + INVALIDATE_SHADER with a
        // graphics flag).
        0x27d0104 => {
            if index < 4 {
                let flags = if p.m.r8(p.m.r64(p.m.r64(p.base + GRAPHICS_FLAGS_HOLDER)) + 0x50c) & 2 != 0 { 0x112 } else { 0x12 };
                p.barrier(p.m.r64(p.m.r64(c + 0x20) + 0xb8), flags);
            }
        }
        // In pass 0 with scene flags 1, 2 and 6 set: per item of its list the scene's +0x8f8 hook main+0x12291e4 (an
        // empty function) and a reset of the core's scene block; no commands (the game's own call does the resets).
        0x1229090 => {
            let g = p.m.r64(p.m.r64(p.base + SCENE_HOLDER));
            let f948 = p.m.r32(g + 0x948);
            if !f948 & 6 == 0
                && p.m.r8(g + 0x98) == 0
                && p.m.r8(p.m.r64(g + 0x70) + 0x2c4) & 1 != 0
                && p.m.r8(c + 0x31) == 0
                && (f948 >> 6) & 1 != 0
            {
                p.cpu_helper(f, &[obj as u64, index as u64, c as u64]);
            }
        }
        _ => {
            note_drawable(f | 1 << 40);
            p.helper("forwarded draw", f, &[obj as u64, index as u64, c as u64])?;
        }
    }
    Ok(())
}

/// sub_12de37c(ctx, entry): the entry's render state. The context caches the last entry's state bits; on a change it
/// derives the cull / depth / stencil settings of its sead::GraphicsContext (ctx+0x128) and applies it.
unsafe fn entry_state(p: &mut Port, c: usize, e: usize) -> R<()> {
    let mut u8_ = p.m.r16(e + 0xe) as u32;
    let bit = (u8_ >> 4) & 1;
    let mut u9: u32;
    if bit as i32 == p.m.i8(c + 0x3f) as i32 {
        u9 = p.m.r8(c + 0x3e) as u32;
    } else {
        p.m.w::<u8>(c + 0x3f, bit as u8);
        if p.m.r64(c + 0x100) != 0 {
            let cmd = p.m.r64(p.m.r64(c + 0x20) + 0xb8) as u64;
            p.render_enable(cmd as usize, true);
            p.m.w::<u32>(c + 0x100, 0);
            p.m.w::<u32>(c + 0x104, 0);
        }
        let mut u13 = p.m.r8(c + 0x3d) as u32;
        if (p.m.r16(c + 0x40) >> 0xb) & 1 != 0 {
            u13 |= 3;
        }
        u13 |= p.m.r8(c + 0x32) as u32;
        let b5 = p.m.r8(c + 0x195) as u32;
        let u14 = u13 & 4;
        if (b5 & 1) != u14 >> 2 || (u14 != 0) as u32 != (b5 & 2) >> 1 || (u14 != 0) as u32 != (b5 & 4) >> 2 {
            p.m.w::<u8>(c + 0x195, (b5 & 0xfe | u14 >> 2) as u8);
        }
        let c10: u8 = match u13 & 3 {
            0 => 3,
            1 => {
                if u13 & 8 == 0 {
                    2
                } else {
                    1
                }
            }
            2 => {
                if u13 & 8 != 0 {
                    2
                } else {
                    1
                }
            }
            _ => 0,
        };
        if c10 != p.m.r8(c + 0x18d) {
            p.m.w::<u8>(c + 0x18d, c10);
        }
        let k = (u13 >> 4) & 3;
        let c10 = if k != 1 { 2u8 } else { 1 };
        let c3 = if k != 2 { c10 } else { 0 };
        if c3 != p.m.r8(c + 0x193) || c3 != p.m.r8(c + 0x194) {
            p.m.w::<u8>(c + 0x193, c3);
            p.m.w::<u8>(c + 0x194, c3);
        }
        p.m.w::<u16>(c + 0xc6, 0);
        p.m.w::<u8>(c + 0x3c, 8);
        let v = p.m.r16(c + 0x40) & 0x3fff;
        p.m.w::<u16>(c + 0x40, v);
        u9 = 0xff;
        p.m.w::<u8>(c + 0x3e, 0xff);
        for o in [0x64, 0x68, 0x5c, 0x60] {
            p.m.w::<u32>(c + o, 0xffff_ffff);
        }
        p.m.w::<u16>(c + 0x42, 0xff);
        for o in [0x78, 0x88, 0x98, 0xa8] {
            p.m.w::<u32>(c + o, 0xffff_ffff);
        }
        for o in [0x108, 0x10c, 0x110, 0x114] {
            p.m.w::<u32>(c + o, 0);
        }
        u8_ = p.m.r8(e + 0xe) as u32;
    }
    let u13 = p.m.r32(c + 0x4c) & p.m.r32(e + 8);
    let u4 = if u13 != 0 { (u8_ & 0xf) | 0xff80 } else { u8_ & 0xf };
    if u9 != u4 & 0xff {
        let u14 = p.m.r32(c);
        p.m.w::<u8>(c + 0x3e, u4 as u8);
        let u8b = u4 & 6;
        let mut b6 = u13 == 0 && u8b == 0;
        let f40 = p.m.r16(c + 0x40);
        let v = (f40 & 0xfff0) | (f40 & 6) | (((u14 & 0xc) == 0) as u16) << 3 | (u8b != 0 && (u14 & 0x20000) == 0) as u16;
        p.m.w::<u16>(c + 0x40, v);
        let mut u11: u8 = if u13 == 0 { 4 } else { 3 };
        match p.m.r8(c + 0x31) {
            4 => {
                b6 = false;
                u11 = 6;
            }
            5 => {
                u11 = 4;
                b6 = true;
            }
            _ => {}
        }
        let mut u12: u8 = if p.m.r32(c + 0x54) != 0 { u11 } else { 2 };
        if (u14 >> 5) & 1 != 0 {
            b6 = false;
            u12 = 8;
        }
        if (u14 >> 2) & 1 != 0 {
            b6 = true;
        }
        let v = p.m.r32(c + 0x54);
        p.m.w::<u32>(c + 0x144, v);
        p.m.w::<u8>(c + 0x18c, u12);
        p.m.w::<u8>(c + 0x196, ((u14 >> 4) & 1) as u8);
        p.m.w::<u8>(c + 0x128, (((u14 & 0x20) >> 5) ^ 1) as u8);
        p.m.w::<u8>(c + 0x129, ((u14 & 8) == 0 && b6) as u8);
        let mut s = p.m.r8(c + 0x44) as u32;
        if u8b == 0 && u14 & 0xf800 == 0 {
            s = 0;
        }
        p.m.w::<u32>(c + 0x12c, s);
        let (a, b) = (p.m.r64(c + 0xb4), p.m.r64(c + 0xbc));
        p.m.w::<u64>(c + 0x130, a as u64);
        p.m.w::<u64>(c + 0x138, b as u64);
        // The render context's state hook (an object at +0x570 of the context's owner): asked, then called.
        let hook = p.m.r64(c + 0x10) + 0x570;
        let ask = p.vfn(hook, 0x10);
        if p.query(p.base + ask, &[hook as u64]) & 1 != 0 {
            let arg: [u64; 6] = [
                p.m.r64(c + 0x20) as u64,
                2 | (p.m.r32(c + 0xb0) as u64) << 32,
                c as u64,
                (c + 0x128) as u64,
                p.m.r64(c + 0x10) as u64,
                0,
            ];
            match p.delegate_target(hook) {
                Some(None) => {}
                Some(Some(STATE_HOOK)) => state_hook(p, p.m.r32(c + 0xb0), c + 0x128),
                Some(Some(0x27d0464)) => {}
                t => {
                    note_drawable(match t { Some(Some(f)) => f, _ => 0x1_0000_0000 | p.vfn(hook, 0) } | 1 << 41);
                    p.helper("render state hook", p.vfn(hook, 0), &[hook as u64, arg.as_ptr() as u64])?;
                }
            }
        }
        let u13 = if p.m.r8(c + 0x31) == 8 {
            (p.m.i8(c + 0x47) as i32 + 4) as u32
        } else if u4 & 8 != 0 {
            3
        } else if u8b != 0 {
            2
        } else {
            1
        };
        if u13 != p.m.r8(c + 0x3c) as u32 {
            p.m.w::<u8>(c + 0x3c, u13 as u8);
            let l1 = p.m.r64(c + 0x10) + (u13 as i32 as isize as usize).wrapping_mul(8);
            p.m.w::<u32>(c + 0x188, 0);
            let (x, y) = (p.m.r32(l1 + 0x4e4), p.m.r32(l1 + 0x4e0));
            p.m.w::<u32>(c + 0x180, x);
            p.m.w::<u32>(c + 0x184, y);
        }
        if p.m.r32(c) & 0x10000 == 0 {
            p.apply_graphics_context(c + 0x128, p.m.r64(c + 0x20));
        }
        p.m.w::<u32>(c + 0xa8, 0xffff_ffff);
    }
    Ok(())
}

/// main+0x27d0468, the render state hook of the context in use (its argument's kind is always 2 here): two context
/// modes force the colour targets on, a blend setup and depth function 5.
unsafe fn state_hook(p: &mut Port, mode: u32, g: usize) {
    if mode.wrapping_sub(4) <= 4 {
        return;
    }
    match mode {
        0x12 => {
            p.m.w::<u32>(g + 4, 0xff);
            p.m.w::<u32>(g + 0x28, 0x0202_0202);
        }
        0x11 => {
            p.m.w::<u32>(g + 4, 0xff);
            p.m.w::<u32>(g + 0x28, 0x0202_0202);
            p.m.w::<u16>(g + 0x2c, 0x0303);
        }
        _ => return,
    }
    p.m.w::<u8>(g + 0x64, 5);
}

const PASS_GLOBALS_HOLDER: usize = 0x4631f68;
const WORKER_RINGS_HOLDER: usize = 0x4639fb8;
const CONTEXT_VTABLE: usize = 0x462cdf0;

/// The pass listener at `v`+0x570 (the render context's state hook, a sead::Delegate1; virtual +0x10: bound) for an
/// event of `kind` other than 2: the hooks seen here act only on kind 2 (main+0x27d0468) or never (main+0x27d0464).
pub(super) unsafe fn pass_event(p: &Port, v: usize) -> R<()> {
    let l = v + 0x570;
    if p.query(p.base + p.vfn(l, 0x10), &[l as u64]) & 1 == 0 {
        return Ok(());
    }
    match p.delegate_target(l) {
        Some(None) | Some(Some(STATE_HOOK)) | Some(Some(0x27d0464)) => Ok(()),
        Some(Some(f)) => super::vfx::callback(p.base + f, p.base),
        None => super::vfx::callback(p.base + p.vfn(l, 0), p.base),
    }
}

/// main+0x931960(renderer, w, args, view, range, flag, out, user): one model pass. Builds the draw context (0x1c0
/// bytes) the render list reads from the pass arguments and the view (+0x558 color, +0x5d0 mask, per-view entries at
/// +0x18), the line width and point size, then draws the renderer's list range. Only the plain list variant
/// (main+0x12de2a0) is ported.
pub(super) unsafe fn model_pass(p: &mut Port, r: usize, w: usize, a: usize, v: usize, start: u32, count: i32, flag: u32, out: usize, user: u64) -> R<()> {
    let mut fl = p.m.r32(a + 0xc);
    let mut kind = p.m.r8(v + 9) as u32;
    if fl & 0x100000 != 0 {
        kind = 1;
    }
    let mut bits = ((fl >> 19) & 4) | ((fl >> 15) & 8);
    match kind & 0xff {
        1 => bits |= 0x10,
        2 => bits |= 0x20,
        _ => {}
    }
    let color = if fl & 0x4000 != 0 { a + 0x38 } else { v + 0x558 };
    if bits & 0x30 != 0 {
        fl |= 0x4000;
    }
    if p.m.r8(v + 0xb) & 8 != 0 {
        fl |= 0x28000;
    }
    let mut buf = vec![0u64; 0x1c0 / 8];
    let c = buf.as_mut_ptr() as usize;
    p.m.own(c, 0x1c0);
    let m = &mut p.m;
    m.w::<u32>(c, fl);
    m.w::<u32>(c + 4, m.r32(a));
    m.w::<u32>(c + 8, m.r32(a + 0x14));
    m.w::<u32>(c + 0xc, m.r32(a + 0x28));
    m.w::<u64>(c + 0x10, v as u64);
    m.w::<u64>(c + 0x18, (v + 0x20) as u64);
    m.w::<u64>(c + 0x20, w as u64);
    m.w::<u8>(c + 0x30, m.r8(v + 8));
    m.w::<u8>(c + 0x31, m.r8(a + 4));
    m.w::<u8>(c + 0x32, bits as u8);
    m.w::<u8>(c + 0x3f, 0xff);
    m.w::<u32>(c + 0x40, 0xff0000);
    m.w::<u32>(c + 0x45, u32::MAX);
    m.w::<u64>(c + 0x54, 0xffff_ffff);
    m.w::<i64>(c + 0x5c, -1);
    m.w::<i64>(c + 0x64, -1);
    let vt = (m.r64(p.base + CONTEXT_VTABLE) + 0x10) as u64;
    for o in [0x70, 0x80, 0x90, 0xa0] {
        m.w::<u64>(c + o, vt);
    }
    for o in [0x78, 0x88, 0x98, 0xa8, 0xb0, 0x144, 0x198] {
        m.w::<u32>(c + o, u32::MAX);
    }
    m.w::<u16>(c + 0xc4, m.r16(v + 0x4d8));
    m.w::<u64>(c + 0x148, 0xffff_ffff_0000_0000);
    m.w::<u64>(c + 0x1a8, 0xffff_ffff_0000_0000);
    m.w::<u64>(c + 0x1b0, user);
    for o in [0x130, 0x138] {
        m.w::<u64>(c + o, 0x3f80_0000_3f80_0000);
    }
    m.w::<u16>(c + 0x128, 0x101);
    m.w::<u16>(c + 0x18d, 0x502);
    m.w::<u32>(c + 0x18f, 0x0101_0101);
    m.w::<u16>(c + 0x193, 0x202);
    m.w::<u8>(c + 0x18c, 4);
    // Eight 6-byte blend entries from +0x150.
    let (first, s, d): (u32, u8, u8) = if fl & 0x4000 != 0 {
        (0x6464_6161, 0x61, 0x64)
    } else if fl & 0x2000 != 0 {
        (0x0202_0202, 2, 2)
    } else if fl & 0x1000 != 0 {
        (0x0101_0909, 9, 1)
    } else if fl & 0x8000 != 0 {
        (0x0606_0105, 5, 6)
    } else {
        (0x0606_0505, 5, 6)
    };
    m.w::<u32>(c + 0x150, first);
    m.w::<u16>(c + 0x154, 0x101);
    for k in 1..8 {
        let e = c + 0x150 + k * 6;
        m.w::<[u8; 6]>(e, [s, s, d, d, 1, 1]);
    }
    // Write mask.
    let mask_at = if p.m.r32(a + 0x20) & 1 == 0 { v + 0x5d0 } else { a + 0x1c };
    let mode = p.m.r8(a + 4) as u32;
    let (all, wide) = match mode {
        8 | 1 | 2 => ((fl & 0x80) != 0, fl & 0x200 != 0),
        4 => ((fl & 0x40 == 0) || fl & 0x80 != 0, fl & 0x200 != 0),
        _ => ((fl & 0xc0) != 0x40, true),
    };
    let sel: u64 = match (wide, all) {
        (false, true) => 0x7777_7777,
        (false, false) => 0,
        (true, true) => 0xffff_ffff,
        (true, false) => 0x8888_8888,
    };
    p.m.w::<u64>(c + 0x54, (p.m.r32(mask_at) as u64 & sel) | 1 << 32);
    let mut f40 = p.m.r16(c + 0x40) as u32;
    if fl & 0x1000_0000 == 0 {
        f40 |= 0x1f0;
    }
    f40 &= !0x200;
    if flag & 1 != 0 {
        f40 |= 0x200;
    }
    if p.m.r8(v + 0xb) & 4 != 0 || fl & 0x80000 != 0 {
        f40 |= 0x800;
    }
    p.m.w::<u16>(c + 0x40, f40 as u16);
    let globals = p.m.r64(p.m.r64(p.base + PASS_GLOBALS_HOLDER));
    p.m.w::<u64>(c + 0xc8, (globals + 0x488) as u64);
    p.m.w::<u8>(c + 0x3e, 0xff);
    let views = |p: &Port, i: u32| p.m.r64(v + 0x18) + (if p.m.r32(v + 0x10) > i { i } else { 0 }) as usize * 0x430;
    let ai = p.m.r32(a + 8);
    if ai != 0xff {
        let i = ai & 0xff;
        p.m.w::<u8>(c + 0x46, i as u8);
        let e = views(p, i);
        p.m.w::<u8>(e + 0x421, p.m.r8(e + 0x421) | 1);
        p.m.w::<u64>(c + 0x28, (e + 0x390) as u64);
        let keep = p.m.r8(v + 0xb) & 2 != 0 && p.m.r8(e + 0x420) & 2 != 0 && p.m.r8(c + 3) & 1 == 0;
        let f40 = p.m.r16(c + 0x40);
        p.m.w::<u16>(c + 0x40, if keep { f40 | 0x400 } else { f40 & !0x400 });
        let e = views(p, p.m.r8(c + 0x46) as u32);
        let t = if p.m.r8(v + 0xb) & 0x10 != 0 { p.m.r64(e + 0x288) } else { 0 };
        p.m.w::<u64>(c + 0x120, t as u64);
        p.m.w::<u32>(c + 0x38, p.m.r32(e + 0x290));
    }
    let (c0, c1, c2, c3) = (p.m.f32(color), p.m.f32(color + 4), p.m.f32(color + 8), p.m.f32(color + 12));
    p.m.w::<f32>(c + 0xc0, c3);
    p.m.w::<f32>(c + 0xb4, c0 * c3);
    p.m.w::<f32>(c + 0xb8, c3 * c1);
    p.m.w::<f32>(c + 0xbc, c3 * c2);
    let rings = p.m.r64(p.m.r64(p.base + WORKER_RINGS_HOLDER)) + super::perview::worker(p) as usize * 0x18;
    p.m.w::<u64>(c + 0x1a0, p.m.r64(rings + 0x20) as u64);
    p.m.w::<u32>(c + 0x1a8, p.m.r32(rings + 0x28).wrapping_add(p.m.r32(rings + 0x18).wrapping_sub(p.m.r32(rings + 0x20))));
    p.m.w::<u8>(c + 0x44, p.m.r8(a + 0x24));
    p.m.w::<u8>(c + 0x45, p.m.r8(a + 0x2c));
    p.m.w::<u32>(c + 0xb0, p.m.r32(a + 0x30));
    p.m.w::<u8>(c + 0x47, p.m.r8(a + 0x34));
    p.m.w::<u8>(c + 0x48, p.m.r8(a + 0x4c));
    let e = views(p, ai);
    let k = p.m.r8(e + 0x3d2) as u32 >> 4;
    let k = if p.m.r32(e + 0x410) > k { k } else { 0 };
    p.m.w::<u64>(c + 0xd0, p.m.r64(p.m.r64(e + 0x418) + k as usize * 8) as u64);
    let s = p.m.r64(v + 0x590);
    let k = if s != 0 { p.m.r8(s + 0x15) as u32 & 3 } else { 0 };
    let k = if p.m.r32(v + 0x440) > k { k } else { 0 };
    p.m.w::<u64>(c + 0xd8, p.m.r64(p.m.r64(v + 0x448) + k as usize * 8) as u64);
    let k = p.m.r32(a + 0x18);
    if k != u32::MAX {
        let k = if p.m.r32(v + 0x460) > k { k } else { 0 };
        let r6 = p.m.r64(v + 0x468) + k as usize * 0x18;
        let j = p.m.r8(r6 + 0x10) as u32;
        let j = if p.m.r32(r6) > j { j } else { 0 };
        p.m.w::<u64>(c + 0xe0, p.m.r64(p.m.r64(r6 + 8) + j as usize * 8) as u64);
    }
    if fl & 2 == 0 {
        p.m.w::<u32>(c + 0x4c, p.m.r32(a + 0x48));
    }
    if fl & 0x800_0000 != 0 {
        p.m.w::<u32>(c + 0x50, p.m.r32(a + 0x48));
    }
    pass_event(p, v)?;
    let base_flags: u32 = if fl & 0x200_0000 == 0 { 0xc1 } else { 0x80 };
    let mut lf = if p.m.r8(c + 0x46) as i32 == p.m.r8(v + 0xc) as i8 as i32 { base_flags | 2 } else { base_flags };
    let cmd = p.m.r64(w + 0xb8) as u64;
    p.line_width(cmd as usize, p.m.f32(v + 0x568));
    p.point_size(cmd as usize, p.m.f32(v + 0x56c));
    if fl & 0x400_0000 != 0 {
        lf |= 8;
        if p.m.r8(v + 0xd) == 0xff {
            p.m.w::<u8>(v + 0xd, p.m.r32(a + 8) as u8);
        }
    }
    let same = p.m.r32(a + 8) == p.m.r8(v + 0xd) as i8 as i32 as u32;
    let x28 = if same { (fl & 0x800_0000) >> 26 } else { 0 };
    let x23 = if fl & 0x40_0000 != 0 {
        if p.m.r8(v + 0xa) & 1 != 0 { 8 } else { (p.m.r8(r + 8) as u32) << 3 & 8 }
    } else {
        0
    };
    let o = a + 0x50;
    let odd = p.query(p.base + p.vfn(o, 0x10), &[o as u64]) & 1;
    let variant = x23 | x28 | ((p.m.r32(a + 0x10) != 0xffff) as u32) << 2 | odd as u32;
    let count = if count == -1 { p.m.r32(r + 0x2c).wrapping_sub(start) } else { count as u32 };
    let only = match variant {
        0 => None,
        4 => Some(p.m.r32(a + 0x10)),
        _ => return Err("model list variant"),
    };
    let drawn = draw_list_of(p, p.m.r64(r + 0x20) + start as usize * 8, count, c, p.m.r32(a), only, lf);
    let (count_out, enable) = (p.m.r32(c + 0x34), p.m.r64(c + 0x100));
    p.m.disown(c);
    drop(buf);
    drawn?;
    pass_event(p, v)?;
    if out != 0 {
        p.m.w::<u32>(out, count_out);
    }
    if enable != 0 {
        p.render_enable(p.m.r64(w + 0xb8), true);
    }
    Ok(())
}
