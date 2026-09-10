use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const MAX_PRESETS: usize = 128;
const MAX_STORE_BYTES: u64 = 512 * 1024;
const MAX_NAME_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomarkerPoint {
    pub marker_number: u8,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomarkerPreset {
    pub preset_id: String,
    pub name: String,
    pub client_build: String,
    pub scene_id: i32,
    pub map_id: u32,
    pub saved_at_unix_millis: u64,
    pub points: Vec<AutomarkerPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutomarkerPresetFile {
    schema_version: u16,
    presets: Vec<AutomarkerPreset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomarkerSceneContext {
    pub client_build: String,
    pub scene_id: i32,
    pub map_id: u32,
    pub scene_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomarkerPresetView {
    pub schema_version: u16,
    pub context: Option<AutomarkerSceneContext>,
    pub presets: Vec<AutomarkerPreset>,
    pub capture_supported: bool,
    pub capture_reason: &'static str,
    pub native_load_supported: bool,
    pub native_load_reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveAutomarkerPresetRequest {
    pub preset_id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoadAutomarkerPresetRequest {
    pub preset_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomarkerLoadResult {
    pub supported: bool,
    pub reason: &'static str,
}

#[derive(Debug)]
pub struct AutomarkerPresetStore {
    // Retained while capture-current is capability-gated; it becomes active
    // without a data migration when native waymark observation is proven.
    #[allow(dead_code)]
    path: PathBuf,
    presets: Vec<AutomarkerPreset>,
}

impl AutomarkerPresetStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let presets = load(&path)?;
        Ok(Self { path, presets })
    }

    pub fn compatible(&self, context: AutomarkerSceneContext) -> AutomarkerPresetView {
        let presets = self
            .presets
            .iter()
            .filter(|preset| compatible(preset, &context))
            .cloned()
            .collect();
        view(Some(context), presets)
    }

    pub fn unavailable(&self) -> AutomarkerPresetView {
        view(None, Vec::new())
    }

    #[allow(dead_code)]
    pub fn save(
        &mut self,
        request: SaveAutomarkerPresetRequest,
        context: AutomarkerSceneContext,
        points: Vec<AutomarkerPoint>,
        now_unix_millis: u64,
    ) -> Result<AutomarkerPresetView, String> {
        validate_name(&request.name)?;
        validate_points(&points)?;
        let preset_id = match request.preset_id {
            Some(preset_id) => {
                validate_id(&preset_id)?;
                let existing = self
                    .presets
                    .iter()
                    .find(|preset| preset.preset_id == preset_id)
                    .ok_or_else(|| "the selected automarker preset no longer exists".to_owned())?;
                if !compatible(existing, &context) {
                    return Err(
                        "the selected automarker preset belongs to a different scene or map".into(),
                    );
                }
                preset_id
            }
            None => self.next_id(now_unix_millis),
        };
        let preset = AutomarkerPreset {
            preset_id: preset_id.clone(),
            name: request.name.trim().to_owned(),
            client_build: context.client_build.clone(),
            scene_id: context.scene_id,
            map_id: context.map_id,
            saved_at_unix_millis: now_unix_millis,
            points,
        };
        let mut next = self.presets.clone();
        if let Some(index) = next
            .iter()
            .position(|candidate| candidate.preset_id == preset_id)
        {
            next[index] = preset;
        } else {
            if next.len() >= MAX_PRESETS {
                return Err(format!("automarker preset limit ({MAX_PRESETS}) reached"));
            }
            next.push(preset);
        }
        next.sort_by_key(|preset| std::cmp::Reverse(preset.saved_at_unix_millis));
        write(&self.path, &next)?;
        self.presets = next;
        Ok(self.compatible(context))
    }

    pub fn prepare_load(
        &self,
        request: LoadAutomarkerPresetRequest,
        context: AutomarkerSceneContext,
    ) -> Result<AutomarkerLoadResult, String> {
        validate_id(&request.preset_id)?;
        let preset = self
            .presets
            .iter()
            .find(|preset| preset.preset_id == request.preset_id)
            .ok_or_else(|| "the selected automarker preset no longer exists".to_owned())?;
        if !compatible(preset, &context) {
            return Err(
                "the selected automarker preset belongs to a different scene or map".into(),
            );
        }
        Ok(AutomarkerLoadResult {
            supported: false,
            reason: "native_waymark_request_unverified",
        })
    }

    #[allow(dead_code)]
    fn next_id(&self, now_unix_millis: u64) -> String {
        for suffix in 0_u16..=u16::MAX {
            let candidate = format!("preset-{now_unix_millis:016x}-{suffix:04x}");
            if !self
                .presets
                .iter()
                .any(|preset| preset.preset_id == candidate)
            {
                return candidate;
            }
        }
        unreachable!("preset capacity is bounded below the identifier suffix space")
    }
}

fn view(
    context: Option<AutomarkerSceneContext>,
    presets: Vec<AutomarkerPreset>,
) -> AutomarkerPresetView {
    AutomarkerPresetView {
        schema_version: SCHEMA_VERSION,
        context,
        presets,
        capture_supported: false,
        capture_reason: "native_waymark_state_unverified",
        native_load_supported: false,
        native_load_reason: "native_waymark_request_unverified",
    }
}

fn compatible(preset: &AutomarkerPreset, context: &AutomarkerSceneContext) -> bool {
    // World coordinates belong to the scene/map. Keep the captured build as
    // provenance, but do not hide a saved layout merely because the client
    // received a patch while the scene and map identity stayed the same.
    preset.scene_id == context.scene_id && preset.map_id == context.map_id
}

fn validate_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "preset names must contain 1–{MAX_NAME_CHARS} characters"
        ));
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.len() < 8
        || id.len() > 80
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("invalid automarker preset identifier".into());
    }
    Ok(())
}

fn validate_points(points: &[AutomarkerPoint]) -> Result<(), String> {
    if points.is_empty() || points.len() > 6 {
        return Err("a preset must contain 1–6 numbered markers".into());
    }
    let mut numbers = [false; 6];
    for point in points {
        if !(1..=6).contains(&point.marker_number)
            || !point.x.is_finite()
            || !point.y.is_finite()
            || !point.z.is_finite()
            || [point.x, point.y, point.z]
                .iter()
                .any(|value| value.abs() > 1_000_000.0)
        {
            return Err("preset marker coordinates are invalid".into());
        }
        let seen = &mut numbers[usize::from(point.marker_number - 1)];
        if *seen {
            return Err("preset marker numbers must be unique".into());
        }
        *seen = true;
    }
    Ok(())
}

fn load(path: &Path) -> Result<Vec<AutomarkerPreset>, String> {
    recover_interrupted_write(path)?;
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("could not inspect automarker preset file: {error}")),
    };
    if metadata.len() > MAX_STORE_BYTES {
        return Err("automarker preset file exceeds the 512 KiB safety limit".into());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read automarker preset file: {error}"))?;
    let file: AutomarkerPresetFile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("automarker preset file is invalid: {error}"))?;
    if file.schema_version != SCHEMA_VERSION || file.presets.len() > MAX_PRESETS {
        return Err("automarker preset file has an unsupported schema or size".into());
    }
    let mut identifiers = std::collections::BTreeSet::new();
    for preset in &file.presets {
        validate_id(&preset.preset_id)?;
        if !identifiers.insert(&preset.preset_id) {
            return Err("automarker preset identifiers must be unique".into());
        }
        validate_name(&preset.name)?;
        validate_points(&preset.points)?;
    }
    Ok(file.presets)
}

#[allow(dead_code)]
fn write(path: &Path, presets: &[AutomarkerPreset]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "automarker preset path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create automarker preset folder: {error}"))?;
    let file = AutomarkerPresetFile {
        schema_version: SCHEMA_VERSION,
        presets: presets.to_vec(),
    };
    let mut bytes = serde_json::to_vec_pretty(&file)
        .map_err(|error| format!("could not encode automarker presets: {error}"))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err("automarker preset file exceeds the 512 KiB safety limit".into());
    }
    let temporary = sibling_path(path, "tmp");
    let backup = sibling_path(path, "backup");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("could not create temporary automarker preset file: {error}"))?;
    use std::io::Write as _;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not flush temporary automarker preset file: {error}"))?;
    drop(file);

    if path.exists() {
        if backup.exists() {
            std::fs::remove_file(&backup).map_err(|error| {
                format!("could not remove stale automarker preset backup: {error}")
            })?;
        }
        std::fs::rename(path, &backup).map_err(|error| {
            format!("could not stage the previous automarker preset file: {error}")
        })?;
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, path);
        }
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("could not replace automarker preset file: {error}"));
    }
    if backup.exists() {
        std::fs::remove_file(&backup)
            .map_err(|error| format!("could not remove automarker preset backup: {error}"))?;
    }
    Ok(())
}

fn recover_interrupted_write(path: &Path) -> Result<(), String> {
    let backup = sibling_path(path, "backup");
    if !path.exists() && backup.exists() {
        std::fs::rename(&backup, path)
            .map_err(|error| format!("could not recover automarker preset backup: {error}"))?;
    } else if path.exists() && backup.exists() {
        std::fs::remove_file(&backup)
            .map_err(|error| format!("could not remove stale automarker preset backup: {error}"))?;
    }
    Ok(())
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{suffix}"));
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rlogs-automarker-{name}-{}-{}.json",
            std::process::id(),
            crate::unix_millis()
        ))
    }

    fn context(scene_id: i32) -> AutomarkerSceneContext {
        AutomarkerSceneContext {
            client_build: "24687926".into(),
            scene_id,
            map_id: scene_id as u32,
            scene_name: Some(format!("Scene {scene_id}")),
        }
    }

    fn points(x: f32) -> Vec<AutomarkerPoint> {
        vec![AutomarkerPoint {
            marker_number: 1,
            x,
            y: 2.0,
            z: 3.0,
        }]
    }

    #[test]
    fn supports_multiple_presets_and_overwrites_only_by_stable_id() {
        let path = temporary_path("multiple");
        let mut store = AutomarkerPresetStore::open(&path).unwrap();
        let first = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Opener".into(),
                },
                context(1100),
                points(1.0),
                10,
            )
            .unwrap();
        let first_id = first.presets[0].preset_id.clone();
        let second = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Opener".into(),
                },
                context(1100),
                points(4.0),
                11,
            )
            .unwrap();
        assert_eq!(second.presets.len(), 2);
        let updated = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: Some(first_id.clone()),
                    name: "Adjusted".into(),
                },
                context(1100),
                points(7.0),
                12,
            )
            .unwrap();
        assert_eq!(updated.presets.len(), 2);
        assert_eq!(
            updated
                .presets
                .iter()
                .find(|preset| preset.preset_id == first_id)
                .unwrap()
                .points[0]
                .x,
            7.0
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn filters_by_scene_and_map_and_rejects_cross_scene_load() {
        let path = temporary_path("scope");
        let mut store = AutomarkerPresetStore::open(&path).unwrap();
        let saved = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "M1".into(),
                },
                context(1100),
                points(1.0),
                10,
            )
            .unwrap();
        let preset_id = saved.presets[0].preset_id.clone();
        assert!(store.compatible(context(1200)).presets.is_empty());
        assert!(
            store
                .prepare_load(LoadAutomarkerPresetRequest { preset_id }, context(1200))
                .is_err()
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn keeps_scene_presets_visible_across_build_updates() {
        let path = temporary_path("build-provenance");
        let mut store = AutomarkerPresetStore::open(&path).unwrap();
        store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "M1".into(),
                },
                context(1100),
                points(1.0),
                10,
            )
            .unwrap();
        let mut patched = context(1100);
        patched.client_build = "24699999".into();
        let view = store.compatible(patched.clone());
        assert_eq!(view.presets.len(), 1);
        assert_eq!(view.presets[0].client_build, "24687926");
        assert!(!view.capture_supported);
        assert!(!view.native_load_supported);
        assert!(
            store
                .prepare_load(
                    LoadAutomarkerPresetRequest {
                        preset_id: view.presets[0].preset_id.clone()
                    },
                    patched
                )
                .is_ok()
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn recovers_an_atomic_backup_after_an_interrupted_replace() {
        let path = temporary_path("atomic-recovery");
        let mut store = AutomarkerPresetStore::open(&path).unwrap();
        store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Safe".into(),
                },
                context(1100),
                points(1.0),
                10,
            )
            .unwrap();
        drop(store);
        let backup = sibling_path(&path, "backup");
        std::fs::rename(&path, &backup).unwrap();
        let recovered = AutomarkerPresetStore::open(&path).unwrap();
        assert_eq!(recovered.compatible(context(1100)).presets[0].name, "Safe");
        assert!(path.is_file());
        assert!(!backup.exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn persists_exact_xyz_and_keeps_native_loading_locked() {
        let path = temporary_path("persist");
        let mut store = AutomarkerPresetStore::open(&path).unwrap();
        let saved = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Exact".into(),
                },
                context(1100),
                vec![AutomarkerPoint {
                    marker_number: 6,
                    x: -1.25,
                    y: 9.5,
                    z: 44.125,
                }],
                10,
            )
            .unwrap();
        let preset_id = saved.presets[0].preset_id.clone();
        drop(store);
        let reopened = AutomarkerPresetStore::open(&path).unwrap();
        assert_eq!(
            reopened.compatible(context(1100)).presets[0].points[0].z,
            44.125
        );
        assert_eq!(
            reopened
                .prepare_load(LoadAutomarkerPresetRequest { preset_id }, context(1100))
                .unwrap(),
            AutomarkerLoadResult {
                supported: false,
                reason: "native_waymark_request_unverified"
            }
        );
        let _ = std::fs::remove_file(path);
    }
}
