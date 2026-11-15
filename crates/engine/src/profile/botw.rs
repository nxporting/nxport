// Profile definition for The Legend of Zelda: Breath of the Wild.

use super::{GameKind, GameProfile, VersionProfile, BOTW_TITLE_ID};

pub mod frame_time;

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
    #[cfg(nx_runtime)]
    install_hooks: Some(install_v1_9_0_hooks),
    #[cfg(not(nx_runtime))]
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

/// 1.9.0's clock code (frame_time.rs).
pub static SITES_V1_9_0: frame_time::Sites = frame_time::Sites {
    frame_wait: 0xe9487c,
    vfr_ratio: 0x151d484,
    gui_speed: 0x1280ecc,
    gui_speed_code: frame_time::gui_speed_code(0xf000_5db3, 0x9112_e273), // adrp x19, main+0x1e37000; add x19, #0x4b8
    controller_step: (0xeb0f78, 0x1dad0e0),
};

#[cfg(nx_runtime)]
unsafe fn install_v1_9_0_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    frame_time::install(base, mem_space, &SITES_V1_9_0);
}
