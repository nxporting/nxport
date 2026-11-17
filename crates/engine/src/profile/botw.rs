// Profile definition for The Legend of Zelda: Breath of the Wild.

use super::{GameKind, GameProfile, VersionProfile, BOTW_TITLE_ID};

pub mod aspect;
pub mod frame_time;
pub mod hud;

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
    // BotW reads two tiny linear render targets it draws every frame (the 1x1 / 3x3 lighting and exposure values) right
    // after its frame fence. The early fence returns before the GPU wrote them, so the game read last frame's values;
    // in the first frames after a start or a load those are not there yet, and the water stayed milky white until the
    // next load (Savage Wind #362 and #332, with the same overlap). Waiting for those reads is waiting for the frame, so
    // the fence waits for the frame. (Turned off once before, da752b5, for the water height and flicker, which 8a35540
    // fixed instead; turned on again in bf93f40.)
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
    arrow: Some((0x5ee6d0, 0xb325c0, 0x5ee734)),
    snd_floor: Some(0x1551004),
    kf_angle: Some((0x12d57fc, 0xb06d4)),
    phys_world_tick: [0xf5e20, 0x12644c],
    phys_body_tick: [(0x11f95c, 20), (0x1207cc, 26)],
};

/// 1.9.0's projections (aspect.rs).
pub static ASPECT_V1_9_0: aspect::Sites = aspect::Sites {
    update: 0xeb4474,
    perspective: (0x1d05380, 0xeb4a44),
    frustum: (0x1d05480, 0xeb4f24),
};

/// 1.9.0's layout calculations (hud.rs): the library's (`mov x2, sp`) and the game's layout class's (`add x2, sp, #8`);
/// the clipping pane's call of the scissor setter.
pub static HUD_V1_9_0: hud::Sites = hud::Sites {
    layout: [(0xf625dc, 0x9100_03e2), (0x25c98, 0x9100_23e2)],
    scissor: (0xf67138, 0xeb6a54),
};

#[cfg(nx_runtime)]
unsafe fn install_v1_9_0_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    frame_time::install(base, mem_space, &SITES_V1_9_0);
    aspect::install(base, mem_space, &ASPECT_V1_9_0);
    hud::install(base, mem_space, &HUD_V1_9_0);
}
