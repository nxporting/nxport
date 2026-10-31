// agl::RenderBuffer::bind, main+0x7ab9a4(buffer, w): the buffer's colour targets (+0x20, 8) and depth target (+0x60)
// bound as render targets (texture ids at +0x60 into the device's texture table, views at +0xb8), with the context's
// current buffer and name updated. The previous buffer's unbind, targets another thread is setting up and the
// first-use clears (with bit 6 of the graphics holder's +0x780) stop: left to the game.

use super::draw::{Port, R};

const PFN_SET_RENDER_TARGETS: usize = 0x466e258;
const HOLDER: usize = 0x462bbf0;
const EMPTY_NAME: usize = 0x374f715;
const VIEW_SETUP: usize = 0x2a2f550;
const PFN_VIEW_SET_DEFAULTS: usize = 0x466d9b0;
const PFN_VIEW_SET_LEVELS: usize = 0x466d9b8;
const PFN_VIEW_SET_LAYERS: usize = 0x466d9c0;

pub(super) unsafe fn bind(p: &mut Port, rb: usize, w: usize) -> R<()> {
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 {
        let prev = p.m.r64(w + 0xf8);
        if prev != 0 {
            return Err("render buffer unbind");
        }
        p.m.w::<u64>(w + 0x100, 0);
    }
    let holder = p.m.r64(p.m.r64(p.base + HOLDER));
    let targets: [usize; 9] = std::array::from_fn(|k| p.m.r64(rb + 0x20 + k * 8));
    let target = |k: usize| targets[k];
    // With bit 6 of +0x780, a target's first use (its texture table entry's +4 bit 0 set, bit 1 not yet) is cleared
    // after the bind: left to the game.
    if (p.m.r32(holder + 0x780) >> 6) & 1 != 0 {
        for k in 0..9 {
            let t = target(k);
            if t == 0 {
                continue;
            }
            let mut i = (p.m.i32(t + 0x60) as u32).wrapping_sub(p.m.r16(holder + 0x5a2) as u32);
            if p.m.r32(holder + 0x530) <= i {
                i = 0;
            }
            let flags = p.m.r32((p.m.r64(holder + 0x538) as i64 + i as i32 as i64 * 0x20) as usize + 4);
            if flags & 1 != 0 && flags & 2 == 0 {
                return Err("render target first use");
            }
        }
    }
    // The name: the last colour target's, the depth target's only without colour targets, else "Empty".
    let mut name = p.base + EMPTY_NAME;
    for k in 0..7 {
        if target(k) != 0 {
            name = p.m.r64(target(k) + 0xb0);
        }
    }
    if target(7) != 0 || (target(8) != 0 && (0..7).all(|k| target(k) == 0)) {
        let t = if target(7) != 0 { target(7) } else { target(8) };
        name = p.m.r64(t + 0xb0);
    }
    p.m.w::<u64>(w + 0x110, name as u64);
    p.m.w::<u64>(w + 0xf0, rb as u64);
    p.m.w::<u64>(w + 0xf8, rb as u64);
    p.m.w::<u8>(w + 0x108, p.m.r8(w + 0x108) | 2);
    let mut textures = [0u64; 9];
    let mut views = [0u64; 9];
    let mut count = 0i32;
    for k in 0..9 {
        let t = target(k);
        if t == 0 || p.m.i32(t + 0x60) == -1 {
            continue;
        }
        let mut i = (p.m.i32(t + 0x60) as u32).wrapping_sub(p.m.r16(holder + 0x5a2) as u32);
        if p.m.r32(holder + 0x530) <= i {
            i = 0;
        }
        textures[k] = p.m.r64((p.m.r64(holder + 0x538) as i64 + i as i32 as i64 * 0x20) as usize + 0x18) as u64;
        // Setup state +0xc8 (bit 31 a lock): set flags rebuild the target's view (its virtual +0x18, main+0x2a2f550:
        // nvnTextureView defaults, level +0xce, layer +0xcc), then it is cleared; another thread's lock is left to the
        // game.
        if p.m.r32(t + 0xc8) != 0 {
            let old = p.m.fetch_or32(t + 0xc8, 0x8000_0000);
            if (old as i32) < 0 {
                return Err("render target locked");
            }
            if old & 0x7fff_ffff != 0 {
                if p.vfn(t, 0x18) != VIEW_SETUP {
                    return Err("render target setup");
                }
                let v = (t + 0xb8) as u64;
                p.query(p.pfn(PFN_VIEW_SET_DEFAULTS), &[v]);
                p.query(p.pfn(PFN_VIEW_SET_LEVELS), &[v, p.m.r8(t + 0xce) as u64, 1]);
                p.query(p.pfn(PFN_VIEW_SET_LAYERS), &[v, p.m.r16(t + 0xcc) as u64, 1]);
            }
            p.m.w::<u32>(t + 0xc8, 0);
        }
        views[k] = (t + 0xb8) as u64;
        if k < 8 {
            count = k as i32 + 1;
        }
    }
    let cmd = p.m.r64(w + 0xb8);
    if p.native {
        let (colors, depth) = crate::nvn::cmd::render_targets(
            count,
            textures.as_ptr() as *const usize,
            views.as_ptr() as *const usize,
            textures[8] as usize,
            views[8] as *const u8,
        );
        return {
            p.push(cmd, crate::nvn::cmd::ModelItem::Targets { colors, depth });
            Ok(())
        };
    }
    p.call(
        p.pfn(PFN_SET_RENDER_TARGETS),
        &[cmd as u64, count as u64, textures.as_ptr() as u64, views.as_ptr() as u64, textures[8], views[8]],
    );
    Ok(())
}
