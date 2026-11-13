// TotK's dispatch drawable (main+0x763718), ported: the draw context is set up for the drawable (load_resource), then
// the drawable's draw delegate (at +0x88 + 8) is invoked with {context, drawable, the model's pass record, LOD,
// flags}. The delegate targets are per-type draw functions; the ones not ported call the game.

use super::draw::{load_resource, Port, R};

/// Draws of delegate targets the port does not implement: (main+ target, count).
static TARGETS: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

/// Draws of delegate targets the port draws: (main+ target, count).
static PORTED: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

pub(super) fn ported_report() -> String {
    let mut g = std::mem::take(&mut *PORTED.lock().unwrap());
    g.sort_by(|a, b| b.1.cmp(&a.1));
    g.iter().map(|(f, n)| format!("main+{f:#x} {n}")).collect::<Vec<_>>().join(", ")
}

pub(super) fn report() -> String {
    let mut g = std::mem::take(&mut *TARGETS.lock().unwrap());
    g.sort_by(|a, b| b.1.cmp(&a.1));
    g.iter()
        .take(16)
        .map(|(f, n)| if f >> 40 != 0 { format!("per-view main+{:#x} {n}", f & ((1 << 40) - 1)) } else { format!("main+{f:#x} {n}") }).collect::<Vec<_>>().join(", ")
}

/// main+0x763718(drawable, ctx, lod, flags).
pub(super) unsafe fn draw(p: &mut Port, d: usize, c: usize, lod: u32, flags: u32) -> R<()> {
    if !load_resource(p, c, d)? {
        return Ok(());
    }
    let mut pass = p.m.i8(c + 0x45);
    if pass == -1 {
        pass = p.m.i8(d + p.m.r8(c + 0x31) as usize + 0x20);
    }
    let record = (p.m.r64(d + 0x68) as isize + pass as isize * 0x60 + 0x18) as usize;
    let arg: [u64; 4] = [c as u64, d as u64, record as u64, lod as u64 | (flags as u64) << 32];
    let delegate = p.m.r64(d + 0x88) + 8;
    let target = match p.delegate_target(delegate) {
        Some(None) => return Ok(()),
        Some(Some(t)) => t,
        None => p.vfn(delegate, 0),
    };
    let this = {
        let (obj, adj) = (p.m.r64(delegate + 8), p.m.r64(delegate + 0x18));
        obj.wrapping_add(((adj as i64) >> 1) as usize)
    };
    {
        let mut g = PORTED.lock().unwrap();
        match g.iter_mut().find(|x| x.0 == target) {
            Some(x) => x.1 += 1,
            None => g.push((target, 1)),
        }
    }
    match target {
        0x137c740 => return super::cave::target(p, this, arg.as_ptr() as usize, 1, true),
        0x137c7f4 => return super::cave::target(p, this, arg.as_ptr() as usize, 2, true),
        0x137c95c => return super::cave::target(p, this, arg.as_ptr() as usize, 4, true),
        0xad2978 => return super::cave::target(p, this, arg.as_ptr() as usize, 0, false),
        0x137c5d8 => return super::cave::chunk_target(p, this, arg.as_ptr() as usize, 1, true),
        0x137c68c => return super::cave::chunk_target(p, this, arg.as_ptr() as usize, 2, true),
        0x984294 => return super::cave::chunk_target(p, this, arg.as_ptr() as usize, 0, false),
        0x85ff40 => return super::cave::instanced_target(p, this, arg.as_ptr() as usize, 0, false),
        0x137c31c => return super::cave::instanced_target(p, this, arg.as_ptr() as usize, 1, false),
        0x137c4bc => return super::cave::instanced_target(p, this, arg.as_ptr() as usize, 3, true),
        0x137c3a0 => return super::cave::instanced_target(p, this, arg.as_ptr() as usize, 2, true),
        0x137c8a8 => return super::cave::target(p, this, arg.as_ptr() as usize, 3, true),
        0x137ca10 => return super::cave::target(p, this, arg.as_ptr() as usize, 5, true),
        0x2ad00a0 => return per_view(p, this, arg.as_ptr() as usize),
        0x27da4cc => return super::grid::gate(p, this, arg.as_ptr() as usize),
        0x886c90 => return super::cells::draw(p, this, arg.as_ptr() as usize),
        _ => {}
    }
    note(target);
    p.helper("dispatch draw", p.vfn(delegate, 0), &[delegate as u64, arg.as_ptr() as u64])?;
    Ok(())
}

fn note(f: usize) {
    let mut g = TARGETS.lock().unwrap();
    match g.iter_mut().find(|x| x.0 == f) {
        Some(x) => x.1 += 1,
        None => g.push((f, 1)),
    }
}

/// main+0x2ad00a0(this, arg): the drawable's view slot of the context's view (scene table at +0x70 +0x1c8, two bytes
/// per view); drawn through its virtual +0x30 when the scene is in its main state and the slot's entry is enabled.
unsafe fn per_view(p: &mut Port, this: usize, arg: usize) -> R<()> {
    let g = p.m.r64(p.m.r64(p.base + super::render_list::SCENE_HOLDER));
    let ctx = p.m.r64(arg);
    let views = p.m.r64(g + 0x70);
    let mut k = p.m.r8(ctx + 0x30) as u32;
    if p.m.r32(views + 0x1c0) <= k {
        k = 0;
    }
    let slot = p.m.i8(p.m.r64(views + 0x1c8) + k as usize * 2);
    if slot == -1 || !p.m.r32(g + 0x948) & 6 != 0 || p.m.r32(g + 0x1e8) != 0 {
        return Ok(());
    }
    let t = p.m.r64(this + 0x18);
    if !p.m.r32(t) & 0xc != 0 {
        return Ok(());
    }
    let mut u = slot as i32 as u32;
    if p.m.r32(t + 0xea0) <= u {
        u = 0;
    }
    if p.m.r8((p.m.r64(t + 0xea8) as i64 + u as i32 as i64 * 0xe0 + 0xb4) as usize) == 0 {
        return Ok(());
    }
    let f = p.vfn(this, 0x30);
    if f == 0xd7c1d8 {
        return super::perview::draw(p, this, slot as i32 as u32, arg);
    }
    if f == 0x24b2a04 {
        return super::perview::draw_b(p, this, slot as i32 as u32, arg);
    }
    note(f | 1 << 40);
    p.helper("per-view draw", f, &[this as u64, slot as i64 as u64, arg as u64])?;
    Ok(())
}
