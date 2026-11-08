// Sideways mouse movement into the camera's yaw (mouse look, host/input.rs).
//
// The camera keeps its position and the point it looks at; a function turns their difference into a distance, a pitch
// and a yaw in degrees (1.0.0: main+0xb8a1cc, called from main+0xb8a024 with x0 = where to store them and x1 = the
// vector), and the camera stores them (+0x404..0x40c of the object at camera+0x20). The next frame the turn function
// (main+0x9ab570) loads that yaw first and places the camera from it. The call is redirected to `camera_angles` below,
// which computes the same three values the same way (the game's two constants are read from its own code) and adds the
// mouse's sideways movement to the yaw: the game then turns the camera by exactly that, by its own rules (a mode that
// does not place the camera from the yaw ignores it). Nothing else of the game's code changes.
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
    crate::host::input::set_yaw_sink();
    println!("[+] TotK: sideways mouse movement turns the camera's yaw directly (main+{at:#x}, profile/totk/camera_angles.rs)");
}

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
    let yaw = wrap(x.atan2(z) * scale + crate::host::input::take_mouse_yaw());
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
    fn angles_wrap_like_the_game() {
        assert_eq!(wrap(179.5), 179.5);
        assert_eq!(wrap(180.0), -180.0);
        assert!((wrap(-190.0) - 170.0).abs() < 1e-4);
        assert!((wrap(540.0) - -180.0).abs() < 1e-4);
    }
}
