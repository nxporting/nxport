// agl::RenderBuffer::bind, main+0x7ab9a4(buffer, w): the buffer's colour targets (+0x20, 8) and depth target (+0x60)
// bound as render targets (texture ids at +0x60 into the device's texture table, views at +0xb8), with the context's
// current buffer and name updated, then the first-use clears. The previous buffer's unbind and targets another thread
// is setting up stop: left to the game.

use super::draw::{Port, R};

const PFN_SET_RENDER_TARGETS: usize = 0x466e258;
/// The first-use clear colour (four floats).
const CLEAR_COLOR: usize = 0x3840174;
const HOLDER: usize = 0x462bbf0;
const EMPTY_NAME: usize = 0x374f715;
const VIEW_SETUP: usize = 0x2a2f550;
const VIEW_SETUP_ALL_LEVELS: usize = 0x2a2f89c;
const PFN_VIEW_SET_DEFAULTS: usize = 0x466d9b0;
const PFN_VIEW_SET_LEVELS: usize = 0x466d9b8;
const PFN_VIEW_SET_LAYERS: usize = 0x466d9c0;
/// The context type the wrapper is cast to (initialised once, guard byte bit 0).
pub(super) const CONTEXT_TYPE_GUARD: usize = 0x4634f30;
pub(super) const CONTEXT_TYPE: usize = 0x4634f38;

pub(super) unsafe fn bind(p: &mut Port, rb: usize, w: usize) -> R<()> {
    bind_prefix(p, rb, w)?;
    set_targets(p, rb, w, 0)
}

/// The previous buffer's unbind check and the context's current buffer and name.
unsafe fn bind_prefix(p: &mut Port, rb: usize, w: usize) -> R<()> {
    if (p.m.r8(w + 0x108) >> 1) & 1 != 0 {
        let prev = p.m.r64(w + 0xf8);
        if prev != 0 {
            return Err("render buffer unbind");
        }
        p.m.w::<u64>(w + 0x100, 0);
    }
    let targets: [usize; 9] = std::array::from_fn(|k| p.m.r64(rb + 0x20 + k * 8));
    let target = |k: usize| targets[k];
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
    Ok(())
}

/// The subclass bind main+0x8974e8(buffer, w): only the context's +0xf0 updated, then the same targets and clears.
pub(super) unsafe fn bind_plain(p: &mut Port, rb: usize, w: usize) -> R<()> {
    if p.m.r8(p.m.r64(p.base + CONTEXT_TYPE_GUARD)) & 1 == 0 {
        return Err("context type not initialised");
    }
    let g = if w == 0 { 0 } else { p.query(p.base + p.vfn(w, 0), &[w as u64, p.m.r64(p.base + CONTEXT_TYPE) as u64]) as usize };
    if g != w {
        return Err("render buffer bind (context)");
    }
    p.m.w::<u64>(w + 0xf0, rb as u64);
    set_targets(p, rb, w, 0)
}

/// Setup state +0xc8 (bit 31 a lock): set flags rebuild the target's view (its virtual +0x18, main+0x2a2f550:
/// nvnTextureView defaults, level +0xce, layer +0xcc), then it is cleared; another thread's lock is left to the game.
unsafe fn finish_setup(p: &mut Port, t: usize) -> R<()> {
    if p.m.r32(t + 0xc8) != 0 {
        let old = p.m.fetch_or32(t + 0xc8, 0x8000_0000);
        if (old as i32) < 0 {
            return Err("render target locked");
        }
        if old & 0x7fff_ffff != 0 {
            // Colour targets view one level, the other class (main+0x2a2f89c) every level from it.
            let levels = match p.vfn(t, 0x18) {
                VIEW_SETUP => 1,
                VIEW_SETUP_ALL_LEVELS => 0,
                _ => return Err("render target setup"),
            };
            let v = (t + 0xb8) as u64;
            p.query(p.pfn(PFN_VIEW_SET_DEFAULTS), &[v]);
            p.query(p.pfn(PFN_VIEW_SET_LEVELS), &[v, p.m.r8(t + 0xce) as u64, levels]);
            p.query(p.pfn(PFN_VIEW_SET_LAYERS), &[v, p.m.r16(t + 0xcc) as u64, 1]);
        }
        p.m.w::<u32>(t + 0xc8, 0);
    }
    Ok(())
}

unsafe fn set_targets(p: &mut Port, rb: usize, w: usize, skip: u32) -> R<()> {
    let holder = p.m.r64(p.m.r64(p.base + HOLDER));
    let targets: [usize; 9] = std::array::from_fn(|k| p.m.r64(rb + 0x20 + k * 8));
    let target = |k: usize| targets[k];
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
        finish_setup(p, t)?;
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
        p.push(cmd, crate::nvn::cmd::ModelItem::Targets { colors, depth });
        return first_use(p, holder, &targets, cmd, skip);
    }
    p.call(
        p.pfn(PFN_SET_RENDER_TARGETS),
        &[cmd as u64, count as u64, textures.as_ptr() as u64, views.as_ptr() as u64, textures[8], views[8]],
    );
    first_use(p, holder, &targets, cmd, skip)
}

/// With bit 6 of the holder's +0x780, a target's first use (its texture table entry's +4 bit 0 set, bit 1 not yet)
/// is cleared when its report (the holder's +0x798 buffer at the entry's [[+0x10]] +0x84 * 0x20) is set, else only
/// marked used (bit 1). A clear runs under conditional rendering on the holder's +0x7f8 report and is reported
/// (counter 0x10); after any clear, rendering is enabled again. Only the mode with bit 20 set (bit 23 clear, +0x784
/// = 4, +0x782 bits 5 and 7 clear) is ported, from the disassembly; others stop.
/// Targets in `skip` (colour bits 0..7, depth bits 8 / 9) are cleared by the caller: only marked used.
unsafe fn first_use(p: &mut Port, holder: usize, targets: &[usize; 9], cmd: usize, skip: u32) -> R<()> {
    let mode = p.m.r32(holder + 0x780);
    let mut cleared = false;
    for (k, &t) in targets.iter().enumerate() {
        if t == 0 {
            continue;
        }
        let skipped = if k < 8 { (skip >> k) & 1 != 0 } else { skip & 0x300 != 0 };
        if skipped {
            let mut i = (p.m.i32(t + 0x60) as u32).wrapping_sub(p.m.r16(holder + 0x5a2) as u32);
            if p.m.r32(holder + 0x530) <= i {
                i = 0;
            }
            let e = (p.m.r64(holder + 0x538) as i64 + i as i32 as i64 * 0x20) as usize;
            if p.m.r32(e + 4) & 1 != 0 {
                p.m.fetch_or32(e + 4, 2);
            }
            continue;
        }
        if (mode >> 6) & 1 == 0 {
            continue;
        }
        let mut i = (p.m.i32(t + 0x60) as u32).wrapping_sub(p.m.r16(holder + 0x5a2) as u32);
        if p.m.r32(holder + 0x530) <= i {
            i = 0;
        }
        let e = (p.m.r64(holder + 0x538) as i64 + i as i32 as i64 * 0x20) as usize;
        let flags = p.m.r32(e + 4);
        if flags & 1 == 0 || flags & 2 != 0 {
            continue;
        }
        if (mode >> 20) & 1 == 0 {
            return Err("render target first use (mode)");
        }
        let rb = p.m.r64(holder + 0x798);
        if rb == 0 {
            return Err("render target first use (no reports)");
        }
        let off = (p.m.r16(p.m.r64(p.m.r64(e + 0x10)) + 0x84) as u32) << 5;
        let pool = p.m.r64(rb + 0x110);
        let base = if pool != 0 { p.m.r64(holder + 0x790).wrapping_sub(p.m.r64(pool + 0x18)) } else { 0 };
        if p.m.r64(p.m.r64(rb + 0x100) + (base as u32).wrapping_add(off) as usize) == 0 {
            p.m.fetch_or32(e + 4, 2);
            continue;
        }
        let report = (p.m.r64(holder + 0x7f8) + off as usize) as u64;
        p.render_enable_conditional(cmd, 1, report);
        let flags2 = p.m.r8(holder + 0x782);
        if flags2 >> 7 != 0 {
            return Err("render target first use (mode)");
        }
        if k < 8 {
            p.clear_color(cmd, k as i32, p.base + CLEAR_COLOR, 0xf);
        } else {
            let mask = if p.m.r8(t + 0xa8).wrapping_sub(0x5b) <= 1 { 0xff } else { 0 };
            p.clear_depth(cmd, 1.0, true, 0, mask);
        }
        if (mode >> 23) & 1 == 0 {
            if p.m.r8(holder + 0x784) != 4 {
                return Err("render target first use (mode)");
            }
            p.report_counter(cmd, 0x10, report);
        }
        if (flags2 >> 5) & 1 != 0 {
            return Err("render target first use (mode)");
        }
        cleared = true;
    }
    if cleared && mode & 0x30_0000 == 0x10_0000 {
        p.render_enable(cmd, true);
    }
    Ok(())
}

/// main+0xc26458(rb, w): a barrier for each of the render buffer's targets (+0x20..+0x60) that has storage (+0x60 not
/// -1): ORDER_FRAGMENTS, or ORDER_PRIMITIVES with graphics settings +0x780 bit 5, plus INVALIDATE_TEXTURE and, with
/// +0x50c bit 1, INVALIDATE_SHADER.
pub(super) unsafe fn barrier_targets(p: &mut Port, rb: usize, w: usize) {
    let bits = barrier_bits(p);
    let cmd = p.m.r64(w + 0xb8);
    for o in (0x20..=0x60).step_by(8) {
        let t = p.m.r64(rb + o);
        if t != 0 && p.m.i32(t + 0x60) != -1 {
            p.barrier(cmd, bits);
        }
    }
}

/// The barrier the game puts after rendering into a target to be sampled.
pub(super) unsafe fn barrier_bits(p: &Port) -> u32 {
    let s = p.m.r64(p.m.r64(p.base + HOLDER));
    let order = if p.m.r8(s + 0x780) & 0x20 == 0 { 2 } else { 1 };
    let invalidate = if (p.m.r8(s + 0x50c) >> 1) & 1 == 0 { 0x10 } else { 0x110 };
    invalidate | order
}

/// main+0x98a150(rb, w, index, mask, color, stencil, viewport, depth in s0): clears the render buffer's color target
/// `index` (mask bit 0, all channels) and its depth / stencil (bits 1 / 2) within the viewport, where the buffer has
/// those targets. A target still being set up (+0xc8) is left to the game.
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn clear(p: &mut Port, rb: usize, w: usize, index: i32, mask: u32, color: usize, stencil: i32, vp: usize, depth: f32) -> R<()> {
    let target = rb + 0x20 + index as isize as usize * 8;
    let mut m = mask;
    if p.m.r64(target) == 0 {
        m &= !1;
    }
    if p.m.r64(rb + 0x60) == 0 {
        m &= !6;
    }
    if m == 0 {
        return Ok(());
    }
    for o in (0x20..=0x60).step_by(8) {
        let t = p.m.r64(rb + o);
        if t != 0 && p.m.r32(t + 0xc8) != 0 {
            return Err("render target still being set up");
        }
    }
    super::viewport::apply(p, vp, w, rb);
    let cmd = p.m.r64(w + 0xb8);
    if m & 1 != 0 {
        p.clear_color(cmd, index, color, 0xf);
    }
    if m & 6 != 0 {
        p.clear_depth(cmd, depth, (m >> 1) & 1 != 0, stencil, if (m >> 2) & 1 != 0 { 0xff } else { 0 });
    }
    Ok(())
}

/// main+0x8660b4(buffer, w, viewport, _, mask, values): agl::RenderBuffer::bind, then the targets in `mask` cleared
/// within the viewport: colour target k (bit k) to the four floats at values + k * 16, depth / stencil (bits 8 / 9)
/// to values +0x80 / +0x84 with the first clear. A previous buffer still bound (its unbind) stops.
pub(super) unsafe fn bind_clear(p: &mut Port, rb: usize, w: usize, vp: usize, mask: u32, values: usize) -> R<()> {
    bind_prefix(p, rb, w)?;
    set_targets(p, rb, w, mask)?;
    let mut ds = if p.m.r64(rb + 0x60) != 0 { (mask >> 7) & 6 } else { 0 };
    for k in 0..8usize {
        let color = p.m.r64(rb + 0x20 + k * 8) != 0 && (mask >> k) & 1 != 0;
        if !color && ds == 0 {
            continue;
        }
        for o in (0x20..=0x60).step_by(8) {
            let t = p.m.r64(rb + o);
            if t != 0 {
                finish_setup(p, t)?;
            }
        }
        super::viewport::apply(p, vp, w, rb);
        let cmd = p.m.r64(w + 0xb8);
        if color {
            p.clear_color(cmd, k as i32, values + k * 16, 0xf);
        }
        if ds != 0 {
            p.clear_depth(cmd, p.m.f32(values + 0x80), (ds >> 1) & 1 != 0, p.m.i32(values + 0x84), if (ds >> 2) & 1 != 0 { 0xff } else { 0 });
        }
        ds = 0;
    }
    Ok(())
}
