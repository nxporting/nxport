// Profile definition for Super Mario Bros. Wonder.

use super::{GameKind, GameProfile, VersionProfile, WONDER_TITLE_ID};

pub static V1_0_0: VersionProfile = VersionProfile {
    version_string: "1.0.0",
    build_id_prefix: "b37398a1b68747b7a51968433515cb8c",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

pub static PROFILE: GameProfile = GameProfile {
    kind: GameKind::MarioWonder,
    name: "Super Mario Bros. Wonder",
    short_name: "wonder",
    title_id: WONDER_TITLE_ID,
    lower_left_window_origin: false,
    compute_has_preamble: true,
    versions: &[V1_0_0],
    shader_dirs: &["Shader", "Lib", "Effect", "UI", "Layout", "Model"],
    options: None,
    platform_options: None,
    presets: &[],
};
