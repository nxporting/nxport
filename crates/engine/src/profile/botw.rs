// Profile definition for The Legend of Zelda: Breath of the Wild.

use super::{GameKind, GameProfile, VersionProfile, BOTW_TITLE_ID};

pub static V1_0_0: VersionProfile = VersionProfile {
    version_string: "1.0.0",
    build_id_prefix: "2094f152836683a3632c6153fd84ab80",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

pub static V1_6_0: VersionProfile = VersionProfile {
    version_string: "1.6.0",
    build_id_prefix: "8e9978d50bdd20b4c8395a106c27ffde",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

pub static V1_9_0: VersionProfile = VersionProfile {
    version_string: "1.9.0",
    build_id_prefix: "cd57b23fa4bbad65803d9788c01821ee",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

pub static V1_5_0: VersionProfile = VersionProfile {
    version_string: "1.5.0",
    build_id_prefix: "16a91992bba71201e98756f3bc8f5d2f",
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

fn options() -> crate::settings::SettingsPatch {
    let mut s = crate::settings::SettingsPatch::default();
    // The game reads GPU results as soon as its frame fence says the work is done: the collision height tiles and the
    // lighting / exposure values it draws into linear render targets. The early fence lets that wait return with the
    // frame still on the GPU (TotK reads such results a frame late, vulkan/guard.rs), so BotW read heights and lighting
    // that were not written yet (Link lifted into the air near water, milky water, flicker; Savage Wind #362).
    s.graphics.early_fence = Some(false);
    s
}

pub static PROFILE: GameProfile = GameProfile {
    kind: GameKind::BotW,
    name: "The Legend of Zelda: Breath of the Wild",
    short_name: "botw",
    title_id: BOTW_TITLE_ID,
    lower_left_window_origin: false,
    compute_has_preamble: true,
    versions: &[V1_0_0, V1_5_0, V1_6_0, V1_9_0],
    shader_dirs: &["Pack", "Effect", "UI", "Layout", "System", "Model"],
    options: Some(options),
    platform_options: None,
    presets: &[],
    module_patches: &[],
};
