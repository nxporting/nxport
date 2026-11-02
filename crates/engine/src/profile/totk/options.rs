// Settings of The Legend of Zelda: Tears of the Kingdom (hooks into its code; versions 1.0.0 and 1.1.2).

use serde::Serialize;
use struct_patch::Patch;

crate::settings::group! {
    /// Changes made to the game's code (the same on every version).
    Enhancements "Tears of the Kingdom" [dynamic_aspect_ratio, dynamic_fps] {
        /// Render at the window's shape and follow it as the window is resized (the camera, culling and HUD layouts
        /// follow it; narrower than 16:9 shows more vertically). Can be switched while the game runs
        dynamic_aspect_ratio: bool = true,
        /// Workaround: hide HUD capture panes that sample the wrong screen region at shapes other than 16:9 (a ghost
        /// copy of the load screen's heading); their effect is lost there
        hide_hud_captures: bool = true,
        /// Variable frame rate: game time follows the real time each frame took, so the game plays at the right speed at
        /// any frame rate (off: the console's fixed timing, held to 30 or 60 fps)
        dynamic_fps: bool = true,
    }
}

crate::settings::group! {
    /// Probes that hook the game's renderer, for reverse engineering it.
    Diagnostics "Tears of the Kingdom: diagnostics" {
        /// Journal the game's occlusion query reads (with the frame history)
        journal_occl_reads: bool = false,
        /// Count draws by the drawable object that records them
        log_drawables: bool = false,
        /// Count draws by their render item, and how much of each repeats from its previous run
        log_draw_items: bool = false,
        /// Text draws recorded by the engine instead of the game's font drawer
        native_font_draws: bool = false,
        /// Compare each text draw the game records with what --native-font-draws would record
        verify_font_draws: bool = false,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Patch)]
#[serde(rename_all = "kebab-case")]
#[patch(attribute(derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    clap::Args,
    serde::Serialize,
    serde::Deserialize
)))]
#[patch(attribute(serde(default, rename_all = "kebab-case", deny_unknown_fields)))]
pub struct Options {
    // [totk.enhancements]; settings files and presets from before it covered every version still load from
    // [totk."1.1.2"].
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    #[patch(attribute(serde(alias = "1.1.2")))]
    pub enhancements: Enhancements,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub diagnostics: Diagnostics,
}

impl Options {
    /// This game's settings in an editor, by section.
    #[cfg(any(target_os = "macos", windows))]
    pub fn sections(&mut self) -> Vec<(&'static str, &mut dyn crate::settings::edit::Section)> {
        vec![
            ("Enhancements", &mut self.enhancements),
            ("Diagnostics", &mut self.diagnostics),
        ]
    }
}
