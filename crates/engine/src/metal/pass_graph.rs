//! --log-pass-graph=F: the GPU work of presented frames F and F+1 in issue order, for the frames' data flow: render
//! passes with their attachments (load / store actions), the textures each render or compute pass binds, compute
//! dispatches, blits and the presented texture. Textures are named by their root texture (views resolved to it plus
//! their level / slice offset) and described the first time they appear, and each render pass names the engine code
//! that ended it. Lines start with "[pg]"; with --no-issuer --encode-worker=0 they come out in GPU order (the end lines
//! are printed while encoding, the rest while issuing).

use objc2::rc::Retained;
use objc2::Message;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLLoadAction, MTLRenderPassDescriptor, MTLStoreAction, MTLTexture};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static ON: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Log {
    described: rustc_hash::FxHashSet<usize>,
    /// Textures bound in the current pass (root, level, slice, stage), each listed once per pass.
    bound: rustc_hash::FxHashSet<(usize, usize, usize, u8)>,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

pub(super) fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

fn with<R>(f: impl FnOnce(&mut Log) -> R) -> R {
    let mut l = LOG.lock().unwrap_or_else(|e| e.into_inner());
    f(l.get_or_insert_with(Log::default))
}

/// Root texture, level and slice offset of a texture or view; describes the root the first time.
fn name(log: &mut Log, t: &ProtocolObject<dyn MTLTexture>) -> String {
    let (mut level, mut slice) = (0, 0);
    let mut root: Retained<ProtocolObject<dyn MTLTexture>> = t.retain();
    while let Some(p) = root.parentTexture() {
        level += root.parentRelativeLevel();
        slice += root.parentRelativeSlice();
        root = p;
    }
    let id = &*root as *const ProtocolObject<dyn MTLTexture> as *const u8 as usize;
    if log.described.insert(id) {
        println!(
            "[pg] tex T{:x} {}x{}x{} fmt {} levels {} slices {} type {} usage {:#x}{}",
            id,
            root.width(),
            root.height(),
            root.depth(),
            root.pixelFormat().0,
            root.mipmapLevelCount(),
            root.arrayLength(),
            root.textureType().0,
            root.usage().0,
            if root.buffer().is_some() { " linear" } else { "" }
        );
    }
    if level == 0 && slice == 0 {
        format!("T{:x}", id)
    } else {
        format!("T{:x}@{}/{}", id, level, slice)
    }
}

fn load(a: MTLLoadAction) -> &'static str {
    match a {
        MTLLoadAction::Clear => "clear",
        MTLLoadAction::Load => "load",
        _ => "dontcare",
    }
}

fn store(a: MTLStoreAction) -> &'static str {
    match a {
        MTLStoreAction::Store => "store",
        MTLStoreAction::DontCare => "dontcare",
        _ => "other",
    }
}

/// At each present, after the issuer drained (pn: the frame just presented): turns the log on for frames F and F+1.
pub(super) fn at_present(pn: u64, presented: &ProtocolObject<dyn MTLTexture>) {
    let Some(f) = crate::settings::get().log.log_pass_graph else { return };
    if on() {
        with(|l| println!("[pg] present {}", name(l, presented)));
    }
    if pn + 1 == f || pn == f {
        ON.store(true, Ordering::Relaxed);
        println!("[pg] frame {}", pn + 1);
    } else if pn == f + 1 {
        ON.store(false, Ordering::Relaxed);
        println!("[pg] end");
    }
}

pub(super) unsafe fn render_pass(desc: &MTLRenderPassDescriptor, label: &str) {
    with(|l| {
        l.bound.clear();
        let mut s = format!("[pg] pass {}", label);
        for i in 0..8 {
            let a = desc.colorAttachments().objectAtIndexedSubscript(i);
            if let Some(t) = a.texture() {
                s += &format!(" c{}={}:{}/{} {} {}", i, name(l, &t), a.level(), a.slice().max(a.depthPlane()), load(a.loadAction()), store(a.storeAction()));
            }
        }
        let d = desc.depthAttachment();
        if let Some(t) = d.texture() {
            s += &format!(" d={}:{}/{} {} {}", name(l, &t), d.level(), d.slice(), load(d.loadAction()), store(d.storeAction()));
        }
        let st = desc.stencilAttachment();
        if let Some(t) = st.texture() {
            s += &format!(" s={}:{}/{} {} {}", name(l, &t), st.level(), st.slice(), load(st.loadAction()), store(st.storeAction()));
        }
        if desc.colorAttachments().objectAtIndexedSubscript(0).texture().is_none() && d.texture().is_none() {
            s += &format!(" (no attachments {}x{})", desc.renderTargetWidth(), desc.renderTargetHeight());
        }
        println!("{}", s);
    })
}

/// A texture bound to the current render (stage b'v' / b'f') or compute (b'c') pass.
pub(super) fn bind(t: &ProtocolObject<dyn MTLTexture>, stage: u8, index: usize) {
    with(|l| {
        let n = name(l, t);
        let key = (&*t as *const ProtocolObject<dyn MTLTexture> as *const u8 as usize, index, 0, stage);
        if l.bound.insert(key) {
            println!("[pg]   {} {} {}", stage as char, index, n);
        }
    })
}

pub(super) fn compute_pass(label: &str) {
    with(|l| l.bound.clear());
    println!("[pg] compute {}", label);
}

pub(super) fn dispatch(g: (usize, usize, usize), t: (usize, usize, usize)) {
    println!("[pg]   dispatch {}x{}x{} of {}x{}x{}", g.0, g.1, g.2, t.0, t.1, t.2);
}

/// A blit: what it copies (source -> destination; None: a buffer).
pub(super) fn blit(what: &str, src: Option<&ProtocolObject<dyn MTLTexture>>, dst: Option<&ProtocolObject<dyn MTLTexture>>) {
    with(|l| {
        let s = src.map_or("buffer".to_string(), |t| name(l, t));
        let d = dst.map_or("buffer".to_string(), |t| name(l, t));
        println!("[pg] blit {} {} -> {}", what, s, d);
    })
}
