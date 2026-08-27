use std::path::{Path, PathBuf};

use rlogs_events::{ActorKind, ActorLoadoutSlot, CanonicalEvent, EventEnvelope, TimelineEventKind};
use rlogs_game_bpsr::{
    BPSR_GAME_PLUGIN_ID, CharacterProfilePatch, character_id_from_entity_uuid,
    is_localized_class_name, normalize_auxiliary_imagine_tier, project_actor_loadouts,
};
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u16 = 1;
const MAX_CATALOG_BYTES: u64 = 256 * 1024;
const MAX_IDENTITIES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CharacterPresentationIdentity {
    pub game_plugin_id: String,
    pub deployment_id: String,
    pub region_id: String,
    pub world_id: Option<String>,
    pub character_id: String,
    pub display_name: String,
    #[serde(default)]
    pub class_id: Option<i32>,
    #[serde(default)]
    pub specialization_id: Option<i32>,
    #[serde(default)]
    pub level: Option<u32>,
    #[serde(default)]
    pub ability_score: Option<i64>,
    #[serde(default)]
    pub weapon_item_id: Option<i64>,
    #[serde(default)]
    pub weapon_breakthrough_count: Option<u32>,
    #[serde(default)]
    pub seasonal_strength: Option<i64>,
    #[serde(default)]
    pub primary_loadout: Vec<ActorLoadoutSlot>,
    #[serde(default)]
    pub auxiliary_loadout: Vec<ActorLoadoutSlot>,
}

pub trait CharacterIdentityResolver {
    fn resolve_identity(
        &self,
        deployment_id: &str,
        region_id: &str,
        world_id: Option<&str>,
        character_id: &str,
    ) -> Option<&CharacterPresentationIdentity>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CharacterIdentityCatalog {
    schema_version: u16,
    entries: Vec<CharacterPresentationIdentity>,
}

impl Default for CharacterIdentityCatalog {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct CharacterIdentityStore {
    path: PathBuf,
    catalog: CharacterIdentityCatalog,
}

/// Character identity evidence observed during the currently captured run.
///
/// This store is deliberately memory-only. Combat History must never fill a
/// past run from the latest profile catalog because that can retroactively
/// replace a captured class, specialization, or loadout. The persistent store
/// remains useful for profile sync and the next process start, while this
/// ledger is the only resolver used when a run is frozen.
#[derive(Debug, Clone, Default)]
pub struct CaptureTimeCharacterIdentityStore {
    catalog: CharacterIdentityCatalog,
}

impl CharacterIdentityStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let catalog = load(&path)?;
        Ok(Self { path, catalog })
    }

    pub fn observe(&mut self, event: &EventEnvelope) -> Result<bool, String> {
        let Some(identity) = identity_from_event(event, Some(self), None, false)? else {
            return Ok(false);
        };
        self.upsert(identity)
    }

    pub fn resolve_identity(
        &self,
        deployment_id: &str,
        region_id: &str,
        world_id: Option<&str>,
        character_id: &str,
    ) -> Option<&CharacterPresentationIdentity> {
        resolve_identity(
            &self.catalog,
            deployment_id,
            region_id,
            world_id,
            character_id,
        )
    }

    fn upsert(&mut self, identity: CharacterPresentationIdentity) -> Result<bool, String> {
        if !upsert_catalog(&mut self.catalog, identity)? {
            return Ok(false);
        }
        write(&self.path, &self.catalog).map(|()| true)
    }
}

impl CharacterIdentityResolver for CharacterIdentityStore {
    fn resolve_identity(
        &self,
        deployment_id: &str,
        region_id: &str,
        world_id: Option<&str>,
        character_id: &str,
    ) -> Option<&CharacterPresentationIdentity> {
        self.resolve_identity(deployment_id, region_id, world_id, character_id)
    }
}

impl CaptureTimeCharacterIdentityStore {
    #[cfg(test)]
    pub fn observe(&mut self, event: &EventEnvelope) -> Result<bool, String> {
        self.observe_with_optional_fallback(event, None)
    }

    /// Retain the current packet's character state while allowing a prior,
    /// UID-matched catalog to supply only a missing public display name.
    ///
    /// Mutable character state is always projected from `event`; the fallback
    /// is deliberately never copied wholesale into the capture-time ledger.
    pub fn observe_with_name_fallback(
        &mut self,
        event: &EventEnvelope,
        fallback: &impl CharacterIdentityResolver,
    ) -> Result<bool, String> {
        self.observe_with_optional_fallback(event, Some(fallback))
    }

    fn observe_with_optional_fallback(
        &mut self,
        event: &EventEnvelope,
        fallback: Option<&dyn CharacterIdentityResolver>,
    ) -> Result<bool, String> {
        let Some(identity) = identity_from_event(event, Some(self), fallback, true)? else {
            return Ok(false);
        };
        upsert_catalog(&mut self.catalog, identity)
    }

    pub fn clear(&mut self) {
        self.catalog.entries.clear();
    }

    pub fn resolve_identity(
        &self,
        deployment_id: &str,
        region_id: &str,
        world_id: Option<&str>,
        character_id: &str,
    ) -> Option<&CharacterPresentationIdentity> {
        resolve_identity(
            &self.catalog,
            deployment_id,
            region_id,
            world_id,
            character_id,
        )
    }
}

impl CharacterIdentityResolver for CaptureTimeCharacterIdentityStore {
    fn resolve_identity(
        &self,
        deployment_id: &str,
        region_id: &str,
        world_id: Option<&str>,
        character_id: &str,
    ) -> Option<&CharacterPresentationIdentity> {
        self.resolve_identity(deployment_id, region_id, world_id, character_id)
    }
}

/// Build one presentation snapshot from the newest packet-time evidence.
///
/// Actor events are the low-latency source used by the live meter. Profile
/// events remain the richer source used by profile sync. Both feed this same
/// ledger so a cached loadout can never outrank a newer actor packet.
fn identity_from_event(
    event: &EventEnvelope,
    current: Option<&dyn CharacterIdentityResolver>,
    fallback: Option<&dyn CharacterIdentityResolver>,
    captured: bool,
) -> Result<Option<CharacterPresentationIdentity>, String> {
    match &event.event {
        CanonicalEvent::CharacterProfileObserved { profile } => {
            if profile.game_plugin_id != BPSR_GAME_PLUGIN_ID {
                return Ok(None);
            }
            let patch = CharacterProfilePatch::from_game_event(profile).map_err(|error| {
                let context = if captured { "captured " } else { "" };
                format!("could not read {context}BPSR character identity: {error}")
            })?;
            let region = &profile.character.region;
            let character_id = profile.character.character_id.clone();
            let display_name = resolved_display_name(
                patch.display_name.as_deref(),
                patch.class_id,
                region.deployment_id.as_str(),
                region.region_id.as_str(),
                region.world_id.as_deref(),
                character_id.as_str(),
                current,
                fallback,
            );
            let (primary_loadout, auxiliary_loadout) = project_actor_loadouts(&patch);
            Ok(Some(CharacterPresentationIdentity {
                game_plugin_id: profile.game_plugin_id.clone(),
                deployment_id: region.deployment_id.clone(),
                region_id: region.region_id.clone(),
                world_id: region.world_id.clone(),
                character_id,
                display_name,
                class_id: patch.class_id,
                specialization_id: patch.specialization_id,
                level: patch.level,
                ability_score: patch.combat_power,
                weapon_item_id: patch
                    .equipment
                    .as_ref()
                    .and_then(|items| items.iter().find(|item| item.slot_id == 200))
                    .map(|item| item.item_id),
                weapon_breakthrough_count: profile_weapon_breakthrough_count(&patch),
                seasonal_strength: patch
                    .season_strength
                    .or_else(|| patch.season.as_ref().and_then(|season| season.strength)),
                primary_loadout,
                auxiliary_loadout,
            }))
        }
        CanonicalEvent::Timeline(timeline) => {
            let TimelineEventKind::Actor(actor) = &timeline.kind else {
                return Ok(None);
            };
            if !matches!(actor.kind, ActorKind::Player) {
                return Ok(None);
            }
            let Some(character_id) = character_id_from_entity_uuid(actor.actor.entity_uuid.0)
            else {
                return Ok(None);
            };
            let region = &event.region.identity;
            let display_name = resolved_display_name(
                actor.display_name.as_deref(),
                actor.class_id,
                region.deployment_id.as_str(),
                region.region_id.as_str(),
                region.world_id.as_deref(),
                character_id.as_str(),
                current,
                fallback,
            );
            Ok(Some(CharacterPresentationIdentity {
                game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
                deployment_id: region.deployment_id.clone(),
                region_id: region.region_id.clone(),
                world_id: region.world_id.clone(),
                character_id,
                display_name,
                class_id: actor.class_id,
                specialization_id: actor.specialization_id,
                level: actor.level,
                ability_score: actor.ability_score,
                weapon_item_id: actor.weapon_item_id,
                weapon_breakthrough_count: actor.weapon_breakthrough_count,
                seasonal_strength: actor.seasonal_score,
                primary_loadout: actor.primary_loadout.clone(),
                auxiliary_loadout: actor.auxiliary_loadout.clone(),
            }))
        }
        _ => Ok(None),
    }
}

fn resolved_display_name(
    observed: Option<&str>,
    class_id: Option<i32>,
    deployment_id: &str,
    region_id: &str,
    world_id: Option<&str>,
    character_id: &str,
    current: Option<&dyn CharacterIdentityResolver>,
    fallback: Option<&dyn CharacterIdentityResolver>,
) -> String {
    observed
        .map(str::trim)
        .filter(|name| !actor_display_name_is_placeholder(name, class_id))
        .map(str::to_owned)
        .or_else(|| {
            current
                .and_then(|resolver| {
                    resolver.resolve_identity(deployment_id, region_id, world_id, character_id)
                })
                .map(|identity| identity.display_name.clone())
        })
        .or_else(|| {
            fallback
                .and_then(|resolver| {
                    resolver.resolve_identity(deployment_id, region_id, world_id, character_id)
                })
                .map(|identity| identity.display_name.clone())
        })
        // A public character UID is more useful and more stable than "Player 6".
        .unwrap_or_else(|| character_id.to_owned())
}

fn actor_display_name_is_placeholder(name: &str, class_id: Option<i32>) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return true;
    }
    let folded = name.to_ascii_lowercase();
    let numbered_placeholder = ["player", "actor", "uid", "unknown"].iter().any(|prefix| {
        folded == *prefix
            || folded
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.trim().chars().all(|value| value.is_ascii_digit()))
    });
    if numbered_placeholder {
        return true;
    }
    class_id.is_some_and(|class_id| is_localized_class_name(class_id, name).unwrap_or(false))
}

fn resolve_identity<'a>(
    catalog: &'a CharacterIdentityCatalog,
    deployment_id: &str,
    region_id: &str,
    world_id: Option<&str>,
    character_id: &str,
) -> Option<&'a CharacterPresentationIdentity> {
    catalog
        .entries
        .iter()
        .filter(|entry| {
            entry.game_plugin_id == BPSR_GAME_PLUGIN_ID
                && entry.deployment_id == deployment_id
                && entry.character_id == character_id
        })
        .max_by_key(|entry| {
            let region = usize::from(entry.region_id == region_id);
            let world = usize::from(entry.world_id.as_deref() == world_id);
            (region, world)
        })
}

fn upsert_catalog(
    catalog: &mut CharacterIdentityCatalog,
    identity: CharacterPresentationIdentity,
) -> Result<bool, String> {
    if let Some(existing) = catalog.entries.iter_mut().find(|entry| {
        entry.game_plugin_id == identity.game_plugin_id
            && entry.deployment_id == identity.deployment_id
            && entry.region_id == identity.region_id
            && entry.world_id == identity.world_id
            && entry.character_id == identity.character_id
    }) {
        let mut changed = false;
        if existing.display_name != identity.display_name {
            existing.display_name = identity.display_name;
            changed = true;
        }
        if let Some(class_id) = identity.class_id
            && existing.class_id != Some(class_id)
        {
            existing.class_id = Some(class_id);
            changed = true;
        }
        if let Some(specialization_id) = identity.specialization_id
            && existing.specialization_id != Some(specialization_id)
        {
            existing.specialization_id = Some(specialization_id);
            changed = true;
        }
        if let Some(level) = identity.level
            && existing.level != Some(level)
        {
            existing.level = Some(level);
            changed = true;
        }
        if let Some(ability_score) = identity.ability_score
            && existing.ability_score != Some(ability_score)
        {
            existing.ability_score = Some(ability_score);
            changed = true;
        }
        if let Some(weapon_item_id) = identity.weapon_item_id
            && existing.weapon_item_id != Some(weapon_item_id)
        {
            existing.weapon_item_id = Some(weapon_item_id);
            changed = true;
        }
        if let Some(weapon_breakthrough_count) = identity.weapon_breakthrough_count
            && existing.weapon_breakthrough_count != Some(weapon_breakthrough_count)
        {
            existing.weapon_breakthrough_count = Some(weapon_breakthrough_count);
            changed = true;
        }
        if let Some(seasonal_strength) = identity.seasonal_strength
            && existing.seasonal_strength != Some(seasonal_strength)
        {
            existing.seasonal_strength = Some(seasonal_strength);
            changed = true;
        }
        if !identity.primary_loadout.is_empty()
            && existing.primary_loadout != identity.primary_loadout
        {
            existing.primary_loadout = identity.primary_loadout;
            changed = true;
        }
        if !identity.auxiliary_loadout.is_empty()
            && existing.auxiliary_loadout != identity.auxiliary_loadout
        {
            existing.auxiliary_loadout = identity.auxiliary_loadout;
            changed = true;
        }
        if !changed {
            return Ok(false);
        }
    } else {
        if catalog.entries.len() >= MAX_IDENTITIES {
            return Err(format!(
                "character identity catalog reached its {MAX_IDENTITIES}-entry limit"
            ));
        }
        catalog.entries.push(identity);
    }
    catalog.entries.sort_by(|left, right| {
        left.deployment_id
            .cmp(&right.deployment_id)
            .then_with(|| left.region_id.cmp(&right.region_id))
            .then_with(|| left.world_id.cmp(&right.world_id))
            .then_with(|| left.character_id.cmp(&right.character_id))
    });
    Ok(true)
}

fn profile_weapon_breakthrough_count(profile: &CharacterProfilePatch) -> Option<u32> {
    profile
        .equipment
        .as_ref()?
        .iter()
        .find(|item| item.slot_id == 200)?
        .attributes
        .as_ref()?
        .breakthrough_count
        .and_then(|count| u32::try_from(count).ok())
}

fn load(path: &Path) -> Result<CharacterIdentityCatalog, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CharacterIdentityCatalog::default());
        }
        Err(error) => return Err(format!("could not inspect character identities: {error}")),
    };
    if metadata.len() > MAX_CATALOG_BYTES {
        return Err("character identity catalog exceeds the 256 KiB safety limit".into());
    }
    let mut catalog: CharacterIdentityCatalog = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| format!("could not read character identities: {error}"))?,
    )
    .map_err(|error| format!("character identity catalog is invalid: {error}"))?;
    if catalog.schema_version != SCHEMA_VERSION || catalog.entries.len() > MAX_IDENTITIES {
        return Err("character identity catalog has an unsupported shape".into());
    }
    normalize_auxiliary_loadouts(&mut catalog);
    Ok(catalog)
}

fn normalize_auxiliary_loadouts(catalog: &mut CharacterIdentityCatalog) {
    for identity in &mut catalog.entries {
        for slot in &mut identity.auxiliary_loadout {
            slot.tier = if slot.item_id.is_some() {
                normalize_auxiliary_imagine_tier(slot.tier)
            } else {
                None
            };
        }
    }
}

fn write(path: &Path, catalog: &CharacterIdentityCatalog) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "character identity path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create character identity folder: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(catalog)
        .map_err(|error| format!("could not encode character identities: {error}"))?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_CATALOG_BYTES {
        return Err("character identity catalog exceeds the 256 KiB safety limit".into());
    }
    std::fs::write(path, bytes)
        .map_err(|error| format!("could not write character identities: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured_profile_event() -> EventEnvelope {
        serde_json::from_value(serde_json::json!({
            "schema_version": 3,
            "session_id": "captured-run",
            "sequence": 1,
            "region": {
                "identity": {
                    "deployment_id": "global",
                    "region_id": "global",
                    "realm_id": null,
                    "world_id": "asteria"
                },
                "client_build": "24252055",
                "protocol_pack_digest": "sha256:fixture",
                "evidence": []
            },
            "time": {
                "observed_micros": 1,
                "game_time_millis": null
            },
            "provenance": {
                "confidence": "exact",
                "source": {
                    "type": "wire",
                    "capture_sequence": 1,
                    "connection_id": 1,
                    "stream_id": 1
                }
            },
            "sensitivity": "public_gameplay",
            "event": {
                "type": "character_profile_observed",
                "data": {
                    "profile": {
                        "game_plugin_id": BPSR_GAME_PLUGIN_ID,
                        "payload_schema_id": "app.rlogs.bpsr.character-profile",
                        "payload_schema_version": 1,
                        "character": {
                            "region": {
                                "deployment_id": "global",
                                "region_id": "global",
                                "realm_id": null,
                                "world_id": "asteria"
                            },
                            "character_id": "3296036"
                        },
                        "payload": {
                            "character": {
                                "region": {
                                    "deployment_id": "global",
                                    "region_id": "global",
                                    "realm_id": null,
                                    "world_id": "asteria"
                                },
                                "character_id": "3296036"
                            },
                            "display_name": "MarieRose",
                            "class_id": 11,
                            "level": 60,
                            "combat_power": 61734,
                            "season_strength": 3585
                        }
                    }
                }
            }
        }))
        .unwrap()
    }

    fn captured_actor_event(display_name: Option<&str>) -> EventEnvelope {
        serde_json::from_value(serde_json::json!({
            "schema_version": 6,
            "session_id": "captured-run",
            "sequence": 2,
            "region": {
                "identity": {
                    "deployment_id": "global",
                    "region_id": "global",
                    "realm_id": null,
                    "world_id": "asteria"
                },
                "client_build": "24252055",
                "protocol_pack_digest": "sha256:fixture",
                "evidence": []
            },
            "time": {
                "observed_micros": 2,
                "game_time_millis": null
            },
            "provenance": {
                "confidence": "exact",
                "source": {
                    "type": "wire",
                    "capture_sequence": 2,
                    "connection_id": 1,
                    "stream_id": 1
                }
            },
            "sensitivity": "public_gameplay",
            "event": {
                "type": "timeline",
                "data": {
                    "sequence": 2,
                    "time": {
                        "observed_micros": 2,
                        "game_time_millis": null
                    },
                    "provenance": {
                        "confidence": "exact",
                        "source": {
                            "type": "wire",
                            "capture_sequence": 2,
                            "connection_id": 1,
                            "stream_id": 1
                        }
                    },
                    "kind": {
                        "event": "actor",
                        "data": {
                            "actor": {
                                "actor_id": 6,
                                "entity_uuid": 216009015936_i64
                            },
                            "state": "updated",
                            "entity_type_id": 10,
                            "kind": "player",
                            "monster_id": null,
                            "display_name": display_name,
                            "class_id": 11,
                            "specialization_id": 117,
                            "level": 60,
                            "ability_score": 61782,
                            "weapon_item_id": 2000631,
                            "weapon_breakthrough_count": 3,
                            "seasonal_score": 3585,
                            "primary_loadout": [
                                {
                                    "slot_id": 7,
                                    "ability_id": 3948,
                                    "item_id": 3000101,
                                    "tier": 5
                                },
                                {
                                    "slot_id": 8,
                                    "ability_id": 3982,
                                    "item_id": 3001001,
                                    "tier": 5
                                }
                            ],
                            "auxiliary_loadout": []
                        }
                    }
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn capture_time_identity_is_scoped_to_observed_run_evidence() {
        let mut identities = CaptureTimeCharacterIdentityStore::default();
        assert!(identities.observe(&captured_profile_event()).unwrap());

        let identity = identities
            .resolve_identity("global", "global", Some("asteria"), "3296036")
            .unwrap();
        assert_eq!(identity.display_name, "MarieRose");
        assert_eq!(identity.class_id, Some(11));
        assert_eq!(identity.level, Some(60));
        assert_eq!(identity.ability_score, Some(61_734));
        assert_eq!(identity.seasonal_strength, Some(3_585));

        identities.clear();
        assert!(
            identities
                .resolve_identity("global", "global", Some("asteria"), "3296036")
                .is_none()
        );
    }

    #[test]
    fn capture_time_identity_borrows_only_a_missing_name_from_uid_fallback() {
        let fallback = CharacterIdentityStore {
            path: PathBuf::new(),
            catalog: CharacterIdentityCatalog {
                schema_version: SCHEMA_VERSION,
                entries: vec![CharacterPresentationIdentity {
                    game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    world_id: Some("asteria".into()),
                    character_id: "3296036".into(),
                    display_name: "MarieRose".into(),
                    class_id: Some(13),
                    specialization_id: Some(119),
                    level: Some(60),
                    ability_score: Some(99_999),
                    weapon_item_id: Some(2_000_999),
                    weapon_breakthrough_count: Some(9),
                    seasonal_strength: Some(9_999),
                    primary_loadout: Vec::new(),
                    auxiliary_loadout: Vec::new(),
                }],
            },
        };
        let mut value = serde_json::to_value(captured_profile_event()).unwrap();
        let payload = value
            .pointer_mut("/event/data/profile/payload")
            .and_then(serde_json::Value::as_object_mut)
            .unwrap();
        payload.remove("display_name");
        payload.insert("class_id".into(), serde_json::json!(11));
        payload.insert("combat_power".into(), serde_json::json!(61_734));
        let event: EventEnvelope = serde_json::from_value(value).unwrap();

        let mut captured = CaptureTimeCharacterIdentityStore::default();
        assert!(
            captured
                .observe_with_name_fallback(&event, &fallback)
                .unwrap()
        );
        let identity = captured
            .resolve_identity("global", "global", Some("asteria"), "3296036")
            .unwrap();
        assert_eq!(identity.display_name, "MarieRose");
        assert_eq!(identity.class_id, Some(11));
        assert_eq!(identity.ability_score, Some(61_734));
        assert_ne!(identity.ability_score, Some(99_999));
        assert_ne!(identity.weapon_item_id, Some(2_000_999));
    }

    #[test]
    fn actor_packet_replaces_stale_imagine_snapshot_and_resolves_public_uid() {
        let fallback = CharacterIdentityStore {
            path: PathBuf::new(),
            catalog: CharacterIdentityCatalog {
                schema_version: SCHEMA_VERSION,
                entries: vec![CharacterPresentationIdentity {
                    game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
                    deployment_id: "global".into(),
                    region_id: "global".into(),
                    world_id: Some("asteria".into()),
                    character_id: "3296036".into(),
                    display_name: "MarieRose".into(),
                    class_id: Some(11),
                    specialization_id: Some(119),
                    level: Some(60),
                    ability_score: Some(61_734),
                    weapon_item_id: Some(2_000_631),
                    weapon_breakthrough_count: Some(2),
                    seasonal_strength: Some(3_505),
                    primary_loadout: vec![
                        ActorLoadoutSlot {
                            slot_id: 7,
                            ability_id: Some(3_948),
                            item_id: Some(3_000_101),
                            tier: Some(5),
                        },
                        ActorLoadoutSlot {
                            slot_id: 8,
                            ability_id: Some(3_969),
                            item_id: Some(3_000_121),
                            tier: Some(5),
                        },
                    ],
                    auxiliary_loadout: Vec::new(),
                }],
            },
        };
        let mut captured = CaptureTimeCharacterIdentityStore::default();

        assert!(
            captured
                .observe_with_name_fallback(&captured_actor_event(Some("Player 6")), &fallback)
                .unwrap()
        );

        let identity = captured
            .resolve_identity("global", "global", Some("asteria"), "3296036")
            .unwrap();
        assert_eq!(identity.character_id, "3296036");
        assert_eq!(identity.display_name, "MarieRose");
        assert_eq!(identity.specialization_id, Some(117));
        assert_eq!(identity.weapon_breakthrough_count, Some(3));
        assert_eq!(identity.primary_loadout[0].ability_id, Some(3_948));
        assert_eq!(identity.primary_loadout[1].ability_id, Some(3_982));
        assert_eq!(identity.primary_loadout[1].item_id, Some(3_001_001));
        assert_ne!(identity.primary_loadout[1].ability_id, Some(3_969));
    }

    #[test]
    fn actor_packet_uses_public_uid_instead_of_numbered_placeholder_without_cache() {
        let mut captured = CaptureTimeCharacterIdentityStore::default();
        assert!(captured.observe(&captured_actor_event(None)).unwrap());

        let identity = captured
            .resolve_identity("global", "global", Some("asteria"), "3296036")
            .unwrap();
        assert_eq!(identity.display_name, "3296036");
    }

    #[test]
    fn exact_region_identity_wins_before_deployment_fallback() {
        let store = CharacterIdentityStore {
            path: PathBuf::new(),
            catalog: CharacterIdentityCatalog {
                schema_version: 1,
                entries: vec![
                    CharacterPresentationIdentity {
                        game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
                        deployment_id: "global".into(),
                        region_id: "fallback".into(),
                        world_id: None,
                        character_id: "3296036".into(),
                        display_name: "Fallback".into(),
                        class_id: None,
                        specialization_id: None,
                        level: None,
                        ability_score: None,
                        weapon_item_id: None,
                        weapon_breakthrough_count: None,
                        seasonal_strength: None,
                        primary_loadout: Vec::new(),
                        auxiliary_loadout: Vec::new(),
                    },
                    CharacterPresentationIdentity {
                        game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
                        deployment_id: "global".into(),
                        region_id: "global".into(),
                        world_id: None,
                        character_id: "3296036".into(),
                        display_name: "MarieRose".into(),
                        class_id: Some(11),
                        specialization_id: Some(116),
                        level: Some(60),
                        ability_score: Some(61_382),
                        weapon_item_id: Some(2_000_631),
                        weapon_breakthrough_count: Some(3),
                        seasonal_strength: Some(3_505),
                        primary_loadout: vec![ActorLoadoutSlot {
                            slot_id: 7,
                            ability_id: Some(3_948),
                            item_id: Some(3_000_101),
                            tier: Some(5),
                        }],
                        auxiliary_loadout: Vec::new(),
                    },
                ],
            },
        };
        let identity = store
            .resolve_identity("global", "global", None, "3296036")
            .unwrap();
        assert_eq!(identity.display_name, "MarieRose");
        assert_eq!(identity.class_id, Some(11));
        assert_eq!(identity.specialization_id, Some(116));
        assert_eq!(identity.level, Some(60));
        assert_eq!(identity.ability_score, Some(61_382));
        assert_eq!(identity.weapon_item_id, Some(2_000_631));
        assert_eq!(identity.seasonal_strength, Some(3_505));
        assert_eq!(identity.primary_loadout[0].item_id, Some(3_000_101));
    }

    #[test]
    fn loaded_identity_catalog_rejects_auxiliary_t0_and_t5_without_touching_primary_t5() {
        let mut catalog = CharacterIdentityCatalog {
            schema_version: SCHEMA_VERSION,
            entries: vec![CharacterPresentationIdentity {
                game_plugin_id: BPSR_GAME_PLUGIN_ID.into(),
                deployment_id: "global".into(),
                region_id: "global".into(),
                world_id: None,
                character_id: "3296036".into(),
                display_name: "MarieRose".into(),
                class_id: None,
                specialization_id: None,
                level: None,
                ability_score: None,
                weapon_item_id: None,
                weapon_breakthrough_count: None,
                seasonal_strength: None,
                primary_loadout: vec![ActorLoadoutSlot {
                    slot_id: 7,
                    ability_id: Some(3_948),
                    item_id: Some(3_000_101),
                    tier: Some(5),
                }],
                auxiliary_loadout: vec![
                    ActorLoadoutSlot {
                        slot_id: 21,
                        ability_id: Some(3_021),
                        item_id: Some(3_000_009),
                        tier: Some(5),
                    },
                    ActorLoadoutSlot {
                        slot_id: 22,
                        ability_id: Some(3_022),
                        item_id: Some(3_000_025),
                        tier: Some(4),
                    },
                    ActorLoadoutSlot {
                        slot_id: 23,
                        ability_id: Some(3_611),
                        item_id: None,
                        tier: Some(1),
                    },
                ],
            }],
        };

        normalize_auxiliary_loadouts(&mut catalog);

        let identity = &catalog.entries[0];
        assert_eq!(identity.primary_loadout[0].tier, Some(5));
        assert_eq!(identity.auxiliary_loadout[0].tier, None);
        assert_eq!(identity.auxiliary_loadout[1].tier, Some(4));
        assert_eq!(identity.auxiliary_loadout[2].tier, None);
    }
}
