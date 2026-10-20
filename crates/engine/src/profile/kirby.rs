// Profile definition for Kirby and the Forgotten Land.

use super::{GameKind, GameProfile, VersionProfile, KIRBY_TITLE_ID};

pub static V1_0_0: VersionProfile = VersionProfile {
    version_string: "1.0.0",
    build_id_prefix: "a6ce40dc3aedb1becacd2f1ff4515307",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

pub static PROFILE: GameProfile = GameProfile {
    kind: GameKind::Kirby,
    name: "Kirby and the Forgotten Land",
    short_name: "kirby",
    title_id: KIRBY_TITLE_ID,
    lower_left_window_origin: true,
    compute_has_preamble: false,
    versions: &[V1_0_0],
    shader_dirs: &["shader", "gfx", "ptcl", "lyt"],
    options: None,
    platform_options: None,
    presets: &[],
};
