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
// the world camera's widened values written back for its shadows, #103, and its field of view in play only with fovy,
// sine and cosine written too, #162; runtime/hle/framepace.c: the event manager's active event), and checked here
// against the code. Savage Wind takes the world camera to be a projection whose far plane is over 10000; here it is
// the game's own object instead.
//
// The field of view (set_field_of_view, settings display.field_of_view: 70 to 150% of the game's) scales the world
// camera's in play, in the same hook: its fovy and the sine, cosine and tangent of its half are written into that
// projection before the aspect is applied, so the game's copies of it (getFovy / setFovy) and the shadow cascades
// fitted to it see the view drawn. The world camera's projection is the game's own object, not a guess from its
// values: a singleton the game creates once (Sites::world_cameras: the global holding it) keeps a view per screen,
// the first at +0x28, each with its camera's handle (+0xd0) and a PerspectiveProjection at +0x10 that the game sets
// every frame from that camera's fovy (+0x74), near and far (+0x6c / +0x70) and the screen layer's viewport (1.9.0
// main+0x10eda5c, 1.0.0 main+0x10dfd2c); the first view's projection is the world's. During an event (a cutscene, a
// talk, an item fanfare: ksys::evt::Manager's active event, read as its hasActiveEvent does, 1.9.0 main+0x110df98,
// 1.0.0 main+0xc363c8) the game's own value is kept. The factor in use is chosen at each update of the projection,
// which the game makes every frame, so it switches in the frame the event starts or ends.
//
// The hook rewrites instructions in place and calls an HLE function, so it applies natively and to the image that is
// lifted. It is installed for the versions with Sites (1.0.0, 1.5.0, 1.9.0) and keeps the game's own behaviour until
// an aspect or a field of view other than 100% is set.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::profile::readable;

/// Where one version's code is (offsets in main).
pub struct Sites {
    /// Every check of a projection's dirty flag before its matrix is made (Update). 1.9.0 has one, in
    /// sead::Projection::updateMatrixImpl; 1.5.0 inlines it into the three functions that read the matrix.
    pub updates: &'static [Update],
    /// sead::PerspectiveProjection's vtable, and its matrix function (the vtable's entry +0x58).
    pub perspective: (usize, usize),
    /// sead::FrustumProjection's vtable and matrix function.
    pub frustum: (usize, usize),
    /// The global holding the singleton of the screens' views (the world camera's projection at +0x38); None: the field
    /// of view stays the game's.
    pub world_cameras: Option<usize>,
    /// The global holding ksys::evt::Manager (its active event at +0x1d2b8); None: the field of view stays the game's.
    pub event_manager: Option<usize>,
}

/// One dirty-flag check: the code the game has at `at` (the flag's `ldrb w8, [xN, #8]`, the moves around it, `cbz w8,
/// <clean>`) and what replaces it: the same moves, `bl <projection_update>` (word `call`, x0 = the projection) and
/// `cbz w0, <clean>` in the same place, so the branch's offset stays.
pub struct Update {
    pub at: usize,
    pub expected: &'static [u32],
    pub code: &'static [u32],
    pub call: usize,
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
/// The screens' views: the first view (+0x28)'s PerspectiveProjection (+0x10).
const WORLD_PROJECTION: usize = 0x38;
/// ksys::evt::Manager: the event running now (null: none).
const ACTIVE_EVENT: usize = 0x1d2b8;

/// The field of view's range, percent of the game's (Savage Wind's Video > Field of view).
pub const FIELD_OF_VIEW: std::ops::RangeInclusive<u32> = 70..=150;
/// The field of view in play, percent of the game's (100: the game's own).
static FOV: AtomicU32 = AtomicU32::new(100);
/// The two globals (absolute; 0: this version has none), once installed.
static WORLD_CAMERAS: AtomicUsize = AtomicUsize::new(0);
static EVENT_MANAGER: AtomicUsize = AtomicUsize::new(0);
/// Whether the world camera's last update was in an event (for the log).
static IN_EVENT: AtomicBool = AtomicBool::new(false);

/// The aspect the game renders at (f32 bits); 0 until one is set: the hook then keeps the game's own views.
static ASPECT: AtomicU32 = AtomicU32::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// The two vtables (absolute), once installed.
static PERSPECTIVE_VT: AtomicUsize = AtomicUsize::new(0);
static FRUSTUM_VT: AtomicUsize = AtomicUsize::new(0);

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

/// The field of view in play, percent of the game's.
pub fn field_of_view() -> u32 {
    FOV.load(Ordering::Relaxed)
}

/// Whether this version's field of view can be changed (the hook is installed and both globals are known).
pub fn has_field_of_view() -> bool {
    INSTALLED.load(Ordering::Relaxed) && WORLD_CAMERAS.load(Ordering::Relaxed) != 0 && EVENT_MANAGER.load(Ordering::Relaxed) != 0
}

/// Changes the field of view in play (percent of the game's, kept in FIELD_OF_VIEW); the world camera's projection
/// takes it at its next update.
pub fn set_field_of_view(percent: u32) {
    if !has_field_of_view() {
        return;
    }
    let percent = percent.clamp(*FIELD_OF_VIEW.start(), *FIELD_OF_VIEW.end());
    if FOV.swap(percent, Ordering::Relaxed) == percent {
        return;
    }
    if let Some(p) = unsafe { world_projection() } {
        // (also when the game does not set it again now: paused)
        unsafe { *((p + PROJ_DIRTY) as *mut u8) = 1 };
    }
}

/// The world camera's projection, once the game has made it.
unsafe fn world_projection() -> Option<usize> {
    let g = WORLD_CAMERAS.load(Ordering::Relaxed);
    if g == 0 {
        return None;
    }
    let cams = *(g as *const usize);
    let p = cams.checked_add(WORLD_PROJECTION).filter(|_| cams != 0)?;
    (readable(p, PROJ_FIELDS + 4 * PERSPECTIVE_FIELDS) && *(p as *const usize) == PERSPECTIVE_VT.load(Ordering::Relaxed)).then_some(p)
}

/// Whether `p`, a projection being updated, is the world camera's.
unsafe fn is_world_projection(p: usize) -> bool {
    let g = WORLD_CAMERAS.load(Ordering::Relaxed);
    g != 0 && {
        let cams = *(g as *const usize);
        cams != 0 && cams.wrapping_add(WORLD_PROJECTION) == p
    }
}

/// An event is running (ksys::evt::Manager::hasActiveEvent).
unsafe fn in_event() -> bool {
    let g = EVENT_MANAGER.load(Ordering::Relaxed);
    if g == 0 {
        return true;
    }
    let m = *(g as *const usize);
    m != 0 && readable(m + ACTIVE_EVENT, 8) && *((m + ACTIVE_EVENT) as *const usize) != 0
}

/// A perspective projection's values with its fovy scaled by `percent` / 100 (fovy, then the sine, cosine and tangent
/// of its half, as setFovy makes them), or None if they stay (100%, or a view that would reach 180 degrees).
fn scaled_fovy(game: &[u32; PERSPECTIVE_FIELDS], percent: u32) -> Option<[u32; PERSPECTIVE_FIELDS]> {
    if percent == 100 {
        return None;
    }
    let fovy = f32::from_bits(game[0]) * percent as f32 / 100.0;
    if !(fovy > 0.0 && fovy < std::f32::consts::PI) {
        return None;
    }
    let half = fovy / 2.0;
    let mut out = *game;
    out[0] = fovy.to_bits();
    out[1] = half.sin().to_bits();
    out[2] = half.cos().to_bits();
    out[3] = half.tan().to_bits();
    Some(out)
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
    let fov = FOV.load(Ordering::Relaxed);
    if dirty == 0 || (a == 0 && fov == 100 && !ANY_CHANGED.load(Ordering::Relaxed)) {
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
    // The field of view first (the world camera in play), then the screen's shape.
    let mut want = game;
    if n == PERSPECTIVE_FIELDS && fov != 100 && is_world_projection(p) {
        let event = in_event();
        if IN_EVENT.swap(event, Ordering::Relaxed) != event {
            println!("[+] Aspect: {}", if event { "an event: the game's field of view" } else { "in play: the field of view set" });
        }
        if !event {
            want = scaled_fovy(&game, fov).unwrap_or(game);
        }
    }
    if a != 0 {
        want = widened(n == PERSPECTIVE_FIELDS, &want, f32::from_bits(a)).unwrap_or(want);
    }
    let out = (want != game).then_some(want);
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


/// Installs the hook and, with the dynamic aspect ratio setting on, starts at the launch aspect (--launch-aspect, else
/// 16:9). With it off the game keeps its own shape until set_aspect is called.
#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, s: &Sites) {
    let vt_ok = |(vt, matrix): (usize, usize)| *((base + vt + 0x58) as *const usize) == base + matrix;
    if !vt_ok(s.perspective) || !vt_ok(s.frustum) {
        return println!("[!] Aspect: unexpected projection vtables; the picture stays 16:9");
    }
    for u in s.updates {
        let hle = crate::hle!(projection_update(_));
        if let Err(e) = crate::profile::totk::frame_time::replace_with_call(base, mem_space, u.at, u.expected, u.code.to_vec(), u.call, hle) {
            return println!("[!] Aspect: main+{:#x}: {e}; the picture stays 16:9", u.at);
        }
    }
    PERSPECTIVE_VT.store(base + s.perspective.0, Ordering::Relaxed);
    FRUSTUM_VT.store(base + s.frustum.0, Ordering::Relaxed);
    WORLD_CAMERAS.store(s.world_cameras.map_or(0, |g| base + g), Ordering::Relaxed);
    EVENT_MANAGER.store(s.event_manager.map_or(0, |g| base + g), Ordering::Relaxed);
    INSTALLED.store(true, Ordering::Relaxed);
    let settings = crate::settings::get();
    if has_field_of_view() {
        set_field_of_view(settings.display.field_of_view);
        if field_of_view() != 100 {
            println!("[+] Aspect: the world camera's field of view in play is {}% of the game's", field_of_view());
        }
    } else if settings.display.field_of_view != 100 {
        println!("[!] Aspect: no field of view setting for this version; the game's is kept");
    }
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
