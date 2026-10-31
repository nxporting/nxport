// TotK's occlusion query items, main+0xa6c3a4(q, w, view, flags): per item seen by `view` (+10 view mask), a sample
// counter reset, the item's own box (its virtual +0x28 decides; +0x30 / +0x38 / +0x40 draw it) or the shared box
// (volume shape +0x968 of the light shapes, index stream +0x728), and a report into the item's buffer (+0x10, slot
// +8 * 0x20). Called by the sky pass, the decal / occlusion drawable and others.

use super::draw::{Port, R};
use super::lights::{shape_state, stream_draw, SHAPES, STREAMS};
use super::vfx::{program_apply, program_block};

const PFN_SET_POINT_SIZE: usize = 0x466e198;
const PFN_RESET_COUNTER: usize = 0x466e238;
const PFN_REPORT_COUNTER: usize = 0x466e230;
/// The item class drawn here: its program getter (virtual +0x30), setup (+0x38, agl::VertexAttribute::activate in the
/// symbol match) and draw (+0x40).
const ITEM_PROGRAM: usize = 0x2a3819c;
const ITEM_SETUP: usize = 0x2208e68;
const ITEM_DRAW: usize = 0x2208ff8;

/// Item virtual functions the port does not run: (main+ function | slot << 40, count).
static CALLS: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

pub(super) fn report() -> String {
    let mut g = std::mem::take(&mut *CALLS.lock().unwrap());
    g.sort_by(|a, b| b.1.cmp(&a.1));
    g.iter().map(|(f, n)| format!("+{:#x} main+{:#x} {n}", f >> 40, f & ((1 << 40) - 1))).collect::<Vec<_>>().join(", ")
}

fn note(f: usize, slot: usize) {
    let mut g = CALLS.lock().unwrap();
    let k = f | slot << 40;
    match g.iter_mut().find(|x| x.0 == k) {
        Some(x) => x.1 += 1,
        None => g.push((k, 1)),
    }
}

pub(super) unsafe fn draw(p: &mut Port, q: usize, w: usize, view: u32, flags: u64) -> R<()> {
    let v = if p.m.r32(q + 0x98) <= view { 0 } else { view } as usize;
    let data = p.m.r64(q + 0xa0) + v * 0x98;
    let cmd = p.m.r64(w + 0xb8);
    p.flush();
    p.query_f(p.pfn(PFN_SET_POINT_SIZE), &[cmd as u64], &[p.m.f32(q + 0xa8)]);
    let items = p.m.r64(q + 0x10);
    let mut last = 0usize;
    for k in 0..p.m.r32(q + 8) as usize {
        let it = p.m.r64(items + k * 8);
        if (1u32 << (view & 31)) & p.m.r16(it + 10) as u32 == 0 {
            continue;
        }
        if flags & 1 != 0 {
            p.call(p.pfn(PFN_RESET_COUNTER), &[cmd as u64, 1]);
        }
        let f = p.vfn(it, 0x28);
        if p.constant_result(f) != Some(0) {
            note(f, 0x28);
            return Err("occlusion item");
        }
        let (get, setup, draw) = (p.vfn(it, 0x30), p.vfn(it, 0x38), p.vfn(it, 0x40));
        if get != ITEM_PROGRAM || setup != ITEM_SETUP || draw != ITEM_DRAW {
            for (f, slot) in [(get, 0x30), (setup, 0x38), (draw, 0x40)] {
                note(f, slot);
            }
            return Err("occlusion item draw");
        }
        let prog = p.query(p.base + ITEM_PROGRAM, &[it as u64]) as usize;
        if prog != last {
            // main+0x2208e68: the program, its block of this view, the box shape's vertex states and buffers.
            program_apply(p, prog, w)?;
            program_block(p, prog, w, 0, data + 0x18, 0)?;
            shape_state(p, p.m.r64(p.m.r64(p.base + SHAPES)) + 0x5a8, w);
            last = p.query(p.base + ITEM_PROGRAM, &[it as u64]) as usize;
        }
        // main+0x2208ff8: the item's element of the items' block (+0x18) at the program's slots of index 3, then
        // its box (index stream 0x2268 + 0x60 * kind +9, kinds above 2 as 0).
        if p.m.r32(prog + 0x14) != 0 {
            return Err("agl program setup pending");
        }
        let slots = p.m.r32(p.m.r64(prog + 0x50) + if p.m.r32(prog + 0x48) < 4 { 0 } else { 0x30 });
        p.bind_uniform_block(q + 0x18, w, slots, k as i32)?;
        let kind = match p.m.r8(it + 9) {
            k @ 0..=2 => k as usize,
            _ => 0,
        };
        stream_draw(p, p.m.r64(p.m.r64(p.base + STREAMS)) + kind * 0x60 + 0x2268, w);
        if flags & 1 != 0 {
            let at = p.buffer_address_of(it + 0x10) + p.m.r8(it + 8) as u64 * 0x20;
            p.call(p.pfn(PFN_REPORT_COUNTER), &[cmd as u64, 1, at]);
        }
    }
    p.call(p.pfn(super::render_list::PFN_BARRIER), &[cmd as u64, 1]);
    Ok(())
}
