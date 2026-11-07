// Texture states (agl::TextureData's 0xb0 bytes: a 0x30-byte head, a 0x28-byte block, the registered slot id +0x58
// with its description +0x60..+0x78 and block +0x78, two words +0xa0) as the game copies them between its records and
// the render target objects it builds on the stack, with the slot reference counts and free lists; the stack objects
// themselves (host buffers the port owns) and the temporary targets of the dynamic texture allocator.

use super::draw::{Port, R};

/// [[holder]]: the texture slot table (+0x538, 0x20-byte entries, count +0x530, first id +0x5a2), the live count
/// (+0x5a8), the free lists (+0x540, +0x548 per kind 0x10 apart, kind byte +0x5a0) and a mutex (+0x510).
const SLOT_HOLDER: usize = 0x462bbf0;
const REGISTER_SLOT: usize = 0xff09f4;
const LOCK: usize = 0x2b15370;
const UNLOCK: usize = 0x2b15380;
/// "countup", the registration's name.
const COUNTUP: usize = 0x37b4a9b;
/// [[holder]]: the temporary target pool main+0x7f0ad4 allocates from.
const TARGET_POOL: usize = 0x4634e70;
const ALLOC_TARGET: usize = 0x7f0ad4;
/// "agl::TextureData".
const TEXTURE_DATA_NAME: usize = 0x37ccf1f;

pub(super) const BUFFER_VTABLE: usize = 0x4634e78;
pub(super) const COLOR_VTABLE: usize = 0x4634e80;
pub(super) const DEPTH_VTABLE: usize = 0x4634e88;
/// Target object virtual +0x10: colour (the format class flag at +0xcf) and depth (texture builder queries); both only
/// write the object.
const SETUP_COLOR: usize = 0x2a2f530;
const SETUP_DEPTH: usize = 0x2a2f5b8;

const SLOT_ID: usize = 0x58;
const SLOT_DESC: usize = 0x60;
const SLOT_BLOCK: usize = 0x78;
const TAIL: usize = 0xa0;

pub(super) const TARGET_BYTES: usize = 0xf0;
pub(super) const BUFFER_BYTES: usize = 0x80;

/// A zeroed host buffer the port's memory owns (game code called with it and `p.m` see the same bytes).
pub(super) struct Host(Box<[u64]>);

impl Host {
    pub(super) fn new(p: &mut Port, bytes: usize) -> Host {
        let h = Host(vec![0u64; bytes.div_ceil(8)].into_boxed_slice());
        p.m.own(h.addr(), bytes);
        h
    }

    pub(super) fn addr(&self) -> usize {
        self.0.as_ptr() as usize
    }

    pub(super) fn release(self, p: &mut Port) {
        p.m.disown(self.addr());
    }
}

/// The slot table entry of texture id `id` (an id below the first or past the count reads entry 0).
unsafe fn slot_entry(p: &Port, holder: usize, id: i32) -> usize {
    let mut i = (id as u32).wrapping_sub(p.m.r16(holder + 0x5a2) as u32);
    if p.m.r32(holder + 0x530) <= i {
        i = 0;
    }
    (p.m.r64(holder + 0x538) as i64 + i as i32 as i64 * 0x20) as usize
}

/// The game's exclusive-monitor `*a += d`, returning the old value.
unsafe fn fetch_add(p: &mut Port, a: usize, d: i32) -> i32 {
    let old = p.m.i32(a);
    p.m.w::<i32>(a, old.wrapping_add(d));
    old
}

/// State `s`'s slot reference dropped: the entry's count -1, on the last reference the live count -1 and the entry onto
/// the free list of its kind (under the holder's mutex); the state's description cleared.
pub(super) unsafe fn release(p: &mut Port, s: usize) {
    let id = p.m.i32(s + SLOT_ID);
    if id == -1 {
        return;
    }
    let holder = p.m.r64(p.m.r64(p.base + SLOT_HOLDER));
    let e = slot_entry(p, holder, id);
    if fetch_add(p, e, -1) == 1 {
        fetch_add(p, holder + 0x5a8, -1);
        if !p.m.predict() {
            p.query(p.base + LOCK, &[(holder + 0x510) as u64]);
        }
        let k = match p.m.r8(holder + 0x5a0) as usize {
            k if k > 1 => 0,
            k => k,
        };
        let l = holder + k * 0x10;
        let n = p.m.i32(l + 0x540);
        if n < p.m.i32(l + 0x544) {
            p.m.w::<u64>(p.m.r64(l + 0x548) + n as isize as usize * 8, e as u64);
            p.m.w::<i32>(l + 0x540, n + 1);
        }
        if !p.m.predict() {
            p.query(p.base + UNLOCK, &[(holder + 0x510) as u64]);
        }
    }
    p.m.w::<i32>(s + SLOT_ID, -1);
    p.m.w::<u32>(s + SLOT_DESC, 0);
    for k in 0..7 {
        p.m.w::<u64>(s + 0x68 + k * 8, 0);
    }
}

/// State `s`'s slot reference counted up: an entry that was at 0 is registered again by the game (main+0xff09f4 with
/// the description, the state's block and "countup"), which gives the new id.
pub(super) unsafe fn acquire(p: &mut Port, s: usize) -> R<()> {
    let id = p.m.i32(s + SLOT_ID);
    if id == -1 {
        return Ok(());
    }
    let holder = p.m.r64(p.m.r64(p.base + SLOT_HOLDER));
    let e = slot_entry(p, holder, id);
    if fetch_add(p, e, 1) == 0 {
        p.m.w::<i32>(e, 0);
        let desc: [u32; 6] = std::array::from_fn(|k| p.m.r32(s + SLOT_DESC + k * 4));
        let new = p.helper(
            "texture slot registered by the call",
            REGISTER_SLOT,
            &[holder as u64, desc.as_ptr() as u64, (s + SLOT_BLOCK) as u64, 0, (p.base + COUNTUP) as u64, 0],
        )?;
        p.m.w::<i32>(s + SLOT_ID, new as i32);
    }
    Ok(())
}

/// dst = src as the game assigns texture states: head and block, the old slot reference released, the new one's
/// fields and block copied and counted up, the two tail words.
pub(super) unsafe fn assign(p: &mut Port, dst: usize, src: usize) -> R<()> {
    for o in (0..SLOT_ID).step_by(8) {
        let v = p.m.r64(src + o);
        p.m.w::<u64>(dst + o, v as u64);
    }
    release(p, dst);
    for o in [SLOT_ID, SLOT_DESC, SLOT_DESC + 4] {
        let v = p.m.r32(src + o);
        p.m.w::<u32>(dst + o, v);
    }
    for o in (0x68..TAIL).step_by(8) {
        let v = p.m.r64(src + o);
        p.m.w::<u64>(dst + o, v as u64);
    }
    acquire(p, dst)?;
    for o in [TAIL, TAIL + 8] {
        let v = p.m.r64(src + o);
        p.m.w::<u64>(dst + o, v as u64);
    }
    Ok(())
}

/// A default-constructed colour or depth target object (state at +8, slot id +0x60, setup word +0xc8, layer +0xcc,
/// level +0xce).
pub(super) unsafe fn new_target(p: &mut Port, obj: usize, vtable: usize) {
    p.m.w::<u64>(obj, (p.m.r64(p.base + vtable) + 0x10) as u64);
    p.m.w::<i32>(obj + 0x60, -1);
    p.m.w::<u8>(obj + 0xa9, 1);
    p.m.w::<u64>(obj + 0xb0, (p.base + TEXTURE_DATA_NAME) as u64);
    p.m.w::<u64>(obj + 0xc8, 0xff);
    p.m.w::<u64>(obj + 0xd0, 0x800_0000_0000);
}

/// A default-constructed render buffer: size (1, 1), region (0, 0)-(1, 1), no targets.
pub(super) unsafe fn new_buffer(p: &mut Port, rb: usize) {
    p.m.w::<u64>(rb, (p.m.r64(p.base + BUFFER_VTABLE) + 0x10) as u64);
    for (o, v) in [(8usize, 1.0f32), (0xc, 1.0), (0x10, 0.0), (0x14, 0.0), (0x18, 1.0), (0x1c, 1.0)] {
        p.m.w::<f32>(rb + o, v);
    }
}

/// The buffer's size and region for a target of `width` x `height`.
pub(super) unsafe fn set_extent(p: &mut Port, rb: usize, width: u16, height: u16) -> (f32, f32) {
    let (fw, fh) = (width as f32, height as f32);
    let empty = height == 0;
    p.m.w::<f32>(rb + 8, fw);
    p.m.w::<f32>(rb + 0xc, fh);
    p.m.w::<f32>(rb + 0x10, 0.0);
    p.m.w::<f32>(rb + 0x14, if empty { fh } else { 0.0 });
    p.m.w::<f32>(rb + 0x18, fw);
    p.m.w::<f32>(rb + 0x1c, if empty { 0.0 } else { fh });
    (fw, fh)
}

/// Target object `obj` given state `src`: the copy, the setup flag 4 (layer / level flags when they changed), then its
/// virtual +0x10.
pub(super) unsafe fn copy_target(p: &mut Port, obj: usize, src: usize) -> R<()> {
    let layer = p.m.r16(obj + 0xcc);
    let level = p.m.r8(obj + 0xce);
    assign(p, obj + 8, src)?;
    p.m.fetch_or32(obj + 0xc8, 4);
    if p.m.r16(obj + 0xcc) != layer {
        p.m.fetch_or32(obj + 0xc8, 2);
        p.m.w::<u16>(obj + 0xcc, layer);
    }
    if p.m.r8(obj + 0xce) != level {
        p.m.fetch_or32(obj + 0xc8, 1);
        p.m.w::<u8>(obj + 0xce, level);
    }
    let f = p.vfn(obj, 0x10);
    if f != SETUP_COLOR && f != SETUP_DEPTH {
        return Err("target object setup");
    }
    p.query(p.base + f, &[obj as u64]);
    Ok(())
}

/// The object's size at its level (+0xce): width (at least 1) and height (2D class 3: the height, else 1, and at least
/// the level's); `write` stores them back.
pub(super) unsafe fn level_size(p: &mut Port, obj: usize, write: bool) -> (u16, u16) {
    let level = (p.m.r8(obj + 0xce) & 0x1f) as u32;
    let w = ((p.m.r16(obj + 0x38) as u32 >> level) as u16).max(1);
    let h0 = p.m.r16(obj + 0x3a);
    let hs = (h0 as u32 >> level) as u16;
    let h = if p.m.r16(obj + 0x42) != 3 { 1 } else { h0 }.max(hs);
    if write {
        p.m.w::<u16>(obj + 0x38, w);
        p.m.w::<u16>(obj + 0x3a, h);
    }
    (w, h)
}

/// The barrier after rendering into a texture with a slot (its id at `id`).
pub(super) unsafe fn barrier_if_slot(p: &mut Port, id: usize, w: usize) {
    if p.m.i32(id) != -1 {
        let bits = super::targets::barrier_bits(p);
        let cmd = p.m.r64(w + 0xb8);
        p.barrier(cmd, bits);
    }
}

/// A temporary target from the dynamic allocator: main+0x7f0ad4(pool, w, &name, format, width, height, 1, 0, 0, 1, 0,
/// 0), its texture state. Side effects (the pool's state): only past the port's commit point.
pub(super) unsafe fn alloc_target(p: &mut Port, w: usize, name: usize, format: u32, width: u32, height: u32) -> R<usize> {
    let pool = p.m.r64(p.m.r64(p.base + TARGET_POOL));
    let name: [u64; 2] = [(p.base + name) as u64, 0];
    let args = [pool as u64, w as u64, name.as_ptr() as u64, format as u64, width as u64, height as u64, 1, 0, 0, 1, 0, 0];
    Ok(p.call_stack("temporary target allocation", ALLOC_TARGET, &args)? as usize)
}
