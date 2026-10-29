//! Bindings of unit draws (the index ranges of a Cmd::ModelDraw), recorded the first time encode_draw resolves them and
//! replayed while the draw's inputs are unchanged. A unit's vertex buffers, textures, samplers and uniform blocks are
//! the same draw after draw; resolving them again is most of the per-draw encoding work.
//!
//! The key covers every input of encode_draw's vertex-buffer and per-stage sections (shaders, buffer and texture
//! bindings, uniform and storage descriptors, the draw parameters the driver block holds, the viewport the support
//! buffer holds) plus the pool and sampler generations their resolution depends on. A recording is kept only when every
//! buffer it binds outlives the submit: memory pool wrappers, constant banks and the zero buffer as they are; the
//! driver block, support buffer, sampler table and default attribute, whose contents follow from the key, as copies
//! the entry owns. Anything else (uniform copies, padded or repacked vertex streams) leaves the draw unrecorded.
//! The early-fence guard notes made while recording are replayed with the bindings.

use super::translated::{DrawKind, DrawPacket, TinyRec, UboPre};
use super::GuestWrite;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLBuffer, MTLDevice, MTLResourceOptions};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

type Buf = Retained<ProtocolObject<dyn MTLBuffer>>;
pub(super) type RepackKey = (u64, usize, u64, u64);

#[derive(Clone, Copy)]
pub(super) enum Note {
    Read(usize, usize),
    GpuWrite(GuestWrite, u64, usize),
}

pub(super) struct Entry {
    pub(super) rec: TinyRec,
    notes: Vec<Note>,
    /// Repacked vertex streams: (repack cache key, buffer, vertices needed). The entry applies while the repack cache
    /// still holds that buffer for the key and its check of the source passes.
    checks: Vec<(RepackKey, usize, usize)>,
}

#[derive(Default)]
struct Recording {
    notes: Vec<Note>,
    /// Arena allocations whose contents follow from the key: (buffer, offset, length).
    owned: Vec<(usize, usize, usize)>,
    /// Repacked vertex streams: (buffer, repack cache key, vertices needed).
    repacked: Vec<(usize, RepackKey, usize)>,
}

thread_local! {
    static REC: RefCell<Option<Recording>> = const { RefCell::new(None) };
}

pub(super) static HITS: AtomicU64 = AtomicU64::new(0);
pub(super) static RECORDED: AtomicU64 = AtomicU64::new(0);
pub(super) static UNRECORDABLE: AtomicU64 = AtomicU64::new(0);
pub(super) static MISMATCHES: AtomicU64 = AtomicU64::new(0);
/// Recorded draws whose key was already present (verify mode records every draw).
pub(super) static KEY_HITS: AtomicU64 = AtomicU64::new(0);
/// Why draws were not recordable: (stage buffer kind, index) -> count.
static WHY: std::sync::Mutex<Option<rustc_hash::FxHashMap<(&'static str, usize), u64>>> = std::sync::Mutex::new(None);

fn why(kind: &'static str, i: usize) {
    *WHY.lock().unwrap().get_or_insert_with(Default::default).entry((kind, i)).or_default() += 1;
}

pub(super) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().graphics.unit_memo)
        && !(super::replay_b() && crate::settings::get().replay.replay_ab_unit_memo)
}

pub(super) fn verify() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().graphics.verify_unit_memo)
}

/// The draw's key, or None when it cannot be recorded (no unit, or a uniform copied in command order).
pub(super) fn key(p: &DrawPacket) -> Option<u64> {
    if p.unit == 0 || p.tiny.is_some() {
        return None;
    }
    let mut h = rustc_hash::FxHasher::default();
    p.unit.hash(&mut h);
    (p.vs? as *const _ as usize).hash(&mut h);
    p.fs.map_or(0, |f| f as *const _ as usize).hash(&mut h);
    (p.vs_code.data, p.vs_code.control).hash(&mut h);
    p.fs_code.as_ref().map(|c| (c.data, c.control)).hash(&mut h);
    p.attr_bits.hash(&mut h);
    p.vertex_streams.hash(&mut h);
    p.vertex_buffers.hash(&mut h);
    p.sbufs.hash(&mut h);
    p.ubos.hash(&mut h);
    p.handles.hash(&mut h);
    for (s, b, pre) in &p.ubo_pre {
        (s, b).hash(&mut h);
        match pre {
            UboPre::ZeroCopy => 0u8.hash(&mut h),
            UboPre::Copy => 1u8.hash(&mut h),
            UboPre::Snapshot(..) => return None,
        }
    }
    p.sb_desc.hash(&mut h);
    for (s, b, t, sampler, _) in p.resolved_tex.as_ref()?.iter() {
        (s, b, t.as_ref().map_or(0, |t| Retained::as_ptr(t) as *const () as usize), sampler).hash(&mut h);
    }
    p.viewport.map(|(_, _, w, hgt)| (w, hgt)).hash(&mut h);
    match p.call {
        DrawKind::Arrays { first, count, base_instance, instances } => (0u8, first, count, base_instance, instances).hash(&mut h),
        DrawKind::Elements { index_type, count, index_addr, base_vertex, base_instance, instances } => {
            (1u8, index_type, count, index_addr, base_vertex, base_instance, instances).hash(&mut h)
        }
    }
    (p.mode, p.prim_restart).hash(&mut h);
    crate::nvn::api::pools_gen().hash(&mut h);
    crate::nvn::api::samplers_gen().hash(&mut h);
    Some(h.finish())
}

pub(super) fn begin() {
    REC.with(|r| *r.borrow_mut() = Some(Recording::default()));
}

pub(super) fn recording() -> bool {
    REC.with(|r| r.borrow().is_some())
}

/// A guard note made while recording (replayed with the bindings).
pub(super) fn note(n: Note) {
    REC.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.notes.push(n);
        }
    });
}

/// An arena allocation bound by the draw whose contents follow from the key.
pub(super) fn owned(buf: &ProtocolObject<dyn MTLBuffer>, off: usize, len: usize) {
    REC.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.owned.push((buf as *const _ as *const () as usize, off, len));
        }
    });
}

/// A repacked vertex stream bound by the draw (encode_draw checked it against its source just now).
pub(super) fn repacked(buf: &ProtocolObject<dyn MTLBuffer>, key: RepackKey, n: usize) {
    REC.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.repacked.push((buf as *const _ as *const () as usize, key, n));
        }
    });
}

impl Entry {
    pub(super) fn checks(&self) -> &[(RepackKey, usize, usize)] {
        &self.checks
    }
}

/// Ends the recording: the entry to keep, if every bound buffer outlives the submit (`arena`: is this buffer one of
/// the submit's arena chunks).
pub(super) fn finish(
    device: &ProtocolObject<dyn objc2_metal::MTLDevice>,
    rec: &TinyRec,
    arena: impl Fn(usize) -> bool,
) -> Option<Entry> {
    let r = REC.with(|r| r.borrow_mut().take())?;
    let mut copies: Vec<((usize, usize), Buf)> = Vec::new();
    let mut checks: Vec<(RepackKey, usize, usize)> = Vec::new();
    let mut map = |b: &Buf, off: usize, kind: &'static str, i: usize| -> Option<(Buf, usize)> {
        let p = Retained::as_ptr(b) as *const () as usize;
        if let Some(&(_, key, n)) = r.repacked.iter().find(|x| x.0 == p) {
            checks.push((key, p, n));
            return Some((b.clone(), off));
        }
        if !arena(p) {
            return Some((b.clone(), off));
        }
        let Some(&(_, _, len)) = r.owned.iter().find(|&&(q, o, _)| q == p && o == off) else {
            why(if kind == "v" { "vertex-stage arena copy" } else { "fragment-stage arena copy" }, i);
            return None;
        };
        if let Some((_, c)) = copies.iter().find(|(k, _)| *k == (p, off)) {
            return Some((c.clone(), 0));
        }
        let src = unsafe { std::slice::from_raw_parts((b.contents().as_ptr() as *const u8).add(off), len) };
        let c = unsafe {
            device.newBufferWithBytes_length_options(
                std::ptr::NonNull::new(src.as_ptr() as *mut libc::c_void)?,
                len.max(16),
                MTLResourceOptions::empty(),
            )
        }?;
        copies.push(((p, off), c.clone()));
        Some((c, 0))
    };
    let mut out = TinyRec::default();
    for (b, off, i) in &rec.vbufs {
        let (b, off) = map(b, *off, "v", *i)?;
        out.vbufs.push((b, off, *i));
    }
    for (b, off, i) in &rec.fbufs {
        let (b, off) = map(b, *off, "f", *i)?;
        out.fbufs.push((b, off, *i));
    }
    out.vtex = rec.vtex.clone();
    out.ftex = rec.ftex.clone();
    out.vsamp = rec.vsamp.clone();
    out.fsamp = rec.fsamp.clone();
    Some(Entry { rec: out, notes: r.notes, checks })
}

/// The guard notes of a replayed entry.
pub(super) fn replay_notes(e: &Entry) {
    for n in &e.notes {
        match *n {
            Note::Read(a, l) => super::fence_guard::note_read(a, l),
            Note::GpuWrite(k, a, l) => super::note_gpu_guest_write(k, a, l),
        }
    }
}

/// Whether two recordings bind the same resources (buffers the entry copied: by contents).
pub(super) fn same(a: &TinyRec, b: &TinyRec) -> Result<(), String> {
    fn bufs(x: &[(Buf, usize, usize)], y: &[(Buf, usize, usize)], what: &str) -> Result<(), String> {
        if x.len() != y.len() {
            return Err(format!("{what}: {} vs {} bindings", x.len(), y.len()));
        }
        for ((b0, o0, i0), (b1, o1, i1)) in x.iter().zip(y) {
            if i0 != i1 {
                return Err(format!("{what}: index {i0} vs {i1}"));
            }
            if Retained::as_ptr(b0) == Retained::as_ptr(b1) && o0 == o1 {
                continue;
            }
            // A copy the entry owns: compare the bytes it holds with the fresh binding's.
            let n = b0.length().saturating_sub(*o0).min(b1.length().saturating_sub(*o1)).min(0x1000);
            let s0 = unsafe { std::slice::from_raw_parts((b0.contents().as_ptr() as *const u8).add(*o0), n) };
            let s1 = unsafe { std::slice::from_raw_parts((b1.contents().as_ptr() as *const u8).add(*o1), n) };
            if s0 != s1 {
                return Err(format!("{what} {i0}: contents differ"));
            }
        }
        Ok(())
    }
    fn objs<T: ?Sized + objc2::Message>(x: &[(Retained<T>, usize)], y: &[(Retained<T>, usize)], what: &str) -> Result<(), String> {
        if x.len() != y.len() || x.iter().zip(y).any(|(p, q)| p.1 != q.1 || Retained::as_ptr(&p.0) != Retained::as_ptr(&q.0)) {
            return Err(format!("{what} differ"));
        }
        Ok(())
    }
    fn samps(
        x: &[(Option<Retained<ProtocolObject<dyn objc2_metal::MTLSamplerState>>>, usize)],
        y: &[(Option<Retained<ProtocolObject<dyn objc2_metal::MTLSamplerState>>>, usize)],
        what: &str,
    ) -> Result<(), String> {
        let p = |s: &Option<Retained<ProtocolObject<dyn objc2_metal::MTLSamplerState>>>| s.as_ref().map(|s| Retained::as_ptr(s) as *const () as usize);
        if x.len() != y.len() || x.iter().zip(y).any(|(a, b)| a.1 != b.1 || p(&a.0) != p(&b.0)) {
            return Err(format!("{what} differ"));
        }
        Ok(())
    }
    bufs(&a.vbufs, &b.vbufs, "vertex buffers")?;
    bufs(&a.fbufs, &b.fbufs, "fragment buffers")?;
    objs(&a.vtex, &b.vtex, "vertex textures")?;
    objs(&a.ftex, &b.ftex, "fragment textures")?;
    samps(&a.vsamp, &b.vsamp, "vertex samplers")?;
    samps(&a.fsamp, &b.fsamp, "fragment samplers")
}

/// Counters every 600 presented frames.
pub(super) fn report(frame: u64) {
    if frame % 600 != 0 || !enabled() {
        return;
    }
    let (h, r, u, m) = (
        HITS.swap(0, Ordering::Relaxed),
        RECORDED.swap(0, Ordering::Relaxed),
        UNRECORDABLE.swap(0, Ordering::Relaxed),
        MISMATCHES.swap(0, Ordering::Relaxed),
    );
    let kh = KEY_HITS.swap(0, Ordering::Relaxed);
    if h + r + u > 0 {
        println!(
            "[unit-memo] per frame: {:.0} replayed, {:.0} recorded ({:.0} with a known key), {:.0} not recordable{}",
            h as f64 / 600.0,
            r as f64 / 600.0,
            kh as f64 / 600.0,
            u as f64 / 600.0,
            if verify() { format!(", {m} verify mismatches") } else { String::new() }
        );
        let mut w: Vec<_> = WHY.lock().unwrap().take().unwrap_or_default().into_iter().collect();
        w.sort_by(|a, b| b.1.cmp(&a.1));
        let s: Vec<String> = w.iter().take(8).map(|((k, i), n)| format!("{k} buffer {i}: {:.0}", *n as f64 / 600.0)).collect();
        println!("[unit-memo]   not recordable by binding: {}", s.join(", "));
    }
}
