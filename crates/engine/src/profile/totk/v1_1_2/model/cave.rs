// TotK's cave quad-mesh renderer, reached from the dispatch drawable's delegate targets main+0x137c740 / 0x137c95c,
// ported: the subsystem draw (main+0xad2a30), the mesh draw (main+0xad2cf8: named uniform blocks and textures, then
// main+0x138ce90's instance loop: per instance culling, uniform updates, per mesh textures and indexed draws). Pure game
// helpers (name lookups, culling tests, parameter blocks) are called as they are; what records commands is ported.

use super::draw::{load_resource, Port, R};

const PFN_UPDATE_UNIFORM_BUFFER: usize = 0x466e228;
const PFN_DRAW_ELEMENTS: usize = 0x466e088;
const PFN_DRAW_ELEMENTS_BASE_VERTEX: usize = 0x466e090;
const PFN_BUFFER_GET_ADDRESS: usize = 0x466d818;
const PFN_DEVICE_GET_TEXTURE_HANDLE: usize = 0x466d428;
const PFN_BIND_TEXTURE: usize = 0x466e008;
const DEVICE_HOLDER: usize = 0x462bbe8;
const TICK_HOLDER: usize = 0x462ab50;
const LOCK: usize = 0x2b15370;
const UNLOCK: usize = 0x2b15380;
const TICK: usize = 0x2b15810;
const SLEEP: usize = 0x2b15bf0;
const YIELD: usize = 0x2b15c00;
const MAKE_READY: usize = 0x7ff1fc;
const PARAMS: usize = 0xad2bfc;
const SLOT_BY_NAME: usize = 0x985798;
const TEXTURE_SLOT_BY_NAME: usize = 0x138cd08;
const DRAW_STATE_INIT: usize = 0x11cbac4;
const IN_FRUSTUM: usize = 0x138d420;
const IN_OCCLUDER: usize = 0x940c88;
const IN_CASCADE: usize = 0x1387a50;
/// Byte table: mesh class + mode * 5 + 30 when a cascade is drawn -> the mesh draws.
const MESH_DRAWS: usize = 0x3810b18;

/// The delegate targets main+0x137c740 (mode 1) and main+0x137c95c (mode 4): mark the subsystem used this frame
/// (+0x40 bit 4, under its lock), and in the main pass draw it.
pub(super) unsafe fn target(p: &mut Port, this: usize, arg: usize, mode: u32) -> R<()> {
    let s = p.m.r64(this + 0x28);
    p.query(p.base + LOCK, &[(s + 0x20) as u64]);
    p.m.w::<u32>(s + 0x40, p.m.r32(s + 0x40) | 0x10);
    p.query(p.base + UNLOCK, &[(s + 0x20) as u64]);
    let ctx = p.m.r64(arg);
    if p.m.r8(ctx + 0x31) == 3 && p.m.r8(p.m.r64(this + 0x28) + 0x13c2) == 0 {
        let s = p.m.r64(this + 0x28);
        p.m.w::<u64>(s + 0x13d0, p.m.r64(this + 0x20) as u64);
        p.m.w::<u32>(s + 0x13e0, p.m.r8(ctx + 0x30) as u32 | p.m.r32(this + 0xe00) << 8);
        subsystem(p, p.m.r64(s + 0x4e8), arg, mode, s + 5000)?;
    }
    Ok(())
}

/// main+0xad2a30(system, arg, mode, info): the two mesh sets (+0x10: drawn; +0x18: an empty draw), each timed.
unsafe fn subsystem(p: &mut Port, sys: usize, arg: usize, mode: u32, info: usize) -> R<()> {
    if p.m.r8(info + 4) == 0 {
        return Ok(());
    }
    for (k, set) in [(0, sys + 0x10), (1, sys + 0x18)] {
        if p.m.r64(set) == 0 {
            continue;
        }
        let t0 = p.query(p.base + TICK, &[]);
        let mut params = [0u8; 0x118];
        let put = |b: &mut [u8; 0x118], o: usize, v: &[u8]| b[o..o + v.len()].copy_from_slice(v);
        put(&mut params, 4, &0x3d4c_cccd_0000_0001u64.to_le_bytes());
        put(&mut params, 0xc, &0x3f80_0000_3f80_0000u64.to_le_bytes());
        put(&mut params, 0x14, &0x7f80_0000_0000_0000u64.to_le_bytes());
        put(&mut params, 0x20, &0x3c23_d70au32.to_le_bytes());
        put(&mut params, 0x28, &0x461c_4000u32.to_le_bytes());
        put(&mut params, 0xb4, &0x0101_0100u32.to_le_bytes());
        put(&mut params, 0xc1, &0x0101_0101u32.to_le_bytes());
        params[0xc5] = 1;
        p.query(p.base + PARAMS, &[params.as_mut_ptr() as u64, info as u64]);
        if k == 0 {
            mesh_draw(p, p.m.r64(set), arg, mode, params.as_ptr() as usize)?;
        }
        let t1 = p.query(p.base + TICK, &[]);
        p.m.w::<u64>(sys + 0x158, t1.wrapping_sub(t0).wrapping_add(p.m.r64(sys + 0x158) as u64));
        p.m.w::<u32>(sys + 0x160, p.m.r32(sys + 0x160).wrapping_add(1));
        if k == 0 && p.m.r8(info + 4) == 0 {
            return Ok(());
        }
    }
    Ok(())
}

/// A name as the game's lookups take it: {pointer, length}.
fn name(s: &'static [u8]) -> [u64; 2] {
    [s.as_ptr() as u64, (s.len() - 1) as u64]
}

/// main+0xad2cf8(this, arg, mode, params).
unsafe fn mesh_draw(p: &mut Port, this: usize, arg: usize, mode: u32, info: usize) -> R<()> {
    let ctx = p.m.r64(arg);
    // The targets draw only in pass 3; pass 7's light setup (sub_8fd5cc, float arguments) is not ported.
    if p.m.r8(ctx + 0x31) == 7 {
        return Err("cave mesh draw: light pass");
    }
    let w = p.m.r64(ctx + 0x20);
    let w_cell = [w as u64];
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    let program = p.m.r64(p.m.r64(arg + 0x10));
    let mut user = [0xffff_ffffu32; 4];
    for (k, n) in [&b"gsys_user0\0"[..], b"gsys_user1\0", b"gsys_user2\0", b"gsys_user3\0"].iter().enumerate() {
        let nm = name(n);
        p.query(p.base + SLOT_BY_NAME, &[(&mut user[k]) as *mut u32 as u64, program as u64, nm.as_ptr() as u64]);
    }
    for (k, (block, addr)) in [(0x198, 0x210), (0x318, 0x390), (0x3d8, 0x450), (0x498, 0x510)].iter().enumerate() {
        bind_named_block(p, this + block, w, p.m.r64(this + addr) as u64, user[k])?;
    }
    let undefined = b"Undefined\0";
    let mut normals: [u64; 2] = [0xffff_ffff, undefined.as_ptr() as u64];
    let mut weights: [u64; 2] = [0xffff_ffff, undefined.as_ptr() as u64];
    for (out, n) in [(&mut normals, &b"cave_qm_normals\0"[..]), (&mut weights, b"cave_qm_material_weights_ao\0")] {
        let nm = name(n);
        p.query(p.base + TEXTURE_SLOT_BY_NAME, &[out.as_mut_ptr() as u64, program as u64, nm.as_ptr() as u64]);
    }
    let view = p.m.r8(ctx + 0x46) as usize;
    let views = p.m.r64(p.m.r64(ctx + 0x10) + 0x18);
    let mut frustum = views + view * 0x430 + 0x60;
    let shadow = p.m.r64(info + 0x30);
    let (mut cascade, mut cascade_index) = (0usize, 0u32);
    if shadow != 0 && p.m.r8(ctx + 0x31) == 8 && view > 4 && view < 9 {
        let mut k = p.m.r8(ctx + 0x30) as u32;
        if p.m.r32(shadow + 0xd20) <= k {
            k = 0;
        }
        cascade = p.m.r64(shadow + 0xd28) + k as usize * 0xb48;
        cascade_index = view as u32 - 5;
    }
    let slots: [u64; 3] = [w_cell.as_ptr() as u64, normals.as_ptr() as u64, weights.as_ptr() as u64];
    let mut state = [0u8; 0x240];
    p.query(p.base + DRAW_STATE_INIT, &[state.as_mut_ptr() as u64]);
    let occluder = if p.m.r8(info + 0xb6) != 0 { p.m.r64(info + 0xb8) } else { 0 };
    let culled = p.m.r8(info + 0xb5) != 0;
    if !culled {
        frustum = 0;
        cascade = 0;
    }
    let n = instances(p, this, w, mode, frustum, occluder, cascade, cascade_index, &slots, info)?;
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(n / 3));
    Ok(())
}

/// main+0x138728c(block, w, &addr, slots): the uniform block object at its address per stage (its virtual +0x48).
unsafe fn bind_named_block(p: &mut Port, obj: usize, w: usize, addr: u64, slots: u32) -> R<()> {
    if slots == 0xffff_ffff {
        return Ok(());
    }
    let size = p.m.r32(obj + 0x38);
    let cmd = p.m.r64(w + 0xb8);
    for (stage, sh) in [(0, 0), (1, 8), (2, 16), (5, 24)] {
        if (slots >> sh) & 0xff != 0xff {
            p.bind_block(obj, cmd, stage, (slots >> sh) as i8 as i32, addr, size)?;
        }
    }
    Ok(())
}

/// main+0x138ce90: the instances of the info's group (+0x40) from last to first; per instance the culling test, its
/// material (0x100 bytes) and transform (0x30) uniform updates, then per mesh its uniform update, its two named
/// textures and its draw. Returns the indices drawn.
#[allow(clippy::too_many_arguments)]
unsafe fn instances(
    p: &mut Port,
    this: usize,
    w: usize,
    mode: u32,
    frustum: usize,
    occluder: usize,
    cascade: usize,
    cascade_index: u32,
    slots: &[u64; 3],
    info: usize,
) -> R<u32> {
    let groups = p.m.r64(this + 0xb8);
    let ngroups = p.m.i32(this + 0xc4);
    let want = p.m.i32(info + 0x40);
    let Some(g) = (0..ngroups.max(0) as usize).map(|k| groups + k * 12).find(|&g| p.m.i32(g) == want) else {
        return Ok(0);
    };
    let (start, end) = (p.m.i32(g + 4) as i64, p.m.i32(g + 8) as i64);
    if start >= end {
        return Ok(0);
    }
    let cmd = p.m.r64(w + 0xb8);
    let (mut drawn, mut last_mat, mut last_xf) = (0u32, -1i32, -1i32);
    let mut i = end;
    loop {
        i -= 1;
        'instance: {
            if mode >= 6 {
                break 'instance;
            }
            let rec = p.m.r64(this + 0x78) + i as usize * 0x20;
            let class = p.m.i32(rec + 0x1c);
            let ok = match mode {
                0 => true,
                1 => class == 0,
                4 => class == 2,
                5 => class >= 3,
                _ => class == 1,
            };
            if !ok {
                break 'instance;
            }
            let bounds = p.m.r64(this + 0x98) + i as usize * 0x18;
            if cascade == 0 {
                if frustum != 0 {
                    if p.m.f32(bounds).is_nan() {
                        break 'instance;
                    }
                    let mut v = p.query(p.base + IN_FRUSTUM, &[frustum as u64, bounds as u64, (bounds + 0xc) as u64]) & 1;
                    if v != 0 && occluder != 0 {
                        v = p.query(p.base + IN_OCCLUDER, &[occluder as u64, bounds as u64]) & 1;
                    }
                    if v == 0 {
                        break 'instance;
                    }
                }
            } else if p.query(p.base + IN_CASCADE, &[cascade as u64, bounds as u64, cascade_index as u64]) & 1 == 0 {
                break 'instance;
            }
            let mat = p.m.i32(rec);
            if last_mat != mat {
                let src = (p.m.r64(this + 0x18) as i64 + mat as i64 * 0x100) as u64;
                p.call(p.pfn(PFN_UPDATE_UNIFORM_BUFFER), &[cmd as u64, p.m.r64(this + 0x390) as u64, p.m.r64(this + 0x308) as u64, 0, 0x100, src]);
                last_mat = mat;
            }
            let xf = p.m.i32(rec + 4);
            if last_xf != xf {
                let src = (p.m.r64(this + 0x38) as i64 + xf as i64 * 0x30) as u64;
                p.call(p.pfn(PFN_UPDATE_UNIFORM_BUFFER), &[cmd as u64, p.m.r64(this + 0x450) as u64, p.m.r64(this + 0x3c8) as u64, 0, 0x30, src]);
                last_xf = xf;
            }
            let (first, last) = (p.m.i32(rec + 0x14), p.m.i32(rec + 0x18));
            if first >= last {
                break 'instance;
            }
            for m in first..last {
                let src = (p.m.r64(this + 0x518) as i64 + m as i64 * 0x30) as u64;
                p.call(p.pfn(PFN_UPDATE_UNIFORM_BUFFER), &[cmd as u64, p.m.r64(this + 0x510) as u64, p.m.r64(this + 0x488) as u64, 0, 0x30, src]);
                let mesh = p.m.r64(this + 0xac8) + m as usize * 0x18;
                let k = p.m.i32(mesh + 0xc) as i64 + mode as i64 * 5 + (cascade != 0) as i64 * 0x1e;
                if p.m.r8((p.base as i64 + MESH_DRAWS as i64 + k) as usize) == 0 {
                    continue;
                }
                let tex = p.m.r64(mesh + 0x10);
                let normals = p.m.i32(slots[1] as usize);
                if normals != -1 {
                    texture_ready(p, tex + 0xe4, tex)?;
                    bind_texture(p, p.m.r64(slots[0] as usize), normals as u32, tex + 0x58, tex + 0xea);
                }
                let weights = p.m.i32(slots[2] as usize);
                if weights != -1 {
                    texture_ready(p, tex + 0x1d4, tex + 0xf0)?;
                    bind_texture(p, p.m.r64(slots[0] as usize), weights as u32, tex + 0x148, tex + 0x1da);
                }
                let count = p.m.i32(mesh);
                if count != 0 {
                    let base_vertex = p.m.i32(mesh + 8);
                    let prim = p.m.r32(this + 0x584) as u64;
                    let offset = p.m.r32(mesh + 4);
                    let addr = p
                        .query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[(this + 0x5a8) as u64])
                        .wrapping_add(p.m.r32(this + 0x5a4).wrapping_mul(offset) as u64);
                    let itype = p.m.r32(this + 0x580) as u64;
                    if base_vertex == 0 {
                        p.m.w::<u8>(cmd + 0x10, 1);
                        p.call(p.pfn(PFN_DRAW_ELEMENTS), &[cmd as u64, prim, itype, count as u32 as u64, addr]);
                    } else {
                        p.call(
                            p.pfn(PFN_DRAW_ELEMENTS_BASE_VERTEX),
                            &[cmd as u64, prim, itype, count as u32 as u64, addr, base_vertex as u32 as u64],
                        );
                    }
                }
                drawn = drawn.wrapping_add(count as u32);
            }
        }
        if i <= start {
            return Ok(drawn);
        }
    }
}

/// The texture at `make`: its ready flag cleared first (another thread making it ready: waited for; not ready yet:
/// made ready here).
unsafe fn texture_ready(p: &mut Port, flag: usize, make: usize) -> R<()> {
    if p.m.r32(flag) == 0 {
        return Ok(());
    }
    if p.m.predict() {
        return Err("cave texture ready wait");
    }
    let old = p.m.fetch_or32(flag, 0x8000_0000);
    if old & 0x7fff_ffff == 0 {
        p.m.w::<u32>(flag, 0);
    } else if (old as i32) < 0 {
        let step = (p.m.r64(p.m.r64(p.base + TICK_HOLDER)) as i64 / 1_000_000) as u64;
        while p.m.i32(flag) < 0 {
            p.call(p.base + SLEEP, &[step]);
            p.call(p.base + YIELD, &[]);
        }
        (*(flag as *const std::sync::atomic::AtomicU32)).store(0, std::sync::atomic::Ordering::Release);
    } else {
        p.call(p.base + MAKE_READY, &[make as u64]);
        p.m.w::<u32>(flag, 0);
    }
    Ok(())
}

/// main+0xcc9f14(w, slots, &texture id, &sampler id): the texture per stage of the packed slots.
unsafe fn bind_texture(p: &mut Port, w: usize, slots: u32, tid: usize, sid: usize) {
    let (t, s) = (p.m.i32(tid), p.m.r16(sid) as i16 as i32);
    if t == -1 || s == -1 {
        return;
    }
    let dev = p.m.r64(p.m.r64(p.m.r64(p.base + DEVICE_HOLDER)) + 0x30);
    let handle = p.query(p.pfn(PFN_DEVICE_GET_TEXTURE_HANDLE), &[dev as u64, t as u32 as u64, s as u32 as u64]);
    let cmd = p.m.r64(w + 0xb8) as u64;
    for (stage, sh) in [(0u64, 0), (1, 8), (2, 16), (5, 24)] {
        if (slots >> sh) & 0xff != 0xff {
            p.call(p.pfn(PFN_BIND_TEXTURE), &[cmd, stage, (slots >> sh) as i8 as i64 as u64, handle]);
        }
    }
}
