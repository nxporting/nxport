// Multi-game and multi-version registry. Identifies the running game title via ExeFS/main.npdm
// and matches the version via the main NSO module Build ID.

pub mod botw;
pub mod kirby;
pub mod smo;
pub mod totk;
pub mod wonder;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GameKind {
    TotK,
    Kirby,
    MarioWonder,
    SuperMarioOdyssey,
    BotW,
    Unknown(u64),
}

#[derive(Copy, Clone, Debug)]
pub struct MemoryPatch {
    pub offset: usize,
    pub opcode: u32,
    pub description: &'static str,
}

/// A version's code hooks: they patch the guest's arm64 code in place, before it is made executable (or lifted).
#[cfg(nx_runtime)]
pub type InstallHooks = unsafe fn(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace);
#[cfg(not(nx_runtime))]
pub type InstallHooks = unsafe fn(base: usize, mem_space: &mut std::convert::Infallible);

#[derive(Copy, Clone, Debug)]
pub struct VersionProfile {
    pub version_string: &'static str,
    pub build_id_prefix: &'static str,
    pub install_hooks: Option<InstallHooks>,
    pub heap_instance_offset: Option<usize>,
    pub allocator_pool_offset: Option<usize>,
    /// --log-draw-items: a token for the game object whose draw is being recorded on this thread (0: none found).
    pub draw_item: Option<unsafe fn() -> u32>,
}

#[derive(Copy, Clone, Debug)]
pub struct GameProfile {
    pub kind: GameKind,
    pub name: &'static str,
    /// Short lowercase name for identifiers (the app's bundle ID).
    pub short_name: &'static str,
    pub title_id: u64,
    pub lower_left_window_origin: bool,
    pub compute_has_preamble: bool,
    pub versions: &'static [VersionProfile],
    /// RomFS directories that hold shader programs (archives, effects, layouts); empty: all of RomFS.
    pub shader_dirs: &'static [&'static str],
    /// Settings for this game on every platform, and on one platform (layers above the platform's profile).
    pub options: Option<fn() -> crate::settings::SettingsPatch>,
    pub platform_options:
        Option<fn(crate::settings::profiles::Platform) -> crate::settings::SettingsPatch>,
    /// The builder presets (name, preset file) the game's preset list starts with.
    pub presets: &'static [(&'static str, &'static str)],
}

/// The default bundle ID of an app built from a game: com.nxport.<short name>, or com.nxport.t<title id> without a
/// profile.
pub fn bundle_id(title_id: u64, profile: Option<&GameProfile>) -> String {
    match profile.filter(|p| !p.short_name.is_empty()) {
        Some(p) => format!("com.nxport.{}", p.short_name),
        None => format!("com.nxport.t{title_id:016x}"),
    }
}

impl GameProfile {
    pub fn is_totk(&self) -> bool {
        self.kind == GameKind::TotK
    }

    pub fn is_kirby(&self) -> bool {
        self.kind == GameKind::Kirby
    }

    pub fn is_wonder(&self) -> bool {
        self.kind == GameKind::MarioWonder
    }

    pub fn is_smo(&self) -> bool {
        self.kind == GameKind::SuperMarioOdyssey
    }

    pub fn match_version(&self, build_id: &str) -> Option<&'static VersionProfile> {
        let clean_id = build_id.to_lowercase();
        self.versions
            .iter()
            .find(|v| clean_id.starts_with(v.build_id_prefix))
    }
}

pub const TOTK_TITLE_ID: u64 = 0x0100_F2C0_115B_6000;
pub const KIRBY_TITLE_ID: u64 = 0x0100_4D30_0C5A_E000;
pub const WONDER_TITLE_ID: u64 = 0x0100_1510_0B51_4000;
pub const SMO_TITLE_ID: u64 = 0x0100_0000_0001_0000;
pub const BOTW_TITLE_ID: u64 = 0x0100_7EF0_0011_E000;

pub static PROFILES: &[GameProfile] = &[totk::PROFILE, kirby::PROFILE, wonder::PROFILE, smo::PROFILE, botw::PROFILE];

/// What a game dump is: its program id, profile and version (by the main module's build id), if known.
pub struct Identified {
    pub title_id: Option<u64>,
    pub profile: Option<&'static GameProfile>,
    pub version: Option<&'static VersionProfile>,
    /// The profile's name, else the title in the dump's control data, else the program id.
    pub name: String,
}

pub fn identify(dump: &std::path::Path) -> Identified {
    let title_id = crate::game::title_id_of(dump);
    let profile = title_id.and_then(for_title);
    let build_id =
        crate::guest::nso::nso_build_id(&dump.join("ExeFS/main").display().to_string());
    let version = profile
        .zip(build_id)
        .and_then(|(p, id)| p.match_version(&id));
    let name = profile
        .map(|p| p.name.to_string())
        .or_else(|| crate::game::control_of(dump).and_then(|c| c.title))
        .unwrap_or_else(|| title_id.map_or_else(|| "Nintendo Switch".to_string(), |t| format!("Switch title {t:016X}")));
    Identified {
        title_id,
        profile,
        version,
        name,
    }
}

/// The profile of a title id, if there is one.
pub fn for_title(tid: u64) -> Option<&'static GameProfile> {
    PROFILES.iter().find(|p| p.title_id == tid)
}

/// A game's own settings sections (the options its profile defines).
#[cfg(any(target_os = "macos", windows))]
pub fn option_sections<'a>(
    p: &GameProfile,
    s: &'a mut crate::settings::Settings,
) -> Vec<(&'static str, &'a mut dyn crate::settings::edit::Section)> {
    match p.kind {
        GameKind::TotK => s.totk.sections(),
        _ => Vec::new(),
    }
}

/// Clears the options of games other than `p` from `patch` (settings files for one game carry no other's).
pub fn keep_own_options(p: Option<&GameProfile>, patch: &mut crate::settings::SettingsPatch) {
    if !p.is_some_and(|p| p.kind == GameKind::TotK) {
        patch.totk = totk::OptionsPatch::default();
    }
}

/// The settings layers a game's profile adds on `platform`: for every platform, then for this one.
pub fn layers(
    p: &GameProfile,
    platform: crate::settings::profiles::Platform,
) -> Vec<crate::settings::SettingsPatch> {
    p.options
        .map(|f| f())
        .into_iter()
        .chain(p.platform_options.map(|f| f(platform)))
        .collect()
}

static DEFAULT_PROFILE: GameProfile = GameProfile {
    kind: GameKind::Unknown(0),
    name: "Unknown Title",
    short_name: "",
    title_id: 0,
    lower_left_window_origin: false,
    compute_has_preamble: true,
    versions: &[],
    shader_dirs: &[],
    options: None,
    platform_options: None,
    presets: &[],
};

/// Returns the detected GameProfile for the current run (by the title id in main.npdm).
pub fn active() -> &'static GameProfile {
    static ACTIVE: OnceLock<&'static GameProfile> = OnceLock::new();
    if let Some(p) = ACTIVE.get() {
        return p;
    }
    let tid = match crate::game::title_id() {
        Some(t) => t,
        None => return &DEFAULT_PROFILE,
    };
    if let Some(p) = for_title(tid) {
        let _ = ACTIVE.set(p);
        return p;
    }
    let name: &'static str = Box::leak(format!("Switch title {tid:016X}").into_boxed_str());
    let prof: &'static GameProfile = Box::leak(Box::new(GameProfile {
        kind: GameKind::Unknown(tid),
        name,
        short_name: "",
        title_id: tid,
        lower_left_window_origin: false,
        compute_has_preamble: true,
        versions: &[],
        shader_dirs: &[],
        options: None,
        platform_options: None,
        presets: &[],
    }));
    let _ = ACTIVE.set(prof);
    prof
}

fn bundle_display_name() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let macos = exe.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let plist_path = macos.parent()?.join("Info.plist");
    if !plist_path.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(plist_path).ok()?;
    let extract = |key: &str| -> Option<String> {
        let pattern = format!("<key>{}</key>", key);
        let pos = text.find(&pattern)?;
        let rest = &text[pos + pattern.len()..];
        let start = rest.find("<string>")? + "<string>".len();
        let end = rest[start..].find("</string>")? + start;
        let val = rest[start..end].trim();
        let unescaped = val
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'");
        if !unescaped.is_empty() && unescaped != "nxport" && unescaped != "NxportGuest" {
            Some(unescaped)
        } else {
            None
        }
    };
    extract("CFBundleDisplayName").or_else(|| extract("CFBundleName"))
}

/// The display title of the running application/game (e.g. "Kirby and the Forgotten Land", "Super Mario Bros. Wonder",
/// "The Legend of Zelda: Tears of the Kingdom").
pub fn app_title() -> &'static str {
    static TITLE: OnceLock<String> = OnceLock::new();
    if let Some(t) = TITLE.get() {
        return t.as_str();
    }
    if let Some(bundle_name) = bundle_display_name() {
        let _ = TITLE.set(bundle_name);
        return TITLE.get().unwrap().as_str();
    }
    if let Some(dir) = crate::settings::try_get().and_then(|s| s.files.game_dir.as_deref()) {
        let _ = TITLE.set(identify(dir).name);
        return TITLE.get().unwrap().as_str();
    }
    "Nintendo Switch"
}

/// The aspect the game renders at when a profile patch changed it from the game's own (the screen shape asked for).
#[cfg(nx_native)]
pub fn rendered_aspect() -> Option<f64> {
    match active().kind {
        GameKind::TotK => totk::v1_1_2::aspect::rendered_aspect(),
        _ => None,
    }
}

/// Whether the game will render at a screen shape wider than 16:9 (known before its code is patched: the window
/// opens at that shape).
pub fn renders_screen_shape() -> bool {
    match active().kind {
        GameKind::TotK => crate::settings::get().totk.v1_1_2.dynamic_aspect_ratio,
        _ => false,
    }
}

/// The screen's shape changed (the window was resized): a game rendering at another shape follows it.
#[cfg(nx_native)]
pub fn screen_aspect_changed(aspect: f64) {
    if active().kind == GameKind::TotK {
        totk::v1_1_2::aspect::set_aspect(aspect);
    }
}

/// Helper shortcut for compatibility with existing `game::is_totk()` checks.
pub fn is_totk() -> bool {
    active().is_totk()
}

pub fn is_kirby() -> bool {
    active().is_kirby()
}

pub fn is_wonder() -> bool {
    active().is_wonder()
}

pub fn is_smo() -> bool {
    active().is_smo()
}

static MAIN_BUILD_ID: OnceLock<String> = OnceLock::new();

pub fn set_main_build_id(id: &str) {
    let _ = MAIN_BUILD_ID.set(id.to_lowercase());
}

/// Logs the game's version (the dump's control data) and the version profile its main build id matched; warns when the
/// two disagree.
pub fn log_version(build_id: &str) {
    let display = crate::game::control_of(crate::game::dir()).and_then(|c| c.version);
    let matched = active().match_version(build_id);
    println!(
        "[+] Game: {} version {} (main build id {}), profile {}",
        active().name,
        display.as_deref().unwrap_or("unknown"),
        &build_id[..build_id.len().min(32)],
        matched.map_or("none", |v| v.version_string)
    );
    if let (Some(d), Some(v)) = (&display, matched) {
        if d != v.version_string {
            eprintln!("[!] Game version {d} matched the profile of version {}", v.version_string);
        }
    }
}

pub fn main_build_id() -> Option<&'static str> {
    MAIN_BUILD_ID.get().map(|s| s.as_str())
}

pub fn active_version() -> Option<&'static VersionProfile> {
    if let Some(bid) = main_build_id() {
        return active().match_version(bid);
    }
    let exefs = crate::game::exefs();
    if let Some(id) = crate::guest::nso::nso_build_id(&format!("{}/main", exefs.display()))
    {
        set_main_build_id(&id);
        return active().match_version(&id);
    }
    None
}

/// The running game's data directory: games/<title id> in nxport's application data (config::support_dir), holding its
/// saves (save/), mods (mods/), shader cache (shader_cache/), the stages that cache lacks (shader_dump_missing/) and
/// translated_shaders/. The first time, a directory of the older layout (named after the game's title) is moved into it.
pub fn data_dir() -> Option<&'static Path> {
    static D: OnceLock<Option<PathBuf>> = OnceLock::new();
    D.get_or_init(|| {
        let tid = crate::game::title_id()?;
        let nxport = crate::config::support_dir()?;
        let support = nxport.parent()?;
        let dir = nxport.join("games").join(format!("{tid:016x}"));
        if let Some(p) = for_title(tid) {
            move_old_data(&support.join(p.name.replace(':', " -")), &dir);
        }
        Some(dir)
    })
    .as_deref()
}

/// A directory in the running game's data directory.
pub fn data_subdir(name: &str) -> Option<PathBuf> {
    Some(data_dir()?.join(name))
}

/// Moves the entries of `old` into `new`, leaving any whose name `new` already has; removes `old` once empty.
fn move_old_data(old: &Path, new: &Path) {
    let Ok(entries) = std::fs::read_dir(old) else { return };
    if let Err(e) = std::fs::create_dir_all(new) {
        eprintln!("[!] data: {}: {e}", new.display());
        return;
    }
    for e in entries.flatten() {
        let to = new.join(e.file_name());
        if to.exists() {
            eprintln!("[!] data: {} left in place: {} exists", e.path().display(), to.display());
        } else if let Err(err) = std::fs::rename(e.path(), &to) {
            eprintln!("[!] data: moving {} to {}: {err}", e.path().display(), to.display());
        } else {
            println!("[+] data: moved {} to {}", e.path().display(), to.display());
        }
    }
    let _ = std::fs::remove_dir(old);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_data_moves_without_replacing_anything() {
        let root = std::env::temp_dir().join(format!("nxport-move-test-{}", std::process::id()));
        let (old, new) = (root.join("Old Title"), root.join("games/0100000000000000"));
        std::fs::create_dir_all(old.join("save/slot_00")).unwrap();
        std::fs::write(old.join("save/slot_00/file"), b"old save").unwrap();
        std::fs::create_dir_all(old.join("translated_shaders")).unwrap();
        std::fs::create_dir_all(new.join("translated_shaders")).unwrap();
        move_old_data(&old, &new);
        assert_eq!(std::fs::read(new.join("save/slot_00/file")).unwrap(), b"old save");
        assert!(old.join("translated_shaders").is_dir(), "an entry the new directory has stays where it was");
        std::fs::remove_dir(old.join("translated_shaders")).unwrap();
        move_old_data(&old, &new);
        assert!(!old.exists(), "the old directory goes once empty");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_profile_titles() {
        assert_eq!(
            for_title(TOTK_TITLE_ID).unwrap().name,
            "The Legend of Zelda: Tears of the Kingdom"
        );
        assert_eq!(
            for_title(KIRBY_TITLE_ID).unwrap().name,
            "Kirby and the Forgotten Land"
        );
        assert_eq!(
            for_title(WONDER_TITLE_ID).unwrap().name,
            "Super Mario Bros. Wonder"
        );
    }

    #[test]
    fn test_app_title_fallback_non_empty() {
        let title = app_title();
        assert!(!title.is_empty());
    }
}
