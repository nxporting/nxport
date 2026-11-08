// A patch of UltraCam's own code: its overlay size.
//
// UltraCam (an exlaunch plugin) hooks nvnCommandBufferSetViewport, and its hook copies every viewport the game sets into
// ImGui's DisplaySize before calling the real function. TotK sets viewports for all its passes (shadow maps, the bloom
// chain, the 1600x900 scene, its 1920x1080 UI) on three threads, while the overlay is laid out on the presentation
// thread from whatever DisplaySize holds at that moment. When a render thread's pass lands just before, the overlay is
// laid out for 800x450, 180x112 and the like for a frame: its right-edge scrollbar strip lands mid screen as a dark
// vertical line and its menu jumps. On a Switch (and in Ryujinx) the last viewport before the overlay is nearly always
// the 1920x1080 UI pass; nxport's render threads run further ahead, so the race shows. Measured on the user's save:
// 233 strips at a wrong size in 3000 frames, 1 with DisplaySize held at the window's size.
//
// The patch removes the hook's store (the one instruction), so DisplaySize keeps the size UltraCam sets when its overlay
// starts (the window's size, at least 1280x720). The hook still calls the real function. Only the plugin's code changes,
// once, as it loads. The cost: after a docked/handheld switch while the game runs, the overlay keeps the size it started
// with. With this UltraCam a switch to handheld crashes the game first (its resolution hook, called every frame from
// main+0xc5fd5c; the user saw the same crash in Ryujinx), so the cost cannot show until that is fixed.
//
// The hook is found by its instructions, not by address, so other builds compiled the same way get the patch too.

/// UltraCam's nvnCommandBufferSetViewport hook (cmd, x, y, width, height), as compiled in the UltraCam build in the
/// user's TotK mods (subsdk of 1,251,511 bytes, at +0xbbf90). None: words that differ between builds (the call to
/// ImGui::GetIO and the load of the real function's address).
const HOOK: [Option<u32>; 18] = [
    Some(0xa9bd_7bfd), // stp   x29, x30, [sp, #-0x30]!
    Some(0x9100_03fd), // mov   x29, sp
    Some(0xf900_0bf3), // str   x19, [sp, #0x10]
    Some(0xaa00_03f3), // mov   x19, x0
    Some(0x2904_0fe4), // stp   w4, w3, [sp, #0x20]
    Some(0x2905_07e2), // stp   w2, w1, [sp, #0x28]
    None,              // bl    ImGui::GetIO
    Some(0x2944_0fe4), // ldp   w4, w3, [sp, #0x20]
    Some(0xaa00_03e5), // mov   x5, x0               the ImGuiIO
    None,              // adrp  x0, ...
    None,              // ldr   x16, [x0, #...]      the real function
    Some(0xaa13_03e0), // mov   x0, x19
    Some(0x2945_07e2), // ldp   w2, w1, [sp, #0x28]
    Some(0x1e22_007e), // scvtf s30, w3              width
    Some(0x1e22_009f), // scvtf s31, w4              height
    Some(0x2d01_7cbe), // stp   s30, s31, [x5, #8]   io.DisplaySize = (width, height)
    Some(0xf940_0bf3), // ldr   x19, [sp, #0x10]
    Some(0xa8c3_7bfd), // ldp   x29, x30, [sp], #0x30
];

/// Index in HOOK of the DisplaySize store.
const STORE: usize = 15;

const NOP: u32 = 0xd503_201f;

fn word(t: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(t[i * 4..i * 4 + 4].try_into().unwrap())
}

/// Patches a module's text where the hook is found exactly once; anything else is left alone. Returns whether it
/// patched. `text` is the module's code as loaded, before it is lifted or run.
pub fn patch(name: &str, text: &mut [u8]) -> bool {
    let words = text.len() / 4;
    let found: Vec<usize> = (0..words.saturating_sub(HOOK.len() - 1))
        .filter(|&i| HOOK.iter().enumerate().all(|(k, w)| w.is_none_or(|w| word(text, i + k) == w)))
        .collect();
    let [at] = found[..] else { return false };
    let i = (at + STORE) * 4;
    text[i..i + 4].copy_from_slice(&NOP.to_le_bytes());
    println!("[+] TotK: {name}: patched UltraCam's viewport hook to keep its overlay size (at {:#x}, profile/totk/ultracam.rs)", at * 4);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(at: usize) -> Vec<u8> {
        let mut text = vec![0u8; at + HOOK.len() * 4 + 64];
        for (k, w) in HOOK.iter().enumerate() {
            text[at + k * 4..at + k * 4 + 4].copy_from_slice(&w.unwrap_or(0x9400_0000).to_le_bytes());
        }
        text
    }

    #[test]
    fn removes_the_store_wherever_the_hook_is() {
        for at in [0x0, 0x100, 0x1004] {
            let mut text = code(at);
            let before = text.clone();
            assert!(patch("test", &mut text));
            for k in 0..text.len() / 4 {
                let want = if k * 4 == at + STORE * 4 { NOP } else { word(&before, k) };
                assert_eq!(word(&text, k), want, "{at:#x} word {k}");
            }
            // Patched code no longer matches: a second load of the same bytes is not patched twice.
            assert!(!patch("test", &mut text));
        }
    }

    #[test]
    fn leaves_other_code_alone() {
        let mut text = vec![0u8; 0x1000];
        assert!(!patch("other", &mut text));
        assert!(text.iter().all(|&b| b == 0));
        // Two copies: not knowing which one is meant, neither is touched.
        let one = code(0x100);
        let mut two = [one.clone(), one].concat();
        let before = two.clone();
        assert!(!patch("twice", &mut two));
        assert_eq!(two, before);
    }
}
