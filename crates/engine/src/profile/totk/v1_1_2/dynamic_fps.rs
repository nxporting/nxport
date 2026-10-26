// Core dynamic delta-time and frame-pacing hooks for Tears of the Kingdom 1.1.2.
//
// In TotK 1.1.2, the native game engine is hardcoded to a 30 FPS timestep (frame interval 2).
// When the present interval is unlocked to 1 (60+ FPS), the game runs at 2x speed unless
// delta-time is scaled dynamically.
//
// This module provides:
// 1. Frame interval hook at main + 0x008f6670: sets target vblanks to 1.
// 2. Dynamic delta-time hook at main + 0x007f7a58: measures real elapsed time between frames
//    using the system tick and scales the delta time and game speed factor dynamically.
// 3. Default frame interval patch at main + 0x021bd514: sets initial frame interval from 2 to 1.

use std::sync::atomic::{AtomicU64, Ordering};

static LAST_TICK: AtomicU64 = AtomicU64::new(0);

const SITE_INTERVAL: usize = 0x008f6670;
const SITE_CALC: usize = 0x007f7a58;
const SITE_DEFAULT: usize = 0x021bd514;

pub extern "C" fn on_frame_pacer(pacer: usize) {
    if pacer == 0 {
        return;
    }
    unsafe {
        // Force target vblanks to 1
        *((pacer + 0x14) as *mut u32) = 1;
        *((pacer + 0x18) as *mut u64) = 1;
    }
}

pub extern "C" fn on_calc_delta_time(timing_mgr: usize) {
    if timing_mgr == 0 {
        return;
    }
    let now = crate::horizon::hle::hle_nn_os_GetSystemTick();
    let freq = unsafe { crate::horizon::hle::hle_nn_os_GetSystemTickFrequency() } as f32;
    let last = LAST_TICK.swap(now, Ordering::Relaxed);
    let dt = if last == 0 || now <= last {
        1.0 / 60.0
    } else {
        ((now - last) as f32 / freq).clamp(1.0 / 120.0, 1.0 / 20.0)
    };
    let speed_factor = dt * 30.0;
    unsafe {
        let p = timing_mgr as *mut f32;
        *p.add(0) = speed_factor;
        *p.add(1) = speed_factor;
        *p.add(2) = speed_factor; // [timing_mgr, #8]
        *p.add(3) = dt;           // [timing_mgr, #0xc]
    }
}

#[cfg(nx_native)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    extern "C" {
        fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
    }

    let b = |from: usize, to: usize, link: bool| -> Option<u32> {
        let d = to as i64 - from as i64;
        (d.abs() < 1 << 27).then(
            || if link { 0x9400_0000 } else { 0x1400_0000 } | ((d >> 2) as u32 & 0x03ff_ffff),
        )
    };

    // 1. Default frame interval patch at main + 0x021bd514:
    //    0x52800048 (mov w8, #2) -> 0x52800028 (mov w8, #1)
    let p_default = (base + SITE_DEFAULT) as *mut u32;
    const MOV_W8_2: u32 = 0x52800048;
    const MOV_W8_1: u32 = 0x52800028;
    if std::ptr::read(p_default) == MOV_W8_2 {
        std::ptr::write(p_default, MOV_W8_1);
        sys_icache_invalidate(p_default as *mut libc::c_void, 4);
        println!("[+] Dynamic FPS: default frame interval patched from 2 to 1 (main+{:#x})", SITE_DEFAULT);
    }

    // 2. Frame pacer hook at main + 0x008f6670:
    //    Entry: sub sp, sp, #0x60 (0xd10183ff)
    const SUB_SP_60: u32 = 0xd10183ff;
    let site_pacer = (base + SITE_INTERVAL) as *mut u32;
    let tramp_pacer = mem_space.make_hle_trampoline(crate::hle!(on_frame_pacer(_))) as usize;
    let wrap_pacer = mem_space.make_hle_trampoline(crate::hle!(on_frame_pacer(_))) as usize;
    match (
        std::ptr::read(site_pacer),
        b(site_pacer as usize, wrap_pacer, false),
        b(wrap_pacer + 8, tramp_pacer, true),
        b(wrap_pacer + 28, site_pacer as usize + 4, false),
    ) {
        (SUB_SP_60, Some(to_wrap), Some(call), Some(back)) => {
            let code: [u32; 8] = [
                0xa9be07e0, // stp x0, x1, [sp, #-0x20]!
                0xa9017be2, // stp x2, x30, [sp, #0x10]
                call,       // bl <on_frame_pacer trampoline>
                0xa9417be2, // ldp x2, x30, [sp, #0x10]
                0xa8c207e0, // ldp x0, x1, [sp], #0x20
                0xd503201f, // nop
                SUB_SP_60,  // sub sp, sp, #0x60
                back,       // b site+4
            ];
            for (k, c) in code.iter().enumerate() {
                std::ptr::write((wrap_pacer as *mut u32).add(k), *c);
            }
            sys_icache_invalidate(wrap_pacer as *mut libc::c_void, 64);
            std::ptr::write(site_pacer, to_wrap);
            sys_icache_invalidate(site_pacer as *mut libc::c_void, 4);
            println!("[+] Dynamic FPS: frame pacer hook installed at main+{:#x}", SITE_INTERVAL);
        }
        (w, ..) => println!("[!] Dynamic FPS: frame pacer hook not installed (expected {:#x}, got {:#x})", SUB_SP_60, w),
    }

    // 3. Dynamic delta-time hook at main + 0x007f7a58:
    //    Site instruction: stp s0, s1, [x0, #8] (0x2d010400)
    //    At site: x0 points to timing controller, x8 has adrp result.
    const STP_S0_S1: u32 = 0x2d010400;
    let site_calc = (base + SITE_CALC) as *mut u32;
    let tramp_calc = mem_space.make_hle_trampoline(crate::hle!(on_calc_delta_time(_))) as usize;
    let wrap_calc = mem_space.make_hle_trampoline(crate::hle!(on_calc_delta_time(_))) as usize;
    match (
        std::ptr::read(site_calc),
        b(site_calc as usize, wrap_calc, false),
        b(wrap_calc + 8, tramp_calc, true),
        b(wrap_calc + 20, site_calc as usize + 4, false),
    ) {
        (STP_S0_S1, Some(to_wrap), Some(call), Some(back)) => {
            let code: [u32; 6] = [
                0xa9be07e0, // stp x0, x1, [sp, #-0x20]!
                0xa9017be8, // stp x8, x30, [sp, #0x10]
                call,       // bl <on_calc_delta_time trampoline>
                0xa9417be8, // ldp x8, x30, [sp, #0x10]
                0xa8c207e0, // ldp x0, x1, [sp], #0x20
                back,       // b site+4
            ];
            for (k, c) in code.iter().enumerate() {
                std::ptr::write((wrap_calc as *mut u32).add(k), *c);
            }
            sys_icache_invalidate(wrap_calc as *mut libc::c_void, 64);
            std::ptr::write(site_calc, to_wrap);
            sys_icache_invalidate(site_calc as *mut libc::c_void, 4);
            println!("[+] Dynamic FPS: delta-time calculation hook installed at main+{:#x}", SITE_CALC);
        }
        (w, ..) => println!("[!] Dynamic FPS: delta-time calc hook not installed (expected {:#x}, got {:#x})", STP_S0_S1, w),
    }
}
