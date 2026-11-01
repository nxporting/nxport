// Profile and hooks for The Legend of Zelda: Tears of the Kingdom v1.0.0.
// NSO Build ID: 082ce09b06e33a123cb1e2770f5f9147
//
// Frame pacing and game time: frame_time.rs (the pacer at main+0x8f6bec, the time manager's update at main+0x7a83e4).

use crate::profile::VersionProfile;

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

#[cfg(nx_runtime)]
pub unsafe fn install_v1_0_0_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    frame_time::install(base, mem_space, &SITES);
}
