// HUD layouts for screen shapes other than 16:9 (Tears of the Kingdom 1.1.2).
//
// The game draws its HUD into its 16:9 picture, which is then stretched to the screen: every layout's root pane is
// narrowed by 1 / F (F = screen aspect / 16:9) so the HUD keeps its proportions, except full-screen layouts, which
// should fill the screen. Elements at the screen edges then move outward by k * (F - 1) layout units (k = 960 keeps
// their distance to the edge), and some panes are scaled by F^e (backgrounds widened back to the full screen). The
// tables come from a 19.5:9 widescreen mod's edited layouts. The layout library calculates every layout once per frame (main+0x79d804, and
// main+0x40e24 for the game's own layout class); a hook there calls `calculate` first, which writes the panes' scale
// and position and marks them for recalculation. A value the game or an animation wrote since becomes the new base.
// Narrower than 16:9 the picture is squeezed horizontally instead: the same panes are scaled vertically by f^-e
// (f = screen aspect / 16:9 < 1), so the HUD keeps its proportions, and elements at the top and bottom edges move
// outward by k * (1 / f - 1) (SHIFTS_Y).

use std::collections::HashMap;
use std::sync::Mutex;

/// Layout::CalculateImpl entries (`sub sp, sp, #0x60`).
pub const CALC_SITES: [usize; 2] = [0x79d804, 0x40e24];
pub const CALC_ENTRY: u32 = 0xd101_83ff;

// Layout fields.
const LAYOUT_ROOT: usize = 0x18;
const LAYOUT_NAME: usize = 0x30;
// Pane fields.
const PANE_CHILDREN: usize = 0x20; // list head; a child's node is at child + 8, its next pointer at node + 8
const PANE_TRANSLATE_X: usize = 0x30;
const PANE_TRANSLATE_Y: usize = 0x34;
const PANE_SCALE_X: usize = 0x48;
const PANE_SCALE_Y: usize = 0x4c;
const PANE_FLAGS: usize = 0x58;
const PANE_NAME: usize = 0xb0;
const FLAG_MTX_DIRTY: u8 = 0x10;

/// Layouts not narrowed: those drawn across the whole screen, and the selection cursor, which frames other layouts'
/// panes where they are on screen.
const UNCHANGED: [&str; 11] = [
    "AppMenuBG_00",
    "AppMenuOverlay_00",
    "BoxCursor_00",
    "CameraShutter_00",
    "EventFadeWipe_00",
    "EventFade_00",
    "PaAppPictureBookDetail_00",
    "PauseMenuBG_00",
    "ShopMenuParasailBG_00",
    "ShortCutBG_00",
    "TitleMenuBG_00",
];

/// (layout, pane, e): the pane's x scale is multiplied by F^e.
#[rustfmt::skip]
const SCALES: [(&str, &str, i32); 61] = [
    ("AppAlbum_00", "N_Open_01", 1),
    ("AppAlbum_00", "N_FadeInOut_00", -1),
    ("AppMap_00", "N_BlurInOut_00", 1),
    ("AppMap_00", "N_BlurInOut_01", 1),
    ("AppMap_00", "N_BlurInOut_02", 1),
    ("AppMap_00", "N_BlurInOut_03", 1),
    ("AppMenuOverlay_00", "N_FadeInOut_00", -1),
    ("AppMenuOverlay_00", "N_TitleOne_01", -1),
    ("AppMenuOverlay_00", "N_FadeInOut_01", -1),
    ("AppMenuOverlay_00", "N_Icon_00", -1),
    ("AppOpenDemoWindow_00", "P_BG_00", 1),
    ("AutoBuilder_00", "N_BG_00", 1),
    ("CameraPointer_00", "N_InOutScope_00", 1),
    ("CameraPointer_00", "S_Scissor_00", 1),
    ("CameraPointer_00", "N_Type_01", -1),
    ("CameraPointer_00", "W_Shadow_01", 1),
    ("CameraPointer_00", "Pa_CameraFocus_00", 1),
    ("CameraSystemWindow_00", "N_InOut_00", 1),
    ("CameraSystemWindow_00", "T_Text_00", -1),
    ("CameraSystemWindow_00", "Pa_BtnR_00", -1),
    ("CameraSystemWindow_00", "Pa_BtnL_00", -1),
    ("CameraSystemWindow_00", "N_AlbumCon_00", -1),
    ("GameOver_00", "P_BG_00", 1),
    ("LoadingFade_00", "Pa_Map_00", -1),
    ("LoadingFade_00", "Pa_LargeDungeonMap_00", -1),
    ("LoadingFade_00", "Pa_Map_01", -1),
    ("LoadingFade_00", "Pa_LargeDungeonMap_01", -1),
    ("LoadingFade_00", "N_Base_00", 1),
    ("LoadingFade_00", "P_BG_00", 1),
    ("LoadingFade_00", "Pa_IconPlayer_00", -1),
    ("LoadingFade_00", "P_SideBGDeco_00", -1),
    ("MessageLog_01", "P_Capture_00", 1),
    ("OwnedHorseList_00", "N_BG_00", 1),
    ("PaLargeDungeonMap_00", "P_BG_00", 1),
    ("PaRecipeCardList_00", "P_BG_00", 1),
    ("PauseMenuOverlay_00", "P_FooterBase_00", 1),
    ("PauseMenuOverlay_00", "S_DecoScissor_00", 1),
    ("PauseMenuOverlay_00", "P_DecoL_00", -1),
    ("PauseMenuOverlay_00", "P_DecoR_00", -1),
    ("ShopMenu_00", "N_BG_00", 1),
    ("StaffRoll_00", "N_BG_00", 1),
    ("SystemActionGuide_00", "N_SystemBG_00", 1),
    ("SystemActionGuide_00", "N_Header_00", 1),
    ("SystemActionGuide_00", "P_DecoL_01", -1),
    ("SystemActionGuide_00", "P_DecoL_02", -1),
    ("SystemActionGuide_00", "A_Title_00", -1),
    ("SystemLoadList_00", "N_SystemBG_00", 1),
    ("SystemLoadList_00", "N_Header_00", 1),
    ("SystemLoadList_00", "P_DecoL_00", -1),
    ("SystemLoadList_00", "P_DecoR_00", -1),
    ("SystemLoadList_00", "A_Title_00", -1),
    ("SystemOption_00", "N_SystemBG_00", 1),
    ("SystemOption_00", "N_Header_00", 1),
    ("SystemOption_00", "P_DecoL_00", -1),
    ("SystemOption_00", "P_DecoR_00", -1),
    ("SystemOption_00", "A_Title_00", -1),
    ("SystemSaveLoad_00", "N_SystemBG_00", 1),
    ("SystemSaveLoad_00", "N_Header_00", 1),
    ("SystemSaveLoad_00", "P_DecoL_00", -1),
    ("SystemSaveLoad_00", "P_DecoR_00", -1),
    ("SystemSaveLoad_00", "A_Title_00", -1),
];

/// (layout, pane, k): the pane moves k * (F - 1) layout units along x, as seen on screen. Values animations write are
/// shifted the same way.
const SHIFTS: [(&str, &str, f32); 34] = [
    ("AppMap_00", "N_MiniMap_00", -221.5),
    // The minimap's clipping panes (positioned by its animation).
    ("AppMap_00", "N_Cut_00", 960.0),
    ("AppMap_00", "N_Cut_01", 960.0),
    ("AppMap_00", "N_Cut_02", 960.0),
    ("AppMap_00", "N_Cut_03", 960.0),
    ("EnvironmentInfo_00", "RootPane", 960.0),
    ("EventSkip_00", "RootPane", 960.0),
    ("InstantTipsAppMenu_00", "RootPane", 960.0),
    ("InstantTipsWithPict_00", "RootPane", 960.0),
    ("InstantTips_00", "RootPane", 960.0),
    ("LoadingFade_00", "N_Logo_00", 960.0),
    ("LoadingFade_00", "N_Tips_00", -960.0),
    ("LoadingFade_00", "N_Status_00", 960.0),
    ("Location_00", "N_Small_00", -960.0),
    ("NumDisplaySecond_00", "RootPane", 960.0),
    ("NumDisplay_00", "RootPane", 960.0),
    ("PauseMenuOverlay_00", "P_DecoL_00", -233.8),
    ("PauseMenuOverlay_00", "P_DecoR_00", 183.0),
    ("PickUpWin_00", "RootPane", 960.0),
    ("PlayerStatus3D_00", "N_ExtraStaminaTarget_01", -960.0),
    ("PlayerStatus_00", "N_HeartGaugeTargetLT_00", -960.0),
    ("PlayerStatus_00", "N_DevilDemoTargetLT_00", -960.0),
    ("PlayerStatus_00", "N_DevilDemoTargetC_00", -960.0),
    ("PlayerStatus_00", "N_HeartPos_00", -960.0),
    ("PlayerStatus_00", "Pa_CompanionLife_00", -960.0),
    ("SaveLoadIcon_00", "RootPane", -960.0),
    ("SystemActionGuide_00", "P_DecoL_01", -233.8),
    ("SystemActionGuide_00", "P_DecoL_02", 233.8),
    ("SystemLoadList_00", "P_DecoL_00", -233.8),
    ("SystemLoadList_00", "P_DecoR_00", 183.0),
    ("SystemOption_00", "P_DecoL_00", -233.8),
    ("SystemOption_00", "P_DecoR_00", 183.0),
    ("SystemSaveLoad_00", "P_DecoL_00", -233.8),
    ("SystemSaveLoad_00", "P_DecoR_00", 183.0),
];

/// (layout, pane, k): narrower than 16:9, the pane's y translation is moved by k * (1 / f - 1) (k = 540 keeps a pane
/// at the top its distance to the top edge, -540 at the bottom). From the layouts' pane positions (top / bottom edge
/// elements of the gameplay HUD, the inventory, loading and save screens).
#[rustfmt::skip]
const SHIFTS_Y: [(&str, &str, f32); 21] = [
    ("AppMap_00", "N_Cut_00", -540.0),
    ("AppMap_00", "N_Cut_01", -540.0),
    ("AppMap_00", "N_Cut_02", -540.0),
    ("AppMap_00", "N_Cut_03", -540.0),
    ("EnvironmentInfo_00", "RootPane", -540.0),
    ("LoadingFade_00", "N_Logo_00", -540.0),
    ("LoadingFade_00", "N_Tips_00", -540.0),
    ("Location_00", "N_Small_00", -540.0),
    ("PauseMenuOverlay_00", "N_Footer_00", -540.0),
    ("PlayerStatus3D_00", "N_DevilDemoTargetLT_00", 540.0),
    ("PlayerStatus3D_00", "N_ExtraStaminaGauge_00", 540.0),
    ("PlayerStatus3D_00", "N_ExtraStaminaTarget_01", 540.0),
    ("PlayerStatus_00", "N_HeartGaugeTargetLT_00", 540.0),
    ("PlayerStatus_00", "N_DevilDemoTargetLT_00", 540.0),
    ("PlayerStatus_00", "N_DevilDemoTargetC_00", 540.0),
    ("PlayerStatus_00", "N_HeartPos_00", 540.0),
    ("Pouch_00", "Pa_HeartGauge_00", 540.0),
    ("Pouch_00", "N_Align_01", 540.0),
    ("SystemLoadList_00", "N_Guide_01", -540.0),
    ("SystemLoadList_00", "N_Header_00", 540.0),
    ("SystemSaveLoad_00", "N_Header_00", 540.0),
];

/// Capture panes hidden at shapes other than 16:9 (workaround, --hide-hud-captures=false keeps them): they copy a
/// screen region sized from their own size, without the scale the narrowing gives their layout, so they show the
/// wrong part of the screen (the load screen's heading twice).
const HIDDEN: [(&str, &str); 1] = [("SystemLoadList_00", "C_CaptureUp_00")];

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

/// A float the hook writes: the game's own value and what was last written over it.
struct Field {
    at: usize,
    base: f32,
    written: f32,
}

impl Field {
    unsafe fn new(at: usize) -> Field {
        let v = *(at as *const f32);
        Field {
            at,
            base: v,
            written: f32::NAN,
        }
    }

    /// Writes f(base); returns whether the value changed.
    unsafe fn set(&mut self, f: impl Fn(f32) -> f32) -> bool {
        let cur = *(self.at as *const f32);
        if cur.to_bits() != self.written.to_bits() {
            self.base = cur;
        }
        let v = f(self.base);
        self.written = v;
        if v.to_bits() == cur.to_bits() {
            return false;
        }
        *(self.at as *mut f32) = v;
        true
    }
}

struct Pane {
    pane: usize,
    scale_x: Option<(Field, i32)>,
    /// Narrower than 16:9: the y scale, by f^-e (the HUD keeps its proportions in a 16:9 band across the screen).
    scale_y: Option<Field>,
    translate_x: Option<(Field, f32)>,
    translate_y: Option<(Field, f32)>,
}

/// Per layout object: its name pointer and root pane (a layout rebuilt at the same address has others) and the panes
/// it adjusts.
struct Layout {
    name_ptr: usize,
    root: usize,
    panes: Vec<Pane>,
    hidden: Vec<Visibility>,
}

static LAYOUTS: Mutex<Option<HashMap<usize, Layout>>> = Mutex::new(None);

unsafe fn c_str<'a>(p: usize, max: usize) -> &'a [u8] {
    if p == 0 {
        return &[];
    }
    let s = std::slice::from_raw_parts(p as *const u8, max);
    &s[..s.iter().position(|&c| c == 0).unwrap_or(max)]
}

unsafe fn find_pane(pane: usize, name: &[u8]) -> Option<usize> {
    if c_str(pane + PANE_NAME, 24) == name {
        return Some(pane);
    }
    let head = pane + PANE_CHILDREN;
    let mut node = *((head + 8) as *const usize);
    while node != head && node != 0 {
        let child = node - 8;
        if let Some(p) = find_pane(child, name) {
            return Some(p);
        }
        node = *((node + 8) as *const usize);
    }
    None
}

unsafe fn setup(name: &[u8], root: usize) -> Vec<Pane> {
    let name = std::str::from_utf8(name).unwrap_or("");
    let mut panes = Vec::new();
    if !UNCHANGED.contains(&name) {
        panes.push(Pane {
            pane: root,
            scale_x: Some((Field::new(root + PANE_SCALE_X), -1)),
            scale_y: Some(Field::new(root + PANE_SCALE_Y)),
            translate_x: None,
            translate_y: None,
        });
    }
    for &(_, pane, e) in SCALES.iter().filter(|s| s.0 == name) {
        let Some(p) = find_pane(root, pane.as_bytes()) else {
            continue;
        };
        let field = (Field::new(p + PANE_SCALE_X), e);
        match panes.iter_mut().find(|x| x.pane == p) {
            Some(x) => {
                x.scale_x = Some(field);
                x.scale_y = Some(Field::new(p + PANE_SCALE_Y));
            }
            None => panes.push(Pane {
                pane: p,
                scale_x: Some(field),
                scale_y: Some(Field::new(p + PANE_SCALE_Y)),
                translate_x: None,
                translate_y: None,
            }),
        }
    }
    for (_, pane, k) in SHIFTS.iter().filter(|s| s.0 == name) {
        let Some(p) = find_pane(root, pane.as_bytes()) else {
            continue;
        };
        let field = (Field::new(p + PANE_TRANSLATE_X), *k);
        match panes.iter_mut().find(|e| e.pane == p) {
            Some(e) => e.translate_x = Some(field),
            None => panes.push(Pane {
                pane: p,
                scale_x: None,
                scale_y: None,
                translate_x: Some(field),
                translate_y: None,
            }),
        }
    }
    for (_, pane, k) in SHIFTS_Y.iter().filter(|s| s.0 == name) {
        let Some(p) = find_pane(root, pane.as_bytes()) else {
            continue;
        };
        let field = (Field::new(p + PANE_TRANSLATE_Y), *k);
        match panes.iter_mut().find(|e| e.pane == p) {
            Some(e) => e.translate_y = Some(field),
            None => panes.push(Pane {
                pane: p,
                scale_x: None,
                scale_y: None,
                translate_x: None,
                translate_y: Some(field),
            }),
        }
    }
    panes
}

pub extern "C" fn calculate(layout: usize) {
    let Some(aspect) = super::aspect::rendered_aspect() else {
        return;
    };
    let f = (aspect * 9.0 / 16.0) as f32;
    unsafe {
        let root = *((layout + LAYOUT_ROOT) as *const usize);
        if root == 0 {
            return;
        }
        let name_ptr = *((layout + LAYOUT_NAME) as *const usize);
        let mut layouts = LAYOUTS.lock().unwrap();
        let layouts = layouts.get_or_insert_with(HashMap::new);
        let entry = layouts.entry(layout).or_insert_with(|| Layout {
            name_ptr: 0,
            root: 0,
            panes: Vec::new(),
            hidden: Vec::new(),
        });
        if (entry.name_ptr, entry.root) != (name_ptr, root) {
            (entry.name_ptr, entry.root) = (name_ptr, root);
            entry.panes = setup(c_str(name_ptr, 64), root);
            let name = c_str(name_ptr, 64);
            entry.hidden = HIDDEN
                .iter()
                .filter(|h| h.0.as_bytes() == name)
                .filter_map(|h| find_pane(root, h.1.as_bytes()))
                .map(|pane| Visibility {
                    pane,
                    base: *((pane + PANE_FLAGS) as *const u8) & 1 != 0,
                    written: None,
                })
                .collect();
        }
        // Wider than 16:9: x scales and edge offsets by F; narrower: y scales by f (each factor 1 for the other shapes,
        // so a window that changes between them gets the game's values back).
        let (fx, fy) = if f >= 1.0 { (f, 1.0) } else { (1.0, f) };
        let hide = f != 1.0 && crate::settings::get().totk.v1_1_2.hide_hud_captures;
        for h in &mut entry.hidden {
            h.set(hide);
        }
        for p in &mut entry.panes {
            let mut dirty = false;
            if let Some((s, e)) = &mut p.scale_x {
                let factor = fx.powi(*e);
                dirty |= s.set(|v| v * factor);
                if let Some(y) = &mut p.scale_y {
                    let factor = fy.powi(-*e);
                    dirty |= y.set(|v| v * factor);
                }
            }
            if let Some((t, k)) = &mut p.translate_x {
                // The root pane is outside the narrowing: its offset is narrowed with the picture.
                let d = *k * (fx - 1.0) / if p.pane == root { fx } else { 1.0 };
                dirty |= t.set(|v| v + d);
            }
            if let Some((t, k)) = &mut p.translate_y {
                let gy = 1.0 / fy;
                let d = *k * (gy - 1.0) / if p.pane == root { gy } else { 1.0 };
                dirty |= t.set(|v| v + d);
            }
            if dirty {
                *((p.pane + PANE_FLAGS) as *mut u8) |= FLAG_MTX_DIRTY;
            }
        }
    }
}
