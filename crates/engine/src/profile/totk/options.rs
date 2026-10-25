// Settings of The Legend of Zelda: Tears of the Kingdom (hooks into its code; version 1.1.2).

use serde::Serialize;
use struct_patch::Patch;

crate::settings::group! {
    /// Changes made to version 1.1.2's code.
    Version112 "Tears of the Kingdom 1.1.2" [dynamic_aspect_ratio] {
        /// Render at the screen's shape and follow it as the window is resized (camera, culling and HUD
        /// layouts follow it; narrower than 16:9 shows more vertically)
        dynamic_aspect_ratio: bool = true,
        /// Workaround: hide HUD capture panes that sample the wrong screen region at shapes other than 16:9 (a ghost
        /// copy of the load screen's heading); their effect is lost there
        hide_hud_captures: bool = true,
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
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    #[serde(rename = "1.1.2")]
    #[patch(attribute(serde(rename = "1.1.2")))]
    pub v1_1_2: Version112,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub diagnostics: Diagnostics,
}

impl Options {
    /// This game's settings in an editor, by section.
    #[cfg(any(target_os = "macos", windows))]
    pub fn sections(&mut self) -> Vec<(&'static str, &mut dyn crate::settings::edit::Section)> {
        vec![
            ("1.1.2", &mut self.v1_1_2),
            ("Diagnostics", &mut self.diagnostics),
        ]
    }
}
