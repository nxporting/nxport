// TotK's model renderer (main+0x688c68), ported (draw.rs). --verify-model-draws runs the port in predict mode before
// each call of the game's function and compares the NVN commands the game then records with the prediction; calls
// that reach a helper the port does not implement yet are counted by that helper.

mod binds;
mod buffer_passes;
mod cave;
mod cubemap;
mod cells;
mod dispatch;
mod deferred_view_pass;
mod display_lists;
mod dof_blur_chain;
mod dof_passes;
mod draw;
mod grid;
mod items;
mod light_pass;
mod mask_callees;
mod lighting;
mod lights;
mod list_variants;
mod gridtile;
mod material;
mod occlusion;
mod mem;
mod perview;
mod ping_pong;
mod post_chain;
mod program_draws;
mod render_list;
mod ring;
mod scene_passes;
mod scene_points;
mod shadow_systems;
mod snapshot;
mod targets;
mod terrain;
mod texture_slots;
mod vfx;
mod view_callees;
mod view_passes;
mod viewport;
mod volume_mask_chain;

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
    if crate::nvn::api::capturing_to_compare() {
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
    if crate::nvn::api::capturing_to_compare() {
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
    if crate::nvn::api::capturing_to_compare() {
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

static CUBEMAP_FACES: Checked = Checked::new();

thread_local! {
    static CUBEMAP_FACE_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x7ee7a8(w, tex, level, face, kind, value in s0): one face of a cube map mip filter pass (cubemap.rs; hooked with
/// hook_float: `floats` points at the saved d0..d7).
#[no_mangle]
pub unsafe extern "C" fn on_cubemap_face(w: usize, tex: usize, level: u32, face: u32, kind: u32, _x5: u64, _x6: u64, floats: usize) -> u32 {
    let value = (floats as *const u32).read();
    CUBEMAP_FACES.run(&CUBEMAP_FACE_EXPECTED, move |p| cubemap::face(p, w, tex, level, face, kind, value))
}

#[no_mangle]
pub unsafe extern "C" fn on_cubemap_face_end() -> u32 {
    CUBEMAP_FACES.end("cube map faces", &CUBEMAP_FACE_EXPECTED)
}

static TRANSFORMED_QUADS: Checked = Checked::new();

thread_local! {
    static TRANSFORMED_QUAD_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x877418(w, prog, tex, m, rows, v): a full-screen copy with a transform (cubemap.rs).
#[no_mangle]
pub unsafe extern "C" fn on_transformed_quad(w: usize, prog: usize, tex: usize, m: usize, rows: usize, v: usize) -> u32 {
    TRANSFORMED_QUADS.run(&TRANSFORMED_QUAD_EXPECTED, move |p| cubemap::transformed_quad(p, w, prog, tex, m, rows, v))
}

#[no_mangle]
pub unsafe extern "C" fn on_transformed_quad_end() -> u32 {
    TRANSFORMED_QUADS.end("transformed quads", &TRANSFORMED_QUAD_EXPECTED)
}

static COPY_QUADS: Checked = Checked::new();

thread_local! {
    static COPY_QUAD_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x876e28(w, tex, m, rows, a, b, value in s0): a full-screen copy (cubemap.rs; hook_float).
#[no_mangle]
pub unsafe extern "C" fn on_copy_quad(w: usize, tex: usize, m: usize, rows: usize, a: i32, b: u32, _x6: u64, floats: usize) -> u32 {
    let value = (floats as *const f32).read();
    COPY_QUADS.run(&COPY_QUAD_EXPECTED, move |p| cubemap::copy_quad(p, w, tex, m, rows, a, b, value))
}

#[no_mangle]
pub unsafe extern "C" fn on_copy_quad_end() -> u32 {
    COPY_QUADS.end("copy quads", &COPY_QUAD_EXPECTED)
}

static SNAPSHOTS: Checked = Checked::new();

thread_local! {
    static SNAPSHOT_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xcc75ec(snap, w, index, ctx): a snapshot's mip levels (snapshot.rs).
#[no_mangle]
pub unsafe extern "C" fn on_snapshot(snap: usize, w: usize, index: u32, ctx: usize) -> u32 {
    SNAPSHOTS.run(&SNAPSHOT_EXPECTED, move |p| snapshot::draw(p, snap, w, index, ctx))
}

#[no_mangle]
pub unsafe extern "C" fn on_snapshot_end() -> u32 {
    SNAPSHOTS.end("snapshots", &SNAPSHOT_EXPECTED)
}

static LIGHT_SHAPES: Checked = Checked::new();

thread_local! {
    static LIGHT_SHAPE_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x969d28(ctx, light, view): one light with its own program (lights.rs).
#[no_mangle]
pub unsafe extern "C" fn on_light_shape(ctx: usize, light: usize, view: u32) -> u32 {
    LIGHT_SHAPES.run(&LIGHT_SHAPE_EXPECTED, move |p| lights::light_shape(p, ctx, light, view))
}

#[no_mangle]
pub unsafe extern "C" fn on_light_shape_end() -> u32 {
    LIGHT_SHAPES.end("light shapes", &LIGHT_SHAPE_EXPECTED)
}

static PING_PONGS: Checked = Checked::new();

thread_local! {
    static PING_PONG_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xc24c30(this, w): a pass between two render buffers (ping_pong.rs).
#[no_mangle]
pub unsafe extern "C" fn on_ping_pong(this: usize, w: usize) -> u32 {
    PING_PONGS.run(&PING_PONG_EXPECTED, move |p| ping_pong::draw(p, this, w))
}

#[no_mangle]
pub unsafe extern "C" fn on_ping_pong_end() -> u32 {
    PING_PONGS.end("ping-pong passes", &PING_PONG_EXPECTED)
}

static DISPLAY_LISTS: Checked = Checked::new();

thread_local! {
    static DISPLAY_LIST_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xbccb6c(set, w, rebuild): a set of recorded display lists (display_lists.rs).
#[no_mangle]
pub unsafe extern "C" fn on_display_lists(set: usize, w: usize, rebuild: u64) -> u32 {
    DISPLAY_LISTS.run(&DISPLAY_LIST_EXPECTED, move |p| display_lists::call(p, set, w, rebuild))
}

#[no_mangle]
pub unsafe extern "C" fn on_display_lists_end() -> u32 {
    DISPLAY_LISTS.end("display lists", &DISPLAY_LIST_EXPECTED)
}

static CLEARING_BINDS: Checked = Checked::new();

thread_local! {
    static CLEARING_BIND_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x8660b4(buffer, w, viewport, _, mask, values): a render buffer bind with clears (targets.rs).
#[no_mangle]
pub unsafe extern "C" fn on_clearing_bind(rb: usize, w: usize, vp: usize, _x3: u64, mask: u32, values: usize) -> u32 {
    CLEARING_BINDS.run(&CLEARING_BIND_EXPECTED, move |p| targets::bind_clear(p, rb, w, vp, mask, values))
}

#[no_mangle]
pub unsafe extern "C" fn on_clearing_bind_end() -> u32 {
    CLEARING_BINDS.end("clearing binds", &CLEARING_BIND_EXPECTED)
}

static TEXTURED_DRAWS: Checked = Checked::new();
static TINTED_DRAWS: Checked = Checked::new();

thread_local! {
    static TEXTURED_DRAW_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
    static TINTED_DRAW_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xc57014(w, prog, tex) / main+0xc55378(this, w, tex, color, blend): full-screen program draws
/// (program_draws.rs).
#[no_mangle]
pub unsafe extern "C" fn on_textured_draw(w: usize, prog: usize, tex: usize) -> u32 {
    TEXTURED_DRAWS.run(&TEXTURED_DRAW_EXPECTED, move |p| program_draws::textured(p, w, prog, tex))
}

#[no_mangle]
pub unsafe extern "C" fn on_textured_draw_end() -> u32 {
    TEXTURED_DRAWS.end("textured draws", &TEXTURED_DRAW_EXPECTED)
}

#[no_mangle]
pub unsafe extern "C" fn on_tinted_draw(this: usize, w: usize, tex: usize, color: usize, blend: usize) -> u32 {
    TINTED_DRAWS.run(&TINTED_DRAW_EXPECTED, move |p| program_draws::tinted(p, this, w, tex, color, blend))
}

#[no_mangle]
pub unsafe extern "C" fn on_tinted_draw_end() -> u32 {
    TINTED_DRAWS.end("tinted draws", &TINTED_DRAW_EXPECTED)
}

static DEPTH_BUFFER_ENDS: Checked = Checked::new();

thread_local! {
    static DEPTH_BUFFER_END_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// agl::sdw::ShadowMap::endDepthBuffer, main+0xa404ac(map, w, index) (targets.rs).
#[no_mangle]
pub unsafe extern "C" fn on_end_depth_buffer(map: usize, w: usize, index: u32) -> u32 {
    DEPTH_BUFFER_ENDS.run(&DEPTH_BUFFER_END_EXPECTED, move |p| targets::end_depth_buffer(p, map, w, index))
}

#[no_mangle]
pub unsafe extern "C" fn on_end_depth_buffer_end() -> u32 {
    DEPTH_BUFFER_ENDS.end("shadow depth buffer ends", &DEPTH_BUFFER_END_EXPECTED)
}

/// main+0x74fd08(w, handles, index, slots): deferred texture binds by slot bytes (returns whether any slots).
#[no_mangle]
pub unsafe extern "C" fn on_slots_deferred(w: usize, handles: usize, index: i32, slots: u32) -> u32 {
    primitive(|p| Ok(vfx::bind_slots_deferred(p, w, handles, index, slots) as u32))
}

static TEXTURE_QUADS: Checked = Checked::new();

thread_local! {
    static TEXTURE_QUAD_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xbe1b2c(w, prog, tex, viewport, scale, offset): a texture quad in a viewport (cubemap.rs).
#[no_mangle]
pub unsafe extern "C" fn on_texture_quad(w: usize, prog: usize, tex: usize, vp: usize, scale: usize, offset: usize) -> u32 {
    TEXTURE_QUADS.run(&TEXTURE_QUAD_EXPECTED, move |p| cubemap::texture_quad(p, w, prog, tex, vp, scale, offset))
}

#[no_mangle]
pub unsafe extern "C" fn on_texture_quad_end() -> u32 {
    TEXTURE_QUADS.end("texture quads", &TEXTURE_QUAD_EXPECTED)
}

static CAPTURE_QUADS: Checked = Checked::new();

thread_local! {
    static CAPTURE_QUAD_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x98aa34(w, tex, viewport, a, b, scale, offset, value in s0): a capture quad (cubemap.rs; hook_float).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn on_capture_quad(w: usize, tex: usize, vp: usize, a: usize, b: usize, scale: usize, offset: usize, floats: usize) -> u32 {
    let value = (floats as *const u32).read();
    CAPTURE_QUADS.run(&CAPTURE_QUAD_EXPECTED, move |p| cubemap::capture_quad(p, w, tex, vp, a, b, scale, offset, value))
}

#[no_mangle]
pub unsafe extern "C" fn on_capture_quad_end() -> u32 {
    CAPTURE_QUADS.end("capture quads", &CAPTURE_QUAD_EXPECTED)
}

static VIEW_COMPOSITES: Checked = Checked::new();

thread_local! {
    static VIEW_COMPOSITE_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x955ff4(this, w, view, ctx) (scene_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_view_composite(this: usize, w: usize, view: u32, ctx: usize) -> u32 {
    VIEW_COMPOSITES.run(&VIEW_COMPOSITE_EXPECTED, move |p| scene_passes::view_composite(p, this, w, view, ctx))
}

#[no_mangle]
pub unsafe extern "C" fn on_view_composite_end() -> u32 {
    VIEW_COMPOSITES.end("view composites", &VIEW_COMPOSITE_EXPECTED)
}

static VIEW_GRIDS: Checked = Checked::new();
static VIEW_FULLSCREENS: Checked = Checked::new();

thread_local! {
    static VIEW_GRID_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
    static VIEW_FULLSCREEN_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x957448(this, w, view, ctx) / main+0x9737f8(this, w, view, scene) (scene_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_view_grid(this: usize, w: usize, view: u32, ctx: usize) -> u32 {
    VIEW_GRIDS.run(&VIEW_GRID_EXPECTED, move |p| scene_passes::view_grid(p, this, w, view, ctx))
}

#[no_mangle]
pub unsafe extern "C" fn on_view_grid_end() -> u32 {
    VIEW_GRIDS.end("view grids", &VIEW_GRID_EXPECTED)
}

#[no_mangle]
pub unsafe extern "C" fn on_view_fullscreen(this: usize, w: usize, view: u32, scene: usize) -> u32 {
    VIEW_FULLSCREENS.run(&VIEW_FULLSCREEN_EXPECTED, move |p| scene_passes::view_fullscreen(p, this, w, view, scene))
}

#[no_mangle]
pub unsafe extern "C" fn on_view_fullscreen_end() -> u32 {
    VIEW_FULLSCREENS.end("view full-screen passes", &VIEW_FULLSCREEN_EXPECTED)
}

static SCENE_PASSES: Checked = Checked::new();
static ENTRY_VOLUMES: Checked = Checked::new();

thread_local! {
    static SCENE_PASS_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
    static ENTRY_VOLUMES_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x9618d0(this, w, view, scene, ctx, flags) (buffer_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_scene_pass(this: usize, w: usize, view: u32, scene: usize, ctx: usize, flags: u32) -> u32 {
    SCENE_PASSES.run(&SCENE_PASS_EXPECTED, move |p| buffer_passes::scene_pass(p, this, w, view, scene, ctx, flags))
}

#[no_mangle]
pub unsafe extern "C" fn on_scene_pass_end() -> u32 {
    SCENE_PASSES.end("scene buffer passes", &SCENE_PASS_EXPECTED)
}

/// main+0x95bef4(this, args) (buffer_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_entry_volumes(this: usize, args: usize) -> u32 {
    ENTRY_VOLUMES.run(&ENTRY_VOLUMES_EXPECTED, move |p| buffer_passes::entry_volumes(p, this, args))
}

#[no_mangle]
pub unsafe extern "C" fn on_entry_volumes_end() -> u32 {
    ENTRY_VOLUMES.end("entry volumes", &ENTRY_VOLUMES_EXPECTED)
}

static VIEW_COPIES: Checked = Checked::new();

thread_local! {
    static VIEW_COPY_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x959a14(this, w, view, tex, desc, ctx) (buffer_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_view_copy(this: usize, w: usize, view: u32, tex: usize, desc: usize, ctx: usize) -> u32 {
    VIEW_COPIES.run(&VIEW_COPY_EXPECTED, move |p| buffer_passes::view_copy(p, this, w, view, tex, desc, ctx))
}

#[no_mangle]
pub unsafe extern "C" fn on_view_copy_end() -> u32 {
    VIEW_COPIES.end("view copies", &VIEW_COPY_EXPECTED)
}

static REPROJECTIONS: Checked = Checked::new();

thread_local! {
    static REPROJECTION_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x9635b8(this, w, view) (buffer_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_reprojection(x: usize, w: usize, view: u32) -> u32 {
    REPROJECTIONS.run(&REPROJECTION_EXPECTED, move |p| buffer_passes::reprojection(p, x, w, view))
}

#[no_mangle]
pub unsafe extern "C" fn on_reprojection_end() -> u32 {
    REPROJECTIONS.end("reprojection passes", &REPROJECTION_EXPECTED)
}

static VOLUME_MASKS: Checked = Checked::new();

thread_local! {
    static VOLUME_MASK_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x95ad64(this, args) (buffer_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_volume_mask(this: usize, args: usize) -> u32 {
    VOLUME_MASKS.run(&VOLUME_MASK_EXPECTED, move |p| buffer_passes::volume_mask(p, this, args))
}

#[no_mangle]
pub unsafe extern "C" fn on_volume_mask_end() -> u32 {
    VOLUME_MASKS.end("volume masks", &VOLUME_MASK_EXPECTED)
}

static POST_PASSES: Checked = Checked::new();

thread_local! {
    static POST_PASS_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xc555c4(this, w, index, n) (program_draws.rs).
#[no_mangle]
pub unsafe extern "C" fn on_post_pass(this: usize, w: usize, index: u32, n: u32) -> u32 {
    POST_PASSES.run(&POST_PASS_EXPECTED, move |p| program_draws::post_pass(p, this, w, index, n))
}

#[no_mangle]
pub unsafe extern "C" fn on_post_pass_end() -> u32 {
    POST_PASSES.end("post passes", &POST_PASS_EXPECTED)
}

static LIGHT_PASSES: Checked = Checked::new();

thread_local! {
    static LIGHT_PASS_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x966f7c(this, w, view, scene, records, tex, shadows, storage) (light_pass.rs).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn on_light_pass(this: usize, w: usize, view: u32, scene: usize, records: usize, tex: usize, shadows: usize, storage: usize) -> u32 {
    LIGHT_PASSES.run(&LIGHT_PASS_EXPECTED, move |p| light_pass::light_pass(p, this, w, view, scene, records, tex, shadows, storage))
}

#[no_mangle]
pub unsafe extern "C" fn on_light_pass_end() -> u32 {
    LIGHT_PASSES.end("light passes", &LIGHT_PASS_EXPECTED)
}

static VIEW_TARGET_DRAWS: Checked = Checked::new();

thread_local! {
    static VIEW_TARGET_DRAW_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x957820(this, w, view, flags, tex, desc, depth) (buffer_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_view_target_draw(this: usize, w: usize, view: u32, flags: u32, tex: usize, desc: usize, depth: usize) -> u32 {
    VIEW_TARGET_DRAWS.run(&VIEW_TARGET_DRAW_EXPECTED, move |p| buffer_passes::view_target_draw(p, this, w, view, flags, tex, desc, depth))
}

#[no_mangle]
pub unsafe extern "C" fn on_view_target_draw_end() -> u32 {
    VIEW_TARGET_DRAWS.end("view target draws", &VIEW_TARGET_DRAW_EXPECTED)
}

static DOF_COMPOSITES: Checked = Checked::new();
static EDGE_ADJUSTS: Checked = Checked::new();

thread_local! {
    static DOF_COMPOSITE_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
    static EDGE_ADJUST_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xa3e79c(this, w, view, k) / main+0xa434a8(this, w, view, arg, flags) (dof_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_dof_composite(this: usize, w: usize, view: i32, k: u32) -> u32 {
    DOF_COMPOSITES.run(&DOF_COMPOSITE_EXPECTED, move |p| dof_passes::dof_composite(p, this, w, view, k))
}

#[no_mangle]
pub unsafe extern "C" fn on_dof_composite_end() -> u32 {
    DOF_COMPOSITES.end("depth of field composites", &DOF_COMPOSITE_EXPECTED)
}

#[no_mangle]
pub unsafe extern "C" fn on_edge_adjust(this: usize, w: usize, view: u32, arg: u32, flags: u32) -> u32 {
    EDGE_ADJUSTS.run(&EDGE_ADJUST_EXPECTED, move |p| dof_passes::edge_adjust(p, this, w, view, arg, flags))
}

#[no_mangle]
pub unsafe extern "C" fn on_edge_adjust_end() -> u32 {
    EDGE_ADJUSTS.end("edge adjusts", &EDGE_ADJUST_EXPECTED)
}

static VIEW_GATHERS: Checked = Checked::new();
static VIEW_TARGET_PASSES: Checked = Checked::new();
static OCCLUSION_BUFFERS: Checked = Checked::new();
static NOISE_GAUGES: Checked = Checked::new();

thread_local! {
    static VIEW_GATHER_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
    static VIEW_TARGET_PASS_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
    static OCCLUSION_BUFFER_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
    static NOISE_GAUGE_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xd4cac4(this, w, view), main+0xbd8c40(this, w, view), main+0xbdad04(this, w, view, flags),
/// main+0xdb6438(this, w, scale in s0) (view_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_view_gather(this: usize, w: usize, view: u32) -> u32 {
    VIEW_GATHERS.run(&VIEW_GATHER_EXPECTED, move |p| view_passes::view_gather(p, this, w, view))
}

#[no_mangle]
pub unsafe extern "C" fn on_view_gather_end() -> u32 {
    VIEW_GATHERS.end("view gathers", &VIEW_GATHER_EXPECTED)
}

#[no_mangle]
pub unsafe extern "C" fn on_view_target_passes(this: usize, w: usize, view: u32) -> u32 {
    VIEW_TARGET_PASSES.run(&VIEW_TARGET_PASS_EXPECTED, move |p| view_passes::view_target_passes(p, this, w, view))
}

#[no_mangle]
pub unsafe extern "C" fn on_view_target_passes_end() -> u32 {
    VIEW_TARGET_PASSES.end("view target passes", &VIEW_TARGET_PASS_EXPECTED)
}

#[no_mangle]
pub unsafe extern "C" fn on_occlusion_buffers(this: usize, w: usize, view: u32, flags: u32) -> u32 {
    OCCLUSION_BUFFERS.run(&OCCLUSION_BUFFER_EXPECTED, move |p| view_passes::occlusion_buffers(p, this, w, view, flags))
}

#[no_mangle]
pub unsafe extern "C" fn on_occlusion_buffers_end() -> u32 {
    OCCLUSION_BUFFERS.end("occlusion buffers", &OCCLUSION_BUFFER_EXPECTED)
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn on_noise_gauge(this: usize, w: usize, _x2: u64, _x3: u64, _x4: u64, _x5: u64, _x6: u64, floats: usize) -> u32 {
    let scale = (floats as *const f32).read();
    NOISE_GAUGES.run(&NOISE_GAUGE_EXPECTED, move |p| view_passes::noise_gauge(p, this, w, scale))
}

#[no_mangle]
pub unsafe extern "C" fn on_noise_gauge_end() -> u32 {
    NOISE_GAUGES.end("noise gauges", &NOISE_GAUGE_EXPECTED)
}

static TARGET_BLITS: Checked = Checked::new();

thread_local! {
    static TARGET_BLIT_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xe01cb8(this, w, _, ctx) (buffer_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_target_blit(this: usize, w: usize, _x2: usize, ctx: usize) -> u32 {
    TARGET_BLITS.run(&TARGET_BLIT_EXPECTED, move |p| buffer_passes::target_blit(p, this, w, ctx))
}

#[no_mangle]
pub unsafe extern "C" fn on_target_blit_end() -> u32 {
    TARGET_BLITS.end("target blits", &TARGET_BLIT_EXPECTED)
}

static VOLUME_MASK_CHAINS: Checked = Checked::new();

thread_local! {
    static VOLUME_MASK_CHAIN_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xa4c1ec(this, w, view, src) (volume_mask_chain.rs).
#[no_mangle]
pub unsafe extern "C" fn on_volume_mask_chain(this: usize, w: usize, view: u32, src: usize) -> u32 {
    VOLUME_MASK_CHAINS.run(&VOLUME_MASK_CHAIN_EXPECTED, move |p| volume_mask_chain::volume_mask_chain(p, this, w, view, src))
}

#[no_mangle]
pub unsafe extern "C" fn on_volume_mask_chain_end() -> u32 {
    VOLUME_MASK_CHAINS.end("volume mask chains", &VOLUME_MASK_CHAIN_EXPECTED)
}

static CORRECT_DEPTHS: Checked = Checked::new();

thread_local! {
    static CORRECT_DEPTH_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x9590f8(this, w, view, ctx) (buffer_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_correct_depth(this: usize, w: usize, view: u32, ctx: usize) -> u32 {
    CORRECT_DEPTHS.run(&CORRECT_DEPTH_EXPECTED, move |p| buffer_passes::correct_depth(p, this, w, view, ctx))
}

#[no_mangle]
pub unsafe extern "C" fn on_correct_depth_end() -> u32 {
    CORRECT_DEPTHS.end("correct depth passes", &CORRECT_DEPTH_EXPECTED)
}

static DEFERRED_VIEW_PASSES: Checked = Checked::new();

thread_local! {
    static DEFERRED_VIEW_PASS_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x96e6fc(this, w, view, ctx) (deferred_view_pass.rs).
#[no_mangle]
pub unsafe extern "C" fn on_deferred_view_pass(this: usize, w: usize, view: u32, ctx: usize) -> u32 {
    DEFERRED_VIEW_PASSES.run(&DEFERRED_VIEW_PASS_EXPECTED, move |p| deferred_view_pass::deferred_view_pass(p, this, w, view, ctx))
}

#[no_mangle]
pub unsafe extern "C" fn on_deferred_view_pass_end() -> u32 {
    DEFERRED_VIEW_PASSES.end("deferred view passes", &DEFERRED_VIEW_PASS_EXPECTED)
}

static LIGHTING: Checked = Checked::new();

thread_local! {
    static LIGHTING_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x973b40(this, w, view, args) (lighting.rs).
#[no_mangle]
pub unsafe extern "C" fn on_lighting(this: usize, w: usize, view: u32, args: usize) -> u32 {
    LIGHTING.run(&LIGHTING_EXPECTED, move |p| lighting::lighting(p, this, w, view, args))
}

#[no_mangle]
pub unsafe extern "C" fn on_lighting_end() -> u32 {
    LIGHTING.end("lighting", &LIGHTING_EXPECTED)
}

static DOF_BLUR_CHAINS: Checked = Checked::new();

thread_local! {
    static DOF_BLUR_CHAIN_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x96b618(this, w, view, recs, scene, ctx, s7, s8) (dof_blur_chain.rs).
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn on_dof_blur_chain(this: usize, w: usize, view: u32, recs: usize, scene: usize, ctx: usize, s7: usize, s8: usize) -> u32 {
    DOF_BLUR_CHAINS.run(&DOF_BLUR_CHAIN_EXPECTED, move |p| dof_blur_chain::blur_chain(p, this, w, view, recs, scene, ctx, s7, s8))
}

#[no_mangle]
pub unsafe extern "C" fn on_dof_blur_chain_end() -> u32 {
    DOF_BLUR_CHAINS.end("dof blur chains", &DOF_BLUR_CHAIN_EXPECTED)
}

static POST_CHAINS: Checked = Checked::new();

thread_local! {
    static POST_CHAIN_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xc4f7c8(this, w, view, args, n) (post_chain.rs).
#[no_mangle]
pub unsafe extern "C" fn on_post_chain(this: usize, w: usize, view: u32, args: usize, n: u32) -> u32 {
    POST_CHAINS.run(&POST_CHAIN_EXPECTED, move |p| post_chain::post_chain(p, this, w, view, args, n))
}

#[no_mangle]
pub unsafe extern "C" fn on_post_chain_end() -> u32 {
    POST_CHAINS.end("post chains", &POST_CHAIN_EXPECTED)
}

static MASK_POINTS: Checked = Checked::new();
static STATE_BLITS: Checked = Checked::new();

thread_local! {
    static MASK_POINTS_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
    static STATE_BLITS_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0xbde1b8(this, w, ctx, view) / main+0xbde7f0(this, w) (scene_points.rs).
#[no_mangle]
pub unsafe extern "C" fn on_mask_points(this: usize, w: usize, ctx: usize, view: u32) -> u32 {
    MASK_POINTS.run(&MASK_POINTS_EXPECTED, move |p| scene_points::mask_points(p, this, w, ctx, view))
}

#[no_mangle]
pub unsafe extern "C" fn on_mask_points_end() -> u32 {
    MASK_POINTS.end("mask points", &MASK_POINTS_EXPECTED)
}

#[no_mangle]
pub unsafe extern "C" fn on_state_blit(this: usize, w: usize) -> u32 {
    STATE_BLITS.run(&STATE_BLITS_EXPECTED, move |p| scene_points::target_blit(p, this, w))
}

#[no_mangle]
pub unsafe extern "C" fn on_state_blit_end() -> u32 {
    STATE_BLITS.end("state blits", &STATE_BLITS_EXPECTED)
}

static EFFECT_LAYERS: Checked = Checked::new();

thread_local! {
    static EFFECT_LAYER_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x95f810(this, w, view, _, k, flags) (scene_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_effect_layer(this: usize, w: usize, view: u32, _x3: u64, k: i32, flags: u32) -> u32 {
    EFFECT_LAYERS.run(&EFFECT_LAYER_EXPECTED, move |p| scene_passes::effect_layer(p, this, w, view, k, flags as u8))
}

#[no_mangle]
pub unsafe extern "C" fn on_effect_layer_end() -> u32 {
    EFFECT_LAYERS.end("effect layers", &EFFECT_LAYER_EXPECTED)
}

static ATMOSPHERES: Checked = Checked::new();

thread_local! {
    static ATMOSPHERE_EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// main+0x21d95d4(ctx, w, prog) (scene_passes.rs).
#[no_mangle]
pub unsafe extern "C" fn on_atmosphere(ctx: usize, w: usize, prog: usize) -> u32 {
    ATMOSPHERES.run(&ATMOSPHERE_EXPECTED, move |p| scene_passes::atmosphere(p, ctx, w, prog))
}

#[no_mangle]
pub unsafe extern "C" fn on_atmosphere_end() -> u32 {
    ATMOSPHERES.end("atmosphere passes", &ATMOSPHERE_EXPECTED)
}

/// main+0x98a150(rb, w, index, mask, color, stencil, viewport, depth in s0): a render buffer clear (targets.rs; hooked
/// with hook_float: `floats` points at the saved d0..d7).
#[no_mangle]
pub unsafe extern "C" fn on_target_clear(rb: usize, w: usize, index: i32, mask: u32, color: usize, stencil: i32, vp: usize, floats: usize) -> u32 {
    let depth = (floats as *const f32).read();
    primitive(|p| targets::clear(p, rb, w, index, mask, color, stencil, vp, depth).map(|_| 1))
}

/// main+0xc26458(rb, w): barriers for a render buffer's targets (targets.rs).
#[no_mangle]
pub unsafe extern "C" fn on_target_barriers(rb: usize, w: usize) -> u32 {
    primitive(|p| {
        targets::barrier_targets(p, rb, w);
        Ok(1)
    })
}

/// main+0x9314b4(renderer, w, args, view, out, user): a model pass through the renderer's recorded-pass cache.
#[no_mangle]
pub unsafe extern "C" fn on_cached_model_pass(r: usize, w: usize, a: usize, v: usize, out: usize, user: u64) -> u32 {
    primitive(|p| render_list::cached_model_pass(p, r, w, a, v, out, user).map(|_| 1))
}

/// main+0xc56c70(shader, w, mode): with mode bit 0 the shader's program (+8) applied (main+0x7ef4b8's body); returns
/// that bit (0: left to the game, which does nothing either).
#[no_mangle]
pub unsafe extern "C" fn on_shader_apply(shader: usize, w: usize, mode: u32) -> u32 {
    if mode & 1 == 0 {
        return 0;
    }
    primitive(|p| vfx::program_apply(p, p.m.r64(shader + 8), w).map(|_| 1))
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
        if crate::nvn::api::capturing_to_compare() {
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
        if crate::nvn::api::capturing_to_compare() {
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
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn on_gfx_draw_indexed_instanced(g: usize, topology: u32, format: u32, addr: usize, count: i32, base_vertex: i32, instances: i32, base_instance: i32) -> u32 {
    primitive(|p| {
        vfx::draw_indexed_instanced(p, g, topology, format, p.m.r64(addr) as u64, count, base_vertex, instances, base_instance);
        Ok(1)
    })
}

/// nn::gfx's memory flush / invalidation barriers (main+0x300d8 / main+0x30100); they return nothing.
#[no_mangle]
pub unsafe extern "C" fn on_gfx_flush_memory(g: usize, flags: u32) -> u32 {
    primitive(|p| {
        vfx::flush_memory(p, g, flags);
        Ok(1)
    })
}

#[no_mangle]
pub unsafe extern "C" fn on_gfx_invalidate_memory(g: usize, flags: u32) -> u32 {
    primitive(|p| {
        vfx::invalidate_memory(p, g, flags);
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
    if crate::nvn::api::capturing_to_compare() {
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
            let r = list_variants::draw(&mut port, q, w, view, flags);
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
    let expected = crate::nvn::api::capture_dry(|| result = list_variants::draw(&mut port, q, w, view, flags));
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

type CheckFn = Box<dyn Fn(&mut draw::Port) -> draw::R<()>>;

/// --compare-port: the Checked whose calls alternate between the game and the port.
static COMPARE_KEY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

thread_local! {
    /// The game's turn of the compared port is collecting its commands.
    static COMPARE_GAME: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The commands of a compared call (the first eight calls), one per line.
fn print_compare(who: &str, n: u64, cmds: &[String], stop: Option<&str>) {
    if n >= 8 {
        return;
    }
    println!("[compare] {who} call {n}: {} commands{}", cmds.len(), stop.map(|s| format!(", stopped: {s}")).unwrap_or_default());
    for (k, c) in cmds.iter().enumerate() {
        println!("[compare] {who} {n} {k:4} {c}");
    }
}

thread_local! {
    /// Ports to predict after the game's call (keyed by their Checked), innermost last.
    static CHECK_AFTER: std::cell::RefCell<Vec<(usize, CheckFn)>> = const { std::cell::RefCell::new(Vec::new()) };
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
    unsafe fn run(&'static self, expected: &'static std::thread::LocalKey<std::cell::RefCell<Option<Vec<String>>>>, f: impl Fn(&mut draw::Port) -> draw::R<()> + 'static) -> u32 {
        if crate::nvn::api::capturing_to_compare() {
            return 0;
        }
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        let base = crate::guest::loader::main_base();
        let d = &crate::settings::get().totk.diagnostics;
        if d.native_render_lists && COMPARE_KEY.load(Ordering::Relaxed) == self as *const Self as usize {
            // The game's turn: its commands are collected and printed at the end hook. The port's: run for real but
            // with NVN commands instead of native items, collected and not recorded (this call draws nothing).
            if n % 2 == 0 {
                COMPARE_GAME.with(|g| g.set(self as *const Self as usize));
                crate::nvn::api::capture_collect();
                return 0;
            }
            let mut port = draw::Port::new(mem::Mem::applying(), base, false);
            let mut r = Ok(());
            let got = crate::nvn::api::capture_dry_noted(|| r = f(&mut port));
            print_compare("port", n, &got, r.err());
            return 1;
        }
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
                if port.m.committed() {
                    // Past the commit point the game's state already changed: the game's call cannot run on top.
                    println!("[model-draw] stopped after its commit point: {what}");
                    self.stop("after the commit point");
                    port.flush();
                    crate::nvn::api::commit_buffered(commands);
                    self.stop(what);
                    return 1;
                }
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
            // State the game's call itself sets up (a texture it finishes, a block it fills): predicted after it.
            Err(what) if what.ends_with("still finishing") || what.ends_with("by the call") => {
                ring::discard();
                let key = self as *const Self as usize;
                CHECK_AFTER.with(|r| r.borrow_mut().push((key, Box::new(f))));
                ring::watch_all();
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
        let key = self as *const Self as usize;
        if COMPARE_GAME.with(|g| g.get()) == key {
            COMPARE_GAME.with(|g| g.set(0));
            let got = crate::nvn::api::capture_collected();
            print_compare("game", self.calls.load(Ordering::Relaxed) - 1, &got, None);
            return 0;
        }
        if COMPARE_KEY.load(Ordering::Relaxed) == 0 && crate::settings::get().totk.diagnostics.compare_port.as_deref() == Some(name) {
            COMPARE_KEY.store(key, Ordering::Relaxed);
        }
        let after = CHECK_AFTER.with(|r| {
            let mut r = r.borrow_mut();
            if r.last().is_some_and(|x| x.0 == key) { r.pop().map(|x| x.1) } else { None }
        });
        if let Some(f) = after {
            let got = crate::nvn::api::capture_collected();
            let mut port = draw::Port::new(mem::Mem::predicting(), crate::guest::loader::main_base(), false);
            let mut result = Ok(());
            let e = crate::nvn::api::capture_dry(|| result = f(&mut port));
            if let Err(what) = result {
                ring::discard();
                self.stop(what);
                return 0;
            }
            return match ring::resolve_with(e, &got) {
                Ok(e) => self.compare(name, e, got),
                Err(why) => {
                    self.different.fetch_add(1, Ordering::Relaxed);
                    println!("[model-draw] {name} mismatch (checked after the game's call): {why}");
                    0
                }
            };
        }
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
        self.compare(name, e, got)
    }

    fn compare(&self, name: &str, e: Vec<String>, got: Vec<String>) -> u32 {
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
    QUERY_ITEMS.run(&QUERY_ITEM_EXPECTED, move |p| items::query_item(p, item, ctx))
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
    OBJECT_ITEMS.run(&OBJECT_ITEM_EXPECTED, move |p| items::object_item(p, item, ctx))
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
    if crate::nvn::api::capturing_to_compare() {
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
    if crate::nvn::api::capturing_to_compare() {
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
    if crate::nvn::api::capturing_to_compare() {
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
    if crate::nvn::api::capturing_to_compare() {
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
    if let Some(l) = COPY_QUADS.summary("copy quads") {
        println!("{l}");
    }
    if let Some(l) = SNAPSHOTS.summary("snapshots") {
        println!("{l}");
    }
    if let Some(l) = ATMOSPHERES.summary("atmosphere passes") {
        println!("{l}");
    }
    if let Some(l) = SCENE_PASSES.summary("scene buffer passes") {
        println!("{l}");
    }
    if let Some(l) = VIEW_COPIES.summary("view copies") {
        println!("{l}");
    }
    if let Some(l) = REPROJECTIONS.summary("reprojection passes") {
        println!("{l}");
    }
    if let Some(l) = VOLUME_MASKS.summary("volume masks") {
        println!("{l}");
    }
    if let Some(l) = POST_PASSES.summary("post passes") {
        println!("{l}");
    }
    if let Some(l) = LIGHT_PASSES.summary("light passes") {
        println!("{l}");
    }
    if let Some(l) = VIEW_TARGET_DRAWS.summary("view target draws") {
        println!("{l}");
    }
    if let Some(l) = DOF_COMPOSITES.summary("depth of field composites") {
        println!("{l}");
    }
    if let Some(l) = EDGE_ADJUSTS.summary("edge adjusts") {
        println!("{l}");
    }
    if let Some(l) = VIEW_GATHERS.summary("view gathers") {
        println!("{l}");
    }
    if let Some(l) = VIEW_TARGET_PASSES.summary("view target passes") {
        println!("{l}");
    }
    if let Some(l) = OCCLUSION_BUFFERS.summary("occlusion buffers") {
        println!("{l}");
    }
    if let Some(l) = NOISE_GAUGES.summary("noise gauges") {
        println!("{l}");
    }
    if let Some(l) = TARGET_BLITS.summary("target blits") {
        println!("{l}");
    }
    if let Some(l) = VOLUME_MASK_CHAINS.summary("volume mask chains") {
        println!("{l}");
    }
    if let Some(l) = CORRECT_DEPTHS.summary("correct depth passes") {
        println!("{l}");
    }
    if let Some(l) = DEFERRED_VIEW_PASSES.summary("deferred view passes") {
        println!("{l}");
    }
    if let Some(l) = LIGHTING.summary("lighting") {
        println!("{l}");
    }
    if let Some(l) = DOF_BLUR_CHAINS.summary("dof blur chains") {
        println!("{l}");
    }
    if let Some(l) = POST_CHAINS.summary("post chains") {
        println!("{l}");
    }
    if let Some(l) = MASK_POINTS.summary("mask points") {
        println!("{l}");
    }
    if let Some(l) = STATE_BLITS.summary("state blits") {
        println!("{l}");
    }
    if let Some(l) = ENTRY_VOLUMES.summary("entry volumes") {
        println!("{l}");
    }
    if let Some(l) = EFFECT_LAYERS.summary("effect layers") {
        println!("{l}");
    }
    if let Some(l) = VIEW_GRIDS.summary("view grids") {
        println!("{l}");
    }
    if let Some(l) = VIEW_FULLSCREENS.summary("view full-screen passes") {
        println!("{l}");
    }
    if let Some(l) = VIEW_COMPOSITES.summary("view composites") {
        println!("{l}");
    }
    if let Some(l) = CAPTURE_QUADS.summary("capture quads") {
        println!("{l}");
    }
    if let Some(l) = TEXTURE_QUADS.summary("texture quads") {
        println!("{l}");
    }
    if let Some(l) = DEPTH_BUFFER_ENDS.summary("shadow depth buffer ends") {
        println!("{l}");
    }
    if let Some(l) = TEXTURED_DRAWS.summary("textured draws") {
        println!("{l}");
    }
    if let Some(l) = TINTED_DRAWS.summary("tinted draws") {
        println!("{l}");
    }
    if let Some(l) = CLEARING_BINDS.summary("clearing binds") {
        println!("{l}");
    }
    if let Some(l) = DISPLAY_LISTS.summary("display lists") {
        println!("{l}");
    }
    if let Some(l) = PING_PONGS.summary("ping-pong passes") {
        println!("{l}");
    }
    if let Some(l) = LIGHT_SHAPES.summary("light shapes") {
        println!("{l}");
    }
    if let Some(l) = TRANSFORMED_QUADS.summary("transformed quads") {
        println!("{l}");
    }
    if let Some(l) = CUBEMAP_FACES.summary("cube map faces") {
        println!("{l}");
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
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_cached_model_pass(_, _, _, _, _, _))) as usize;
        hook(base, mem_space, 0x9314b4, 0xd102c3ff, pre, post, "Cached model pass");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_model_pass(_, _, _, _, _, _, _, _))) as usize;
        hook(base, mem_space, 0x931960, 0xa9ba7bfd, pre, post, "Model pass");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_block_bind(_, _, _, _))) as usize;
        hook(base, mem_space, 0x74efd4, 0xa9ba7bfd, pre, post, "Block bind");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_target_clear(_, _, _, _, _, _, _, _))) as usize;
        hook_float(base, mem_space, 0x98a150, 0xfc190fe8, pre, post, "Target clear");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_target_barriers(_, _))) as usize;
        hook(base, mem_space, 0xc26458, 0xa9be7bfd, pre, post, "Target barriers");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_shader_apply(_, _, _))) as usize;
        hook(base, mem_space, 0xc56c70, 0xa9bc7bfd, pre, post, "Shader apply");
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
        gfx!(0x68b364, crate::hle!(on_gfx_draw_indexed_instanced(_, _, _, _, _, _, _, _)), "nn::gfx DrawIndexed instanced");
        gfx!(0x300d8, crate::hle!(on_gfx_flush_memory(_, _)), "nn::gfx FlushMemory");
        gfx!(0x74fd08, crate::hle!(on_slots_deferred(_, _, _, _)), "Deferred texture slots");
        gfx!(0x30100, crate::hle!(on_gfx_invalidate_memory(_, _)), "nn::gfx InvalidateMemory");
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
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_cubemap_face(_, _, _, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_cubemap_face_end())) as usize;
        hook_float(base, mem_space, 0x7ee7a8, 0xd10203ff, pre, post, "Cube map faces");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_transformed_quad(_, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_transformed_quad_end())) as usize;
        hook(base, mem_space, 0x877418, 0xd10303ff, pre, post, "Transformed quads");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_copy_quad(_, _, _, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_copy_quad_end())) as usize;
        hook_float(base, mem_space, 0x876e28, 0xd10203ff, pre, post, "Copy quads");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_snapshot(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_snapshot_end())) as usize;
        hook(base, mem_space, 0xcc75ec, 0xd10403ff, pre, post, "Snapshots");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_light_shape(_, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_light_shape_end())) as usize;
        hook(base, mem_space, 0x969d28, 0xd10683ff, pre, post, "Light shapes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_ping_pong(_, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_ping_pong_end())) as usize;
        hook(base, mem_space, 0xc24c30, 0xd10283ff, pre, post, "Ping-pong passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_display_lists(_, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_display_lists_end())) as usize;
        hook(base, mem_space, 0xbccb6c, 0xa9ba7bfd, pre, post, "Display lists");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_clearing_bind(_, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_clearing_bind_end())) as usize;
        hook(base, mem_space, 0x8660b4, 0xd104c3ff, pre, post, "Clearing binds");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_textured_draw(_, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_textured_draw_end())) as usize;
        hook(base, mem_space, 0xc57014, 0xa9bc7bfd, pre, post, "Textured draws");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_tinted_draw(_, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_tinted_draw_end())) as usize;
        hook(base, mem_space, 0xc55378, 0xd10343ff, pre, post, "Tinted draws");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_end_depth_buffer(_, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_end_depth_buffer_end())) as usize;
        hook(base, mem_space, 0xa404ac, 0xa9bd7bfd, pre, post, "Shadow depth buffer ends");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_texture_quad(_, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_texture_quad_end())) as usize;
        hook(base, mem_space, 0xbe1b2c, 0xd10643ff, pre, post, "Texture quads");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_capture_quad(_, _, _, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_capture_quad_end())) as usize;
        hook_float(base, mem_space, 0x98aa34, 0xd10203ff, pre, post, "Capture quads");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_view_composite(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_view_composite_end())) as usize;
        hook(base, mem_space, 0x955ff4, 0xd10383ff, pre, post, "View composites");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_view_grid(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_view_grid_end())) as usize;
        hook(base, mem_space, 0x957448, 0xd10383ff, pre, post, "View grids");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_view_fullscreen(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_view_fullscreen_end())) as usize;
        hook(base, mem_space, 0x9737f8, 0xd10403ff, pre, post, "View full-screen passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_effect_layer(_, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_effect_layer_end())) as usize;
        hook(base, mem_space, 0x95f810, 0xd10483ff, pre, post, "Effect layers");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_atmosphere(_, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_atmosphere_end())) as usize;
        hook(base, mem_space, 0x21d95d4, 0xd10543ff, pre, post, "Atmosphere passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_scene_pass(_, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_scene_pass_end())) as usize;
        hook(base, mem_space, 0x9618d0, 0xa9ba7bfd, pre, post, "Scene buffer passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_entry_volumes(_, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_entry_volumes_end())) as usize;
        hook(base, mem_space, 0x95bef4, 0xa9ba7bfd, pre, post, "Entry volumes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_view_copy(_, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_view_copy_end())) as usize;
        hook(base, mem_space, 0x959a14, 0xa9ba7bfd, pre, post, "View copies");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_reprojection(_, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_reprojection_end())) as usize;
        hook(base, mem_space, 0x9635b8, 0xa9ba7bfd, pre, post, "Reprojection passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_volume_mask(_, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_volume_mask_end())) as usize;
        hook(base, mem_space, 0x95ad64, 0xa9ba7bfd, pre, post, "Volume masks");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_post_pass(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_post_pass_end())) as usize;
        hook(base, mem_space, 0xc555c4, 0xfc180fea, pre, post, "Post passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_light_pass(_, _, _, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_light_pass_end())) as usize;
        hook(base, mem_space, 0x966f7c, 0x6db82beb, pre, post, "Light passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_view_target_draw(_, _, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_view_target_draw_end())) as usize;
        hook(base, mem_space, 0x957820, 0xa9ba7bfd, pre, post, "View target draws");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_dof_composite(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_dof_composite_end())) as usize;
        hook(base, mem_space, 0xa3e79c, 0xa9ba7bfd, pre, post, "Depth of field composites");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_edge_adjust(_, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_edge_adjust_end())) as usize;
        hook(base, mem_space, 0xa434a8, 0xa9ba7bfd, pre, post, "Edge adjusts");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_view_gather(_, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_view_gather_end())) as usize;
        hook(base, mem_space, 0xd4cac4, 0xd10443ff, pre, post, "View gathers");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_view_target_passes(_, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_view_target_passes_end())) as usize;
        hook(base, mem_space, 0xbd8c40, 0xd10443ff, pre, post, "View target passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_occlusion_buffers(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_occlusion_buffers_end())) as usize;
        hook(base, mem_space, 0xbdad04, 0xd10743ff, pre, post, "Occlusion buffers");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_noise_gauge(_, _, _, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_noise_gauge_end())) as usize;
        hook_float(base, mem_space, 0xdb6438, 0xd104c3ff, pre, post, "Noise gauges");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_target_blit(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_target_blit_end())) as usize;
        hook(base, mem_space, 0xe01cb8, 0xa9ba7bfd, pre, post, "Target blits");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_volume_mask_chain(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_volume_mask_chain_end())) as usize;
        hook(base, mem_space, 0xa4c1ec, 0x6db923e9, pre, post, "Volume mask chains");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_correct_depth(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_correct_depth_end())) as usize;
        hook(base, mem_space, 0x9590f8, 0xa9ba7bfd, pre, post, "Correct depth passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_deferred_view_pass(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_deferred_view_pass_end())) as usize;
        hook(base, mem_space, 0x96e6fc, 0xfc180fea, pre, post, "Deferred view passes");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_lighting(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_lighting_end())) as usize;
        hook(base, mem_space, 0x973b40, 0xfc180fea, pre, post, "Lighting");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_dof_blur_chain(_, _, _, _, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_dof_blur_chain_end())) as usize;
        hook(base, mem_space, 0x96b618, 0x6db923e9, pre, post, "Dof blur chains");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_post_chain(_, _, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_post_chain_end())) as usize;
        hook(base, mem_space, 0xc4f7c8, 0x6db733ed, pre, post, "Post chains");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_mask_points(_, _, _, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_mask_points_end())) as usize;
        hook(base, mem_space, 0xbde1b8, 0xa9ba7bfd, pre, post, "Mask points");
        let pre = mem_space.make_hle_trampoline(crate::hle!(on_state_blit(_, _))) as usize;
        let post = mem_space.make_hle_trampoline(crate::hle!(on_state_blit_end())) as usize;
        hook(base, mem_space, 0xbde7f0, 0xd10503ff, pre, post, "State blits");
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
/// The first instruction of a hooked function as it runs from `to`: an adrp is re-encoded for the page it named.
fn relocate(instruction: u32, from: usize, to: usize) -> Option<u32> {
    if instruction & 0x9f00_0000 != 0x9000_0000 {
        return Some(instruction);
    }
    let imm = (((instruction >> 5) & 0x7ffff) << 2 | (instruction >> 29) & 3) as i64;
    let imm = (imm << 43) >> 43;
    let page = ((from & !0xfff) as i64).wrapping_add(imm << 12);
    let delta = (page - (to & !0xfff) as i64) >> 12;
    if delta.abs() >= 1 << 20 {
        return None;
    }
    let d = delta as u32;
    Some((instruction & 0x9f00_001f) | (d & 3) << 29 | ((d >> 2) & 0x7ffff) << 5)
}

/// hook() for functions with floating-point arguments: d0..d7 are kept for the function and `pre` gets a pointer to
/// them in place of x7 (its own x7 argument).
unsafe fn hook_float(
    base: usize,
    mem_space: &mut crate::guest::loader::GuestMemorySpace,
    site_off: usize,
    instruction: u32,
    pre: usize,
    post: usize,
    what: &str,
) {
    if crate::settings::get().totk.diagnostics.no_port_hook.iter().any(|h| h.trim_start_matches("0x") == format!("{site_off:x}")) {
        println!("[+] {what} hook left out at main+{site_off:#x}");
        return;
    }
    let site = (base + site_off) as *mut u32;
    let actual = std::ptr::read(site);
    if actual != instruction {
        println!("[!] {what} hook not installed: main+{site_off:#x} = {actual:#010x}");
        return;
    }
    let wrap = mem_space.make_code_block(31) as usize;
    let b = |from: usize, to: usize, link: bool| -> Option<u32> {
        let d = to as i64 - from as i64;
        (d.abs() < 1 << 27).then(|| (if link { 0x9400_0000 } else { 0x1400_0000 }) | ((d >> 2) as u32 & 0x03ff_ffff))
    };
    let at = |k: usize| wrap + k * 4;
    let (Some(to_wrap), Some(call_pre), Some(call_body), Some(call_post), Some(back), Some(displaced)) = (
        b(site as usize, wrap, false),
        b(at(11), pre, true),
        b(at(21), at(29), true),
        b(at(23), post, true),
        b(at(30), site as usize + 4, false),
        relocate(instruction, site as usize, at(29)),
    ) else {
        println!("[!] {what} hook not installed: wrapper out of range");
        return;
    };
    let code: [u32; 31] = [
        0xa9b67bfd,  // 0: stp x29, x30, [sp, #-0xa0]!
        0x910003fd,  // 1: mov x29, sp
        0xa90107e0,  // 2: stp x0, x1, [sp, #0x10]
        0xa9020fe2,  // 3: stp x2, x3, [sp, #0x20]
        0xa90317e4,  // 4: stp x4, x5, [sp, #0x30]
        0xa9041fe6,  // 5: stp x6, x7, [sp, #0x40]
        0x6d0507e0,  // 6: stp d0, d1, [sp, #0x50]
        0x6d060fe2,  // 7: stp d2, d3, [sp, #0x60]
        0x6d0717e4,  // 8: stp d4, d5, [sp, #0x70]
        0x6d081fe6,  // 9: stp d6, d7, [sp, #0x80]
        0x910143e7,  // 10: add x7, sp, #0x50
        call_pre,    // 11: bl pre
        0x350001e0,  // 12: cbnz w0, 27 (handled)
        0x6d481fe6,  // 13: ldp d6, d7, [sp, #0x80]
        0x6d4717e4,  // 14: ldp d4, d5, [sp, #0x70]
        0x6d460fe2,  // 15: ldp d2, d3, [sp, #0x60]
        0x6d4507e0,  // 16: ldp d0, d1, [sp, #0x50]
        0xa9441fe6,  // 17: ldp x6, x7, [sp, #0x40]
        0xa94317e4,  // 18: ldp x4, x5, [sp, #0x30]
        0xa9420fe2,  // 19: ldp x2, x3, [sp, #0x20]
        0xa94107e0,  // 20: ldp x0, x1, [sp, #0x10]
        call_body,   // 21: bl 29 (the function)
        0xf9000be0,  // 22: str x0, [sp, #0x10]
        call_post,   // 23: bl post
        0xf9400be0,  // 24: ldr x0, [sp, #0x10]
        0xa8ca7bfd,  // 25: ldp x29, x30, [sp], #0xa0
        0xd65f03c0,  // 26: ret
        0xa8ca7bfd,  // 27: ldp x29, x30, [sp], #0xa0
        0xd65f03c0,  // 28: ret
        displaced,   // 29: the displaced instruction
        back,        // 30: b site+4
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

unsafe fn hook(
    base: usize,
    mem_space: &mut crate::guest::loader::GuestMemorySpace,
    site_off: usize,
    instruction: u32,
    pre: usize,
    post: usize,
    what: &str,
) {
    if crate::settings::get().totk.diagnostics.no_port_hook.iter().any(|h| h.trim_start_matches("0x") == format!("{site_off:x}")) {
        println!("[+] {what} hook left out at main+{site_off:#x}");
        return;
    }
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
    // The displaced instruction runs at at(20).
    let Some(displaced) = relocate(instruction, site as usize, at(20)) else {
        println!("[!] {what} hook not installed: adrp out of range");
        return;
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
