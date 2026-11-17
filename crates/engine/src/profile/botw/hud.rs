// HUD layouts at screen shapes other than 16:9 (Breath of the Wild 1.9.0).
//
// The layouts are drawn through 16:9 perspective layers, which aspect.rs widens with every other 16:9 view, so each
// layout keeps its 16:9 layout in the middle of the screen (narrower than 16:9, in a 16:9 band across it). Two things
// then need the screen's shape:
// - Panes drawn over the whole screen (FILL: fades, the loading screen's background, the menus' backgrounds) should
//   fill it: their scale is multiplied by F = screen aspect / 16:9 along x (narrower: by 16:9 / aspect along y). The
//   layout library calculates each layout once per frame (Layout::Calculate, main+0xf62598, and the same code in the
//   game's layout class, main+0x25c64): right after the draw info takes the layout, hooks there call `calculate`,
//   which writes the panes' scale and marks them for recalculation. A value the game or an animation wrote since
//   becomes the new base.
// - Panes that clip what they hold (the minimap) turn their rectangle into a scissor of the 16:9 canvas
//   (main+0xf66f0c): that rectangle is moved to where the layout is shown (scissor_pane), or the clip stayed at the 16:9
//   place while the minimap moved inward.
// - Panes the game keeps just past the 16:9 screen's right edge (OUTSIDE: the pause menu's strips that show its pages
//   sliding in, captured from beyond the screen) come into view on a wider screen, as dark bands: they are hidden
//   while the screen is wider than 16:9.
// The panes are the layouts' own (names, sizes and places as the game's layout files have them: each FILL pane is as
// large as the 1280x720 layout, each OUTSIDE pane lies wholly right of x = 640).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::profile::ui2d::{c_str, find_pane, Field};

/// Where one version's code is (offsets in main).
pub struct Sites {
    /// The layout calculations, after their draw info took the layout: `str x21, [x19, #0xc0]` and the call of the root
    /// pane's calculation, as the version has it (layout_code_v1_9 / layout_code_v1_5).
    pub layout: [(usize, [u32; 8]); 2],
    /// The clipping pane's draw: its `bl` of the scissor setter (SCISSOR_CODE before it), and the setter.
    pub scissor: (usize, usize),
}

// Layout fields: its root pane, its name.
const LAYOUT_ROOT: usize = 0x18;
const LAYOUT_NAME: usize = 0x30;
// Pane fields: scale x, y; flags (0x10: its matrix to recalculate); name.
const PANE_SCALE_X: usize = 0x48;
const PANE_SCALE_Y: usize = 0x4c;
const PANE_FLAGS: usize = 0x58;
const PANE_NAME: usize = 0xb8;
const FLAG_MTX_DIRTY: u8 = 0x10;
// The draw info: the layout being calculated.
const DRAW_INFO_LAYOUT: usize = 0xc0;

/// (layout, pane): panes drawn over the whole screen. The pause menu's and the quick menus' backgrounds (their root:
/// every pane in them is the screen's size: the blurred capture of the game, its tint), the loading screen's and the
/// fades' black (Fade_00) with the band of the previous picture across its top, and the start screen's black.
const FILL: [(&str, &str); 8] = [
    ("PauseMenuBG_00", "RootPane"),
    ("SeekPadMenuBG_00", "RootPane"),
    ("Fade_00", "P_Black_00"),
    ("Fade_00", "P_Black_01"),
    ("Fade_00", "P_BlackBG_00"),
    ("Fade_00", "P_Capture_00"),
    ("Title_00", "Black8_00"),
    ("Title_00", "P_BackGround_00"),
];

/// (layout, pane): panes past the 16:9 screen's right edge (the pause menu's page slide strips, x 641 to 741, and the
/// item panel's, at its height).
const OUTSIDE: [(&str, &str); 4] = [
    ("PauseMenu_00", "P_CaptureSlideR_00"),
    ("PauseMenu_00", "P_CaptureSlideR_01"),
    ("PauseMenuInfo_00", "P_CaptureSlideR_00"),
    ("PauseMenuInfo_00", "P_CaptureSlideR_01"),
];

/// A pane's visibility (flags bit 0) the hook overrides: the game's own value and what was last written.
struct Visibility {
    pane: usize,
    base: bool,
    written: Option<bool>,
}

impl Visibility {
    unsafe fn set(&mut self, hide: bool) {
        let flags = (self.pane + PANE_FLAGS) as *mut u8;
        let cur = *flags & 1 != 0;
        if self.written != Some(cur) {
            self.base = cur;
        }
        let v = self.base && !hide;
        self.written = Some(v);
        if v != cur {
            *flags = (*flags & !1) | v as u8;
        }
    }
}

/// Per layout object: its name pointer and root pane (a layout rebuilt at the same address has others), the scale of
/// each pane that fills the screen, and the panes hidden past its edge.
struct Layout {
    name_ptr: usize,
    root: usize,
    panes: Vec<(usize, Field, Field)>,
    outside: Vec<Visibility>,
}

static LAYOUTS: Mutex<Option<HashMap<usize, Layout>>> = Mutex::new(None);

/// (x, y) factors for panes that fill the screen: F along x wider than 16:9, 1 / f along y narrower; (1, 1) at 16:9.
fn fill_factors() -> (f32, f32) {
    match super::aspect::rendered_aspect() {
        Some(a) => {
            let f = (a * 9.0 / 16.0) as f32;
            if f >= 1.0 {
                (f, 1.0)
            } else {
                (1.0, 1.0 / f)
            }
        }
        None => (1.0, 1.0),
    }
}

/// Adjusts `layout`'s panes to the screen's shape (the game's own values at 16:9).
pub fn calculate(layout: usize) {
    unsafe {
        let root = *((layout + LAYOUT_ROOT) as *const usize);
        if root == 0 {
            return;
        }
        let name_ptr = *((layout + LAYOUT_NAME) as *const usize);
        let mut layouts = LAYOUTS.lock().unwrap();
        let layouts = layouts.get_or_insert_with(HashMap::new);
        if !matches!(layouts.get(&layout), Some(e) if (e.name_ptr, e.root) == (name_ptr, root)) {
            // Only layouts with panes to change are kept.
            let name = c_str(name_ptr, 64);
            let panes: Vec<_> = FILL
                .iter()
                .filter(|(l, _)| l.as_bytes() == name)
                .filter_map(|(_, p)| find_pane(root, p.as_bytes(), PANE_NAME))
                .map(|p| (p, Field::new(p + PANE_SCALE_X), Field::new(p + PANE_SCALE_Y)))
                .collect();
            let outside: Vec<_> = OUTSIDE
                .iter()
                .filter(|(l, _)| l.as_bytes() == name)
                .filter_map(|(_, p)| find_pane(root, p.as_bytes(), PANE_NAME))
                .map(|pane| Visibility { pane, base: *((pane + PANE_FLAGS) as *const u8) & 1 != 0, written: None })
                .collect();
            if panes.is_empty() && outside.is_empty() {
                layouts.remove(&layout);
                return;
            }
            layouts.insert(layout, Layout { name_ptr, root, panes, outside });
        }
        let Some(entry) = layouts.get_mut(&layout) else { return };
        let (fx, fy) = fill_factors();
        for v in &mut entry.outside {
            v.set(fx > 1.0);
        }
        for (p, x, y) in &mut entry.panes {
            if x.set(|v| v * fx) | y.set(|v| v * fy) {
                *((*p + PANE_FLAGS) as *mut u8) |= FLAG_MTX_DIRTY;
            }
        }
    }
}

/// A layout calculation, after its draw info took the layout: `calculate` first, then the root pane (x0, which the
/// game's code calls next) and the draw info (x1, passed on with it).
pub unsafe extern "C" fn layout_calculate(draw_info: usize) -> crate::guest::exec::hle_abi::U64Pair {
    let layout = *((draw_info + DRAW_INFO_LAYOUT) as *const usize);
    calculate(layout);
    crate::guest::exec::hle_abi::U64Pair(*((layout + LAYOUT_ROOT) as *const usize) as u64, draw_info as u64)
}

/// The scissor setter the clipping pane calls (absolute).
static SCISSOR_TO: AtomicUsize = AtomicUsize::new(0);

/// The clipping pane's scissor: its rectangle (x0 + 8: left, top, right, bottom on the 16:9 canvas; the canvas's own at
/// +8 of the pointer the caller keeps at x0 + 0x38) moved to where the layout is shown, about the canvas's centre: x
/// by 1 / F wider than 16:9, y by f narrower. Then the game's setter.
#[cfg(nx_runtime)]
pub unsafe extern "C" fn scissor_pane(rect: usize, a1: u64, a2: u64) {
    if let Some(a) = super::aspect::rendered_aspect() {
        let f = (a * 9.0 / 16.0) as f32;
        let canvas = *((rect + 0x38) as *const usize) + 8;
        let (c, r) = (canvas as *const f32, (rect + 8) as *mut f32);
        let (k, cx, cy) = if f >= 1.0 { (0, (*c + *c.add(2)) / 2.0, 0.0) } else { (1, 0.0, (*c.add(1) + *c.add(3)) / 2.0) };
        let (centre, s) = if k == 0 { (cx, 1.0 / f) } else { (cy, f) };
        for i in [k, k + 2] {
            *r.add(i) = centre + (*r.add(i) - centre) * s;
        }
    }
    crate::guest::call(SCISSOR_TO.load(Ordering::Relaxed), &[rect as u64, a1, a2]);
}

/// The calculation's call of its root pane's in 1.9.0: `str x21, [x19, #0xc0]; and w3, w20, #1; <x2 = the context>;
/// ldr x0, [x21, #0x18]; mov x1, x19; ldr x8, [x0]; ldr x8, [x8, #0xa0]; blr x8`, with `context` the version's
/// `mov x2, sp` / `add x2, sp, #8`.
pub const fn layout_code_v1_9(context: u32) -> [u32; 8] {
    [0xf900_6275, 0x1200_0283, context, 0xf940_0ea0, 0xaa13_03e1, 0xf940_0008, 0xf940_5108, 0xd63f_0100]
}

/// The same call in 1.5.0, which loads the root pane's function first: `str x21, [x19, #0xc0]; ldr x0, [x21, #0x18];
/// ldr x8, [x0]; ldr x8, [x8, #0xa0]; and w3, w20, #1; <x2 = the context>; mov x1, x19; blr x8`.
pub const fn layout_code_v1_5(context: u32) -> [u32; 8] {
    [0xf900_6275, 0xf940_0ea0, 0xf940_0008, 0xf940_5108, 0x1200_0283, context, 0xaa13_03e1, 0xd63f_0100]
}

/// The context word (`mov x2, sp` or `add x2, sp, #8`) of a layout site's code.
fn layout_context(code: &[u32; 8]) -> Option<u32> {
    code.iter().copied().find(|&w| w == 0x9100_03e2 || w == 0x9100_23e2)
}
/// The clipping pane's call of the scissor setter: `ldr x1, [sp, #0x48]; mov x0, sp; mov x2, x23; bl <setter>`.
const SCISSOR_CODE: [u32; 4] = [0xf940_27e1, 0x9100_03e0, 0xaa17_03e2, 0x9400_0000];

/// Installs the layout hooks (`str x21, [x19, #0xc0]; mov x0, x19; bl <layout_calculate>`: x0 = the root pane, x1 =
/// the draw info, then the rest as it was) and the clipping pane's (its `bl` to scissor_pane).
#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace, s: &Sites) {
    use crate::profile::totk::frame_time::replace_with_call;
    let mut ok = true;
    for (at, expected) in s.layout {
        let Some(context) = layout_context(&expected) else {
            println!("[!] HUD: main+{at:#x}: no context in the site's code; full-screen panes stay 16:9");
            ok = false;
            continue;
        };
        // str x21, [x19, #0xc0]; mov x0, x19; bl <layout_calculate>; and w3, w20, #1; <context>; then the call.
        let code = vec![0xf900_6275, 0xaa13_03e0, 0x9400_0000, 0x1200_0283, context, 0xf940_0008, 0xf940_5108, 0xd63f_0100];
        if let Err(e) = replace_with_call(base, mem_space, at, &expected, code, 2, crate::hle!(layout_calculate(_))) {
            println!("[!] HUD: main+{at:#x}: {e}; full-screen panes stay 16:9");
            ok = false;
        }
    }
    let (at, setter) = s.scissor;
    let mut expected = SCISSOR_CODE;
    expected[3] |= ((setter as i64 - (at + 12) as i64) >> 2) as u32 & 0x03ff_ffff;
    let mut code = expected.to_vec();
    code[3] = 0x9400_0000;
    match replace_with_call(base, mem_space, at, &expected, code, 3, crate::hle!(scissor_pane(_, _, _))) {
        Ok(()) => SCISSOR_TO.store(base + setter, Ordering::Relaxed),
        Err(e) => {
            println!("[!] HUD: main+{at:#x}: {e}; clipping panes keep their 16:9 place");
            ok = false;
        }
    }
    if ok {
        println!("[+] HUD: full-screen panes fill the screen and clipping panes follow the layout at other shapes");
    }
}
