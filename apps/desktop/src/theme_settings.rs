use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const MAX_SETTINGS_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    Midnight,
    Graphite,
    Aurora,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeDensity {
    Compact,
    Comfortable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeFont {
    System,
    Humanist,
    Mono,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeSettings {
    pub schema_version: u16,
    pub preset: ThemePreset,
    pub density: ThemeDensity,
    pub font: ThemeFont,
    pub font_scale_percent: u16,
    pub accent: String,
    pub background: String,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            preset: ThemePreset::Midnight,
            density: ThemeDensity::Comfortable,
            font: ThemeFont::System,
            font_scale_percent: 100,
            accent: "#64dfd2".into(),
            background: "soft-glow".into(),
        }
    }
}

#[derive(Debug)]
pub struct ThemeSettingsStore {
    path: PathBuf,
    settings: ThemeSettings,
}

impl ThemeSettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let settings = load(&path)?;
        Ok(Self { path, settings })
    }

    pub fn snapshot(&self) -> ThemeSettings {
        self.settings.clone()
    }

    pub fn update(&mut self, settings: ThemeSettings) -> Result<ThemeSettings, String> {
        validate(&settings)?;
        write(&self.path, &settings)?;
        self.settings = settings;
        Ok(self.snapshot())
    }
}

fn load(path: &Path) -> Result<ThemeSettings, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ThemeSettings::default());
        }
        Err(error) => return Err(format!("could not inspect Themes settings: {error}")),
    };
    if metadata.len() > MAX_SETTINGS_BYTES {
        return Err("Themes settings exceed the 64 KiB safety limit".into());
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("could not read Themes settings: {error}"))?;
    let settings = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Themes settings are invalid: {error}"))?;
    validate(&settings)?;
    Ok(settings)
}

fn validate(settings: &ThemeSettings) -> Result<(), String> {
    if settings.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported Themes settings schema {}; expected {SCHEMA_VERSION}",
            settings.schema_version
        ));
    }
    if !(85..=130).contains(&settings.font_scale_percent) {
        return Err("font scale must be between 85 and 130 percent".into());
    }
    let accent = settings.accent.as_bytes();
    if accent.len() != 7 || accent[0] != b'#' || !accent[1..].iter().all(u8::is_ascii_hexdigit) {
        return Err("accent must be a six-digit hexadecimal color".into());
    }
    if !matches!(
        settings.background.as_str(),
        "none" | "soft-glow" | "aurora" | "glass"
    ) {
        return Err("unsupported background treatment".into());
    }
    Ok(())
}

fn write(path: &Path, settings: &ThemeSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Themes settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create Themes settings folder: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("could not encode Themes settings: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes).map_err(|error| format!("could not write Themes settings: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_user_visible_theme_controls() {
        let mut settings = ThemeSettings::default();
        settings.accent = "#AABBCC".into();
        settings.font_scale_percent = 115;
        settings.background = "glass".into();
        assert!(validate(&settings).is_ok());
        settings.accent = "blue".into();
        assert!(validate(&settings).is_err());
    }
}
