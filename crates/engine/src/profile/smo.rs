// Profile definition for Super Mario Odyssey.

use super::{GameKind, GameProfile, VersionProfile, SMO_TITLE_ID};

pub static V1_4_1: VersionProfile = VersionProfile {
    version_string: "1.4.1",
    build_id_prefix: "b424be150a8e7d78701cbe7a439d9ebf",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

pub static PROFILE: GameProfile = GameProfile {
    kind: GameKind::SuperMarioOdyssey,
    name: "Super Mario Odyssey",
    short_name: "smo",
    title_id: SMO_TITLE_ID,
    lower_left_window_origin: false,
    compute_has_preamble: true,
    versions: &[V1_4_1],
    shader_dirs: &["ShaderData", "EffectData", "LayoutData", "SystemData", "ObjectData"],
    options: None,
    platform_options: None,
    presets: &[],
};
