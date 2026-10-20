// The builder: a window that turns a game dump into a native app. It identifies the game, edits the settings the app
// starts with (on top of the platform's and the game's profiles, with presets), and runs the build on a worker thread:
// shader stages from RomFS, the shader cache, then the macOS app or the iOS app. Everything the steps print goes to
// the log.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use nxport::settings::edit::{Audience, Section};
use nxport::settings::profiles::{self, Platform};
use nxport::settings::{self, Settings, SettingsPatch};
use nxport::ui::imgui;
use nxport::ui::window::PathKind;
use struct_patch::Patch;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Target {
    #[serde(rename = "mac", alias = "mac-app", alias = "macapp")]
    MacApp,
    #[serde(rename = "ios", alias = "ipa")]
    Ipa,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", default)]
struct BuildPreset {
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<Target>,
    #[serde(skip_serializing_if = "Option::is_none")]
    out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bundle_game: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entitlements: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prebuild_shaders: Option<bool>,
}

/// What the builder remembers per game and target: the mods folder, bundle ID, and settings used last.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", default)]
struct TargetMemory {
    #[serde(skip_serializing_if = "Option::is_none")]
    mods: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bundle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<SettingsPatch>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct GameMemory {
    /// The preset selected last for this game (whatever the target).
    #[serde(skip_serializing_if = "Option::is_none")]
    preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mac: Option<TargetMemory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ios: Option<TargetMemory>,
}

impl GameMemory {
    fn path(title_id: u64) -> PathBuf {
        data_dir().join("games").join(format!("{title_id:016x}.toml"))
    }

    fn load(title_id: u64) -> GameMemory {
        std::fs::read_to_string(Self::path(title_id)).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    fn save(&self, title_id: u64) -> Result<(), String> {
        let path = Self::path(title_id);
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(path.parent().unwrap()).and_then(|_| std::fs::write(&path, text)).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn target(&mut self, t: Target) -> &mut Option<TargetMemory> {
        match t {
            Target::MacApp => &mut self.mac,
            Target::Ipa => &mut self.ios,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct PresetFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    build: Option<BuildPreset>,
    #[serde(flatten)]
    settings: SettingsPatch,
}

fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest).display().to_string();
        }
    }
    p.to_string()
}

fn save_builder_preset(path: &Path, build: BuildPreset, edits: &SettingsPatch) -> Result<(), String> {
    let file = PresetFile {
        build: Some(build),
        settings: edits.clone(),
    };
    let mut v = toml::Value::try_from(&file).map_err(|e| e.to_string())?;
    settings::prune_empty(&mut v);
    let text = toml::to_string_pretty(&v).or_else(|_| toml::to_string(&v)).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn parse_builder_preset(text: &str) -> Result<(Option<BuildPreset>, SettingsPatch), String> {
    let preset: PresetFile = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok((preset.build, preset.settings))
}

#[derive(Clone, Debug, PartialEq)]
struct PresetInfo {
    name: String,
    path: PathBuf,
}

fn load_builder_preset(path: &Path) -> Result<(Option<BuildPreset>, SettingsPatch), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_builder_preset(&text)
}

impl Target {
    const ALL: [Target; 2] = [Target::MacApp, Target::Ipa];

    fn label(self) -> &'static str {
        match self {
            Target::MacApp => "macOS app",
            Target::Ipa => "iOS app (.ipa)",
        }
    }

    fn platform(self) -> Platform {
        match self {
            Target::MacApp => Platform::MacOS,
            Target::Ipa => Platform::IOS,
        }
    }
}

struct Game {
    name: String,
    version: Option<String>,
    title_id: u64,
    profile: Option<&'static nxport::profile::GameProfile>,
    /// A 32-bit program: cannot run on Apple CPUs.
    is_32bit: bool,
}

#[derive(Default)]
struct Job {
    step: String,
    steps_done: usize,
    steps: usize,
    sub_step: Option<String>,
    sub_done: usize,
    sub_total: usize,
    sub_frac: f32,
    result: Option<Result<PathBuf, String>>,
}

/// Everything this process prints (the steps' progress), for the log view.
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Stop after the running step.
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Sends this process's stdout and stderr through a pipe into LOG, and on to where they went before.
fn capture_output() {
    unsafe {
        let mut fds = [0; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        let console = libc::dup(1);
        libc::dup2(fds[1], 1);
        libc::dup2(fds[1], 2);
        libc::close(fds[1]);
        use std::os::fd::FromRawFd;
        let reader = std::fs::File::from_raw_fd(fds[0]);
        let mut console = std::fs::File::from_raw_fd(console);
        std::thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                let _ = writeln!(console, "{line}");
                let mut log = LOG.lock().unwrap();
                log.push(line);
                let n = log.len();
                if n > 5000 {
                    log.drain(..n - 5000);
                }
            }
        });
    }
}

type Step = (String, Box<dyn FnOnce() -> Result<(), String> + Send>);

struct Builder {
    dump: String,
    game: Option<Game>,
    target: Target,
    mods: String,
    out_dir: String,
    /// The profiles' settings for the game on the target, and the edited settings above them.
    base: Settings,
    settings: Settings,
    preset_name: String,
    preset_search: String,
    presets: Vec<PresetInfo>,
    message: Option<String>,
    job: Option<Arc<Mutex<Job>>>,
    /// Every settings group open (snapshots).
    expand_all: bool,
    /// iOS: the game files go into the app (large) instead of its Documents/Game.
    bundle_game: bool,
    /// The dump's RomFS size in bytes, measured in the background.
    romfs_size: Arc<Mutex<Option<u64>>>,
    entitlements: bool,
    /// Translate the game's shaders into the app ahead of time (the app translates only what the cache lacks); off, it
    /// translates every shader as the game loads it.
    prebuild_shaders: bool,
    bundle_id: String,
}

fn data_dir() -> PathBuf {
    // Tests: a directory per test thread.
    #[cfg(test)]
    return std::env::temp_dir().join(format!("nxport-builder-test-{}-{:?}", std::process::id(), std::thread::current().id()));
    #[cfg(not(test))]
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Library/Application Support/nxport")
}

fn default_out_dir(target: Target) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match target {
        Target::MacApp => home.map(|h| h.join("Applications").display().to_string()).unwrap_or_default(),
        Target::Ipa => home.map(|h| h.join("Desktop").display().to_string()).unwrap_or_default(),
    }
}

/// A game's presets: `games/<title id>/presets/*.toml` in the builder's data directory.
fn game_presets_dir(title_id: u64) -> PathBuf {
    data_dir().join("games").join(format!("{title_id:016x}")).join("presets")
}

/// A game's presets, by name. The first time a game is opened its list is created from its profile's presets, if any;
/// from then on they are the game's own.
fn game_presets(title_id: u64, profile: Option<&nxport::profile::GameProfile>) -> Vec<PresetInfo> {
    let dir = game_presets_dir(title_id);
    let builtin = profile.map_or(&[][..], |p| p.presets);
    if !builtin.is_empty() && !dir.exists() && std::fs::create_dir_all(&dir).is_ok() {
        for (name, text) in builtin {
            let _ = std::fs::write(dir.join(format!("{name}.toml")), text);
        }
    }
    let mut list: Vec<PresetInfo> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .filter_map(|p| Some(PresetInfo { name: p.file_stem()?.to_string_lossy().into_owned(), path: p }))
        .collect();
    list.sort_by_key(|p| p.name.to_lowercase());
    list
}

/// A name usable as a preset file name.
fn valid_preset_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && !n.starts_with('.') && !n.contains(['/', '\\', ':'])
}

impl Builder {
    fn new(dump: Option<PathBuf>) -> Self {
        let mut b = Builder {
            dump: String::new(),
            game: None,
            target: Target::MacApp,
            mods: String::new(),
            out_dir: default_out_dir(Target::MacApp),
            base: Settings::default(),
            settings: Settings::default(),
            preset_name: String::new(),
            preset_search: String::new(),
            presets: Vec::new(),
            message: None,
            job: None,
            expand_all: false,
            bundle_game: false,
            romfs_size: Arc::new(Mutex::new(None)),
            entitlements: true,
            prebuild_shaders: true,
            bundle_id: String::new(),
        };
        if let Some(d) = dump {
            b.set_dump(d.display().to_string());
        }
        b.rebase();
        b
    }

    fn set_dump(&mut self, dump: String) {
        if self.game.is_some() {
            let _ = self.remember();
        }
        self.dump = dump;
        self.romfs_size = Arc::new(Mutex::new(None));
        let (size, romfs) = (self.romfs_size.clone(), Path::new(&self.dump).join("RomFS"));
        std::thread::spawn(move || *size.lock().unwrap() = Some(dir_size(&romfs)));
        let path = Path::new(&self.dump);
        let id = nxport::profile::identify(path);
        let control = nxport::game::control_of(path);
        self.game = id.title_id.map(|tid| Game {
            name: id.profile.map(|p| p.name.to_string())
                .or_else(|| control.as_ref().and_then(|c| c.title.clone()))
                .unwrap_or_else(|| format!("Switch title {tid:016X}")),
            version: id.version.map(|v| v.version_string.to_string())
                .or_else(|| control.as_ref().and_then(|c| c.version.clone())),
            title_id: tid,
            profile: id.profile,
            is_32bit: nxport::game::is_32bit_of(path),
        });
        self.presets = self.game.as_ref().map_or_else(Vec::new, |g| game_presets(g.title_id, g.profile));
        self.preset_name = self.game.as_ref().and_then(|g| GameMemory::load(g.title_id).preset).unwrap_or_default();
        self.recall();
    }

    /// The mods folder, bundle ID, and settings last used for this game and target.
    fn recall(&mut self) {
        let mem = self.game.as_ref().map(|g| GameMemory::load(g.title_id)).unwrap_or_default().target(self.target).clone();
        let mem = mem.unwrap_or_default();
        let own = Path::new(&self.dump).join("mods");
        self.mods = mem.mods.or_else(|| (!self.dump.is_empty() && own.is_dir()).then(|| own.display().to_string())).unwrap_or_default();
        self.bundle_id = mem.bundle_id.unwrap_or_default();
        let edits = mem.settings.unwrap_or_default();
        self.base = settings::resolve(self.base_layers());
        self.settings = settings::resolve(self.base_layers().into_iter().chain([edits]));
    }

    /// Remembers the preset for this game, and the mods folder, bundle ID, and settings for this game and target.
    fn remember(&self) -> Result<(), String> {
        let Some(g) = &self.game else { return Ok(()) };
        let mut mem = GameMemory::load(g.title_id);
        let some = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        let edits = self.edits();
        // Only a preset that exists: a name typed but never saved is not a selection.
        mem.preset = self.presets.iter().find(|p| p.name.eq_ignore_ascii_case(self.preset_name.trim())).map(|p| p.name.clone());
        *mem.target(self.target) = Some(TargetMemory {
            mods: some(&self.mods),
            bundle_id: if self.target == Target::Ipa { some(&self.bundle_id) } else { None },
            settings: (edits != SettingsPatch::default()).then_some(edits),
        });
        mem.save(g.title_id)
    }

    fn base_layers(&self) -> Vec<SettingsPatch> {
        let p = self.target.platform();
        let mut layers = vec![profiles::platform(p)];
        if let Some(prof) = self.game.as_ref().and_then(|g| g.profile) {
            layers.extend(nxport::profile::layers(prof, p));
        }
        layers
    }

    /// New base settings (another game or target): the edits made so far stay on top.
    fn rebase(&mut self) {
        let edits = self.edits();
        self.base = settings::resolve(self.base_layers());
        self.settings = settings::resolve(self.base_layers().into_iter().chain([edits]));
    }

    /// The edits: what differs from the base.
    fn edits(&self) -> SettingsPatch {
        let mut patch = self.settings.clone().into_patch_by_diff(self.base.clone());
        nxport::profile::keep_own_options(self.game.as_ref().and_then(|g| g.profile), &mut patch);
        patch
    }

    fn frame(&mut self, ui: &imgui::Ui) -> bool {
        if let Some(p) = nxport::ui::window::take_path("dump") {
            self.set_dump(p.display().to_string());
        }
        if let Some(p) = nxport::ui::window::take_path("mods") {
            self.mods = p.display().to_string();
        }
        if let Some(p) = nxport::ui::window::take_path("out") {
            self.out_dir = p.display().to_string();
        }
        let size = ui.io().display_size();
        ui.window("nxport")
            .position([0.0, 0.0], imgui::Condition::Always)
            .size(size, imgui::Condition::Always)
            .flags(imgui::WindowFlags::NO_DECORATION | imgui::WindowFlags::NO_MOVE | imgui::WindowFlags::NO_SAVED_SETTINGS)
            .build(|| self.contents(ui));
        true
    }

    fn contents(&mut self, ui: &imgui::Ui) {
        let running = self.job.as_ref().is_some_and(|j| j.lock().unwrap().result.is_none());
        let _width = ui.push_item_width(-260.0);
        let _disabled = ui.begin_disabled_with_cond(running);

        ui.separator_with_text("Game");
        let mut dump = self.dump.clone();
        if ui.input_text("Dump##dump", &mut dump).build() {
            self.set_dump(dump);
        }
        ui.same_line();
        if ui.button("Choose…##dump") {
            nxport::ui::window::request_path("dump", PathKind::Directory);
        }
        match &self.game {
            Some(g) => {
                ui.text(format!("{} {}", g.name, g.version.as_ref().map_or_else(|| "(unknown version)".to_string(), |v| format!("v{v}"))));
                ui.same_line();
                ui.text_disabled(format!("{:016X}{}", g.title_id, if g.profile.is_some() { "" } else { "  no game profile" }));
                if g.is_32bit {
                    ui.text_colored([1.0, 0.45, 0.4, 1.0], "A 32-bit game: Apple CPUs cannot run 32-bit ARM code, so it is not supported");
                }
            }
            None if !self.dump.is_empty() => ui.text_colored([1.0, 0.45, 0.4, 1.0], "Not a game dump (needs ExeFS/main.npdm and RomFS/)"),
            None => ui.text_disabled("A directory with ExeFS/, RomFS/, Logo/, and control.nacp if needed"),
        }
        self.preset_bar(ui);
        ui.input_text("Mods##mods", &mut self.mods).build();
        ui.same_line();
        if ui.button("Choose…##mods") {
            nxport::ui::window::request_path("mods", PathKind::Directory);
        }

        ui.separator_with_text("Output");
        let prev_target = self.target;
        let mut t = Target::ALL.iter().position(|&x| x == self.target).unwrap_or(0);
        if ui.combo_simple_string("Target", &mut t, &Target::ALL.map(Target::label)) {
            let _ = self.remember();
            self.target = Target::ALL[t];
            if self.out_dir.trim().is_empty() || self.out_dir == default_out_dir(prev_target) {
                self.out_dir = default_out_dir(self.target);
            }
            self.recall();
            // A preset made for the other target no longer describes what is set.
            if self.selected_preset_target().is_some_and(|pt| pt != self.target) {
                self.preset_name.clear();
                let _ = self.remember();
            }
        }
        ui.checkbox("Translate shaders ahead of time", &mut self.prebuild_shaders);
        if ui.is_item_hovered() {
            ui.tooltip_text("Builds the game's shaders into the app (about 25 minutes); the app translates only shaders the cache lacks, and keeps them. Off, the app translates every shader as the game loads it: a much slower first start, then kept for later starts.");
        }
        if self.target == Target::Ipa {
            ui.checkbox("Game files inside the app", &mut self.bundle_game);
            if ui.is_item_hovered() {
                let size = self.romfs_size.lock().unwrap().map_or_else(|| "measuring".to_string(), |b| format!("{:.1} GB", b as f64 / 1e9));
                ui.tooltip_text(format!("Makes the app as large as the game's files ({size}). Without it they are copied into the app's Documents/Game."));
            }
            ui.checkbox("Request entitlements", &mut self.entitlements);
            if ui.is_item_hovered() {
                ui.tooltip_text("Increased memory limit and extended virtual addressing; off for signing setups that refuse them.");
            }
            let default_id = self.game.as_ref().map_or_else(String::new, |g| nxport::profile::bundle_id(g.title_id, g.profile));
            ui.input_text("Bundle ID##bundle_id", &mut self.bundle_id).hint(default_id).build();
        }
        ui.input_text("Folder##out", &mut self.out_dir).build();
        ui.same_line();
        if ui.button("Choose…##out") {
            nxport::ui::window::request_path("out", PathKind::Directory);
        }

        ui.separator_with_text("Settings");
        let all = self.expand_all;
        // The game's own settings, by section (shown first, under the game's full title).
        if let Some(g) = self.game.as_ref().and_then(|g| g.profile) {
            let sections = game_sections(&mut self.settings, g);
            let visible: Vec<_> = sections.into_iter().filter(|(_, s)| s.has(Audience::User)).collect();
            if !visible.is_empty() {
                header(ui, g.name, true, |ui| {
                    for (title, s) in visible {
                        if let Some(_n) = ui.tree_node_config(title).default_open(true).push() {
                            s.edit(ui, Audience::User);
                        }
                    }
                });
            }
        }
        for (i, section) in general_sections(&mut self.settings).into_iter().enumerate() {
            if section.has(Audience::User) {
                header(ui, section.heading(), (self.game.is_none() && i == 0) || all, |ui| {
                    section.edit(ui, Audience::User);
                });
            }
        }
        header(ui, "Developer", all, |ui| {
            if let Some(g) = self.game.as_ref().and_then(|g| g.profile) {
                let sections: Vec<_> = game_sections(&mut self.settings, g)
                    .into_iter()
                    .filter(|(_, s)| s.has(Audience::Developer))
                    .collect();
                if !sections.is_empty() {
                    if let Some(_n) = ui.tree_node_config(g.name).default_open(all).push() {
                        for (title, s) in sections {
                            if let Some(_n2) = ui.tree_node_config(title).default_open(true).push() {
                                s.edit(ui, Audience::Developer);
                            }
                        }
                    }
                }
            }
            for section in general_sections(&mut self.settings) {
                if section.has(Audience::Developer) {
                    if let Some(_n) = ui.tree_node_config(section.heading()).default_open(all).push() {
                        section.edit(ui, Audience::Developer);
                    }
                }
            }
        });
        drop(_disabled);

        ui.separator();
        self.build_bar(ui, running);
    }

    /// The selected preset, if it is one of the game's.
    fn selected_preset(&self) -> Option<&PresetInfo> {
        self.presets.iter().find(|p| p.name.eq_ignore_ascii_case(self.preset_name.trim()))
    }

    /// The target the selected preset is for, if it names one.
    fn selected_preset_target(&self) -> Option<Target> {
        load_builder_preset(&self.selected_preset()?.path).ok()?.0?.target
    }

    /// Loads one of the game's presets and makes it the selection; a preset that fails to load changes nothing.
    fn select_preset(&mut self, name: &str, path: &Path) {
        match self.apply_preset(path) {
            Ok(()) => {
                self.preset_name = name.to_string();
                self.message = None;
                let _ = self.remember();
            }
            Err(e) => self.message = Some(format!("Preset {name}: {e}")),
        }
    }

    fn apply_preset(&mut self, path: &Path) -> Result<(), String> {
        let (build, patch) = load_builder_preset(path)?;
        if let Some(b) = build {
            if let Some(t) = b.target.filter(|&t| t != self.target) {
                let _ = self.remember();
                let prev_target = self.target;
                self.target = t;
                if self.out_dir.trim().is_empty() || self.out_dir == default_out_dir(prev_target) {
                    self.out_dir = default_out_dir(self.target);
                }
                self.recall();
            }
            if let Some(o) = b.out {
                self.out_dir = expand_home(&o);
            }
            if let Some(bg) = b.bundle_game {
                self.bundle_game = bg;
            }
            if let Some(ent) = b.entitlements {
                self.entitlements = ent;
            }
            if let Some(p) = b.prebuild_shaders {
                self.prebuild_shaders = p;
            }
        }
        self.rebase();
        self.settings = settings::resolve(self.base_layers().into_iter().chain([patch]));
        Ok(())
    }

    #[cfg(test)]
    fn apply_preset_by_name(&mut self, name: &str) -> Result<(), String> {
        let p = self.presets.iter().find(|p| p.name.eq_ignore_ascii_case(name)).cloned()
            .ok_or_else(|| format!("Preset not found: {name}"))?;
        self.select_preset(&p.name, &p.path);
        match self.message.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Saves what is set as the game's preset `name` (replacing a preset of that name).
    fn save_preset(&mut self, name: &str) {
        let name = name.trim();
        let Some(g) = &self.game else { return };
        if !valid_preset_name(name) {
            self.message = Some(format!("Not a usable preset name: {name:?}"));
            return;
        }
        let dir = game_presets_dir(g.title_id);
        // Replace a preset whose name differs only in case instead of adding a second one.
        let file = self.presets.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map_or_else(|| dir.join(format!("{name}.toml")), |p| p.path.clone());
        let build = BuildPreset {
            target: Some(self.target),
            out: if self.out_dir.trim().is_empty() { None } else { Some(self.out_dir.trim().to_string()) },
            bundle_game: if self.target == Target::Ipa { Some(self.bundle_game) } else { None },
            entitlements: if self.target == Target::Ipa { Some(self.entitlements) } else { None },
            prebuild_shaders: Some(self.prebuild_shaders),
        };
        match std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| save_builder_preset(&file, build, &self.edits())) {
            Err(e) => self.message = Some(e),
            Ok(()) => {
                self.presets = game_presets(g.title_id, g.profile);
                self.preset_name = file.file_stem().map_or_else(|| name.to_string(), |s| s.to_string_lossy().into_owned());
                self.message = None;
                let _ = self.remember();
            }
        }
    }

    /// Deletes the selected preset from the game's list.
    fn delete_preset(&mut self) {
        let (Some(g), Some(p)) = (&self.game, self.selected_preset().cloned()) else { return };
        if let Err(e) = std::fs::remove_file(&p.path) {
            self.message = Some(format!("{}: {e}", p.path.display()));
            return;
        }
        self.presets = game_presets(g.title_id, g.profile);
        self.preset_name.clear();
        self.message = None;
        let _ = self.remember();
    }

    fn preset_bar(&mut self, ui: &imgui::Ui) {
        if self.game.is_none() {
            let _d = ui.begin_disabled();
            ui.set_next_item_width(260.0);
            if let Some(_t) = ui.begin_combo("Preset", "Choose a game first") {}
            return;
        }
        let preview = if self.preset_name.is_empty() { "Select preset…" } else { &self.preset_name };
        let mut load: Option<PresetInfo> = None;

        ui.set_next_item_width(260.0);
        if let Some(_token) = ui.begin_combo("Preset", preview) {
            if ui.is_window_appearing() {
                ui.set_keyboard_focus_here();
                self.preset_search.clear();
            }
            ui.set_next_item_width(-1.0);
            let enter = ui.input_text("##preset_search", &mut self.preset_search)
                .hint("search or new name…")
                .enter_returns_true(true)
                .build();
            let query = self.preset_search.trim().to_string();
            let exact_match = self.presets.iter().find(|p| p.name.eq_ignore_ascii_case(&query)).cloned();
            if enter {
                match exact_match {
                    Some(p) => load = Some(p),
                    None if valid_preset_name(&query) => self.preset_name = query.clone(),
                    None => {}
                }
                ui.close_current_popup();
            } else if exact_match.is_none() && valid_preset_name(&query) {
                if ui.selectable(format!("New preset: \"{query}\" (then Save)")) {
                    self.preset_name = query.clone();
                    ui.close_current_popup();
                }
                ui.separator();
            }
            let q = query.to_lowercase();
            for p in &self.presets {
                if q.is_empty() || p.name.to_lowercase().contains(&q) {
                    let selected = self.preset_name.eq_ignore_ascii_case(&p.name);
                    if ui.selectable_config(&p.name).selected(selected).build() {
                        load = Some(p.clone());
                    }
                }
            }
        }
        ui.same_line();
        let can_save = valid_preset_name(&self.preset_name);
        let save_clicked = {
            let _d = ui.begin_disabled_with_cond(!can_save);
            ui.button("Save")
        };
        if ui.is_item_hovered_with_flags(imgui::ItemHoveredFlags::ALLOW_WHEN_DISABLED) {
            ui.tooltip_text("Saves the current settings as this game's preset under the shown name.");
        }
        ui.same_line();
        let delete_clicked = {
            let _d = ui.begin_disabled_with_cond(self.selected_preset().is_none());
            ui.button("Delete")
        };

        if let Some(p) = load {
            self.select_preset(&p.name, &p.path);
        }
        if save_clicked && can_save {
            self.save_preset(&self.preset_name.clone());
        }
        if delete_clicked {
            self.delete_preset();
        }
        if let Some(m) = &self.message {
            ui.text_colored([1.0, 0.45, 0.4, 1.0], m);
        }
    }

    fn build_bar(&mut self, ui: &imgui::Ui, running: bool) {
        let ready = self.game.as_ref().is_some_and(|g| !g.is_32bit) && !self.out_dir.trim().is_empty();
        if running {
            if ui.button("Stop after this step") {
                CANCEL.store(true, Ordering::Relaxed);
            }
        } else {
            let _d = ui.begin_disabled_with_cond(!ready);
            if ui.button("Build") {
                self.start();
            }
        }
        let Some(job) = &self.job else { return };
        let j = job.lock().unwrap();
        ui.same_line();
        let fraction = if j.steps == 0 {
            0.0
        } else {
            (j.steps_done as f32 + j.sub_frac) / j.steps as f32
        };
        let overlay = match &j.result {
            None => {
                if let Some(sub) = &j.sub_step {
                    if j.sub_total > 0 {
                        format!("{} ({} of {}): {} ({}/{})", j.step, j.steps_done + 1, j.steps, sub, j.sub_done, j.sub_total)
                    } else {
                        format!("{} ({} of {}): {}", j.step, j.steps_done + 1, j.steps, sub)
                    }
                } else if j.steps > 0 {
                    format!("{} ({} of {})", j.step, j.steps_done + 1, j.steps)
                } else {
                    String::new()
                }
            }
            Some(Ok(app)) => format!("Built {}", app.display()),
            Some(Err(e)) => e.clone(),
        };
        match &j.result {
            None => ui.text(&overlay),
            Some(Ok(app)) => ui.text_colored([0.5, 0.9, 0.5, 1.0], format!("Built {}", app.display())),
            Some(Err(e)) => ui.text_colored([1.0, 0.45, 0.4, 1.0], e),
        }
        drop(j);
        ui.progress_bar(fraction).build();
        ui.child_window("log").size([0.0, 0.0]).border(true).build(ui, || {
            for line in LOG.lock().unwrap().iter() {
                ui.text(line);
            }
            if ui.scroll_y() >= ui.scroll_max_y() - 4.0 {
                ui.set_scroll_here_y(1.0);
            }
        });
    }

    fn start(&mut self) {
        if let Err(e) = self.remember() {
            self.message = Some(e);
        }
        let Some(game) = &self.game else { return };
        let work = data_dir().join("builds").join(format!("{:016x}", game.title_id));
        let dump = PathBuf::from(&self.dump);
        let mods = Some(PathBuf::from(self.mods.trim())).filter(|m| !m.as_os_str().is_empty());
        let out_dir = PathBuf::from(self.out_dir.trim());
        let edits = self.edits();
        let (stages, cache) = (work.join("stages"), work.join("shader_cache"));
        let job = Arc::new(Mutex::new(Job::default()));
        let mut steps: Vec<Step> = Vec::new();
        // Without the cache the app translates every shader as the game loads it.
        let prebuild = self.prebuild_shaders;
        if prebuild {
            {
                let (dump, stages) = (dump.clone(), stages.clone());
                let job = job.clone();
                steps.push(("Extracting shader stages".into(), Box::new(move || {
                    let romfs = dump.join("RomFS");
                    let dirs = nxport_build::extract_shaders::default_dirs(&romfs);
                    let dirs: Vec<&str> = dirs.iter().map(String::as_str).collect();
                    let job_cb = job.clone();
                    let cb = move |sub: nxport_build::extract_shaders::ShaderExtractPhase, done: usize, total: usize| {
                        let mut j = job_cb.lock().unwrap();
                        j.sub_step = Some(sub.to_string());
                        j.sub_done = done;
                        j.sub_total = total;
                        if total > 0 {
                            let f = (done as f32 / total as f32).min(1.0);
                            j.sub_frac = j.sub_frac.max(f);
                        }
                    };
                    nxport_build::extract_shaders::extract_all(&romfs, &stages, &dirs, None, Some(&cb)).map_err(|e| e.to_string())
                })));
            }
            let batch_enabled = self.settings.graphics.batch;
            {
                let (stages, cache) = (stages.clone(), cache.clone());
                let job = job.clone();
                steps.push(("Building the shader cache".into(), Box::new(move || {
                    let mut translation = nxport_build::translation::get();
                    translation.no_batch_kernels = !batch_enabled;
                    nxport_build::translation::install(translation);

                    let cpus = std::thread::available_parallelism().map_or(6, |n| n.get());
                    let jobs = cpus.saturating_sub(2).max(2);
                    let job_cb = job.clone();
                    let on_progress = Arc::new(move |phase: nxport_build::shader_cache_build::ShaderBuildPhase, done: usize, total: usize| {
                        let mut j = job_cb.lock().unwrap();
                        j.sub_step = Some(phase.to_string());
                        j.sub_done = done;
                        j.sub_total = total;
                        let phase_idx = phase.phase_index();
                        let num_phases = nxport_build::shader_cache_build::ShaderBuildPhase::NUM_PHASES;
                        let phase_frac = if total > 0 {
                            (done as f32 / total as f32).min(1.0)
                        } else {
                            0.0
                        };
                        let new_sub_frac = (phase_idx as f32 + phase_frac) / num_phases as f32;
                        j.sub_frac = j.sub_frac.max(new_sub_frac).min(1.0);
                    });
                    nxport_build::shader_cache_build::build(&nxport_build::shader_cache_build::Options {
                        dump_dirs: vec![stages],
                        out_dir: cache,
                        ctrl_dir: None,
                        jobs,
                        msl_out: None,
                        index_only: false,
                        on_progress: Some(on_progress),
                    })
                })));
            }
        }
        let result = match self.target {
            Target::MacApp => {
                let app = out_dir.join(format!("{}.app", game.name.replace([':', '/'], " -")));
                let settings_file = work.join("settings.toml");
                let (dump, cache, app2) = (dump.clone(), cache.clone(), app.clone());
                steps.push(("Packaging the app".into(), Box::new(move || {
                    let mut edits = edits.clone();
                    if mods.is_none() {
                        edits.files.no_mods = Some(true);
                    }
                    settings::save_preset(&settings_file, &edits)?;
                    nxport_build::pack::pack_app(&nxport_build::pack::PackOptions {
                        dump_dir: &dump,
                        mods_dir: mods.as_deref(),
                        shader_cache_dir: prebuild.then_some(cache.as_path()),
                        out_app: Some(&app2),
                        settings: Some(&settings_file),
                        symlink_assets: false,
                    })
                    .map_err(|e| e.to_string())
                })));
                Arc::new(Mutex::new(Some(app)))
            }
            Target::Ipa => {
                let ios_cache = work.join("shader_cache_ios");
                if prebuild {
                    let (cache, ios_cache) = (cache.clone(), ios_cache.clone());
                    let job = job.clone();
                    steps.push(("Building the iOS shader cache".into(), Box::new(move || {
                        let job_cb = job.clone();
                        let cb = move |sub: nxport_build::shader_cache_build::IosShaderBuildPhase, done: usize, total: usize| {
                            let mut j = job_cb.lock().unwrap();
                            j.sub_step = Some(sub.to_string());
                            j.sub_done = done;
                            j.sub_total = total;
                            let phase_idx = sub.phase_index();
                            let num_phases = nxport_build::shader_cache_build::IosShaderBuildPhase::NUM_PHASES;
                            let phase_frac = if total > 0 {
                                (done as f32 / total as f32).min(1.0)
                            } else {
                                0.0
                            };
                            let new_sub_frac = (phase_idx as f32 + phase_frac) / num_phases as f32;
                            j.sub_frac = j.sub_frac.max(new_sub_frac).min(1.0);
                        };
                        let cpus = std::thread::available_parallelism().map_or(6, |n| n.get());
                        let jobs = cpus.saturating_sub(2).max(2);
                        nxport_build::shader_cache_build::build_ios(&cache, &ios_cache, jobs, false, Some(&cb))
                    })));
                }
                let out = Arc::new(Mutex::new(None));
                let (dump, out2) = (dump.clone(), out.clone());
                let (bundle_game, entitlements) = (self.bundle_game, self.entitlements);
                let bundle_id = Some(self.bundle_id.trim().to_string()).filter(|s| !s.is_empty());
                let job = job.clone();
                steps.push(("Building the iOS app".into(), Box::new(move || {
                    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
                    let job_cb = job.clone();
                    let ipa = nxport_build::ipa::build(
                        &nxport_build::ipa::Options {
                            dump: &dump,
                            mods: mods.as_deref(),
                            cache: prebuild.then_some(ios_cache.as_path()),
                            out_dir: &out_dir,
                            edits,
                            bundle_game,
                            entitlements,
                            bundle_id,
                        },
                        &move |step| {
                            println!("[+] {step}");
                            let mut j = job_cb.lock().unwrap();
                            j.sub_step = Some(step.to_string());
                            j.sub_done = 0;
                            j.sub_total = 0;
                            j.sub_frac = j.sub_frac.max(step.default_fraction());
                        },
                    )?;
                    *out2.lock().unwrap() = Some(ipa);
                    Ok(())
                })));
                out
            }
        };
        job.lock().unwrap().steps = steps.len();
        self.job = Some(job.clone());
        LOG.lock().unwrap().clear();
        CANCEL.store(false, Ordering::Relaxed);
        std::thread::spawn(move || {
            let r = std::fs::create_dir_all(&work).map_err(|e| e.to_string()).and_then(|_| run_steps(&job, steps));
            let r = r.and_then(|_| result.lock().unwrap().take().ok_or_else(|| "no output".to_string()));
            job.lock().unwrap().result = Some(r);
        });
    }
}


fn run_steps(job: &Arc<Mutex<Job>>, steps: Vec<Step>) -> Result<(), String> {
    for (i, (name, f)) in steps.into_iter().enumerate() {
        if CANCEL.load(Ordering::Relaxed) {
            return Err("Stopped".into());
        }
        {
            let mut j = job.lock().unwrap();
            j.step = name.clone();
            j.steps_done = i;
            j.sub_step = None;
            j.sub_done = 0;
            j.sub_total = 0;
            j.sub_frac = 0.0;
        }
        println!("== {name}");
        f().map_err(|e| format!("{name}: {e}"))?;
        {
            let mut j = job.lock().unwrap();
            j.steps_done = i + 1;
            j.sub_step = None;
            j.sub_done = 0;
            j.sub_total = 0;
            j.sub_frac = 0.0;
        }
    }
    Ok(())
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| {
        let p = e.path();
        if p.is_dir() { dir_size(&p) } else { e.metadata().map_or(0, |m| m.len()) }
    }).sum()
}

fn tree_flags(open: bool) -> imgui::TreeNodeFlags {
    if open { imgui::TreeNodeFlags::DEFAULT_OPEN } else { imgui::TreeNodeFlags::empty() }
}

/// A collapsing section.
fn header(ui: &imgui::Ui, title: &str, open: bool, body: impl FnOnce(&imgui::Ui)) {
    if ui.collapsing_header(title, tree_flags(open)) {
        let _id = ui.push_id(title);
        body(ui);
    }
}

/// The engine's settings groups, in the order the editor shows them.
fn general_sections(s: &mut Settings) -> Vec<&mut dyn Section> {
    vec![&mut s.system, &mut s.display, &mut s.input, &mut s.audio, &mut s.graphics, &mut s.guest, &mut s.files, &mut s.replay,
        &mut s.trace, &mut s.dump, &mut s.log, &mut s.debug]
}

/// A game's own settings sections.
fn game_sections<'a>(s: &'a mut Settings, game: &nxport::profile::GameProfile) -> Vec<(&'static str, &'a mut dyn Section)> {
    nxport::profile::option_sections(game, s)
}

/// The builder: turns a game dump into an app.
#[derive(clap::Parser)]
#[command(name = "nxport-builder", version)]
struct Args {
    /// Game dump to start with
    dump: Option<PathBuf>,
    /// Render the window into this PNG instead of opening it
    #[arg(long)]
    snapshot: Option<PathBuf>,
    /// Snapshot with every settings group open
    #[arg(long)]
    expand: bool,
    /// Snapshot height in points
    #[arg(long, default_value_t = 760.0)]
    height: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Args { dump, snapshot, expand, height } = clap::Parser::parse();
    let mut b = Builder::new(dump);
    b.expand_all = expand;
    match snapshot {
        Some(png) => {
            let (w, h, bgra) = nxport::ui::window::snapshot(900.0, height, 2.0, 3, Box::new(move |ui| b.frame(ui)))?;
            nxport_build::ppm2png::write_bgra_to_png(w, h, &bgra, &png)?;
        }
        None => {
            capture_output();
            nxport::ui::window::run("nxport", 900.0, 760.0, Box::new(move |ui| b.frame(ui)))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game whose profile comes with presets, and one without a profile.
    const WITH: u64 = 0x0100_0000_0000_A000;
    const WITHOUT: u64 = 0x0100_0000_0000_B000;
    static PROFILE: nxport::profile::GameProfile = nxport::profile::GameProfile {
        kind: nxport::profile::GameKind::Unknown(WITH),
        name: "Test game",
        short_name: "test",
        title_id: WITH,
        lower_left_window_origin: false,
        compute_has_preamble: true,
        versions: &[],
        shader_dirs: &[],
        options: None,
        platform_options: None,
        presets: &[
            ("Mac", "[build]\ntarget = \"mac\"\n"),
            ("iPad", "[build]\ntarget = \"ios\"\n\n[system]\nfps = 30\nhandheld = true\n\n[display]\nlaunch-aspect = \"23:16\"\n"),
            ("iPhone SE", "[build]\ntarget = \"ios\"\n\n[display]\nlaunch-aspect = \"16:9\"\n"),
        ],
    };

    /// A builder with a game loaded (no dump on disk).
    fn with_game(title_id: u64) -> Builder {
        let profile = (title_id == WITH).then_some(&PROFILE);
        let mut b = Builder::new(None);
        b.game = Some(Game {
            name: "Test game".to_string(),
            version: None,
            title_id,
            profile,
            is_32bit: false,
        });
        b.presets = game_presets(title_id, profile);
        b.rebase();
        b
    }

    #[test]
    fn a_game_starts_with_its_profile_presets_and_others_with_none() {
        let b = with_game(WITH);
        let names: Vec<_> = b.presets.iter().map(|p| p.name.as_str()).collect();
        for (n, _) in PROFILE.presets {
            assert!(names.contains(n), "{n} missing: {names:?}");
        }
        let b = with_game(WITHOUT);
        assert!(b.presets.is_empty());
        assert!(!game_presets_dir(WITHOUT).exists(), "nothing written for a game without presets");
    }

    #[test]
    fn presets_apply() {
        let mut b = with_game(WITH);
        b.apply_preset_by_name("iPad").unwrap();
        assert_eq!(b.target, Target::Ipa);
        assert_eq!(b.out_dir, default_out_dir(Target::Ipa));
        assert_eq!(b.settings.system.fps, nxport::settings::FpsMode::Stock30);
        assert!(b.settings.system.handheld);
        assert_eq!(b.settings.display.launch_aspect.map(|a| a.to_string()), Some("23:16".into()));
        b.apply_preset_by_name("iPhone SE").unwrap();
        assert_eq!(b.settings.display.launch_aspect.map(|a| a.to_string()), Some("16:9".into()));
        b.apply_preset_by_name("Mac").unwrap();
        assert_eq!(b.target, Target::MacApp);
        assert_eq!(b.out_dir, default_out_dir(Target::MacApp));
    }

    #[test]
    fn profile_presets_load_and_leave_mods_and_bundle_id_to_the_user() {
        for (name, text) in nxport::profile::PROFILES.iter().flat_map(|p| p.presets) {
            parse_builder_preset(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            let v: toml::Table = toml::from_str(text).unwrap();
            let build = v.get("build").and_then(|b| b.as_table());
            for key in ["mods", "bundle-id"] {
                assert!(!build.is_some_and(|b| b.contains_key(key)), "{name}: {key} belongs to the game, not the device");
            }
        }
    }

    #[test]
    fn saved_presets_have_no_empty_tables() {
        let path = std::env::temp_dir().join(format!("nxport-preset-test-{}.toml", std::process::id()));
        save_builder_preset(&path, BuildPreset::default(), &SettingsPatch::default()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(!text.contains('['), "empty tables written: {text:?}");
    }

    #[test]
    fn presets_belong_to_their_game() {
                let mut a = with_game(WITH);
        a.settings.system.handheld = true;
        a.save_preset("Handheld");
        assert_eq!(a.preset_name, "Handheld");
        // A built-in preset can be removed from the game's list.
        a.apply_preset_by_name("Mac").unwrap();
        a.delete_preset();
        let mut b = with_game(WITHOUT);
        assert!(b.presets.is_empty());
        b.save_preset("Mine");
        let names = |tid| game_presets(tid, (tid == WITH).then_some(&PROFILE)).into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(names(WITHOUT), ["Mine"]);
        let t = names(WITH);
        assert!(t.contains(&"Handheld".to_string()) && !t.contains(&"Mac".to_string()) && !t.contains(&"Mine".to_string()), "{t:?}");
        // Deleting every preset does not bring the built-in ones back.
        for p in game_presets(WITH, Some(&PROFILE)) {
            std::fs::remove_file(p.path).unwrap();
        }
        assert!(game_presets(WITH, Some(&PROFILE)).is_empty());
    }

    #[test]
    fn saving_under_another_case_replaces_the_preset() {
        let mut b = with_game(WITH);
        b.save_preset("ipad");
        let n = b.presets.iter().filter(|p| p.name.eq_ignore_ascii_case("ipad")).count();
        assert_eq!(n, 1);
        assert_eq!(b.preset_name, "iPad");
    }

    #[test]
    fn unusable_names_are_refused() {
        let mut b = with_game(WITH);
        for name in ["", "  ", "a/b", "..", ".hidden"] {
            b.save_preset(name);
            assert!(b.message.is_some(), "{name:?} accepted");
        }
    }

    #[test]
    fn non_totk_games_strip_totk_options() {
        let mut b = Builder::new(None);
        b.game = Some(Game {
            name: "Kirby and the Forgotten Land".to_string(),
            version: Some("1.0.0".to_string()),
            title_id: nxport::profile::KIRBY_TITLE_ID,
            profile: Some(&nxport::profile::kirby::PROFILE),
            is_32bit: false,
        });
        b.settings.totk.v1_1_2.dynamic_aspect_ratio = false;
        let edits = b.edits();
        assert_eq!(edits.totk, nxport::profile::totk::OptionsPatch::default());
    }

    #[test]
    fn game_memory_round_trips() {
        let mut mem = GameMemory { preset: Some("60fps".to_string()), ..Default::default() };
        let mut patch = SettingsPatch::default();
        patch.system.fps = Some(nxport::settings::FpsMode::Unlocked);
        *mem.target(Target::MacApp) = Some(TargetMemory {
            mods: Some("my_kirby_mods".to_string()),
            bundle_id: None,
            settings: Some(patch.clone()),
        });
        let back: GameMemory = toml::from_str(&toml::to_string_pretty(&mem).unwrap()).unwrap();
        assert_eq!(back.preset.as_deref(), Some("60fps"));
        let target = back.mac.as_ref().unwrap();
        assert_eq!(target.settings.as_ref(), Some(&patch));
        assert_eq!(target.mods.as_deref(), Some("my_kirby_mods"));
        // Files written before the preset moved out of the target tables still load.
        let old: GameMemory = toml::from_str("[mac]\npreset = \"Mac\"\nmods = \"m\"\n").unwrap();
        assert_eq!(old.mac.unwrap().mods.as_deref(), Some("m"));
    }

    #[test]
    fn selecting_a_preset_for_another_target_keeps_the_selection() {
        let mut b = with_game(WITH);
        assert_eq!(b.target, Target::MacApp);
        b.apply_preset_by_name("iPhone SE").unwrap();
        assert_eq!(b.target, Target::Ipa);
        assert_eq!(b.preset_name, "iPhone SE");
        b.apply_preset_by_name("Mac").unwrap();
        assert_eq!(b.target, Target::MacApp);
        assert_eq!(b.preset_name, "Mac");
    }

    #[test]
    fn a_preset_that_fails_to_load_changes_nothing() {
        let mut b = with_game(WITH);
        b.apply_preset_by_name("iPad").unwrap();
        let before = (b.preset_name.clone(), b.target, b.settings.clone());
        let broken = game_presets_dir(WITH).join("Broken.toml");
        std::fs::write(&broken, "not = [valid").unwrap();
        b.select_preset("Broken", &broken);
        assert_eq!((b.preset_name.clone(), b.target, b.settings.clone()), before);
        assert!(b.message.is_some());
    }

    #[test]
    fn the_selected_preset_target_is_known() {
        let mut b = with_game(WITH);
        b.apply_preset_by_name("iPad").unwrap();
        assert_eq!(b.selected_preset_target(), Some(Target::Ipa));
        b.preset_name = "not a preset".into();
        assert_eq!(b.selected_preset_target(), None);
    }
}
