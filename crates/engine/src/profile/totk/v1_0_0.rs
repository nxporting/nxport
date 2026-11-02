// Profile and hooks for The Legend of Zelda: Tears of the Kingdom v1.0.0.
// NSO Build ID: 082ce09b06e33a123cb1e2770f5f9147
//
// Frame pacing and game time: frame_time.rs (the pacer at main+0x8f6bec, the time manager's update at main+0x7a83e4).
// Screen shapes: aspect.rs, hud.rs.

use crate::profile::VersionProfile;

use super::aspect;
use super::frame_time::{self, Sites};

pub static PROFILE: VersionProfile = VersionProfile {
    version_string: "1.0.0",
    build_id_prefix: "082ce09b06e33a123cb1e2770f5f9147",
    #[cfg(nx_runtime)]
    install_hooks: Some(install_v1_0_0_hooks),
    #[cfg(not(nx_runtime))]
    install_hooks: None,
    heap_instance_offset: None,
    allocator_pool_offset: None,
    draw_item: None,
};

pub static SITES: Sites = Sites {
    lowest: 0xe9a018,
    veto: 0x8f6d74,
    step: 0x7a83f4,
    step_code: frame_time::step_code(0xb001_eda8),           // adrp x8, main+0x455d000
    input: 0x8f68b8,
    input_code: frame_time::input_code(0x9001_e388, 0xf943_5108), // adrp x8, main+0x4566000; ldr x8, [x8, #0x6a0]
};

/// Screen shapes other than 16:9: 1.1.2's sites at their 1.0.0 addresses (the code matches apart from pc-relative
/// immediates).
pub static ASPECT_SITES: aspect::Sites = aspect::Sites {
    aspect_const: 0x377ac54,
    ratio: [0x1968c2c, 0x1e5eae8],
    show: [(0x1a8f178, 0x1a8f1a0), (0x1a8e688, 0x1a8e6b0)],
    moved: 0x1a93954,
    floor: [0x12ade60, 0x12ae24c],
    camera: [0xc7e528, 0x1233e78],
    fovy: 0x1a13d1c, // in sead::PerspectiveProjection::setFovy_ (main+0x1a13cdc)
    layout: [0x74f344, 0x40ea8], // in main+0x74f330 and main+0x40e94
};

#[cfg(nx_runtime)]
pub unsafe fn install_v1_0_0_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    // Screen shapes first, whatever the settings (as 1.1.2).
    aspect::install(base, mem_space, &ASPECT_SITES);
    frame_time::install(base, mem_space, &SITES);
}
