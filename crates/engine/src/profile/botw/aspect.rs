// Screen shapes other than 16:9 in Breath of the Wild (1.9.0).
//
// The game's views are sead projections, each made into a matrix by sead::Projection::updateMatrixImpl when its dirty
// flag (+8) is set; every getter of the matrix, of the device matrix and of a point's projection goes through it. With
// an aspect set (`set_aspect`: the launch shape, then the window's as it is resized), the hook there gives every 16:9
// view the screen's shape before its matrix is made, in the projection itself, so the game's own uses of it (the
// shadow cascades are fitted to the world camera's view, a screen point turned back into a camera ray) see the view
// that is drawn:
// - perspective projections take the screen's aspect; narrower than 16:9 tan(fovy / 2) is scaled by 16:9 / aspect,
//   and fovy, sin and cos follow it, so the horizontal view is kept and more is seen vertically (Vert+, as TotK). Their
//   offset (a view moved by a fraction of its width, the inventory's Link drawn to the right) is scaled to stay the
//   same distance in the 16:9 part.
// - frustum projections (some of the HUD's layers) are widened about their centre (narrower: made taller).
// The picture keeps its size and is stretched to the screen (config::display_aspect), so the world shows more to the
// sides, and the HUD's layers, 16:9 views too, keep the HUD's 16:9 layout in the middle.
// What the game set is kept per projection along with what was written, so a projection is not widened twice, the next
// change of shape starts from the game's values, and a value the game sets again is taken as new. set_aspect marks the
// projections it changed dirty, so those the game no longer updates follow too.
//
// Found by Savage Wind (MIT; tools/patches.py, runtime/nvn/output_scale.c: the projections' update, matrix and offset,
// and the world camera's widened values written back for its shadows, #103), and checked here against the code.
//
// The hook rewrites instructions in place and calls an HLE function, so it applies natively and to the image that is
// lifted. It is installed whenever the game is BotW 1.9.0 and keeps the game's own behaviour until an aspect is set.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use crate::profile::readable;

/// Where one version's code is (offsets in main).
pub struct Sites {
    /// sead::Projection::updateMatrixImpl's check of the dirty flag: `ldrb w8, [x0, #8]; mov x19, x0; cbz w8, <clean>`.
    pub update: usize,
    /// sead::PerspectiveProjection's vtable, and its matrix function (the vtable's entry +0x58).
    pub perspective: (usize, usize),
    /// sead::FrustumProjection's vtable and matrix function.
    pub frustum: (usize, usize),
}

const SIXTEEN_NINTHS: f32 = 16.0 / 9.0;
/// The narrowest shape (9:21).
const NARROWEST: f32 = 9.0 / 21.0;

// sead::Projection: the matrix's dirty flag +8. PerspectiveProjection: fovy +0xa0, sin / cos / tan of fovy / 2
// +0xa4 / +0xa8 / +0xac, aspect +0xb0, offset +0xb4 / +0xb8 (fractions of the view's width and height).
// FrustumProjection: top +0xa0, bottom +0xa4, left +0xa8, right +0xac.
const PROJ_DIRTY: usize = 8;
const PROJ_FIELDS: usize = 0xa0;
const PERSPECTIVE_FIELDS: usize = 7;
const FRUSTUM_FIELDS: usize = 4;

/// The aspect the game renders at (f32 bits); 0 until one is set: the hook then keeps the game's own views.
static ASPECT: AtomicU32 = AtomicU32::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// The two vtables (absolute), once installed.
static PERSPECTIVE_VT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static FRUSTUM_VT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A projection the hook changed: its vtable, the values the game set and those written over them.
struct Changed {
    vtable: usize,
    set: [u32; PERSPECTIVE_FIELDS],
    wrote: [u32; PERSPECTIVE_FIELDS],
}

static CHANGED: Mutex<Option<HashMap<usize, Changed>>> = Mutex::new(None);
/// Whether CHANGED holds any projection (the hook's quick way out while the game keeps its own shape).
static ANY_CHANGED: AtomicBool = AtomicBool::new(false);

/// The aspect the game renders at, once one is set.
pub fn rendered_aspect() -> Option<f64> {
    let bits = ASPECT.load(Ordering::Relaxed);
    (bits != 0).then(|| f32::from_bits(bits) as f64)
}

/// Changes the aspect the game renders at: the projections it changed are marked dirty and take it at their next
/// update. No-op if not installed.
pub fn set_aspect(aspect: f64) {
    if !INSTALLED.load(Ordering::Relaxed) {
        return;
    }
    let a = (aspect as f32).max(NARROWEST);
    // The game's own shape: no aspect, so that every view gets the game's values back.
    let bits = if a == SIXTEEN_NINTHS { 0 } else { a.to_bits() };
    if ASPECT.swap(bits, Ordering::Relaxed) == bits {
        return;
    }
    let mut changed = CHANGED.lock().unwrap();
    if let Some(map) = changed.as_mut() {
        map.retain(|&p, c| unsafe {
            let live = readable(p, PROJ_FIELDS + 4 * PERSPECTIVE_FIELDS) && *(p as *const usize) == c.vtable;
            if live {
                *((p + PROJ_DIRTY) as *mut u8) = 1;
            }
            live
        });
    }
}

/// The shape of the game's 16:9 views as the game computes it: 16 / 9 in float.
fn is_sixteen_ninths(v: f32) -> bool {
    v == SIXTEEN_NINTHS
}

/// What a projection of `n` fields with the game's values `game` becomes at the screen's aspect `a`, or None if it keeps
/// them (not 16:9, or the screen is 16:9).
fn widened(perspective: bool, game: &[u32; PERSPECTIVE_FIELDS], a: f32) -> Option<[u32; PERSPECTIVE_FIELDS]> {
    let f = |k: usize| f32::from_bits(game[k]);
    let mut out = *game;
    if perspective {
        let own = f(4);
        if !is_sixteen_ninths(own) || a == own {
            return None;
        }
        let s = a / own;
        out[4] = a.to_bits();
        if s > 1.0 {
            out[5] = (f(5) / s).to_bits();
        } else {
            // Vert+: the horizontal view kept.
            let tan = f(3) / s;
            let cos = 1.0 / (1.0 + tan * tan).sqrt();
            out[0] = (2.0 * tan.atan()).to_bits();
            out[1] = (tan * cos).to_bits();
            out[2] = cos.to_bits();
            out[3] = tan.to_bits();
            out[6] = (f(6) * s).to_bits();
        }
    } else {
        let (top, bottom, left, right) = (f(0), f(1), f(2), f(3));
        if top == bottom || !is_sixteen_ninths((right - left) / (top - bottom)) {
            return None;
        }
        let s = a / SIXTEEN_NINTHS;
        if s > 1.0 {
            let c = (left + right) / 2.0;
            out[2] = (c + (left - c) * s).to_bits();
            out[3] = (c + (right - c) * s).to_bits();
        } else {
            let c = (top + bottom) / 2.0;
            out[0] = (c + (top - c) / s).to_bits();
            out[1] = (c + (bottom - c) / s).to_bits();
        }
    }
    Some(out)
}

/// sead::Projection::updateMatrixImpl's check of the dirty flag (x0 = the projection): a dirty 16:9 view takes the
/// screen's shape before its matrix is made. Returns the flag.
pub unsafe extern "C" fn projection_update(p: usize) -> u64 {
    let dirty = *((p + PROJ_DIRTY) as *const u8);
    let a = ASPECT.load(Ordering::Relaxed);
    if dirty == 0 || (a == 0 && !ANY_CHANGED.load(Ordering::Relaxed)) {
        return dirty as u64;
    }
    let vt = *(p as *const usize);
    let n = if vt == PERSPECTIVE_VT.load(Ordering::Relaxed) {
        PERSPECTIVE_FIELDS
    } else if vt == FRUSTUM_VT.load(Ordering::Relaxed) {
        FRUSTUM_FIELDS
    } else {
        return dirty as u64;
    };
    let fields = (p + PROJ_FIELDS) as *mut u32;
    let mut cur = [0u32; PERSPECTIVE_FIELDS];
    for (k, c) in cur.iter_mut().enumerate().take(n) {
        *c = *fields.add(k);
    }
    let mut changed = CHANGED.lock().unwrap();
    let map = changed.get_or_insert_with(HashMap::new);
    // The game's values: those kept when the projection still holds what was written, else its own.
    let game = match map.get(&p) {
        Some(c) if c.vtable == vt && c.wrote == cur => c.set,
        _ => cur,
    };
    let out = match a {
        0 => None,
        a => widened(n == PERSPECTIVE_FIELDS, &game, f32::from_bits(a)),
    };
    let write = out.unwrap_or(game);
    if write != cur {
        for (k, w) in write.iter().enumerate().take(n) {
            *fields.add(k) = *w;
        }
    }
    match out {
        Some(wrote) => {
            map.insert(p, Changed { vtable: vt, set: game, wrote });
        }
        None => {
            map.remove(&p);
        }
    }
    ANY_CHANGED.store(!map.is_empty(), Ordering::Relaxed);
    dirty as u64
}

/// The update's check as the game has it: `ldrb w8, [x0, #8]; mov x19, x0; cbz w8, +0x4c`.
const UPDATE_CODE: [u32; 3] = [0x3940_2008, 0xaa00_03f3, 0x3400_0268];

/// Installs the hook and, with the dynamic aspect ratio setting on, starts at the launch aspect (--launch-aspect, else
/// 16:9). With it off the game keeps its own shape until set_aspect is called.
#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, s: &Sites) {
    let vt_ok = |(vt, matrix): (usize, usize)| *((base + vt + 0x58) as *const usize) == base + matrix;
    if !vt_ok(s.perspective) || !vt_ok(s.frustum) {
        return println!("[!] Aspect: unexpected projection vtables; the picture stays 16:9");
    }
    // mov x19, x0; bl <projection_update> (x0 = the flag); cbz w0, +0x4c.
    let code = vec![UPDATE_CODE[1], 0x9400_0000, UPDATE_CODE[2] & !31];
    let hle = crate::hle!(projection_update(_));
    if let Err(e) = crate::profile::totk::frame_time::replace_with_call(base, mem_space, s.update, &UPDATE_CODE, code, 1, hle) {
        return println!("[!] Aspect: main+{:#x}: {e}; the picture stays 16:9", s.update);
    }
    PERSPECTIVE_VT.store(base + s.perspective.0, Ordering::Relaxed);
    FRUSTUM_VT.store(base + s.frustum.0, Ordering::Relaxed);
    INSTALLED.store(true, Ordering::Relaxed);
    let settings = crate::settings::get();
    if settings.totk.enhancements.dynamic_aspect_ratio {
        let a = settings.display.launch_aspect;
        set_aspect(a.map_or(16.0 / 9.0, |a| a.ratio()));
        match a {
            Some(a) => println!("[+] Aspect: the game renders at the screen's shape, starting at {a}"),
            None => println!("[+] Aspect: the game renders at the screen's shape"),
        }
    } else {
        println!("[+] Aspect: hook installed; the game keeps its 16:9 shape (the dynamic aspect ratio setting is off)");
    }
}
