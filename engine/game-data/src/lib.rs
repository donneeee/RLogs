//! Validated, shared game-data end products.
//!
//! Acquisition and extraction are intentionally outside this crate. Runtime
//! consumers open a sharded bundle and load only the ID/locale buckets they
//! actually touch. Client-build availability is metadata on canonical records;
//! it is not represented by separate regional source trees.

mod sharded;

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use sharded::{
    CachePolicy, CacheStats, CompiledBundleManifest, CompiledShardDescriptor, GameDataStore,
    RecordKey, ShardKind, build_bundle_manifest, encode_json_shard, localization_bucket,
    numeric_id_bucket, stable_key_bucket,
};

pub const GAME_DATA_SCHEMA_VERSION: u16 = 2;
pub const COMPILED_BUNDLE_SCHEMA_VERSION: u16 = 2;
pub const DEFAULT_SHARD_BITS: u8 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GameDataBuild {
    /// Game deployment, such as `global` or `cn`; this is not a player region.
    pub deployment_id: String,
    /// Distribution channel, such as `steam` or an official standalone launcher.
    pub channel: String,
    /// Exact client data build observed while reviewing this definition.
    pub client_build: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameDataManifest {
    pub schema_version: u16,
    pub catalog_id: String,
    pub catalog_revision: String,
    pub supported_builds: Vec<GameDataBuild>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Class,
    Specialization,
    Skill,
    SkillEffect,
    RecountGroup,
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
    WeaponEquipment,
    EquipmentSet,
    Imagine,
    Cosmetic,
    ProfileImage,
    NameCard,
    Medal,
    GuildIcon,
    Profession,
    Talent,
    Module,
    ModuleType,
    ModuleSlot,
    ModuleEffect,
    ModuleLinkEffect,
}

impl SymbolKind {
    pub const ALL: [Self; 34] = [
        Self::Class,
        Self::Specialization,
        Self::Skill,
        Self::SkillEffect,
        Self::RecountGroup,
        Self::StatusEffect,
        Self::Monster,
        Self::Npc,
        Self::Summon,
        Self::Projectile,
        Self::Trap,
        Self::Mechanic,
        Self::EntityType,
        Self::Scene,
        Self::Map,
        Self::Dungeon,
        Self::DungeonObjective,
        Self::Item,
        Self::Equipment,
        Self::WeaponEquipment,
        Self::EquipmentSet,
        Self::Imagine,
        Self::Cosmetic,
        Self::ProfileImage,
        Self::NameCard,
        Self::Medal,
        Self::GuildIcon,
        Self::Profession,
        Self::Talent,
        Self::Module,
        Self::ModuleType,
        Self::ModuleSlot,
        Self::ModuleEffect,
        Self::ModuleLinkEffect,
    ];

    pub const fn folder(self) -> &'static str {
        match self {
            Self::Class => "classes",
            Self::Specialization => "specializations",
            Self::Skill => "skills",
            Self::SkillEffect => "skill-effects",
            Self::RecountGroup => "recount-groups",
            Self::StatusEffect => "status-effects",
            Self::Monster => "monsters",
            Self::Npc => "npcs",
            Self::Summon => "summons",
            Self::Projectile => "projectiles",
            Self::Trap => "traps",
            Self::Mechanic => "mechanics",
            Self::EntityType => "entity-types",
            Self::Scene => "scenes",
            Self::Map => "maps",
            Self::Dungeon => "dungeons",
            Self::DungeonObjective => "dungeon-objectives",
            Self::Item => "items",
            Self::Equipment => "equipment",
            Self::WeaponEquipment => "weapon-equipment",
            Self::EquipmentSet => "equipment-sets",
            Self::Imagine => "imagines",
            Self::Cosmetic => "cosmetics",
            Self::ProfileImage => "profile-images",
            Self::NameCard => "name-cards",
            Self::Medal => "medals",
            Self::GuildIcon => "guild-icons",
            Self::Profession => "professions",
            Self::Talent => "talents",
            Self::Module => "modules",
            Self::ModuleType => "module-types",
            Self::ModuleSlot => "module-slots",
            Self::ModuleEffect => "module-effects",
            Self::ModuleLinkEffect => "module-link-effects",
        }
    }
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
    /// Builds where this exact canonical definition has been reviewed.
    pub availability: Vec<GameDataBuild>,
    pub provenance: SymbolProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizationEntry {
    pub schema_version: u16,
    pub locale: String,
    pub key: String,
    pub text: String,
    /// Builds where this official localized value has been reviewed.
    pub availability: Vec<GameDataBuild>,
    pub provenance: SymbolProvenance,
}

impl GameDataRecord {
    pub fn is_available_in(&self, build: &GameDataBuild) -> bool {
        self.availability.iter().any(|available| available == build)
    }
}

impl LocalizationEntry {
    pub fn is_available_in(&self, build: &GameDataBuild) -> bool {
        self.availability.iter().any(|available| available == build)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRecord {
    pub key: String,
    pub relative_path: String,
    pub media_type: String,
    pub sha256: String,
}

pub fn validate_source_data(
    manifest: &GameDataManifest,
    records: &[GameDataRecord],
    localization: &[LocalizationEntry],
    assets: &[AssetRecord],
) -> Result<(), GameDataError> {
    validate_manifest(manifest)?;
    validate_records(manifest, records)?;
    validate_localization(manifest, localization)?;
    validate_localization_references(records, localization)?;
    validate_public_boundaries(records, localization)?;
    validate_assets(assets)?;
    Ok(())
}

fn validate_manifest(manifest: &GameDataManifest) -> Result<(), GameDataError> {
    if manifest.schema_version != GAME_DATA_SCHEMA_VERSION {
        return Err(GameDataError::UnsupportedSchemaVersion {
            actual: manifest.schema_version,
        });
    }
    for (field, value) in [
        ("catalog_id", manifest.catalog_id.as_str()),
        ("catalog_revision", manifest.catalog_revision.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(GameDataError::EmptyField(field));
        }
    }
    if manifest.supported_builds.is_empty() {
        return Err(GameDataError::EmptyAvailability(
            "manifest.supported_builds",
        ));
    }
    let mut builds = HashSet::with_capacity(manifest.supported_builds.len());
    for build in &manifest.supported_builds {
        validate_build(build)?;
        if !builds.insert(build) {
            return Err(GameDataError::DuplicateBuild {
                deployment_id: build.deployment_id.clone(),
                channel: build.channel.clone(),
                client_build: build.client_build.clone(),
            });
        }
    }
    Ok(())
}

fn validate_records(
    manifest: &GameDataManifest,
    records: &[GameDataRecord],
) -> Result<(), GameDataError> {
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
        validate_availability(
            manifest,
            &record.availability,
            "record.availability",
            &record.stable_key,
        )?;
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

fn validate_localization(
    manifest: &GameDataManifest,
    entries: &[LocalizationEntry],
) -> Result<(), GameDataError> {
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
        validate_availability(
            manifest,
            &entry.availability,
            "localization.availability",
            &entry.key,
        )?;
        if !keys.insert((entry.locale.clone(), entry.key.clone())) {
            return Err(GameDataError::DuplicateLocalization {
                locale: entry.locale.clone(),
                key: entry.key.clone(),
            });
        }
    }
    Ok(())
}

fn validate_localization_references(
    records: &[GameDataRecord],
    entries: &[LocalizationEntry],
) -> Result<(), GameDataError> {
    let mut availability_by_key = BTreeMap::<&str, HashSet<&GameDataBuild>>::new();
    for entry in entries {
        availability_by_key
            .entry(&entry.key)
            .or_default()
            .extend(&entry.availability);
    }
    for record in records {
        let mut references = record
            .localization_key
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        for (name, value) in &record.attributes {
            collect_localization_references(name, value, &mut references);
        }
        for key in references {
            let available = availability_by_key.get(key).ok_or_else(|| {
                GameDataError::MissingLocalizationReference {
                    stable_key: record.stable_key.clone(),
                    localization_key: key.to_owned(),
                }
            })?;
            if let Some(build) = record
                .availability
                .iter()
                .find(|build| !available.contains(build))
            {
                return Err(GameDataError::UnavailableLocalizationReference {
                    stable_key: record.stable_key.clone(),
                    localization_key: key.to_owned(),
                    deployment_id: build.deployment_id.clone(),
                    channel: build.channel.clone(),
                    client_build: build.client_build.clone(),
                });
            }
        }
    }
    Ok(())
}

fn collect_localization_references<'a>(
    name: &str,
    value: &'a serde_json::Value,
    references: &mut Vec<&'a str>,
) {
    if name.ends_with("_localization_key")
        && let Some(key) = value.as_str()
    {
        references.push(key);
    }
    match value {
        serde_json::Value::Object(values) => {
            for (nested_name, nested_value) in values {
                collect_localization_references(nested_name, nested_value, references);
            }
        }
        serde_json::Value::Array(values) => {
            for nested_value in values {
                collect_localization_references(name, nested_value, references);
            }
        }
        _ => {}
    }
}

fn validate_public_boundaries(
    records: &[GameDataRecord],
    localization: &[LocalizationEntry],
) -> Result<(), GameDataError> {
    for record in records {
        validate_provenance(&record.provenance, &record.stable_key)?;
        validate_attribute_keys(&record.attributes, &record.stable_key)?;
    }
    for entry in localization {
        validate_provenance(&entry.provenance, &entry.key)?;
    }
    Ok(())
}

fn validate_provenance(provenance: &SymbolProvenance, key: &str) -> Result<(), GameDataError> {
    for value in [&provenance.source, &provenance.reference] {
        let normalized = value.replace('\\', "/").to_ascii_lowercase();
        let bytes = normalized.as_bytes();
        let has_drive_path = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'/';
        if has_drive_path
            || normalized.starts_with('/')
            || normalized.contains("/.codex_tmp/")
            || normalized.contains("private-game-research")
        {
            return Err(GameDataError::PrivateProvenance(key.to_owned()));
        }
    }
    Ok(())
}

fn validate_attribute_keys(
    attributes: &BTreeMap<String, serde_json::Value>,
    stable_key: &str,
) -> Result<(), GameDataError> {
    fn visit(name: &str, value: &serde_json::Value, stable_key: &str) -> Result<(), GameDataError> {
        let normalized = name.to_ascii_lowercase().replace(['-', '.', ' '], "_");
        const PROHIBITED: [&str; 11] = [
            "password",
            "account",
            "credential",
            "login",
            "openid",
            "open_id",
            "auth_token",
            "access_token",
            "refresh_token",
            "session_token",
            "region_id",
        ];
        if PROHIBITED
            .iter()
            .any(|prohibited| normalized.contains(prohibited))
        {
            return Err(GameDataError::ProhibitedAttribute {
                stable_key: stable_key.to_owned(),
                attribute: name.to_owned(),
            });
        }
        match value {
            serde_json::Value::Object(values) => {
                for (nested_name, nested_value) in values {
                    visit(nested_name, nested_value, stable_key)?;
                }
            }
            serde_json::Value::Array(values) => {
                for nested_value in values {
                    if let serde_json::Value::Object(object) = nested_value {
                        for (nested_name, nested_value) in object {
                            visit(nested_name, nested_value, stable_key)?;
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    for (name, value) in attributes {
        visit(name, value, stable_key)?;
    }
    Ok(())
}

fn validate_build(build: &GameDataBuild) -> Result<(), GameDataError> {
    for (field, value) in [
        ("build.deployment_id", build.deployment_id.as_str()),
        ("build.channel", build.channel.as_str()),
        ("build.client_build", build.client_build.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(GameDataError::EmptyField(field));
        }
    }
    Ok(())
}

fn validate_availability(
    manifest: &GameDataManifest,
    availability: &[GameDataBuild],
    field: &'static str,
    key: &str,
) -> Result<(), GameDataError> {
    if availability.is_empty() {
        return Err(GameDataError::EmptyAvailability(field));
    }
    let supported = manifest.supported_builds.iter().collect::<HashSet<_>>();
    let mut seen = HashSet::with_capacity(availability.len());
    for build in availability {
        validate_build(build)?;
        if !supported.contains(build) {
            return Err(GameDataError::UnsupportedBuildAvailability {
                key: key.to_owned(),
                deployment_id: build.deployment_id.clone(),
                channel: build.channel.clone(),
                client_build: build.client_build.clone(),
            });
        }
        if !seen.insert(build) {
            return Err(GameDataError::DuplicateBuildAvailability(key.to_owned()));
        }
    }
    Ok(())
}

fn validate_assets(assets: &[AssetRecord]) -> Result<(), GameDataError> {
    let mut keys = HashSet::with_capacity(assets.len());
    for asset in assets {
        if !keys.insert(asset.key.clone()) {
            return Err(GameDataError::DuplicateAssetKey(asset.key.clone()));
        }
        if !asset.sha256.starts_with("sha256:") {
            return Err(GameDataError::InvalidAssetDigest(asset.key.clone()));
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum GameDataError {
    #[error("unsupported game-data schema version {actual}")]
    UnsupportedSchemaVersion { actual: u16 },

    #[error("game-data field {0} must not be empty")]
    EmptyField(&'static str),

    #[error("game-data availability field {0} must not be empty")]
    EmptyAvailability(&'static str),

    #[error("duplicate supported build {deployment_id}/{channel}/{client_build}")]
    DuplicateBuild {
        deployment_id: String,
        channel: String,
        client_build: String,
    },

    #[error("{key} references unsupported build {deployment_id}/{channel}/{client_build}")]
    UnsupportedBuildAvailability {
        key: String,
        deployment_id: String,
        channel: String,
        client_build: String,
    },

    #[error("{0} repeats a build in its availability metadata")]
    DuplicateBuildAvailability(String),

    #[error("{stable_key} references missing localization key {localization_key}")]
    MissingLocalizationReference {
        stable_key: String,
        localization_key: String,
    },

    #[error(
        "{stable_key} localization {localization_key} is unavailable for {deployment_id}/{channel}/{client_build}"
    )]
    UnavailableLocalizationReference {
        stable_key: String,
        localization_key: String,
        deployment_id: String,
        channel: String,
        client_build: String,
    },

    #[error("{0} contains a private acquisition path in public provenance")]
    PrivateProvenance(String),

    #[error("{stable_key} contains prohibited public attribute {attribute}")]
    ProhibitedAttribute {
        stable_key: String,
        attribute: String,
    },

    #[error("duplicate {kind:?} id {id}")]
    DuplicateSymbolId { kind: SymbolKind, id: i64 },

    #[error("duplicate stable key {0}")]
    DuplicateStableKey(String),

    #[error("duplicate localization key {key} for locale {locale}")]
    DuplicateLocalization { locale: String, key: String },

    #[error("duplicate asset key {0}")]
    DuplicateAssetKey(String),

    #[error("asset {0} does not have a sha256 digest")]
    InvalidAssetDigest(String),

    #[error("compiled game-data manifest is invalid: {0}")]
    InvalidCompiledManifest(String),

    #[error("compiled game-data shard is invalid: {0}")]
    InvalidShard(String),

    #[error("game-data path is unsafe: {0}")]
    UnsafePath(String),

    #[error("game-data I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("game-data serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("game-data cache lock was poisoned")]
    CachePoisoned,
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
            catalog_id: "rlogs-official".into(),
            catalog_revision: "review-1".into(),
            supported_builds: vec![build()],
        }
    }

    fn build() -> GameDataBuild {
        GameDataBuild {
            deployment_id: "global".into(),
            channel: "steam".into(),
            client_build: "example-build".into(),
        }
    }

    fn skill(id: i64, key: &str) -> GameDataRecord {
        GameDataRecord {
            schema_version: GAME_DATA_SCHEMA_VERSION,
            kind: SymbolKind::Skill,
            id,
            stable_key: key.into(),
            localization_key: Some(format!("game.skill.{id}")),
            icon: None,
            attributes: BTreeMap::new(),
            availability: vec![build()],
            provenance: provenance(),
        }
    }

    #[test]
    fn source_validation_rejects_duplicate_ids() {
        let result = validate_source_data(
            &manifest(),
            &[skill(1714, "skill.one"), skill(1714, "skill.duplicate")],
            &[],
            &[],
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
    fn symbol_folders_are_stable_and_human_readable() {
        assert_eq!(SymbolKind::Skill.folder(), "skills");
        assert_eq!(SymbolKind::RecountGroup.folder(), "recount-groups");
        assert_eq!(SymbolKind::StatusEffect.folder(), "status-effects");
        assert_eq!(SymbolKind::DungeonObjective.folder(), "dungeon-objectives");
        assert_eq!(SymbolKind::WeaponEquipment.folder(), "weapon-equipment");
        assert_eq!(SymbolKind::ProfileImage.folder(), "profile-images");
        assert_eq!(SymbolKind::NameCard.folder(), "name-cards");
        assert_eq!(SymbolKind::Medal.folder(), "medals");
        assert_eq!(SymbolKind::GuildIcon.folder(), "guild-icons");
        assert_eq!(SymbolKind::Module.folder(), "modules");
        assert_eq!(SymbolKind::ModuleEffect.folder(), "module-effects");
    }

    #[test]
    fn source_validation_rejects_unlisted_build_availability() {
        let mut record = skill(1714, "skill.one");
        record.availability[0].client_build = "different-build".into();
        let result = validate_source_data(&manifest(), &[record], &[], &[]);
        assert!(matches!(
            result,
            Err(GameDataError::UnsupportedBuildAvailability { .. })
        ));
    }

    #[test]
    fn reviewed_f32_values_survive_json_round_trips_exactly() {
        let value = serde_json::json!(f32::from_bits(0x1450_a985));
        let encoded = serde_json::to_vec(&value).unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn source_validation_rejects_account_fields() {
        let mut record = skill(1714, "skill.one");
        record.localization_key = None;
        record
            .attributes
            .insert("account_id".into(), serde_json::json!("not-public"));
        let result = validate_source_data(&manifest(), &[record], &[], &[]);
        assert!(matches!(
            result,
            Err(GameDataError::ProhibitedAttribute { .. })
        ));
    }

    #[test]
    fn source_validation_checks_nested_localization_references() {
        let mut record = skill(1714, "skill.one");
        record.localization_key = None;
        record.attributes.insert(
            "levels".into(),
            serde_json::json!([{
                "overview_localization_key": "module-effect.1110.level.1.overview"
            }]),
        );
        let result = validate_source_data(&manifest(), &[record], &[], &[]);
        assert!(matches!(
            result,
            Err(GameDataError::MissingLocalizationReference { .. })
        ));
    }
}
