// Profile definition for The Legend of Zelda: Tears of the Kingdom.

pub mod options;
pub mod v1_0_0;
pub mod v1_1_2;

pub use options::{Options, OptionsPatch};

use super::{GameKind, GameProfile, TOTK_TITLE_ID};

pub static PROFILE: GameProfile = GameProfile {
    kind: GameKind::TotK,
    name: "The Legend of Zelda: Tears of the Kingdom",
    short_name: "totk",
    title_id: TOTK_TITLE_ID,
    lower_left_window_origin: false,
    compute_has_preamble: true,
    versions: &[v1_0_0::PROFILE, v1_1_2::PROFILE],
    shader_dirs: &["Shader", "Effect", "UI", "Lib"],
    options: None,
    platform_options: None,
    presets: &[
        ("Mac", include_str!("presets/Mac.toml")),
        ("iPad", include_str!("presets/iPad.toml")),
        ("iPhone 12-16 & 16e", include_str!("presets/iPhone 12-16 & 16e.toml")),
        ("iPhone 16 Pro & Pro Max", include_str!("presets/iPhone 16 Pro & Pro Max.toml")),
        ("iPhone SE", include_str!("presets/iPhone SE.toml")),
    ],
};
