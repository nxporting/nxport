// TotK's model renderer (main+0x688c68), ported (draw.rs). --verify-model-draws runs the port in predict mode before
// each call of the game's function and compares the NVN commands the game then records with the prediction; calls
// that reach a helper the port does not implement yet are counted by that helper.

mod binds;
mod cave;
mod cells;
mod dispatch;
mod draw;
mod grid;
mod items;
mod lights;
mod gridtile;
mod material;
mod occlusion;
mod mem;
mod perview;
mod render_list;
mod ring;
mod targets;
mod terrain;
mod vfx;
mod viewport;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const SITE: usize = 0x688c68;
const SITE_INSTRUCTION: u32 = 0xd10383ff; // sub sp, sp, #0xe0
/// The render-list loop (render_list.rs).
const LIST_SITE: usize = 0x12de2a0;
const LIST_SITE_INSTRUCTION: u32 = 0xd10183ff; // sub sp, sp, #0x60
/// The dispatch drawable's draw (dispatch.rs), checked by itself when no enclosing render list is.
/// agl / sead primitives the pass functions are built from, drawn natively in apply mode: (site, entry instruction).
const GRAPHICS_CONTEXT_APPLY: (usize, u32) = (0x74d980, 0xd10243ff); // sead::GraphicsContext::apply
const PROGRAM_APPLY: (usize, u32) = (0x7ef4b8, 0xa9bd7bfd);
const PROGRAM_BLOCK: (usize, u32) = (0x96306c, 0xa9ba7bfd);
const PROGRAM_BLOCK_AT: (usize, u32) = (0x9771a8, 0xa9ba7bfd);
const PROGRAM_TEXTURE: (usize, u32) = (0x96ac04, 0xd10143ff);
const STREAM_DRAW: (usize, u32) = (0x966e70, 0xa9bc7bfd);
const SHAPE_BUFFERS: (usize, u32) = (0xf216b4, 0xa9ba7bfd);
const SHAPE_STATES: (usize, u32) = (0x978ba4, 0xa9be7bfd);
const VIEWPORT_APPLY: (usize, u32) = (0x74474c, 0xa9bc7bfd); // sead::Viewport::apply
const RENDER_BUFFER_BIND: (usize, u32) = (0x7ab9a4, 0xd103c3ff); // agl::RenderBuffer::bind
const OCCLUSION_SITE: usize = 0xa6c3a4;
const OCCLUSION_SITE_INSTRUCTION: u32 = 0xd10203ff; // sub sp, sp, #0x80
/// An item's occlusion query (items.rs).
const QUERY_ITEM_SITE: usize = 0x895ce8;
const QUERY_ITEM_SITE_INSTRUCTION: u32 = 0xa9bb7bfd; // stp x29, x30, [sp, #-0x50]!
/// An item drawn under its query (items.rs).
const OBJECT_ITEM_SITE: usize = 0x7fddf0;
const OBJECT_ITEM_SITE_INSTRUCTION: u32 = 0xd10143ff; // sub sp, sp, #0x50
const LIGHTS_SITE: usize = 0x9777c4;
const LIGHTS_SITE_INSTRUCTION: u32 = 0xd10743ff; // sub sp, sp, #0x1d0
const DISPATCH_SITE: usize = 0x763718;
const MATERIAL_SITE: usize = 0x7b42f0;
const MATERIAL_SITE_INSTRUCTION: u32 = 0xd10243ff; // sub sp, sp, #0x90
const VFX_PASS_SITE: usize = 0xa414ac;
const VFX_PASS_SITE_INSTRUCTION: u32 = 0xd10603ff; // sub sp, sp, #0x180
const VFX_VIEW_SITE: usize = 0xa41a38;
const VFX_VIEW_SITE_INSTRUCTION: u32 = 0xa9ba7bfd; // stp x29, x30, [sp, #-0x60]!
const VFX_SITE: usize = 0x843c;
const VFX_SITE_INSTRUCTION: u32 = 0xd10243ff; // sub sp, sp, #0x90
const DISPATCH_SITE_INSTRUCTION: u32 = 0xd10143ff; // sub sp, sp, #0x50

static CALLS: AtomicU64 = AtomicU64::new(0);
static SAME: AtomicU64 = AtomicU64::new(0);
static DIFFERENT: AtomicU64 = AtomicU64::new(0);
static STOPS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

thread_local! {
    static EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

fn note_stop(what: &'static str) {
    let mut s = STOPS.lock().unwrap();
    match s.iter_mut().find(|e| e.0 == what) {
        Some(e) => e.1 += 1,
        None => s.push((what, 1)),
    }
}

#[no_mangle]
pub unsafe extern "C" fn on_model_draw(d: usize, c: usize, shape: u32, flags: u32) -> u32 {
    let n = CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    if n % 200_000 == 0 {
        report();
    }
    let base = crate::guest::loader::main_base();
    if crate::settings::get().totk.diagnostics.native_model_draws {
        static METAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let metal = *METAL.get_or_init(|| crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Metal);
        let mut port = draw::Port::new(mem::Mem::applying(), base, metal);
        let r = draw::draw(&mut port, d, c, shape, flags);
        port.flush();
        if let Err(what) = r {
            // Applying never stops (helpers it does not implement call the game's); a stop here is a port bug.
            note_stop(what);
        }
        return 1;
    }
    let mut port = draw::Port::new(mem::Mem::predicting(), base, false);
    let mut result = Ok(());
    let expected = crate::nvn::api::capture_dry(|| result = draw::draw(&mut port, d, c, shape, flags));
    match result {
        Ok(()) => {
            EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            ring::watch();
            crate::nvn::api::capture_collect();
        }
        Err(what) => {
            ring::discard();
            note_stop(what)
        }
    }
    0
}

thread_local! {
    /// The view (+0x30) and pass kind (+0xb0) of the render list being checked.
    static LIST_KEY: std::cell::Cell<Option<(u8, u32)>> = const { std::cell::Cell::new(None) };
    /// The render list being checked (list, count, ctx, mask, flags), for the second prediction after the game's call.
    static LIST_ARGS: std::cell::Cell<Option<(usize, u32, usize, u32, u32)>> = const { std::cell::Cell::new(None) };
}

/// Render-list differences the second prediction reproduced (the port read state that changed during the game's call).
static MOVED: AtomicU64 = AtomicU64::new(0);

/// A render list that differed predicted again after the game's call: whether the game's command at the first
/// difference `i` is now predicted.
unsafe fn moved_during_call(got: &[String], i: usize) -> bool {
    let Some((list, count, c, mask, flags)) = LIST_ARGS.with(|a| a.take()) else { return false };
    let mut port = draw::Port::new(mem::Mem::predicting(), crate::guest::loader::main_base(), false);
    let mut result = Ok(());
    let again = crate::nvn::api::capture_dry(|| result = render_list::draw_list(&mut port, list, count, c, mask, flags));
    ring::discard();
    vfx::discard_blocks();
    result.is_ok() && again.get(i).is_some() && again.get(i) == got.get(i)
}

/// Render-list checks by (view, pass kind): (same, different).
static BY_VIEW: Mutex<Vec<((u8, u32), (u64, u64))>> = Mutex::new(Vec::new());

fn tally_view(same: bool) {
    let Some(key) = LIST_KEY.with(|k| k.take()) else { return };
    let mut v = BY_VIEW.lock().unwrap();
    let e = match v.iter_mut().find(|e| e.0 == key) {
        Some(e) => e,
        None => {
            v.push((key, (0, 0)));
            v.last_mut().unwrap()
        }
    };
    if same { e.1 .0 += 1 } else { e.1 .1 += 1 }
}

#[no_mangle]
pub unsafe extern "C" fn on_model_draw_end() -> u32 {
    let Some(expected) = EXPECTED.with(|e| e.borrow_mut().take()) else { return 0 };
    let got = crate::nvn::api::capture_collected();
    let expected = match ring::resolve(expected) {
        Ok(e) => e,
        Err(why) => {
            let k = DIFFERENT.fetch_add(1, Ordering::Relaxed);
            if k < 6 {
                println!("[model-draw] mismatch #{k}: {why}");
            }
            return 0;
        }
    };
    tally_view(expected == got);
    if expected == got {
        LIST_ARGS.with(|a| a.set(None));
        SAME.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let first = (0..expected.len().max(got.len())).find(|&i| expected.get(i) != got.get(i)).unwrap_or(0);
    if moved_during_call(&got, first) {
        MOVED.fetch_add(1, Ordering::Relaxed);
    }
    let k = DIFFERENT.fetch_add(1, Ordering::Relaxed);
    if k < 6 {
        println!("[model-draw] mismatch #{k}:");
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            println!("[model-draw]   {} port: {e}", if e == g { " " } else { "!" });
            if e != g {
                println!("[model-draw]     game: {g}");
            }
        }
    }
    0
}

static LIST_CALLS: AtomicU64 = AtomicU64::new(0);

#[no_mangle]
pub unsafe extern "C" fn on_render_list(list: usize, count: u32, c: usize, mask: u32, _x4: usize, flags: u32) -> u32 {
    let n = LIST_CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    CALLS.fetch_add(1, Ordering::Relaxed);
    if n % 20_000 == 0 {
        report();
    }
    let base = crate::guest::loader::main_base();
    let d = &crate::settings::get().totk.diagnostics;
    if d.native_render_lists {
        // Applied undoably, its commands kept: a list the port stops in is undone and left whole to the game (the
        // game's own call also does the lazy setup the port leaves to it).
        static METAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let metal = *METAL.get_or_init(|| crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Metal);
        let mut port = draw::Port::new(mem::Mem::applying_undoably(), base, metal && d.native_model_draws);
        let (r, commands) = crate::nvn::api::capture_buffered(|| {
            let r = render_list::draw_list(&mut port, list, count, c, mask, flags);
            if r.is_ok() {
                port.flush();
            }
            r
        });
        ring::discard();
        vfx::discard_blocks();
        if let Err(what) = r {
            port.discard();
            port.m.rollback();
            note_stop(what);
            return 0;
        }
        crate::nvn::api::commit_buffered(commands);
        return 1;
    }
    let mut port = draw::Port::new(mem::Mem::predicting(), base, false);
    let mut result = Ok(());
    let expected =
        crate::nvn::api::capture_dry(|| result = render_list::draw_list(&mut port, list, count, c, mask, flags));
    match result {
        Ok(()) => {
            LIST_KEY.with(|k| k.set(Some((*((c + 0x30) as *const u8), *((c + 0xb0) as *const u32)))));
            LIST_ARGS.with(|a| a.set(Some((list, count, c, mask, flags))));
            EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            ring::watch();
            crate::nvn::api::capture_collect();
        }
        Err(what) => {
            ring::discard();
            note_stop(what)
        }
    }
    0
}

static DISPATCH_CALLS: AtomicU64 = AtomicU64::new(0);
static DISPATCH_SAME: AtomicU64 = AtomicU64::new(0);
static DISPATCH_DIFFERENT: AtomicU64 = AtomicU64::new(0);
static DISPATCH_STOPS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

thread_local! {
    static DISPATCH_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// --verify-render-lists: a dispatch drawable drawn by the game (its render list was not predicted) is predicted by
/// itself and compared.
#[no_mangle]
pub unsafe extern "C" fn on_dispatch(d: usize, c: usize, lod: u32, flags: u32) -> u32 {
    if crate::nvn::api::capturing() {
        return 0;
    }
    DISPATCH_CALLS.fetch_add(1, Ordering::Relaxed);
    let mut port = draw::Port::new(mem::Mem::predicting(), crate::guest::loader::main_base(), false);
    let mut result = Ok(());
    let expected = crate::nvn::api::capture_dry(|| result = dispatch::draw(&mut port, d, c, lod, flags));
    match result {
        Ok(()) => {
            DISPATCH_EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            ring::watch();
            crate::nvn::api::capture_collect();
        }
        Err(what) => {
            ring::discard();
            let mut s = DISPATCH_STOPS.lock().unwrap();
            match s.iter_mut().find(|x| x.0 == what) {
                Some(x) => x.1 += 1,
                None => s.push((what, 1)),
            }
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_dispatch_end() -> u32 {
    let Some(expected) = DISPATCH_EXPECTED.with(|e| e.borrow_mut().take()) else { return 0 };
    let got = crate::nvn::api::capture_collected();
    let expected = match ring::resolve(expected) {
        Ok(e) => e,
        Err(why) => {
            DISPATCH_DIFFERENT.fetch_add(1, Ordering::Relaxed);
            println!("[model-draw] dispatch mismatch: {why}");
            return 0;
        }
    };
    if expected == got {
        DISPATCH_SAME.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let k = DISPATCH_DIFFERENT.fetch_add(1, Ordering::Relaxed);
    if k < 6 {
        println!("[model-draw] dispatch mismatch #{k}:");
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            println!("[model-draw]   {} port: {e}", if e == g { " " } else { "!" });
            if e != g {
                println!("[model-draw]     game: {g}");
            }
        }
    }
    0
}

static LIGHT_CALLS: AtomicU64 = AtomicU64::new(0);
static LIGHT_SAME: AtomicU64 = AtomicU64::new(0);
static LIGHT_DIFFERENT: AtomicU64 = AtomicU64::new(0);
static LIGHT_STOPS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

thread_local! {
    static LIGHT_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// The light volumes main+0x9777c4: --verify-render-lists predicts and compares them; --native-render-lists draws
/// them (undoably, left to the game where the port stops).
#[no_mangle]
pub unsafe extern "C" fn on_lights(obj: usize, w: usize, view: u32, arg: usize) -> u32 {
    if crate::nvn::api::capturing() {
        return 0;
    }
    LIGHT_CALLS.fetch_add(1, Ordering::Relaxed);
    let base = crate::guest::loader::main_base();
    let d = &crate::settings::get().totk.diagnostics;
    let stop = |what: &'static str| {
        let mut s = LIGHT_STOPS.lock().unwrap();
        match s.iter_mut().find(|x| x.0 == what) {
            Some(x) => x.1 += 1,
            None => s.push((what, 1)),
        }
    };
    if d.native_render_lists {
        static METAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let metal = *METAL.get_or_init(|| crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Metal);
        let mut port = draw::Port::new(mem::Mem::applying_undoably(), base, metal && d.native_model_draws);
        let (r, commands) = crate::nvn::api::capture_buffered(|| {
            let r = lights::draw(&mut port, obj, w, view, arg);
            if r.is_ok() {
                port.flush();
            }
            r
        });
        ring::discard();
        if let Err(what) = r {
            port.discard();
            port.m.rollback();
            stop(what);
            return 0;
        }
        crate::nvn::api::commit_buffered(commands);
        return 1;
    }
    let mut port = draw::Port::new(mem::Mem::predicting(), base, false);
    let mut result = Ok(());
    let expected = crate::nvn::api::capture_dry(|| result = lights::draw(&mut port, obj, w, view, arg));
    match result {
        Ok(()) => {
            LIGHT_EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            ring::watch();
            crate::nvn::api::capture_collect();
        }
        Err(what) => {
            ring::discard();
            stop(what);
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_lights_end() -> u32 {
    let Some(expected) = LIGHT_EXPECTED.with(|e| e.borrow_mut().take()) else { return 0 };
    let got = crate::nvn::api::capture_collected();
    let expected = match ring::resolve_with(expected, &got) {
        Ok(e) => e,
        Err(why) => {
            LIGHT_DIFFERENT.fetch_add(1, Ordering::Relaxed);
            println!("[model-draw] light volumes mismatch: {why}");
            return 0;
        }
    };
    if expected == got {
        LIGHT_SAME.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let k = LIGHT_DIFFERENT.fetch_add(1, Ordering::Relaxed);
    if k < 4 {
        println!("[model-draw] light volumes mismatch #{k}:");
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            println!("[model-draw]   {} port: {e}", if e == g { " " } else { "!" });
            if e != g {
                println!("[model-draw]     game: {g}");
            }
        }
    }
    0
}

/// Primitive calls drawn natively and left to the game.
static PRIMITIVES: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// A primitive of the game drawn natively (apply mode): undoably, its commands kept; nonzero from `f` = handled (the
/// game's function is skipped and returns it), else undone and left to the game.
unsafe fn primitive(f: impl FnOnce(&mut draw::Port) -> draw::R<u32>) -> u32 {
    if crate::nvn::api::capturing() {
        return 0;
    }
    let mut port = draw::Port::new(mem::Mem::applying_undoably(), crate::guest::loader::main_base(), true);
    let (r, commands) = crate::nvn::api::capture_buffered(|| {
        let r = f(&mut port);
        if matches!(r, Ok(v) if v != 0) {
            port.flush();
        }
        r
    });
    match r {
        Ok(v) if v != 0 => {
            crate::nvn::api::commit_buffered(commands);
            PRIMITIVES[0].fetch_add(1, Ordering::Relaxed);
            v
        }
        _ => {
            port.discard();
            port.m.rollback();
            PRIMITIVES[1].fetch_add(1, Ordering::Relaxed);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn on_graphics_context_apply(g: usize, w: usize) -> u32 {
    primitive(|p| {
        p.apply_graphics_context(g, w);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_program_apply(prog: usize, w: usize) -> u32 {
    primitive(|p| vfx::program_apply(p, prog, w).map(|_| 1))
}

#[no_mangle]
pub unsafe extern "C" fn on_program_block(prog: usize, w: usize, index: u32, block: usize, element: i32) -> u32 {
    primitive(|p| vfx::program_block(p, prog, w, index, block, element).map(|b| b as u32))
}

/// main+0x9771a8: main+0x96306c with the block through a pointer.
#[no_mangle]
pub unsafe extern "C" fn on_program_block_at(prog: usize, w: usize, index: u32, block: usize, element: i32) -> u32 {
    primitive(|p| vfx::program_block(p, prog, w, index, p.m.r64(block), element).map(|b| b as u32))
}

/// main+0x96ac04(prog, w, slot, texture): returns whether the texture was bound (0: left to the game, which binds
/// nothing either).
#[no_mangle]
pub unsafe extern "C" fn on_program_texture(prog: usize, w: usize, slot: u32, tex: usize) -> u32 {
    primitive(|p| vfx::program_texture(p, prog, w, slot, tex).map(|b| b as u32))
}

/// main+0x931960(renderer, w, args, view, range (start, count), flag, out, user): one model pass, as the frame's
/// command-recording workers (main+0x90ddb0) call it.
#[no_mangle]
pub unsafe extern "C" fn on_model_pass(r: usize, w: usize, a: usize, v: usize, range: usize, flag: u32, out: usize, user: u64) -> u32 {
    primitive(|p| {
        let (start, count) = (p.m.r32(range), p.m.i32(range + 4));
        render_list::model_pass(p, r, w, a, v, start, count, flag, out, user).map(|_| 1)
    })
}

/// main+0x74efd4(block, w, *slots, element): a uniform or storage block's element at the slots (bound: handled; no
/// slots: left to the game, which binds nothing either).
#[no_mangle]
pub unsafe extern "C" fn on_block_bind(block: usize, w: usize, slots: usize, element: i32) -> u32 {
    primitive(|p| {
        let s = p.m.r32(slots);
        p.bind_uniform_block(block, w, s, element).map(|b| b as u32)
    })
}

/// main+0x9c7e04 / main+0x9c7c80: the frame's default texture and uniform bindings (binds.rs).
#[no_mangle]
pub unsafe extern "C" fn on_default_textures(settings: usize, w: usize) -> u32 {
    primitive(|p| binds::default_textures(p, settings, w).map(|_| 1))
}

#[no_mangle]
pub unsafe extern "C" fn on_default_uniforms(settings: usize, w: usize) -> u32 {
    primitive(|p| binds::default_uniforms(p, settings, w).map(|_| 1))
}

#[no_mangle]
pub unsafe extern "C" fn on_stream_draw(stream: usize, w: usize, instances: u32) -> u32 {
    primitive(|p| {
        lights::stream_draw_n(p, stream, w, instances);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_shape_buffers(shape: usize, w: usize) -> u32 {
    primitive(|p| {
        lights::shape_buffers(p, shape, w);
        Ok(1)
    })
}

/// main+0x978ba4(shape, w): the shape's vertex attribute and stream states.
#[no_mangle]
pub unsafe extern "C" fn on_shape_states(shape: usize, w: usize) -> u32 {
    primitive(|p| {
        let cmd = p.m.r64(w + 0xb8);
        p.bind_vertex_attribs(cmd, 0x10, shape + 0x11c);
        p.bind_vertex_streams(cmd, 0x10, shape + 0x160);
        Ok(1)
    })
}

static VIEWPORTS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

fn viewport_compared(expected: &[String], got: &[String]) {
    let k = (expected == got) as usize;
    if VIEWPORTS[k].fetch_add(1, Ordering::Relaxed) < 4 && k == 0 {
        println!("[model-draw] viewport differs: port {expected:?} game {got:?}");
    }
}

/// sead::Viewport::apply: --verify-render-lists compares the port with the game's commands; apply mode draws it.
#[no_mangle]
pub unsafe extern "C" fn on_viewport_apply(vp: usize, w: usize, fb: usize) -> u32 {
    if crate::settings::get().totk.diagnostics.verify_render_lists {
        if crate::nvn::api::capturing() {
            return 0;
        }
        let mut port = draw::Port::new(mem::Mem::predicting(), crate::guest::loader::main_base(), false);
        let expected = crate::nvn::api::capture_dry(|| viewport::apply(&mut port, vp, w, fb));
        crate::nvn::api::capture_watch(expected, viewport_compared);
        return 0;
    }
    primitive(|p| {
        viewport::apply(p, vp, w, fb);
        Ok(1)
    })
}

static TARGETS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

fn targets_compared(expected: &[String], got: &[String]) {
    let k = (expected == got) as usize;
    if TARGETS[k].fetch_add(1, Ordering::Relaxed) < 4 && k == 0 {
        println!("[model-draw] render targets differ: port {expected:?} game {got:?}");
    }
}

/// agl::RenderBuffer::bind: --verify-render-lists compares the port with the game's commands; apply mode draws it.
#[no_mangle]
pub unsafe extern "C" fn on_render_buffer_bind(rb: usize, w: usize) -> u32 {
    if crate::settings::get().totk.diagnostics.verify_render_lists {
        if crate::nvn::api::capturing() {
            return 0;
        }
        let mut port = draw::Port::new(mem::Mem::predicting(), crate::guest::loader::main_base(), false);
        let mut r = Ok(());
        let expected = crate::nvn::api::capture_dry(|| r = targets::bind(&mut port, rb, w));
        if r.is_ok() {
            crate::nvn::api::capture_watch(expected, targets_compared);
        }
        return 0;
    }
    primitive(|p| targets::bind(p, rb, w).map(|_| 1))
}

// The nn::gfx command buffer (NVNcommandBuffer at +0xb0) the UI layouts, fonts and effects draw through: each method
// drawn natively in apply mode (vfx.rs has the ports).

#[no_mangle]
pub unsafe extern "C" fn on_gfx_constant_buffer(g: usize, slot: i32, stage: u32, addr: usize, size: u64) -> u32 {
    primitive(|p| {
        vfx::bind_uniform(p, g, slot, stage, p.m.r64(addr) as u64, size);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_storage_buffer(g: usize, slot: i32, stage: u32, addr: usize, size: u64) -> u32 {
    primitive(|p| {
        vfx::bind_storage(p, g, slot, stage, p.m.r64(addr) as u64, size);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_vertex_buffer(g: usize, index: i32, addr: usize, _stride: u64, size: u64) -> u32 {
    primitive(|p| {
        vfx::bind_vertex_buffer(p, g, index, p.m.r64(addr) as u64, size);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_draw_indexed(g: usize, topology: u32, format: u32, addr: usize, count: i32, base_vertex: i32) -> u32 {
    primitive(|p| {
        vfx::draw_indexed_base(p, g, topology, format, p.m.r64(addr) as u64, count, base_vertex);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_dispatch(g: usize, x: i32, y: i32, z: i32) -> u32 {
    primitive(|p| {
        vfx::dispatch(p, g, x, y, z);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_draw(g: usize, topology: u32, count: u32, first: u32) -> u32 {
    primitive(|p| {
        vfx::draw_plain(p, g, topology, count, first);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_draw_instanced(g: usize, topology: u32, count: u32, first: u32, instances: u32, base: u32) -> u32 {
    primitive(|p| {
        vfx::draw_arrays(p, g, topology, count, first, instances, base);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_texture(g: usize, slot: i32, stage: u32, tex: usize, smp: usize) -> u32 {
    primitive(|p| {
        vfx::bind_texture(p, g, slot, stage, tex, smp);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_shader(g: usize, shader: usize, stages: u32) -> u32 {
    primitive(|p| {
        p.set_shader(g, shader, stages);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_vertex_state(g: usize, s: usize) -> u32 {
    primitive(|p| {
        vfx::bind_vertex_state(p, g, s);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_blend_state(g: usize, s: usize) -> u32 {
    primitive(|p| {
        vfx::set_blend_state(p, g, s);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_depth_stencil_state(g: usize, s: usize) -> u32 {
    primitive(|p| {
        vfx::set_depth_stencil_state(p, g, s);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_rasterizer_state(g: usize, s: usize) -> u32 {
    primitive(|p| {
        vfx::set_rasterizer_state(p, g, s);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_primitive_end() -> u32 {
    0
}

static OCCL_CALLS: AtomicU64 = AtomicU64::new(0);
static OCCL_SAME: AtomicU64 = AtomicU64::new(0);
static OCCL_DIFFERENT: AtomicU64 = AtomicU64::new(0);
static OCCL_STOPS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

thread_local! {
    static OCCL_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// The occlusion query items main+0xa6c3a4: --verify-render-lists predicts and compares them; --native-render-lists draws
/// them (undoably, left to the game where the port stops).
#[no_mangle]
pub unsafe extern "C" fn on_occlusion(q: usize, w: usize, view: u32, flags: u64) -> u32 {
    if crate::nvn::api::capturing() {
        return 0;
    }
    OCCL_CALLS.fetch_add(1, Ordering::Relaxed);
    let base = crate::guest::loader::main_base();
    let d = &crate::settings::get().totk.diagnostics;
    let stop = |what: &'static str| {
        let mut s = OCCL_STOPS.lock().unwrap();
        match s.iter_mut().find(|x| x.0 == what) {
            Some(x) => x.1 += 1,
            None => s.push((what, 1)),
        }
    };
    if d.native_render_lists {
        static METAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let metal = *METAL.get_or_init(|| crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Metal);
        let mut port = draw::Port::new(mem::Mem::applying_undoably(), base, metal && d.native_model_draws);
        let (r, commands) = crate::nvn::api::capture_buffered(|| {
            let r = occlusion::draw(&mut port, q, w, view, flags);
            if r.is_ok() {
                port.flush();
            }
            r
        });
        ring::discard();
        if let Err(what) = r {
            port.discard();
            port.m.rollback();
            stop(what);
            return 0;
        }
        crate::nvn::api::commit_buffered(commands);
        return 1;
    }
    let mut port = draw::Port::new(mem::Mem::predicting(), base, false);
    let mut result = Ok(());
    let expected = crate::nvn::api::capture_dry(|| result = occlusion::draw(&mut port, q, w, view, flags));
    match result {
        Ok(()) => {
            OCCL_EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            ring::watch();
            crate::nvn::api::capture_collect();
        }
        Err(what) => {
            ring::discard();
            stop(what);
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_occlusion_end() -> u32 {
    let Some(expected) = OCCL_EXPECTED.with(|e| e.borrow_mut().take()) else { return 0 };
    let got = crate::nvn::api::capture_collected();
    let expected = match ring::resolve_with(expected, &got) {
        Ok(e) => e,
        Err(why) => {
            OCCL_DIFFERENT.fetch_add(1, Ordering::Relaxed);
            println!("[model-draw] occlusion items mismatch: {why}");
            return 0;
        }
    };
    if expected == got {
        OCCL_SAME.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let k = OCCL_DIFFERENT.fetch_add(1, Ordering::Relaxed);
    if k < 4 {
        println!("[model-draw] occlusion items mismatch #{k}:");
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            println!("[model-draw]   {} port: {e}", if e == g { " " } else { "!" });
            if e != g {
                println!("[model-draw]     game: {g}");
            }
        }
    }
    0
}

/// Calls, identical, different, stops of a port checked at its own entry hook (items).
struct Checked {
    calls: AtomicU64,
    same: AtomicU64,
    different: AtomicU64,
    stops: Mutex<Vec<(&'static str, u64)>>,
}

impl Checked {
    const fn new() -> Self {
        Checked { calls: AtomicU64::new(0), same: AtomicU64::new(0), different: AtomicU64::new(0), stops: Mutex::new(Vec::new()) }
    }

    fn stop(&self, what: &'static str) {
        let mut s = self.stops.lock().unwrap();
        match s.iter_mut().find(|x| x.0 == what) {
            Some(x) => x.1 += 1,
            None => s.push((what, 1)),
        }
    }

    fn summary(&self, name: &str) -> Option<String> {
        let n = self.calls.load(Ordering::Relaxed);
        if n == 0 {
            return None;
        }
        let stops: Vec<String> = self.stops.lock().unwrap().iter().map(|(w, k)| format!("{w} {k}")).collect();
        Some(format!(
            "[model-draw] {name}: {n} calls, {} identical, {} different; stops: {}",
            self.same.load(Ordering::Relaxed),
            self.different.load(Ordering::Relaxed),
            stops.join(", ")
        ))
    }

    /// Apply mode: drawn undoably (1: the game's function is skipped); verify mode: predicted, compared at `end`.
    unsafe fn run(&'static self, expected: &'static std::thread::LocalKey<std::cell::RefCell<Option<Vec<String>>>>, f: impl Fn(&mut draw::Port) -> draw::R<()>) -> u32 {
        if crate::nvn::api::capturing() {
            return 0;
        }
        self.calls.fetch_add(1, Ordering::Relaxed);
        let base = crate::guest::loader::main_base();
        let d = &crate::settings::get().totk.diagnostics;
        if d.native_render_lists {
            static METAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            let metal = *METAL.get_or_init(|| crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Metal);
            let mut port = draw::Port::new(mem::Mem::applying_undoably(), base, metal && d.native_model_draws);
            let (r, commands) = crate::nvn::api::capture_buffered(|| {
                let r = f(&mut port);
                if r.is_ok() {
                    port.flush();
                }
                r
            });
            ring::discard();
            if let Err(what) = r {
                port.discard();
                port.m.rollback();
                self.stop(what);
                return 0;
            }
            crate::nvn::api::commit_buffered(commands);
            return 1;
        }
        let mut port = draw::Port::new(mem::Mem::predicting(), base, false);
        let mut result = Ok(());
        let e = crate::nvn::api::capture_dry(|| result = f(&mut port));
        match result {
            Ok(()) => {
                expected.with(|x| *x.borrow_mut() = Some(e));
                ring::watch();
                crate::nvn::api::capture_collect();
            }
            Err(what) => {
                ring::discard();
                self.stop(what);
            }
        }
        0
    }

    unsafe fn end(&self, name: &str, expected: &'static std::thread::LocalKey<std::cell::RefCell<Option<Vec<String>>>>) -> u32 {
        let Some(e) = expected.with(|x| x.borrow_mut().take()) else { return 0 };
        let got = crate::nvn::api::capture_collected();
        let e = match ring::resolve_with(e, &got) {
            Ok(e) => e,
            Err(why) => {
                self.different.fetch_add(1, Ordering::Relaxed);
                println!("[model-draw] {name} mismatch: {why}");
                return 0;
            }
        };
        if e == got {
            self.same.fetch_add(1, Ordering::Relaxed);
            return 0;
        }
        let k = self.different.fetch_add(1, Ordering::Relaxed);
        if k < 4 {
            println!("[model-draw] {name} mismatch #{k}:");
            for i in 0..e.len().max(got.len()) {
                let (a, b) = (e.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
                println!("[model-draw]   {} port: {a}", if a == b { " " } else { "!" });
                if a != b {
                    println!("[model-draw]     game: {b}");
                }
            }
        }
        0
    }
}

static QUERY_ITEMS: Checked = Checked::new();

thread_local! {
    static QUERY_ITEM_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// An item's occlusion query, main+0x895ce8 (items.rs).
#[no_mangle]
pub unsafe extern "C" fn on_query_item(item: usize, ctx: usize) -> u32 {
    QUERY_ITEMS.run(&QUERY_ITEM_EXPECTED, |p| items::query_item(p, item, ctx))
}

#[no_mangle]
pub unsafe extern "C" fn on_query_item_end() -> u32 {
    QUERY_ITEMS.end("item queries", &QUERY_ITEM_EXPECTED)
}

static OBJECT_ITEMS: Checked = Checked::new();

thread_local! {
    static OBJECT_ITEM_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// An item drawn under its occlusion query, main+0x7fddf0 (items.rs).
#[no_mangle]
pub unsafe extern "C" fn on_object_item(item: usize, ctx: usize) -> u32 {
    OBJECT_ITEMS.run(&OBJECT_ITEM_EXPECTED, |p| items::object_item(p, item, ctx))
}

#[no_mangle]
pub unsafe extern "C" fn on_object_item_end() -> u32 {
    OBJECT_ITEMS.end("item objects", &OBJECT_ITEM_EXPECTED)
}

static VFX_CALLS: AtomicU64 = AtomicU64::new(0);
static VFX_SAME: AtomicU64 = AtomicU64::new(0);
static VFX_DIFFERENT: AtomicU64 = AtomicU64::new(0);
static VFX_STOPS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

thread_local! {
    static VFX_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// --verify-render-lists: an effect emitter drawn by the game outside a predicted list is predicted by itself.
#[no_mangle]
pub unsafe extern "C" fn on_vfx(sys: usize, g: usize, em: usize, a3: usize, dp: usize, cache: usize) -> u32 {
    if crate::nvn::api::capturing() {
        return 0;
    }
    VFX_CALLS.fetch_add(1, Ordering::Relaxed);
    if !crate::settings::get().totk.diagnostics.verify_render_lists {
        // Apply mode: an emitter the port draws returns true (main+0x843c's result); one it does not draw, or stops
        // at, is left to the game.
        let mut port = draw::Port::new(mem::Mem::applying_undoably(), crate::guest::loader::main_base(), true);
        let (r, commands) = crate::nvn::api::capture_buffered(|| {
            let r = vfx::draw_emitter(&mut port, sys, g, em, a3, dp, cache);
            if matches!(r, Ok(true)) {
                port.flush();
            }
            r
        });
        ring::discard();
        match r {
            Ok(true) => {
                crate::nvn::api::commit_buffered(commands);
                return 1;
            }
            r => {
                port.discard();
                port.m.rollback();
                vfx::discard_blocks();
                if let Err(what) = r {
                    let mut s = VFX_STOPS.lock().unwrap();
                    match s.iter_mut().find(|x| x.0 == what) {
                        Some(x) => x.1 += 1,
                        None => s.push((what, 1)),
                    }
                }
                return 0;
            }
        }
    }
    let mut port = draw::Port::new(mem::Mem::predicting(), crate::guest::loader::main_base(), false);
    let mut result = Ok(());
    let expected = crate::nvn::api::capture_dry(|| result = vfx::draw_emitter(&mut port, sys, g, em, a3, dp, cache).map(|_| ()));
    match result {
        Ok(()) => {
            VFX_EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            ring::watch();
            crate::nvn::api::capture_collect();
        }
        Err(what) => {
            ring::discard();
            vfx::discard_blocks();
            let mut s = VFX_STOPS.lock().unwrap();
            match s.iter_mut().find(|x| x.0 == what) {
                Some(x) => x.1 += 1,
                None => s.push((what, 1)),
            }
        }
    }
    0
}

static VFXV_CALLS: AtomicU64 = AtomicU64::new(0);
static VFXV_SAME: AtomicU64 = AtomicU64::new(0);
static VFXV_DIFFERENT: AtomicU64 = AtomicU64::new(0);
static VFXV_STOPS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

thread_local! {
    static VFXV_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

static VFXP_CALLS: AtomicU64 = AtomicU64::new(0);
static VFXP_SAME: AtomicU64 = AtomicU64::new(0);
static VFXP_DIFFERENT: AtomicU64 = AtomicU64::new(0);
static VFXP_STOPS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

thread_local! {
    static VFXP_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// --verify-render-lists: an effect pass of a scene view (main+0xa414ac) predicted by itself.
static MAT_CALLS: AtomicU64 = AtomicU64::new(0);
static MAT_SAME: AtomicU64 = AtomicU64::new(0);
static MAT_DIFFERENT: AtomicU64 = AtomicU64::new(0);
static MAT_STOPS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

thread_local! {
    static MAT_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// --verify-render-lists: a material bind (main+0x7b42f0, run while the game records a material's list) predicted
/// and compared with what the game records.
#[no_mangle]
pub unsafe extern "C" fn on_material(d: usize, w: usize, entry: usize, set: usize, a: u32, b: u32) -> u32 {
    if crate::nvn::api::capturing() {
        return 0;
    }
    MAT_CALLS.fetch_add(1, Ordering::Relaxed);
    let mut port = draw::Port::new(mem::Mem::predicting(), crate::guest::loader::main_base(), false);
    let mut result = Ok(());
    let expected =
        crate::nvn::api::capture_dry(|| result = material::bind(&mut port, d, w, entry, set, a as i32, b as i32));
    match result {
        Ok(()) => {
            MAT_EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            crate::nvn::api::capture_collect();
        }
        Err(what) => {
            let mut s = MAT_STOPS.lock().unwrap();
            match s.iter_mut().find(|x| x.0 == what) {
                Some(x) => x.1 += 1,
                None => s.push((what, 1)),
            }
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_material_end() -> u32 {
    let Some(expected) = MAT_EXPECTED.with(|e| e.borrow_mut().take()) else { return 0 };
    let got = crate::nvn::api::capture_collected();
    if expected == got {
        MAT_SAME.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let k = MAT_DIFFERENT.fetch_add(1, Ordering::Relaxed);
    if k < 4 {
        println!("[model-draw] material mismatch #{k}:");
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            println!("[model-draw]   {} port: {e}", if e == g { " " } else { "!" });
            if e != g {
                println!("[model-draw]     game: {g}");
            }
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_vfx_pass(obj: usize, w: usize, views: usize, pass: u32, path: u32, _x5: usize, over: usize, mode: u32) -> u32 {
    if crate::nvn::api::capturing() {
        return 0;
    }
    VFXP_CALLS.fetch_add(1, Ordering::Relaxed);
    let mut port = draw::Port::new(mem::Mem::predicting(), crate::guest::loader::main_base(), false);
    let mut result = Ok(());
    let expected = crate::nvn::api::capture_dry(|| result = vfx::draw_pass(&mut port, obj, w, views, pass, path, over, mode));
    match result {
        Ok(()) => {
            VFXP_EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            ring::watch();
            crate::nvn::api::capture_collect();
        }
        Err(what) => {
            ring::discard();
            vfx::discard_blocks();
            let mut s = VFXP_STOPS.lock().unwrap();
            match s.iter_mut().find(|x| x.0 == what) {
                Some(x) => x.1 += 1,
                None => s.push((what, 1)),
            }
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_vfx_pass_end() -> u32 {
    let Some(expected) = VFXP_EXPECTED.with(|e| e.borrow_mut().take()) else { return 0 };
    let got = crate::nvn::api::capture_collected();
    let why = vfx::check_blocks();
    let expected = match ring::resolve(expected) {
        Ok(e) => e,
        Err(why) => {
            VFXP_DIFFERENT.fetch_add(1, Ordering::Relaxed);
            println!("[model-draw] vfx pass mismatch: {why}");
            return 0;
        }
    };
    if expected == got && why.is_none() {
        VFXP_SAME.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let k = VFXP_DIFFERENT.fetch_add(1, Ordering::Relaxed);
    if k < 6 {
        println!("[model-draw] vfx pass mismatch #{k}: {}", why.unwrap_or_default());
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            if e != g {
                println!("[model-draw]   ! port: {e}");
                println!("[model-draw]     game: {g}");
            }
        }
    }
    0
}

/// --verify-render-lists: the effect system's draw for a view (main+0xa41a38) predicted by itself.
#[no_mangle]
pub unsafe extern "C" fn on_vfx_view(sys0: usize, g: usize, groups: usize, n: u32, mask: u32, a3: usize, flags: u32) -> u32 {
    if crate::nvn::api::capturing() {
        return 0;
    }
    VFXV_CALLS.fetch_add(1, Ordering::Relaxed);
    let mut port = draw::Port::new(mem::Mem::predicting(), crate::guest::loader::main_base(), false);
    let mut result = Ok(());
    let expected = crate::nvn::api::capture_dry(|| result = vfx::draw_view(&mut port, sys0, g, groups, n, mask, a3, flags));
    match result {
        Ok(()) => {
            VFXV_EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            ring::watch();
            crate::nvn::api::capture_collect();
        }
        Err(what) => {
            ring::discard();
            vfx::discard_blocks();
            let mut s = VFXV_STOPS.lock().unwrap();
            match s.iter_mut().find(|x| x.0 == what) {
                Some(x) => x.1 += 1,
                None => s.push((what, 1)),
            }
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_vfx_view_end() -> u32 {
    let Some(expected) = VFXV_EXPECTED.with(|e| e.borrow_mut().take()) else { return 0 };
    let got = crate::nvn::api::capture_collected();
    let why = vfx::check_blocks();
    let expected = match ring::resolve(expected) {
        Ok(e) => e,
        Err(why) => {
            VFXV_DIFFERENT.fetch_add(1, Ordering::Relaxed);
            println!("[model-draw] vfx view mismatch: {why}");
            return 0;
        }
    };
    if expected == got && why.is_none() {
        VFXV_SAME.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let k = VFXV_DIFFERENT.fetch_add(1, Ordering::Relaxed);
    if k < 6 {
        println!("[model-draw] vfx view mismatch #{k}: {}", why.unwrap_or_default());
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            if e != g {
                println!("[model-draw]   ! port: {e}");
                println!("[model-draw]     game: {g}");
            }
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_vfx_end() -> u32 {
    let Some(expected) = VFX_EXPECTED.with(|e| e.borrow_mut().take()) else { return 0 };
    let got = crate::nvn::api::capture_collected();
    if let Some(why) = vfx::check_blocks() {
        let k = VFX_DIFFERENT.fetch_add(1, Ordering::Relaxed);
        if k < 6 {
            println!("[model-draw] vfx mismatch: {why}");
        }
        return 0;
    }
    let expected = match ring::resolve(expected) {
        Ok(e) => e,
        Err(why) => {
            VFX_DIFFERENT.fetch_add(1, Ordering::Relaxed);
            println!("[model-draw] vfx mismatch: {why}");
            return 0;
        }
    };
    if expected == got {
        VFX_SAME.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let k = VFX_DIFFERENT.fetch_add(1, Ordering::Relaxed);
    if k < 6 {
        println!("[model-draw] vfx mismatch #{k}:");
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            println!("[model-draw]   {} port: {e}", if e == g { " " } else { "!" });
            if e != g {
                println!("[model-draw]     game: {g}");
            }
        }
    }
    0
}

fn report() {
    let (n, same, diff) = (CALLS.load(Ordering::Relaxed), SAME.load(Ordering::Relaxed), DIFFERENT.load(Ordering::Relaxed));
    let mut s = STOPS.lock().unwrap().clone();
    s.sort_by(|a, b| b.1.cmp(&a.1));
    let stops: Vec<String> = s.iter().map(|(w, k)| format!("{w} {:.1}%", *k as f64 * 100.0 / n as f64)).collect();
    let vf: Vec<String> =
        draw::UNKNOWN_VFNS.lock().unwrap().iter().map(|(slot, f)| format!("+{slot:#x}=main+{f:#x}")).collect();
    if !vf.is_empty() {
        println!("[model-draw] virtual functions met: {}", vf.join(" "));
    }
    let dp = dispatch::ported_report();
    if !dp.is_empty() {
        println!("[model-draw] dispatch targets reached: {dp}");
    }
    let (ls, ld) = (material::LISTS[0].load(Ordering::Relaxed), material::LISTS[1].load(Ordering::Relaxed));
    if ls + ld > 0 {
        println!("[model-draw] material lists called: {} as their material binds now, {ld} differ", ls);
    }
    let mn = MAT_CALLS.load(Ordering::Relaxed);
    if mn > 0 {
        let mut s = MAT_STOPS.lock().unwrap().clone();
        s.sort_by(|a, b| b.1.cmp(&a.1));
        let stops: Vec<String> = s.iter().map(|(w, k)| format!("{w} {k}")).collect();
        println!(
            "[model-draw] material binds checked: {mn}, {} identical, {} different; unported: {}",
            MAT_SAME.load(Ordering::Relaxed),
            MAT_DIFFERENT.load(Ordering::Relaxed),
            stops.join(", ")
        );
    }
    let bv: Vec<String> = BY_VIEW
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.1 .1 != 0)
        .map(|((v, k), (s, d))| format!("view {v} kind {k:#x}: {d} of {}", s + d))
        .collect();
    if !bv.is_empty() {
        println!("[model-draw] different by view: {}", bv.join(", "));
    }
    let pv = perview::report();
    if !pv.is_empty() {
        println!("[model-draw] per-view calls left to the game: {pv}");
    }
    let cl = cells::report();
    if !cl.is_empty() {
        println!("[model-draw] cell renderer calls left to the game: {cl}");
    }
    let gt = grid::report();
    if !gt.is_empty() {
        println!("[model-draw] grid targets left to the game: {gt}");
    }
    let dt = dispatch::report();
    if !dt.is_empty() {
        println!("[model-draw] dispatch targets left to the game: {dt}");
    }
    let tv = terrain::report();
    if !tv.is_empty() {
        println!("[model-draw] terrain variants left to the game: {tv}");
    }
    let dr = render_list::drawables_report();
    if !dr.is_empty() {
        println!("[model-draw] drawables left to the game: {dr}");
    }
    let pn = VFXP_CALLS.load(Ordering::Relaxed);
    if pn != 0 {
        let mut s = VFXP_STOPS.lock().unwrap().clone();
        s.sort_by(|a, b| b.1.cmp(&a.1));
        let stops: Vec<String> = s.iter().map(|(w, k)| format!("{w} {:.1}%", *k as f64 * 100.0 / pn as f64)).collect();
        println!(
            "[model-draw] vfx passes checked by themselves: {pn}, {:.1}% identical, {} different; unported: {}",
            VFXP_SAME.load(Ordering::Relaxed) as f64 * 100.0 / pn as f64,
            VFXP_DIFFERENT.load(Ordering::Relaxed),
            stops.join(", ")
        );
    }
    let wn = VFXV_CALLS.load(Ordering::Relaxed);
    if wn != 0 {
        let mut s = VFXV_STOPS.lock().unwrap().clone();
        s.sort_by(|a, b| b.1.cmp(&a.1));
        let stops: Vec<String> = s.iter().map(|(w, k)| format!("{w} {:.1}%", *k as f64 * 100.0 / wn as f64)).collect();
        println!(
            "[model-draw] vfx views checked by themselves: {wn}, {:.1}% identical, {} different; unported: {}",
            VFXV_SAME.load(Ordering::Relaxed) as f64 * 100.0 / wn as f64,
            VFXV_DIFFERENT.load(Ordering::Relaxed),
            stops.join(", ")
        );
    }
    let vn = VFX_CALLS.load(Ordering::Relaxed);
    if vn != 0 {
        let mut s = VFX_STOPS.lock().unwrap().clone();
        s.sort_by(|a, b| b.1.cmp(&a.1));
        let stops: Vec<String> = s.iter().map(|(w, k)| format!("{w} {:.1}%", *k as f64 * 100.0 / vn as f64)).collect();
        println!(
            "[model-draw] vfx emitters checked by themselves: {vn}, {:.1}% identical, {} different; unported: {}; callbacks: {}",
            VFX_SAME.load(Ordering::Relaxed) as f64 * 100.0 / vn as f64,
            VFX_DIFFERENT.load(Ordering::Relaxed),
            stops.join(", "),
            vfx::report()
        );
    }
    let (ts, td) = (TARGETS[1].load(Ordering::Relaxed), TARGETS[0].load(Ordering::Relaxed));
    if ts + td != 0 {
        println!("[model-draw] render buffer binds: {ts} identical, {td} different");
    }
    let (vs, vd) = (VIEWPORTS[1].load(Ordering::Relaxed), VIEWPORTS[0].load(Ordering::Relaxed));
    if vs + vd != 0 {
        println!("[model-draw] viewports: {vs} identical, {vd} different");
    }
    let (pd, pl) = (PRIMITIVES[0].load(Ordering::Relaxed), PRIMITIVES[1].load(Ordering::Relaxed));
    if pd + pl != 0 {
        println!("[model-draw] agl / sead primitives drawn natively: {pd}, left to the game: {pl}");
    }
    if let Some(l) = QUERY_ITEMS.summary("item queries") {
        println!("{l}");
    }
    if let Some(l) = OBJECT_ITEMS.summary("item objects") {
        println!("{l}");
    }
    let on = OCCL_CALLS.load(Ordering::Relaxed);
    if on != 0 {
        let s = OCCL_STOPS.lock().unwrap().clone();
        let stops: Vec<String> = s.iter().map(|(w, k)| format!("{w} {:.1}%", *k as f64 * 100.0 / on as f64)).collect();
        println!(
            "[model-draw] occlusion items: {on} calls, {} identical, {} different; stops: {}; item functions: {}",
            OCCL_SAME.load(Ordering::Relaxed),
            OCCL_DIFFERENT.load(Ordering::Relaxed),
            stops.join(", "),
            occlusion::report()
        );
    }
    let ln = LIGHT_CALLS.load(Ordering::Relaxed);
    if ln != 0 {
        let s = LIGHT_STOPS.lock().unwrap().clone();
        let stops: Vec<String> = s.iter().map(|(w, k)| format!("{w} {:.1}%", *k as f64 * 100.0 / ln as f64)).collect();
        println!(
            "[model-draw] light volumes: {ln} calls, {} identical, {} different; stops: {}",
            LIGHT_SAME.load(Ordering::Relaxed),
            LIGHT_DIFFERENT.load(Ordering::Relaxed),
            stops.join(", ")
        );
    }
    let dn = DISPATCH_CALLS.load(Ordering::Relaxed);
    if dn != 0 {
        let mut s = DISPATCH_STOPS.lock().unwrap().clone();
        s.sort_by(|a, b| b.1.cmp(&a.1));
        let stops: Vec<String> = s.iter().map(|(w, k)| format!("{w} {:.1}%", *k as f64 * 100.0 / dn as f64)).collect();
        println!(
            "[model-draw] dispatch draws checked by themselves: {dn}, {:.1}% identical, {} different; unported: {}",
            DISPATCH_SAME.load(Ordering::Relaxed) as f64 * 100.0 / dn as f64,
            DISPATCH_DIFFERENT.load(Ordering::Relaxed),
            stops.join(", ")
        );
    }
    println!(
        "[model-draw] {n} calls: {:.1}% identical, {diff} different ({} reproduced by predicting again after the game's \
         call); unported: {}",
        same as f64 * 100.0 / n as f64,
        MOVED.load(Ordering::Relaxed),
        stops.join(", ")
    );
}

extern "C" {
    fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
}

pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    let d = &crate::settings::get().totk.diagnostics;
    if d.verify_model_draws || (d.native_model_draws && !d.native_render_lists) {
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_model_draw(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_model_draw_end())) as usize;
        hook(base, mem_space, SITE, SITE_INSTRUCTION, pre, post, "Model draw");
    }
    if d.verify_render_lists || d.native_render_lists {
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_render_list(_, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_model_draw_end())) as usize;
        hook(base, mem_space, LIST_SITE, LIST_SITE_INSTRUCTION, pre, post, "Render list");
    }
    let metal = crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Metal;
    if d.native_render_lists && d.native_model_draws && metal {
        let post = mem_space.make_hle_trampoline(crate::hle!(on_primitive_end())) as usize;
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_graphics_context_apply(_, _))) as usize;
        hook(base, mem_space, GRAPHICS_CONTEXT_APPLY.0, GRAPHICS_CONTEXT_APPLY.1, pre, post, "sead::GraphicsContext::apply");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_program_apply(_, _))) as usize;
        hook(base, mem_space, PROGRAM_APPLY.0, PROGRAM_APPLY.1, pre, post, "Program apply");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_program_block(_, _, _, _, _))) as usize;
        hook(base, mem_space, PROGRAM_BLOCK.0, PROGRAM_BLOCK.1, pre, post, "Program block");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_model_pass(_, _, _, _, _, _, _, _))) as usize;
        hook(base, mem_space, 0x931960, 0xa9ba7bfd, pre, post, "Model pass");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_block_bind(_, _, _, _))) as usize;
        hook(base, mem_space, 0x74efd4, 0xa9ba7bfd, pre, post, "Block bind");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_default_textures(_, _))) as usize;
        hook(base, mem_space, 0x9c7e04, 0xa9ba7bfd, pre, post, "Default textures");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_default_uniforms(_, _))) as usize;
        hook(base, mem_space, 0x9c7c80, 0xa9ba7bfd, pre, post, "Default uniforms");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_program_texture(_, _, _, _))) as usize;
        hook(base, mem_space, PROGRAM_TEXTURE.0, PROGRAM_TEXTURE.1, pre, post, "Program texture");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_program_block_at(_, _, _, _, _))) as usize;
        hook(base, mem_space, PROGRAM_BLOCK_AT.0, PROGRAM_BLOCK_AT.1, pre, post, "Program block (pointer)");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_stream_draw(_, _, _))) as usize;
        hook(base, mem_space, STREAM_DRAW.0, STREAM_DRAW.1, pre, post, "Index stream draw");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_shape_buffers(_, _))) as usize;
        hook(base, mem_space, SHAPE_BUFFERS.0, SHAPE_BUFFERS.1, pre, post, "Shape vertex buffers");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_shape_states(_, _))) as usize;
        hook(base, mem_space, SHAPE_STATES.0, SHAPE_STATES.1, pre, post, "Shape vertex states");
        macro_rules! gfx {
            ($site:expr, $f:expr, $what:expr) => {{
                let pre = mem_space.make_hle_trampoline($f) as usize;
                let first = std::ptr::read((base + $site) as *const u32);
                hook(base, mem_space, $site, first, pre, post, $what);
            }};
        }
        gfx!(0x30274, crate::hle!(on_gfx_constant_buffer(_, _, _, _, _)), "nn::gfx SetConstantBuffer");
        gfx!(0x302e0, crate::hle!(on_gfx_storage_buffer(_, _, _, _, _)), "nn::gfx SetUnorderedAccessBuffer");
        gfx!(0x7650a4, crate::hle!(on_gfx_vertex_buffer(_, _, _, _, _)), "nn::gfx SetVertexBuffer");
        gfx!(0x2f86c, crate::hle!(on_gfx_draw_indexed(_, _, _, _, _, _)), "nn::gfx DrawIndexed");
        gfx!(0x2f7a0, crate::hle!(on_gfx_dispatch(_, _, _, _)), "nn::gfx Dispatch");
        gfx!(0x2f7b4, crate::hle!(on_gfx_draw(_, _, _, _)), "nn::gfx Draw");
        gfx!(0x2f804, crate::hle!(on_gfx_draw_instanced(_, _, _, _, _, _)), "nn::gfx Draw instanced");
        gfx!(0x74ee70, crate::hle!(on_gfx_texture(_, _, _, _, _)), "nn::gfx SetTextureAndSampler");
        gfx!(0x74b81c, crate::hle!(on_gfx_shader(_, _, _)), "nn::gfx SetShader");
        gfx!(0x7650f4, crate::hle!(on_gfx_vertex_state(_, _)), "nn::gfx SetVertexState");
        gfx!(0x781064, crate::hle!(on_gfx_blend_state(_, _)), "nn::gfx SetBlendState");
        gfx!(0x78112c, crate::hle!(on_gfx_depth_stencil_state(_, _)), "nn::gfx SetDepthStencilState");
        gfx!(0x2f8ec, crate::hle!(on_gfx_rasterizer_state(_, _)), "nn::gfx SetRasterizerState");
    }
    if (d.native_render_lists && d.native_model_draws && metal) || d.verify_render_lists {
        let post = mem_space.make_hle_trampoline(crate::hle!(on_primitive_end())) as usize;
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_viewport_apply(_, _, _))) as usize;
        hook(base, mem_space, VIEWPORT_APPLY.0, VIEWPORT_APPLY.1, pre, post, "sead::Viewport::apply");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_render_buffer_bind(_, _))) as usize;
        hook(base, mem_space, RENDER_BUFFER_BIND.0, RENDER_BUFFER_BIND.1, pre, post, "agl::RenderBuffer::bind");
    }
    if d.verify_render_lists || (d.native_render_lists && d.native_model_draws && metal) {
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_vfx(_, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_vfx_end())) as usize;
        hook(base, mem_space, VFX_SITE, VFX_SITE_INSTRUCTION, pre, post, "Vfx emitter draw");
    }
    if d.verify_render_lists || d.native_render_lists {
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_lights(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_lights_end())) as usize;
        hook(base, mem_space, LIGHTS_SITE, LIGHTS_SITE_INSTRUCTION, pre, post, "Light volumes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_occlusion(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_occlusion_end())) as usize;
        hook(base, mem_space, OCCLUSION_SITE, OCCLUSION_SITE_INSTRUCTION, pre, post, "Occlusion items");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_query_item(_, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_query_item_end())) as usize;
        hook(base, mem_space, QUERY_ITEM_SITE, QUERY_ITEM_SITE_INSTRUCTION, pre, post, "Item queries");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_object_item(_, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_object_item_end())) as usize;
        hook(base, mem_space, OBJECT_ITEM_SITE, OBJECT_ITEM_SITE_INSTRUCTION, pre, post, "Item objects");
    }
    if d.verify_render_lists {
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_dispatch(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_dispatch_end())) as usize;
        hook(base, mem_space, DISPATCH_SITE, DISPATCH_SITE_INSTRUCTION, pre, post, "Dispatch draw");

        let pre = mem_space.make_hle_trampoline(crate::hle!(on_vfx_view(_, _, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_vfx_view_end())) as usize;
        hook(base, mem_space, VFX_VIEW_SITE, VFX_VIEW_SITE_INSTRUCTION, pre, post, "Vfx view draw");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_vfx_pass(_, _, _, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_vfx_pass_end())) as usize;
        hook(base, mem_space, VFX_PASS_SITE, VFX_PASS_SITE_INSTRUCTION, pre, post, "Vfx pass draw");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_material(_, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_material_end())) as usize;
        hook(base, mem_space, MATERIAL_SITE, MATERIAL_SITE_INSTRUCTION, pre, post, "Material bind");
    }
    if d.verify_render_lists || d.verify_model_draws {
        let pre = mem_space.make_hle_trampoline(crate::hle!(ring::on_ring_alloc(_))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(ring::on_ring_alloc_end())) as usize;
        hook(base, mem_space, ring::SITE, ring::SITE_INSTRUCTION, pre, post, "Ring allocation");
        let pre = mem_space.make_hle_trampoline(crate::hle!(ring::on_ring_block())) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(ring::on_ring_block_end(_))) as usize;
        hook(base, mem_space, ring::BLOCK_SITE, ring::BLOCK_SITE_INSTRUCTION, pre, post, "Ring block allocation");
    }
}

/// The entry of the function at main+`site` branches to a wrapper: pre(args); unless it returned nonzero, the function
/// itself (the displaced instruction, then the rest of it) as a call; then post().
unsafe fn hook(
    base: usize,
    mem_space: &mut crate::guest::loader::GuestMemorySpace,
    site_off: usize,
    instruction: u32,
    pre: usize,
    post: usize,
    what: &str,
) {
    let site = (base + site_off) as *mut u32;
    let actual = std::ptr::read(site);
    if actual != instruction {
        println!("[!] {what} hook not installed: main+{site_off:#x} = {actual:#010x}");
        return;
    }
    let wrap = mem_space.make_code_block(22) as usize;
    let b = |from: usize, to: usize, link: bool| -> Option<u32> {
        let d = to as i64 - from as i64;
        (d.abs() < 1 << 27).then(|| (if link { 0x9400_0000 } else { 0x1400_0000 }) | ((d >> 2) as u32 & 0x03ff_ffff))
    };
    let at = |k: usize| wrap + k * 4;
    let (Some(to_wrap), Some(call_pre), Some(call_body), Some(call_post), Some(back)) = (
        b(site as usize, wrap, false),
        b(at(6), pre, true),
        b(at(12), at(20), true),
        b(at(14), post, true),
        b(at(21), site as usize + 4, false),
    ) else {
        println!("[!] {what} hook not installed: wrapper out of branch range");
        return;
    };
    // The displaced instruction runs at at(20): an adrp is re-encoded for the page it named.
    let displaced = if instruction & 0x9f00_0000 == 0x9000_0000 {
        let imm = (((instruction >> 5) & 0x7ffff) << 2 | (instruction >> 29) & 3) as i64;
        let imm = (imm << 43) >> 43;
        let page = ((site as usize & !0xfff) as i64).wrapping_add(imm << 12);
        let delta = (page - (at(20) & !0xfff) as i64) >> 12;
        if delta.abs() >= 1 << 20 {
            println!("[!] {what} hook not installed: adrp out of range");
            return;
        }
        let d = delta as u32;
        (instruction & 0x9f00_001f) | (d & 3) << 29 | ((d >> 2) & 0x7ffff) << 5
    } else {
        instruction
    };
    let code: [u32; 22] = [
        0xa9ba7bfd,  // 0: stp x29, x30, [sp, #-0x60]!
        0x910003fd,  // 1: mov x29, sp
        0xa90107e0,  // 2: stp x0, x1, [sp, #0x10]
        0xa9020fe2,  // 3: stp x2, x3, [sp, #0x20]
        0xa90317e4,  // 4: stp x4, x5, [sp, #0x30]
        0xa9041fe6,  // 5: stp x6, x7, [sp, #0x40]
        call_pre,    // 6: bl pre
        0x35000160,  // 7: cbnz w0, 18 (handled)
        0xa9441fe6,  // 8: ldp x6, x7, [sp, #0x40]
        0xa94317e4,  // 9: ldp x4, x5, [sp, #0x30]
        0xa9420fe2,  // 10: ldp x2, x3, [sp, #0x20]
        0xa94107e0,  // 11: ldp x0, x1, [sp, #0x10]
        call_body,   // 12: bl 20 (the function)
        0xf9000be0,  // 13: str x0, [sp, #0x10] (its result, kept across post)
        call_post,   // 14: bl post
        0xf9400be0,  // 15: ldr x0, [sp, #0x10]
        0xa8c67bfd,  // 16: ldp x29, x30, [sp], #0x60
        0xd65f03c0,  // 17: ret
        0xa8c67bfd,  // 18: ldp x29, x30, [sp], #0x60
        0xd65f03c0,  // 19: ret
        displaced,   // 20: the displaced instruction
        back,        // 21: b site+4
    ];
    let w = wrap as *mut u32;
    for (k, c) in code.iter().enumerate() {
        std::ptr::write(w.add(k), *c);
    }
    sys_icache_invalidate(w as *mut libc::c_void, code.len() * 4);
    std::ptr::write(site, to_wrap);
    sys_icache_invalidate(site as *mut libc::c_void, 4);
    println!("[+] {what} hook installed at main+{site_off:#x}");
}
