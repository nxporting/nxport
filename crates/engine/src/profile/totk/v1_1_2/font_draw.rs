// nn::font::RectDrawer's per-string draw (main+0x3a2a4, called once or twice by RectDrawer::Draw at main+0x3a19c):
// records the font program's draws straight into the NVN command buffer instead of running the game's nn::gfx calls.
//
//   x0 RectDrawer*, x1 nn::gfx::CommandBuffer*, x2 vertex buffer data (8 texture-use records + count),
//   w3 content flags, x4 GpuAddress* constant buffer, x5 GpuAddress* per-character records, x6 pipelines (TotK: null)
//
// --native-font-draws records the same commands the function would; --verify-font-draws lets the game record them and
// compares, call by call, with what the native path would have recorded.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::nvn::cmd::*;

pub const SITE: usize = 0x0003a2a4;
const SITE_INSTRUCTION: u32 = 0xd10343ff; // sub sp, sp, #0xd0

// RectDrawer
const RD_RES_SHADER_FILE: usize = 0x08;
const RD_CODE_TYPE: usize = 0x10;
const RD_VS_UNIFORM_SLOTS: usize = 0x14;
const RD_VS_CHAR_SLOTS: usize = 0x2c;
const RD_PS_UNIFORM_SLOTS: usize = 0x44;
const RD_BW_SLOTS: usize = 0x5c;
const RD_TEXTURE_SLOTS: usize = 0x74;
const RD_VERTEX_STATES: usize = 0x90; // 6 x 0x20: +4 stream count, +8 attribute count, +0x10 streams, +0x18 attributes
const RD_VERTEX_BUFFER: usize = 0x270; // nn::gfx::Buffer (0x48): +8 NVNbuffer*
const RD_INDEX_BUFFER: usize = 0x2b8;
const RD_BW_ENABLED_BUFFER: usize = 0x300;
const RD_BW_DISABLED_BUFFER: usize = 0x348;
const RD_SAMPLER_SLOT: usize = 0x408;

// Content flags (w3) and texture-use flags
const INVISIBLE_BORDER: u32 = 1 << 0;
const SHADOW: u32 = 1 << 1;
const PER_CHARACTER_TRANSFORM: u32 = 1 << 2;
const USE_BW_INTERPOLATION: u32 = 1 << 0;
const USE_BORDER: u32 = 1 << 1;

const VERTEX_STRIDE: u64 = 28;
const CONSTANT_BUFFER_SIZE: usize = 128;
const CHAR_RECORD_SIZE: usize = 64;
const CHAR_RECORD_TRANSFORM_SIZE: usize = 96;
const TEXTURE_USE_MAX: usize = 8;

// NVN
const STAGE_VERTEX: i32 = 0;
const STAGE_FRAGMENT: i32 = 1;
const TRIANGLES: i32 = 4;
const UNSIGNED_SHORT: i32 = 1;

static VERIFIED: AtomicU64 = AtomicU64::new(0);
static MISMATCHED: AtomicU64 = AtomicU64::new(0);
static EMPTY: AtomicU64 = AtomicU64::new(0);

unsafe fn rd<T: Copy>(addr: usize) -> T {
    std::ptr::read_unaligned(addr as *const T)
}

/// The NVNprogram of shader variation `v`: res shader file -> container (first block) -> variation -> program of the
/// drawer's code type -> nn::gfx::Shader -> NVNprogram*, and the shader's NVN stage bits.
unsafe fn program(drawer: usize, v: usize) -> (usize, i32) {
    let file: usize = rd(drawer + RD_RES_SHADER_FILE);
    let container = file + rd::<u16>(file + 0x16) as usize;
    let variations: usize = rd(container + 0x20);
    let code_type: i32 = rd(drawer + RD_CODE_TYPE);
    // Binary, intermediate, source
    let field = [0x10, 0x08, 0x00][code_type.clamp(0, 2) as usize];
    let res_program: usize = rd(variations + v * 0x40 + field);
    let shader: usize = rd(res_program + 0x68);
    (rd(shader + 8), rd(shader + 4))
}

unsafe fn buffer_address(gfx_buffer: usize) -> u64 {
    crate::nvn::api::hle_nvnBufferGetAddress(rd::<usize>(gfx_buffer + 8) as *const _)
}

/// Records what the function records into `cmd` (an NVNcommandBuffer).
unsafe fn record(drawer: usize, cmd: usize, vb_data: usize, flags: u32, cb_addr: u64, chars_addr: u64) {
    let slot = |base: usize, v: usize| rd::<i32>(drawer + base + v * 4);
    let uses = rd::<i32>(vb_data + TEXTURE_USE_MAX * 16).clamp(0, TEXTURE_USE_MAX as i32) as usize;
    let mut chars = chars_addr;
    let mut last = usize::MAX;
    for u in 0..uses {
        let tex_obj: usize = rd(vb_data + u * 16);
        let count: u32 = rd(vb_data + u * 16 + 8);
        let use_flags: u32 = rd(vb_data + u * 16 + 12);
        if count == 0 {
            continue;
        }
        let border = use_flags & USE_BORDER != 0;
        let mut v = match (border, flags & INVISIBLE_BORDER != 0) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2,
        };
        if flags & PER_CHARACTER_TRANSFORM != 0 {
            v += 3;
        }
        if v != last {
            let (prog, stages) = program(drawer, v);
            hle_nvnCommandBufferBindProgram(cmd, prog, stages);
            let vs = drawer + RD_VERTEX_STATES + v * 0x20;
            hle_nvnCommandBufferBindVertexAttribState(cmd, rd(vs + 8), rd(vs + 0x18));
            hle_nvnCommandBufferBindVertexStreamState(cmd, rd(vs + 4), rd(vs + 0x10));
            hle_nvnCommandBufferBindVertexBuffer(cmd, 0, buffer_address(drawer + RD_VERTEX_BUFFER), (VERTEX_STRIDE * 4) as usize);
            hle_nvnCommandBufferBindUniformBuffer(cmd, STAGE_VERTEX, slot(RD_VS_UNIFORM_SLOTS, v), cb_addr, CONSTANT_BUFFER_SIZE);
            hle_nvnCommandBufferBindUniformBuffer(cmd, STAGE_FRAGMENT, slot(RD_PS_UNIFORM_SLOTS, v), cb_addr, CONSTANT_BUFFER_SIZE);
            last = v;
        }
        let record_size = if flags & PER_CHARACTER_TRANSFORM != 0 { CHAR_RECORD_TRANSFORM_SIZE } else { CHAR_RECORD_SIZE };
        let instances = count as usize * if flags & SHADOW != 0 { 2 } else { 1 };
        let size = record_size * instances;
        hle_typed_cb_BindStorageBuffer(cmd, STAGE_VERTEX, slot(RD_VS_CHAR_SLOTS, v), chars, size);
        chars += size as u64;
        let handle = crate::nvn::state::hle_nvnDeviceGetTextureHandle(
            std::ptr::null(),
            rd::<u64>(tex_obj + 0x20) as i32,
            rd::<u64>(drawer + RD_SAMPLER_SLOT) as i32,
        );
        hle_nvnCommandBufferBindTexture(cmd, STAGE_FRAGMENT, slot(RD_TEXTURE_SLOTS, v), handle);
        let bw = if use_flags & USE_BW_INTERPOLATION != 0 { RD_BW_ENABLED_BUFFER } else { RD_BW_DISABLED_BUFFER };
        hle_nvnCommandBufferBindUniformBuffer(cmd, STAGE_FRAGMENT, slot(RD_BW_SLOTS, v), buffer_address(drawer + bw), 4);
        hle_typed_cb_DrawElementsInstanced(
            cmd,
            TRIANGLES,
            UNSIGNED_SHORT,
            6,
            buffer_address(drawer + RD_INDEX_BUFFER),
            0,
            0,
            instances as i32,
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn on_rect_drawer_add_draw(
    drawer: usize,
    gfx_cmd: usize,
    vb_data: usize,
    flags: u32,
    cb: usize,
    chars: usize,
    pipelines: usize,
) -> u32 {
    let d = &crate::settings::get().totk.diagnostics;
    // Pipelines given by the caller carry their own render state: leave those to the game.
    if pipelines != 0 {
        return 0;
    }
    let cmd: usize = rd(gfx_cmd + 0xb0);
    let (cb_addr, chars_addr) = (rd::<u64>(cb), rd::<u64>(chars));
    if d.verify_font_draws {
        if let Some(left) = crate::nvn::api::capture_finish() {
            report(&left.0, &left.1);
        }
        let expected = crate::nvn::api::capture_dry(|| record(drawer, cmd, vb_data, flags, cb_addr, chars_addr));
        crate::nvn::api::capture_watch(expected, report);
        return 0;
    }
    record(drawer, cmd, vb_data, flags, cb_addr, chars_addr);
    1
}

fn report(expected: &[String], got: &[String]) {
    if expected == got {
        // Calls that record nothing are not compared: the game's commands after them could belong to anything.
        if expected.is_empty() {
            EMPTY.fetch_add(1, Ordering::Relaxed);
        }
        let n = VERIFIED.fetch_add(1, Ordering::Relaxed) + 1;
        if n.is_power_of_two() {
            println!(
                "[font-draw] {n} calls identical ({} recorded nothing; {} mismatched)",
                EMPTY.load(Ordering::Relaxed),
                MISMATCHED.load(Ordering::Relaxed)
            );
        }
        return;
    }
    let n = MISMATCHED.fetch_add(1, Ordering::Relaxed);
    if n < 8 {
        println!("[font-draw] mismatch #{n}:");
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            println!("[font-draw]   {} native: {e}", if e == g { " " } else { "!" });
            if e != g {
                println!("[font-draw]     game:   {g}");
            }
        }
    }
}

extern "C" {
    fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
}

/// The entry branches to a wrapper that calls on_rect_drawer_add_draw with the arguments, then returns at once when it
/// returned nonzero, else runs the displaced instruction and continues in the function.
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    let site = (base + SITE) as *mut u32;
    let actual = std::ptr::read(site);
    if actual != SITE_INSTRUCTION {
        println!("[!] Font draw hook not installed: main+{SITE:#x} = {actual:#010x}");
        return;
    }
    let tramp = mem_space.make_hle_trampoline(crate::hle!(on_rect_drawer_add_draw(_, _, _, _, _, _, _))) as usize;
    let wrap = mem_space.make_hle_trampoline(crate::hle!(on_rect_drawer_add_draw(_, _, _, _, _, _, _))) as usize;
    let b = |from: usize, to: usize, link: bool| -> Option<u32> {
        let d = to as i64 - from as i64;
        (d.abs() < 1 << 27).then(|| (if link { 0x9400_0000 } else { 0x1400_0000 }) | ((d >> 2) as u32 & 0x03ff_ffff))
    };
    let (Some(to_wrap), Some(call), Some(back)) =
        (b(site as usize, wrap, false), b(wrap + 0x14, tramp, true), b(wrap + 0x34, site as usize + 4, false))
    else {
        println!("[!] Font draw hook not installed: wrapper out of branch range");
        return;
    };
    let code: [u32; 16] = [
        0xa9bb7bfd,       // stp x29, x30, [sp, #-0x50]!
        0xa90107e0,       // stp x0, x1, [sp, #0x10]
        0xa9020fe2,       // stp x2, x3, [sp, #0x20]
        0xa90317e4,       // stp x4, x5, [sp, #0x30]
        0xa9041fe6,       // stp x6, x7, [sp, #0x40]
        call,             // bl on_rect_drawer_add_draw
        0x35000100,       // cbnz w0, +0x20 (handled)
        0xa9441fe6,       // ldp x6, x7, [sp, #0x40]
        0xa94317e4,       // ldp x4, x5, [sp, #0x30]
        0xa9420fe2,       // ldp x2, x3, [sp, #0x20]
        0xa94107e0,       // ldp x0, x1, [sp, #0x10]
        0xa8c57bfd,       // ldp x29, x30, [sp], #0x50
        SITE_INSTRUCTION, // the displaced instruction
        back,             // b site+4
        0xa8c57bfd,       // handled: ldp x29, x30, [sp], #0x50
        0xd65f03c0,       // ret
    ];
    let w = wrap as *mut u32;
    for (k, c) in code.iter().enumerate() {
        std::ptr::write(w.add(k), *c);
    }
    sys_icache_invalidate(w as *mut libc::c_void, 64);
    std::ptr::write(site, to_wrap);
    sys_icache_invalidate(site as *mut libc::c_void, 4);
    println!("[+] Font draw hook installed at main+{SITE:#x}");
}
