// main+0xcc6f38(this, ctx, index): the decal / ambient occlusion part of the scene renderer's frame, one view record
// (+0x1a8, 0x19f0 each, count +0x1a0) with its bit 0 set: the listener's virtual +0x50 (before), the snapshot textures,
// the shadow-mask pass, the decal AO manager, the decal AO point draw, the cube map update, the listener again (after).
// Everything the port checks it may stop at comes before p.m.commit(); the game's own functions (listener, the shadow
// mask begin / draw / end, DecalAoMgr::updateDecal_draw, the cube map faces / backend) are called by the game after it.

use super::draw::{Port, R};
use super::scene_passes::default_state;
use super::vfx::{program_apply, program_storage_block};

const RET: u32 = 0xd65f_03c0;
/// [[var]]: the object whose +0x38 count / +0x40 array hold the program sets (the AO point program at +0xc18).
const PROGRAM_SETS: usize = 0x4639f00;
/// [[var]]: nvnCommandBufferDrawArrays.
const GOT_DRAW_ARRAYS: usize = 0x4613b80;
/// [var]: the render state holder ([[var]] +0x50c bit 1: invalidate texture in the barrier).
const BARRIER_HOLDER: usize = 0x462bbf0;
/// [var]: the default object of the cube map update's source (when the record's buffer has none of its own).
const CUBEMAP_SOURCE_DEFAULT: usize = 0x46312d0;
/// The guard byte ([var]) and storage ([var]) of the static object passed with it.
const CUBEMAP_GUARD: usize = 0x462ba00;
const CUBEMAP_STATIC: usize = 0x462ba08;

const POOL_FLUSH: usize = 0xc7dcf0;
const SHADOW_BEGIN: usize = 0x2236954;
const SHADOW_DRAW: usize = 0xa5175c;
const SHADOW_END: usize = 0x2236bf0;
const DECAL_AO_DRAW: usize = 0x2221318;
const CUBEMAP_FACE: usize = 0xcc7e70;
const CUBEMAP_BACKEND: usize = 0xcc913c;
const CUBEMAP_FINISH: usize = 0xcc9024;

/// The view record `index` (0 when out of range) of this+0x1a8.
unsafe fn record(p: &Port, this: usize, index: u32) -> usize {
    let i = if p.m.r32(this + 0x1a0) <= index { 0 } else { index } as i32 as isize as usize;
    p.m.r64(this + 0x1a8).wrapping_add(i.wrapping_mul(0x19f0))
}

/// The listener (+0x4a00) virtual +0x50 (obj, w, after, this, record): its function unless it is a bare return.
unsafe fn listener(p: &mut Port, this: usize, w: usize, after: u64, rec: usize) -> R<()> {
    let obj = p.m.r64(this + 0x4a00);
    if obj == 0 {
        return Ok(());
    }
    let f = p.vfn(obj, 0x50);
    if p.m.r32(p.base + f) == RET {
        return Ok(());
    }
    p.helper("decal ao listener by the call", f, &[obj as u64, w as u64, after, this as u64, rec as u64])?;
    Ok(())
}

pub(super) unsafe fn decal_ao(p: &mut Port, this: usize, ctx: usize, index: u32) -> R<()> {
    let rec = record(p, this, index);
    let flags = p.m.r8(rec + 0x648);
    if flags & 1 == 0 {
        return Ok(());
    }
    let w = p.m.r64(ctx + 0x58);
    let view = p.m.r8(rec + 8) as u32;
    let bit2 = (flags >> 2) & 1 != 0;

    // Checks of every stage the port runs itself.
    let snaps = snapshot_plan(p, this, bit2)?;
    let shadow = shadow_wanted(p, this + 0xab0, rec, bit2);
    let decal = decal_wanted(p, this);
    let points = points_plan(p, this, view)?;
    let cube = cube_plan(p, this, rec, bit2)?;
    p.m.commit();
    // commit

    listener(p, this, w, 0, rec)?;
    if let Some((mgr, snapshots)) = snaps {
        snapshot_textures(p, mgr, &snapshots, w, view, bit2)?;
    }
    if shadow {
        let obj = this + 0xab0 + 0x5b8;
        p.helper("decal ao shadow begin by the call", SHADOW_BEGIN, &[obj as u64])?;
        p.helper("decal ao shadow draw by the call", SHADOW_DRAW, &[rec as u64, w as u64, 0])?;
        p.helper("decal ao shadow end by the call", SHADOW_END, &[obj as u64, w as u64])?;
    }
    if decal {
        p.helper("decal ao update by the call", DECAL_AO_DRAW, &[p.m.r64(this + 0x1e68) as u64, w as u64])?;
    }
    if let Some((table, elem, prog)) = points {
        decal_points(p, table, elem, prog, w)?;
    }
    if let Some((obj, mgr, source)) = cube {
        cube_update(p, obj, mgr, source, w)?;
    }
    let rec2 = record(p, this, index);
    listener(p, this, w, 1, rec2)?;
    Ok(())
}

unsafe fn root(p: &Port, mut s: usize) -> usize {
    while p.m.r64(s + 0x168) != 0 {
        s = p.m.r64(s + 0x168);
    }
    s
}

/// main+0xcc7380 (snapshot manager, w, view, bit): the manager (+0x4168) when the scene's +0x470 flag is set: the
/// snapshots (count +0x1d0, pointers +0x1d8) drawn (all with the bit, else those whose +0x860 is not 2) and the
/// shared context pointers they are drawn with; each checked here for what main+0xcc75ec (snapshot.rs) cannot do
/// (a copy of another snapshot) or must not stop at (its program still being set up).
unsafe fn snapshot_plan(p: &mut Port, this: usize, bit2: bool) -> R<Option<(usize, Vec<usize>)>> {
    let mgr = p.m.r64(this + 0x4168);
    if mgr == 0 || p.m.r8(p.m.r64(this + 0x3f68) + 0x470) == 0 {
        return Ok(None);
    }
    let n = p.m.i32(mgr + 0x1d0);
    if n == 0 {
        return Ok(Some((mgr, Vec::new())));
    }
    let list = p.m.r64(mgr + 0x1d8);
    let c = context(mgr);
    let mut drawn = Vec::new();
    for i in 0..n.max(0) as usize {
        let s = p.m.r64(list + i * 8);
        if !bit2 && p.m.i32(s + 0x860) == 2 {
            continue;
        }
        if p.m.r8(s + 0x325) as i8 != -1 {
            return Err("snapshot copy");
        }
        let kind = p.m.i32(root(p, s) + 0x860) as isize as usize;
        let shader = if p.m.r8(s + 0x323) == 0 {
            c[3].wrapping_add(kind.wrapping_mul(0x30))
        } else {
            let mut v = p.m.r8(s + 0x323) as i32 * 8;
            v += match p.m.i32(root(p, s) + 0x708) {
                1 => -6,
                2 => -2,
                3 => -4,
                _ => -8,
            };
            if p.m.r8(s + 0x728) == 0 {
                v += 1;
            }
            c[2].wrapping_add((v as isize as usize).wrapping_mul(0x90)).wrapping_add(kind.wrapping_mul(0x30))
        };
        let prog = p.m.r64(shader + 8);
        if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
            return Err("agl program setup pending");
        }
        drawn.push(s);
    }
    Ok(Some((mgr, drawn)))
}

/// The context pointers main+0xcc7380 builds on its stack for each snapshot's draw.
fn context(mgr: usize) -> [usize; 8] {
    [mgr + 0x2638, mgr + 0x28f0, mgr + 0x2910, mgr + 0x4d10, mgr + 0x488, mgr + 0x488, mgr + 0x648, mgr + 0x2c8]
}

/// main+0xcc7380: each snapshot drawn (snapshot.rs), then, with any in the manager, the barrier (order fragments,
/// invalidate texture with the holder's +0x50c bit 1).
unsafe fn snapshot_textures(p: &mut Port, mgr: usize, drawn: &[usize], w: usize, view: u32, _bit2: bool) -> R<()> {
    let c = context(mgr);
    for &s in drawn {
        super::snapshot::draw(p, s, w, view, c.as_ptr() as usize)?;
    }
    if p.m.i32(mgr + 0x1d0) > 0 {
        let bits = if p.m.r8(p.m.r64(p.m.r64(p.base + BARRIER_HOLDER)) + 0x50c) & 2 == 0 { 0x12 } else { 0x112 };
        let cmd = p.m.r64(w + 0xb8) as usize;
        p.barrier(cmd, bits);
    }
    Ok(())
}

/// main+0x cc709c's conditions (shadow mask object `obj` = this+0xab0): the record's bit 2, the object's +0x230 bit 0,
/// its scene's +0x510, the flags +0x5c0 / +0x116c, the three float pairs not out of order (a NaN passes, as the
/// compare's branches) and the record's +0x658 bit 0.
unsafe fn shadow_wanted(p: &Port, obj: usize, rec: usize, bit2: bool) -> bool {
    if !bit2 || p.m.r8(obj + 0x230) & 1 == 0 || p.m.r8(p.m.r64(obj + 8) + 0x510) == 0 {
        return false;
    }
    if p.m.r8(obj + 0x5c0) | p.m.r8(obj + 0x116c) == 0 {
        return false;
    }
    for (a, b) in [(0x1190, 0x119c), (0x1194, 0x11a0), (0x1198, 0x11a4)] {
        if p.m.f32(obj + a) > p.m.f32(obj + b) {
            return false;
        }
    }
    p.m.r8(rec + 0x658) & 1 != 0
}

/// agl::fx::DecalAoMgr::updateDecal_draw(mgr [+0x1e68], w) when the scene (+0x1df8) has +0x610 and +0x1358 set.
unsafe fn decal_wanted(p: &Port, this: usize) -> bool {
    let s = p.m.r64(this + 0x1df8);
    p.m.r8(s + 0x610) != 0 && p.m.r64(this + 0x1e68) != 0 && p.m.r8(s + 0x1358) != 0
}

/// main+0xcc7160's checks: the table (+0x4a18, count +0x18, flags +0x104 with bits 0 and 1 both set, elements of 0x90
/// bytes at +0x20, each with its count at +0x88) and the program of the program set holder's entry +0xc18 (sets
/// count > 9), which must exist, be set up, and the element a block the storage binder knows.
unsafe fn points_plan(p: &mut Port, this: usize, view: u32) -> R<Option<(usize, usize, usize)>> {
    let table = p.m.r64(this + 0x4a18);
    let v = if p.m.r32(table + 0x18) <= view { 0 } else { view } as usize;
    if (!p.m.r8(table + 0x104)) & 3 != 0 {
        return Ok(None);
    }
    let elem = p.m.r64(table + 0x20) + v * 0x90;
    if p.m.i32(elem + 0x88) == 0 {
        return Ok(None);
    }
    let holder = p.m.r64(p.m.r64(p.base + PROGRAM_SETS));
    let prog = if p.m.r32(holder + 0x38) > 9 { p.m.r64(holder + 0x40) + 0xc18 } else { 0 };
    if prog == 0 {
        return Err("decal ao program set");
    }
    if p.m.r32(prog + 0x10) != 0 || p.m.r32(prog + 0x14) != 0 {
        return Err("agl program setup pending");
    }
    p.block_alignment(elem)?;
    match p.vfn(elem, 0x48) {
        0x2a2c078 | 0x2a303b4 => {}
        _ if p.m.stops() => return Err("uniform block virtual function"),
        _ => {}
    }
    Ok(Some((table, elem, prog)))
}

/// main+0xcc7160(table, w, record): the element's pool range flushed, the default state without blending or
/// depth (colour mask 0), the program applied with the element as its storage block 0, one point per entry
/// (count at +0x88), and the barrier of main+0xcc94a8(w, 1) (bits 0x21).
unsafe fn decal_points(p: &mut Port, _table: usize, elem: usize, prog: usize, w: usize) -> R<()> {
    p.cpu_helper(POOL_FLUSH, &[elem as u64, 0]);
    let mut g = default_state(p);
    g[0] &= 0xffff_0000;
    g[1] = 0;
    g[7] = 0;
    p.apply_graphics_context(g.as_ptr() as usize, w);
    program_apply(p, prog, w)?;
    program_storage_block(p, prog, w, 0, elem, 0)?;
    let count = p.m.i32(elem + 0x88);
    let cmd = p.m.r64(w + 0xb8) as usize;
    if count != 0 {
        let f = p.m.r64(p.m.r64(p.base + GOT_DRAW_ARRAYS));
        p.draw_arrays(f, cmd, 0, 0, count, 0, 0);
    }
    p.barrier(cmd, 0x21);
    Ok(())
}

/// main+0xcc726c's conditions (manager at this+0x3f60): the record's bit 2, the manager's agent object +0x1f0, its
/// scene's +0x530, the record's source (+0x5f0, its flags +0x8a: bit 0 or 5, with bit 1 and +0x1f0 its own object
/// +8, else +0x78, else the default) and the static object the source's virtual +0 is asked with (built once by the
/// game: a first call stops here).
unsafe fn cube_plan(p: &mut Port, this: usize, rec: usize, bit2: bool) -> R<Option<(usize, usize, usize)>> {
    let mgr = this + 0x3f60;
    if !bit2 || p.m.r64(mgr + 0x1f0) == 0 || p.m.r8(p.m.r64(mgr + 8) + 0x530) == 0 {
        return Ok(None);
    }
    let b = p.m.r64(rec + 0x5f0);
    let f = p.m.r16(b + 0x8a);
    let own = (f & 1 != 0 || (f >> 5) & 1 != 0) && (f >> 1) & 1 != 0 && p.m.r64(b + 0x1f0) != 0;
    let source = if own {
        p.m.r64(b + 0x1f0) + 8
    } else if p.m.r64(b + 0x78) != 0 {
        p.m.r64(b + 0x78)
    } else {
        p.m.r64(p.base + CUBEMAP_SOURCE_DEFAULT)
    };
    if p.m.r8(p.m.r64(p.base + CUBEMAP_GUARD)) & 1 == 0 {
        return Err("decal ao static object (first call)");
    }
    Ok(Some((p.m.r64(mgr + 0x1f0), mgr, source)))
}

/// main+0xcc726c's call: the source's virtual +0 with the static object, then main+0xcc7498 on the manager's agent
/// with its result.
unsafe fn cube_update(p: &mut Port, agent: usize, _mgr: usize, source: usize, w: usize) -> R<()> {
    let f = p.m.r64(p.m.r64(source)).wrapping_sub(p.base);
    let tex = p.helper("decal ao source by the call", f, &[source as u64, p.m.r64(p.base + CUBEMAP_STATIC) as u64])?;
    cube_agent(p, agent, w, tex as usize)
}

/// main+0xcc7498(agent, w, _, tex): the texture's +0x44 / +0x4c copied to the agent's +0xc60 / +0xc68, the mode
/// +0xc50, then with the agent's +8 bit 2 each of its entries (+0xeb8, count +0xeb0, 0x1c0 bytes): with a target
/// (+0x160), the faces from +0x1b8 up to the step +0x1b9 (at most 6) drawn by main+0xcc7e70, the backend
/// (main+0xcc913c) when the step is past 6 and the entry not done (+0x199), main+0xcc9024 when it is, and the step
/// stored as the new +0x1b8.
unsafe fn cube_agent(p: &mut Port, agent: usize, w: usize, tex: usize) -> R<()> {
    if tex != 0 {
        let a = p.m.r64(tex + 0x44);
        let b = p.m.r32(tex + 0x4c);
        p.m.w::<u32>(agent + 0xc68, b);
        p.m.w::<u64>(agent + 0xc60, a as u64);
    }
    p.m.w::<u32>(agent + 0xc50, if p.m.r8(agent + 0xb) == 1 { 3 } else { 0 });
    if p.m.r8(agent + 8) & 4 == 0 {
        return Ok(());
    }
    let n = p.m.r32(agent + 0xeb0) as usize;
    if n == 0 {
        return Ok(());
    }
    let entries = p.m.r64(agent + 0xeb8);
    let mut count = 0u32;
    for i in 0..n {
        let e = entries + i * 0x1c0;
        if p.m.r64(e + 0x160) == 0 {
            continue;
        }
        let from = p.m.i8(e + 0x1b8) as i32;
        if from < 6 {
            let to = (p.m.i8(e + 0x1b9) as i32).min(6);
            let mut k = from;
            while k < to {
                p.helper("decal ao cube map face by the call", CUBEMAP_FACE, &[agent as u64, w as u64, e as u64, count as u64, k as u32 as u64])?;
                k += 1;
                count = count.wrapping_add(1);
            }
        }
        if p.m.i8(e + 0x1b9) as i32 > 6 && p.m.r8(e + 0x199) == 0 {
            p.helper("decal ao cube map backend by the call", CUBEMAP_BACKEND, &[e as u64, w as u64])?;
        }
        if p.m.r8(e + 0x199) != 0 {
            p.helper("decal ao cube map finish by the call", CUBEMAP_FINISH, &[e as u64])?;
        }
        p.m.w::<u8>(e + 0x1b8, p.m.r8(e + 0x1b9));
    }
    Ok(())
}
