// TotK's dispatch drawable (main+0x763718), ported: the draw context is set up for the drawable (load_resource), then
// the drawable's draw delegate (at +0x88 + 8) is invoked with {context, drawable, the model's pass record, LOD,
// flags}. The delegate targets are per-type draw functions; the ones not ported call the game.

use super::draw::{load_resource, Port, R};

/// Draws of delegate targets the port does not implement: (main+ target, count).
static TARGETS: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

pub(super) fn report() -> String {
    let mut g = std::mem::take(&mut *TARGETS.lock().unwrap());
    g.sort_by(|a, b| b.1.cmp(&a.1));
    g.iter().take(16).map(|(f, n)| format!("main+{f:#x} {n}")).collect::<Vec<_>>().join(", ")
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
    match target {
        0x137c740 => return super::cave::target(p, this, arg.as_ptr() as usize, 1),
        0x137c95c => return super::cave::target(p, this, arg.as_ptr() as usize, 4),
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
