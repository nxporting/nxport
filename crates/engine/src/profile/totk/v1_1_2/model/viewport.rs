// sead::Viewport::apply (main+0x74474c(viewport, w, frame buffer)): the viewport (+8 x, +0xc y, +0x10 / +0x14 the far
// corner, +0x18 orientation 1..5, +0x1c / +0x20 depth range) mapped into the frame buffer's region (+8 / +0xc size,
// +0x10..+0x1c region), set as scissor and viewport, then the depth range. Operation order as in the game (no fused
// multiply-adds; float to integer conversions truncate and saturate as Rust's `as` does).

use super::draw::Port;

const PFN_SET_SCISSOR: usize = 0x466e110;
const PFN_SET_VIEWPORT: usize = 0x466e0f8;
const PFN_SET_DEPTH_RANGE: usize = 0x466e120;

pub(super) unsafe fn apply(p: &mut Port, vp: usize, w: usize, fb: usize) {
    let f = |o: usize| p.m.f32(o);
    let (x, y) = (f(vp + 8), f(vp + 0xc));
    let (x2, y2) = (f(vp + 0x10), f(vp + 0x14));
    let (fw, fh) = (f(fb + 8), f(fb + 0xc));
    let mode = p.m.r32(vp + 0x18).wrapping_sub(1);
    let (mut a, mut b) = (y, x);
    match mode {
        0 => (a, b) = ((fh - (x2 - x)) - x, y),
        1 => (a, b) = (x, (fw - (y2 - y)) - y),
        2 => (a, b) = ((fh - (y2 - y)) - y, (fw - (x2 - x)) - x),
        3 => b = (fw - (x2 - x)) - x,
        4 => a = (fh - (y2 - y)) - y,
        _ => {}
    }
    let (r0, r1, r2, r3) = (f(fb + 0x10), f(fb + 0x14), f(fb + 0x18), f(fb + 0x1c));
    let hr = r3 - r1;
    let wr = r2 - r0;
    let x0 = r0 + (b / fw) * wr;
    let (hy, wx) = (y2 - y, x2 - x);
    let (sa, sb) = if mode > 1 { (hy, wx) } else { (wx, hy) };
    let ya = r1 + (a / fh) * hr;
    let hs = hr * (sa / fh);
    let xe = x0 + wr * (sb / fw);
    let y0 = (hr - hs) - ya;
    let rx = x0 as i32;
    let ry = y0 as i32;
    let rw = (xe - (x0 as i32) as f32) as u32 as i32;
    let rh = ((hs + y0) - (y0 as i32) as f32) as u32 as i32;
    let cmd = p.m.r64(w + 0xb8);
    let (near, far) = (f(vp + 0x1c), f(vp + 0x20));
    if p.native {
        return p.push(cmd, crate::nvn::cmd::ModelItem::Viewport { x: rx, y: ry, w: rw, h: rh, near, far });
    }
    p.call(p.pfn(PFN_SET_SCISSOR), &[cmd as u64, rx as u64, ry as u64, rw as u64, rh as u64]);
    p.call(p.pfn(PFN_SET_VIEWPORT), &[cmd as u64, rx as u64, ry as u64, rw as u64, rh as u64]);
    p.flush();
    let set: unsafe extern "C" fn(u64, f32, f32) = std::mem::transmute(p.pfn(PFN_SET_DEPTH_RANGE));
    set(cmd as u64, near, far);
}

/// The viewport's vtable pointer and the frame's orientation.
const VIEWPORT_VTABLE: usize = 0x46323b0;
const ORIENTATION: usize = 0x46312d8;

/// sead::Viewport's constructor from a frame buffer of size (w, h): the region from the origin to (w, h) (swapped for
/// orientations 1 and 2, unset for unknown ones), the frame's orientation, depth range 0..1. As words for `apply`.
pub(super) unsafe fn full(p: &Port, w: f32, h: f32) -> [u32; 10] {
    let orientation = p.m.i32(p.m.r64(p.base + ORIENTATION));
    let span = |v: f32| if v <= 0.0 { (v, 0.0) } else { (0.0, v) };
    let (x, y, x2, y2) = match orientation as u32 {
        0 | 3..=5 => {
            let ((x, x2), (y, y2)) = (span(w), span(h));
            (x, y, x2, y2)
        }
        1 | 2 => {
            let ((x, x2), (y, y2)) = (span(h), span(w));
            (x, y, x2, y2)
        }
        _ => (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX),
    };
    let mut vp = [0u32; 10];
    let vt = (p.m.r64(p.base + VIEWPORT_VTABLE) + 0x10) as u64;
    vp[0] = vt as u32;
    vp[1] = (vt >> 32) as u32;
    for (n, v) in [x, y, x2, y2].into_iter().enumerate() {
        vp[2 + n] = v.to_bits();
    }
    vp[6] = orientation as u32;
    vp[7] = 0f32.to_bits();
    vp[8] = 1f32.to_bits();
    vp
}
