use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 4;
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
    pub activity_family_id: String,
    pub saved_at_unix_millis: u64,
    pub points: Vec<AutomarkerPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutomarkerPresetFile {
    schema_version: u16,
    presets: Vec<AutomarkerPreset>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyAutomarkerPresetFileV1 {
    schema_version: u16,
    presets: Vec<LegacyAutomarkerPresetV1>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct LegacyAutomarkerPresetV1 {
    preset_id: String,
    name: String,
    client_build: String,
    scene_id: i32,
    map_id: u32,
    saved_at_unix_millis: u64,
    points: Vec<AutomarkerPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyAutomarkerPresetFileV2 {
    schema_version: u16,
    presets: Vec<LegacyAutomarkerPresetV2>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
struct LegacyAutomarkerPresetV2 {
    preset_id: String,
    name: String,
    client_build: String,
    scene_id: i32,
    map_id: u32,
    activity_family_id: String,
    saved_at_unix_millis: u64,
    points: Vec<AutomarkerPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyAutomarkerPresetFileV3 {
    schema_version: u16,
    presets: Vec<LegacyAutomarkerPresetV2>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomarkerSceneContext {
    pub client_build: String,
    pub scene_id: i32,
    pub map_id: u32,
    pub activity_family_id: String,
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
    pub capture_session_id: Option<String>,
    pub deployment_id: Option<String>,
    pub protocol_pack_digest: Option<String>,
    pub native_load_supported: bool,
    pub native_load_reason: &'static str,
    pub preview_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveAutomarkerPresetRequest {
    pub preset_id: Option<String>,
    pub name: String,
    pub points: Vec<AutomarkerPoint>,
    pub expected_context: AutomarkerSceneContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoadAutomarkerPresetRequest {
    pub preset_id: String,
    pub expected_context: AutomarkerSceneContext,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomarkerLocalLoadResult {
    pub context: AutomarkerSceneContext,
    pub preset: AutomarkerPreset,
}

#[derive(Debug)]
pub struct AutomarkerPresetStore {
    // Retained while capture-current is capability-gated; older files at this
    // path are migrated through the reviewed automarker-family catalog.
    #[allow(dead_code)]
    path: PathBuf,
    presets: Vec<AutomarkerPreset>,
}

impl AutomarkerPresetStore {
    pub fn open(
        path: impl Into<PathBuf>,
        scene_families: &BTreeMap<i32, String>,
    ) -> Result<Self, String> {
        let path = path.into();
        let (presets, migrated) = load(&path, scene_families)?;
        if migrated {
            write(&path, &presets)?;
        }
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

    pub fn save(
        &mut self,
        request: SaveAutomarkerPresetRequest,
        context: AutomarkerSceneContext,
        now_unix_millis: u64,
    ) -> Result<AutomarkerPresetView, String> {
        if request.expected_context.client_build != context.client_build
            || request.expected_context.scene_id != context.scene_id
            || request.expected_context.map_id != context.map_id
            || request.expected_context.activity_family_id != context.activity_family_id
        {
            return Err("the live automarker context changed after the editor loaded; refresh the scene before saving".into());
        }
        validate_name(&request.name)?;
        validate_points(&request.points)?;
        let points = request.points;
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
                        "the selected automarker preset belongs to a different dungeon family"
                            .into(),
                    );
                }
                preset_id
            }
            None => self.next_id(now_unix_millis),
        };
        let preset = AutomarkerPreset {
            preset_id: preset_id.clone(),
            name: request.name.trim().to_owned(),
            activity_family_id: context.activity_family_id.clone(),
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

    pub fn load(
        &self,
        request: LoadAutomarkerPresetRequest,
        context: AutomarkerSceneContext,
    ) -> Result<AutomarkerLocalLoadResult, String> {
        if request.expected_context.client_build != context.client_build
            || request.expected_context.scene_id != context.scene_id
            || request.expected_context.map_id != context.map_id
            || request.expected_context.activity_family_id != context.activity_family_id
        {
            return Err("the live automarker context changed after the editor loaded; refresh the scene before loading".into());
        }
        validate_id(&request.preset_id)?;
        let preset = self
            .presets
            .iter()
            .find(|preset| preset.preset_id == request.preset_id)
            .ok_or_else(|| "the selected automarker preset no longer exists".to_owned())?;
        if !compatible(preset, &context) {
            return Err(
                "the selected automarker preset belongs to a different dungeon family".into(),
            );
        }
        Ok(AutomarkerLocalLoadResult {
            context,
            preset: preset.clone(),
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
        capture_session_id: None,
        deployment_id: None,
        protocol_pack_digest: None,
        native_load_supported: false,
        // The current-build request topology is capture-proven. Placement
        // remains disabled until rLogs has a transport that can ask the game
        // to allocate its own live counters and authenticated request fields.
        native_load_reason: "native_waymark_transport_unavailable",
        preview_session_id: String::new(),
    }
}

fn compatible(preset: &AutomarkerPreset, context: &AutomarkerSceneContext) -> bool {
    // The reviewed run-rule catalog is the authority for dungeon-family
    // identity. Scene/map/build remain capture provenance; numeric adjacency
    // and display-name similarity are never used to infer compatibility.
    preset.activity_family_id == context.activity_family_id
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

fn load(
    path: &Path,
    scene_families: &BTreeMap<i32, String>,
) -> Result<(Vec<AutomarkerPreset>, bool), String> {
    recover_interrupted_write(path)?;
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), false));
        }
        Err(error) => return Err(format!("could not inspect automarker preset file: {error}")),
    };
    if metadata.len() > MAX_STORE_BYTES {
        return Err("automarker preset file exceeds the 512 KiB safety limit".into());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read automarker preset file: {error}"))?;
    let schema_version = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| value.get("schemaVersion")?.as_u64())
        .ok_or_else(|| "automarker preset file has no valid schema version".to_owned())?;
    let (presets, migrated) = match schema_version {
        1 => {
            let file: LegacyAutomarkerPresetFileV1 = serde_json::from_slice(&bytes)
                .map_err(|error| format!("legacy automarker preset file is invalid: {error}"))?;
            if file.schema_version != 1 || file.presets.len() > MAX_PRESETS {
                return Err(
                    "legacy automarker preset file has an unsupported schema or size".into(),
                );
            }
            let presets = file.presets.into_iter().map(|preset| {
                let activity_family_id = scene_families.get(&preset.scene_id).cloned().ok_or_else(|| {
                    format!("legacy automarker preset {} uses scene {} without a reviewed dungeon-family identity", preset.preset_id, preset.scene_id)
                })?;
                Ok(AutomarkerPreset {
                    preset_id: preset.preset_id,
                    name: preset.name,
                    activity_family_id,
                    saved_at_unix_millis: preset.saved_at_unix_millis,
                    points: preset.points,
                })
            }).collect::<Result<Vec<_>, String>>()?;
            (presets, true)
        }
        2 => {
            let file: LegacyAutomarkerPresetFileV2 =
                serde_json::from_slice(&bytes).map_err(|error| {
                    format!("schema-two automarker preset file is invalid: {error}")
                })?;
            if file.schema_version != 2 || file.presets.len() > MAX_PRESETS {
                return Err(
                    "schema-two automarker preset file has an unsupported schema or size".into(),
                );
            }
            let presets = file
                .presets
                .into_iter()
                .map(|mut preset| {
                    preset.activity_family_id = scene_families
                        .get(&preset.scene_id)
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "schema-two automarker preset {} uses scene {} without a reviewed automarker-family identity",
                                preset.preset_id, preset.scene_id
                            )
                        })?;
                    Ok(AutomarkerPreset {
                        preset_id: preset.preset_id,
                        name: preset.name,
                        activity_family_id: preset.activity_family_id,
                        saved_at_unix_millis: preset.saved_at_unix_millis,
                        points: preset.points,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            (presets, true)
        }
        3 => {
            let file: LegacyAutomarkerPresetFileV3 =
                serde_json::from_slice(&bytes).map_err(|error| {
                    format!("schema-three automarker preset file is invalid: {error}")
                })?;
            if file.schema_version != 3 || file.presets.len() > MAX_PRESETS {
                return Err(
                    "schema-three automarker preset file has an unsupported schema or size".into(),
                );
            }
            let presets = file.presets.into_iter().map(|preset| {
                let expected_family_id = scene_families.get(&preset.scene_id).ok_or_else(|| {
                    format!(
                        "schema-three automarker preset {} uses scene {} without a reviewed automarker-family identity",
                        preset.preset_id, preset.scene_id
                    )
                })?;
                if &preset.activity_family_id != expected_family_id {
                    return Err(format!(
                        "automarker preset {} does not match the reviewed automarker-family identity for scene {}",
                        preset.preset_id, preset.scene_id
                    ));
                }
                Ok(AutomarkerPreset {
                    preset_id: preset.preset_id,
                    name: preset.name,
                    activity_family_id: preset.activity_family_id,
                    saved_at_unix_millis: preset.saved_at_unix_millis,
                    points: preset.points,
                })
            }).collect::<Result<Vec<_>, String>>()?;
            (presets, true)
        }
        4 => {
            let file: AutomarkerPresetFile = serde_json::from_slice(&bytes)
                .map_err(|error| format!("automarker preset file is invalid: {error}"))?;
            if file.schema_version != SCHEMA_VERSION || file.presets.len() > MAX_PRESETS {
                return Err("automarker preset file has an unsupported schema or size".into());
            }
            (file.presets, false)
        }
        _ => return Err("automarker preset file has an unsupported schema or size".into()),
    };
    let mut identifiers = std::collections::BTreeSet::new();
    for preset in &presets {
        validate_id(&preset.preset_id)?;
        if !identifiers.insert(&preset.preset_id) {
            return Err("automarker preset identifiers must be unique".into());
        }
        validate_name(&preset.name)?;
        if preset.activity_family_id.trim().is_empty() || preset.activity_family_id.len() > 128 {
            return Err("automarker preset dungeon-family identity is invalid".into());
        }
        validate_points(&preset.points)?;
    }
    Ok((presets, migrated))
}

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
    // The live rename above is the commit point. Backup cleanup is recovery
    // hygiene only; reporting it as a save failure after commit would leave
    // the caller's memory and the successfully replaced file divergent.
    finish_committed_write(&backup)
}

fn finish_committed_write(backup: &Path) -> Result<(), String> {
    if backup.exists() {
        let _ = std::fs::remove_file(backup);
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

    fn families() -> BTreeMap<i32, String> {
        [
            (1_621, "tina-mindrealm"),
            (1_631, "tina-mindrealm"),
            (1_632, "tina-mindrealm"),
            (1_633, "dungeon.1633"),
            (1_100, "mech-facility"),
        ]
        .into_iter()
        .map(|(scene, family)| (scene, family.to_owned()))
        .collect()
    }

    fn open(path: &Path) -> AutomarkerPresetStore {
        AutomarkerPresetStore::open(path, &families()).unwrap()
    }

    fn context(scene_id: i32, map_id: u32, activity_family_id: &str) -> AutomarkerSceneContext {
        AutomarkerSceneContext {
            client_build: "24687926".into(),
            scene_id,
            map_id,
            activity_family_id: activity_family_id.into(),
            scene_name: Some(format!("Scene {scene_id}")),
        }
    }

    fn tina(scene_id: i32) -> AutomarkerSceneContext {
        context(
            scene_id,
            scene_id as u32,
            if scene_id == 1_633 {
                "dungeon.1633"
            } else {
                "tina-mindrealm"
            },
        )
    }

    fn mech() -> AutomarkerSceneContext {
        context(1_100, 1_100, "mech-facility")
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
        let mut store = open(&path);
        let first = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Opener".into(),
                    points: points(1.0),
                    expected_context: mech(),
                },
                mech(),
                10,
            )
            .unwrap();
        let first_id = first.presets[0].preset_id.clone();
        let second = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Alternate".into(),
                    points: points(4.0),
                    expected_context: mech(),
                },
                mech(),
                10,
            )
            .unwrap();
        assert_eq!(second.presets.len(), 2);
        assert_ne!(second.presets[0].preset_id, second.presets[1].preset_id);
        assert!(
            second
                .presets
                .iter()
                .any(|preset| preset.name == "Alternate")
        );
        let updated = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: Some(first_id.clone()),
                    name: "Adjusted".into(),
                    points: points(7.0),
                    expected_context: mech(),
                },
                mech(),
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
        assert_eq!(
            updated
                .presets
                .iter()
                .find(|preset| preset.name == "Alternate")
                .unwrap()
                .points[0]
                .x,
            4.0
        );
        drop(store);
        let reopened = open(&path);
        assert_eq!(reopened.compatible(mech()).presets.len(), 2);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn persisted_presets_contain_no_user_or_live_session_identity() {
        let path = temporary_path("portable");
        let mut store = open(&path);
        store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Portable setup".into(),
                    points: points(1.0),
                    expected_context: mech(),
                },
                mech(),
                10,
            )
            .unwrap();

        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let preset = persisted["presets"][0].as_object().unwrap();
        assert_eq!(
            preset
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            [
                "activityFamilyId",
                "name",
                "points",
                "presetId",
                "savedAtUnixMillis"
            ]
            .into_iter()
            .collect()
        );
        for forbidden in [
            "accountId",
            "accountUuid",
            "characterId",
            "characterUuid",
            "playerId",
            "playerUuid",
            "sessionId",
            "sessionSequence",
            "skillUuid",
            "entityUuid",
            "clientBuild",
            "sceneId",
            "mapId",
        ] {
            assert!(
                !preset.contains_key(forbidden),
                "portable preset unexpectedly persisted {forbidden}"
            );
        }
        assert_eq!(preset["activityFamilyId"], "mech-facility");
        assert_eq!(preset["points"][0]["markerNumber"], 1);
        assert_eq!(preset["points"][0]["x"], 1.0);

        drop(store);
        assert_eq!(open(&path).compatible(mech()).presets.len(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tina_master_presets_are_tier_independent_but_isolated_from_other_scene_families() {
        let path = temporary_path("scope");
        let mut store = open(&path);
        let saved = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "M1".into(),
                    points: points(1.0),
                    expected_context: tina(1_633),
                },
                tina(1_633),
                10,
            )
            .unwrap();
        let preset_id = saved.presets[0].preset_id.clone();
        assert_eq!(store.compatible(tina(1_633)).presets.len(), 1);
        assert!(
            store
                .load(
                    LoadAutomarkerPresetRequest {
                        preset_id,
                        expected_context: tina(1_633)
                    },
                    tina(1_633)
                )
                .is_ok()
        );
        for scene_id in [1_621, 1_631, 1_632] {
            assert!(store.compatible(tina(scene_id)).presets.is_empty());
            assert!(
                store
                    .load(
                        LoadAutomarkerPresetRequest {
                            preset_id: saved.presets[0].preset_id.clone(),
                            expected_context: tina(scene_id),
                        },
                        tina(scene_id),
                    )
                    .is_err()
            );
        }
        assert!(store.compatible(mech()).presets.is_empty());
        assert!(
            store
                .load(
                    LoadAutomarkerPresetRequest {
                        preset_id: saved.presets[0].preset_id.clone(),
                        expected_context: mech(),
                    },
                    mech(),
                )
                .is_err()
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn keeps_family_presets_visible_across_build_scene_and_map_updates() {
        let path = temporary_path("build-provenance");
        let mut store = open(&path);
        store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "M1".into(),
                    points: points(1.0),
                    expected_context: mech(),
                },
                mech(),
                10,
            )
            .unwrap();
        let mut patched = mech();
        patched.client_build = "24699999".into();
        patched.scene_id = 1_101;
        patched.map_id = 9_999;
        let view = store.compatible(patched.clone());
        assert_eq!(view.presets.len(), 1);
        assert!(!view.capture_supported);
        assert!(!view.native_load_supported);
        assert!(
            store
                .load(
                    LoadAutomarkerPresetRequest {
                        preset_id: view.presets[0].preset_id.clone(),
                        expected_context: patched.clone(),
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
        let mut store = open(&path);
        store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Safe".into(),
                    points: points(1.0),
                    expected_context: mech(),
                },
                mech(),
                10,
            )
            .unwrap();
        drop(store);
        let backup = sibling_path(&path, "backup");
        std::fs::rename(&path, &backup).unwrap();
        let recovered = open(&path);
        assert_eq!(recovered.compatible(mech()).presets[0].name, "Safe");
        assert!(path.is_file());
        assert!(!backup.exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn local_load_returns_exact_persisted_xyz_without_native_side_effects() {
        let path = temporary_path("persist");
        let mut store = open(&path);
        let saved = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Exact".into(),
                    points: vec![AutomarkerPoint {
                        marker_number: 6,
                        x: -1.25,
                        y: 9.5,
                        z: 44.125,
                    }],
                    expected_context: mech(),
                },
                mech(),
                10,
            )
            .unwrap();
        let preset_id = saved.presets[0].preset_id.clone();
        drop(store);
        let reopened = open(&path);
        assert_eq!(reopened.compatible(mech()).presets[0].points[0].z, 44.125);
        assert_eq!(
            reopened
                .load(
                    LoadAutomarkerPresetRequest {
                        preset_id,
                        expected_context: mech()
                    },
                    mech()
                )
                .unwrap()
                .preset
                .points[0]
                .z,
            44.125
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_duplicate_numbers_and_unbounded_manual_coordinates_before_persisting() {
        let path = temporary_path("manual-validation");
        let mut store = open(&path);
        let duplicate = store.save(
            SaveAutomarkerPresetRequest {
                preset_id: None,
                name: "Duplicate".into(),
                points: vec![
                    AutomarkerPoint {
                        marker_number: 1,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    AutomarkerPoint {
                        marker_number: 1,
                        x: 1.0,
                        y: 1.0,
                        z: 1.0,
                    },
                ],
                expected_context: mech(),
            },
            mech(),
            10,
        );
        assert!(duplicate.unwrap_err().contains("must be unique"));
        let unbounded = store.save(
            SaveAutomarkerPresetRequest {
                preset_id: None,
                name: "Unbounded".into(),
                points: vec![AutomarkerPoint {
                    marker_number: 2,
                    x: 1_000_001.0,
                    y: 0.0,
                    z: 0.0,
                }],
                expected_context: mech(),
            },
            mech(),
            11,
        );
        assert!(unbounded.unwrap_err().contains("coordinates are invalid"));
        assert!(!path.exists());
    }

    #[test]
    fn rejects_a_save_when_the_editor_context_changed_even_within_one_family() {
        let path = temporary_path("context-race");
        let mut store = open(&path);
        let result = store.save(
            SaveAutomarkerPresetRequest {
                preset_id: None,
                name: "Stale editor".into(),
                points: points(1.0),
                expected_context: tina(1_633),
            },
            tina(1_631),
            10,
        );
        assert!(result.unwrap_err().contains("context changed"));
        assert!(!path.exists());
    }

    #[test]
    fn rejects_a_load_when_the_editor_context_changed_even_within_one_family() {
        let path = temporary_path("load-context-race");
        let mut store = open(&path);
        let saved = store
            .save(
                SaveAutomarkerPresetRequest {
                    preset_id: None,
                    name: "Stale editor".into(),
                    points: points(1.0),
                    expected_context: tina(1_633),
                },
                tina(1_633),
                10,
            )
            .unwrap();
        let result = store.load(
            LoadAutomarkerPresetRequest {
                preset_id: saved.presets[0].preset_id.clone(),
                expected_context: tina(1_633),
            },
            tina(1_631),
        );
        assert!(result.unwrap_err().contains("context changed"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn committed_backup_cleanup_failure_is_non_fatal() {
        let live = temporary_path("cleanup-live");
        let backup = temporary_path("cleanup-failure");
        std::fs::write(&live, b"committed").unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        assert!(finish_committed_write(&backup).is_ok());
        assert_eq!(std::fs::read(&live).unwrap(), b"committed");
        assert!(
            backup.is_dir(),
            "the injected remove-file failure remains recoverable hygiene"
        );
        std::fs::remove_dir(&backup).unwrap();
        std::fs::remove_file(&live).unwrap();
    }

    #[test]
    fn migrates_schema_one_scene_provenance_through_the_reviewed_family_catalog() {
        let path = temporary_path("schema-one");
        let legacy = serde_json::json!({
            "schemaVersion": 1,
            "presets": [{
                "presetId": "preset-legacy-tina",
                "name": "Tina M1",
                "clientBuild": "24687926",
                "sceneId": 1633,
                "mapId": 1633,
                "savedAtUnixMillis": 10,
                "points": [{ "markerNumber": 1, "x": 1.0, "y": 2.0, "z": 3.0 }]
            }]
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        let store = open(&path);
        let migrated = store.compatible(tina(1_633));
        assert_eq!(migrated.presets.len(), 1);
        assert_eq!(migrated.presets[0].activity_family_id, "dungeon.1633");
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted["schemaVersion"], 4);
        assert_eq!(persisted["presets"][0]["activityFamilyId"], "dungeon.1633");
        assert!(persisted["presets"][0].get("clientBuild").is_none());
        assert!(persisted["presets"][0].get("sceneId").is_none());
        assert!(persisted["presets"][0].get("mapId").is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn migrates_schema_two_family_identity_deterministically_from_stored_scene_id() {
        let path = temporary_path("schema-two");
        let legacy = serde_json::json!({
            "schemaVersion": 2,
            "presets": [{
                "presetId": "preset-legacy-tina-v2",
                "name": "Tina master",
                "clientBuild": "24687926",
                "sceneId": 1633,
                "mapId": 1633,
                "activityFamilyId": "tina-mindrealm",
                "savedAtUnixMillis": 10,
                "points": [{ "markerNumber": 1, "x": 1.0, "y": 2.0, "z": 3.0 }]
            }]
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        let store = open(&path);
        let migrated = store.compatible(tina(1_633));
        assert_eq!(migrated.presets.len(), 1);
        assert_eq!(migrated.presets[0].activity_family_id, "dungeon.1633");
        assert!(store.compatible(tina(1_631)).presets.is_empty());
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted["schemaVersion"], 4);
        assert_eq!(persisted["presets"][0]["activityFamilyId"], "dungeon.1633");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_schema_three_family_identity_that_disagrees_with_the_reviewed_scene() {
        let path = temporary_path("schema-three-family-mismatch");
        let invalid = serde_json::json!({
            "schemaVersion": 3,
            "presets": [{
                "presetId": "preset-forged-family-v3",
                "name": "Wrong family",
                "clientBuild": "24687926",
                "sceneId": 1633,
                "mapId": 1633,
                "activityFamilyId": "tina-mindrealm",
                "savedAtUnixMillis": 10,
                "points": [{ "markerNumber": 1, "x": 1.0, "y": 2.0, "z": 3.0 }]
            }]
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&invalid).unwrap()).unwrap();
        assert!(
            AutomarkerPresetStore::open(&path, &families())
                .unwrap_err()
                .contains("does not match the reviewed automarker-family identity")
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn migrates_schema_three_to_portable_schema_four_and_survives_catalog_scene_removal() {
        let path = temporary_path("schema-three-portable");
        let legacy = serde_json::json!({
            "schemaVersion": 3,
            "presets": [{
                "presetId": "preset-portable-v3",
                "name": "Portable",
                "clientBuild": "24687926",
                "sceneId": 1100,
                "mapId": 1100,
                "activityFamilyId": "mech-facility",
                "savedAtUnixMillis": 10,
                "points": [{ "markerNumber": 1, "x": 1.0, "y": 2.0, "z": 3.0 }]
            }]
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        drop(open(&path));

        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted["schemaVersion"], 4);
        assert!(persisted["presets"][0].get("clientBuild").is_none());
        assert!(persisted["presets"][0].get("sceneId").is_none());
        assert!(persisted["presets"][0].get("mapId").is_none());

        let reopened = AutomarkerPresetStore::open(&path, &BTreeMap::new()).unwrap();
        assert_eq!(reopened.compatible(mech()).presets.len(), 1);
        assert!(reopened.compatible(tina(1_633)).presets.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn schema_four_rejects_unknown_identity_and_provenance_fields() {
        for field in [
            "accountUuid",
            "characterUuid",
            "playerUuid",
            "entityUuid",
            "skillUuid",
            "sessionSequence",
            "clientBuild",
            "sceneId",
            "mapId",
        ] {
            let path = temporary_path(field);
            let mut preset = serde_json::json!({
                "presetId": "preset-unknown-field",
                "name": "Portable",
                "activityFamilyId": "mech-facility",
                "savedAtUnixMillis": 10,
                "points": [{ "markerNumber": 1, "x": 1.0, "y": 2.0, "z": 3.0 }]
            });
            preset
                .as_object_mut()
                .unwrap()
                .insert(field.into(), serde_json::json!(1));
            let invalid = serde_json::json!({ "schemaVersion": 4, "presets": [preset] });
            std::fs::write(&path, serde_json::to_vec_pretty(&invalid).unwrap()).unwrap();
            assert!(
                AutomarkerPresetStore::open(&path, &families()).is_err(),
                "accepted {field}"
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn refuses_to_guess_a_family_for_an_unknown_legacy_scene() {
        let path = temporary_path("schema-one-unknown");
        let legacy = serde_json::json!({
            "schemaVersion": 1,
            "presets": [{
                "presetId": "preset-legacy-unknown",
                "name": "Unknown",
                "clientBuild": "24687926",
                "sceneId": 999999,
                "mapId": 999999,
                "savedAtUnixMillis": 10,
                "points": [{ "markerNumber": 1, "x": 1.0, "y": 2.0, "z": 3.0 }]
            }]
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        assert!(
            AutomarkerPresetStore::open(&path, &families())
                .unwrap_err()
                .to_string()
                .contains("without a reviewed dungeon-family identity")
        );
        let unchanged: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(unchanged["schemaVersion"], 1);
        let _ = std::fs::remove_file(path);
    }
}
