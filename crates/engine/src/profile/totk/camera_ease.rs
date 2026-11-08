// The camera turns sideways at full speed at once, as it pitches: no ease-in.
//
// TotK eases its camera's yaw in: while the right stick is held, a timer (+0x84 of the camera's input state) grows by
// the frame's time times a rate until 1, and the yaw speed is scaled by (cos(timer * a + b) + 1) / 2 (1.0.0:
// main+0x9abc34..0x9abc58, the timer's step at main+0x9ac5f0), so a turn reaches full speed after about 20 frames.
// Pitch has no such ramp. With a mouse the ramp is felt as lag on every sideways movement (the user: "up and down
// movement is not smoothed while side to side movement has a slight easing"; they want none), and it starts over each
// time the stick comes back to rest. The patch sets the timer to 1 instead of stepping it: the scale is 1 from the
// first frame. It applies to a controller too: a code patch is the same at build and run time, whatever the settings.
//
// The step is found by its instructions, not by address, so other builds compiled the same way get the patch too.

/// The timer's step: t + step, clamped to [t, 1] (s11 = 1.0), stored at +0x84.
const STEP: [u32; 5] = [
    0x1e21_2801, // fadd  s1, s0, s1           t + step
    0x1e20_2020, // fcmp  s1, s0
    0x1e2b_5420, // fccmp s1, s11, #0, pl
    0x1e21_ad60, // fcsel s0, s11, s1, ge      at most 1
    0xbd00_8680, // str   s0, [x20, #0x84]
];

/// What the step's first word becomes: `fmov s1, s11`, the timer at 1.
const AT_ONCE: u32 = 0x1e20_4161;

fn word(t: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(t[i * 4..i * 4 + 4].try_into().unwrap())
}

/// Patches a module's text where the step is found exactly once; anything else, or code already patched, is left
/// alone. Returns whether it patched. `text` is the module's code as loaded, before it is lifted or run.
pub fn patch(name: &str, text: &mut [u8]) -> bool {
    let words = text.len() / 4;
    let found: Vec<usize> = (0..words.saturating_sub(STEP.len() - 1))
        .filter(|&i| STEP.iter().enumerate().all(|(k, &w)| word(text, i + k) == w))
        .collect();
    let [at] = found[..] else { return false };
    text[at * 4..at * 4 + 4].copy_from_slice(&AT_ONCE.to_le_bytes());
    println!("[+] TotK: {name}: the camera turns sideways without its ease-in (at {:#x}, profile/totk/camera_ease.rs)", at * 4);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(at: usize) -> Vec<u8> {
        let mut text = vec![0u8; at + STEP.len() * 4 + 64];
        for (k, w) in STEP.iter().enumerate() {
            text[at + k * 4..at + k * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        text
    }

    #[test]
    fn sets_the_timer_at_once() {
        let mut text = code(0x100);
        let before = text.clone();
        assert!(patch("test", &mut text));
        for k in 0..text.len() / 4 {
            let want = if k * 4 == 0x100 { AT_ONCE } else { word(&before, k) };
            assert_eq!(word(&text, k), want, "word {k}");
        }
        // Patched code no longer matches: a second load of the same bytes is not patched twice.
        assert!(!patch("test", &mut text));
    }

    #[test]
    fn leaves_other_code_alone() {
        let mut text = vec![0u8; 0x400];
        assert!(!patch("other", &mut text));
        let one = code(0x40);
        let mut two = [one.clone(), one].concat();
        let before = two.clone();
        assert!(!patch("twice", &mut two));
        assert_eq!(two, before);
    }
}
