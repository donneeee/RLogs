use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const MAX_SETTINGS_BYTES: u64 = 64 * 1024;
const MAX_CAPTURE_INTERFACE_BYTES: usize = 512;
const MAX_DUMPCAP_PATH_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoreSettings {
    pub schema_version: u16,
    pub close_to_tray: bool,
    #[serde(default)]
    pub hide_overlays_when_unfocused: bool,
    /// Presentation-only clock policy shared by every overlay. Canonical run,
    /// encounter, and submission timing is never rewritten by this setting.
    #[serde(default = "default_pause_overlay_timers_outside_combat")]
    pub pause_overlay_timers_outside_combat: bool,
    #[serde(default = "default_overlay_timer_inactivity_seconds")]
    pub overlay_timer_inactivity_seconds: u16,
    pub capture_interface: Option<String>,
    pub dumpcap_path: Option<String>,
}

impl Default for CoreSettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            close_to_tray: false,
            hide_overlays_when_unfocused: false,
            pause_overlay_timers_outside_combat: default_pause_overlay_timers_outside_combat(),
            overlay_timer_inactivity_seconds: default_overlay_timer_inactivity_seconds(),
            capture_interface: None,
            dumpcap_path: None,
        }
    }
}

const fn default_pause_overlay_timers_outside_combat() -> bool {
    true
}

const fn default_overlay_timer_inactivity_seconds() -> u16 {
    8
}

#[derive(Debug)]
pub struct CoreSettingsStore {
    path: PathBuf,
    settings: CoreSettings,
}

impl CoreSettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let settings = load(&path)?;
        Ok(Self { path, settings })
    }

    pub fn snapshot(&self) -> CoreSettings {
        self.settings.clone()
    }

    pub fn update(&mut self, settings: CoreSettings) -> Result<CoreSettings, String> {
        validate(&settings)?;
        write(&self.path, &settings)?;
        self.settings = settings;
        Ok(self.snapshot())
    }
}

fn load(path: &Path) -> Result<CoreSettings, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CoreSettings::default());
        }
        Err(error) => return Err(format!("could not inspect Core settings: {error}")),
    };
    if metadata.len() > MAX_SETTINGS_BYTES {
        return Err("Core settings exceed the 64 KiB safety limit".into());
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("could not read Core settings: {error}"))?;
    let settings = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Core settings are invalid: {error}"))?;
    validate(&settings)?;
    Ok(settings)
}

fn validate(settings: &CoreSettings) -> Result<(), String> {
    if settings.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported Core settings schema {}; expected {SCHEMA_VERSION}",
            settings.schema_version
        ));
    }
    if settings.overlay_timer_inactivity_seconds > 300 {
        return Err("overlay timer inactivity delay must be between 0 and 300 seconds".into());
    }
    validate_optional_text(
        "capture interface",
        settings.capture_interface.as_deref(),
        MAX_CAPTURE_INTERFACE_BYTES,
    )?;
    validate_optional_text(
        "dumpcap path",
        settings.dumpcap_path.as_deref(),
        MAX_DUMPCAP_PATH_BYTES,
    )
}

fn validate_optional_text(field: &str, value: Option<&str>, maximum: usize) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.trim().is_empty() {
        return Err(format!("{field} must be omitted instead of blank"));
    }
    if value.len() > maximum {
        return Err(format!("{field} exceeds {maximum} bytes"));
    }
    if value.contains('\0') {
        return Err(format!("{field} contains a null byte"));
    }
    Ok(())
}

fn write(path: &Path, settings: &CoreSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Core settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create Core settings folder: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("could not encode Core settings: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes).map_err(|error| format!("could not write Core settings: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rlogs-core-settings-{}-{name}.json",
            std::process::id()
        ))
    }

    #[test]
    fn defaults_are_private_and_updates_round_trip() {
        let path = test_path("round-trip");
        let _ = std::fs::remove_file(&path);
        let mut store = CoreSettingsStore::open(&path).unwrap();
        assert_eq!(store.snapshot(), CoreSettings::default());
        let updated = CoreSettings {
            close_to_tray: true,
            capture_interface: Some("3".into()),
            dumpcap_path: Some("C:\\Program Files\\Wireshark\\dumpcap.exe".into()),
            ..CoreSettings::default()
        };
        store.update(updated.clone()).unwrap();
        assert_eq!(CoreSettingsStore::open(&path).unwrap().snapshot(), updated);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn invalid_or_oversized_values_fail_closed() {
        let path = test_path("invalid");
        let mut store = CoreSettingsStore::open(&path).unwrap();
        let invalid = CoreSettings {
            capture_interface: Some(" ".into()),
            ..CoreSettings::default()
        };
        assert!(store.update(invalid).is_err());
        assert_eq!(store.snapshot(), CoreSettings::default());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn existing_settings_default_the_overlay_focus_policy_off() {
        let path = test_path("overlay-focus-default");
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            br#"{
  "schemaVersion": 1,
  "closeToTray": true,
  "captureInterface": null,
  "dumpcapPath": null
}
"#,
        )
        .unwrap();
        let settings = CoreSettingsStore::open(&path).unwrap().snapshot();
        assert!(settings.close_to_tray);
        assert!(!settings.hide_overlays_when_unfocused);
        assert!(settings.pause_overlay_timers_outside_combat);
        assert_eq!(settings.overlay_timer_inactivity_seconds, 8);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_an_unbounded_overlay_timer_delay() {
        let path = test_path("overlay-timer-delay");
        let _ = std::fs::remove_file(&path);
        let mut store = CoreSettingsStore::open(&path).unwrap();
        let invalid = CoreSettings {
            overlay_timer_inactivity_seconds: 301,
            ..CoreSettings::default()
        };
        assert!(store.update(invalid).is_err());
        let _ = std::fs::remove_file(path);
    }
}
