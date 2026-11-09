// Mouse movement into the camera's yaw and pitch (mouse look, host/input.rs).
//
// The camera keeps its position and the point it looks at; a function turns their difference into a distance, a pitch
// and a yaw in degrees (1.0.0: main+0xb8a1cc, called from main+0xb8a024 with x0 = where to store them and x1 = the
// vector), and the camera stores them (+0x404..0x40c of the object at camera+0x20). The next frame the turn function
// (main+0x9ab570) loads that yaw first and places the camera from it. The call is redirected to `camera_angles` below,
// which computes the same three values the same way (the game's two constants are read from its own code) and adds the
// mouse's sideways movement to the yaw: the game then turns the camera by exactly that, by its own rules (a mode that
// does not place the camera from the yaw ignores it).
//
// The pitch the camera is placed with is rebuilt each frame from the camera's +0x108. In its automatic mode (+0x180 = 3)
// that is eased toward a default; any right stick input switches it to manual (4), where +0x108 is the pitch the stick
// adds to and it stays where it is put. The game's test for that input (1.0.0 main+0x9abb28: fcmp s14, #0.0; b.gt) is
// rewritten to ask `camera_input`, which answers as the game would (s14, the stick's strength, above 0) or yes when the
// mouse has moved: the game then applies all its own conditions and picks the mode. The mouse's vertical degrees are
// added to +0x108 when the game stores its angles, if the camera is in manual mode; otherwise dropped, as a stick would
// turn nothing there. Both read the game's registers from the lifted runtime (the camera in x19, s14), so the pitch part
// is installed where guest code is lifted only (Windows; macOS has no mouse look yet). Nothing else of the game's code
// changes.
//
// The call is found by the instructions around it, not by address, so other builds compiled the same way get it too.
// Like aspect.rs, the rewritten call enters an HLE trampoline (no code of our own in guest memory), the same at lift and
// run time.

use std::sync::atomic::{AtomicU32, Ordering};

/// Around the call: stp s0, s2, [x29, #-0x10]; fsub s0, s1, s3; stur s0, [x29, #-8]; bl (the function);
/// ldr x9, [sp, #8]; add x8, x21, #0x404.
const AROUND: [u32; 6] = [0x2d3e_0ba0, 0x1e23_3820, 0xbc1f_83a0, 0x9400_0000, 0xf940_07e9, 0x9110_12a8];
const CALL: usize = 3;
/// The function's first words: sub sp, sp, #0x40; str d10, [sp, #0x10]; str x19, [sp, #0x38]; stp d9, d8, [sp, #0x18].
const CALLEE: [u32; 4] = [0xd101_03ff, 0xfd00_0bea, 0xf900_1ff3, 0x6d01_a3e9];
/// Where in the function its two constants are loaded: adrp x8 then ldr s1 (the pitch offset), adrp x8 then ldr s10
/// (degrees per radian).
const CONSTANTS_AT: usize = 0x88;

/// The camera-input test: ldrb w8, [x27, #0x141]; tbnz w8, #5, ...; fcmp s14, #0.0; b.gt (manual mode).
#[cfg_attr(nx_native, allow(dead_code))]
const DECISION: [(u32, u32); 4] = [(0x3945_0768, !0), (0x3728_0008, 0xfff8_001f), (0x1e20_21c8, !0), (0x5400_000c, 0xff00_001f)];
#[cfg_attr(nx_native, allow(dead_code))]
const TEST: usize = 2;
/// The camera's pitch state and mode.
const PITCH: usize = 0x108;
const MODE: usize = 0x180;
const MANUAL: u32 = 4;

static OFFSET: AtomicU32 = AtomicU32::new(0);
static SCALE: AtomicU32 = AtomicU32::new(0);

fn word(text: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(text[at..at + 4].try_into().unwrap())
}

/// The call's offset in `text`, if found exactly once with the expected function behind it.
fn find(text: &[u8]) -> Option<usize> {
    let mut found = None;
    let n = text.len() / 4;
    for i in 0..n.saturating_sub(AROUND.len()) {
        let ok = AROUND.iter().enumerate().all(|(k, &w)| {
            let v = word(text, 4 * (i + k));
            if k == CALL {
                v & 0xfc00_0000 == w
            } else {
                v == w
            }
        });
        if ok {
            if found.is_some() {
                return None;
            }
            found = Some(4 * (i + CALL));
        }
    }
    let at = found?;
    let callee = callee_of(at, word(text, at))?;
    (callee + 16 <= text.len() && CALLEE.iter().enumerate().all(|(k, &w)| word(text, callee + 4 * k) == w)).then_some(at)
}

/// The offset in `text` of the camera-input test's fcmp, if found exactly once.
#[cfg_attr(nx_native, allow(dead_code))]
fn find_decision(text: &[u8]) -> Option<usize> {
    let mut found = None;
    for i in 0..(text.len() / 4).saturating_sub(DECISION.len()) {
        if DECISION.iter().enumerate().all(|(k, &(w, m))| word(text, 4 * (i + k)) & m == w) {
            if found.is_some() {
                return None;
            }
            found = Some(4 * (i + TEST));
        }
    }
    found
}

/// The target of the bl `w` at `at`.
fn callee_of(at: usize, w: u32) -> Option<usize> {
    let imm = ((w & 0x03ff_ffff) as i32) << 6 >> 4;
    usize::try_from(at as i64 + imm as i64).ok()
}

/// The f32 an `adrp xN` at `at` followed by `ldr sM, [xN, #imm]` loads, in `text` at `base` (data reached through
/// guest memory).
unsafe fn adrp_ldr_f32(base: usize, text: &[u8], at: usize) -> Option<f32> {
    let (adrp, ldr) = (word(text, at), word(text, at + 4));
    if adrp & 0x9f00_0000 != 0x9000_0000 || ldr & 0xffc0_0000 != 0xbd40_0000 {
        return None;
    }
    let lo = ((adrp >> 29) & 3) as i64;
    let hi = (((adrp >> 5) & 0x7ffff) as i64) << 45 >> 43;
    let page = ((base + at) as i64 & !0xfff) + ((hi | lo) << 12);
    let addr = page + (((ldr >> 10) & 0xfff) as i64) * 4;
    Some(std::ptr::read(addr as *const f32))
}

/// Rewrites the call (main's `text`, loaded at `base`) to enter `camera_angles`. Nothing happens unless the code and
/// its constants are as expected.
#[cfg(nx_runtime)]
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    let text = std::slice::from_raw_parts(base as *const u8, mem_space.text_size);
    let Some(at) = find(text) else {
        return println!("[!] TotK: the camera's angle code was not found; mouse aiming steers by speed only");
    };
    let callee = callee_of(at, word(text, at)).unwrap();
    let (Some(offset), Some(scale)) =
        (adrp_ldr_f32(base, text, callee + CONSTANTS_AT), adrp_ldr_f32(base, text, callee + CONSTANTS_AT + 8))
    else {
        return println!("[!] TotK: unexpected code in the camera's angle function; mouse aiming steers by speed only");
    };
    if (scale - 180.0 / std::f32::consts::PI).abs() > 1e-3 || !offset.is_finite() {
        return println!("[!] TotK: unexpected constants in the camera's angle function ({offset}, {scale})");
    }
    OFFSET.store(offset.to_bits(), Ordering::Relaxed);
    SCALE.store(scale.to_bits(), Ordering::Relaxed);
    let tramp = mem_space.make_hle_trampoline(crate::hle!(camera_angles(_, _))) as usize;
    let d = tramp as i64 - (base + at) as i64;
    if d.abs() >= 1 << 27 {
        return println!("[!] TotK: the camera hook is out of branch range");
    }
    std::ptr::write((base + at) as *mut u32, 0x9400_0000 | ((d >> 2) as u32 & 0x03ff_ffff));
    crate::os::flush_icache(base + at, 4);
    #[cfg(not(nx_native))]
    let pitch = install_decision(base, text, mem_space);
    #[cfg(nx_native)]
    let pitch = false;
    crate::host::input::set_angle_sink(pitch);
    let what = if pitch { "the camera's yaw and pitch" } else { "the camera's yaw" };
    println!("[+] TotK: mouse movement turns {what} directly (main+{at:#x}, profile/totk/camera_angles.rs)");
}

/// Rewrites the camera-input test to ask `camera_input`: bl to it, then cbnz w0 to the test's own target.
#[cfg(all(nx_runtime, not(nx_native)))]
unsafe fn install_decision(base: usize, text: &[u8], mem_space: &mut crate::guest::loader::GuestMemorySpace) -> bool {
    let Some(at) = find_decision(text) else {
        println!("[!] TotK: the camera's input test was not found; mouse pitch steers by speed only");
        return false;
    };
    let bgt = word(text, at + 4);
    let tramp = mem_space.make_hle_trampoline(crate::hle!(camera_input())) as usize;
    let d = tramp as i64 - (base + at) as i64;
    if d.abs() >= 1 << 27 {
        println!("[!] TotK: the camera input hook is out of branch range");
        return false;
    }
    std::ptr::write((base + at) as *mut u32, 0x9400_0000 | ((d >> 2) as u32 & 0x03ff_ffff));
    // b.gt and cbnz w0 keep their 19-bit offset in the same bits.
    std::ptr::write((base + at + 4) as *mut u32, 0x3500_0000 | (bgt & 0x00ff_ffe0));
    crate::os::flush_icache(base + at, 8);
    true
}

/// The camera-input test: the game's own (s14, the right stick's strength, above 0) or mouse movement waiting.
pub unsafe extern "C" fn camera_input() -> u64 {
    let cpu = crate::lifted::current();
    let stick = !cpu.is_null() && f32::from_bits((*cpu).v[14] as u32) > 0.0;
    let mouse = crate::host::input::take_mouse_input();
    if mouse {
        SEEN.store(true, Ordering::Release);
    }
    (stick || mouse) as u64
}

/// The game has tested for camera input since mouse movement arrived (its answer is in the camera's mode).
static SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// To [-180, 180), as the game's code wraps them.
fn wrap(a: f32) -> f32 {
    if (-180.0..180.0).contains(&a) {
        a
    } else {
        a - 360.0 * ((a + 180.0) / 360.0).floor()
    }
}

/// The distance, pitch and yaw of `v` (x, y, z: the camera's position minus the point it looks at) into `out`, as the
/// game's function computes them; yaw and pitch also go to mouse aiming.
pub unsafe extern "C" fn camera_angles(out: *mut f32, v: *const f32) -> usize {
    let (x, y, z) = (*v, *v.add(1), *v.add(2));
    let flat = (x * x) as f64 + (z * z) as f64;
    let all = flat + (y * y) as f64;
    let h = if flat > 0.0 { flat.sqrt() as f32 } else { 0.0 };
    let dist = if all > 0.0 { all.sqrt() as f32 } else { 0.0 };
    let (offset, scale) = (f32::from_bits(OFFSET.load(Ordering::Relaxed)), f32::from_bits(SCALE.load(Ordering::Relaxed)));
    let pitch = wrap((offset - h.atan2(y)) * scale);
    // The game builds the next frame's camera from this yaw (the turn function main+0x9ab570 loads it first and places
    // the camera from it): sideways mouse movement is added here.
    let yaw = wrap(x.atan2(z) * scale + crate::host::input::take_mouse_angle(0));
    // The pitch state (camera +0x108, the camera in x19 of the caller), in manual mode only.
    // In manual mode added; else kept until the game has tested the input, and dropped if it then stayed out of manual
    // mode (as a stick would turn nothing there).
    let dp = crate::host::input::take_mouse_angle(1);
    let cpu = crate::lifted::current();
    if dp != 0.0 && !cpu.is_null() {
        let camera = (*cpu).x[19] as usize;
        if std::ptr::read((camera + MODE) as *const u32) == MANUAL {
            let p = (camera + PITCH) as *mut f32;
            std::ptr::write(p, wrap(std::ptr::read(p) + dp));
            SEEN.store(false, Ordering::Release);
        } else if !SEEN.swap(false, Ordering::AcqRel) {
            crate::host::input::keep_mouse_angle(1, dp);
        }
    }
    *out = dist;
    *out.add(1) = pitch;
    *out.add(2) = yaw;
    out as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_with(at: usize, callee: usize) -> Vec<u8> {
        let mut t = vec![0u8; callee + 0x200];
        let put = |t: &mut Vec<u8>, a: usize, w: u32| t[a..a + 4].copy_from_slice(&w.to_le_bytes());
        for (k, &w) in AROUND.iter().enumerate() {
            let a = at - 4 * CALL + 4 * k;
            let w = if k == CALL { w | (((callee - at) / 4) as u32 & 0x03ff_ffff) } else { w };
            put(&mut t, a, w);
        }
        for (k, &w) in CALLEE.iter().enumerate() {
            put(&mut t, callee + 4 * k, w);
        }
        t
    }

    #[test]
    fn finds_the_call_by_its_code() {
        let t = text_with(0x40, 0x100);
        assert_eq!(find(&t), Some(0x40));
        assert_eq!(callee_of(0x40, word(&t, 0x40)), Some(0x100));
        // Another function behind the call: not this one.
        let mut other = t.clone();
        other[0x100] ^= 1;
        assert_eq!(find(&other), None);
        // Found twice: ambiguous.
        let mut twice = text_with(0x40, 0x180);
        let first = text_with(0x40, 0x180);
        twice[0x80 - 12..0x80 + 12].copy_from_slice(&first[0x40 - 12..0x40 + 12]);
        assert_eq!(find(&twice), None);
    }

    #[test]
    fn finds_the_camera_input_test() {
        // 1.0.0's words: ldrb w8, [x27, #0x141]; tbnz w8, #5, +0x1774; fcmp s14, #0.0; b.gt +0x1e48.
        let words = [0x3945_0768u32, 0x3728_bb88, 0x1e20_21c8, 0x5400_f22c];
        let mut text = vec![0u8; 0x40];
        for (k, w) in words.iter().enumerate() {
            text[0x10 + 4 * k..0x14 + 4 * k].copy_from_slice(&w.to_le_bytes());
        }
        assert_eq!(find_decision(&text), Some(0x18));
        // The cbnz written in place of b.gt branches to the same target: the same 19-bit offset.
        let cbnz = 0x3500_0000 | (words[3] & 0x00ff_ffe0);
        assert_eq!(cbnz, 0x3500_f220);
        assert_eq!((cbnz >> 5) & 0x7ffff, (words[3] >> 5) & 0x7ffff);
        text[0x18] ^= 1;
        assert_eq!(find_decision(&text), None);
    }

    #[test]
    fn angles_wrap_like_the_game() {
        assert_eq!(wrap(179.5), 179.5);
        assert_eq!(wrap(180.0), -180.0);
        assert!((wrap(-190.0) - 170.0).abs() < 1e-4);
        assert!((wrap(540.0) - -180.0).abs() < 1e-4);
    }
}
