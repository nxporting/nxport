// The per-view instance renderer main+0xd7c1d8(this, slot, arg), reached through the per-view dispatch target
// (main+0x2ad00a0 -> virtual +0x30). Instances of the drawable's component are culled once per frame, view and pass
// into a per-worker bitset (a visible bit and a drawn bit per instance), then the visible ones of the model's LOD
// kind draw (main+0xd7e090). Predicting only: applying lets the game draw until every path is ported (the
// Degeneracy / Deformable paths, region lists, lazily created instance records are not).

use super::draw::{Port, R};

const WORKER_KEY: usize = 0x462af70;
const TLS_GET: usize = 0x2b15be0;
const CORE: usize = 0x2b15c10;
const SCENE_HOLDER: usize = 0x462bc08;
const FRAME_HOLDER: usize = 0x4631738;
const CAST_GUARD: usize = 0x46327f0;
const CAST_TYPE: usize = 0x46327f8;
const LOCK: usize = 0x2b15370;
const UNLOCK: usize = 0x2b15380;
const VIEW_DATA: usize = 0x1a358b8;
const OCCLUSION: usize = 0xd7ddf4;
const DEFAULT_LOD: usize = 0x4632760;
const DEVICE_HOLDER: usize = 0x462bbe8;
const PFN_DEVICE_GET_TEXTURE_HANDLE: usize = 0x466d428;
const PFN_BIND_TEXTURE: usize = 0x466e008;
const PFN_SET_PRIMITIVE_RESTART: usize = 0x466e050;
const COMPONENT_TYPE: u32 = 0xd21b65b6;

/// Virtual calls left to the game: (main+ function | slot << 40, count).
static CALLS: std::sync::Mutex<Vec<(usize, u64)>> = std::sync::Mutex::new(Vec::new());

fn note(f: usize, slot: usize) {
    let mut g = CALLS.lock().unwrap();
    let k = f | slot << 40;
    match g.iter_mut().find(|x| x.0 == k) {
        Some(x) => x.1 += 1,
        None => g.push((k, 1)),
    }
}

pub(super) fn report() -> String {
    let mut g = std::mem::take(&mut *CALLS.lock().unwrap());
    g.sort_by(|a, b| b.1.cmp(&a.1));
    g.iter().map(|(f, n)| format!("+{:#x} main+{:#x} {n}", f >> 40, f & ((1 << 40) - 1))).collect::<Vec<_>>().join(", ")
}

/// |v| as FCMP + FCSEL lt gives it (a NaN is negated).
fn abs(v: f32) -> f32 {
    if !(v >= 0.0) { -v } else { v }
}

pub(super) unsafe fn worker(p: &Port) -> u32 {
    let v = p.query(p.base + TLS_GET, &[p.m.r32(p.m.r64(p.base + WORKER_KEY)) as u64]);
    if v == 0 { p.query(p.base + CORE, &[]) as u32 } else { (v as u32).wrapping_sub(1) }
}

/// `lit` occurs at one of the first `n` positions of `s`, a match also ending at the string's NUL (the game's search
/// loops).
unsafe fn find(p: &Port, s: usize, lit: &[u8], n: usize) -> bool {
    (0..n).any(|i| {
        for (k, &c) in lit.iter().enumerate() {
            let b = p.m.r8(s + i + k);
            if b != c {
                return false;
            }
            if b == 0 {
                return true;
            }
        }
        true
    })
}

unsafe fn strlen(p: &Port, s: usize) -> usize {
    (0..0x40_0000).find(|&i| p.m.r8(s + i) == 0).unwrap_or(0x40_0000)
}

fn rot(b: u8) -> u8 {
    b.rotate_right(3)
}

/// The LOD kind the model's name gives ("..NormalNear" 0, "..Normal..Middle.." 1, "..Normal..Far.." 2), with the
/// name tail the "Deformable" search runs over; None: not drawn; Err: the Degeneracy names (not ported).
unsafe fn name_kind(p: &Port, name: usize) -> R<Option<(u32, usize)>> {
    match p.m.r8(name + 2) {
        b'N' => {
            if (0..5).any(|k| p.m.r8(name + 3 + k) != b"ormal"[k]) {
                return Ok(None);
            }
            let tail = name + 8;
            let c = p.m.r8(tail);
            if c == b'N' && p.m.r8(name + 9) == b'e' && p.m.r8(name + 10) == b'a' && p.m.r8(name + 11) == b'r' {
                return Ok(Some((0, tail)));
            }
            if c == 0 {
                return Ok(None);
            }
            let len = strlen(p, name + 9) + 1;
            if len >= 6 && find(p, tail, b"Middle", len - 5) {
                return Ok(Some((1, tail)));
            }
            if len >= 3 && find(p, tail, b"Far", len - 2) {
                return Ok(Some((2, tail)));
            }
            Ok(None)
        }
        b'D' => {
            if (0..9).any(|k| p.m.r8(name + 3 + k) != b"egeneracy"[k]) {
                return Ok(None);
            }
            let z = |c: u8| p.m.r8(name + 0xc) == c && p.m.r8(name + 0xd) == 0;
            // 3 / 4: the Degeneracy pairs (Y / X).
            Ok(if z(b'Y') { Some((3, name + 0xc)) } else if z(b'X') { Some((4, name + 0xc)) } else { None })
        }
        _ => Ok(None),
    }
}

/// main+0x1891e94(pb, w, defaults, table, entry): the texture of the program's entry `entry`.
unsafe fn bind_entry(p: &mut Port, pb: usize, w: usize, defaults: usize, table: &[u64; 32], entry: i32) {
    let e = (pb as i64 + entry as i64 * 0x18) as usize;
    let idx = p.m.r8(e + 0x18) as usize;
    let mut t = table.get(idx).copied().unwrap_or(0) as usize;
    if t == 0 {
        t = p.m.r64(p.m.r64(defaults + 0x50) + idx * 8);
        if t == 0 {
            return;
        }
        t = p.m.r64(t + 0x40);
        if t == 0 {
            return;
        }
    }
    let sampler = p.m.r64(p.m.r64(p.m.r64(pb) + 0x90) + idx * 8);
    let tid = p.m.i32(t + 0x90);
    if tid == -1 || sampler & 0xffff == 0xffff {
        return;
    }
    let slots = p.m.r32(e + 8);
    let dev = p.m.r64(p.m.r64(p.m.r64(p.base + DEVICE_HOLDER)) + 0x30);
    let handle = p.query(p.pfn(PFN_DEVICE_GET_TEXTURE_HANDLE), &[dev as u64, tid as u32 as u64, sampler as i16 as i32 as u32 as u64]);
    let cmd = p.m.r64(w + 0xb8) as u64;
    for (stage, sh) in [(0u64, 0), (1, 8), (2, 16), (5, 24)] {
        if (slots >> sh) & 0xff != 0xff {
            p.call(p.pfn(PFN_BIND_TEXTURE), &[cmd, stage, (slots >> sh) as i8 as i64 as u64, handle]);
        }
    }
}

/// What main+0xd7e090 reads from the caller's frame.
struct Draw {
    block: usize,
    w: usize,
    slots: u32,
    idx: [i32; 2],
    lod: [usize; 2],
    sets: [u64; 32],
    table: [u64; 32],
    pb: usize,
    defaults: usize,
    entries: [i32; 32],
    meshes: usize,
}

/// The instance's record, created by main+0xd7e090 on its first draw (into the overlay; the game's own call creates
/// it the same way): the next of the model's records (counter +0x110, array +0xf8, count +0xf0), filled by
/// main+0xd7e8c4 with the instance's +0x20..0x30 and per texture set slot (+0x40c slots, LOD bit bytes at +0x410)
/// the first set in the instance's chain (+0x38, next +0x18) that has the slot's LOD with a usable texture. Only what
/// the draw reads is written (not the slots' texture coordinates, nor the instance's uniform data).
unsafe fn create_record(p: &mut Port, comp: usize, inst: usize) -> usize {
    let n = p.m.r32(comp + 0x110);
    p.m.w::<u32>(comp + 0x110, n.wrapping_add(1));
    if p.m.r32(comp + 0xf0) <= n {
        return 0;
    }
    let rec = (p.m.r64(comp + 0xf8) as i64 + n as i32 as i64 * 0x50) as usize;
    p.m.w::<u64>(rec, p.m.r64(inst + 0x20) as u64);
    p.m.w::<u64>(rec + 8, p.m.r64(inst + 0x28) as u64);
    for k in 0..p.m.i32(comp + 0x40c).max(0) as usize {
        let set = choose_set(p, comp, inst, k);
        p.m.w::<u64>(rec + 0x18 + k * 0x10, set as u64);
    }
    p.m.w::<u64>(inst + 0x78, rec as u64);
    rec
}

/// main+0xd7e8c4's set for slot k: the first set in the instance's chain with the slot's LOD and a usable texture.
unsafe fn choose_set(p: &Port, comp: usize, inst: usize, k: usize) -> usize {
    let bit = p.m.r8(p.m.r64(p.m.r64(comp + 0x410) + k * 8) + 1);
    let mut set = 0usize;
    if bit != 0xff {
        let mut s = p.m.r64(inst + 0x38);
        let b = bit as u32 & 31;
        while s != 0 {
            let bits = p.m.r32(s + 0xc);
            if (bits >> b) & 1 != 0 {
                let m = bits & !(u32::MAX << b);
                let i = if m >= 2 { m.count_ones() } else { m };
                let tex = p.m.r64(p.m.r64(s + 0x10) + i as usize * 8);
                let usable = tex != 0
                    && (p.m.r8(tex + 0x12) & 4 != 0
                        || (p.m.r64(tex + 8) != 0 && p.m.r32(tex + 0x18) != 0 && p.m.r32(tex + 0x18) >> 30 == 0));
                if usable {
                    set = s;
                    break;
                }
            }
            s = p.m.r64(s + 0x18);
        }
    }
    set
}

/// The pair's record, created on its first draw (main+0xd7c1d8 / main+0x24b2a04, into the overlay): the next of the
/// model's pair records (counter +0x114, array +0x108, 16-byte entries, count +0x100) holding the records of its two
/// instances (created first where missing, main+0x24b8774), in the order the pair's +0 kind (2 or 3: A first) gives.
/// The pair's uniform data (main+0x24b8980) is not written.
unsafe fn create_pair(p: &mut Port, comp: usize, e: usize) -> usize {
    let n = p.m.r32(comp + 0x114);
    p.m.w::<u32>(comp + 0x114, n.wrapping_add(1));
    let rec = if p.m.r32(comp + 0x100) > n { (p.m.r64(comp + 0x108) as i64 + n as i32 as i64 * 16) as usize } else { 0 };
    let (a, b) = (p.m.r64(e + 8), p.m.r64(e + 0x10));
    let (x, y) = if p.m.r32(e) & !1 == 2 { (a, b) } else { (b, a) };
    for i in [x, y] {
        if p.m.r64(i + 0x78) == 0 && create_record(p, comp, i) == 0 {
            return 0;
        }
    }
    if rec == 0 {
        return 0;
    }
    p.m.w::<u64>(rec, p.m.r64(x + 0x78) as u64);
    p.m.w::<u64>(rec + 8, p.m.r64(y + 0x78) as u64);
    p.m.w::<u64>(e + 0x18, rec as u64);
    rec
}

/// main+0xd7e090: one instance: its block element, its two texture sets' textures for the LOD, its mesh's draw.
unsafe fn draw_instance(p: &mut Port, d: &mut Draw, inst: usize) -> R<u32> {
    let mut rec = p.m.r64(inst + 0x78);
    if rec == 0 {
        rec = create_record(p, d.block - 0x118, inst);
        if rec == 0 {
            return Err("per-view instance record");
        }
    }
    p.bind_uniform_block(d.block, d.w, d.slots, p.m.r16(inst) as i32)?;
    for k in 0..2 {
        let idx = d.idx[k];
        if idx == -1 {
            continue;
        }
        let set = p.m.r64(rec + if k == 0 { 8 } else { 0x18 }) as u64;
        let i = idx as usize & 31;
        if d.sets[i] == set {
            continue;
        }
        d.sets[i] = set;
        let s = set as usize;
        let bits = p.m.r32(s + 0xc);
        let b = p.m.r8(d.lod[k] + 1) as u32 & 31;
        if (bits >> b) & 1 == 0 {
            return Err("per-view texture set");
        }
        let m = bits & !(u32::MAX << b);
        let n = if m >= 2 { m.count_ones() } else { m };
        let tex = p.m.r64(p.m.r64(s + 0x10) + n as usize * 8);
        d.table[i] = (p.m.r64(tex + 8) + 8) as u64;
        let e = d.entries[i];
        let pb = d.pb;
        bind_entry(p, pb, d.w, d.defaults, &d.table, e);
    }
    let mesh = p.m.r64(d.meshes + if (p.m.r8(inst + 5) >> 2) & 1 != 0 { 8 } else { 0 });
    let n = mesh_draw(p, mesh, d.w)?;
    Ok(n.wrapping_sub(2))
}

/// The mesh's draw (virtual +0x28 with one instance); returns what it returns.
unsafe fn mesh_draw(p: &mut Port, mesh: usize, w: usize) -> R<u32> {
    let f = p.vfn(mesh, 0x28);
    let n = if f == 0x2acfb44 {
        // The mesh's whole range (+0x28 indices), one instance: its virtual +0x30 (w, first, count, instances, base).
        let g = p.vfn(mesh, 0x30);
        let count = p.m.r32(mesh + 0x28);
        if g == 0x24454e8 {
            // main+0x24454e8: the mesh's buffer at +0x10, first +0x24, base vertex +0x1c, base instance +0x18.
            if count != 0 {
                let (bi, bv, first) = (p.m.i32(mesh + 0x18), p.m.i32(mesh + 0x1c), p.m.r32(mesh + 0x24));
                super::cave::mesh_elements(p, p.m.r64(mesh + 0x10), w, first, count, 1, bv, bi);
            }
            count as u64
        } else {
            note(g, 0x30);
            p.helper("per-view mesh range", g, &[mesh as u64, w as u64, 0, count as u64, 1, 0])?
        }
    } else {
        note(f, 0x28);
        p.helper("per-view mesh draw", f, &[mesh as u64, w as u64, 1])?
    };
    Ok(n as u32)
}

/// main+0xd7c1d8(this, slot, arg).
pub(super) unsafe fn draw(p: &mut Port, this: usize, slot: u32, arg: usize) -> R<()> {
    if !p.m.predict() {
        p.call(p.base + 0xd7c1d8, &[this as u64, slot as u64, arg as u64]);
        return Ok(());
    }
    let t = p.m.r64(this + 0x18);
    let scene = p.m.r64(p.m.r64(t + 0x70) + 8);
    let vi = if p.m.r32(t + 0xea0) > slot { slot } else { 0 } as i32 as i64;
    let ent = (p.m.r64(t + 0xea8) as i64 + vi * 0xe0) as usize;
    let comp_owner = p.m.r64(ent + 8);
    let (ctx, d) = (p.m.r64(arg), p.m.r64(arg + 8));
    let w = p.m.r64(ctx + 0x20);
    let core = worker(p);
    let core = if p.m.r32(this + 0x88) > core { core } else { 0 };
    let g = p.m.r64(p.m.r64(p.base + SCENE_HOLDER));
    if !p.m.r32(g + 0x948) & 6 != 0 || p.m.r8(g + 0x98) != 0 || p.m.r8(p.m.r64(g + 0x70) + 0x2c4) & 1 == 0 {
        return Ok(());
    }
    let caches = p.m.r64(this + 0x90);
    let x = p.m.r64(g + 0x8f8);
    if x != 0 && ((p.m.r8(x + 0x10) >> 3) & 1 != 0 || p.m.r32(x + 0x58) != 0xfe) {
        return Ok(());
    }
    if (p.m.r8(comp_owner + 0x5a1) >> 2) & 1 != 0 || p.m.r64(t + 8) == 0 {
        return Ok(());
    }
    let n = p.m.i32(comp_owner + 0xf0);
    let list = p.m.r64(comp_owner + 0xf8);
    let found = (0..n.max(0) as usize).map(|k| p.m.r64(list + k * 8)).find(|&c| p.m.r32(c + 0x10) == COMPONENT_TYPE).unwrap_or(0);
    if p.m.r8(p.m.r64(p.base + CAST_GUARD)) & 1 == 0 {
        return Err("per-view (static init)");
    }
    let comp = if found == 0 { 0 } else { p.query(p.base + p.vfn(found, 0), &[found as u64, p.m.r64(p.base + CAST_TYPE) as u64]) as usize };
    let id = p.m.r16(d + 0x18) as u32;
    let Some(tier) = (0..6usize).find(|&k| p.m.r32(scene + 0xad4 + k * 4) == id) else { return Ok(()) };
    let lock = comp_owner + 0x580;
    p.query(p.base + LOCK, &[lock as u64]);
    let r = locked(p, this, arg, t, scene, ent, comp_owner, comp, core, caches, ctx, d, w, tier);
    p.query(p.base + UNLOCK, &[lock as u64]);
    r
}

#[allow(clippy::too_many_arguments)]
unsafe fn locked(
    p: &mut Port,
    this: usize,
    arg: usize,
    t: usize,
    scene: usize,
    ent: usize,
    owner: usize,
    comp: usize,
    core: u32,
    caches: usize,
    ctx: usize,
    d: usize,
    w: usize,
    tier: usize,
) -> R<()> {
    let name = p.m.r64(p.m.r64(p.m.r64(d + 0x68) + 8) + 8);
    let Some((kind, tail)) = name_kind(p, name)? else { return Ok(()) };
    let record = p.m.r64(arg + 0x10);
    if record == 0 {
        return Ok(());
    }
    let prog = p.m.r64(record + 8);
    if prog == 0 {
        return Ok(());
    }
    if p.m.r8(tail) != 0 {
        let len = strlen(p, tail + 1) + 1;
        if len >= 10 && find(p, tail, b"Deformable", len - 9) {
            return Err("per-view deformable");
        }
    }
    // Passes other than 0 / 3 cull without the occluder; pass 7 with the scene's view scale.
    let pass = p.m.r8(ctx + 0x31);
    let main = pass == 3 || pass == 0;
    let occluder = if main { p.m.r64(ent + 0xb8) } else { 0 };
    let scale = if pass == 7 { p.m.f32(scene + 0xbbc) } else { -1.0 };
    // The worker's cache: frame stamp, view | pass << 16 key, then the bit pairs.
    let cache = caches + core as usize * 0x208;
    let frame = p.m.r32(p.m.r64(p.m.r64(p.base + FRAME_HOLDER)) + 0xc);
    if p.m.r32(cache + 0x204) != frame {
        p.m.w::<u32>(cache + 0x204, frame);
        p.m.w::<u32>(cache + 0x200, u32::MAX);
    }
    let key = p.m.r8(ctx + 0x46) as u32 | (pass as u32) << 16;
    if p.m.r32(cache + 0x200) != key {
        p.m.w::<u32>(cache + 0x200, key);
        rebuild(p, t, comp, cache, ctx, occluder, scale)?;
    }
    let defaults = p.m.r64(p.m.r64(d + 8) + 0x180) + p.m.r16(d + 0x18) as usize * 0x80;
    let mut table = [0u64; 32];
    let mut sets = [0u64; 32];
    let mut entries = [-1i32; 32];
    let block_slots = |p: &Port, o: usize| p.m.r32(p.m.r64(prog) + 0x10 + p.m.i32(scene + o) as isize as usize * 4);
    p.bind_uniform_block(ent + 0x30, w, block_slots(p, 0xbac), 0)?;
    p.bind_uniform_block(scene + 0xa50, w, block_slots(p, 0xbb4), 0)?;
    let pb = p.m.r64(p.m.r64(d + 0x68) + p.m.r8(ctx + 0x31) as usize * 0x60 + 0x40);
    let mut k = 0usize;
    while (k as i32) < p.m.i32(pb + 0x308) {
        let i = p.m.r8(pb + 0x18 + k * 0x18) as usize;
        if i < 32 {
            entries[i] = k as i32;
        }
        k += 1;
    }
    let row = scene + tier * 0x20;
    let pick = |p: &Port, o: usize| {
        let i = p.m.i32(row + o);
        if i != -1 && entries.get(i as usize).copied().unwrap_or(-1) == -1 { -1 } else { i }
    };
    let (i_a, i_b, i_c, i_d) = (pick(p, 0xaec), pick(p, 0xaf0), pick(p, 0xaf4), pick(p, 0xaf8));
    let _i_e = pick(p, 0xb08);
    let slots_a = block_slots(p, 0xbb0);
    // The LOD records: the first of t+0x80 whose tag rotates to 0x41, and the first rotating to 0.
    let lods = p.m.r64(t + 0x80);
    let nl = p.m.i32(t + 0x78);
    if nl == 0 {
        return Err("per-view no LOD list");
    }
    let tag = |p: &Port, want: u8| {
        (0..nl as usize).map(|k| p.m.r64(lods + k * 8)).find(|&e| rot(p.m.r8(e)) == want).unwrap_or(p.m.r64(p.base + DEFAULT_LOD))
    };
    let (lod_a, lod_b) = (tag(p, 0x41), tag(p, 0));
    let meshes = owner + 0x128;
    let obj = p.m.r64(meshes);
    let begin = p.vfn(obj, 0x38);
    if begin == 0x2ad0f4c {
        // Primitive restart on, index 0xffff.
        p.call(p.pfn(PFN_SET_PRIMITIVE_RESTART), &[p.m.r64(w + 0xb8) as u64, 1, 0xffff]);
    } else {
        note(begin, 0x38);
        p.helper("per-view begin", begin, &[obj as u64, w as u64])?;
    }
    for i in [i_c, i_d] {
        if i != -1 {
            if let Some(e) = table.get_mut(i as usize) {
                *e = (this + 0xa0) as u64;
            }
            bind_entry(p, pb, w, defaults, &table, entries[i as usize & 31]);
        }
    }
    let _ = lod_b;
    let mut dr = Draw {
        block: comp + 0x118,
        w,
        slots: slots_a,
        idx: [i_a, pick(p, 0xaf4)],
        lod: [lod_a, lod_b],
        sets,
        table,
        pb,
        defaults,
        entries,
        meshes,
    };
    sets = dr.sets;
    let _ = sets;
    let mut drawn = 0u32;
    if kind >= 3 {
        // Pairs (Y: comp+0xb0, X: comp+0xc0) whose two instances are visible: the pair's block element, four slots
        // through the pair's record of its two instance records, the mesh at owner+0x138 / 0x148 (+8 flipped).
        let pl = comp + if kind == 3 { 0xb0 } else { 0xc0 };
        let np = p.m.i32(pl).max(0) as usize;
        let pairs = p.m.r64(pl + 8);
        for k in 0..np {
            let e = p.m.r64(pairs + k * 8);
            let (a, b) = (p.m.r64(e + 8), p.m.r64(e + 0x10));
            let vis = |p: &Port, i: usize| {
                let id = p.m.r16(i) as usize;
                (p.m.r8(i + 5) >> 3) & 1 != 0 && (p.m.r64(cache + ((id >> 2) & 0x3ff8)) as u64 >> ((id << 1) & 0x3e)) & 1 != 0
            };
            if !vis(p, a) || !vis(p, b) {
                continue;
            }
            let mut prec = p.m.r64(e + 0x18);
            if prec == 0 {
                prec = create_pair(p, comp, e);
                if prec == 0 {
                    return Err("per-view pair record");
                }
            }
            p.bind_uniform_block(comp + 0x198, w, slots_a, p.m.r16(e + 4) as i32)?;
            let (ra, rb) = (p.m.r64(prec), p.m.r64(prec + 8));
            for (idx, set, lod) in [(i_a, ra + 8, lod_a), (i_b, rb + 8, lod_a), (i_c, ra + 0x18, lod_b), (i_d, rb + 0x18, lod_b)] {
                set_slot(p, idx, p.m.r64(set) as u64, lod, &mut dr.sets, &mut dr.table, pb, w, defaults, &entries)?;
            }
            let flip = ((p.m.r8(a + 5) | p.m.r8(b + 5)) >> 2) & 1 != 0;
            let base = owner + if kind == 3 { 0x138 } else { 0x148 };
            let mesh = p.m.r64(base + if flip { 8 } else { 0 });
            drawn = drawn.wrapping_add(mesh_draw(p, mesh, w)?.wrapping_sub(2));
        }
    }
    let n = if kind >= 3 { 0 } else { p.m.i32(comp + 0xa0).max(0) as usize };
    let list = p.m.r64(comp + 0xa8);
    for k in 0..n {
        let inst = p.m.r64(list + k * 8);
        let f = p.m.r8(inst + 5);
        if (f >> 3) & 1 == 0 || (f & 3) as u32 != kind {
            continue;
        }
        let id = p.m.r16(inst) as usize;
        let word = p.m.r64(cache + ((id >> 2) & 0x3ff8)) as u64;
        let sh = (id << 1) & 0x3e;
        if (word >> sh) & 1 == 0 || (word >> (sh | 1)) & 1 != 0 {
            continue;
        }
        drawn = drawn.wrapping_add(draw_instance(p, &mut dr, inst)?);
    }
    let end = p.vfn(obj, 0x40);
    if end == 0x2ad0f68 {
        p.call(p.pfn(PFN_SET_PRIMITIVE_RESTART), &[p.m.r64(w + 0xb8) as u64, 0, 0]);
    } else {
        note(end, 0x40);
        p.helper("per-view end", end, &[obj as u64, w as u64])?;
    }
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(drawn));
    Ok(())
}

/// The cache rebuild (main+0xd7d2a8): cleared, view data (main+0x1a358b8), then every instance from last to first:
/// one whose group bit (u16 at +2) is set and whose box passes the view's tests marks its own pair (u16 at +0)
/// visible and not drawn.
unsafe fn rebuild(p: &mut Port, t: usize, comp: usize, cache: usize, ctx: usize, occ: usize, scale: f32) -> R<()> {
    let n = p.m.r32(comp + 0xa0);
    let bytes = ((n.wrapping_mul(2).wrapping_add(7)) >> 3) as usize;
    for k in 0..bytes {
        p.m.w::<u8>(cache + k, 0);
    }
    let mut view = [0u8; 0x120];
    p.query_f(p.base + VIEW_DATA, &[view.as_mut_ptr() as u64, ctx as u64], &[scale]);
    if p.m.r32(t + 0x1df8) != 0 {
        return Err("per-view regions");
    }
    let (lo_off, hi_off) = (0.0f32, 0.0f32);
    p.m.w::<u64>(cache, p.m.r64(cache) as u64 | 3);
    if n == 0 {
        return Ok(());
    }
    let v = view.as_ptr() as usize;
    let list = p.m.r64(comp + 0xa8);
    for k in (0..n as usize).rev() {
        let inst = p.m.r64(list + k * 8);
        let h = p.m.r16(inst + 2) as usize;
        let word = p.m.r64(cache + ((h >> 2) & 0x3ff8)) as u64;
        if (word >> ((h << 1) & 0x3e)) & 1 == 0 {
            continue;
        }
        let half = p.m.f32(inst + 0x10) * 0.5;
        let (cx, cz) = (p.m.f32(inst + 8), p.m.f32(inst + 0xc));
        let y0 = lo_off + p.m.f32(inst + 0x14);
        let (x0, z0, x1) = (cx - half, cz - half, cx + half);
        let y1 = hi_off + p.m.f32(inst + 0x18);
        let z1 = cz + half;
        let plane = |p: &Port, pl: usize, sub: bool| {
            let (nx, ny, nz, d) = (p.m.f32(pl), p.m.f32(pl + 4), p.m.f32(pl + 8), p.m.f32(pl + 0xc));
            let s = nx * if nx > 0.0 { x0 } else { x1 } + ny * if ny > 0.0 { y0 } else { y1 } + nz * if nz > 0.0 { z0 } else { z1 };
            let _ = sub;
            s - d > 0.0
        };
        let culled = if p.m.r8(v) == 0 {
            let c = p.m.r64(v + 0xe0);
            let ci = p.m.i8(v + 2) as i32;
            let ci = if p.m.r32(c + 0x360) > ci as u32 { ci } else { 0 } as i64;
            let planes = (p.m.r64(c + 0x368) as i64 + ci * 0x590) as usize;
            let np = p.m.i32(planes + 0x3f8);
            (0..np.max(0) as usize).any(|k| plane(p, planes + (if k < 0x18 { k } else { 0 }) * 0x10 + 0x3fc, true))
        } else {
            if p.m.r8(v + 1) != 0 {
                let f = |o: usize| p.m.f32(v + o);
                let inside = x0 <= f(0xd0) && f(0xc4) <= x1 && y0 <= f(0xd4) && f(0xc8) <= y1 && z0 <= f(0xd8) && f(0xcc) <= z1;
                if !inside {
                    continue;
                }
            }
            let np = p.m.r8(v + 3) as usize;
            (0..np).any(|k| plane(p, v + 4 + k * 0x10, false))
        };
        if culled {
            continue;
        }
        if occ != 0 && !p.m.r32(occ) & 0x440 == 0 {
            let (ex, ey, ez) = ((x1 - x0) * 0.5, (y1 - y0) * 0.5, (z1 - z0) * 0.5);
            let (mx, my, mz) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5, (z0 + z1) * 0.5);
            let o = |k: usize| p.m.f32(occ + k);
            let dist = o(0x78) + (mx * o(0x6c) + my * o(0x70) + mz * o(0x74));
            let ext = abs(ex * o(0x6c)) + abs(ey * o(0x70)) + abs(ez * o(0x74));
            let near = -dist - ext;
            if !(o(0x160) > near) {
                let a = o(0x58) + (mx * o(0x4c) + my * o(0x50) + mz * o(0x54));
                let b = o(0x68) + (mx * o(0x5c) + my * o(0x60) + mz * o(0x64));
                let ea = abs(ex * o(0x4c)) + abs(ey * o(0x50)) + abs(ez * o(0x54));
                let eb = abs(ex * o(0x5c)) + abs(ey * o(0x60)) + abs(ez * o(0x64));
                // s5 / s6 still hold the extent and the near distance of the test above.
                let r = p.query_f(p.base + OCCLUSION, &[occ as u64], &[a, b, dist, ea, eb, ext, near]);
                if r & 7 == 1 {
                    continue;
                }
            }
        }
        let id = p.m.r16(inst) as usize;
        if id <= 0x7ff {
            let a = cache + ((id >> 2) & 0x3ff8);
            p.m.w::<u64>(a, p.m.r64(a) as u64 | 1u64 << ((id << 1) & 0x3e));
        }
        let bit = 1 | id << 1;
        if bit <= 0xfff {
            let a = cache + (id >> 5) * 8;
            p.m.w::<u64>(a, p.m.r64(a) as u64 & !(1u64 << (bit & 63)));
        }
    }
    Ok(())
}


const SCENE_CAST_GUARD: usize = 0x47254d0;
const SCENE_CAST_TYPE: usize = 0x47254c8;
const COMPONENT_TYPE_B: u32 = 0x24950135;

/// A texture set slot of main+0x24b2a04's instance draw: when the instance's set differs from the slot's last one,
/// the set's texture for the LOD bit goes to the table and the program entry is bound. Err when the LOD bit is
/// missing (the game would read a null texture).
#[allow(clippy::too_many_arguments)]
unsafe fn set_slot(p: &mut Port, idx: i32, set: u64, lod: usize, sets: &mut [u64; 32], table: &mut [u64; 32], pb: usize, w: usize, defaults: usize, entries: &[i32; 32]) -> R<bool> {
    if idx == -1 {
        return Ok(false);
    }
    let i = idx as usize & 31;
    if sets[i] == set {
        return Ok(false);
    }
    sets[i] = set;
    let s = set as usize;
    let bits = p.m.r32(s + 0xc);
    let b = p.m.r8(lod + 1) as u32 & 31;
    if (bits >> b) & 1 == 0 {
        return Err("per-view texture set");
    }
    let m = bits & !(u32::MAX << b);
    let n = if m >= 2 { m.count_ones() } else { m };
    let tex = p.m.r64(p.m.r64(s + 0x10) + n as usize * 8);
    table[i] = (p.m.r64(tex + 8) + 8) as u64;
    bind_entry(p, pb, w, defaults, table, entries[i]);
    Ok(true)
}

/// The first set of the chain from `c` (next at +0x18) of type <= 2 with a present texture for the LOD, or 0.
unsafe fn chain(p: &Port, mut c: usize, lod: usize) -> usize {
    while c != 0 {
        if p.m.r8(c) & 0xf <= 2 {
            let bits = p.m.r32(c + 0xc);
            let b = p.m.r8(lod + 1) as u32 & 31;
            if (bits >> b) & 1 != 0 {
                let m = bits & !(u32::MAX << b);
                let nn = if m >= 2 { m.count_ones() } else { m };
                let tex = p.m.r64(p.m.r64(c + 0x10) + nn as usize * 8);
                if tex != 0 {
                    let s18 = p.m.r32(tex + 0x18);
                    if (p.m.r8(tex + 0x12) >> 2) & 1 != 0 || (p.m.r64(tex + 8) != 0 && s18 != 0 && s18 >> 30 == 0) {
                        return c;
                    }
                }
            }
        }
        c = p.m.r64(c + 0x18);
    }
    0
}

/// main+0x24b2a04(this, slot, arg): as main+0xd7c1d8 with the scene's 8 tiers (+0x1244), exact LOD names
/// (..NormalNear / NormalMiddle / NormalFar) and an inlined instance draw over seven texture set slots.
pub(super) unsafe fn draw_b(p: &mut Port, this: usize, slot: u32, arg: usize) -> R<()> {
    if !p.m.predict() {
        p.call(p.base + 0x24b2a04, &[this as u64, slot as u64, arg as u64]);
        return Ok(());
    }
    let t = p.m.r64(this + 0x18);
    let first = p.m.r64(p.m.r64(t + 0x70));
    if p.m.r8(p.base + SCENE_CAST_GUARD) & 1 == 0 {
        return Err("per-view (static init)");
    }
    let scene = if first == 0 { 0 } else { p.query(p.base + p.vfn(first, 0), &[first as u64, (p.base + SCENE_CAST_TYPE) as u64]) as usize };
    let vi = if p.m.r32(t + 0xea0) > slot { slot } else { 0 } as i32 as i64;
    let ent = (p.m.r64(t + 0xea8) as i64 + vi * 0xe0) as usize;
    let owner = p.m.r64(ent + 8);
    let (ctx, d) = (p.m.r64(arg), p.m.r64(arg + 8));
    let w = p.m.r64(ctx + 0x20);
    let core = worker(p);
    let core = if p.m.r32(this + 0x88) > core { core } else { 0 };
    let g = p.m.r64(p.m.r64(p.base + SCENE_HOLDER));
    if !p.m.r32(g + 0x948) & 6 != 0 || p.m.r8(g + 0x98) != 0 || p.m.r8(p.m.r64(g + 0x70) + 0x2c4) & 1 == 0 {
        return Ok(());
    }
    let caches = p.m.r64(this + 0x90);
    let x = p.m.r64(g + 0x8f8);
    if x != 0 && (p.m.r8(x + 0x10) & 1 != 0 || p.m.r32(x + 0x58) != 0xfe) {
        return Ok(());
    }
    if (p.m.r8(owner + 0x5a1) >> 2) & 1 != 0 || p.m.r64(t + 8) == 0 {
        return Ok(());
    }
    let n = p.m.i32(owner + 0xf0);
    let list = p.m.r64(owner + 0xf8);
    let found = (0..n.max(0) as usize).map(|k| p.m.r64(list + k * 8)).find(|&c| p.m.r32(c + 0x10) == COMPONENT_TYPE_B).unwrap_or(0);
    if p.m.r8(p.m.r64(p.base + CAST_GUARD)) & 1 == 0 {
        return Err("per-view (static init)");
    }
    let comp = if found == 0 { 0 } else { p.query(p.base + p.vfn(found, 0), &[found as u64, p.m.r64(p.base + CAST_TYPE) as u64]) as usize };
    let id = p.m.r16(d + 0x18) as u32;
    let Some(tier) = (0..8usize).find(|&k| p.m.r32(scene + 0x1244 + k * 4) == id) else { return Ok(()) };
    let name = p.m.r64(p.m.r64(p.m.r64(d + 0x68) + 8) + 8);
    let kind = match p.m.r8(name + 2) {
        b'N' => {
            if (0..5).any(|k| p.m.r8(name + 3 + k) != b"ormal"[k]) {
                return Ok(());
            }
            let rest = |s: &[u8]| (0..s.len()).all(|k| p.m.r8(name + 9 + k) == s[k]);
            match p.m.r8(name + 8) {
                b'N' if rest(b"ear") => 0,
                b'M' if rest(b"iddle") => 1,
                b'F' if rest(b"ar") => 2,
                _ => return Ok(()),
            }
        }
        b'D' => {
            if (0..9).any(|k| p.m.r8(name + 3 + k) != b"egeneracy"[k]) {
                return Ok(());
            }
            let z = |c: u8| p.m.r8(name + 0xc) == c && p.m.r8(name + 0xd) == 0;
            if z(b'Y') {
                3
            } else if z(b'X') {
                4
            } else {
                return Ok(());
            }
        }
        _ => return Ok(()),
    };
    // 3 / 4: the Degeneracy pairs (Y / X); their name tail starts at +0xc.
    let tail = if kind >= 3 { name + 0xc } else { name + 8 };
    let record = p.m.r64(arg + 0x10);
    if record == 0 {
        return Ok(());
    }
    let prog = p.m.r64(record + 8);
    if prog == 0 {
        return Ok(());
    }
    if p.m.r8(tail) != 0 {
        let len = strlen(p, tail + 1) + 1;
        if len >= 10 && find(p, tail, b"Deformable", len - 9) {
            return Err("per-view deformable");
        }
    }
    let pass = p.m.r8(ctx + 0x31);
    let main = pass == 3 || pass == 0;
    let occluder = if main { p.m.r64(ent + 0xb8) } else { 0 };
    let scale = if pass == 7 { p.m.f32(scene + 0x13f0) } else { -1.0 };
    let cache = caches + core as usize * 0x208;
    let frame = p.m.r32(p.m.r64(p.m.r64(p.base + FRAME_HOLDER)) + 0xc);
    if p.m.r32(cache + 0x204) != frame {
        p.m.w::<u32>(cache + 0x200, u32::MAX);
        p.m.w::<u32>(cache + 0x204, frame);
    }
    let key = p.m.r8(ctx + 0x46) as u32 | (pass as u32) << 16;
    if p.m.r32(cache + 0x200) != key {
        p.m.w::<u32>(cache + 0x200, key);
        rebuild(p, t, comp, cache, ctx, occluder, scale)?;
    }
    let defaults = p.m.r64(p.m.r64(d + 8) + 0x180) + p.m.r16(d + 0x18) as usize * 0x80;
    let mut table = [0u64; 32];
    let mut sets = [0u64; 32];
    let mut entries = [-1i32; 32];
    let block_slots = |p: &Port, o: usize| p.m.r32(p.m.r64(prog) + 0x10 + p.m.i32(scene + o) as isize as usize * 4);
    p.bind_uniform_block(ent + 0x30, w, block_slots(p, 0x13e4), 0)?;
    let pb = p.m.r64(p.m.r64(d + 0x68) + pass as usize * 0x60 + 0x40);
    let mut k = 0usize;
    while (k as i32) < p.m.i32(pb + 0x308) {
        let i = p.m.r8(pb + 0x18 + k * 0x18) as usize;
        if i < 32 {
            entries[i] = k as i32;
        }
        k += 1;
    }
    let row = scene + tier * 0x30;
    let pick = |p: &Port, o: usize| {
        let i = p.m.i32(row + o);
        if i != -1 && entries.get(i as usize).copied().unwrap_or(-1) == -1 { -1 } else { i }
    };
    let (i_1b4, i_1b0, i_1ac, i_1a8) = (pick(p, 0x1264), pick(p, 0x126c), pick(p, 0x1270), pick(p, 0x1274));
    let (i_1a4, i_1a0, i_19c) = (pick(p, 0x1278), pick(p, 0x127c), pick(p, 0x1280));
    let i_40 = pick(p, 0x1268);
    let lods = p.m.r64(t + 0x80);
    let nl = p.m.i32(t + 0x78);
    if nl == 0 {
        return Err("per-view no LOD list");
    }
    let tag = |p: &Port, want: u8| {
        (0..nl as usize).map(|k| p.m.r64(lods + k * 8)).find(|&e| rot(p.m.r8(e)) == want).unwrap_or(p.m.r64(p.base + DEFAULT_LOD))
    };
    let (lod_0, lod_1, lod_20, lod_50, lod_41) = (tag(p, 0), tag(p, 1), tag(p, 0x20), tag(p, 0x50), tag(p, 0x41));
    let meshes = owner + 0x128;
    let obj = p.m.r64(meshes);
    let begin = p.vfn(obj, 0x38);
    if begin == 0x2ad0f4c {
        p.call(p.pfn(PFN_SET_PRIMITIVE_RESTART), &[p.m.r64(w + 0xb8) as u64, 1, 0xffff]);
    } else {
        note(begin, 0x38);
        p.helper("per-view begin", begin, &[obj as u64, w as u64])?;
    }
    if i_1ac != -1 {
        table[i_1ac as usize & 31] = (this + 0xa0) as u64;
        bind_entry(p, pb, w, defaults, &table, entries[i_1ac as usize & 31]);
    }
    let slots_a = block_slots(p, 0x13e8);
    let mut drawn = 0u32;
    if kind >= 3 {
        // Pairs (Y: comp+0xb0, X: comp+0xc0; {_, _, instance A, instance B, record of their two records}), drawn
        // when both instances are visible.
        let pl = comp + if kind == 3 { 0xb0 } else { 0xc0 };
        let np = p.m.i32(pl).max(0) as usize;
        let pairs = p.m.r64(pl + 8);
        for k in 0..np {
            let e = p.m.r64(pairs + k * 8);
            let (a, b) = (p.m.r64(e + 8), p.m.r64(e + 0x10));
            let vis = |p: &Port, i: usize| {
                let id = p.m.r16(i) as usize;
                (p.m.r8(i + 5) >> 3) & 1 != 0 && (p.m.r64(cache + ((id >> 2) & 0x3ff8)) as u64 >> ((id << 1) & 0x3e)) & 1 != 0
            };
            if !vis(p, a) || !vis(p, b) {
                continue;
            }
            let mut prec = p.m.r64(e + 0x18);
            if prec == 0 {
                prec = create_pair(p, comp, e);
                if prec == 0 {
                    return Err("per-view pair record");
                }
            }
            p.bind_uniform_block(comp + 0x198, w, slots_a, p.m.r16(e + 4) as i32)?;
            let ra = p.m.r64(prec);
            set_slot(p, i_1b4, p.m.r64(ra + 8) as u64, lod_0, &mut sets, &mut table, pb, w, defaults, &entries)?;
            set_slot(p, i_40, p.m.r64(p.m.r64(prec + 8) + 8) as u64, lod_0, &mut sets, &mut table, pb, w, defaults, &entries)?;
            set_slot(p, i_1a8, p.m.r64(ra + 0x18) as u64, lod_1, &mut sets, &mut table, pb, w, defaults, &entries)?;
            let c = chain(p, p.m.r64(ra + 0x18), lod_1);
            if i_1a4 != -1 && sets[i_1a4 as usize & 31] != c as u64 {
                if c == 0 {
                    return Err("per-view empty set chain");
                }
                set_slot(p, i_1a4, c as u64, lod_1, &mut sets, &mut table, pb, w, defaults, &entries)?;
            }
            if set_slot(p, i_1a0, p.m.r64(ra + 0x28) as u64, lod_20, &mut sets, &mut table, pb, w, defaults, &entries)? && i_19c != -1 {
                table[i_19c as usize & 31] = table[i_1a0 as usize & 31];
                bind_entry(p, pb, w, defaults, &table, entries[i_19c as usize & 31]);
            }
            for (idx, o, lod) in [(i_1b0, 0x38, lod_41), (i_1ac, 0x48, lod_50)] {
                let s = p.m.r64(ra + o);
                if idx != -1 && s != 0 {
                    set_slot(p, idx, s as u64, lod, &mut sets, &mut table, pb, w, defaults, &entries)?;
                }
            }
            let flip = ((p.m.r8(a + 5) | p.m.r8(b + 5)) >> 2) & 1 != 0;
            let base = meshes + if kind == 3 { 0x10 } else { 0x20 };
            let mesh = p.m.r64(base + if flip { 8 } else { 0 });
            drawn = drawn.wrapping_add(mesh_draw(p, mesh, w)?.wrapping_sub(2));
        }
    }
    let n = if kind >= 3 { 0 } else { p.m.i32(comp + 0xa0).max(0) as usize };
    let list = p.m.r64(comp + 0xa8);
    for k in 0..n {
        let inst = p.m.r64(list + k * 8);
        let f = p.m.r8(inst + 5);
        if (f >> 3) & 1 == 0 || (f & 3) as u32 != kind {
            continue;
        }
        let iid = p.m.r16(inst) as usize;
        let word = p.m.r64(cache + ((iid >> 2) & 0x3ff8)) as u64;
        let sh = (iid << 1) & 0x3e;
        if (word >> sh) & 1 == 0 || (word >> (sh | 1)) & 1 != 0 {
            continue;
        }
        let mut rec = p.m.r64(inst + 0x78);
        if rec == 0 {
            rec = create_record(p, comp, inst);
            if rec == 0 {
                return Err("per-view instance record");
            }
        }
        p.bind_uniform_block(comp + 0x118, w, slots_a, iid as i32)?;
        // The first slot's set is the instance's own (+0x28); the others are the record's.
        set_slot(p, i_1b4, p.m.r64(inst + 0x28) as u64, lod_0, &mut sets, &mut table, pb, w, defaults, &entries)?;
        set_slot(p, i_1a8, p.m.r64(rec + 0x18) as u64, lod_1, &mut sets, &mut table, pb, w, defaults, &entries)?;
        let c = chain(p, p.m.r64(rec + 0x18), lod_1);
        if i_1a4 != -1 && sets[i_1a4 as usize & 31] != c as u64 {
            if c == 0 {
                return Err("per-view empty set chain");
            }
            set_slot(p, i_1a4, c as u64, lod_1, &mut sets, &mut table, pb, w, defaults, &entries)?;
        }
        if set_slot(p, i_1a0, p.m.r64(rec + 0x28) as u64, lod_20, &mut sets, &mut table, pb, w, defaults, &entries)? && i_19c != -1 {
            table[i_19c as usize & 31] = table[i_1a0 as usize & 31];
            bind_entry(p, pb, w, defaults, &table, entries[i_19c as usize & 31]);
        }
        for (idx, o, lod) in [(i_1b0, 0x38, lod_41), (i_1ac, 0x48, lod_50)] {
            let s = p.m.r64(rec + o);
            if idx != -1 && s != 0 {
                set_slot(p, idx, s as u64, lod, &mut sets, &mut table, pb, w, defaults, &entries)?;
            }
        }
        let mesh = p.m.r64(meshes + if (f >> 2) & 1 != 0 { 8 } else { 0 });
        drawn = drawn.wrapping_add(mesh_draw(p, mesh, w)?.wrapping_sub(2));
    }
    let end = p.vfn(obj, 0x40);
    if end == 0x2ad0f68 {
        p.call(p.pfn(PFN_SET_PRIMITIVE_RESTART), &[p.m.r64(w + 0xb8) as u64, 0, 0]);
    } else {
        note(end, 0x40);
        p.helper("per-view end", end, &[obj as u64, w as u64])?;
    }
    p.m.w::<u32>(ctx + 0x34, p.m.r32(ctx + 0x34).wrapping_add(drawn));
    Ok(())
}
