use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use std::fmt;

const SCHEMA_VERSION: u16 = 2;
const MAX_SETTINGS_BYTES: u64 = 128 * 1024;
const MAX_SETUPS: usize = 16;
const MODULE_IDS: [&str; 7] = [
    "map",
    "player",
    "actions",
    "party",
    "target",
    "objectives",
    "alerts",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayModuleLayout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub visible: bool,
    pub z_order: u16,
    pub opacity: f64,
    pub scale: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlaySetupLayout {
    pub name: String,
    pub locked: bool,
    pub modules: BTreeMap<String, OverlayModuleLayout>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayLayoutSettings {
    pub schema_version: u16,
    pub revision: u64,
    pub canvas_enabled: bool,
    pub selected_setup_id: String,
    pub legacy_migration_complete: bool,
    pub setups: BTreeMap<String, OverlaySetupLayout>,
}

impl Default for OverlayLayoutSettings {
    fn default() -> Self {
        let setup_id = "default".to_owned();
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            canvas_enabled: false,
            selected_setup_id: setup_id.clone(),
            legacy_migration_complete: false,
            setups: BTreeMap::from([(setup_id, default_setup())]),
        }
    }
}

fn module(x: f64, y: f64, width: f64, height: f64, z_order: u16) -> OverlayModuleLayout {
    OverlayModuleLayout {
        x,
        y,
        width,
        height,
        visible: true,
        z_order,
        opacity: 1.0,
        scale: 1.0,
    }
}

fn default_setup() -> OverlaySetupLayout {
    OverlaySetupLayout {
        name: "Default HUD".into(),
        locked: false,
        modules: BTreeMap::from([
            ("map".into(), module(0.015, 0.15, 0.325, 0.65, 1)),
            ("player".into(), module(0.015, 0.06, 0.263, 0.16, 2)),
            ("actions".into(), module(0.015, 0.79, 0.325, 0.16, 3)),
            ("party".into(), module(0.65, 0.06, 0.225, 0.48, 4)),
            ("target".into(), module(0.363, 0.06, 0.263, 0.18, 5)),
            ("objectives".into(), module(0.363, 0.45, 0.263, 0.28, 6)),
            ("alerts".into(), module(0.65, 0.53, 0.225, 0.28, 7)),
        ]),
    }
}

#[derive(Debug)]
pub struct OverlayLayoutSettingsStore {
    path: PathBuf,
    settings: OverlayLayoutSettings,
}

#[derive(Debug)]
pub enum OverlayLayoutUpdateError {
    Conflict(String),
    Validation(String),
    Io(String),
}

impl fmt::Display for OverlayLayoutUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict(value) | Self::Validation(value) | Self::Io(value) => {
                formatter.write_str(value)
            }
        }
    }
}

impl OverlayLayoutSettingsStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let settings = load(&path)?;
        Ok(Self { path, settings })
    }

    pub fn snapshot(&self) -> OverlayLayoutSettings {
        self.settings.clone()
    }

    pub fn update(
        &mut self,
        mut settings: OverlayLayoutSettings,
    ) -> Result<OverlayLayoutSettings, OverlayLayoutUpdateError> {
        if settings.revision != self.settings.revision {
            return Err(OverlayLayoutUpdateError::Conflict(
                "overlay layout changed in another window; reload before saving".into(),
            ));
        }
        normalize_and_validate(&mut settings).map_err(OverlayLayoutUpdateError::Validation)?;
        settings.revision = self.settings.revision.saturating_add(1);
        write(&self.path, &settings).map_err(OverlayLayoutUpdateError::Io)?;
        self.settings = settings;
        Ok(self.snapshot())
    }

    pub fn set_canvas_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<OverlayLayoutSettings, OverlayLayoutUpdateError> {
        if self.settings.canvas_enabled == enabled {
            return Ok(self.snapshot());
        }
        let mut settings = self.snapshot();
        settings.canvas_enabled = enabled;
        self.update(settings)
    }

    /// Enables the shared canvas in passive play mode in one durable update.
    ///
    /// A native hotkey bypasses the editor's save-before-show path, so the
    /// host must commit both facts together: the canvas is enabled and the
    /// selected setup is locked. This prevents editor chrome from being
    /// restored inside a click-through fullscreen window.
    pub fn set_canvas_passive_enabled(
        &mut self,
    ) -> Result<OverlayLayoutSettings, OverlayLayoutUpdateError> {
        let already_passive = self.settings.canvas_enabled
            && self
                .settings
                .setups
                .get(&self.settings.selected_setup_id)
                .is_some_and(|setup| setup.locked);
        if already_passive {
            return Ok(self.snapshot());
        }
        let mut settings = self.snapshot();
        settings.canvas_enabled = true;
        let selected = settings.selected_setup_id.clone();
        let setup = settings.setups.get_mut(&selected).ok_or_else(|| {
            OverlayLayoutUpdateError::Validation("selected overlay setup is missing".into())
        })?;
        setup.locked = true;
        self.update(settings)
    }
}

fn normalize_and_validate(settings: &mut OverlayLayoutSettings) -> Result<(), String> {
    if settings.schema_version != SCHEMA_VERSION {
        return Err("unsupported overlay layout schema".into());
    }
    if settings.setups.is_empty() || settings.setups.len() > MAX_SETUPS {
        return Err("overlay layouts require between 1 and 16 setups".into());
    }
    if !settings.setups.contains_key(&settings.selected_setup_id) {
        return Err("selected overlay setup is missing".into());
    }
    for (id, setup) in &mut settings.setups {
        if id.is_empty() || id.len() > 64 || setup.name.trim().is_empty() || setup.name.len() > 96 {
            return Err("overlay setup identity is invalid".into());
        }
        let ids = setup
            .modules
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if ids != MODULE_IDS.into_iter().collect() {
            return Err("overlay setup must contain exactly the seven supported modules".into());
        }
        for value in setup.modules.values_mut() {
            if ![
                value.x,
                value.y,
                value.width,
                value.height,
                value.opacity,
                value.scale,
            ]
            .into_iter()
            .all(f64::is_finite)
            {
                return Err("overlay module geometry must be finite".into());
            }
            value.opacity = value.opacity.clamp(0.2, 1.0);
            value.scale = value.scale.clamp(0.5, 2.0);
            value.width = value.width.clamp(0.08, 1.0 / value.scale);
            value.height = value.height.clamp(0.06, 1.0 / value.scale);
            value.x = value.x.clamp(0.0, 1.0 - value.width * value.scale);
            value.y = value.y.clamp(0.0, 1.0 - value.height * value.scale);
            value.z_order = value.z_order.min(1000);
        }
    }
    Ok(())
}

fn load(path: &Path) -> Result<OverlayLayoutSettings, String> {
    recover_interrupted_write(path)?;
    match std::fs::metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(OverlayLayoutSettings::default());
        }
        Err(error) => {
            return Err(format!(
                "could not inspect overlay layout settings: {error}"
            ));
        }
    }
    match read_valid(path) {
        Ok(settings) => Ok(settings),
        Err(live_error) => {
            let backup = sibling_path(path, "backup");
            let settings = read_valid(&backup).map_err(|backup_error| {
                format!("{live_error}; backup recovery failed: {backup_error}")
            })?;
            std::fs::remove_file(path)
                .map_err(|error| format!("could not remove invalid overlay layout: {error}"))?;
            durable_rename(&backup, path).map_err(|error| {
                format!("could not restore valid overlay layout backup: {error}")
            })?;
            Ok(settings)
        }
    }
}

fn read_valid(path: &Path) -> Result<OverlayLayoutSettings, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("could not inspect overlay layout settings: {error}"))?;
    if metadata.len() > MAX_SETTINGS_BYTES {
        return Err("overlay layout settings exceed 128 KiB".into());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read overlay layout settings: {error}"))?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err("overlay layout settings exceed 128 KiB".into());
    }
    let mut value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("overlay layout settings are invalid: {error}"))?;
    if value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        == Some(1)
    {
        let object = value
            .as_object_mut()
            .ok_or_else(|| "overlay layout settings must be an object".to_owned())?;
        object.insert(
            "schemaVersion".into(),
            serde_json::Value::from(SCHEMA_VERSION),
        );
        object.insert("canvasEnabled".into(), serde_json::Value::Bool(false));
    }
    let mut settings: OverlayLayoutSettings = serde_json::from_value(value)
        .map_err(|error| format!("overlay layout settings are invalid: {error}"))?;
    normalize_and_validate(&mut settings)?;
    Ok(settings)
}

fn write(path: &Path, settings: &OverlayLayoutSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "overlay layout settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create overlay layout settings folder: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("could not encode overlay layout settings: {error}"))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err("overlay layout settings exceed 128 KiB".into());
    }
    let temporary = sibling_path(path, "tmp");
    let backup = sibling_path(path, "backup");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("could not create temporary overlay layout: {error}"))?;
    use std::io::Write as _;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not flush temporary overlay layout: {error}"))?;
    drop(file);
    if path.exists() {
        if backup.exists() {
            std::fs::remove_file(&backup).map_err(|error| {
                format!("could not remove stale overlay layout backup: {error}")
            })?;
        }
        durable_rename(path, &backup)
            .map_err(|error| format!("could not stage overlay layout backup: {error}"))?;
    }
    if let Err(error) = durable_rename(&temporary, path) {
        if backup.exists() {
            let _ = durable_rename(&backup, path);
        }
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("could not replace overlay layout: {error}"));
    }
    Ok(())
}

fn recover_interrupted_write(path: &Path) -> Result<(), String> {
    let temporary = sibling_path(path, "tmp");
    let backup = sibling_path(path, "backup");
    if !path.exists() && backup.exists() {
        durable_rename(&backup, path)
            .map_err(|error| format!("could not recover overlay layout backup: {error}"))?;
    }
    if temporary.exists() {
        std::fs::remove_file(&temporary).map_err(|error| {
            format!("could not remove stale overlay layout temporary file: {error}")
        })?;
    }
    Ok(())
}

#[cfg(windows)]
fn durable_rename(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let from = from
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let to = to
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: Both paths are NUL-terminated and remain valid for the duration
    // of the synchronous Win32 call.
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn durable_rename(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    std::fs::rename(from, to)?;
    let parent = to
        .parent()
        .ok_or_else(|| std::io::Error::other("overlay layout settings path has no parent"))?;
    std::fs::File::open(parent).and_then(|directory| directory.sync_all())
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{suffix}"));
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_geometry_and_visuals_at_the_authority_boundary() {
        let mut settings = OverlayLayoutSettings::default();
        let map = settings
            .setups
            .get_mut("default")
            .unwrap()
            .modules
            .get_mut("map")
            .unwrap();
        map.x = -4.0;
        map.y = 8.0;
        map.width = 9.0;
        map.height = 0.001;
        map.opacity = 0.01;
        map.scale = 8.0;
        map.z_order = u16::MAX;
        normalize_and_validate(&mut settings).unwrap();
        let map = &settings.setups["default"].modules["map"];
        assert_eq!(
            (
                map.x,
                map.y,
                map.width,
                map.height,
                map.opacity,
                map.scale,
                map.z_order
            ),
            (0.0, 0.88, 0.5, 0.06, 0.2, 2.0, 1000)
        );
    }

    #[test]
    fn rejects_missing_modules_and_reset_is_recovery_safe() {
        let mut settings = OverlayLayoutSettings::default();
        settings
            .setups
            .get_mut("default")
            .unwrap()
            .modules
            .remove("alerts");
        assert!(normalize_and_validate(&mut settings).is_err());
        let reset = OverlayLayoutSettings::default();
        assert_eq!(reset.setups["default"].modules.len(), 7);
        assert!(!reset.setups["default"].locked);
        assert!(!reset.canvas_enabled);
    }

    #[test]
    fn schema_one_migrates_to_disabled_canvas_without_losing_layout() {
        let path = std::env::temp_dir().join(format!(
            "rlogs-overlay-layout-v1-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut legacy = serde_json::to_value(OverlayLayoutSettings::default()).unwrap();
        let object = legacy.as_object_mut().unwrap();
        object.insert("schemaVersion".into(), serde_json::Value::from(1));
        object.remove("canvasEnabled");
        object["setups"]["default"]["locked"] = serde_json::Value::Bool(true);
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();

        let migrated = OverlayLayoutSettingsStore::open(&path).unwrap().snapshot();
        assert_eq!(migrated.schema_version, 2);
        assert!(!migrated.canvas_enabled);
        assert!(migrated.setups["default"].locked);
        assert_eq!(migrated.setups["default"].modules.len(), 7);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn store_increments_revision_and_persists_safe_reset() {
        let path = std::env::temp_dir().join(format!(
            "rlogs-overlay-layout-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = OverlayLayoutSettingsStore::open(&path).unwrap();
        let mut changed = store.snapshot();
        let stale = changed.clone();
        changed.canvas_enabled = true;
        changed.setups.get_mut("default").unwrap().locked = true;
        assert_eq!(store.update(changed).unwrap().revision, 1);
        assert!(store.update(stale).is_err());
        let reset_request = OverlayLayoutSettings {
            revision: 1,
            legacy_migration_complete: true,
            ..OverlayLayoutSettings::default()
        };
        let reset = store.update(reset_request).unwrap();
        assert_eq!(reset.revision, 2);
        assert!(reset.legacy_migration_complete);
        assert!(!reset.canvas_enabled);
        assert!(!reset.setups["default"].locked);
        let reopened = OverlayLayoutSettingsStore::open(&path).unwrap();
        assert_eq!(reopened.snapshot(), reset);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(sibling_path(&path, "backup"));
    }
    #[test]
    fn canvas_enablement_round_trips_and_no_op_preserves_revision() {
        let path = std::env::temp_dir().join(format!(
            "rlogs-overlay-visibility-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = OverlayLayoutSettingsStore::open(&path).unwrap();
        let enabled = store.set_canvas_enabled(true).unwrap();
        assert!(enabled.canvas_enabled);
        assert_eq!(enabled.revision, 1);
        assert_eq!(store.set_canvas_enabled(true).unwrap().revision, 1);
        drop(store);
        let mut reopened = OverlayLayoutSettingsStore::open(&path).unwrap();
        assert!(reopened.snapshot().canvas_enabled);
        assert!(!reopened.set_canvas_enabled(false).unwrap().canvas_enabled);
        drop(reopened);
        assert!(
            !OverlayLayoutSettingsStore::open(&path)
                .unwrap()
                .snapshot()
                .canvas_enabled
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(sibling_path(&path, "backup"));
    }

    #[test]
    fn passive_enablement_atomically_locks_the_selected_setup() {
        let path = std::env::temp_dir().join(format!(
            "rlogs-overlay-passive-visibility-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = OverlayLayoutSettingsStore::open(&path).unwrap();
        let selected_id = store.snapshot().selected_setup_id;
        let mut edited = store.snapshot();
        let map = edited.setups[&selected_id].modules["map"].clone();
        edited.setups.get_mut(&selected_id).unwrap().locked = false;
        edited
            .setups
            .get_mut(&selected_id)
            .unwrap()
            .modules
            .get_mut("map")
            .unwrap()
            .opacity = 0.43;
        edited
            .setups
            .get_mut(&selected_id)
            .unwrap()
            .modules
            .get_mut("map")
            .unwrap()
            .x = 0.17;
        let edited = store.update(edited).unwrap();
        let edited_map = edited.setups[&selected_id].modules["map"].clone();
        assert_ne!(edited_map, map);
        let enabled = store.set_canvas_passive_enabled().unwrap();
        assert!(enabled.canvas_enabled);
        assert!(enabled.setups[&enabled.selected_setup_id].locked);
        assert_eq!(enabled.setups[&selected_id].modules["map"], edited_map);
        assert_eq!(enabled.revision, 2);
        assert_eq!(store.set_canvas_passive_enabled().unwrap().revision, 2);
        let hidden = store.set_canvas_enabled(false).unwrap();
        assert!(!hidden.canvas_enabled);
        assert_eq!(hidden.setups[&selected_id].modules["map"], edited_map);
        let restored = store.set_canvas_passive_enabled().unwrap();
        assert!(restored.canvas_enabled);
        assert!(restored.setups[&selected_id].locked);
        assert_eq!(restored.setups[&selected_id].modules["map"], edited_map);
        drop(store);

        let reopened = OverlayLayoutSettingsStore::open(&path).unwrap().snapshot();
        assert!(reopened.canvas_enabled);
        assert!(reopened.setups[&reopened.selected_setup_id].locked);
        assert_eq!(reopened.setups[&selected_id].modules["map"], edited_map);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(sibling_path(&path, "backup"));
    }

    #[test]
    fn invalid_or_oversized_live_file_recovers_only_from_a_bounded_valid_backup() {
        let path = std::env::temp_dir().join(format!(
            "rlogs-overlay-recovery-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = OverlayLayoutSettingsStore::open(&path).unwrap();
        let mut first = store.snapshot();
        first.legacy_migration_complete = true;
        first.canvas_enabled = true;
        store.update(first).unwrap();
        let mut second = store.snapshot();
        second.canvas_enabled = false;
        second.setups.get_mut("default").unwrap().locked = true;
        store.update(second).unwrap();
        assert!(sibling_path(&path, "backup").exists());
        std::fs::write(&path, b"not json").unwrap();
        let recovered = OverlayLayoutSettingsStore::open(&path).unwrap().snapshot();
        assert!(!recovered.setups["default"].locked);
        assert!(recovered.canvas_enabled);

        let mut store = OverlayLayoutSettingsStore::open(&path).unwrap();
        let mut next = store.snapshot();
        next.canvas_enabled = false;
        next.setups.get_mut("default").unwrap().locked = true;
        store.update(next).unwrap();
        std::fs::write(&path, vec![b'x'; MAX_SETTINGS_BYTES as usize + 1]).unwrap();
        let recovered = OverlayLayoutSettingsStore::open(&path).unwrap().snapshot();
        assert!(!recovered.setups["default"].locked);
        assert!(recovered.canvas_enabled);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(sibling_path(&path, "backup"));
    }
}
