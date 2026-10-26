// Profile and hooks for The Legend of Zelda: Tears of the Kingdom v1.1.2.
// NSO Build ID: 9a10ed9435c06733da597d8094d9000ab5d3ee60

// The hooks patch the game's arm64 code in place: native runtime only.
#[cfg(nx_native)]
pub mod aspect;
#[cfg(nx_native)]
pub mod dynamic_fps;
#[cfg(nx_native)]
pub mod hud;

use crate::profile::VersionProfile;

pub static PROFILE: VersionProfile = VersionProfile {
    version_string: "1.1.2",
    build_id_prefix: "9a10ed9435c06733da597d8094d9000a",
    #[cfg(nx_native)]
    install_hooks: Some(install_v1_1_2_hooks),
    #[cfg(not(nx_native))]
    install_hooks: None,
    heap_instance_offset: Some(0x047137d0),
    allocator_pool_offset: Some(0x0466e500),
    #[cfg(nx_native)]
    draw_item: Some(draw_item),
    #[cfg(not(nx_native))]
    draw_item: None,
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

#[cfg(nx_native)]
pub unsafe fn install_v1_1_2_hooks(
    base: usize,
    mem_space: &mut crate::guest::loader::GuestMemorySpace,
) {
    // 1. Frame rate unlock: patch min vblanks from 2 to 1 (Stock30 keeps 30 fps).
    {
        let fps_unlocked = crate::settings::get().system.fps != crate::settings::FpsMode::Stock30;
        let p = (base + 0xea87e0) as *mut u32;
        const MOV_X8_2: u32 = 0xd280_0048; // movz x8, #2
        const MOV_X8_1: u32 = 0xd280_0028; // movz x8, #1
        if fps_unlocked {
            if std::ptr::read(p) == MOV_X8_2 {
                std::ptr::write(p, MOV_X8_1);
                sys_icache_invalidate(p as *mut libc::c_void, 4);
                println!("[+] Frame pacer: minimum present interval 1 (up to 60+ fps; --fps=stock for the 30 fps cap)");
            } else if std::ptr::read(p) == MOV_X8_1 {
                println!("[+] Frame pacer: minimum present interval already 1 (patched by a mod)");
            } else {
                println!("[!] Frame pacer: unexpected instruction {:#010x} at guest+0xea87e0; fps unlock not applied", std::ptr::read(p));
            }
        } else {
            println!("[+] Frame pacer: stock 30 fps cap active (minimum present interval 2)");
        }
        if fps_unlocked && crate::settings::get().totk.v1_1_2.dynamic_fps {
            dynamic_fps::install(base, mem_space);
        }
    }

    // 2. Dynamic aspect ratio (unless --dynamic-aspect-ratio=false): the game renders at the screen's shape (aspect.rs,
    //    hud.rs; narrower than 16:9 it sees more vertically) and follows it when it changes (a resized window); otherwise
    //    its 16:9 picture is letterboxed. It starts at --launch-aspect, else 16:9.
    if crate::settings::get().totk.v1_1_2.dynamic_aspect_ratio {
        let a = crate::settings::get().display.launch_aspect;
        let ratio = a.map_or(16.0 / 9.0, |a| a.ratio());
        match aspect::apply(base, ratio) {
            Ok(()) => {
                match a {
                    Some(a) => println!("[+] Aspect: the game renders at the screen's shape, starting at {a}"),
                    None => println!("[+] Aspect: the game renders at the screen's shape"),
                }
                install_hud_hooks(base, mem_space);
            }
            Err(e) => println!("[!] Dynamic aspect ratio not applied ({e}); the picture stays 16:9"),
        }
        sys_icache_invalidate(base as *mut libc::c_void, 0x0480_0000);
    }

    // 3. --journal-occl-reads (diagnostic): journal every call of the occlusion query read (main+0x10c4d44, which
    // takes its query object in x0) with the state it reads (metal::journal_occl_read). The entry instruction branches
    // to a wrapper in the spare words of an HLE trampoline: the displaced stp, the logger call (x0 saved), back.
    if crate::settings::get().totk.diagnostics.journal_occl_reads {
        const SITE: usize = 0x10c4d44;
        const STP_X29_X30_M40: u32 = 0xa9bc7bfd; // stp x29, x30, [sp, #-0x40]!
        let site = (base + SITE) as *mut u32;
        let tramp =
            mem_space.make_hle_trampoline(crate::hle!(crate::metal::journal_occl_read(_))) as usize;
        let wrap = tramp + 40; // words 10..14 of the 16-word entry (make_hle_trampoline uses 0..9)
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
        let wrap =
            mem_space.make_hle_trampoline(crate::hle!(crate::nvn::cmd::note_drawable(_, _, _))) as usize; // its words: the wrapper
        let post =
            mem_space.make_hle_trampoline(crate::hle!(crate::nvn::cmd::note_drawable(_, _, _))) as usize; // after the call
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
                sys_icache_invalidate(pw as *mut libc::c_void, 64);
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
}

/// Calls hud::calculate(layout) at the entry of each layout calculation: the entry branches to a wrapper (a trampoline
/// entry's words) that keeps the arguments, makes the call, runs the displaced instruction and returns.
#[cfg(nx_native)]
unsafe fn install_hud_hooks(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    use hud::{CALC_ENTRY, CALC_SITES};
    let tramp = mem_space.make_hle_trampoline(crate::hle!(hud::calculate(_))) as usize;
    let b = |from: usize, to: usize, link: bool| -> Option<u32> {
        let d = to as i64 - from as i64;
        (d.abs() < 1 << 27)
            .then(|| if link { 0x9400_0000 } else { 0x1400_0000 } | ((d >> 2) as u32 & 0x03ff_ffff))
    };
    for s in CALC_SITES {
        let site = base + s;
        let wrap = mem_space.make_hle_trampoline(crate::hle!(hud::calculate(_))) as usize;
        match (
            std::ptr::read(site as *const u32),
            b(site, wrap, false),
            b(wrap + 8, tramp, true),
            b(wrap + 28, site + 4, false),
        ) {
            (CALC_ENTRY, Some(to_wrap), Some(call), Some(back)) => {
                let code: [u32; 8] = [
                    0xa9be07e0, // stp x0, x1, [sp, #-0x20]!
                    0xa9017be2, // stp x2, x30, [sp, #0x10]
                    call,       // bl <hud::calculate trampoline>
                    0xa9417be2, // ldp x2, x30, [sp, #0x10]
                    0xa8c207e0, // ldp x0, x1, [sp], #0x20
                    0xd503201f, // nop
                    CALC_ENTRY, // sub sp, sp, #0x60
                    back,       // b site+4
                ];
                for (k, c) in code.iter().enumerate() {
                    std::ptr::write((wrap as *mut u32).add(k), *c);
                }
                sys_icache_invalidate(wrap as *mut libc::c_void, 64);
                std::ptr::write(site as *mut u32, to_wrap);
                sys_icache_invalidate(site as *mut libc::c_void, 4);
            }
            (w, ..) => println!(
                "[!] HUD hook not installed (main+{s:#x} = {w:#010x}, or out of branch range)"
            ),
        }
    }
}
