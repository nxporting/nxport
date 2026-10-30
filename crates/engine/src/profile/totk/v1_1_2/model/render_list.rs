// TotK's render-list loop (main+0x12de2a0) and its per-entry render state (main+0x12de37c), ported. The loop walks a
// gsys render queue: for each entry whose flags match the pass mask it updates the render state cache of the draw
// context (gsys::ModelRenderCache) and calls the drawable's virtual draw (vtable +0x60). Model units
// (main+0x688c68) draw through the model port; other drawables call the game's function.

use super::draw::{self, Port, R};

const PFN_SET_RENDER_ENABLE: usize = 0x466e248;
const MODEL_DRAW: usize = 0x688c68;
const TERRAIN_DRAW: usize = 0x2a47078;
const DISPATCH_DRAW: usize = 0x763718;
const FORWARD_DRAW: usize = 0x2a3f77c;
const STATE_HOOK: usize = 0x27d0468;
const PFN_BARRIER: usize = 0x466e290;
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
        .map(|(f, n)| if f >> 40 != 0 { format!("forwarded main+{:#x} {n}", f & ((1 << 40) - 1)) } else { format!("main+{f:#x} {n}") })
        .collect::<Vec<_>>()
        .join(", ")
}

/// sub_12de2a0(list, count, ctx, mask, _, flags).
pub(super) unsafe fn draw_list(p: &mut Port, list: usize, count: u32, c: usize, mask: u32, flags: u32) -> R<()> {
    for i in 0..count as usize {
        let e = p.m.r64(list + i * 8);
        if p.m.r32(e + 8) & mask == 0 {
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
        // A barrier before the first four views (ORDER_FRAGMENTS | INVALIDATE_TEXTURE, + INVALIDATE_SHADER with a
        // graphics flag).
        0x27d0104 => {
            if index < 4 {
                let flags = if p.m.r8(p.m.r64(p.m.r64(p.base + GRAPHICS_FLAGS_HOLDER)) + 0x50c) & 2 != 0 { 0x112 } else { 0x12 };
                p.call(p.pfn(PFN_BARRIER), &[p.m.r64(p.m.r64(c + 0x20) + 0xb8) as u64, flags]);
            }
        }
        // A list drawn only in pass 0 with scene flags 1, 2 and 6 set.
        0x1229090 => {
            let g = p.m.r64(p.m.r64(p.base + SCENE_HOLDER));
            let f948 = p.m.r32(g + 0x948);
            if !f948 & 6 == 0
                && p.m.r8(g + 0x98) == 0
                && p.m.r8(p.m.r64(g + 0x70) + 0x2c4) & 1 != 0
                && p.m.r8(c + 0x31) == 0
                && (f948 >> 6) & 1 != 0
            {
                note_drawable(f | 1 << 40);
                p.helper("forwarded draw", f, &[obj as u64, index as u64, c as u64])?;
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
            p.call(p.pfn(PFN_SET_RENDER_ENABLE), &[cmd, 1]);
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
                _ => {
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
