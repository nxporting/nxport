// Build presets and the builder's memory of each game and target: saving, loading and parsing preset files, the
// presets a game has.

use super::*;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub(crate) struct BuildPreset {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target: Option<Target>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bundle_game: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) entitlements: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prebuild_shaders: Option<bool>,
}

/// What the builder remembers per game and target: the mods folder, bundle ID, and settings used last.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub(crate) struct TargetMemory {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mods: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bundle_id: Option<String>,
    /// iOS: the GPU the pipeline list is precompiled for (nxport_builderlib::pipeline_archive::IOS_GPUS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) gpu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) settings: Option<SettingsPatch>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct GameMemory {
    /// The preset selected last for this game (whatever the target).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mac: Option<TargetMemory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ios: Option<TargetMemory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) windows: Option<TargetMemory>,
}

impl GameMemory {
    pub(crate) fn path(title_id: u64) -> PathBuf {
        data_dir().join("games").join(format!("{title_id:016x}.toml"))
    }

    pub(crate) fn load(title_id: u64) -> GameMemory {
        std::fs::read_to_string(Self::path(title_id)).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub(crate) fn save(&self, title_id: u64) -> Result<(), String> {
        let path = Self::path(title_id);
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(path.parent().unwrap()).and_then(|_| std::fs::write(&path, text)).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub(crate) fn target(&mut self, t: Target) -> &mut Option<TargetMemory> {
        match t {
            Target::MacApp => &mut self.mac,
            Target::Ipa => &mut self.ios,
            Target::WindowsApp => &mut self.windows,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct PresetFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) build: Option<BuildPreset>,
    #[serde(flatten)]
    pub(crate) settings: SettingsPatch,
}

pub(crate) fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(rest).display().to_string();
        }
    }
    p.to_string()
}

pub(crate) fn save_builder_preset(path: &Path, build: BuildPreset, edits: &SettingsPatch) -> Result<(), String> {
    let file = PresetFile {
        build: Some(build),
        settings: edits.clone(),
    };
    let mut v = toml::Value::try_from(&file).map_err(|e| e.to_string())?;
    settings::prune_empty(&mut v);
    let text = toml::to_string_pretty(&v).or_else(|_| toml::to_string(&v)).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

pub(crate) fn parse_builder_preset(text: &str) -> Result<(Option<BuildPreset>, SettingsPatch), String> {
    let preset: PresetFile = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok((preset.build, preset.settings))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PresetInfo {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
}

pub(crate) fn load_builder_preset(path: &Path) -> Result<(Option<BuildPreset>, SettingsPatch), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_builder_preset(&text)
}

/// A game's presets: `games/<title id>/presets/*.toml` in the builder's data directory.
pub(crate) fn game_presets_dir(title_id: u64) -> PathBuf {
    data_dir().join("games").join(format!("{title_id:016x}")).join("presets")
}

/// A game's presets, by name. The first time a game is opened its list is created from its profile's presets, if any;
/// from then on they are the game's own.
pub(crate) fn game_presets(title_id: u64, profile: Option<&nxport::profile::GameProfile>) -> Vec<PresetInfo> {
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
pub(crate) fn valid_preset_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && !n.starts_with('.') && !n.contains(['/', '\\', ':'])
}
