// Profile and hooks for The Legend of Zelda: Tears of the Kingdom v1.1.2.
// NSO Build ID: 9a10ed9435c06733da597d8094d9000ab5d3ee60

// The hook writes code of its own into trampolines: native runtime only.
#[cfg(nx_native)]
pub mod font_draw;

use crate::profile::VersionProfile;

use super::aspect;
use super::frame_time::{self, Sites};

pub static PROFILE: VersionProfile = VersionProfile {
    version_string: "1.1.2",
    build_id_prefix: "9a10ed9435c06733da597d8094d9000a",
    #[cfg(nx_runtime)]
    install_hooks: Some(install_v1_1_2_hooks),
    #[cfg(not(nx_runtime))]
    install_hooks: None,
    heap_instance_offset: Some(0x047137d0),
    allocator_pool_offset: Some(0x0466e500),
    #[cfg(nx_native)]
    draw_item: Some(draw_item),
    #[cfg(not(nx_native))]
    draw_item: None,
};

/// Frame pacing and game time (frame_time.rs): the pacer at main+0x8f6670, the time manager's update at main+0x7f7a28.
pub static SITES: Sites = Sites {
    lowest: 0xea87e0,
    veto: 0x8f67f8,
    step: 0x7f7a38,
    step_code: frame_time::step_code(0xd001_f1c8),           // adrp x8, main+0x4631000
    input: 0x8f633c,
    input_code: frame_time::input_code(0xf001_ea08, 0xf945_cd08), // adrp x8, main+0x4639000; ldr x8, [x8, #0xb98]
    camera_rate: Some((0x1d3675c, 0xc54)),
    ground_carry: Some((0xbbb4b4, 0xf940_0348)), // ldr x8, [x26]
    cloth: None, // 1.1.2 leaves that clock unset
    short_cut_bg: Some((0x462cb20, 0x19af400, 0xc5a4_9707)),
};

/// Screen shapes other than 16:9 (aspect.rs, hud.rs).
pub static ASPECT_SITES: aspect::Sites = aspect::Sites {
    aspect_const: 0x3813d0c,
    ratio: [0x19b5480, 0x1ece314],
    show: [(0x1ae042c, 0x1ae0454), (0x1adf824, 0x1adf868)],
    moved: 0x1ae4c24,
    floor: [0x12c2410, 0x12c2828],
    camera: [0xccbcb8, 0x1255594],
    fovy: 0x1a61bd4, // in sead::PerspectiveProjection::setFovy_ (main+0x1a61b94)
    layout: [0x79d818, 0x40e38], // in main+0x79d804 and main+0x40e24
};

/// --log-draw-items: main+0x763718 takes (renderer, item, ...), keeps the item at [x29 - 0x20] and calls the renderer's
/// draw callback (return address main+0x763798); most of the frame's draws come through it.
#[cfg(nx_native)]
unsafe fn draw_item() -> u32 {
    let base = crate::cpu::crash_handler::get_guest_base();
    let mut fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack));
    for _ in 0..16 {
        if fp < 0x1000 || fp & 7 != 0 {
            break;
        }
        let next = *(fp as *const usize);
        let ret = *((fp + 8) as *const usize);
        if ret == base + 0x763798 && next > 0x1000 && next & 7 == 0 {
            let item = *((next - 0x20) as *const usize);
            return ((item >> 3) as u32) | 1;
        }
        if next <= fp {
            break;
        }
        fp = next;
    }
    0
}

#[cfg(nx_native)]
extern "C" {
    fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
}

/// The hooks every runtime takes (they rewrite instructions and call HLE trampolines, so a lifted game has them too),
/// then natively the ones that write code of their own into trampolines.
#[cfg(nx_runtime)]
pub unsafe fn install_v1_1_2_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    // 1. Screen shapes, installed whatever the settings: the dynamic aspect ratio is switched while the game runs
    //    (host/live.rs), and a lifted game keeps the hooks it was lifted with.
    aspect::install(base, mem_space, &ASPECT_SITES);
    // 2. Game time from real time, and the pacer unlocked; the frame-rate setting caps it while the game runs.
    // Installed whatever the setting, as the aspect hooks.
    frame_time::install(base, mem_space, &SITES);
    // 3. Mouse aiming reads the camera's angles (installed whatever the settings, as above).
    super::camera_angles::install(base, mem_space);
    #[cfg(nx_native)]
    install_native_hooks(base, mem_space);
}

#[cfg(nx_native)]
unsafe fn install_native_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    // 3. --journal-occl-reads (diagnostic): journal every call of the occlusion query read (main+0x10c4d44, which
    // takes its query object in x0) with the state it reads (metal::journal_occl_read). The entry instruction branches
    // to a wrapper in the spare words of an HLE trampoline: the displaced stp, the logger call (x0 saved), back.
    if crate::settings::get().totk.diagnostics.journal_occl_reads {
        const SITE: usize = 0x10c4d44;
        const STP_X29_X30_M40: u32 = 0xa9bc7bfd; // stp x29, x30, [sp, #-0x40]!
        let site = (base + SITE) as *mut u32;
        let tramp =
            mem_space.make_hle_trampoline(crate::hle!(crate::metal::journal_occl_read(_))) as usize;
        let wrap = tramp + 12; // words 3..7 of the 8-word entry (make_hle_trampoline uses 0..2)
        let b = |from: usize, to: usize, link: bool| -> Option<u32> {
            let d = to as i64 - from as i64;
            (d.abs() < 1 << 27).then(
                || if link { 0x9400_0000 } else { 0x1400_0000 } | ((d >> 2) as u32 & 0x03ff_ffff),
            )
        };
        match (std::ptr::read(site), b(site as usize, wrap, false), b(wrap + 8, tramp, true), b(wrap + 16, site as usize + 4, false)) {
            (STP_X29_X30_M40, Some(to_wrap), Some(call), Some(back)) => {
                let w = wrap as *mut u32;
                std::ptr::write(w, STP_X29_X30_M40);
                std::ptr::write(w.add(1), 0xf81f0fe0); // str x0, [sp, #-16]!
                std::ptr::write(w.add(2), call); // bl <logger trampoline>
                std::ptr::write(w.add(3), 0xf84107e0); // ldr x0, [sp], #16
                std::ptr::write(w.add(4), back); // b site+4
                std::ptr::write(site, to_wrap);
                sys_icache_invalidate(site as *mut libc::c_void, 4);
                println!("[+] Occlusion read journal hook installed at main+{:#x}", SITE);
            }
            (w, ..) => println!("[!] Occlusion read journal hook not installed (main+{:#x} = {:#010x}, or out of branch range)", SITE, w),
        }
    }

    // 4. --log-drawables (RE probe): the render-list loop main+0x12de2a0 calls each entry's
    // drawable through its vtable (entry+0x10, slot 0x60) at main+0x12de360 (`blr x8`, the object in x0). The call
    // branches to a wrapper (a second trampoline entry's 16 words) that saves the argument registers, tells
    // nvn::cmd::note_drawable(object, target, context), restores them and makes the call; draws recorded meanwhile carry the
    // drawable (gfx_cmd note_draw_caller).
    if crate::settings::get().totk.diagnostics.log_drawables {
        const SITE: usize = 0x12de360;
        const BLR_X8: u32 = 0xd63f0100;
        let site = (base + SITE) as *mut u32;
        let tramp =
            mem_space.make_hle_trampoline(crate::hle!(crate::nvn::cmd::note_drawable(_, _, _))) as usize;
        // Entries are 8 words, made one after another: the wrapper (15 words) takes two, the code after the call a third.
        let wrap =
            mem_space.make_hle_trampoline(crate::hle!(crate::nvn::cmd::note_drawable(_, _, _))) as usize;
        mem_space.make_hle_trampoline(crate::hle!(crate::nvn::cmd::note_drawable(_, _, _)));
        mem_space.make_hle_trampoline(crate::hle!(crate::nvn::cmd::note_drawable(_, _, _)));
        let post = wrap + 64;
        let b = |from: usize, to: usize, link: bool| -> Option<u32> {
            let d = to as i64 - from as i64;
            (d.abs() < 1 << 27).then(
                || if link { 0x9400_0000 } else { 0x1400_0000 } | ((d >> 2) as u32 & 0x03ff_ffff),
            )
        };
        match (std::ptr::read(site), b(site as usize, wrap, false), b(wrap + 28, tramp, true), b(wrap + 56, post, false),
               b(post + 20, tramp, true), b(post + 28, site as usize + 4, false)) {
            (BLR_X8, Some(to_wrap), Some(call), Some(back), Some(call2), Some(back2)) => {
                // After the call: note_drawable(0, 0, 0) (no drawable), x0 / x1 kept (the callee's return value).
                let pw = post as *mut u32;
                let pcode: [u32; 8] = [
                    0xa9bf07e0, // stp x0, x1, [sp, #-16]!
                    0xd2800000, // mov x0, #0
                    0xd2800001, // mov x1, #0
                    0xd2800002, // mov x2, #0
                    0xd503201f, // nop
                    call2,      // bl <note_drawable trampoline>
                    0xa8c107e0, // ldp x0, x1, [sp], #16
                    back2,      // b site+4
                ];
                for (k, c) in pcode.iter().enumerate() {
                    std::ptr::write(pw.add(k), *c);
                }
                sys_icache_invalidate(pw as *mut libc::c_void, 32);
                let w = wrap as *mut u32;
                let code: [u32; 15] = [
                    0xa9ba07e0, // stp x0, x1, [sp, #-0x60]!
                    0xa9010fe2, // stp x2, x3, [sp, #0x10]
                    0xa90217e4, // stp x4, x5, [sp, #0x20]
                    0xa9037be8, // stp x8, x30, [sp, #0x30]
                    0xa9041fe6, // stp x6, x7, [sp, #0x40]
                    0xaa0103e2, // mov x2, x1 (the draw context)
                    0xaa0803e1, // mov x1, x8
                    call,       // bl <note_drawable trampoline>
                    0xa9441fe6, // ldp x6, x7, [sp, #0x40]
                    0xa9437be8, // ldp x8, x30, [sp, #0x30]
                    0xa94217e4, // ldp x4, x5, [sp, #0x20]
                    0xa9410fe2, // ldp x2, x3, [sp, #0x10]
                    0xa8c607e0, // ldp x0, x1, [sp], #0x60
                    BLR_X8,     // the original call
                    back,       // b <post>: clear the drawable, then site+4
                ];
                for (k, c) in code.iter().enumerate() {
                    std::ptr::write(w.add(k), *c);
                }
                sys_icache_invalidate(w as *mut libc::c_void, 64);
                std::ptr::write(site, to_wrap);
                sys_icache_invalidate(site as *mut libc::c_void, 4);
                println!("[+] Drawable probe hook installed at main+{:#x}", SITE);
            }
            (w, ..) => println!("[!] Drawable probe hook not installed (main+{:#x} = {:#010x}, or out of branch range)", SITE, w),
        }
    }

    // 5. Text draws (nn::font::RectDrawer)
    let d = &crate::settings::get().totk.diagnostics;
    if d.native_font_draws || d.verify_font_draws {
        font_draw::install(base, mem_space);
    }
}
