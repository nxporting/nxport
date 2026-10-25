// Settings layers that come with the program: one per platform, and per game (all platforms, then one platform).

use super::SettingsPatch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacOS,
    IOS,
    Windows,
}

pub const PLATFORM: Platform = if cfg!(target_os = "ios") {
    Platform::IOS
} else if cfg!(windows) {
    Platform::Windows
} else {
    Platform::MacOS
};

/// The platform's general profile.
pub fn platform(p: Platform) -> SettingsPatch {
    let mut s = SettingsPatch::default();
    s.graphics.translated_shaders = Some(true);
    if p == Platform::IOS {
        s.system.fps = Some(crate::settings::FpsMode::Unlocked);
        // TIMESTAMP reports carry the GPU's times: the game's dynamic resolution then sees what the GPU takes.
        s.graphics.gpu_timestamps = Some(true);
        // Unused textures are freed after 600 frames even where their memory is not shared (the phone's memory limit).
        s.graphics.tex_sweep_unshared = Some(true);
        // Encode time, GPU tail and blocked time with the fps line, and GPU time per pass every 600 frames (log.txt).
        s.log.log_timeline = Some(true);
        s.log.gpu_pass_profile = Some(600);
    }
    if p == Platform::Windows {
        s.graphics.gpu = Some(crate::settings::GpuBackend::Vulkan);
    }
    s
}
