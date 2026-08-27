use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const MAX_SETTINGS_BYTES: u64 = 64 * 1024;
const MAX_SHORTCUT_BYTES: usize = 96;

pub const COMBAT_OVERLAY_TOGGLE_ACTION_ID: &str = "app.rlogs.combat-overlay.toggle-visibility";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyActionDefinition {
    pub action_id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub category: &'static str,
}

pub const HOTKEY_ACTIONS: &[HotkeyActionDefinition] = &[HotkeyActionDefinition {
    action_id: COMBAT_OVERLAY_TOGGLE_ACTION_ID,
    label: "Show/hide Combat Overlay",
    description: "Toggle the live Combat Overlay while another application has focus.",
    category: "Combat Overlay",
}];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HotkeySettings {
    pub schema_version: u16,
    pub bindings: BTreeMap<String, String>,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            bindings: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeySettingsView {
    pub schema_version: u16,
    pub actions: &'static [HotkeyActionDefinition],
    pub bindings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HotkeyAssignmentRequest {
    pub action_id: String,
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyAssignmentResult {
    pub settings: HotkeySettingsView,
    pub displaced_action_id: Option<String>,
}

#[derive(Debug)]
pub struct HotkeySettingsStore {
    path: PathBuf,
    settings: HotkeySettings,
}

impl HotkeySettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let settings = load(&path)?;
        Ok(Self { path, settings })
    }

    pub fn snapshot(&self) -> HotkeySettingsView {
        HotkeySettingsView {
            schema_version: self.settings.schema_version,
            actions: HOTKEY_ACTIONS,
            bindings: self.settings.bindings.clone(),
        }
    }

    pub fn assign(
        &mut self,
        request: HotkeyAssignmentRequest,
    ) -> Result<HotkeyAssignmentResult, String> {
        validate_action_id(&request.action_id)?;
        let shortcut = request
            .shortcut
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        if let Some(shortcut) = shortcut.as_deref() {
            validate_shortcut(shortcut)?;
        }

        let previous = self.settings.clone();
        let displaced_action_id =
            assign_binding(&mut self.settings.bindings, &request.action_id, shortcut);
        if let Err(error) = write(&self.path, &self.settings) {
            self.settings = previous;
            return Err(error);
        }
        Ok(HotkeyAssignmentResult {
            settings: self.snapshot(),
            displaced_action_id,
        })
    }

    pub fn restore_bindings(
        &mut self,
        bindings: BTreeMap<String, String>,
    ) -> Result<HotkeySettingsView, String> {
        let previous = self.settings.clone();
        self.settings.bindings = bindings;
        validate(&self.settings)?;
        if let Err(error) = write(&self.path, &self.settings) {
            self.settings = previous;
            return Err(error);
        }
        Ok(self.snapshot())
    }
}

fn assign_binding(
    bindings: &mut BTreeMap<String, String>,
    action_id: &str,
    shortcut: Option<String>,
) -> Option<String> {
    let displaced = shortcut.as_deref().and_then(|shortcut| {
        bindings
            .iter()
            .find(|(candidate, binding)| {
                candidate.as_str() != action_id && binding.eq_ignore_ascii_case(shortcut)
            })
            .map(|(candidate, _)| candidate.clone())
    });
    if let Some(displaced) = displaced.as_deref() {
        bindings.remove(displaced);
    }
    match shortcut {
        Some(shortcut) => {
            bindings.insert(action_id.to_owned(), shortcut);
        }
        None => {
            bindings.remove(action_id);
        }
    }
    displaced
}

fn load(path: &Path) -> Result<HotkeySettings, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HotkeySettings::default());
        }
        Err(error) => return Err(format!("could not inspect Hotkey settings: {error}")),
    };
    if metadata.len() > MAX_SETTINGS_BYTES {
        return Err("Hotkey settings exceed the 64 KiB safety limit".into());
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("could not read Hotkey settings: {error}"))?;
    let settings: HotkeySettings = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Hotkey settings are invalid: {error}"))?;
    validate(&settings)?;
    Ok(settings)
}

fn validate(settings: &HotkeySettings) -> Result<(), String> {
    if settings.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported Hotkey settings schema {}; expected {SCHEMA_VERSION}",
            settings.schema_version
        ));
    }
    for (action_id, shortcut) in &settings.bindings {
        validate_action_id(action_id)?;
        validate_shortcut(shortcut)?;
    }
    let mut unique = BTreeMap::<String, &str>::new();
    for (action_id, shortcut) in &settings.bindings {
        let normalized = shortcut.to_ascii_lowercase();
        if let Some(existing) = unique.insert(normalized, action_id) {
            return Err(format!(
                "hotkey {shortcut} is assigned to both {existing} and {action_id}"
            ));
        }
    }
    Ok(())
}

fn validate_action_id(action_id: &str) -> Result<(), String> {
    if HOTKEY_ACTIONS
        .iter()
        .any(|action| action.action_id == action_id)
    {
        Ok(())
    } else {
        Err(format!("unknown hotkey action {action_id}"))
    }
}

fn validate_shortcut(shortcut: &str) -> Result<(), String> {
    if shortcut.is_empty() {
        return Err("a blank shortcut must be stored as no binding".into());
    }
    if shortcut.len() > MAX_SHORTCUT_BYTES {
        return Err(format!(
            "hotkey shortcut exceeds {MAX_SHORTCUT_BYTES} bytes"
        ));
    }
    if shortcut.contains('\0') || shortcut.chars().any(char::is_control) {
        return Err("hotkey shortcut contains an invalid control character".into());
    }
    let parts = shortcut.split('+').collect::<Vec<_>>();
    if parts.iter().any(|part| part.trim().is_empty()) || parts.len() > 5 {
        return Err("hotkey shortcut has an invalid key combination".into());
    }
    if parts.len() == 1 && !is_standalone_key(parts[0]) {
        return Err("letter, number, and punctuation shortcuts require a modifier".into());
    }
    Ok(())
}

fn is_standalone_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper
        .strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number))
        || matches!(upper.as_str(), "PAUSE" | "PRINTSCREEN" | "SCROLLLOCK")
}

fn write(path: &Path, settings: &HotkeySettings) -> Result<(), String> {
    validate(settings)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Hotkey settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create Hotkey settings folder: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("could not encode Hotkey settings: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes).map_err(|error| format!("could not write Hotkey settings: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rlogs-hotkey-settings-{}-{name}.json",
            std::process::id()
        ))
    }

    #[test]
    fn defaults_empty_and_assignments_round_trip() {
        let path = test_path("round-trip");
        let _ = std::fs::remove_file(&path);
        let mut store = HotkeySettingsStore::open(&path).unwrap();
        assert!(store.snapshot().bindings.is_empty());
        store
            .assign(HotkeyAssignmentRequest {
                action_id: COMBAT_OVERLAY_TOGGLE_ACTION_ID.into(),
                shortcut: Some("Ctrl+Shift+O".into()),
            })
            .unwrap();
        assert_eq!(
            HotkeySettingsStore::open(&path)
                .unwrap()
                .snapshot()
                .bindings
                .get(COMBAT_OVERLAY_TOGGLE_ACTION_ID),
            Some(&"Ctrl+Shift+O".to_owned())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn assigning_an_existing_shortcut_displaces_the_previous_action() {
        let mut bindings = BTreeMap::from([
            ("action.one".to_owned(), "Ctrl+Shift+KeyO".to_owned()),
            ("action.two".to_owned(), "Ctrl+Shift+KeyP".to_owned()),
        ]);
        let displaced = assign_binding(&mut bindings, "action.two", Some("ctrl+shift+keyo".into()));
        assert_eq!(displaced.as_deref(), Some("action.one"));
        assert!(!bindings.contains_key("action.one"));
        assert_eq!(
            bindings.get("action.two"),
            Some(&"ctrl+shift+keyo".to_owned())
        );
    }

    #[test]
    fn clearing_and_invalid_unmodified_letters_are_handled() {
        let path = test_path("clear");
        let _ = std::fs::remove_file(&path);
        let mut store = HotkeySettingsStore::open(&path).unwrap();
        assert!(
            store
                .assign(HotkeyAssignmentRequest {
                    action_id: COMBAT_OVERLAY_TOGGLE_ACTION_ID.into(),
                    shortcut: Some("O".into()),
                })
                .is_err()
        );
        store
            .assign(HotkeyAssignmentRequest {
                action_id: COMBAT_OVERLAY_TOGGLE_ACTION_ID.into(),
                shortcut: Some("F10".into()),
            })
            .unwrap();
        store
            .assign(HotkeyAssignmentRequest {
                action_id: COMBAT_OVERLAY_TOGGLE_ACTION_ID.into(),
                shortcut: None,
            })
            .unwrap();
        assert!(store.snapshot().bindings.is_empty());
        let _ = std::fs::remove_file(path);
    }
}
