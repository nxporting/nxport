// Settings that come from where the program is installed, and derived display settings.

/// Inside a macOS .app: the dump, mods and shader cache the bundle carries (a platform-environment layer).
pub fn app_bundle_layer() -> crate::settings::SettingsPatch {
    let mut p = crate::settings::SettingsPatch::default();
    let Some(resources) = std::env::current_exe().ok().and_then(|exe| {
        let macos = exe.parent()?;
        (macos.file_name()? == "MacOS").then(|| macos.parent().map(|c| c.join("Resources")))?
    }) else {
        return p;
    };
    let dir = |names: &[&str]| names.iter().map(|n| resources.join(n)).find(|d| d.is_dir());
    p.files.game_dir = dir(&["Dump"])
        .filter(|d| d.join("ExeFS").is_dir() || d.join("exefs").is_dir())
        .or_else(|| (resources.join("ExeFS").is_dir() || resources.join("exefs").is_dir()).then(|| resources.clone()));
    p.files.mods_dir = dir(&["Mods", "mods"]);
    if p.files.mods_dir.is_none() {
        p.files.no_mods = Some(true);
    }
    p.files.shader_cache = dir(&["shader_cache"]);
    p.files.shared_fonts = dir(&["fonts"]);
    p
}

/// Inside a macOS .app: the settings it was built with (Resources/settings.toml), a layer above the game's profile.
pub fn app_bundle_settings() -> Option<crate::settings::SettingsPatch> {
    let exe = std::env::current_exe().ok()?;
    let macos = exe.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let file = macos.parent()?.join("Resources/settings.toml");
    if !file.is_file() {
        return None;
    }
    match crate::settings::load_preset(&file) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("[!] {}: {e}", file.display());
            None
        }
    }
}

/// The aspect (width / height) to present the game's picture at: the screen shape asked for (--aspect) when the game's
/// profile made the game render at it, else None (the game's own output aspect, letterboxed to the screen).
#[cfg(nx_native)]
pub fn display_aspect() -> Option<f64> {
    crate::profile::rendered_aspect()
}
