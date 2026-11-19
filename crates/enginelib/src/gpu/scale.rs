// Render targets drawn above the game's resolution (Graphics > Resolution), as Ryujinx scales them: only targets of
// the game's scene size class are scaled, by a whole factor, so texel fetches and blits between them stay exact.
// The class is the scene size the game's profile reports (profile::scene_size) divided by a whole k: the scene and its
// /2, /4, ... chains. Everything else, and anything the CPU or a storage binding touches, stays at the game's size.
// The backends ask scale() when they create a texture, pass_scale() for a pass's targets, and demote() a texture's
// memory on a use that needs the game's size.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// What a backend knows of a texture when it decides its scale.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub width: u32,
    pub height: u32,
    /// 2D, 2D array or multisampled 2D.
    pub two_d: bool,
    pub compressed: bool,
    /// Rows in guest memory as the CPU reads them (linear layout).
    pub linear: bool,
    /// May be bound as a storage image.
    pub image: bool,
    pub storage: usize,
}

static DEMOTED: Mutex<Option<HashSet<(usize, u32, u32)>>> = Mutex::new(None);

/// The scale render targets of the scene's class get (1: the game's resolution; the setting at launch).
pub fn factor() -> u32 {
    static K: OnceLock<u32> = OnceLock::new();
    *K.get_or_init(|| crate::settings::get().graphics.resolution_scale.clamp(1, 4))
}

/// Whether width x height is the scene size divided by a whole k (floored, as the game sizes its chains).
fn in_class(width: u32, height: u32, scene: (u32, u32)) -> bool {
    let (w, h) = scene;
    if width == 0 || height == 0 || w == 0 || h == 0 || width > w {
        return false;
    }
    let k = w / width;
    (k..=k + 1).any(|k| k >= 1 && w / k == width && h / k == height)
}

/// The scale a texture is created with: factor() for render targets of the scene's class, else 1.
pub fn scale(s: &Shape) -> u32 {
    let k = factor();
    if k == 1 || !s.two_d || s.compressed || s.linear || s.image {
        return 1;
    }
    let Some(scene) = crate::profile::scene_size() else {
        return 1;
    };
    // Memory the CPU can reach holds the game's pixels as it reads and writes them (write-back, texel copies, heap
    // compaction by CPU copies): only GPU-only pools are scaled.
    if !in_class(s.width, s.height, scene)
        || is_demoted(s.storage, s.width, s.height)
        || crate::nvn::api::memory_pool_cpu_writable(s.storage)
    {
        return 1;
    }
    k
}

/// The `width` x `height` (game size) texture at `storage` is used at the game's size from now on (a copy across size
/// classes, a pass with unscaled targets). The backend re-creates it at scale 1. A texture made there at another size
/// (dynamic resolution) is decided afresh.
pub fn demote(storage: usize, width: u32, height: u32, what: impl FnOnce() -> String) {
    if factor() == 1 {
        return;
    }
    let mut d = DEMOTED.lock().unwrap();
    if d.get_or_insert_with(HashSet::default).insert((storage, width, height)) {
        println!("[+] Scale: texture memory {storage:#x} stays at the game's resolution: {}", what());
    }
}

pub fn is_demoted(storage: usize, width: u32, height: u32) -> bool {
    factor() != 1 && DEMOTED.lock().unwrap().as_ref().is_some_and(|d| d.contains(&(storage, width, height)))
}

/// The scale a pass renders at: its targets' common scale, 1 if any target is unscaled.
pub fn pass_scale(targets: &[u32]) -> u32 {
    if targets.is_empty() || targets.iter().any(|&s| s != targets[0]) {
        return 1;
    }
    targets[0]
}

/// A sampled texture's entry in the shaders' render-scale table: its scale, negative in a fragment stage when the pass
/// target is the texture's size times a whole ratio on both axes (the shader then adds gl_FragCoord's sub-texel
/// position to fetches, Ryujinx TextureBindingsManager).
pub fn sampled_scale(tex_scale: u32, tex: (u32, u32), target: Option<(u32, u32)>, fragment: bool) -> f32 {
    let s = tex_scale as f32;
    if tex_scale == 1 || !fragment {
        return s;
    }
    match target {
        Some((tw, th)) if tex.0 != 0 && tex.1 != 0 && (tw as f32 / tex.0 as f32) == (th as f32 / tex.1 as f32) => -s,
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class() {
        let scene = (1600, 900);
        for (w, h) in [(1600, 900), (800, 450), (400, 225), (200, 112), (160, 90), (80, 45), (40, 22)] {
            assert!(in_class(w, h, scene), "{w}x{h}");
        }
        for (w, h) in [(1920, 1080), (165, 1080), (696, 42), (1600, 450), (256, 256), (0, 0)] {
            assert!(!in_class(w, h, scene), "{w}x{h}");
        }
    }

    #[test]
    fn pass() {
        assert_eq!(pass_scale(&[2, 2]), 2);
        assert_eq!(pass_scale(&[2, 1]), 1);
        assert_eq!(pass_scale(&[]), 1);
    }

    #[test]
    fn sampled() {
        assert_eq!(sampled_scale(1, (800, 450), Some((1600, 900)), true), 1.0);
        assert_eq!(sampled_scale(2, (800, 450), Some((1600, 900)), true), -2.0);
        assert_eq!(sampled_scale(2, (800, 450), Some((1600, 900)), false), 2.0);
        assert_eq!(sampled_scale(2, (800, 450), Some((1920, 900)), true), 2.0);
    }
}
