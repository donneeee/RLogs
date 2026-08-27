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
    OverworldScene,
    WorldArea,
    WorldObject,
    WorldEvent,
    MapSticker,
    SubScene,
    ActivityTarget,
    SceneEvent,
    WorldActivity,
    Dungeon,
    DungeonSeason,
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
    pub const ALL: [Self; 44] = [
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
        Self::OverworldScene,
        Self::WorldArea,
        Self::WorldObject,
        Self::WorldEvent,
        Self::MapSticker,
        Self::SubScene,
        Self::ActivityTarget,
        Self::SceneEvent,
        Self::WorldActivity,
        Self::Dungeon,
        Self::DungeonSeason,
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
            Self::OverworldScene => "overworld-scenes",
            Self::WorldArea => "world-areas",
            Self::WorldObject => "world-objects",
            Self::WorldEvent => "world-events",
            Self::MapSticker => "map-stickers",
            Self::SubScene => "subscenes",
            Self::ActivityTarget => "activity-targets",
            Self::SceneEvent => "scene-events",
            Self::WorldActivity => "world-activities",
            Self::Dungeon => "dungeons",
            Self::DungeonSeason => "dungeon-seasons",
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
    validate_dungeon_seasons(records)?;
    validate_overworld_catalog(records)?;
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

fn validate_dungeon_seasons(records: &[GameDataRecord]) -> Result<(), GameDataError> {
    let dungeons = records
        .iter()
        .filter(|record| record.kind == SymbolKind::Dungeon)
        .map(|record| (record.id, record))
        .collect::<BTreeMap<_, _>>();

    for season in records
        .iter()
        .filter(|record| record.kind == SymbolKind::DungeonSeason)
    {
        let invalid = |reason: &str| GameDataError::InvalidRecordSchema {
            stable_key: season.stable_key.clone(),
            reason: reason.to_owned(),
        };
        if season
            .attributes
            .get("season_id")
            .and_then(|value| value.as_i64())
            != Some(season.id)
        {
            return Err(invalid("season_id must equal the record ID"));
        }
        let families = season
            .attributes
            .get("activity_families")
            .and_then(|value| value.as_array())
            .filter(|families| !families.is_empty())
            .ok_or_else(|| invalid("activity_families must be a non-empty array"))?;
        let mut family_names = HashSet::with_capacity(families.len());
        for family in families {
            let family = family
                .as_object()
                .ok_or_else(|| invalid("every activity family must be an object"))?;
            let family_name = family
                .get("family")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| invalid("every activity family needs a name"))?;
            if !family_names.insert(family_name) {
                return Err(invalid("activity family names must be unique"));
            }

            let tier_count = if let Some(identity) = family.get("tier_identity") {
                let identity = identity
                    .as_object()
                    .ok_or_else(|| invalid("tier_identity must be an object"))?;
                let minimum = identity
                    .get("minimum")
                    .and_then(|value| value.as_i64())
                    .ok_or_else(|| invalid("tier_identity.minimum must be an integer"))?;
                let maximum = identity
                    .get("maximum")
                    .and_then(|value| value.as_i64())
                    .ok_or_else(|| invalid("tier_identity.maximum must be an integer"))?;
                let count = identity
                    .get("count")
                    .and_then(|value| value.as_i64())
                    .ok_or_else(|| invalid("tier_identity.count must be an integer"))?;
                if minimum <= 0 || maximum < minimum || count != maximum - minimum + 1 {
                    return Err(invalid("tier_identity range and count do not agree"));
                }
                Some(count)
            } else {
                None
            };

            let activities = family
                .get("activities")
                .and_then(|value| value.as_array())
                .filter(|activities| !activities.is_empty())
                .ok_or_else(|| invalid("each activity family needs activities"))?;
            let mut activity_ids = HashSet::with_capacity(activities.len());
            for activity in activities {
                let activity = activity
                    .as_object()
                    .ok_or_else(|| invalid("every season activity must be an object"))?;
                let dungeon_id = activity
                    .get("dungeon_id")
                    .and_then(|value| value.as_i64())
                    .ok_or_else(|| invalid("season activity dungeon_id must be an integer"))?;
                if !activity_ids.insert(dungeon_id) {
                    return Err(invalid(
                        "dungeon IDs must be unique inside an activity family",
                    ));
                }
                let dungeon_key = activity
                    .get("dungeon_key")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| invalid("season activity needs a dungeon_key"))?;
                let dungeon = dungeons
                    .get(&dungeon_id)
                    .ok_or_else(|| invalid("season activity references a missing dungeon ID"))?;
                if dungeon.stable_key != dungeon_key {
                    return Err(invalid(
                        "season activity dungeon_key does not match the canonical dungeon",
                    ));
                }
                if season
                    .availability
                    .iter()
                    .any(|build| !dungeon.availability.contains(build))
                {
                    return Err(invalid(
                        "season activity references a dungeon unavailable for the season build",
                    ));
                }
                if let Some(count) = tier_count {
                    let first = activity
                        .get("first_tier_row_id")
                        .and_then(|value| value.as_i64())
                        .ok_or_else(|| invalid("tiered activity needs first_tier_row_id"))?;
                    let last = activity
                        .get("last_tier_row_id")
                        .and_then(|value| value.as_i64())
                        .ok_or_else(|| invalid("tiered activity needs last_tier_row_id"))?;
                    if last - first + 1 != count {
                        return Err(invalid(
                            "tier row bounds must contain exactly tier_identity.count rows",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_overworld_catalog(records: &[GameDataRecord]) -> Result<(), GameDataError> {
    let by_key = records
        .iter()
        .map(|record| (record.stable_key.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let by_kind_id = records
        .iter()
        .map(|record| ((record.kind, record.id), record))
        .collect::<BTreeMap<_, _>>();

    let invalid = |record: &GameDataRecord, reason: &str| GameDataError::InvalidRecordSchema {
        stable_key: record.stable_key.clone(),
        reason: reason.to_owned(),
    };
    let referenced_record =
        |record: &GameDataRecord, key: &str, kind: SymbolKind, relation: &str| {
            by_key
                .get(key)
                .copied()
                .filter(|target| target.kind == kind)
                .ok_or_else(|| {
                    invalid(
                        record,
                        &format!("{relation} references missing {kind:?} key {key}"),
                    )
                })
        };

    for area in records
        .iter()
        .filter(|record| record.kind == SymbolKind::WorldArea)
    {
        let area_id = required_i64_attribute(area, "area_id")?;
        if area_id != area.id || area.stable_key != format!("world-area.{}", area.id) {
            return Err(invalid(area, "area identity does not match the record ID"));
        }
        let scene_id = required_i64_attribute(area, "scene_id")?;
        let scene_key = required_str_attribute(area, "scene_key")?;
        let scene = referenced_record(area, scene_key, SymbolKind::OverworldScene, "scene_key")?;
        if scene.id != scene_id {
            return Err(invalid(area, "scene_id and scene_key do not agree"));
        }
    }

    for object in records
        .iter()
        .filter(|record| record.kind == SymbolKind::WorldObject)
    {
        if required_i64_attribute(object, "world_object_id")? != object.id
            || object.stable_key != format!("world-object.{}", object.id)
        {
            return Err(invalid(
                object,
                "world-object identity does not match the record ID",
            ));
        }
    }

    for scene in records
        .iter()
        .filter(|record| record.kind == SymbolKind::OverworldScene)
    {
        let scene_id = required_i64_attribute(scene, "scene_id")?;
        if scene_id != scene.id || scene.stable_key != format!("overworld-scene.{}", scene.id) {
            return Err(invalid(
                scene,
                "scene identity does not match the record ID",
            ));
        }
        let scene_key = required_str_attribute(scene, "scene_key")?;
        let canonical_scene = referenced_record(scene, scene_key, SymbolKind::Scene, "scene_key")?;
        if canonical_scene.id != scene_id {
            return Err(invalid(
                scene,
                "scene_id and canonical scene_key do not agree",
            ));
        }
        let map_key = required_str_attribute(scene, "map_key")?;
        let map = referenced_record(scene, map_key, SymbolKind::Map, "map_key")?;
        if map.id != scene_id {
            return Err(invalid(scene, "scene_id and map_key do not agree"));
        }

        let area_ids = required_array_attribute(scene, "area_ids")?;
        let mut seen_area_ids = HashSet::with_capacity(area_ids.len());
        for value in area_ids {
            let area_id = value
                .as_i64()
                .ok_or_else(|| invalid(scene, "area_ids must contain only integers"))?;
            if !seen_area_ids.insert(area_id) {
                return Err(invalid(scene, "area_ids must be unique"));
            }
            let area = by_kind_id
                .get(&(SymbolKind::WorldArea, area_id))
                .copied()
                .ok_or_else(|| invalid(scene, "area_ids references a missing world area"))?;
            if required_i64_attribute(area, "scene_id")? != scene_id {
                return Err(invalid(
                    scene,
                    "area_ids includes an area owned by another scene",
                ));
            }
        }

        for transition in required_array_attribute(scene, "transitions")? {
            let transition = transition
                .as_object()
                .ok_or_else(|| invalid(scene, "transitions must contain objects"))?;
            let target_id = transition
                .get("target_scene_id")
                .and_then(|value| value.as_i64())
                .ok_or_else(|| invalid(scene, "transition target_scene_id must be an integer"))?;
            match transition.get("target_scene_key") {
                Some(serde_json::Value::String(target_key)) => {
                    let target = referenced_record(
                        scene,
                        target_key,
                        SymbolKind::Scene,
                        "transition target_scene_key",
                    )?;
                    if target.id != target_id {
                        return Err(invalid(
                            scene,
                            "transition target_scene_id and target_scene_key do not agree",
                        ));
                    }
                }
                Some(serde_json::Value::Null) => {
                    if transition
                        .get("target_resolution")
                        .and_then(|value| value.as_str())
                        != Some("unresolved_current_table_reference")
                    {
                        return Err(invalid(
                            scene,
                            "null transition target keys must remain explicitly unresolved",
                        ));
                    }
                }
                _ => {
                    return Err(invalid(
                        scene,
                        "transition target_scene_key must be a scene key or null",
                    ));
                }
            }
        }

        validate_scene_owned_ids(
            scene,
            scene_id,
            required_array_attribute(scene, "world_points")?,
            "point_id",
        )?;
        let placements = scene
            .attributes
            .get("placements")
            .and_then(|value| value.as_object())
            .ok_or_else(|| invalid(scene, "placements must be an object"))?;
        for domain in ["monsters", "npcs", "world_objects", "zones"] {
            let rows = placements
                .get(domain)
                .and_then(|value| value.as_array())
                .ok_or_else(|| invalid(scene, "every placement domain must be an array"))?;
            validate_scene_owned_ids(scene, scene_id, rows, "entity_definition_id")?;
            for row in rows {
                let row = row
                    .as_object()
                    .ok_or_else(|| invalid(scene, "placement rows must be objects"))?;
                let definition = match domain {
                    "monsters" => Some((
                        "monster_id",
                        "monster_key",
                        SymbolKind::Monster,
                        "monster placement",
                    )),
                    "npcs" => Some(("npc_id", "npc_key", SymbolKind::Npc, "NPC placement")),
                    "world_objects" => Some((
                        "world_object_id",
                        "world_object_key",
                        SymbolKind::WorldObject,
                        "world-object placement",
                    )),
                    _ => None,
                };
                if let Some((id_field, key_field, kind, relation)) = definition {
                    let definition_id = row
                        .get(id_field)
                        .and_then(|value| value.as_i64())
                        .ok_or_else(|| {
                            invalid(scene, "placement definition ID must be an integer")
                        })?;
                    let definition_key = row
                        .get(key_field)
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| {
                            invalid(scene, "placement definition key must be a string")
                        })?;
                    let target = referenced_record(scene, definition_key, kind, relation)?;
                    if target.id != definition_id {
                        return Err(invalid(
                            scene,
                            "placement definition ID and key do not agree",
                        ));
                    }
                }
            }
        }
    }

    for event in records
        .iter()
        .filter(|record| record.kind == SymbolKind::WorldEvent)
    {
        if required_i64_attribute(event, "event_id")? != event.id {
            return Err(invalid(event, "event_id must equal the record ID"));
        }
        let event_kind = required_str_attribute(event, "event_kind")?;
        if event.stable_key != format!("world-event.{event_kind}.{}", event.id) {
            return Err(invalid(
                event,
                "event stable key does not match its kind and ID",
            ));
        }
        if let Some(scene_id) = event
            .attributes
            .get("scene_id")
            .and_then(|value| value.as_i64())
        {
            let scene_key = required_str_attribute(event, "scene_key")?;
            let scene =
                referenced_record(event, scene_key, SymbolKind::OverworldScene, "scene_key")?;
            if scene.id != scene_id {
                return Err(invalid(event, "scene_id and scene_key do not agree"));
            }
        }
        if let Some(dungeon_id) = event
            .attributes
            .get("dungeon_id")
            .and_then(|value| value.as_i64())
            .filter(|id| *id > 0)
            && !by_kind_id.contains_key(&(SymbolKind::Dungeon, dungeon_id))
        {
            return Err(invalid(event, "dungeon_id references a missing dungeon"));
        }
    }

    for sticker in records
        .iter()
        .filter(|record| record.kind == SymbolKind::MapSticker)
    {
        if required_i64_attribute(sticker, "map_sticker_id")? != sticker.id
            || sticker.stable_key != format!("map-sticker.{}", sticker.id)
        {
            return Err(invalid(
                sticker,
                "map-sticker identity does not match the record ID",
            ));
        }
        let scene_ids = required_array_attribute(sticker, "scene_ids")?;
        let scene_keys = required_array_attribute(sticker, "scene_keys")?;
        if scene_ids.len() != scene_keys.len() {
            return Err(invalid(
                sticker,
                "scene_ids and scene_keys must have matching lengths",
            ));
        }
        for (scene_id, scene_key) in scene_ids.iter().zip(scene_keys) {
            let scene_id = scene_id
                .as_i64()
                .ok_or_else(|| invalid(sticker, "scene_ids must contain integers"))?;
            let scene_key = scene_key
                .as_str()
                .ok_or_else(|| invalid(sticker, "scene_keys must contain strings"))?;
            let scene = referenced_record(sticker, scene_key, SymbolKind::Scene, "scene_keys")?;
            if scene.id != scene_id {
                return Err(invalid(sticker, "scene_ids and scene_keys do not agree"));
            }
        }
        validate_map_sticker_tasks(sticker, &by_kind_id)?;
    }

    for subscene in records
        .iter()
        .filter(|record| record.kind == SymbolKind::SubScene)
    {
        if required_i64_attribute(subscene, "subscene_id")? != subscene.id
            || subscene.stable_key != format!("subscene.{}", subscene.id)
        {
            return Err(invalid(
                subscene,
                "subscene identity does not match the record ID",
            ));
        }
        required_i64_attribute(subscene, "subscene_type_id")?;
        required_str_attribute(subscene, "resource_path")?;
        validate_scene_reference_arrays(subscene, "owner_scene_ids", "owner_scene_keys", &by_key)?;
        validate_scene_reference_arrays(
            subscene,
            "resource_scene_ids",
            "resource_scene_keys",
            &by_key,
        )?;
        validate_nested_integer_array(subscene, "activation_conditions")?;
    }

    for target in records
        .iter()
        .filter(|record| record.kind == SymbolKind::ActivityTarget)
    {
        if required_i64_attribute(target, "activity_target_id")? != target.id
            || target.stable_key != format!("activity-target.{}", target.id)
        {
            return Err(invalid(
                target,
                "activity-target identity does not match the record ID",
            ));
        }
        required_i64_attribute(target, "target_type_id")?;
        required_i64_attribute(target, "required_count")?;
        required_i64_attribute(target, "description_id")?;
        validate_integer_array(target, "parameters")?;
        validate_nested_integer_array(target, "target_positions")?;
        validate_nested_string_array(target, "special_variables")?;
        validate_nested_string_array(target, "special_variable_limits")?;
        validate_string_array(target, "special_variable_names")?;
        validate_integer_array(target, "show_special_variable_progress")?;
        validate_string_array(target, "show_change")?;
        validate_string_array(target, "show_color")?;
        validate_kind_reference_arrays(
            target,
            "scene_event_ids",
            "scene_event_keys",
            SymbolKind::SceneEvent,
            &by_key,
        )?;
        for scene_event_key in required_array_attribute(target, "scene_event_keys")? {
            let scene_event_key = scene_event_key
                .as_str()
                .ok_or_else(|| invalid(target, "scene_event_keys must contain strings"))?;
            let scene_event = by_key.get(scene_event_key).copied().ok_or_else(|| {
                invalid(target, "scene_event_keys references a missing scene event")
            })?;
            let has_back_reference = required_array_attribute(scene_event, "targets")?
                .iter()
                .filter_map(serde_json::Value::as_object)
                .any(|reference| {
                    reference
                        .get("target_id")
                        .and_then(serde_json::Value::as_i64)
                        == Some(target.id)
                });
            if !has_back_reference {
                return Err(invalid(
                    target,
                    "scene-event back-reference does not contain this activity target",
                ));
            }
        }
        for field in ["team_shared", "show_progress"] {
            if !target
                .attributes
                .get(field)
                .is_some_and(serde_json::Value::is_boolean)
            {
                return Err(invalid(target, &format!("{field} must be a boolean")));
            }
        }

        let scene_id = required_i64_attribute(target, "scene_id")?;
        let scene_resolution = required_str_attribute(target, "scene_resolution")?;
        match target.attributes.get("scene_key") {
            Some(serde_json::Value::String(scene_key)) => {
                let scene = referenced_record(target, scene_key, SymbolKind::Scene, "scene_key")?;
                if scene.id != scene_id || scene_resolution != "current_scene" {
                    return Err(invalid(
                        target,
                        "resolved activity-target scene fields do not agree",
                    ));
                }
            }
            Some(serde_json::Value::Null) => {
                let expected_resolution = if scene_id == 0 {
                    "global"
                } else {
                    "unresolved_current_table_reference"
                };
                if scene_resolution != expected_resolution {
                    return Err(invalid(
                        target,
                        "null activity-target scene keys need an explicit resolution",
                    ));
                }
            }
            _ => {
                return Err(invalid(
                    target,
                    "scene_key must be a canonical scene key or null",
                ));
            }
        }
    }

    for event in records
        .iter()
        .filter(|record| record.kind == SymbolKind::SceneEvent)
    {
        if required_i64_attribute(event, "scene_event_id")? != event.id
            || event.stable_key != format!("scene-event.{}", event.id)
        {
            return Err(invalid(
                event,
                "scene-event identity does not match the record ID",
            ));
        }
        required_i64_attribute(event, "name_id")?;
        let difficulty = required_i64_attribute(event, "difficulty_flag")?;
        if !matches!(difficulty, 0 | 1) {
            return Err(invalid(event, "difficulty_flag must be 0 or 1"));
        }
        let time_limit = required_i64_attribute(event, "time_limit_seconds")?;
        if time_limit < 0 {
            return Err(invalid(event, "time_limit_seconds cannot be negative"));
        }
        validate_nested_integer_array(event, "completion_actions")?;
        validate_scene_reference_arrays(event, "scene_ids", "scene_keys", &by_key)?;

        for target in required_array_attribute(event, "targets")? {
            let target = target
                .as_object()
                .ok_or_else(|| invalid(event, "targets must contain objects"))?;
            let target_id = target
                .get("target_id")
                .and_then(|value| value.as_i64())
                .ok_or_else(|| invalid(event, "target_id must be an integer"))?;
            let resolution = target
                .get("target_resolution")
                .and_then(|value| value.as_str())
                .ok_or_else(|| invalid(event, "target_resolution must be a string"))?;
            match target.get("target_key") {
                Some(serde_json::Value::String(target_key)) => {
                    let referenced = referenced_record(
                        event,
                        target_key,
                        SymbolKind::ActivityTarget,
                        "target_key",
                    )?;
                    if referenced.id != target_id || resolution != "current_activity_target" {
                        return Err(invalid(
                            event,
                            "resolved scene-event target fields do not agree",
                        ));
                    }
                }
                Some(serde_json::Value::Null)
                    if resolution == "unresolved_current_table_reference" => {}
                Some(serde_json::Value::Null) => {
                    return Err(invalid(
                        event,
                        "null scene-event target keys must remain explicitly unresolved",
                    ));
                }
                _ => {
                    return Err(invalid(
                        event,
                        "target_key must be an activity-target key or null",
                    ));
                }
            }
        }
    }

    for activity in records
        .iter()
        .filter(|record| record.kind == SymbolKind::WorldActivity)
    {
        if required_i64_attribute(activity, "world_activity_id")? != activity.id
            || activity.stable_key != format!("world-activity.{}", activity.id)
        {
            return Err(invalid(
                activity,
                "world-activity identity does not match the record ID",
            ));
        }
        if required_str_attribute(activity, "review_state")? != "identity_only" {
            return Err(invalid(
                activity,
                "world-activity fields must stay identity-only until reviewed",
            ));
        }
        required_str_attribute(activity, "withheld_field_reason")?;
    }

    Ok(())
}

fn required_i64_attribute(record: &GameDataRecord, name: &str) -> Result<i64, GameDataError> {
    record
        .attributes
        .get(name)
        .and_then(|value| value.as_i64())
        .ok_or_else(|| GameDataError::InvalidRecordSchema {
            stable_key: record.stable_key.clone(),
            reason: format!("{name} must be an integer"),
        })
}

fn required_str_attribute<'a>(
    record: &'a GameDataRecord,
    name: &str,
) -> Result<&'a str, GameDataError> {
    record
        .attributes
        .get(name)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| GameDataError::InvalidRecordSchema {
            stable_key: record.stable_key.clone(),
            reason: format!("{name} must be a non-empty string"),
        })
}

fn required_array_attribute<'a>(
    record: &'a GameDataRecord,
    name: &str,
) -> Result<&'a Vec<serde_json::Value>, GameDataError> {
    record
        .attributes
        .get(name)
        .and_then(|value| value.as_array())
        .ok_or_else(|| GameDataError::InvalidRecordSchema {
            stable_key: record.stable_key.clone(),
            reason: format!("{name} must be an array"),
        })
}

fn validate_scene_owned_ids(
    scene: &GameDataRecord,
    scene_id: i64,
    rows: &[serde_json::Value],
    id_field: &str,
) -> Result<(), GameDataError> {
    for row in rows {
        let owned_id = row
            .as_object()
            .and_then(|row| row.get(id_field))
            .and_then(|value| value.as_i64())
            .ok_or_else(|| GameDataError::InvalidRecordSchema {
                stable_key: scene.stable_key.clone(),
                reason: format!("{id_field} must be an integer"),
            })?;
        if owned_id <= 0 || owned_id / 10_000_000 != scene_id {
            return Err(GameDataError::InvalidRecordSchema {
                stable_key: scene.stable_key.clone(),
                reason: format!("{id_field} does not encode the owning scene"),
            });
        }
    }
    Ok(())
}

fn validate_map_sticker_tasks(
    sticker: &GameDataRecord,
    by_kind_id: &BTreeMap<(SymbolKind, i64), &GameDataRecord>,
) -> Result<(), GameDataError> {
    let invalid = |reason: &str| GameDataError::InvalidRecordSchema {
        stable_key: sticker.stable_key.clone(),
        reason: reason.to_owned(),
    };
    let task_ids = required_array_attribute(sticker, "task_ids")?;
    let tasks = required_array_attribute(sticker, "tasks")?;
    if task_ids.len() != tasks.len() {
        return Err(invalid("task_ids and tasks must have matching lengths"));
    }
    for (task_id, task) in task_ids.iter().zip(tasks) {
        let task_id = task_id
            .as_i64()
            .ok_or_else(|| invalid("task_ids must contain integers"))?;
        let task = task
            .as_object()
            .ok_or_else(|| invalid("tasks must contain objects"))?;
        if task.get("task_id").and_then(|value| value.as_i64()) != Some(task_id) {
            return Err(invalid("task_ids and task records do not agree"));
        }
        let target_ids = task
            .get("target_ids")
            .and_then(|value| value.as_array())
            .ok_or_else(|| invalid("task target_ids must be an array"))?;
        let targets = task
            .get("targets")
            .and_then(|value| value.as_array())
            .ok_or_else(|| invalid("task targets must be an array"))?;
        if target_ids.len() != targets.len() {
            return Err(invalid(
                "task target_ids and targets must have matching lengths",
            ));
        }
        for (target_id, target) in target_ids.iter().zip(targets) {
            let target_id = target_id
                .as_i64()
                .ok_or_else(|| invalid("target_ids must contain integers"))?;
            let target = target
                .as_object()
                .ok_or_else(|| invalid("targets must contain objects"))?;
            if target.get("target_id").and_then(|value| value.as_i64()) != Some(target_id) {
                return Err(invalid("target_ids and target records do not agree"));
            }
            if let Some(scene_id) = target
                .get("scene_id")
                .and_then(|value| value.as_i64())
                .filter(|id| *id > 0)
                && !by_kind_id.contains_key(&(SymbolKind::Scene, scene_id))
            {
                return Err(invalid("map-sticker target references a missing scene"));
            }
        }
    }
    Ok(())
}

fn validate_scene_reference_arrays(
    record: &GameDataRecord,
    ids_field: &str,
    keys_field: &str,
    by_key: &BTreeMap<&str, &GameDataRecord>,
) -> Result<(), GameDataError> {
    validate_kind_reference_arrays(record, ids_field, keys_field, SymbolKind::Scene, by_key)
}

fn validate_kind_reference_arrays(
    record: &GameDataRecord,
    ids_field: &str,
    keys_field: &str,
    target_kind: SymbolKind,
    by_key: &BTreeMap<&str, &GameDataRecord>,
) -> Result<(), GameDataError> {
    let invalid = |reason: &str| GameDataError::InvalidRecordSchema {
        stable_key: record.stable_key.clone(),
        reason: reason.to_owned(),
    };
    let ids = required_array_attribute(record, ids_field)?;
    let keys = required_array_attribute(record, keys_field)?;
    if ids.len() != keys.len() {
        return Err(invalid(&format!(
            "{ids_field} and {keys_field} must have matching lengths"
        )));
    }
    let mut seen_ids = HashSet::with_capacity(ids.len());
    for (id, key) in ids.iter().zip(keys) {
        let id = id
            .as_i64()
            .ok_or_else(|| invalid(&format!("{ids_field} must contain integers")))?;
        if !seen_ids.insert(id) {
            return Err(invalid(&format!("{ids_field} must be unique")));
        }
        let key = key
            .as_str()
            .ok_or_else(|| invalid(&format!("{keys_field} must contain strings")))?;
        let scene = by_key
            .get(key)
            .copied()
            .filter(|target| target.kind == target_kind)
            .ok_or_else(|| {
                invalid(&format!(
                    "{keys_field} references missing {target_kind:?} key {key}"
                ))
            })?;
        if scene.id != id {
            return Err(invalid(&format!(
                "{ids_field} and {keys_field} do not agree"
            )));
        }
    }
    Ok(())
}

fn validate_integer_array(record: &GameDataRecord, field: &str) -> Result<(), GameDataError> {
    if required_array_attribute(record, field)?
        .iter()
        .any(|value| !value.is_i64() && !value.is_u64())
    {
        return Err(GameDataError::InvalidRecordSchema {
            stable_key: record.stable_key.clone(),
            reason: format!("{field} must contain only integers"),
        });
    }
    Ok(())
}

fn validate_nested_integer_array(
    record: &GameDataRecord,
    field: &str,
) -> Result<(), GameDataError> {
    let invalid = || GameDataError::InvalidRecordSchema {
        stable_key: record.stable_key.clone(),
        reason: format!("{field} must contain only integer arrays"),
    };
    for values in required_array_attribute(record, field)? {
        let values = values.as_array().ok_or_else(&invalid)?;
        if values
            .iter()
            .any(|value| !value.is_i64() && !value.is_u64())
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn validate_string_array(record: &GameDataRecord, field: &str) -> Result<(), GameDataError> {
    if required_array_attribute(record, field)?
        .iter()
        .any(|value| !value.is_string())
    {
        return Err(GameDataError::InvalidRecordSchema {
            stable_key: record.stable_key.clone(),
            reason: format!("{field} must contain only strings"),
        });
    }
    Ok(())
}

fn validate_nested_string_array(record: &GameDataRecord, field: &str) -> Result<(), GameDataError> {
    let invalid = || GameDataError::InvalidRecordSchema {
        stable_key: record.stable_key.clone(),
        reason: format!("{field} must contain only string arrays"),
    };
    for values in required_array_attribute(record, field)? {
        let values = values.as_array().ok_or_else(&invalid)?;
        if values.iter().any(|value| !value.is_string()) {
            return Err(invalid());
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

    #[error("{stable_key} has an invalid record schema: {reason}")]
    InvalidRecordSchema { stable_key: String, reason: String },

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

    fn dungeon(id: i64) -> GameDataRecord {
        GameDataRecord {
            schema_version: GAME_DATA_SCHEMA_VERSION,
            kind: SymbolKind::Dungeon,
            id,
            stable_key: format!("dungeon.{id}"),
            localization_key: None,
            icon: None,
            attributes: BTreeMap::new(),
            availability: vec![build()],
            provenance: provenance(),
        }
    }

    fn dungeon_season(dungeon_id: i64, dungeon_key: &str) -> GameDataRecord {
        GameDataRecord {
            schema_version: GAME_DATA_SCHEMA_VERSION,
            kind: SymbolKind::DungeonSeason,
            id: 3,
            stable_key: "dungeon-season.3".into(),
            localization_key: None,
            icon: None,
            attributes: serde_json::from_value(serde_json::json!({
                "season_id": 3,
                "activity_families": [{
                    "family": "master",
                    "tier_identity": {
                        "minimum": 1,
                        "maximum": 20,
                        "count": 20
                    },
                    "activities": [{
                        "dungeon_id": dungeon_id,
                        "dungeon_key": dungeon_key,
                        "first_tier_row_id": dungeon_id * 100 + 1,
                        "last_tier_row_id": dungeon_id * 100 + 20
                    }]
                }]
            }))
            .unwrap(),
            availability: vec![build()],
            provenance: provenance(),
        }
    }

    fn record(
        kind: SymbolKind,
        id: i64,
        stable_key: &str,
        attributes: serde_json::Value,
    ) -> GameDataRecord {
        GameDataRecord {
            schema_version: GAME_DATA_SCHEMA_VERSION,
            kind,
            id,
            stable_key: stable_key.into(),
            localization_key: None,
            icon: None,
            attributes: serde_json::from_value(attributes).unwrap(),
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
        assert_eq!(SymbolKind::DungeonSeason.folder(), "dungeon-seasons");
        assert_eq!(SymbolKind::DungeonObjective.folder(), "dungeon-objectives");
        assert_eq!(SymbolKind::OverworldScene.folder(), "overworld-scenes");
        assert_eq!(SymbolKind::WorldArea.folder(), "world-areas");
        assert_eq!(SymbolKind::WorldObject.folder(), "world-objects");
        assert_eq!(SymbolKind::WorldEvent.folder(), "world-events");
        assert_eq!(SymbolKind::MapSticker.folder(), "map-stickers");
        assert_eq!(SymbolKind::SubScene.folder(), "subscenes");
        assert_eq!(SymbolKind::ActivityTarget.folder(), "activity-targets");
        assert_eq!(SymbolKind::SceneEvent.folder(), "scene-events");
        assert_eq!(SymbolKind::WorldActivity.folder(), "world-activities");
        assert_eq!(SymbolKind::WeaponEquipment.folder(), "weapon-equipment");
        assert_eq!(SymbolKind::ProfileImage.folder(), "profile-images");
        assert_eq!(SymbolKind::NameCard.folder(), "name-cards");
        assert_eq!(SymbolKind::Medal.folder(), "medals");
        assert_eq!(SymbolKind::GuildIcon.folder(), "guild-icons");
        assert_eq!(SymbolKind::Module.folder(), "modules");
        assert_eq!(SymbolKind::ModuleEffect.folder(), "module-effects");
    }

    #[test]
    fn dungeon_seasons_reference_canonical_dungeons_and_complete_tiers() {
        assert!(
            validate_source_data(
                &manifest(),
                &[dungeon(1633), dungeon_season(1633, "dungeon.1633")],
                &[],
                &[],
            )
            .is_ok()
        );

        let result = validate_source_data(
            &manifest(),
            &[dungeon(1633), dungeon_season(1633, "dungeon.wrong-key")],
            &[],
            &[],
        );
        assert!(matches!(
            result,
            Err(GameDataError::InvalidRecordSchema { .. })
        ));
    }

    #[test]
    fn overworld_behavior_graph_requires_canonical_resolved_references() {
        let scene = record(SymbolKind::Scene, 7, "scene.7", serde_json::json!({}));
        let subscene = record(
            SymbolKind::SubScene,
            6,
            "subscene.6",
            serde_json::json!({
                "subscene_id": 6,
                "subscene_type_id": 1,
                "resource_path": "scenes/fld001_story_raidentrance",
                "owner_scene_ids": [7],
                "owner_scene_keys": ["scene.7"],
                "resource_scene_ids": [7],
                "resource_scene_keys": ["scene.7"],
                "activation_conditions": [[9, 40110]]
            }),
        );
        let target = record(
            SymbolKind::ActivityTarget,
            10,
            "activity-target.10",
            serde_json::json!({
                "activity_target_id": 10,
                "target_type_id": 1,
                "required_count": 1,
                "scene_id": 7,
                "scene_key": "scene.7",
                "scene_resolution": "current_scene",
                "parameters": [100],
                "target_positions": [[1, 2]],
                "team_shared": true,
                "description_id": 0,
                "show_progress": true,
                "special_variables": [["counter"]],
                "special_variable_limits": [["10"]],
                "special_variable_names": ["counter"],
                "show_special_variable_progress": [1],
                "show_change": ["1"],
                "show_color": ["#ffffff"],
                "scene_event_ids": [20],
                "scene_event_keys": ["scene-event.20"]
            }),
        );
        let event = record(
            SymbolKind::SceneEvent,
            20,
            "scene-event.20",
            serde_json::json!({
                "scene_event_id": 20,
                "name_id": 0,
                "targets": [{
                    "target_id": 10,
                    "target_key": "activity-target.10",
                    "target_resolution": "current_activity_target"
                }],
                "scene_ids": [7],
                "scene_keys": ["scene.7"],
                "difficulty_flag": 1,
                "completion_actions": [[2, 3]],
                "time_limit_seconds": 90
            }),
        );
        let activity = record(
            SymbolKind::WorldActivity,
            1002,
            "world-activity.1002",
            serde_json::json!({
                "world_activity_id": 1002,
                "review_state": "identity_only",
                "withheld_field_reason": "fields need independent review"
            }),
        );
        let valid = vec![scene.clone(), subscene, target, event.clone(), activity];
        assert!(validate_overworld_catalog(&valid).is_ok());

        let mut invalid_event = event;
        let first_target = invalid_event
            .attributes
            .get_mut("targets")
            .and_then(serde_json::Value::as_array_mut)
            .and_then(|targets| targets.first_mut())
            .and_then(serde_json::Value::as_object_mut)
            .unwrap();
        first_target.insert("target_key".into(), serde_json::Value::Null);
        let invalid = vec![scene, invalid_event];
        assert!(matches!(
            validate_overworld_catalog(&invalid),
            Err(GameDataError::InvalidRecordSchema { .. })
        ));
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
