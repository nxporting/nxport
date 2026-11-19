// The size Breath of the Wild renders its 3D scene at: the input to resolution scaling's size classes.
//
// The scene's render targets (1600x900 docked) are made by one function per version (1.9.0 main+0x1063bdc): their
// size is a float rectangle of the view it renders for (the width and height it computes, stored at [sp+0x38] and used
// for each of its targets), not the screen's (gsys::SystemTask's frame-buffer size at +0x4c0 is the output, 1920x1080
// docked). So the size is taken where those targets are made: a texture initialized by its call of the render-target
// helper (1.9.0 main+0xed6364, 1.0.0 main+0xa735fc) has the scene's size. The game makes them again when the size
// changes (dynamic resolution, a mode switch), so the latest is the one in use.
//
// The call is recognised by two return addresses next to each other in the guest's call chain: the site's (the
// helper's caller) right above the helper's own call toward the texture's initialization. Other callers of the helper
// (per-frame targets on the worker threads) and textures made deeper inside other creations do not have that pair. The
// depth below the helper varies with the path it takes (1.9.0: two or three frames), so no fixed depth is assumed.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Where a version makes its scene targets (offsets in main): the return address of its call of the render-target
/// helper, and the helper's own return address on the way to the texture's initialization.
#[derive(Clone, Copy)]
pub struct Scene {
    pub site: usize,
    pub helper: usize,
}

/// 1.9.0: main+0x1063cb0 `bl 0xed6364`; the helper's main+0xed6424 `bl 0xed6448`.
pub const SCENE_V1_9_0: Scene = Scene { site: 0x1063cb4, helper: 0xed6428 };
/// 1.0.0: the function computes the size from its view (main+0xb86abc .. 0xb86acc, [sp+0x34], [sp+0x30]); main+0xb86bf4
/// `bl 0xa735fc`, the helper's main+0xa736bc `bl 0xa736e0` (textures made there: 1600x900, 1440x810, 1280x720 seen).
pub const SCENE_V1_0_0: Scene = Scene { site: 0xb86bf8, helper: 0xa736c0 };

/// Frames walked at most, and game frames looked at at most (the pair is 4-5 game frames above the initialization).
const FRAMES: usize = 32;
const GAME_FRAMES: usize = 10;

static SITE: AtomicUsize = AtomicUsize::new(0);
static HELPER: AtomicUsize = AtomicUsize::new(0);
/// The scene's size, width << 32 | height (0: not seen yet).
static SIZE: AtomicU64 = AtomicU64::new(0);

/// Records where this version makes its scene targets.
pub fn install(base: usize, scene: Scene) {
    HELPER.store(base + scene.helper, Ordering::Relaxed);
    SITE.store(base + scene.site, Ordering::Relaxed);
}

/// A texture of `width` x `height` was initialized with the guest frame pointer `fp`: if the scene-target creation made
/// it, that is the scene's size.
///
/// # Safety
/// `fp` is the frame pointer at the initialization (a frame-record chain over the guest's stack, possibly starting in
/// host frames and an HLE entry stub's, as nvn::frame_map::guest_chain walks it).
pub unsafe fn texture_initialized(width: u32, height: u32, fp: usize) {
    let site = SITE.load(Ordering::Relaxed);
    if site == 0 {
        return;
    }
    let helper = HELPER.load(Ordering::Relaxed);
    let base = crate::guest::loader::main_base();
    // No system call per frame: this runs for every texture the game initializes, thousands a second.
    let (mut fp, mut game, mut below) = (fp, 0, 0);
    for _ in 0..FRAMES {
        if fp < 0x1000 || fp & 7 != 0 || game == GAME_FRAMES {
            return;
        }
        let next = *(fp as *const usize);
        let ret = *((fp + 8) as *const usize);
        let stub = crate::gpu::report_guard::in_hle_stubs(ret);
        if !stub && ret >= base && ret < base + (256 << 20) {
            if ret == site && below == helper {
                let size = (width as u64) << 32 | height as u64;
                if SIZE.swap(size, Ordering::Relaxed) != size {
                    println!("[+] Scene: rendered at {width}x{height}");
                }
                return;
            }
            below = ret;
            game += 1;
        }
        // Up the stack, except from an HLE entry stub's frame to the game's (another stack natively).
        if next <= fp && !stub {
            return;
        }
        fp = next;
    }
}

/// The scene's render size (width, height), once its targets were made.
pub fn scene_size() -> Option<(u32, u32)> {
    let s = SIZE.load(Ordering::Relaxed);
    (s != 0).then_some(((s >> 32) as u32, s as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frame records on a fake stack: each [next fp, return address], the first at the initialization's fp.
    fn stack(rets: &[usize]) -> Vec<usize> {
        let mut s = vec![0usize; rets.len() * 2 + 2];
        let base = s.as_ptr() as usize;
        for (i, &r) in rets.iter().enumerate() {
            s[2 * i] = base + 16 * (i + 1); // the caller's record, higher up
            s[2 * i + 1] = r;
        }
        s
    }

    #[test]
    fn the_size_is_taken_only_under_the_site_and_helper_pair() {
        // main_base() is 0 in tests: any small return address counts as the game's.
        install(0, Scene { site: 0x1063cb4, helper: 0xed6428 });
        SIZE.store(0, Ordering::Relaxed);
        // The site above some other frame (a deeper creation inside another one): not the scene's.
        let s = stack(&[0xecad34, 0xed71c8, 0x1063cb4, 0x100]);
        unsafe { texture_initialized(16, 16, s.as_ptr() as usize) };
        assert_eq!(scene_size(), None);
        // The helper's return right below the site, whatever the depth under it.
        let s = stack(&[0xecad34, 0xed71c8, 0xed66b4, 0xed6428, 0x1063cb4, 0x100]);
        unsafe { texture_initialized(1600, 900, s.as_ptr() as usize) };
        assert_eq!(scene_size(), Some((1600, 900)));
        let s = stack(&[0xed7144, 0xed66b4, 0xed6428, 0x1063cb4]);
        unsafe { texture_initialized(1440, 810, s.as_ptr() as usize) };
        assert_eq!(scene_size(), Some((1440, 810)));
        install(0, Scene { site: 0, helper: 0 });
    }
}
