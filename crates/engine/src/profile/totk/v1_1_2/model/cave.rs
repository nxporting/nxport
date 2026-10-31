// TotK's cave quad-mesh renderer, reached from the dispatch drawable's delegate targets main+0x137c740 / 0x137c95c,
// ported: the subsystem draw (main+0xad2a30), the mesh draw (main+0xad2cf8: named uniform blocks and textures, then
// main+0x138ce90's instance loop: per instance culling, uniform updates, per mesh textures and indexed draws). Pure game
// helpers (name lookups, culling tests, parameter blocks) are called as they are; what records commands is ported.

use super::draw::{load_resource, Port, R};

const PFN_UPDATE_UNIFORM_BUFFER: usize = 0x466e228;
const PFN_DRAW_ELEMENTS_BASE_VERTEX: usize = 0x466e090;
const PFN_BUFFER_GET_ADDRESS: usize = 0x466d818;
const PFN_DEVICE_GET_TEXTURE_HANDLE: usize = 0x466d428;
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
const LIGHT_VOLUME: usize = 0x8fd5cc;
const LIGHT_FRUSTUM: usize = 0x8fd654;
const FOOTPRINT: usize = 0x93a4c4;
const IN_FRUSTUM: usize = 0x138d420;
const IN_OCCLUDER: usize = 0x940c88;
const IN_CASCADE: usize = 0x1387a50;
/// Byte table: mesh class + mode * 5 + 30 when a cascade is drawn -> the mesh draws.
const MESH_DRAWS: usize = 0x3810b18;

/// The delegate targets main+0x137c740 / 0x137c7f4 / 0x137c95c (modes 1 / 2 / 4, `main_pass`: drawn only in pass
/// 3 while +0x13c2 is 0) and main+0xad2978 (mode 0: drawn except in pass 3 while +0x13c2 is not 1): mark the
/// subsystem used this frame (+0x40 bit 4, under its lock), then draw it.
pub(super) unsafe fn target(p: &mut Port, this: usize, arg: usize, mode: u32, main_pass: bool) -> R<()> {
    let s = p.m.r64(this + 0x28);
    p.query(p.base + LOCK, &[(s + 0x20) as u64]);
    p.m.w::<u32>(s + 0x40, p.m.r32(s + 0x40) | 0x10);
    p.query(p.base + UNLOCK, &[(s + 0x20) as u64]);
    let ctx = p.m.r64(arg);
    let (pass3, flag) = (p.m.r8(ctx + 0x31) == 3, p.m.r8(p.m.r64(this + 0x28) + 0x13c2));
    if if main_pass { pass3 && flag == 0 } else { !pass3 || flag == 1 } {
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
    // Pass 7 (lights): the view's light volume (main+0x8fd5cc: near / far, overridden by the info's +0x1c / +0x24,
    // and two more parameters) culls instead of the view's frustum.
    if p.m.r8(ctx + 0x31) == 7 {
        let v = views + view * 0x430;
        let near = p.m.f32(if p.m.r8(info + 0x1c) != 0 { info + 0x20 } else { v + 0x1e0 });
        let far = p.m.f32(if p.m.r8(info + 0x24) != 0 { info + 0x28 } else { v + 0x1e4 });
        p.query_f(
            p.base + LIGHT_VOLUME,
            &[state.as_mut_ptr() as u64, (v + 0x100) as u64, (v + 0x160) as u64, (v + 0x1fc) as u64],
            &[near, far, p.m.f32(v + 0x1e8), p.m.f32(v + 0x1ec)],
        );
        frustum = state.as_ptr() as usize;
    }
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
                        p.draw_elements(cmd as usize, prim as i32, itype as i32, count as i32, addr as u64);
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
            p.bind_texture(cmd as usize, stage as i32, (slots >> sh) as i8 as i32, handle as u64);
        }
    }
}

const STORAGE_SLOT_BY_NAME: usize = 0x1387540;
const INSTANCE_PARAMS: usize = 0x860094;
const PFN_DRAW_ELEMENTS_INSTANCED: usize = 0x466e0a0;

/// The instanced cave subsystem's delegate targets: main+0x85ff40 (mode 0) and 0x137c31c (mode 1) through
/// main+0x85ffc4 (drawn while the info's +1 is set), main+0x137c3a0 / 0x137c4bc (modes 2 / 3, drawn while +0x1389 is
/// set, `flag_gate`): mark the subsystem used (+0x40 bit 2), then main+0x860148 timed, in every pass.
pub(super) unsafe fn instanced_target(p: &mut Port, this: usize, arg: usize, mode: u32, flag_gate: bool) -> R<()> {
    let s = p.m.r64(this + 0x28);
    p.query(p.base + LOCK, &[(s + 0x20) as u64]);
    p.m.w::<u32>(s + 0x40, p.m.r32(s + 0x40) | 4);
    p.query(p.base + UNLOCK, &[(s + 0x20) as u64]);
    let s = p.m.r64(this + 0x28);
    let ctx = p.m.r64(arg);
    p.m.w::<u64>(s + 0x13d0, p.m.r64(this + 0x20) as u64);
    p.m.w::<u32>(s + 0x13e0, p.m.r8(ctx + 0x30) as u32 | p.m.r32(this + 0xe00) << 8);
    let s = p.m.r64(this + 0x28);
    let (sys, info) = (p.m.r64(s + 0x4e8), s + 5000);
    let on = if flag_gate { p.m.r8(s + 0x1389) != 0 } else { p.m.r8(info + 1) != 0 };
    if !on || p.m.r64(sys + 0x20) == 0 {
        return Ok(());
    }
    let t0 = p.query(p.base + TICK, &[]);
    let mut params = [0u8; 0xb0];
    params[0..8].copy_from_slice(&0x3d4c_cccd_0000_0001u64.to_le_bytes());
    params[8..0x10].copy_from_slice(&0x3f80_0000_3f80_0000u64.to_le_bytes());
    params[0x10..0x18].copy_from_slice(&0x7f80_0000_0000_0000u64.to_le_bytes());
    params[0x18..0x1c].copy_from_slice(&0x0101_0100u32.to_le_bytes());
    params[0xa4..0xa6].copy_from_slice(&0x100u16.to_le_bytes());
    p.query(p.base + INSTANCE_PARAMS, &[t0, params.as_mut_ptr() as u64, info as u64]);
    instanced_draw(p, p.m.r64(sys + 0x20), arg, mode, u32::from_le_bytes(params[0x30..0x34].try_into().unwrap()))?;
    let t1 = p.query(p.base + TICK, &[]);
    p.m.w::<u64>(sys + 0x1f8, t1.wrapping_sub(t0).wrapping_add(p.m.r64(sys + 0x1f8) as u64));
    p.m.w::<u32>(sys + 0x200, p.m.r32(sys + 0x200).wrapping_add(1));
    Ok(())
}

/// main+0x860148: the named blocks and storage buffers (main+0x138b448), then by mode the group's records
/// (0: main+0x138b9d0, 1 / 2: main+0x138bb24 over record set 0 / 1) or its single instanced mesh (3: main+0x138bc84).
unsafe fn instanced_draw(p: &mut Port, this: usize, arg: usize, mode: u32, group: u32) -> R<()> {
    let ctx = p.m.r64(arg);
    let w = p.m.r64(ctx + 0x20);
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    let program = p.m.r64(p.m.r64(arg + 0x10));
    let view = p.m.i32(this + 4) as i64;
    let user0 = slot_by_name(p, SLOT_BY_NAME, program, b"gsys_user0\0");
    bind_named_block(p, this + 0x6808, w, p.m.r64(this + 0x6880) as u64, user0)?;
    let v = (this as i64 + view * 400) as usize;
    let big = (this as i64 + view * 0x1270) as usize;
    let groups = p.m.r64(this + 0x2808);
    let n = p.m.i32(this + 0x2814).max(0) as usize;
    let find = |p: &Port| (0..n).map(|k| groups + k * 0x3c).find(|&g| p.m.r32(g) == group);
    let mut storage: Vec<(&'static [u8], usize, u64)> = vec![
        (b"cave_CaveInstanceDynamicDataSSBO\0", v + 0xd0, 0),
        (b"cave_QuadStaticDataSSBO\0", this + 0x6998, 0),
        (b"cave_QuadStreamDataSSBO\0", v + 8, 0),
        (b"cave_QuadDynamicDataSSBO\0", big + 0x328, 0),
    ];
    match mode {
        3 => {
            if let Some(g) = find(p) {
                storage.push((b"cave_VertexDynamicDataSSBO\0", big + 0x580, p.m.i32(g + 0x1c) as i64 as u64 * 0x10));
            }
        }
        1 => storage.push((b"cave_EdgeDynamicDataSSBO\0", big + 0x3f0, 0)),
        2 => storage.push((b"cave_EdgeDynamicDataSSBO\0", big + 0x4b8, 0)),
        _ => {}
    }
    storage.push((b"cave_PointGeometryDataSSBO\0", this + 0x6b98, 0));
    storage.push((b"cave_PointMaterialDataSSBO\0", this + 0x6c60, 0));
    for (n, obj, offset) in storage {
        let slot = slot_by_name(p, STORAGE_SLOT_BY_NAME, program, n);
        if slot != 0xffff_ffff {
            bind_named_block(p, obj + 0x40, w, (p.m.r64(obj + 0xb8) as u64).wrapping_add(offset), slot)?;
        }
    }
    let user1 = slot_by_name(p, SLOT_BY_NAME, program, b"gsys_user1\0");
    let Some(g) = find(p) else { return Ok(()) };
    if mode == 3 {
        let (a, b) = (p.m.i32(g + 0x1c), p.m.i32(g + 0x20));
        let k = b.wrapping_sub(a);
        if k != 0 && a <= b {
            mesh_elements(p, this + 0x6578, w, 0, p.m.r32(this + 0x65a0), k as u32, 0, 0);
        }
        return Ok(());
    }
    if mode > 3 {
        return Ok(());
    }
    // The group's records {a, b, c, d}: a ring block bound as gsys_user1 ({a, 1 << c, 1 << d, 0, 0} in mode 0,
    // {a, 0, 0, 1 << c, 1 << d} in modes 1 / 2), then the mesh at (c, d) drawn with b instances.
    let set = mode.saturating_sub(1) as usize;
    let range = if mode == 0 { p.m.r64(g + 0x24) } else { p.m.r64(g + 0x2c + set * 8) } as i64;
    let count = range.wrapping_sub(range << 32) >> 32;
    if count == 0 {
        return Ok(());
    }
    let first = if mode == 0 { big as i64 + 0x658 } else { big as i64 + set as i64 * 0x514 + 0xb6c } + ((range << 32) >> 28);
    for r in (0..count as usize).map(|k| first as usize + k * 16) {
        let (a, b, c, d) = (p.m.r32(r), p.m.r32(r + 4), p.m.i32(r + 8), p.m.i32(r + 12));
        let (pc, pd) = (1u32.wrapping_shl(c as u32), 1u32.wrapping_shl(d as u32));
        let data: [u32; 5] = if mode == 0 { [a, pc, pd, 0, 0] } else { [a, 0, 0, pc, pd] };
        let block = p.ring_block(data.as_ptr() as usize, 0x14);
        p.bind_uniform_block(block, w, user1, 0)?;
        let mesh = if mode == 0 {
            (this as i64 + d as i64 * 0x360 + c as i64 * 0x60 + 0x4718) as usize
        } else {
            (this as i64 + c as i64 * 0x360 + d as i64 * 0x60 + 0x28b8) as usize
        };
        mesh_elements(p, mesh, w, 0, p.m.r32(mesh + 0x28), b, 0, 0);
    }
    Ok(())
}

unsafe fn slot_by_name(p: &mut Port, f: usize, program: usize, n: &'static [u8]) -> u32 {
    let nm = name(n);
    let mut slot = 0xffff_ffffu32;
    p.query(p.base + f, &[(&mut slot) as *mut u32 as u64, program as u64, nm.as_ptr() as u64]);
    slot
}

/// main+0x13878e4(mesh, w, first, count, instances, base vertex, base instance).
#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn mesh_elements(p: &mut Port, mesh: usize, w: usize, first: u32, count: u32, instances: u32, base_vertex: i32, base_instance: i32) {
    if count == 0 {
        return;
    }
    let prim = p.m.r32(mesh + 0xc) as u64;
    let cmd = p.m.r64(w + 0xb8);
    let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[(mesh + 0x30) as u64]).wrapping_add(p.m.r32(mesh + 0x2c).wrapping_mul(first) as u64);
    let itype = p.m.r32(mesh + 8) as u64;
    if instances < 2 && base_instance == 0 {
        if base_vertex != 0 {
            p.call(p.pfn(PFN_DRAW_ELEMENTS_BASE_VERTEX), &[cmd as u64, prim, itype, count as u64, addr, base_vertex as u32 as u64]);
            return;
        }
        p.m.w::<u8>(cmd + 0x10, 1);
        p.draw_elements(cmd as usize, prim as i32, itype as i32, count as i32, addr as u64);
        return;
    }
    p.call(
        p.pfn(PFN_DRAW_ELEMENTS_INSTANCED),
        &[cmd as u64, prim, itype, count as u64, addr, base_vertex as u32 as u64, base_instance as u32 as u64, instances as u64],
    );
}

const PFN_BIND_VERTEX_BUFFER: usize = 0x466dfb8;
const PFN_BIND_VERTEX_ATTRIB_STATE: usize = 0x466dfa0;
const PFN_BIND_VERTEX_STREAM_STATE: usize = 0x466dfa8;
const GRAPHICS_HOLDER: usize = 0x462bbf0;
const WORKER_KEY: usize = 0x462af70;
const TLS_GET: usize = 0x2b15be0;
const CORE: usize = 0x2b15c10;
const CHUNK_DRAWS: usize = 0x3810a58;
const CHUNK_PARAMS: usize = 0x985920;
const IN_FRUSTUM_BOX: usize = 0x937d08;

/// The game's worker index: its thread-local value - 1, else the core number.
unsafe fn worker(p: &Port) -> u32 {
    let v = p.query(p.base + TLS_GET, &[p.m.r32(p.m.r64(p.base + WORKER_KEY)) as u64]);
    if v == 0 {
        p.query(p.base + CORE, &[]) as u32
    } else {
        (v as u32).wrapping_sub(1)
    }
}

/// The cave chunk renderer's delegate targets main+0x137c5d8 / 0x137c68c (modes 1 / 2, drawn only in pass 3 while
/// +0x13c1 is 0) and main+0x984294 (mode 0, drawn except in pass 3 while +0x13c1 is not 1): mark the subsystem used
/// (+0x40 bit 3), then main+0x98434c: the chunk set (main+0x984528) and the second set (main+0x9859e4), each timed.
pub(super) unsafe fn chunk_target(p: &mut Port, this: usize, arg: usize, mode: u32, main_pass: bool) -> R<()> {
    let s = p.m.r64(this + 0x28);
    p.query(p.base + LOCK, &[(s + 0x20) as u64]);
    p.m.w::<u32>(s + 0x40, p.m.r32(s + 0x40) | 8);
    p.query(p.base + UNLOCK, &[(s + 0x20) as u64]);
    let ctx = p.m.r64(arg);
    let (pass3, flag) = (p.m.r8(ctx + 0x31) == 3, p.m.r8(p.m.r64(this + 0x28) + 0x13c1));
    if !(if main_pass { pass3 && flag == 0 } else { !pass3 || flag == 1 }) {
        return Ok(());
    }
    let s = p.m.r64(this + 0x28);
    p.m.w::<u64>(s + 0x13d0, p.m.r64(this + 0x20) as u64);
    p.m.w::<u32>(s + 0x13e0, p.m.r8(ctx + 0x30) as u32 | p.m.r32(this + 0xe00) << 8);
    let (sys, info) = (p.m.r64(s + 0x4e8), s + 5000);
    if p.m.r8(info + 2) == 0 {
        return Ok(());
    }
    if p.m.r64(sys) != 0 {
        let t0 = p.query(p.base + TICK, &[]);
        let mut params = [0u8; 0xc0];
        params[0x18] = p.m.r8(info + 0x20);
        params[0x1c..0x20].copy_from_slice(&p.m.r32(info + 0x24).to_le_bytes());
        params[0x20] = p.m.r8(info + 0x28);
        params[0x24..0x28].copy_from_slice(&p.m.r32(info + 0x2c).to_le_bytes());
        params[0x30..0x38].copy_from_slice(&(p.m.r64(info + 0x48) as u64).to_le_bytes());
        params[0x40..0x44].copy_from_slice(&p.m.r32(info + 0x58).to_le_bytes());
        params[0xb5] = p.m.r8(info + 0xcd);
        params[0xb6] = p.m.r8(info + 0xce);
        params[0xb8..0xc0].copy_from_slice(&(p.m.r64(info + 0xd0) as u64).to_le_bytes());
        chunks(p, p.m.r64(sys), arg, mode, params.as_ptr() as usize)?;
        let t1 = p.query(p.base + TICK, &[]);
        p.m.w::<u64>(sys + 0xb8, t1.wrapping_sub(t0).wrapping_add(p.m.r64(sys + 0xb8) as u64));
        p.m.w::<u32>(sys + 0xc0, p.m.r32(sys + 0xc0).wrapping_add(1));
        if p.m.r8(info + 2) == 0 {
            return Ok(());
        }
    }
    if p.m.r64(sys + 8) != 0 {
        let mut params = [0u8; 0xb0];
        p.query(p.base + CHUNK_PARAMS, &[params.as_mut_ptr() as u64, info as u64]);
        let t0 = p.query(p.base + TICK, &[]);
        second_set(p, p.m.r64(sys + 8), arg, mode, params.as_ptr() as usize)?;
        let t1 = p.query(p.base + TICK, &[]);
        p.m.w::<u64>(sys + 0xb8, t1.wrapping_sub(t0).wrapping_add(p.m.r64(sys + 0xb8) as u64));
        p.m.w::<u32>(sys + 0xc0, p.m.r32(sys + 0xc0).wrapping_add(1));
    }
    Ok(())
}

/// The plane test of main+0x984528's culling: some plane (normal at +0, distance at +0xc) has the box's corner
/// along its normal in front of it.
unsafe fn outside(p: &Port, plane: usize, bounds: usize, d_first: bool) -> bool {
    let (nx, ny, nz, d) = (p.m.f32(plane), p.m.f32(plane + 4), p.m.f32(plane + 8), p.m.f32(plane + 0xc));
    let corner = |n: f32, k: usize| if n > 0.0 { p.m.f32(bounds + k * 4) } else { p.m.f32(bounds + 0xc + k * 4) };
    let s = nx * corner(nx, 0) + ny * corner(ny, 1) + nz * corner(nz, 2);
    if d_first { d + s > 0.0 } else { s - d > 0.0 }
}

/// main+0x984528(this, arg, mode, params): named blocks, the worker's vertex state, then the group's records last to
/// first: culling, uniform updates, the worker's vertex and index buffers re-initialized over the record's storage,
/// vertex buffers bound, the indexed draw.
unsafe fn chunks(p: &mut Port, this: usize, arg: usize, mode: u32, info: usize) -> R<()> {
    let ctx = p.m.r64(arg);
    let w = p.m.r64(ctx + 0x20);
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    let program = p.m.r64(p.m.r64(arg + 0x10));
    let mut user = [0xffff_ffffu32; 4];
    for (k, n) in [&b"gsys_user0\0"[..], b"gsys_user1\0", b"gsys_user2\0", b"gsys_user3\0"].iter().enumerate() {
        user[k] = slot_by_name(p, SLOT_BY_NAME, program, n);
    }
    for (k, (block, addr)) in [(0x158, 0x1d0), (0x2d8, 0x350), (0x398, 0x410), (0x458, 0x4d0)].iter().enumerate() {
        bind_named_block(p, this + block, w, p.m.r64(this + addr) as u64, user[k])?;
    }
    let cmd = p.m.r64(w + 0xb8);
    let core = worker(p);
    let pick = |p: &Port, n: usize| if p.m.r32(this + n) > core { core } else { 0 };
    let pc = (p.m.r64(this + 0x5a0) as i64 + pick(p, 0x598) as i32 as i64 * 0x340) as usize;
    p.call(p.pfn(PFN_BIND_VERTEX_ATTRIB_STATE), &[cmd as u64, 0x10, (pc + 0x27c) as u64]);
    p.call(p.pfn(PFN_BIND_VERTEX_STREAM_STATE), &[cmd as u64, 0x10, (pc + 0x2c0) as u64]);
    let view = p.m.r8(ctx + 0x46) as usize;
    let frustum = p.m.r64(p.m.r64(ctx + 0x10) + 0x18) + view * 0x430 + 0x60;
    let shadow = p.m.r64(info + 0x30);
    let (mut cascade, mut cascade_index) = (0usize, 0u32);
    if shadow != 0 && p.m.r8(ctx + 0x31) == 8 && (5..=8).contains(&view) {
        let mut k = p.m.r8(ctx + 0x30) as u32;
        if p.m.r32(shadow + 0xd20) <= k {
            k = 0;
        }
        cascade = p.m.r64(shadow + 0xd28) + k as usize * 0xb48;
        cascade_index = (view as u32 - 5) & 0xff;
    }
    // Pass 7 (lights): the view's light volume (main+0x8fd654 over the draw state, the view's light matrix copied to
    // +0xa0 and its footprint at +0xd0) culls instead of the view's frustum.
    let mut state = [0u8; 0x240];
    let mut frustum = frustum;
    if p.m.r8(ctx + 0x31) == 7 {
        let v = p.m.r64(p.m.r64(ctx + 0x10) + 0x18) + view * 0x430;
        p.query(p.base + DRAW_STATE_INIT, &[state.as_mut_ptr() as u64]);
        let near = p.m.f32(if p.m.r8(info + 0x18) != 0 { info + 0x1c } else { v + 0x1e0 });
        let far = p.m.f32(if p.m.r8(info + 0x20) != 0 { info + 0x24 } else { v + 0x1e4 });
        for k in 0..0x30usize {
            state[0xa0 + k] = p.m.r8(v + 0x100 + k);
        }
        p.query(p.base + FOOTPRINT, &[state.as_mut_ptr() as u64 + 0xd0, (v + 0x100) as u64]);
        p.query_f(
            p.base + LIGHT_FRUSTUM,
            &[state.as_mut_ptr() as u64, (v + 0x160) as u64, (v + 0x1fc) as u64],
            &[near, far, p.m.f32(v + 0x1e8), p.m.f32(v + 0x1ec)],
        );
        frustum = state.as_ptr() as usize;
    }
    let culled = p.m.r8(info + 0xb5) != 0;
    let occluder = if p.m.r8(info + 0xb6) != 0 { p.m.r64(info + 0xb8) } else { 0 };
    let cascade = if culled { cascade } else { 0 };
    let frustum = if culled { frustum } else { 0 };
    let groups = p.m.r64(this + 0x5a8);
    let n = p.m.i32(this + 0x5b4).max(0) as usize;
    let want = p.m.r32(info + 0x40);
    let mut drawn = 0u32;
    if let Some(g) = (0..n).map(|k| groups + k * 0x14).find(|&g| p.m.r32(g) == want) {
        let core2 = worker(p);
        let ib = (p.m.r64(this + 0x8b8) as i64 + (if p.m.r32(this + 0x8b0) > core2 { core2 } else { 0 }) as i32 as i64 * 0x60) as usize;
        let core3 = worker(p);
        let (start, end) = (p.m.i32(g + 0xc) as i64, p.m.i32(g + 0x10) as i64);
        let table = p.base + CHUNK_DRAWS + (cascade != 0) as usize * 0x2d;
        let (mut last1, mut last2) = (-1i32, -1i32);
        let mut i = end;
        while i > start {
            i -= 1;
            let rec = p.m.r64(this + 0x98) + i as usize * 0x30;
            let idx = p.m.i32(rec + 0x1c) as i64;
            let ent = (p.m.r64(this + 0x38) as i64 + idx * 0x40) as usize;
            if p.m.r64(ent) == 0 {
                continue;
            }
            let lod = p.m.i32(ent + 0x1c).min(2) as i64;
            let k = (table as i64 + lod * 15 + mode as i32 as i64 * 5 + p.m.r16(rec + 0x28) as i64) as usize;
            if p.m.r8(k) == 0 {
                continue;
            }
            let bounds = (p.m.r64(this + 0x58) as i64 + idx * 0x18) as usize;
            if cascade != 0 {
                let mut c = cascade_index;
                if p.m.r32(cascade + 0x360) <= c {
                    c = 0;
                }
                let planes = p.m.r64(cascade + 0x368) + c as usize * 0x590;
                let np = p.m.i32(planes + 0x3f8);
                if (0..np.max(0) as usize).any(|k| outside(p, planes + (if k < 0x18 { k } else { 0 }) * 0x10 + 0x3fc, bounds, false)) {
                    continue;
                }
            } else if frustum != 0 {
                let f = |o: usize| p.m.f32(frustum + o);
                let b = |o: usize| p.m.f32(bounds + o);
                if !(b(0) <= f(0x210)) || !(f(0x204) <= b(0xc)) || !(b(4) <= f(0x214)) || !(f(0x208) <= b(0x10)) {
                    continue;
                }
                if !(b(8) <= f(0x218)) || !(f(0x20c) <= b(0x14)) {
                    continue;
                }
                if (0..6).any(|k| outside(p, frustum + 0x1a4 + k * 0x10, bounds, true)) {
                    continue;
                }
                if occluder != 0 && p.query(p.base + IN_OCCLUDER, &[occluder as u64, bounds as u64]) & 1 == 0 {
                    continue;
                }
            }
            let v = p.m.i32(ent + 8);
            if last1 != v {
                let mut u = v as u32;
                if p.m.r32(this + 0x108) <= u {
                    u = 0;
                }
                let src = (p.m.r64(this + 0x110) as i64 + u as i32 as i64 * 0x30) as u64;
                p.call(p.pfn(PFN_UPDATE_UNIFORM_BUFFER), &[cmd as u64, p.m.r64(this + 0x350) as u64, p.m.r64(this + 0x2c8) as u64, 0, 0x30, src]);
                last1 = p.m.i32(ent + 8);
            }
            if last2 != last1 {
                let block = p.m.r64(p.m.r64(p.m.r64(ent)) + 8);
                let x = p.m.r64(block + 0xa0) as u64;
                let size = ((x as i64) >> 27) as u64 & !0x1f;
                p.call(
                    p.pfn(PFN_UPDATE_UNIFORM_BUFFER),
                    &[cmd as u64, p.m.r64(this + 0x410) as u64, p.m.r64(this + 0x388) as u64, 0, size, block as u64 + (x & 0xffff_ffff)],
                );
                last2 = p.m.i32(ent + 8);
            }
            let src = (p.m.r64(this + 0x78) as i64 + idx * 0x60) as u64;
            p.call(p.pfn(PFN_UPDATE_UNIFORM_BUFFER), &[cmd as u64, p.m.r64(this + 0x4d0) as u64, p.m.r64(this + 0x448) as u64, 0, 0x60, src]);
            // The worker's vertex buffer over the record's storage.
            let k3 = if p.m.r32(this + 0x598) > core3 { core3 } else { 0 };
            let vb = (p.m.r64(this + 0x5a0) as i64 + k3 as i32 as i64 * 0x340) as usize;
            p.m.w::<u32>(vb + 0x138, p.m.r32(rec));
            p.m.w::<u32>(vb + 0x13c, p.m.r32(rec + 4));
            p.m.w::<u32>(vb + 0x158, p.m.r32(rec + 0x18));
            p.m.w::<u64>(vb + 0x140, p.m.r64(rec + 8) as u64);
            p.m.w::<u64>(vb + 0x148, p.m.r64(rec + 0x10) as u64);
            let counters = p.m.r64(p.m.r64(p.base + GRAPHICS_HOLDER));
            p.reinit_buffer(vb + 0x48, counters + 0x810, p.m.r64(vb + 0x140), p.m.r32(vb + 0x138), p.m.r32(vb + 0x158));
            let streams = p.m.r32(vb + 0x268) as usize;
            let list = p.m.r64(vb + 0x270);
            for k in 0..streams {
                let e = p.m.r64(list + k * 8);
                if e == 0 {
                    continue;
                }
                let a = p.buffer_address_of(e + 8);
                let size = p.m.r32(e + 0x118);
                p.m.w::<u8>(cmd + 0x10, 1);
                p.call(p.pfn(PFN_BIND_VERTEX_BUFFER), &[cmd as u64, k as u64, a, size as u64]);
            }
            // The worker's index buffer over the same storage (+0x28: half the record's size, in elements of +0x2c).
            p.m.w::<u32>(ib + 0x10, p.m.r32(rec));
            p.m.w::<u32>(ib + 0x14, p.m.r32(rec + 4));
            p.m.w::<u64>(ib + 0x18, p.m.r64(rec + 8) as u64);
            p.m.w::<u64>(ib + 0x20, p.m.r64(rec + 0x10) as u64);
            p.m.w::<i32>(ib + 0x28, p.m.i32(rec + 0x18) >> 1);
            p.reinit_buffer(ib + 0x30, counters + 0x81c, p.m.r64(ib + 0x18), p.m.r32(ib + 0x10), p.m.r32(ib + 0x28).wrapping_mul(p.m.r32(ib + 0x2c)));
            let count = p.m.r32(rec + 0x24);
            if count != 0 {
                let prim = p.m.r32(ib + 0xc) as u64;
                let first = p.m.r32(rec + 0x20);
                let addr = p.buffer_address_of(ib + 0x30).wrapping_add(p.m.r32(ib + 0x2c).wrapping_mul(first) as u64);
                let itype = p.m.r32(ib + 8) as u64;
                p.m.w::<u8>(cmd + 0x10, 1);
                p.draw_elements(cmd as usize, prim as i32, itype as i32, count as i32, addr as u64);
            }
            drawn = drawn.wrapping_add(p.m.r32(rec + 0x24));
        }
    }
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(drawn / 3));
    Ok(())
}

/// main+0x9859e4(this, arg, mode, params): named blocks and storage buffers (main+0x1387188); in mode 0 the group's
/// chunks drawn one instance each with their index as base instance (main+0x13876c8).
unsafe fn second_set(p: &mut Port, this: usize, arg: usize, mode: u32, info: usize) -> R<()> {
    let ctx = p.m.r64(arg);
    let w = p.m.r64(ctx + 0x20);
    load_resource(p, ctx, p.m.r64(arg + 8))?;
    let program = p.m.r64(p.m.r64(arg + 0x10));
    let user0 = slot_by_name(p, SLOT_BY_NAME, program, b"gsys_user0\0");
    bind_named_block(p, this + 0x4760, w, p.m.r64(this + 0x47d8) as u64, user0)?;
    for (n, obj) in [
        (&b"cave_CaveInstanceDynamicDataSSBO\0"[..], this + 8),
        (b"cave_ChunkDynamicDataSSBO\0", this + 0x198),
        (b"cave_DrawCallDataSSBO\0", this + 0x328),
    ] {
        let slot = slot_by_name(p, STORAGE_SLOT_BY_NAME, program, n);
        if slot != 0xffff_ffff {
            bind_named_block(p, obj + 0x40, w, p.m.r64(obj + 0xb8) as u64, slot)?;
        }
    }
    let view = p.m.r8(ctx + 0x46) as usize;
    let shadow = p.m.r64(info + 0x20);
    let (mut cascade, mut cascade_index) = (0usize, 0u32);
    if shadow != 0 && p.m.r8(ctx + 0x31) == 8 && (5..=8).contains(&view) {
        cascade_index = view as u32 - 5;
        let mut k = p.m.r8(ctx + 0x30) as u32;
        if p.m.r32(shadow + 0xd20) <= k {
            k = 0;
        }
        cascade = p.m.r64(shadow + 0xd28) + k as usize * 0xb48;
    }
    if mode != 0 {
        return Ok(());
    }
    let culled = p.m.r8(info + 0xa5) != 0;
    let cascade = if culled { cascade } else { 0 };
    let frustum = if culled { p.m.r64(p.m.r64(ctx + 0x10) + 0x18) + view * 0x430 + 0x60 } else { 0 };
    let group = p.m.r32(info + 0x30);
    let groups = p.m.r64(this + 0x4f8);
    let n = p.m.i32(this + 0x504).max(0) as usize;
    let Some(g) = (0..n).map(|k| groups + k * 0x14).find(|&g| p.m.r32(g) == group) else { return Ok(()) };
    let (start, end) = (p.m.i32(g + 0xc), p.m.i32(g + 0x10));
    for i in start..end {
        let rec = (p.m.r64(this + 0x4d8) as i64 + i as i64 * 0xc) as usize;
        let chunk = (p.m.r64(this + 0x4b8) as i64 + p.m.i32(rec) as i64 * 0x50) as usize;
        if p.m.r8(chunk + 0x20) != 0 {
            continue;
        }
        let (first, count) = (p.m.r32(rec + 4), p.m.r32(rec + 8));
        let visible = if cascade != 0 {
            p.query(p.base + IN_CASCADE, &[cascade as u64, (chunk + 0x34) as u64, cascade_index as u64]) & 1 != 0
        } else if frustum != 0 {
            p.query(p.base + IN_FRUSTUM_BOX, &[frustum as u64, (chunk + 0x34) as u64, (chunk + 0x40) as u64]) & 1 != 0
        } else {
            true
        };
        if count != 0 && visible {
            mesh_elements(p, this + 0x4a98, w, first, count, 1, 0, i);
        }
    }
    Ok(())
}
