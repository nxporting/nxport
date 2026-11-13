// The effect system's last unported callees: the screen effect renderer's image filter (main+0x122f480) and the two
// shadow systems' effect passes (main+0x222368c, main+0x222377c).

use super::draw::{Port, R};
use super::targets;
use super::texture_slots::barrier_if_slot;
use super::vfx::{draw_cascade, draw_pass};

/// main+0x122f480(r, w, view): the screen effect renderer's image filter: the record of the view (+0x3e8, 0x720 bytes
/// each; view 0 when out of range) has its render buffer (+0x5b0) unbound (main+0xbe19ac) and its texture slot
/// (+0x620) barriered (main+0xbe1950).
pub(super) unsafe fn image_filter(p: &mut Port, r: usize, w: usize, view: u32) -> R<()> {
    let vi = if p.m.r32(r + 0x3e0) > view { view } else { 0 } as usize;
    let rec = p.m.r64(r + 0x3e8).wrapping_add(vi.wrapping_mul(0x720));
    targets::unbind(p, w)?;
    barrier_if_slot(p, rec + 0x620, w);
    Ok(())
}

/// main+0x222368c(sys, w, o, _, flag, _): passes 0x14 and 0x15 of the shadow system: without bit 0 of the flag the
/// ordinary effect passes (main+0xa414ac on the system's views +0x18), else the cascade views of o+0x18 (the second
/// reads o+0x18 again; both take the first's +0x60 as the info).
pub(super) unsafe fn shadow_system_a(p: &mut Port, sys: usize, w: usize, o: usize, flag: u32) -> R<()> {
    if flag & 1 == 0 {
        let views = p.m.r64(sys + 0x18);
        draw_pass(p, o, w, views, 0x14, 0, 0, 0)?;
        return draw_pass(p, o, w, views, 0x15, 0, 0, 0);
    }
    let v = p.m.r64(o + 0x18);
    draw_cascade(p, o, w, 0x14, 1, v, v + 0x160, v + 0x60, u32::MAX)?;
    let v2 = p.m.r64(o + 0x18);
    draw_cascade(p, o, w, 0x15, 1, v2, v2 + 0x160, v + 0x60, u32::MAX)
}

/// main+0x222377c(sys, w, o, _, _, flag, _): passes 0x11 and 0x12, as `shadow_system_a` (cascade mode 2).
pub(super) unsafe fn shadow_system_b(p: &mut Port, sys: usize, w: usize, o: usize, flag: u32) -> R<()> {
    if flag & 1 == 0 {
        let views = p.m.r64(sys + 0x18);
        draw_pass(p, o, w, views, 0x11, 0, 0, 0)?;
        return draw_pass(p, o, w, views, 0x12, 0, 0, 0);
    }
    let v = p.m.r64(o + 0x18);
    draw_cascade(p, o, w, 0x11, 2, v, v + 0x160, v + 0x60, u32::MAX)?;
    let v2 = p.m.r64(o + 0x18);
    draw_cascade(p, o, w, 0x12, 2, v2, v2 + 0x160, v + 0x60, u32::MAX)
}
