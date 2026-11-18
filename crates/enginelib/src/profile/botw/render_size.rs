// The size Breath of the Wild renders its 3D scene at: the input to resolution scaling's size classes.
//
// The scene's render targets (1600x900 docked) are made by one function per version (1.9.0 main+0x1063bdc): their
// size is a float rectangle of the view it renders for (the width and height it computes, stored at [sp+0x38] and used
// for each of its targets), not the screen's (gsys::SystemTask's frame-buffer size at +0x4c0 is the output, 1920x1080
// docked). So the size is taken where those targets are made: a texture initialized with that function's first call
// (its return address) among its callers has the scene's size. The game makes them again when the size changes (a
// mode switch), so the latest is the one in use.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// The return address (offset in main) of the scene-target creation's first texture creation, per version.
pub const SCENE_V1_9_0: usize = 0x1063cb4;

/// How many callers above the texture's initialization the creation can be (1.9.0: the fifth).
const DEPTH: usize = 6;

static SITE: AtomicUsize = AtomicUsize::new(0);
/// The scene's size, width << 32 | height (0: not seen yet).
static SIZE: AtomicU64 = AtomicU64::new(0);

/// Records where this version makes its scene targets (main's base plus the return address's offset).
pub fn install(base: usize, offset: usize) {
    SITE.store(base + offset, Ordering::Relaxed);
}

/// A texture of `width` x `height` was initialized with the guest frame pointer `fp`: if the scene-target creation made
/// it, that is the scene's size.
///
/// # Safety
/// `fp` is the guest's frame pointer at the initialization (its frames are read only where readable).
pub unsafe fn texture_initialized(width: u32, height: u32, fp: usize) {
    let site = SITE.load(Ordering::Relaxed);
    if site == 0 {
        return;
    }
    let mut fp = fp;
    for _ in 0..DEPTH {
        if fp == 0 || !super::super::readable(fp, 16) {
            return;
        }
        if *((fp + 8) as *const usize) == site {
            let size = (width as u64) << 32 | height as u64;
            if SIZE.swap(size, Ordering::Relaxed) != size {
                println!("[+] Scene: rendered at {width}x{height}");
            }
            return;
        }
        let next = *(fp as *const usize);
        if next <= fp {
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
