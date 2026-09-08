use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use rlogs_events::{DungeonObjectiveCatalogReference, DungeonObjectiveCatalogResolution};
use rlogs_game_data::{CachePolicy, GameDataBuild, GameDataError, GameDataStore, SymbolKind};
use serde::Deserialize;
use thiserror::Error;

use crate::GameBuild;

/// Build-pinned lookup used to attach stable behavior identities to raw
/// dungeon objective events.
///
/// Implementations must never replace or reinterpret the raw objective ID.
/// A lookup failure only changes the catalog-resolution metadata carried next
/// to that authoritative wire value.
pub trait ObjectiveCatalogResolver: Debug + Send + Sync {
    fn resolve(
        &self,
        objective_id: i64,
    ) -> Result<Option<DungeonObjectiveCatalogReference>, ObjectiveCatalogError>;
}

const BUNDLED_OBJECTIVES_JSON: &str =
    include_str!("../game-data/runtime/dungeon-objectives.current-build.v1.json");

#[derive(Debug, Clone)]
pub struct BundledObjectiveCatalog;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DungeonObjectivePresentation {
    pub stable_key: String,
    pub localization_key: Option<String>,
    pub required_count: i64,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BundledObjectiveCatalogFile {
    schema_version: u16,
    deployment_id: String,
    channel: String,
    client_build: String,
    proof: BundledObjectiveProof,
    entries: Vec<BundledObjectiveEntry>,
}

#[derive(Debug, Deserialize)]
struct BundledObjectiveProof {
    source_table: String,
    current_table_sha256: String,
    current_table_rows: usize,
    source_and_current_tables_are_byte_identical: bool,
}

#[derive(Debug, Deserialize)]
struct BundledObjectiveEntry {
    objective_id: i64,
    stable_key: String,
    localization_key: Option<String>,
    required_count: i64,
    #[serde(default)]
    scene_event_keys: Vec<String>,
    #[serde(default)]
    names: BTreeMap<String, String>,
}

static BUNDLED_OBJECTIVES: OnceLock<Result<BundledObjectiveCatalogFile, String>> = OnceLock::new();

impl BundledObjectiveCatalog {
    pub fn open_for_game_build(build: &GameBuild) -> Result<Self, ObjectiveCatalogError> {
        let catalog = bundled_objectives()?;
        if catalog.deployment_id != build.deployment_id
            || catalog.channel != build.channel
            || catalog.client_build != build.build_id
        {
            return Err(ObjectiveCatalogError::UnsupportedBuild {
                deployment_id: build.deployment_id.clone(),
                channel: build.channel.clone(),
                client_build: build.build_id.clone(),
            });
        }
        Ok(Self)
    }
}

impl ObjectiveCatalogResolver for BundledObjectiveCatalog {
    fn resolve(
        &self,
        objective_id: i64,
    ) -> Result<Option<DungeonObjectiveCatalogReference>, ObjectiveCatalogError> {
        let Some(entry) = find_bundled_objective(objective_id)? else {
            return Ok(None);
        };
        Ok(Some(DungeonObjectiveCatalogReference {
            resolution: DungeonObjectiveCatalogResolution::ResolvedCurrentBuild,
            activity_target_key: Some(entry.stable_key.clone()),
            localization_key: entry.localization_key.clone(),
            required_count: Some(entry.required_count),
            scene_event_keys: entry.scene_event_keys.clone(),
        }))
    }
}

pub fn bundled_dungeon_objective_presentation(
    client_build: &str,
    objective_id: i64,
    locale: &str,
) -> Result<Option<DungeonObjectivePresentation>, ObjectiveCatalogError> {
    let catalog = bundled_objectives()?;
    let expected = format!(
        "{}/{}-{}",
        catalog.deployment_id, catalog.channel, catalog.client_build
    );
    if client_build != expected {
        return Ok(None);
    }
    Ok(
        find_bundled_objective(objective_id)?.map(|entry| DungeonObjectivePresentation {
            stable_key: entry.stable_key.clone(),
            localization_key: entry.localization_key.clone(),
            required_count: entry.required_count,
            name: entry
                .names
                .get(locale)
                .or_else(|| entry.names.get("en-US"))
                .cloned(),
        }),
    )
}

fn find_bundled_objective(
    objective_id: i64,
) -> Result<Option<&'static BundledObjectiveEntry>, ObjectiveCatalogError> {
    let catalog = bundled_objectives()?;
    Ok(catalog
        .entries
        .binary_search_by_key(&objective_id, |entry| entry.objective_id)
        .ok()
        .map(|index| &catalog.entries[index]))
}

fn bundled_objectives() -> Result<&'static BundledObjectiveCatalogFile, ObjectiveCatalogError> {
    match BUNDLED_OBJECTIVES.get_or_init(|| {
        let catalog: BundledObjectiveCatalogFile =
            serde_json::from_str(BUNDLED_OBJECTIVES_JSON).map_err(|error| error.to_string())?;
        if catalog.schema_version != 1
            || catalog.proof.source_table != "ctb.TargetTable"
            || !catalog.proof.source_and_current_tables_are_byte_identical
            || catalog.proof.current_table_rows != catalog.entries.len()
            || !catalog.proof.current_table_sha256.starts_with("sha256:")
            || catalog
                .entries
                .windows(2)
                .any(|pair| pair[0].objective_id >= pair[1].objective_id)
        {
            return Err("bundled dungeon objective proof or ordering is invalid".into());
        }
        Ok(catalog)
    }) {
        Ok(catalog) => Ok(catalog),
        Err(detail) => Err(ObjectiveCatalogError::InvalidBundledCatalog(detail.clone())),
    }
}

/// Lazy resolver backed by the shared, sharded game-data bundle.
///
/// Only the numeric activity-target shard touched by a packet is loaded. Scene
/// event backreferences are embedded as stable keys in that record, so runtime
/// decoding does not need a second full-catalog index.
#[derive(Debug)]
pub struct GameDataObjectiveCatalog {
    store: Arc<GameDataStore>,
    build: GameDataBuild,
}

impl GameDataObjectiveCatalog {
    pub fn open_for_game_build(
        root: impl AsRef<Path>,
        build: &GameBuild,
    ) -> Result<Self, ObjectiveCatalogError> {
        Self::open_for_game_build_with_policy(root, build, CachePolicy::default())
    }

    pub fn open_for_game_build_with_policy(
        root: impl AsRef<Path>,
        build: &GameBuild,
        policy: CachePolicy,
    ) -> Result<Self, ObjectiveCatalogError> {
        let store = Arc::new(GameDataStore::open(root, policy)?);
        Self::new(store, game_data_build(build))
    }

    pub fn new(
        store: Arc<GameDataStore>,
        build: GameDataBuild,
    ) -> Result<Self, ObjectiveCatalogError> {
        if !store.manifest().game_data.supported_builds.contains(&build) {
            return Err(ObjectiveCatalogError::UnsupportedBuild {
                deployment_id: build.deployment_id,
                channel: build.channel,
                client_build: build.client_build,
            });
        }
        Ok(Self { store, build })
    }
}

impl ObjectiveCatalogResolver for GameDataObjectiveCatalog {
    fn resolve(
        &self,
        objective_id: i64,
    ) -> Result<Option<DungeonObjectiveCatalogReference>, ObjectiveCatalogError> {
        let Some(record) =
            self.store
                .record_for_build(SymbolKind::ActivityTarget, objective_id, &self.build)?
        else {
            return Ok(None);
        };

        let scene_event_keys = record
            .attributes
            .get("scene_event_keys")
            .ok_or_else(|| ObjectiveCatalogError::InvalidActivityTarget {
                objective_id,
                detail: "scene_event_keys is missing".into(),
            })?
            .as_array()
            .ok_or_else(|| ObjectiveCatalogError::InvalidActivityTarget {
                objective_id,
                detail: "scene_event_keys is not an array".into(),
            })?
            .iter()
            .map(|value| {
                value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                    ObjectiveCatalogError::InvalidActivityTarget {
                        objective_id,
                        detail: "scene_event_keys contains a non-string value".into(),
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(DungeonObjectiveCatalogReference {
            resolution: DungeonObjectiveCatalogResolution::ResolvedCurrentBuild,
            activity_target_key: Some(record.stable_key.clone()),
            localization_key: record.localization_key.clone(),
            required_count: record
                .attributes
                .get("required_count")
                .and_then(serde_json::Value::as_i64),
            scene_event_keys,
        }))
    }
}

fn game_data_build(build: &GameBuild) -> GameDataBuild {
    GameDataBuild {
        deployment_id: build.deployment_id.clone(),
        channel: build.channel.clone(),
        client_build: build.build_id.clone(),
    }
}

#[derive(Debug, Error)]
pub enum ObjectiveCatalogError {
    #[error(transparent)]
    GameData(#[from] GameDataError),
    #[error(
        "game-data bundle does not support deployment {deployment_id}, channel {channel}, build {client_build}"
    )]
    UnsupportedBuild {
        deployment_id: String,
        channel: String,
        client_build: String,
    },
    #[error("activity target {objective_id} is invalid: {detail}")]
    InvalidActivityTarget { objective_id: i64, detail: String },
    #[error("bundled dungeon objective catalog is invalid: {0}")]
    InvalidBundledCatalog(String),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use rlogs_game_data::{
        CompiledShardDescriptor, GAME_DATA_SCHEMA_VERSION, GameDataManifest, GameDataRecord,
        ResearchConfidence, ShardKind, SymbolProvenance, build_bundle_manifest, encode_json_shard,
        numeric_id_bucket,
    };
    use serde_json::json;
    use sha2::{Digest, Sha256};

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    struct FixtureDirectory(std::path::PathBuf);

    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn lazy_catalog_resolves_stable_objective_and_scene_event_keys() {
        let (directory, build) = fixture_catalog();
        let catalog = GameDataObjectiveCatalog::open_for_game_build(&directory.0, &build).unwrap();

        let reference = catalog.resolve(9_001).unwrap().unwrap();
        assert_eq!(
            reference.resolution,
            DungeonObjectiveCatalogResolution::ResolvedCurrentBuild
        );
        assert_eq!(
            reference.activity_target_key.as_deref(),
            Some("activity-target.9001")
        );
        assert_eq!(
            reference.scene_event_keys,
            ["scene-event.77", "scene-event.88"]
        );
        assert!(catalog.resolve(9_002).unwrap().is_none());
    }

    #[test]
    fn catalog_rejects_a_different_client_build() {
        let (directory, mut build) = fixture_catalog();
        build.build_id = "build-2".into();

        assert!(matches!(
            GameDataObjectiveCatalog::open_for_game_build(&directory.0, &build),
            Err(ObjectiveCatalogError::UnsupportedBuild { .. })
        ));
    }

    #[test]
    fn bundled_catalog_resolves_current_build_objective_and_presentation() {
        let build = GameBuild {
            deployment_id: "global".into(),
            region_id: Some("north-america".into()),
            channel: "steam".into(),
            build_id: "24687926".into(),
            executable_version: None,
        };
        let catalog = BundledObjectiveCatalog::open_for_game_build(&build).unwrap();
        let reference = catalog.resolve(651_103).unwrap().unwrap();
        assert_eq!(
            reference.resolution,
            DungeonObjectiveCatalogResolution::ResolvedCurrentBuild
        );
        assert_eq!(
            reference.activity_target_key.as_deref(),
            Some("activity-target.651103")
        );
        assert_eq!(reference.required_count, Some(400));

        let presentation =
            bundled_dungeon_objective_presentation("global/steam-24687926", 651_103, "en-US")
                .unwrap()
                .unwrap();
        assert_eq!(presentation.required_count, 400);
        assert_eq!(
            presentation.name.as_deref(),
            Some("Complete the spatial investigation to unlock the boss battle")
        );
    }

    #[test]
    fn bundled_catalog_rejects_other_builds() {
        let build = GameBuild {
            deployment_id: "global".into(),
            region_id: Some("europe".into()),
            channel: "steam".into(),
            build_id: "different-build".into(),
            executable_version: None,
        };
        assert!(matches!(
            BundledObjectiveCatalog::open_for_game_build(&build),
            Err(ObjectiveCatalogError::UnsupportedBuild { .. })
        ));
        assert!(
            bundled_dungeon_objective_presentation(
                "global/steam-different-build",
                651_103,
                "en-US"
            )
            .unwrap()
            .is_none()
        );
    }

    fn fixture_catalog() -> (FixtureDirectory, GameBuild) {
        let directory = std::env::temp_dir().join(format!(
            "rlogs-bpsr-objective-catalog-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let data_build = GameDataBuild {
            deployment_id: "global".into(),
            channel: "steam".into(),
            client_build: "build-1".into(),
        };
        let mut attributes = BTreeMap::new();
        attributes.insert(
            "scene_event_keys".into(),
            json!(["scene-event.77", "scene-event.88"]),
        );
        let record = GameDataRecord {
            schema_version: GAME_DATA_SCHEMA_VERSION,
            kind: SymbolKind::ActivityTarget,
            id: 9_001,
            stable_key: "activity-target.9001".into(),
            localization_key: Some("activity-target.9001.name".into()),
            icon: None,
            attributes,
            availability: vec![data_build.clone()],
            provenance: SymbolProvenance {
                source: "fixture".into(),
                reference: "fixture:activity-target.9001".into(),
                confidence: ResearchConfidence::Verified,
            },
        };
        let shard_bits = 4;
        let bucket = numeric_id_bucket(record.id, shard_bits);
        let relative_path = format!("records/activity-targets/{bucket:02x}.json.zst");
        let (compressed, uncompressed_bytes, content_sha256) =
            encode_json_shard(&[record]).unwrap();
        let compressed_sha256 = format!("sha256:{:x}", Sha256::digest(&compressed));
        let descriptor = CompiledShardDescriptor {
            kind: ShardKind::Records,
            symbol_kind: Some(SymbolKind::ActivityTarget),
            locale: None,
            bucket,
            relative_path: relative_path.clone(),
            entries: 1,
            compressed_bytes: compressed.len() as u64,
            uncompressed_bytes,
            compressed_sha256,
            content_sha256,
        };
        let manifest = build_bundle_manifest(
            GameDataManifest {
                schema_version: GAME_DATA_SCHEMA_VERSION,
                catalog_id: "fixture".into(),
                catalog_revision: "fixture-1".into(),
                supported_builds: vec![data_build],
            },
            shard_bits,
            vec![descriptor],
        )
        .unwrap();
        let shard_path = directory.join(relative_path);
        fs::create_dir_all(shard_path.parent().unwrap()).unwrap();
        fs::write(shard_path, compressed).unwrap();
        fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        (
            FixtureDirectory(directory),
            GameBuild {
                deployment_id: "global".into(),
                region_id: Some("north-america".into()),
                channel: "steam".into(),
                build_id: "build-1".into(),
                executable_version: None,
            },
        )
    }
}
