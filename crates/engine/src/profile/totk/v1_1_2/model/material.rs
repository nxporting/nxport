// A model unit's material state (main+0x7b42f0), as the game records it into the material's cached command list
// (main+0x7b3c80: begin recording, this, end recording, the handle kept at +0x48 of the material's slot). The draw
// path calls those lists; this is what they contain, from the game's material objects.

use super::draw::{Port, R};

const PFN_DEVICE_GET_TEXTURE_HANDLE: usize = 0x466d428;
const PFN_BIND_TEXTURE: usize = 0x466e008;
const PFN_BUFFER_GET_ADDRESS: usize = 0x466d818;
const DEVICE_HOLDER: usize = 0x462bbe8;

/// main+0x7b42f0(d, w, entry, shader set, a, b): the unit `d`'s material (entry +8): the material's two extra
/// textures, the shader set's textures from the unit's defaults, the material's blocks (+0x1c slots: the defaults'
/// buffer, +0x40 block objects of the model's +0xd8 mask, +0x28 slots: the unit's shape buffer, +0x24 slots: the
/// model's +0x170 buffer).
pub(super) unsafe fn bind(p: &mut Port, d: usize, w: usize, entry: usize, set: usize, a: i32, b: i32) -> R<()> {
    let mat = p.m.r64(entry + 8);
    if mat == 0 {
        return Ok(());
    }
    let defaults = p.m.r64(p.m.r64(d + 8) + 0x180) + p.m.r16(d + 0x18) as usize * 0x80;
    // The defaults' two optional textures at the material's slots (+0x10 -> +0x8c / +0x90).
    for (o, so) in [(0x18usize, 0x8cusize), (0x20, 0x90)] {
        let tex = p.m.r64(p.m.r64(defaults + 0x68) + o);
        if tex == 0 {
            continue;
        }
        let slots = p.m.r32(p.m.r64(mat + 0x10) + so);
        if slots == u32::MAX {
            continue;
        }
        p.texture_ready(tex, "material texture still finishing")?;
        bind_texture_slots(p, w, slots, tex + 0x58, tex + 0xea);
    }
    if set != 0 && p.m.i32(set + 0x308) >= 1 {
        super::grid::bind_textures(p, w, set, defaults, &[0; 32], u32::MAX)?;
    }
    let cmd = p.m.r64(w + 0xb8);
    let defaults = p.m.r64(p.m.r64(d + 8) + 0x180) + p.m.r16(d + 0x18) as usize * 0x80;
    // The defaults' buffer (+0x60 bytes, +0x40 buffers 0x48 apart when +8 bit 0), element b.
    let size = p.m.r64(defaults + 0x60);
    if size != 0 {
        let bufs = p.m.r64(defaults + 0x40);
        let e = if bufs != 0 && p.m.r16(defaults + 8) & 1 != 0 { bufs + b as i64 as usize * 0x48 } else { 0 };
        let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[p.m.r64(e + 8) as u64]);
        bind_slots(p, cmd, p.m.r32(p.m.r64(mat) + 0x1c), addr, size as u32 as u64);
    }
    // The model's block objects (+0x98, eight, those with material slots at +0x14 of [mat+8]).
    let model = p.m.r64(d + 8);
    if p.m.r16(model + 0xd8) & 0xf != 0 {
        return Err("material blocks (main+0x766ef4)");
    }
    if p.m.r16(model + 0xd8) & 0xff0 != 0 {
        for k in 0..8usize {
            let slots = p.m.r32(p.m.r64(mat + 8) + k * 4 + 0x14);
            if slots == u32::MAX {
                continue;
            }
            let o = p.m.r64(model + 0x98 + k * 8);
            if o == 0 {
                continue;
            }
            let n = (p.m.r8(o + 0x42) & 0xf) as i32;
            // SDIV by zero gives 0 (the remainder is then b).
            let sel = if b != -1 { if n == 0 { b } else { b.wrapping_rem(n) } } else { (p.m.r8(o + 0x42) >> 4) as i32 };
            let count = p.m.r16(o + 0x40) as i64;
            let size = p.m.r32(o + 0x38);
            let al = p.block_alignment(o)?;
            let al2 = p.block_alignment(o)?;
            let stride = ((size as i32).wrapping_add(al).wrapping_sub(1) & al2.wrapping_neg()) as u32 as u64;
            let addr = (p.m.r64(o + 0x78) as u64).wrapping_add(stride.wrapping_mul((sel as i64 * count) as u64));
            for (stage, sh) in [(0, 0), (1, 8), (2, 16), (5, 24)] {
                let s = (slots >> sh) & 0xff;
                if s != 0xff {
                    p.bind_block(o, cmd, stage, s as i8 as i32, addr, size)?;
                }
            }
        }
    }
    // The unit's shape buffer (the model's +0x178 table, 0x70 apart by +0x16; +0x20 buffers; per-unit element when
    // +0xd: +0xf * a + b), 0x100 bytes.
    let t = p.m.r64(model + 0x178) + p.m.r16(d + 0x16) as usize * 0x70;
    let idx = if p.m.r8(t + 0xd) != 0 { (p.m.r8(t + 0xf) as i32).wrapping_mul(a).wrapping_add(b) } else { b };
    let bufs = p.m.r64(t + 0x20);
    let e = if bufs != 0 { bufs + idx as i64 as usize * 0x48 } else { 0 };
    let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[p.m.r64(e + 8) as u64]);
    bind_slots(p, cmd, p.m.r32(p.m.r64(mat) + 0x28), addr, 0x100);
    // The model's +0x170 buffer (+0x40 bytes, +0x38 buffers), element b.
    let m170 = p.m.r64(model + 0x170);
    let size = p.m.r64(m170 + 0x40);
    if size != 0 {
        let bufs = p.m.r64(m170 + 0x38);
        let e = if bufs != 0 { bufs + b as i64 as usize * 0x48 } else { 0 };
        let addr = p.query(p.pfn(PFN_BUFFER_GET_ADDRESS), &[p.m.r64(e + 8) as u64]);
        bind_slots(p, cmd, p.m.r32(p.m.r64(mat) + 0x24), addr, size as u32 as u64);
    }
    extra_shapes(p, d)
}

/// The unit's extra shape list ([d+0x68]+0xb00): not ported.
unsafe fn extra_shapes(p: &Port, d: usize) -> R<()> {
    if p.m.r64(p.m.r64(d + 0x68) + 0xb00) != 0 {
        return Err("material extra shapes");
    }
    Ok(())
}

/// A uniform buffer at the stage slots packed in `slots` (stages 0, 1, 2 and 5).
unsafe fn bind_slots(p: &mut Port, cmd: usize, slots: u32, addr: u64, size: u64) {
    for (stage, sh) in [(0, 0), (1, 8), (2, 16), (5, 24)] {
        let s = (slots >> sh) & 0xff;
        if s != 0xff {
            p.bind_uniform_buffer(cmd, stage, s as i8 as i32, addr, size);
        }
    }
}

/// main+0xcc9f14(w, slots, &texture id, &sampler id): the texture handle at the stage slots packed in `slots`.
unsafe fn bind_texture_slots(p: &mut Port, w: usize, slots: u32, id: usize, sampler: usize) {
    let tid = p.m.i32(id);
    let smp = p.m.r16(sampler) as i16 as i32;
    if tid == -1 || smp == -1 {
        return;
    }
    let dev = p.m.r64(p.m.r64(p.m.r64(p.base + DEVICE_HOLDER)) + 0x30);
    let h = p.query(p.pfn(PFN_DEVICE_GET_TEXTURE_HANDLE), &[dev as u64, tid as u32 as u64, smp as u32 as u64]);
    let cmd = p.m.r64(w + 0xb8) as u64;
    for (stage, sh) in [(0u64, 0), (1, 8), (2, 16), (5, 24)] {
        let s = (slots >> sh) & 0xff;
        if s != 0xff {
            p.call(p.pfn(PFN_BIND_TEXTURE), &[cmd, stage, s as i8 as i64 as u64, h]);
        }
    }
}

/// Material lists the draw path calls whose commands match a bind from the game's objects now: (same, different).
pub(super) static LISTS: [std::sync::atomic::AtomicU64; 2] = [std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0)];

/// Verify: the material list at `handles` (recorded by main+0x7b3c80 for pass `pass`, slot `b`: the pass's program
/// list called, then the bind) against the same from the game's objects now.
pub(super) unsafe fn check_list(p: &Port, d: usize, w: usize, pass: usize, b: i32, handles: usize) {
    use std::sync::atomic::Ordering;
    let Some(recorded) = crate::nvn::api::list_texts(p.m.r64(handles) as u64) else { return };
    let e = p.m.r64(d + 0x68) + pass * 0x60;
    let mut q = Port::new(super::mem::Mem::predicting(), p.base, false);
    let mut r = Ok(());
    let mut now = crate::nvn::api::capture_dry_nested(|| r = bind(&mut q, d, w, e + 0x18, p.m.r64(e + 0x40), 0, b));
    if r.is_err() {
        return;
    }
    // main+0x7b3c80 calls the pass's program list ([e+0x28]+0x48 when its +0x50 bit 0) first.
    let prog = p.m.r64(e + 0x28);
    if prog != 0 && p.m.r8(prog + 0x50) & 1 != 0 {
        now.insert(0, format!("nvnCommandBufferCallCommands CallCommands([{}])", p.m.r64(prog + 0x48)));
    }
    let k = if now == recorded { 0 } else { 1 };
    if LISTS[k].fetch_add(1, Ordering::Relaxed) < 3 && k == 1 {
        let i = (0..now.len().max(recorded.len())).find(|&i| now.get(i) != recorded.get(i)).unwrap_or(0);
        println!(
            "[model-draw] material list differs from its material at {i}: now {:?}, recorded {:?}",
            now.get(i),
            recorded.get(i)
        );
    }
}

type Description = std::sync::Arc<Vec<crate::nvn::cmd::Cmd>>;
type Recording = std::sync::Weak<crate::nvn::api::RecordedCommandList>;

/// Is `rec` still the recording list `handle` refers to?
fn current(handle: u64, rec: &Recording) -> bool {
    crate::nvn::api::command_list_peek(handle).is_some_and(|r| rec.upgrade().is_some_and(|w| std::sync::Arc::ptr_eq(&r, &w)))
}

/// Native material descriptions by list handle, with the recording they describe and the pass's program list (the
/// game records a changed material or vertex input again at the same command memory, which keeps the handle).
static DESCRIBED: std::sync::Mutex<Option<std::collections::HashMap<u64, (Recording, Option<(u64, Recording)>, Description)>>> =
    std::sync::Mutex::new(None);

/// The material list `handle` (pass `pass`, slot `b` of unit `d`) described from the game's objects: the model's
/// vertex input (the pass's program list), then the bind's typed commands. Rebuilt when the game records either list
/// again. None when a
/// command is not of the kinds the executor applies (the list is run instead).
pub(super) unsafe fn described(p: &Port, handle: u64, d: usize, w: usize, pass: usize, b: i32) -> Option<Description> {
    let recording = crate::nvn::api::command_list_peek(handle)?;
    if let Some((rec, program, m)) = DESCRIBED.lock().unwrap().get_or_insert_with(Default::default).get(&handle) {
        if rec.upgrade().is_some_and(|r| std::sync::Arc::ptr_eq(&r, &recording))
            && program.as_ref().is_none_or(|(h, p)| current(*h, p))
        {
            return Some(m.clone());
        }
    }
    let e = p.m.r64(d + 0x68) + pass * 0x60;
    let mut q = Port::new(super::mem::Mem::applying(), p.base, false);
    let mut r = Ok(());
    let cmds = crate::nvn::api::capture_typed_dry(|| r = bind(&mut q, d, w, e + 0x18, p.m.r64(e + 0x40), 0, b))?;
    r.ok()?;
    use crate::nvn::cmd::Cmd;
    // The executor applies these kinds only.
    if !cmds.iter().all(|c| matches!(c, Cmd::BindTexture { .. } | Cmd::BindUniformBuffer { .. } | Cmd::BindStorageBuffer { .. })) {
        return None;
    }
    let mut all = Vec::with_capacity(cmds.len() + 1);
    let mut program = None;
    let prog = p.m.r64(e + 0x28);
    if prog != 0 && p.m.r8(prog + 0x50) & 1 != 0 {
        // The pass's program list: the model's vertex input as sub_764f4c binds it.
        let list = p.m.r64(prog + 0x48) as u64;
        program = Some((list, std::sync::Arc::downgrade(&crate::nvn::api::command_list_peek(list)?)));
        if let Some(v) = p.vertex_input(p.m.r64(d + 0x68)) {
            all.push(Cmd::VertexInput(Box::new(v)));
        }
    }
    all.extend(cmds);
    let m = std::sync::Arc::new(all);
    let mut g = DESCRIBED.lock().unwrap();
    let map = g.get_or_insert_with(Default::default);
    if map.len() > 1 << 17 {
        map.retain(|_, (rec, _, _)| rec.strong_count() > 0);
    }
    map.insert(handle, (std::sync::Arc::downgrade(&recording), program, m.clone()));
    Some(m)
}
