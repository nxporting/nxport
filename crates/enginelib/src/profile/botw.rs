// Profile definition for The Legend of Zelda: Breath of the Wild.

use super::{GameKind, GameProfile, VersionProfile, BOTW_TITLE_ID};

pub mod aspect;
pub mod frame_time;
pub mod draw_distance;
pub mod hud;
pub mod render_size;

pub static V1_0_0: VersionProfile = VersionProfile {
    version_string: "1.0.0",
    build_id_prefix: "2094f152836683a3632c6153fd84ab80",
    #[cfg(nx_runtime)]
    install_hooks: Some(install_v1_0_0_hooks),
    #[cfg(not(nx_runtime))]
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
    #[cfg(nx_runtime)]
    install_hooks: Some(install_v1_5_0_hooks),
    #[cfg(not(nx_runtime))]
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
    demo_move: Some(0xc711dc),
};

/// 1.9.0's projections (aspect.rs).
pub static ASPECT_V1_9_0: aspect::Sites = aspect::Sites {
    // sead::Projection::updateMatrixImpl: `ldrb w8, [x0, #8]; mov x19, x0; cbz w8`.
    updates: &[aspect::Update {
        at: 0xeb4474,
        expected: &[0x3940_2008, 0xaa00_03f3, 0x3400_0268],
        code: &[0xaa00_03f3, 0x9400_0000, 0x3400_0260],
        call: 1,
    }],
    perspective: (0x1d05380, 0xeb4a44),
    frustum: (0x1d05480, 0xeb4f24),
    // The screens' views: made once by main+0x10ed664 (0x208 bytes; its views' projections set by main+0x10eda5c).
    world_cameras: Some(0x1e39d10),
    // ksys::evt::Manager: made by main+0x110bf98 (0x1d438 bytes); hasActiveEvent main+0x110df98.
    event_manager: Some(0x1e3a638),
};

/// 1.9.0's layout calculations (hud.rs): the library's (`mov x2, sp`) and the game's layout class's (`add x2, sp, #8`);
/// the clipping pane's call of the scissor setter.
pub static HUD_V1_9_0: hud::Sites = hud::Sites {
    layout: [(0xf625dc, hud::layout_code_v1_9(0x9100_03e2)), (0x25c98, hud::layout_code_v1_9(0x9100_23e2))],
    scissor: (0xf67138, 0xeb6a54),
    scissor_code: hud::SCISSOR_CODE,
    texgen: Some((0x2df24, hud::TEXGEN_CODE_V1_9)),
};

/// 1.0.0's projection sites (aspect.rs). No names for this version: the vtables are the ones whose getters are 1.5.0's
/// code (main+0xa22d74 / main+0xa232d0 their matrix functions), and the dirty-flag checks are inlined into the same three
/// matrix readers as in 1.5.0, reading the flag with the byte after it (`ldrh w8, [xN, #8]; and w9, w8, #0xff;
/// cbz w9`; the clean path tests w8's second byte, so the call is followed by the same `ldrh`).
pub static ASPECT_V1_0_0: aspect::Sites = aspect::Sites {
    updates: &[
        aspect::Update {
            at: 0xa2264c,
            expected: &[0xaa00_03f3, 0x7940_1268, 0x1200_1d09, 0x3400_0149],
            code: &[0xaa00_03f3, 0x9400_0000, 0x7940_1268, 0x3400_0140],
            call: 1,
        },
        aspect::Update {
            at: 0xa226d4,
            expected: &[0xaa00_03f3, 0x7940_1268, 0x1200_1d09, 0x3400_0149],
            code: &[0xaa00_03f3, 0x9400_0000, 0x7940_1268, 0x3400_0140],
            call: 1,
        },
        aspect::Update {
            at: 0xa22764,
            expected: &[0xaa00_03f6, 0x7940_12c8, 0xaa02_03f5, 0xaa01_03f3, 0x1200_1d09, 0x3400_0149],
            code: &[0xaa00_03f6, 0xaa02_03f5, 0xaa01_03f3, 0x9400_0000, 0x7940_12c8, 0x3400_0140],
            call: 3,
        },
    ],
    perspective: (0x21b75e0, 0xa22d74),
    frustum: (0x21b76e0, 0xa232d0),
    // The screens' views: made by main+0xc121f0 (as 1.9.0's; its views' projections set by main+0x10dfd2c).
    world_cameras: Some(0x22de770),
    // ksys::evt::Manager (through the GOT entry main+0x2264738); hasActiveEvent main+0xc363c8, the same offset.
    event_manager: Some(0x22def10),
};

/// 1.0.0's layout calculations (in 1.5.0's order) and the clipping pane's scissor call (its rectangle at sp + 8).
pub static HUD_V1_0_0: hud::Sites = hud::Sites {
    layout: [(0x9baa44, hud::layout_code_v1_5(0x9100_23e2)), (0xae2d6c, hud::layout_code_v1_5(0x9100_03e2))],
    scissor: (0xae6884, 0xa25438),
    scissor_code: [0xf940_27e1, 0x9100_23e0, 0xaa17_03e2],
    texgen: Some((0x9c7f04, hud::TEXGEN_CODE_V1_0)),
};

#[cfg(nx_runtime)]
unsafe fn install_v1_0_0_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    render_size::install(base, render_size::SCENE_V1_0_0);
    aspect::install(base, mem_space, &ASPECT_V1_0_0);
    hud::install(base, mem_space, &HUD_V1_0_0);
}

/// 1.5.0's projection sites (aspect.rs; names from the decomp of this version): the dirty-flag checks inlined into
/// sead::Projection's matrix readers, PerspectiveProjection::doUpdateMatrix main+0xb1dfa0, FrustumProjection's
/// main+0xb1e4e0.
pub static ASPECT_V1_5_0: aspect::Sites = aspect::Sites {
    updates: &[
        // getProjectionMatrix (main+0xb1d8a0) and getDeviceProjectionMatrix (main+0xb1d918): `mov x19, x0;
        // ldrb w8, [x19, #8]; cbz w8`.
        aspect::Update { at: 0xb1d8ac, expected: &[0xaa00_03f3, 0x3940_2268, 0x3400_0148], code: &[0xaa00_03f3, 0x9400_0000, 0x3400_0140], call: 1 },
        aspect::Update { at: 0xb1d928, expected: &[0xaa00_03f3, 0x3940_2268, 0x3400_0148], code: &[0xaa00_03f3, 0x9400_0000, 0x3400_0140], call: 1 },
        // cameraPosToScreenPos (main+0xb1d9a4): `mov x22, x0; ldrb w8, [x22, #8]; mov x21, x2; mov x19, x1; cbz w8`.
        aspect::Update {
            at: 0xb1d9b4,
            expected: &[0xaa00_03f6, 0x3940_22c8, 0xaa02_03f5, 0xaa01_03f3, 0x3400_0148],
            code: &[0xaa00_03f6, 0xaa02_03f5, 0xaa01_03f3, 0x9400_0000, 0x3400_0140],
            call: 3,
        },
    ],
    perspective: (0x24bf9e8, 0xb1dfa0),
    frustum: (0x24bfae8, 0xb1e4e0),
    // Not found for this version yet: the field of view stays the game's.
    world_cameras: None,
    event_manager: None,
};

/// 1.5.0's layout calculations (nn::ui2d::Layout::CalculateImpl, eui::LayoutEx::CalculateImpl) and
/// eui::ScissorPane::Draw's call of sead::Viewport::applyScissor.
pub static HUD_V1_5_0: hud::Sites = hud::Sites {
    layout: [(0xab7738, hud::layout_code_v1_5(0x9100_23e2)), (0xbde5e8, hud::layout_code_v1_5(0x9100_03e2))],
    scissor: (0xbe2080, 0xb20510),
    scissor_code: hud::SCISSOR_CODE,
    texgen: Some((0xac4530, hud::TEXGEN_CODE_V1_0)),
};

#[cfg(nx_runtime)]
unsafe fn install_v1_5_0_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    aspect::install(base, mem_space, &ASPECT_V1_5_0);
    hud::install(base, mem_space, &HUD_V1_5_0);
}

#[cfg(nx_runtime)]
unsafe fn install_v1_9_0_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    draw_distance::install(base, mem_space, 0x10a4684);
    render_size::install(base, render_size::SCENE_V1_9_0);
    frame_time::install(base, mem_space, &SITES_V1_9_0);
    aspect::install(base, mem_space, &ASPECT_V1_9_0);
    hud::install(base, mem_space, &HUD_V1_9_0);
}
