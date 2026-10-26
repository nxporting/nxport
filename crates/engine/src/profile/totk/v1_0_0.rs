// Profile and hooks for The Legend of Zelda: Tears of the Kingdom v1.0.0.
// NSO Build ID: 082ce09b06e33a123cb1e2770f5f9147
//
// Frame pacing: each frame, main+0x8f6bec picks the present interval from the frame's measured duration (the pacing
// state at frame object + 0x248: lowest and highest interval, bias, frames before raising, cooldown, current interval,
// the interval it may drop to, a list of listeners that can veto a drop), and main+0x8f68b8 sets the game's time step
// to interval / 2 (1.0 at 30 fps). The game steps time by the interval it presents at, so a lower interval needs no
// delta-time changes. Stock, the lowest interval is 2 and a listener vetoes drops to it from 3.
//
// The hooks only rewrite instructions, so they apply to every runtime: natively, and to the image that is lifted (a
// lifted game takes the fps setting it was lifted with). Without vsync the time step stays half a frame of 30 fps per
// frame, so the game runs faster than real time above 60 fps.

use crate::profile::VersionProfile;

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

/// main+0xe9a018 builds the pacing limits {lowest 2, highest 3} (movz x8, #2; movk x8, #3, lsl #32).
const SITE_LOWEST: usize = 0xe9a018;
const MOVZ_X8_2: u32 = 0xd280_0048;
const MOVZ_X8_1: u32 = 0xd280_0028;
/// main+0x8f6d74: after asking the listeners, a veto keeps the current interval (cbnz w8, main+0x8f6c94).
const SITE_VETO: usize = 0x8f6d74;
const CBNZ_VETO: u32 = 0x35ff_f908;
const NOP: u32 = 0xd503_201f;

#[cfg(nx_runtime)]
unsafe fn patch(base: usize, site: usize, from: u32, to: u32, what: &str) -> bool {
    let p = (base + site) as *mut u32;
    match std::ptr::read(p) {
        w if w == from => {
            std::ptr::write(p, to);
            crate::os::flush_icache(p as usize, 4);
            true
        }
        w if w == to => true,
        w => {
            println!("[!] Frame pacer: unexpected instruction {w:#010x} at main+{site:#x} ({what}); not patched");
            false
        }
    }
}

#[cfg(nx_runtime)]
pub unsafe fn install_v1_0_0_hooks(base: usize, _mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    if crate::settings::get().system.fps == crate::settings::FpsMode::Stock30 {
        println!("[+] Frame pacer: stock 30 fps cap active (lowest present interval 2)");
        return;
    }
    // Both or neither: the lowest interval alone stays vetoed.
    let p = (base + SITE_VETO) as *const u32;
    if std::ptr::read(p) != CBNZ_VETO && std::ptr::read(p) != NOP {
        println!("[!] Frame pacer: unexpected instruction {:#010x} at main+{SITE_VETO:#x}; fps unlock not applied", std::ptr::read(p));
        return;
    }
    if patch(base, SITE_LOWEST, MOVZ_X8_2, MOVZ_X8_1, "lowest interval") && patch(base, SITE_VETO, CBNZ_VETO, NOP, "veto") {
        println!("[+] Frame pacer: lowest present interval 1, drops not vetoed (up to 60 fps with vsync)");
    }
}
