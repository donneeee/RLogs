//! Validated runtime indexes for reviewed game-data end products.
//!
//! Acquisition and extraction are intentionally outside this crate.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const GAME_DATA_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameDataManifest {
    pub schema_version: u16,
    pub deployment_id: String,
    pub region_id: Option<String>,
    pub client_build: String,
    pub source_revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Class,
    Specialization,
    Skill,
    StatusEffect,
    Monster,
    Npc,
    Summon,
    Projectile,
    Trap,
    Mechanic,
    EntityType,
    Scene,
    Map,
    Dungeon,
    DungeonObjective,
    Item,
    Equipment,
    EquipmentSet,
    Imagine,
    Cosmetic,
    Profession,
    Talent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchConfidence {
    Verified,
    Corroborated,
    Candidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolProvenance {
    /// Build-scoped source identifier, never an extraction command.
    pub source: String,
    pub reference: String,
    pub confidence: ResearchConfidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameDataRecord {
    pub schema_version: u16,
    pub kind: SymbolKind,
    pub id: i64,
    pub stable_key: String,
    pub localization_key: Option<String>,
    pub icon: Option<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
    pub provenance: SymbolProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizationEntry {
    pub schema_version: u16,
    pub locale: String,
    pub key: String,
    pub text: String,
    pub provenance: SymbolProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRecord {
    pub key: String,
    pub relative_path: String,
    pub media_type: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledGameDataPayload {
    pub manifest: GameDataManifest,
    pub records: Vec<GameDataRecord>,
    pub localization: Vec<LocalizationEntry>,
    pub assets: Vec<AssetRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledGameDataArtifact {
    pub schema_version: u16,
    pub content_digest: String,
    pub payload: CompiledGameDataPayload,
}

impl CompiledGameDataArtifact {
    pub fn build(
        manifest: GameDataManifest,
        mut records: Vec<GameDataRecord>,
        mut localization: Vec<LocalizationEntry>,
        mut assets: Vec<AssetRecord>,
    ) -> Result<Self, GameDataError> {
        validate_manifest(&manifest)?;
        validate_records(&records)?;
        validate_localization(&localization)?;
        validate_assets(&assets)?;

        records.sort_by(|left, right| {
            (left.kind, left.id, &left.stable_key).cmp(&(right.kind, right.id, &right.stable_key))
        });
        localization
            .sort_by(|left, right| (&left.locale, &left.key).cmp(&(&right.locale, &right.key)));
        assets.sort_by(|left, right| left.key.cmp(&right.key));

        let payload = CompiledGameDataPayload {
            manifest,
            records,
            localization,
            assets,
        };
        let content_digest = digest_payload(&payload)?;
        Ok(Self {
            schema_version: GAME_DATA_SCHEMA_VERSION,
            content_digest,
            payload,
        })
    }

    pub fn validate(&self) -> Result<(), GameDataError> {
        if self.schema_version != GAME_DATA_SCHEMA_VERSION {
            return Err(GameDataError::UnsupportedSchemaVersion {
                actual: self.schema_version,
            });
        }
        validate_manifest(&self.payload.manifest)?;
        validate_records(&self.payload.records)?;
        validate_localization(&self.payload.localization)?;
        validate_assets(&self.payload.assets)?;
        let actual = digest_payload(&self.payload)?;
        if actual != self.content_digest {
            return Err(GameDataError::DigestMismatch {
                expected: self.content_digest.clone(),
                actual,
            });
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct GameDataIndex {
    artifact: CompiledGameDataArtifact,
    by_id: HashMap<(SymbolKind, i64), usize>,
    by_key: HashMap<String, usize>,
    localized: HashMap<String, HashMap<String, usize>>,
    assets: HashMap<String, usize>,
}

impl GameDataIndex {
    pub fn from_json(json: &[u8]) -> Result<Self, GameDataError> {
        let artifact: CompiledGameDataArtifact = serde_json::from_slice(json)
            .map_err(|error| GameDataError::Serialization(error.to_string()))?;
        Self::build_index(artifact)
    }

    pub fn build_index(artifact: CompiledGameDataArtifact) -> Result<Self, GameDataError> {
        artifact.validate()?;
        let by_id = artifact
            .payload
            .records
            .iter()
            .enumerate()
            .map(|(index, record)| ((record.kind, record.id), index))
            .collect();
        let by_key = artifact
            .payload
            .records
            .iter()
            .enumerate()
            .map(|(index, record)| (record.stable_key.clone(), index))
            .collect();
        let mut localized = HashMap::<String, HashMap<String, usize>>::new();
        for (index, entry) in artifact.payload.localization.iter().enumerate() {
            localized
                .entry(entry.locale.clone())
                .or_default()
                .insert(entry.key.clone(), index);
        }
        let assets = artifact
            .payload
            .assets
            .iter()
            .enumerate()
            .map(|(index, asset)| (asset.key.clone(), index))
            .collect();
        Ok(Self {
            artifact,
            by_id,
            by_key,
            localized,
            assets,
        })
    }

    pub fn record(&self, kind: SymbolKind, id: i64) -> Option<&GameDataRecord> {
        self.by_id
            .get(&(kind, id))
            .map(|index| &self.artifact.payload.records[*index])
    }

    pub fn record_by_key(&self, key: &str) -> Option<&GameDataRecord> {
        self.by_key
            .get(key)
            .map(|index| &self.artifact.payload.records[*index])
    }

    pub fn localized(&self, locale: &str, key: &str) -> Option<&str> {
        self.localized
            .get(locale)?
            .get(key)
            .map(|index| self.artifact.payload.localization[*index].text.as_str())
    }

    pub fn asset(&self, key: &str) -> Option<&AssetRecord> {
        self.assets
            .get(key)
            .map(|index| &self.artifact.payload.assets[*index])
    }

    pub fn manifest(&self) -> &GameDataManifest {
        &self.artifact.payload.manifest
    }

    pub fn digest(&self) -> &str {
        &self.artifact.content_digest
    }
}

fn validate_manifest(manifest: &GameDataManifest) -> Result<(), GameDataError> {
    if manifest.schema_version != GAME_DATA_SCHEMA_VERSION {
        return Err(GameDataError::UnsupportedSchemaVersion {
            actual: manifest.schema_version,
        });
    }
    for (field, value) in [
        ("deployment_id", manifest.deployment_id.as_str()),
        ("client_build", manifest.client_build.as_str()),
        ("source_revision", manifest.source_revision.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(GameDataError::EmptyField(field));
        }
    }
    Ok(())
}

fn validate_records(records: &[GameDataRecord]) -> Result<(), GameDataError> {
    let mut ids = HashSet::with_capacity(records.len());
    let mut keys = HashSet::with_capacity(records.len());
    for record in records {
        if record.schema_version != GAME_DATA_SCHEMA_VERSION {
            return Err(GameDataError::UnsupportedSchemaVersion {
                actual: record.schema_version,
            });
        }
        if record.stable_key.trim().is_empty() {
            return Err(GameDataError::EmptyField("record.stable_key"));
        }
        if !ids.insert((record.kind, record.id)) {
            return Err(GameDataError::DuplicateSymbolId {
                kind: record.kind,
                id: record.id,
            });
        }
        if !keys.insert(record.stable_key.clone()) {
            return Err(GameDataError::DuplicateStableKey(record.stable_key.clone()));
        }
    }
    Ok(())
}

fn validate_localization(entries: &[LocalizationEntry]) -> Result<(), GameDataError> {
    let mut keys = HashSet::with_capacity(entries.len());
    for entry in entries {
        if entry.schema_version != GAME_DATA_SCHEMA_VERSION {
            return Err(GameDataError::UnsupportedSchemaVersion {
                actual: entry.schema_version,
            });
        }
        if entry.locale.trim().is_empty() || entry.key.trim().is_empty() {
            return Err(GameDataError::EmptyField("localization.locale_or_key"));
        }
        if !keys.insert((entry.locale.clone(), entry.key.clone())) {
            return Err(GameDataError::DuplicateLocalization {
                locale: entry.locale.clone(),
                key: entry.key.clone(),
            });
        }
    }
    Ok(())
}

fn validate_assets(assets: &[AssetRecord]) -> Result<(), GameDataError> {
    let mut keys = HashSet::with_capacity(assets.len());
    let mut paths = HashSet::with_capacity(assets.len());
    for asset in assets {
        if !keys.insert(asset.key.clone()) {
            return Err(GameDataError::DuplicateAssetKey(asset.key.clone()));
        }
        if !paths.insert(asset.relative_path.clone()) {
            return Err(GameDataError::DuplicateAssetPath(
                asset.relative_path.clone(),
            ));
        }
        if !asset.sha256.starts_with("sha256:") {
            return Err(GameDataError::InvalidAssetDigest(asset.key.clone()));
        }
    }
    Ok(())
}

fn digest_payload(payload: &CompiledGameDataPayload) -> Result<String, GameDataError> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| GameDataError::Serialization(error.to_string()))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GameDataError {
    #[error("unsupported game-data schema version {actual}")]
    UnsupportedSchemaVersion { actual: u16 },

    #[error("game-data field {0} must not be empty")]
    EmptyField(&'static str),

    #[error("duplicate {kind:?} id {id}")]
    DuplicateSymbolId { kind: SymbolKind, id: i64 },

    #[error("duplicate stable key {0}")]
    DuplicateStableKey(String),

    #[error("duplicate localization key {key} for locale {locale}")]
    DuplicateLocalization { locale: String, key: String },

    #[error("duplicate asset key {0}")]
    DuplicateAssetKey(String),

    #[error("duplicate asset path {0}")]
    DuplicateAssetPath(String),

    #[error("asset {0} does not have a sha256 digest")]
    InvalidAssetDigest(String),

    #[error("compiled game-data digest mismatch: expected {expected}, actual {actual}")]
    DigestMismatch { expected: String, actual: String },

    #[error("game-data serialization failed: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provenance() -> SymbolProvenance {
        SymbolProvenance {
            source: "reviewed-game-data".into(),
            reference: "skills/stormblade/iaido/1714-example.json".into(),
            confidence: ResearchConfidence::Verified,
        }
    }

    fn manifest() -> GameDataManifest {
        GameDataManifest {
            schema_version: GAME_DATA_SCHEMA_VERSION,
            deployment_id: "global".into(),
            region_id: None,
            client_build: "example-build".into(),
            source_revision: "review-1".into(),
        }
    }

    fn skill(id: i64, key: &str) -> GameDataRecord {
        GameDataRecord {
            schema_version: GAME_DATA_SCHEMA_VERSION,
            kind: SymbolKind::Skill,
            id,
            stable_key: key.into(),
            localization_key: Some(format!("game.skill.{id}")),
            icon: Some(format!("icons/skills/stormblade/iaido/{id}.webp")),
            attributes: BTreeMap::new(),
            provenance: provenance(),
        }
    }

    #[test]
    fn compiled_artifact_builds_constant_time_indexes() {
        let artifact = CompiledGameDataArtifact::build(
            manifest(),
            vec![skill(1714, "skill.stormblade.iaido.1714")],
            vec![LocalizationEntry {
                schema_version: GAME_DATA_SCHEMA_VERSION,
                locale: "en-US".into(),
                key: "game.skill.1714".into(),
                text: "Example Skill".into(),
                provenance: provenance(),
            }],
            vec![AssetRecord {
                key: "skill.stormblade.iaido.1714".into(),
                relative_path: "icons/skills/stormblade/iaido/1714.webp".into(),
                media_type: "image/webp".into(),
                sha256: format!("sha256:{:064x}", 1),
            }],
        )
        .unwrap();
        let index = GameDataIndex::build_index(artifact).unwrap();

        assert_eq!(
            index
                .record(SymbolKind::Skill, 1714)
                .map(|record| record.stable_key.as_str()),
            Some("skill.stormblade.iaido.1714")
        );
        assert_eq!(
            index.localized("en-US", "game.skill.1714"),
            Some("Example Skill")
        );
        assert!(index.asset("skill.stormblade.iaido.1714").is_some());
    }

    #[test]
    fn duplicate_ids_fail_instead_of_overwriting() {
        let result = CompiledGameDataArtifact::build(
            manifest(),
            vec![skill(1714, "skill.one"), skill(1714, "skill.duplicate")],
            Vec::new(),
            Vec::new(),
        );

        assert!(matches!(
            result,
            Err(GameDataError::DuplicateSymbolId {
                kind: SymbolKind::Skill,
                id: 1714
            })
        ));
    }

    #[test]
    fn tampered_compiled_payload_is_rejected() {
        let mut artifact = CompiledGameDataArtifact::build(
            manifest(),
            vec![skill(1714, "skill.one")],
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        artifact.payload.records[0].id = 9999;

        assert!(matches!(
            artifact.validate(),
            Err(GameDataError::DigestMismatch { .. })
        ));
    }
}
