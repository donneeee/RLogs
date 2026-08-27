use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const MAX_SETTINGS_BYTES: u64 = 512 * 1024;
const MAX_IDENTIFIERS: usize = 2_048;
const MAX_IDENTIFIER_BYTES: usize = 192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutSettings {
    pub schema_version: u16,
    pub workspace_order: Vec<String>,
    pub active_workspace_id: Option<String>,
    pub active_tabs: BTreeMap<String, String>,
    pub tab_orders: BTreeMap<String, Vec<String>>,
    pub section_orders: BTreeMap<String, Vec<String>>,
    pub lock_tab_dragging: bool,
    pub lock_section_dragging: bool,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            workspace_order: Vec::new(),
            active_workspace_id: None,
            active_tabs: BTreeMap::new(),
            tab_orders: BTreeMap::new(),
            section_orders: BTreeMap::new(),
            lock_tab_dragging: false,
            lock_section_dragging: false,
        }
    }
}

#[derive(Debug)]
pub struct LayoutSettingsStore {
    path: PathBuf,
    settings: LayoutSettings,
}

impl LayoutSettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let settings = load(&path)?;
        Ok(Self { path, settings })
    }

    pub fn snapshot(&self) -> LayoutSettings {
        self.settings.clone()
    }

    pub fn update(&mut self, settings: LayoutSettings) -> Result<LayoutSettings, String> {
        validate(&settings)?;
        write(&self.path, &settings)?;
        self.settings = settings;
        Ok(self.snapshot())
    }
}

fn load(path: &Path) -> Result<LayoutSettings, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LayoutSettings::default());
        }
        Err(error) => return Err(format!("could not inspect layout settings: {error}")),
    };
    if metadata.len() > MAX_SETTINGS_BYTES {
        return Err("layout settings exceed the 512 KiB safety limit".into());
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("could not read layout settings: {error}"))?;
    let settings = serde_json::from_slice(&bytes)
        .map_err(|error| format!("layout settings are invalid: {error}"))?;
    validate(&settings)?;
    Ok(settings)
}

fn validate(settings: &LayoutSettings) -> Result<(), String> {
    if settings.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported layout settings schema {}; expected {SCHEMA_VERSION}",
            settings.schema_version
        ));
    }
    let mut count = 0usize;
    validate_unique_list("workspace order", &settings.workspace_order, &mut count)?;
    validate_optional_identifier(
        "active workspace",
        settings.active_workspace_id.as_deref(),
        &mut count,
    )?;
    for (workspace, tab) in &settings.active_tabs {
        validate_identifier("active tab workspace", workspace, &mut count)?;
        validate_identifier("active tab", tab, &mut count)?;
    }
    for (workspace, tabs) in &settings.tab_orders {
        validate_identifier("tab order workspace", workspace, &mut count)?;
        validate_unique_list("tab order", tabs, &mut count)?;
    }
    for (workspace, sections) in &settings.section_orders {
        validate_identifier("section order workspace", workspace, &mut count)?;
        validate_unique_list("section order", sections, &mut count)?;
    }
    Ok(())
}

fn validate_unique_list(field: &str, values: &[String], count: &mut usize) -> Result<(), String> {
    let mut unique = BTreeSet::new();
    for value in values {
        validate_identifier(field, value, count)?;
        if !unique.insert(value) {
            return Err(format!("{field} contains duplicate identifier {value}"));
        }
    }
    Ok(())
}

fn validate_optional_identifier(
    field: &str,
    value: Option<&str>,
    count: &mut usize,
) -> Result<(), String> {
    if let Some(value) = value {
        validate_identifier(field, value, count)?;
    }
    Ok(())
}

fn validate_identifier(field: &str, value: &str, count: &mut usize) -> Result<(), String> {
    *count = count
        .checked_add(1)
        .ok_or_else(|| "layout identifier count overflowed".to_owned())?;
    if *count > MAX_IDENTIFIERS {
        return Err(format!(
            "layout settings exceed {MAX_IDENTIFIERS} identifiers"
        ));
    }
    if value.is_empty() || value.len() > MAX_IDENTIFIER_BYTES || value.contains('\0') {
        return Err(format!("{field} contains an invalid identifier"));
    }
    Ok(())
}

fn write(path: &Path, settings: &LayoutSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "layout settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create layout settings folder: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("could not encode layout settings: {error}"))?;
    bytes.push(b'\n');
    std::fs::write(path, bytes).map_err(|error| format!("could not write layout settings: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_section_order_is_persisted_without_changing_membership() {
        let settings = LayoutSettings {
            tab_orders: BTreeMap::from([(
                "app.rlogs.combat-meter".into(),
                vec![
                    "app.rlogs.combat-meter:options".into(),
                    "app.rlogs.combat-meter:history".into(),
                ],
            )]),
            section_orders: BTreeMap::from([(
                "app.rlogs.combat-meter".into(),
                vec![
                    "app.rlogs.combat-overlay:overlay".into(),
                    "app.rlogs.combat-meter:meter".into(),
                ],
            )]),
            ..LayoutSettings::default()
        };
        assert!(validate(&settings).is_ok());
    }
}
