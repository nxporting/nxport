// Profile definition for Silver Pines (Unity 6000.3, IL2CPP).

use super::{GameKind, GameProfile, VersionProfile, SILVER_PINES_TITLE_ID};

pub static V1_0_0: VersionProfile = VersionProfile {
    version_string: "1.0.0",
    build_id_prefix: "91046036f8850de3ec306056fcc6ba12",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

pub static PROFILE: GameProfile = GameProfile {
    kind: GameKind::SilverPines,
    name: "Silver Pines",
    short_name: "silverpines",
    title_id: SILVER_PINES_TITLE_ID,
    lower_left_window_origin: false,
    compute_has_preamble: true,
    versions: &[V1_0_0],
    shader_dirs: &[],
    options: Some(options),
    platform_options: None,
    presets: &[],
    module_patches: &[],
    main_thread_tick: None,
    mods: super::bundled::SILVER_PINES,
};

fn options() -> crate::settings::SettingsPatch {
    let mut s = crate::settings::SettingsPatch::default();
    // Off until the player turns it on (the Unity menu's Texture Size changes the size while the game runs).
    s.files.disabled_mods = Some(vec!["Full-size textures at launch".into()]);
    s
}
