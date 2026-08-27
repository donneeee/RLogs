use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;

use rlogs_events::{DungeonObjectiveCatalogReference, DungeonObjectiveCatalogResolution};
use rlogs_game_data::{CachePolicy, GameDataBuild, GameDataError, GameDataStore, SymbolKind};
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
