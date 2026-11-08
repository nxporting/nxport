// The right stick past full deflection, for mouse look: a fast mouse turns the camera faster than the stick can.
//
// TotK keeps the right stick at most 1 long in three places: the controller's update clamps it to the unit circle after
// scaling the HID values to floats (1.0.0: main+0x23f92d0); the camera's input step (the function that reads
// QueryPlayerCameraControllerInput's result, main+0x2070b40) maps lengths over 0.995 to exactly 1 in its dead zone
// (main+0xd05480), and clamps to the unit circle after it (main+0xd0533c). Each is one branch. Changed, a stick longer
// than 1 reaches the camera's turn; a real stick never gets past 1, so play with a controller is unchanged. Past 1 the
// dead zone's formula grows with the squared length (measured: a stick of 3 turns the camera 9.15 times as fast as a
// full one, 1 gives 1.01), so mouse look sends the square root of the strength it wants (stick_for), up to
// RIGHT_STICK_MAX (host/input.rs).
//
// The code is found by its instructions, not by address, so other builds compiled the same way get the patch too.

/// How fast mouse look may turn the patched game's camera, in full sticks.
pub const RIGHT_STICK_MAX: f32 = 8.0;

/// The stick length past 1 that turns the camera `t` times as fast as a full stick: the camera's response there is
/// about the square of the length.
fn stick_for(t: f32) -> f32 {
    t.sqrt()
}

const NOP: u32 = 0xd503_201f;

/// What a site's branch becomes.
#[derive(Clone, Copy)]
enum Change {
    /// The clamp is skipped: the branch to the normalising code goes.
    Remove,
    /// The branch to the linear dead zone is always taken.
    Always,
}

/// One site: the words around it (None: any word), the index of the branch in them, and the branch's condition.
struct Site {
    words: &'static [Option<u32>],
    branch: usize,
    cond: u32,
    change: Change,
}

const GT: u32 = 0xc;
const LE: u32 = 0xd;

const SITES: [Site; 3] = [
    // The controller's clamp: the length against s2 = 1.0, then the stores of x and y.
    Site {
        words: &[
            Some(0x1e22_2000), // fcmp  s0, s2
            None,              // b.gt  normalise
            Some(0xbd01_2a69), // str   s9, [x19, #0x128]   right stick x
            Some(0xbd01_2e68), // str   s8, [x19, #0x12c]   right stick y
        ],
        branch: 1,
        cond: GT,
        change: Change::Remove,
    },
    // The camera's dead zone: the length against s2 = 0.995; at or under it the linear mapping, over it exactly 1.
    Site {
        words: &[
            Some(0x1e22_2100), // fcmp  s8, s2
            None,              // b.le  linear
            Some(0x1e20_2008), // fcmp  s0, #0.0
        ],
        branch: 1,
        cond: LE,
        change: Change::Always,
    },
    // The camera's clamp after the dead zone: the squared length against s9 = 1.0.
    Site {
        words: &[
            Some(0x1e20_2868), // fadd  s8, s3, s0          squared length
            Some(0x1e29_2100), // fcmp  s8, s9
            None,              // b.gt  normalise
            Some(0x1e21_c109), // fsqrt s9, s8
            Some(0x1e29_2120), // fcmp  s9, s9
        ],
        branch: 2,
        cond: GT,
        change: Change::Remove,
    },
];

fn word(t: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(t[i * 4..i * 4 + 4].try_into().unwrap())
}

/// `b.<cond>` at any offset.
fn is_b_cond(w: u32, cond: u32) -> bool {
    w & 0xff00_001f == 0x5400_0000 | cond
}

/// The unconditional branch to where the conditional branch `w` goes.
fn always(w: u32) -> u32 {
    let imm19 = (w >> 5) & 0x7ffff;
    let offset = ((imm19 << 13) as i32) >> 13; // sign-extended, in words
    0x1400_0000 | (offset as u32 & 0x03ff_ffff)
}

/// What the site's branch is once changed, given the branch it was.
fn changed(s: &Site, w: u32) -> u32 {
    match s.change {
        Change::Remove => NOP,
        Change::Always => always(w),
    }
}

/// Whether `w` is the site's branch, or what a patch made of it (a game patched at build time runs the patched bytes).
fn is_site_branch(s: &Site, w: u32) -> bool {
    match s.change {
        Change::Remove => is_b_cond(w, s.cond) || w == NOP,
        Change::Always => is_b_cond(w, s.cond) || w & 0xfc00_0000 == 0x1400_0000,
    }
}

/// The word index of the one place the site is found, or None (no match, or several).
fn find(text: &[u8], s: &Site) -> Option<usize> {
    let words = text.len() / 4;
    let mut found = (0..words.saturating_sub(s.words.len() - 1)).filter(|&i| {
        is_site_branch(s, word(text, i + s.branch))
            && s.words.iter().enumerate().all(|(k, w)| w.is_none_or(|w| word(text, i + k) == w))
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// Changes all three sites where each is found exactly once in a module, and lets mouse look past full deflection;
/// with any missing, changes nothing. Code already patched (a build's bytes) only turns the mouse look side on. Returns
/// whether it changed the code. `text` is the module's code as loaded, before it is lifted or run.
pub fn patch(name: &str, text: &mut [u8]) -> bool {
    let mut at = [0usize; 3];
    for (k, s) in SITES.iter().enumerate() {
        let Some(i) = find(text, s) else { return false };
        at[k] = i + s.branch;
    }
    crate::host::input::set_right_stick_range(RIGHT_STICK_MAX, stick_for);
    let mut changes = 0;
    for (s, &i) in SITES.iter().zip(&at) {
        let w = word(text, i);
        if is_b_cond(w, s.cond) {
            text[i * 4..i * 4 + 4].copy_from_slice(&changed(s, w).to_le_bytes());
            changes += 1;
        }
    }
    if changes == 0 {
        return false;
    }
    println!(
        "[+] TotK: {name}: the right stick may pass full deflection (branches at {:#x}, {:#x}, {:#x} changed, \
         profile/totk/camera_stick.rs)",
        at[0] * 4,
        at[1] * 4,
        at[2] * 4
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a site at byte `at`, its branch `b.<cond>` 0x22 words forward.
    fn put(text: &mut [u8], at: usize, s: &Site) {
        for (k, w) in s.words.iter().enumerate() {
            let w = if k == s.branch { 0x5400_0440 | s.cond } else { w.unwrap_or(0x9400_0000) };
            text[at + k * 4..at + k * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
    }

    #[test]
    fn changes_all_three_sites() {
        let mut text = vec![0u8; 0x400];
        let places = [0x40, 0x100, 0x200];
        for (s, &p) in SITES.iter().zip(&places) {
            put(&mut text, p, s);
        }
        let before = text.clone();
        assert!(patch("test", &mut text));
        let branch = |k: usize| places[k] + SITES[k].branch * 4;
        for i in 0..text.len() / 4 {
            let want = match i * 4 {
                b if b == branch(0) || b == branch(2) => NOP,
                b if b == branch(1) => 0x1400_0022,
                _ => word(&before, i),
            };
            assert_eq!(word(&text, i), want, "word {i}");
        }
        // Patched code is recognised and left as it is.
        let patched = text.clone();
        assert!(!patch("test", &mut text));
        assert_eq!(text, patched);
    }

    #[test]
    fn a_missing_site_leaves_everything_alone() {
        let mut text = vec![0u8; 0x400];
        put(&mut text, 0x40, &SITES[0]);
        put(&mut text, 0x200, &SITES[2]);
        let before = text.clone();
        assert!(!patch("test", &mut text));
        assert_eq!(text, before);
    }

    #[test]
    fn a_conditional_branch_becomes_the_same_jump_always() {
        // b.le +0x88 (the dead zone in 1.0.0, main+0xd05480 to 0xd05508) and a backward one.
        assert_eq!(always(0x5400_044d), 0x1400_0022);
        assert_eq!(always(0x54ff_ff0d), 0x17ff_fff8);
    }
}
